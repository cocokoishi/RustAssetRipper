use super::*;
use crate::resources::custom_curve::reverse_utf8_crc32;

pub(super) struct ResolvedMachine {
    pub(super) full_path_id: u32,
    pub(super) name: String,
    pub(super) parent_index: Option<usize>,
    pub(super) is_extra: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathStatus {
    Known,
    Unknown,
    /// An exit-port transition proves the parent, whose path is still unknown.
    FixedParent,
}

struct MachinePath {
    id: u32,
    parent: Option<usize>,
    status: PathStatus,
}

#[derive(Default)]
struct ParentRestrictions {
    no_direct: Vec<usize>,
    not_under: Vec<String>,
}

enum Destination {
    State(usize),
    Machine { index: usize, is_entry: bool },
}

/// Owns only path recovery, not generated assets, YAML, or export identities.
/// The ordering matches AnimatorStateMachineContext.Process: definitive exits,
/// restrictions, state/AnyState proposals, selector proposals, sharing, fallback.
struct HierarchyInference<'a> {
    nodes: Vec<MachinePath>,
    paths: &'a mut HashMap<u32, String>,
    selectors: &'a [SelectorState],
    states: &'a [StateConstant],
    restrictions: Vec<ParentRestrictions>,
    // Preserve the source dictionary's insertion order when sharing parents.
    relations: Vec<(usize, Vec<usize>)>,
}

pub(super) fn resolve_machine_paths(
    path_ids: &[u32],
    paths: &mut HashMap<u32, String>,
    states: &[StateConstant],
    selectors: &[SelectorState],
    raw: &SerializedValue,
    root_name: &str,
) -> Result<Vec<ResolvedMachine>, Box<dyn Error>> {
    if path_ids.is_empty() {
        return Err(invalid_project(
            "AnimatorController has no root state-machine selector path",
        ));
    }
    if states.is_empty() {
        let recovered_root_name = root_name.replace('.', "_");
        if let Some(existing) = paths.get(&path_ids[0]) {
            if existing != &recovered_root_name {
                return Err(invalid_project(
                    "AnimatorController state-free root path conflicts with its layer name",
                ));
            }
        } else {
            paths.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController root machine path")
            })?;
            paths.insert(path_ids[0], recovered_root_name);
        }
    }
    let mut inference = HierarchyInference {
        nodes: Vec::new(),
        paths,
        selectors,
        states,
        restrictions: Vec::new(),
        relations: Vec::new(),
    };
    inference
        .nodes
        .try_reserve_exact(path_ids.len())
        .map_err(|_| {
            invalid_project("could not allocate AnimatorController path recovery nodes")
        })?;
    inference
        .restrictions
        .try_reserve_exact(path_ids.len())
        .map_err(|_| {
            invalid_project("could not allocate AnimatorController parent restrictions")
        })?;
    for (index, &id) in path_ids.iter().enumerate() {
        let (status, parent) = match inference.paths.get(&id) {
            Some(path) if index == 0 => {
                if path.contains('.') {
                    return Err(invalid_project(
                        "AnimatorController root machine path is not a root",
                    ));
                }
                (PathStatus::Known, None)
            }
            Some(path) => {
                let (parent_path, _) = path.rsplit_once('.').ok_or_else(|| {
                    invalid_project("AnimatorController nested machine has no parent path")
                })?;
                let parent_id = crc32_ieee(parent_path.as_bytes());
                let parent = path_ids.iter().position(|&id| id == parent_id).ok_or_else(|| {
                    invalid_project(format!("AnimatorController machine {path:?} has no selector for parent {parent_path:?}"))
                })?;
                (PathStatus::Known, Some(parent))
            }
            None if index == 0 => {
                return Err(invalid_project(
                    "AnimatorController root selector path cannot be recovered from state paths",
                ));
            }
            None => (PathStatus::Unknown, None),
        };
        inference.nodes.push(MachinePath { id, status, parent });
        inference.restrictions.push(ParentRestrictions::default());
    }
    // Validate selector uniqueness even when no transition happens to use it.
    for (index, &id) in path_ids.iter().enumerate() {
        for is_entry in [true, false] {
            if selectors
                .iter()
                .filter(|s| s.full_path_id == id && s.is_entry == is_entry)
                .count()
                > 1
            {
                return Err(invalid_project(format!(
                    "AnimatorController machine {index} has duplicate {} selectors",
                    if is_entry { "entry" } else { "exit" }
                )));
            }
        }
    }
    if inference
        .nodes
        .iter()
        .any(|n| n.status != PathStatus::Known)
    {
        inference.locate_definitive_parents()?;
        inference.restrict_parents()?;
        // Source visits state-owning machines first, then their global state indices.
        for owner in 0..inference.nodes.len() {
            let owner_id = inference.nodes[owner].id;
            for state in states.iter().filter(|s| s.parent_path_id == owner_id) {
                for transition in
                    array_field(&state.raw, "m_TransitionConstantArray", "StateConstant")?
                {
                    let transition = unwrap_data(transition, "TransitionConstant")?;
                    inference.propose_from_state_transition(owner, transition)?;
                }
            }
        }
        for transition in array_field(
            raw,
            "m_AnyStateTransitionConstantArray",
            "StateMachineConstant",
        )? {
            inference
                .propose_from_state_transition(0, unwrap_data(transition, "TransitionConstant")?)?;
        }
        inference.locate_from_selectors(true)?;
        inference.locate_from_selectors(false)?;
        inference.share_parents()?;
        inference.assign_remaining_parents()?;
        inference.promote_paths()?;
    }
    let mut resolved = Vec::new();
    resolved
        .try_reserve_exact(inference.nodes.len())
        .map_err(|_| invalid_project("could not allocate AnimatorController resolved hierarchy"))?;
    for (index, node) in inference.nodes.iter().enumerate() {
        if node.status != PathStatus::Known {
            return Err(invalid_project(format!(
                "AnimatorController empty machine {} has an unresolved or cyclic parent chain",
                node.id
            )));
        }
        let path = inference.paths.get(&node.id).ok_or_else(|| {
            invalid_project("AnimatorController resolved machine is missing its path")
        })?;
        resolved.push(ResolvedMachine {
            full_path_id: node.id,
            name: if index == 0 {
                path.clone()
            } else {
                path.rsplit('.').next().unwrap_or(path).to_owned()
            },
            parent_index: node.parent,
            is_extra: index >= path_ids.len(),
        });
    }
    Ok(resolved)
}

