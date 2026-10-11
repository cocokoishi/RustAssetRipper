use super::*;
use crate::resources::animator_controller::{
    state_behaviour_slots, StateBehaviourKey, StateBehaviourRange,
};

/// Compiled (layer, state/path) ranges select ordered slots, not distinct assets.
/// In particular state lists preserve explicit null slots; machine lists omit them.
pub(super) struct BehaviourTable<'a> {
    ranges: Vec<(StateBehaviourKey, StateBehaviourRange)>,
    indices: Vec<u32>,
    pub(super) pointers: &'a [SerializedValue],
    has_description: bool,
}

impl<'a> BehaviourTable<'a> {
    pub(super) fn read(root: &'a SerializedValue) -> Result<Self, Box<dyn Error>> {
        let pointers = optional_field(root, "m_StateMachineBehaviours")?
            .map(|v| as_array(v, "AnimatorController behaviour pointers"))
            .transpose()?
            .unwrap_or(&[]);
        let mut table = Self {
            ranges: Vec::new(),
            indices: Vec::new(),
            pointers,
            has_description: false,
        };
        let Some(description) = optional_field(root, "m_StateMachineBehaviourVectorDescription")?
        else {
            return Ok(table);
        };
        table.has_description = true;
        let ranges = array_field(
            description,
            "m_StateMachineBehaviourRanges",
            "BehaviourVectorDescription",
        )?;
        table.ranges.try_reserve_exact(ranges.len()).map_err(|_| {
            invalid_project("could not allocate AnimatorController behaviour ranges")
        })?;
        for pair in ranges {
            let pair = unwrap_data_or_self(pair, "BehaviourRange pair")?;
            let key = required_field(pair, "first", "BehaviourRange pair")?;
            let range = required_field(pair, "second", "BehaviourRange pair")?;
            let key = StateBehaviourKey {
                layer_index: required_i32_field(key, "m_LayerIndex", "BehaviourRange key")?,
                state_id: required_u32_field(key, "m_StateID", "BehaviourRange key")?,
            };
            if table.ranges.iter().any(|(existing, _)| *existing == key) {
                return Err(invalid_project(format!(
                    "AnimatorController has duplicate behaviour range for layer {} state {}",
                    key.layer_index, key.state_id
                )));
            }
            table.ranges.push((
                key,
                StateBehaviourRange {
                    start_index: required_u32_field(range, "m_StartIndex", "BehaviourRange")?,
                    count: required_u32_field(range, "m_Count", "BehaviourRange")?,
                },
            ));
        }
        let indices = array_field(
            description,
            "m_StateMachineBehaviourIndices",
            "BehaviourVectorDescription",
        )?;
        table
            .indices
            .try_reserve_exact(indices.len())
            .map_err(|_| {
                invalid_project("could not allocate AnimatorController behaviour indices")
            })?;
        for index in indices {
            table
                .indices
                .push(as_u32(index, "BehaviourVectorDescription index")?);
        }
        Ok(table)
    }

    pub(super) fn slots(&self, layer: usize, path_id: u32) -> Result<Vec<usize>, Box<dyn Error>> {
        let layer = i32::try_from(layer)
            .map_err(|_| invalid_project("AnimatorController behaviour layer index overflows"))?;
        state_behaviour_slots(
            self.has_description,
            &self.ranges,
            &self.indices,
            self.pointers.len(),
            layer,
            path_id,
        )
        .map_err(|error| {
            invalid_project(format!(
                "AnimatorController layer {layer} state {path_id} behaviour lookup failed: {error}"
            ))
        })
    }
}

pub(super) fn bind_behaviours(
    root_document: &UnityYamlDocument,
    table: &BehaviourTable<'_>,
    layers: &[ControllerLayer],
    machines: &mut [StateMachineConstant],
) -> Result<(), Box<dyn Error>> {
    let body = controller_body(root_document)?;
    let pointers = match node_field(body, "m_StateMachineBehaviours")? {
        Some(UnityYamlNode {
            kind: UnityYamlNodeKind::Sequence { items, .. },
            ..
        }) => items.as_slice(),
        Some(_) => {
            return Err(invalid_project(
                "AnimatorController behaviour YAML field is not a sequence",
            ))
        }
        None if table.pointers.is_empty() => &[],
        None => {
            return Err(invalid_project(
                "AnimatorController behaviour pointer conversion is missing",
            ))
        }
    };
    if pointers.len() != table.pointers.len() {
        return Err(invalid_project(
            "AnimatorController behaviour pointer conversion changed the slot count",
        ));
    }
    for (machine_index, machine) in machines.iter_mut().enumerate() {
        let owner = layers
            .iter()
            .position(|layer| {
                layer.state_machine_index == machine_index && layer.synchronized_layer_index == 0
            })
            .ok_or_else(|| {
                invalid_project(
                    "AnimatorController behaviour lookup has no unsynchronized owning layer",
                )
            })?;
        for state in &mut machine.states {
            state.behaviours = selected_nodes(table, pointers, owner, state.state_path_id, false)?;
        }
        for node in &mut machine.machines {
            node.behaviours = selected_nodes(table, pointers, owner, node.full_path_id, true)?;
        }
    }
    Ok(())
}

fn selected_nodes(
    table: &BehaviourTable<'_>,
    pointers: &[UnityYamlNode],
    layer: usize,
    path_id: u32,
    omit_null: bool,
) -> Result<Vec<UnityYamlNode>, Box<dyn Error>> {
    let slots = table.slots(layer, path_id)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(slots.len())
        .map_err(|_| invalid_project("could not allocate AnimatorController bound behaviours"))?;
    for slot in slots {
        let SerializedValue::PPtr { path_id, .. } = table.pointers[slot] else {
            return Err(invalid_project(
                "AnimatorController behaviour slot is not a PPtr",
            ));
        };
        if omit_null && path_id == 0 {
            continue;
        }
        result.push(pointers[slot].clone());
    }
    Ok(result)
}