/// Unity 4's controller constants have no selector graph: the source chooses the default
/// state's state-machine path as the root, then attaches every other encountered state path
/// directly beneath it.
pub(super) fn resolve_legacy_machine_paths(
    states: &[super::StateConstant],
    paths: &HashMap<u32, String>,
    layer_binding_path_id: u32,
    layer_name: &str,
    default_state_index: Option<usize>,
) -> Result<Vec<ResolvedMachine>, Box<dyn Error>> {
    if states.is_empty() {
        return Ok(vec![ResolvedMachine {
            full_path_id: layer_binding_path_id,
            name: layer_name.replace('.', "_"),
            parent_index: None,
            is_extra: false,
        }]);
    }

    let default_state = states
        .get(default_state_index.ok_or_else(|| {
            invalid_project("AnimatorController legacy state hierarchy has no default state")
        })?)
        .ok_or_else(|| {
            invalid_project("AnimatorController legacy default state is out of bounds")
        })?;
    let root_path_id = default_state.parent_path_id;
    let mut path_ids = Vec::new();
    path_ids
        .try_reserve_exact(states.len())
        .map_err(|_| invalid_project("could not allocate legacy AnimatorController paths"))?;
    path_ids.push(root_path_id);
    for state in states {
        if !path_ids.contains(&state.parent_path_id) {
            path_ids.push(state.parent_path_id);
        }
    }

    let mut resolved = Vec::new();
    resolved
        .try_reserve_exact(path_ids.len())
        .map_err(|_| invalid_project("could not allocate legacy AnimatorController hierarchy"))?;
    for (index, full_path_id) in path_ids.into_iter().enumerate() {
        let name = paths.get(&full_path_id).cloned().ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController legacy state-machine path {full_path_id} is missing its TOS name"
            ))
        })?;
        resolved.push(ResolvedMachine {
            full_path_id,
            name,
            parent_index: (index != 0).then_some(0),
            is_extra: false,
        });
    }
    Ok(resolved)
}

impl HierarchyInference<'_> {
    fn destination(&self, value: u32) -> Result<Option<Destination>, Box<dyn Error>> {
        if value == u32::MAX {
            return Ok(None);
        }
        if value < TRANSITION_DESTINATION_FLAG {
            if (value as usize) >= self.states.len() {
                return Err(invalid_project(format!(
                    "AnimatorController transition targets absent state index {value}"
                )));
            }
            return Ok(Some(Destination::State(value as usize)));
        }
        let selector_index = (value % TRANSITION_DESTINATION_FLAG) as usize;
        let selector = self.selectors.get(selector_index).ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController transition targets absent selector index {selector_index}"
            ))
        })?;
        let index = self
            .nodes
            .iter()
            .position(|n| n.id == selector.full_path_id)
            .ok_or_else(|| invalid_project("AnimatorController selector has no hierarchy node"))?;
        Ok(Some(Destination::Machine {
            index,
            is_entry: selector.is_entry,
        }))
    }

    fn selector_transitions(&self, index: usize, is_entry: bool) -> &[SerializedValue] {
        self.selectors
            .iter()
            .find(|s| s.full_path_id == self.nodes[index].id && s.is_entry == is_entry)
            .map_or(&[], |s| s.transitions.as_slice())
    }

    fn selector_destinations(
        &self,
        index: usize,
        is_entry: bool,
    ) -> Result<Vec<u32>, Box<dyn Error>> {
        let transitions = self.selector_transitions(index, is_entry);
        let mut destinations = Vec::new();
        destinations
            .try_reserve_exact(transitions.len())
            .map_err(|_| {
                invalid_project("could not allocate AnimatorController selector destinations")
            })?;
        for transition in transitions {
            destinations.push(required_u32_field(
                transition,
                "m_Destination",
                "SelectorTransitionConstant",
            )?);
        }
        Ok(destinations)
    }

    fn unknown_ancestor(&self, mut index: usize) -> Result<usize, Box<dyn Error>> {
        for _ in 0..self.nodes.len() {
            match self.nodes[index].status {
                PathStatus::Unknown => return Ok(index),
                PathStatus::Known => return Err(invalid_project("AnimatorController fixed empty machine chain reaches an already resolved parent")),
                PathStatus::FixedParent => {
                    index = self.nodes[index].parent.ok_or_else(|| invalid_project("AnimatorController fixed empty machine has no parent"))?;
                }
            }
        }
        Err(invalid_project(
            "AnimatorController empty machine parent chain contains a cycle",
        ))
    }

    fn locate_definitive_parents(&mut self) -> Result<(), Box<dyn Error>> {
        for index in 0..self.nodes.len() {
            if self.nodes[index].status != PathStatus::Unknown {
                continue;
            }
            let parent = self
                .selector_transitions(index, false)
                .iter()
                .find_map(|t| {
                    match required_u32_field(t, "m_Destination", "SelectorTransitionConstant")
                        .and_then(|value| self.destination(value))
                    {
                        Ok(Some(Destination::Machine {
                            index,
                            is_entry: false,
                        })) => Some(Ok(index)),
                        Err(error) => Some(Err(error)),
                        _ => None,
                    }
                })
                .transpose()?;
            if let Some(parent) = parent {
                self.nodes[index].parent = Some(parent);
                self.nodes[index].status = PathStatus::FixedParent;
            }
        }
        self.promote_paths()
    }

    fn promote_paths(&mut self) -> Result<(), Box<dyn Error>> {
        loop {
            let mut changed = false;
            for index in (1..self.nodes.len()).rev() {
                if self.nodes[index].status == PathStatus::Known {
                    continue;
                }
                let Some(parent) = self.nodes[index].parent else {
                    continue;
                };
                let Some(parent_path) = self.paths.get(&self.nodes[parent].id) else {
                    continue;
                };
                let path =
                    reverse_utf8_crc32(&format!("{parent_path}.EMPTY_"), self.nodes[index].id);
                if crc32_ieee(path.as_bytes()) != self.nodes[index].id {
                    return Err(invalid_project(
                        "AnimatorController reversed empty-machine path has an invalid CRC",
                    ));
                }
                self.paths.try_reserve(1).map_err(|_| {
                    invalid_project("could not allocate AnimatorController recovered path")
                })?;
                self.paths.insert(self.nodes[index].id, path);
                self.nodes[index].status = PathStatus::Known;
                changed = true;
            }
            if !changed {
                return Ok(());
            }
        }
    }

    fn restrict_parents(&mut self) -> Result<(), Box<dyn Error>> {
        for source in 0..self.nodes.len() {
            for destination in self.selector_destinations(source, false)? {
                let Some(Destination::Machine {
                    mut index,
                    is_entry: true,
                }) = self.destination(destination)?
                else {
                    continue;
                };
                if let Some(path) = self.paths.get(&self.nodes[source].id).cloned() {
                    if self.nodes[index].status == PathStatus::Known {
                        continue;
                    }
                    index = self.unknown_ancestor(index)?;
                    if self.restrictions[index]
                        .not_under
                        .iter()
                        .all(|bad| !path.starts_with(bad))
                    {
                        self.restrictions[index]
                            .not_under
                            .try_reserve(1)
                            .map_err(|_| {
                                invalid_project(
                                    "could not allocate AnimatorController path restrictions",
                                )
                            })?;
                        self.restrictions[index].not_under.push(path);
                    }
                } else if self.nodes[index].status == PathStatus::Known
                    && self.nodes[source].status == PathStatus::Unknown
                    && !self.restrictions[source].no_direct.contains(&index)
                {
                    self.restrictions[source]
                        .no_direct
                        .try_reserve(1)
                        .map_err(|_| {
                            invalid_project(
                                "could not allocate AnimatorController parent exclusions",
                            )
                        })?;
                    self.restrictions[source].no_direct.push(index);
                }
            }
        }
        // Redundant source restrictions can remain: they have identical acceptance
        // semantics, and retaining them avoids another allocation/simplification pass.
        Ok(())
    }

    fn try_parent(
        &mut self,
        index: usize,
        mut candidate: usize,
        recursive: bool,
    ) -> Result<bool, Box<dyn Error>> {
        for _ in 0..self.nodes.len() {
            let Some(candidate_path) = self.paths.get(&self.nodes[candidate].id) else {
                return Ok(false);
            };
            if let Some(current_path) = self.nodes[index]
                .parent
                .and_then(|p| self.paths.get(&self.nodes[p].id))
            {
                if candidate_path.bytes().filter(|&b| b == b'.').count()
                    <= current_path.bytes().filter(|&b| b == b'.').count()
                {
                    return Ok(false);
                }
            }
            let restrictions = &self.restrictions[index];
            if !restrictions.no_direct.contains(&candidate)
                && !restrictions
                    .not_under
                    .iter()
                    .any(|bad| candidate_path.starts_with(bad))
            {
                self.nodes[index].parent = Some(candidate);
                return Ok(true);
            }
            if !recursive {
                return Ok(false);
            }
            let Some(parent) = self.nodes[candidate].parent else {
                return Ok(false);
            };
            candidate = parent;
        }
        Err(invalid_project(
            "AnimatorController candidate parent hierarchy contains a cycle",
        ))
    }

    fn propose_from_state_transition(
        &mut self,
        owner: usize,
        transition: &SerializedValue,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(Destination::Machine {
            index,
            is_entry: true,
        }) = self.destination(required_u32_field(
            transition,
            "m_DestinationState",
            "TransitionConstant",
        )?)? {
            if self.nodes[index].status != PathStatus::Known {
                let unknown = self.unknown_ancestor(index)?;
                self.try_parent(unknown, owner, true)?;
            }
        }
        Ok(())
    }

    fn locate_from_selectors(&mut self, is_entry: bool) -> Result<(), Box<dyn Error>> {
        for original_source in 0..self.nodes.len() {
            let destinations = self.selector_destinations(original_source, is_entry)?;
            let source = if self.nodes[original_source].status == PathStatus::FixedParent {
                self.unknown_ancestor(original_source)?
            } else {
                original_source
            };
            for destination in destinations {
                match self.destination(destination)? {
                    Some(Destination::State(state))
                        if self.nodes[source].status == PathStatus::Unknown =>
                    {
                        let parent = self.nodes.iter().position(|n| n.id == self.states[state].parent_path_id).ok_or_else(|| invalid_project("AnimatorController selector targets a state without a parent machine"))?;
                        self.try_parent(source, parent, true)?;
                    }
                    Some(Destination::Machine {
                        mut index,
                        is_entry: destination_entry,
                    }) if is_entry || destination_entry => {
                        if self.nodes[index].status == PathStatus::FixedParent {
                            index = self.unknown_ancestor(index)?;
                        }
                        match (
                            self.nodes[source].status == PathStatus::Unknown,
                            self.nodes[index].status == PathStatus::Unknown,
                        ) {
                            (true, false) => {
                                let candidate = if is_entry {
                                    Some(index)
                                } else {
                                    self.nodes[index].parent
                                };
                                if let Some(candidate) = candidate {
                                    self.try_parent(source, candidate, true)?;
                                }
                            }
                            (false, true) => {
                                let candidate = if is_entry {
                                    Some(source)
                                } else {
                                    self.nodes[source].parent
                                };
                                if let Some(candidate) = candidate {
                                    self.try_parent(index, candidate, true)?;
                                }
                            }
                            (true, true) => {
                                self.link(source, index)?;
                                self.link(index, source)?;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn link(&mut self, source: usize, destination: usize) -> Result<(), Box<dyn Error>> {
        let relation = if let Some(index) = self
            .relations
            .iter()
            .position(|(index, _)| *index == source)
        {
            &mut self.relations[index].1
        } else {
            self.relations.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController related machines")
            })?;
            self.relations.push((source, Vec::new()));
            &mut self
                .relations
                .last_mut()
                .ok_or_else(|| invalid_project("AnimatorController relation was not inserted"))?
                .1
        };
        if !relation.contains(&destination) {
            relation.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController related machine links")
            })?;
            relation.push(destination);
        }
        Ok(())
    }

    fn share_parents(&mut self) -> Result<(), Box<dyn Error>> {
        // Every update increases a node's parent depth in the finite known-path set.
        loop {
            let mut changed = false;
            for relation in 0..self.relations.len() {
                let Some(parent) = self.nodes[self.relations[relation].0].parent else {
                    continue;
                };
                for link in 0..self.relations[relation].1.len() {
                    changed |= self.try_parent(self.relations[relation].1[link], parent, true)?;
                }
            }
            if !changed {
                return Ok(());
            }
        }
    }

    fn assign_remaining_parents(&mut self) -> Result<(), Box<dyn Error>> {
        let original_count = self.nodes.len();
        let mut needs_extra = false;
        for index in 1..original_count {
            if self.nodes[index].status != PathStatus::Unknown || self.nodes[index].parent.is_some()
            {
                continue;
            }
            for candidate in 0..original_count {
                if self.nodes[candidate].status == PathStatus::Known
                    && self.try_parent(index, candidate, false)?
                {
                    break;
                }
            }
            needs_extra |= self.nodes[index].parent.is_none();
        }
        if needs_extra {
            if self.paths.contains_key(&0) || self.nodes.iter().any(|node| node.id == 0) {
                return Err(invalid_project(
                    "AnimatorController extra machine sentinel collides with an existing path",
                ));
            }
            let root_path = self.paths.get(&self.nodes[0].id).ok_or_else(|| {
                invalid_project("AnimatorController extra machine has no root path")
            })?;
            let path = format!("{root_path}.Extra StateMachine");
            self.paths.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController extra machine path")
            })?;
            self.paths.insert(0, path);
            self.nodes.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController extra machine")
            })?;
            self.nodes.push(MachinePath {
                id: 0,
                parent: Some(0),
                status: PathStatus::Known,
            });
            for node in &mut self.nodes[1..original_count] {
                if node.status == PathStatus::Unknown && node.parent.is_none() {
                    node.parent = Some(original_count);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialized_file::object::SerializedFieldValue;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn selector_transition(destination: u32) -> SerializedValue {
        SerializedValue::Struct(vec![field(
            "m_Destination",
            SerializedValue::U32(destination),
        )])
    }

    fn legacy_state(state_path_id: u32, parent_path_id: u32) -> super::super::StateConstant {
        super::super::StateConstant {
            raw: SerializedValue::Struct(Vec::new()),
            name: String::new(),
            state_path_id,
            parent_path_id,
            file_id: i64::from(state_path_id),
            motion: super::super::local_pointer(0),
            transition_file_ids: Vec::new(),
            behaviours: Vec::new(),
        }
    }

    #[test]
    fn state_free_nested_selector_paths_follow_entry_transition_ownership() {
        let root_path = "Base_Layer";
        let nested_path = "Base_Layer.EMPTY_Child";
        let root_id = crate::resources::blend_shape::crc32_ieee(root_path.as_bytes());
        let nested_id = crate::resources::blend_shape::crc32_ieee(nested_path.as_bytes());
        // Selector destinations use the original selector-array indices. The final entry
        // destination is the default-state slot and remains null for a state-free machine.
        let selectors = vec![
            SelectorState {
                full_path_id: root_id,
                is_entry: true,
                transitions: vec![selector_transition(30_002), selector_transition(u32::MAX)],
            },
            SelectorState {
                full_path_id: root_id,
                is_entry: false,
                transitions: Vec::new(),
            },
            SelectorState {
                full_path_id: nested_id,
                is_entry: true,
                transitions: vec![selector_transition(u32::MAX)],
            },
            SelectorState {
                full_path_id: nested_id,
                is_entry: false,
                transitions: Vec::new(),
            },
        ];
        let raw = SerializedValue::Struct(vec![field(
            "m_AnyStateTransitionConstantArray",
            SerializedValue::Array(Vec::new()),
        )]);
        let mut paths = HashMap::new();

        let resolved = resolve_machine_paths(
            &[root_id, nested_id],
            &mut paths,
            &[],
            &selectors,
            &raw,
            "Base.Layer",
        )
        .unwrap();

        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].name, "Base_Layer");
        assert_eq!(resolved[0].parent_index, None);
        assert_eq!(resolved[1].parent_index, Some(0));
        let recovered_nested_path = paths.get(&nested_id).unwrap();
        assert_eq!(crc32_ieee(recovered_nested_path.as_bytes()), nested_id);
        assert_eq!(
            recovered_nested_path.rsplit('.').next(),
            Some(resolved[1].name.as_str())
        );
    }

    #[test]
    fn legacy_default_state_selects_root_and_remaining_paths_are_direct_children() {
        let root_path = "Legacy.Root";
        let child_path = "Other.Machine";
        let root_id = crate::resources::blend_shape::crc32_ieee(root_path.as_bytes());
        let child_id = crate::resources::blend_shape::crc32_ieee(child_path.as_bytes());
        let states = [
            legacy_state(10, root_id),
            legacy_state(11, child_id),
            legacy_state(12, root_id),
        ];
        let raw = SerializedValue::Struct(vec![field(
            "m_DefaultState",
            SerializedValue::U32(u32::MAX),
        )]);
        let default_state = super::super::legacy_default_state_index(&raw, states.len()).unwrap();
        let paths = HashMap::from([
            (root_id, root_path.to_owned()),
            (child_id, child_path.to_owned()),
        ]);

        let resolved =
            resolve_legacy_machine_paths(&states, &paths, 999, "ignored.layer", default_state)
                .unwrap();

        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].full_path_id, root_id);
        assert_eq!(resolved[0].name, root_path);
        assert_eq!(resolved[0].parent_index, None);
        assert_eq!(resolved[1].full_path_id, child_id);
        assert_eq!(resolved[1].name, child_path);
        assert_eq!(resolved[1].parent_index, Some(0));

        assert_eq!(
            super::super::legacy_default_state_index(
                &SerializedValue::Struct(vec![field("m_DefaultState", SerializedValue::U32(1))]),
                states.len()
            )
            .unwrap(),
            Some(1)
        );
    }
}
