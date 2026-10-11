use super::*;
use crate::export::unity_yaml::UnityYamlDocument;

type ObjectKey = (usize, usize);

pub(super) struct PreparedControllerCollections {
    pub(super) roots: HashMap<ObjectKey, ControllerCollectionPlan>,
    pub(super) member_keys: HashSet<ObjectKey>,
}

pub(super) struct ControllerCollectionPlan {
    pub(super) is_release: bool,
    pub(super) children: Vec<ControllerMemberPlan>,
}

pub(super) struct ControllerMemberPlan {
    pub(super) key: ObjectKey,
    pub(super) file_id: i64,
    /// External references use the first owner; subsequent collections emit local copies.
    pub(super) register_global: bool,
}

pub(super) struct RoutedControllerCollection<'a> {
    pub(super) is_release: bool,
    pub(super) children: Vec<RoutedControllerMember<'a>>,
}

pub(super) struct RoutedControllerMember<'a> {
    pub(super) object: &'a WorkspaceObject,
    pub(super) file_id: i64,
    pub(super) register_global: bool,
}

/// FetchEditorHierarchy owns state/machine/transition/BlendTree/behaviour assets;
/// clip motions, scripts, masks, and arbitrary references from behaviours stay external.
pub(super) fn prepare_controller_collections(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    graph: &MultiFileReferenceGraph<'_>,
    already_owned: &HashSet<ExportAssetObjectKey>,
    limits: ExportPointerMapLimits,
) -> Result<PreparedControllerCollections, Box<dyn Error>> {
    let mut prepared = PreparedControllerCollections {
        roots: HashMap::new(),
        member_keys: HashSet::new(),
    };
    let mut claimed = HashSet::new();
    let mut planned_assets = 0usize;
    for root in objects.objects().iter().filter(|o| {
        o.factory.class_id == 91
            && matches!(
                o.factory.kind,
                crate::processing::asset_factory::FactoryAssetKind::Generated(_)
            )
    }) {
        planned_assets = planned_assets
            .checked_add(1)
            .filter(|count| *count <= limits.max_assets)
            .ok_or_else(|| {
                invalid_project("AnimatorController collections exceed the export asset limit")
            })?;
        let file = workspace.files().get(root.file_index).ok_or_else(|| {
            invalid_project("AnimatorController collection has no workspace file")
        })?;
        let is_release = crate::serialized_file::object::serialized_file_object_read_flags(
            file.header.format_version,
            file.metadata.prefix.target_platform,
            &file.virtual_path,
            &file.file_name,
            file.metadata.prefix.body_endian,
        )
        .is_release();
        let mut traversal = ControllerTraversal {
            objects,
            graph,
            keys: Vec::new(),
            seen: HashSet::new(),
            pending: Vec::new(),
            max_members: limits.max_assets - planned_assets,
        };
        let payload = root.payload.as_ref().map_err(|error| {
            controller_error(root, format!("could not decode hierarchy: {error}"))
        })?;
        if is_release {
            for pointer in animator_controller::release_controller_behaviour_pointers(payload)? {
                traversal.enqueue(root, pointer, &[114])?;
            }
        } else {
            traversal.pending.push((root.file_index, root.object_index));
            traversal.seen.insert((root.file_index, root.object_index));
        }
        traversal.walk()?;
        planned_assets = planned_assets
            .checked_add(traversal.keys.len())
            .ok_or_else(|| {
                invalid_project("AnimatorController collection member count overflows")
            })?;
        let mut children = Vec::new();
        children
            .try_reserve_exact(traversal.keys.len())
            .map_err(|_| {
                invalid_project("could not allocate AnimatorController collection members")
            })?;
        let mut modifiers = HashMap::<i32, u32>::new();
        let mut used_ids = HashSet::new();
        used_ids.insert(crate::export::ids::get_main_export_id(91, 0)?);
        for key in traversal.keys {
            if key == (root.file_index, root.object_index) {
                continue;
            }
            let object = objects.get(key.0, key.1).ok_or_else(|| {
                controller_error(root, "hierarchy member is absent from ObjectWorkspace")
            })?;
            modifiers.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController member ID counters")
            })?;
            let modifier = modifiers.entry(object.factory.class_id).or_default();
            *modifier = modifier.checked_add(1).ok_or_else(|| {
                invalid_project("AnimatorController member ID modifier overflows")
            })?;
            let file_id =
                crate::export::ids::get_main_export_id(object.factory.class_id, *modifier)?;
            used_ids
                .try_reserve(1)
                .map_err(|_| invalid_project("could not allocate AnimatorController member IDs"))?;
            if !used_ids.insert(file_id) {
                return Err(controller_error(
                    root,
                    "hierarchy members have duplicate generated fileIDs",
                ));
            }
            claimed.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController member ownership")
            })?;
            let register_global =
                !already_owned.contains(&source_object_key(key.0, key.1)) && claimed.insert(key);
            children.push(ControllerMemberPlan {
                key,
                file_id,
                register_global,
            });
            prepared.member_keys.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController member lookup")
            })?;
            prepared.member_keys.insert(key);
        }
        prepared
            .roots
            .try_reserve(1)
            .map_err(|_| invalid_project("could not allocate AnimatorController collections"))?;
        prepared.roots.insert(
            (root.file_index, root.object_index),
            ControllerCollectionPlan {
                is_release,
                children,
            },
        );
    }
    Ok(prepared)
}

struct ControllerTraversal<'a, 'graph> {
    objects: &'a ObjectWorkspace,
    graph: &'a MultiFileReferenceGraph<'graph>,
    keys: Vec<ObjectKey>,
    seen: HashSet<ObjectKey>,
    pending: Vec<ObjectKey>,
    max_members: usize,
}

impl ControllerTraversal<'_, '_> {
    fn enqueue(
        &mut self,
        source: &WorkspaceObject,
        pointer: &SerializedValue,
        classes: &[i32],
    ) -> Result<(), Box<dyn Error>> {
        let SerializedValue::PPtr { file_id, path_id } = pointer else {
            return Err(controller_error(source, "hierarchy field is not a PPtr"));
        };
        if *path_id == 0 {
            return Ok(());
        }
        let edge = self
            .graph
            .references_from_object(source.file_index, source.object_index)
            .find(|edge| edge.file_id == *file_id && edge.path_id == *path_id)
            .ok_or_else(|| {
                controller_error(
                    source,
                    format!("hierarchy PPtr ({file_id}, {path_id}) has no typed graph edge"),
                )
            })?;
        let MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } = edge.resolution
        else {
            return Err(controller_error(
                source,
                format!(
                    "hierarchy PPtr ({file_id}, {path_id}) is unresolved: {:?}",
                    edge.resolution
                ),
            ));
        };
        let object = self
            .objects
            .get(target_file_index, target_object_index)
            .ok_or_else(|| {
                controller_error(
                    source,
                    "hierarchy reference resolves outside ObjectWorkspace",
                )
            })?;
        if !classes.contains(&object.factory.class_id) {
            return Err(controller_error(
                source,
                format!(
                    "hierarchy reference targets class {}, expected {classes:?}",
                    object.factory.class_id
                ),
            ));
        }
        // AnimationClips are external even when reached through state/tree motions.
        if object.factory.class_id == 74 {
            return Ok(());
        }
        let key = (target_file_index, target_object_index);
        if !self.seen.contains(&key) {
            if self.keys.len() >= self.max_members {
                return Err(controller_error(
                    source,
                    "controller members and local copies exceed the export asset limit",
                ));
            }
            self.seen.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController traversal index")
            })?;
            self.pending.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController traversal queue")
            })?;
            self.keys
                .try_reserve(1)
                .map_err(|_| invalid_project("could not allocate AnimatorController hierarchy"))?;
            self.seen.insert(key);
            self.keys.push(key);
            self.pending.push(key);
        }
        Ok(())
    }

    fn optional_pointer(
        &mut self,
        source: &WorkspaceObject,
        root: &SerializedValue,
        name: &str,
        classes: &[i32],
    ) -> Result<(), Box<dyn Error>> {
        if let Some(pointer) = field(root, name)? {
            self.enqueue(source, pointer, classes)?;
        }
        Ok(())
    }

    fn pointer_list(
        &mut self,
        source: &WorkspaceObject,
        root: &SerializedValue,
        name: &str,
        classes: &[i32],
    ) -> Result<(), Box<dyn Error>> {
        for pointer in array(root, name)? {
            self.enqueue(source, pointer, classes)?;
        }
        Ok(())
    }

    fn walk(&mut self) -> Result<(), Box<dyn Error>> {
        while let Some(key) = self.pending.pop() {
            let object = self
                .objects
                .get(key.0, key.1)
                .ok_or_else(|| invalid_project("AnimatorController traversal object is absent"))?;
            let payload = object.payload.as_ref().map_err(|error| {
                controller_error(
                    object,
                    format!("could not decode hierarchy member: {error}"),
                )
            })?;
            let root = &payload.root;
            match object.factory.class_id {
                91 => {
                    for layer in array(root, "m_AnimatorLayers")? {
                        for pair in array(layer, "m_Behaviours")? {
                            self.optional_pointer(object, pair, "m_State", &[1102])?;
                            self.pointer_list(object, pair, "m_StateMachineBehaviours", &[114])?;
                        }
                        for pair in array(layer, "m_Motions")? {
                            self.optional_pointer(object, pair, "m_State", &[1102])?;
                            self.optional_pointer(object, pair, "m_Motion", &[74, 206])?;
                        }
                        self.optional_pointer(object, layer, "m_StateMachine", &[1107])?;
                    }
                }
                1107 => {
                    self.pointer_list(object, root, "m_AnyStateTransitions", &[1101])?;
                    for child in array(root, "m_ChildStateMachines")? {
                        self.optional_pointer(object, child, "m_StateMachine", &[1107])?;
                    }
                    for child in array(root, "m_ChildStates")? {
                        self.optional_pointer(object, child, "m_State", &[1102])?;
                    }
                    self.pointer_list(object, root, "m_EntryTransitions", &[1109])?;
                    self.pointer_list(object, root, "m_StateMachineBehaviours", &[114])?;
                    for pair in array(root, "m_StateMachineTransitions")? {
                        self.pointer_list(object, pair, "second", &[1109])?;
                    }
                    self.pointer_list(object, root, "m_ChildStateMachine", &[1107])?;
                    self.pointer_list(object, root, "m_States", &[1102])?;
                    for dictionary in ["m_LocalTransitions", "m_OrderedTransitions"] {
                        for pair in array(root, dictionary)? {
                            self.optional_pointer(object, pair, "first", &[1102])?;
                            self.pointer_list(object, pair, "second", &[1101])?;
                        }
                    }
                }
                1102 => {
                    self.optional_pointer(object, root, "m_Motion", &[74, 206])?;
                    self.pointer_list(object, root, "m_Motions", &[74, 206])?;
                    self.pointer_list(object, root, "m_StateMachineBehaviours", &[114])?;
                    self.pointer_list(object, root, "m_Transitions", &[1101])?;
                }
                206 => {
                    for child in array(root, "m_Childs")? {
                        self.optional_pointer(object, child, "m_Motion", &[74, 206])?;
                    }
                }
                // Behaviours and transitions belong to the collection, but their arbitrary
                // references are dependencies, not recursively claimed editor hierarchy.
                114 | 1101 | 1109 => {}
                class_id => {
                    return Err(controller_error(
                        object,
                        format!("unsupported editor hierarchy member class {class_id}"),
                    ))
                }
            }
        }
        Ok(())
    }
}

fn field<'a>(
    value: &'a SerializedValue,
    name: &str,
) -> Result<Option<&'a SerializedValue>, Box<dyn Error>> {
    value.unique_field_in_base_chain(name).map_err(|error| {
        invalid_project(format!(
            "AnimatorController hierarchy field {name}: {error}"
        ))
    })
}

fn array<'a>(
    value: &'a SerializedValue,
    name: &str,
) -> Result<&'a [SerializedValue], Box<dyn Error>> {
    match field(value, name)? {
        Some(SerializedValue::Array(values)) => Ok(values),
        None => Ok(&[]),
        Some(_) => Err(invalid_project(format!(
            "AnimatorController hierarchy field {name} is not an array"
        ))),
    }
}

fn controller_error(object: &WorkspaceObject, reason: impl std::fmt::Display) -> Box<dyn Error> {
    invalid_project(format!(
        "AnimatorController hierarchy file {} object {} PathID {}: {reason}",
        object.file_index, object.object_index, object.path_id
    ))
}

pub(super) fn hide_controller_behaviour(
    document: &mut UnityYamlDocument,
) -> Result<(), Box<dyn Error>> {
    use crate::export::unity_yaml::{UnityYamlNode, UnityYamlNodeKind, UnityYamlScalar};
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut document.root.kind else {
        return Err(invalid_project(
            "AnimatorController behaviour document has no root mapping",
        ));
    };
    let Some((_, body)) = entries.first_mut() else {
        return Err(invalid_project(
            "AnimatorController behaviour document is empty",
        ));
    };
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut body.kind else {
        return Err(invalid_project(
            "AnimatorController behaviour body is not a mapping",
        ));
    };
    let flags = entries.iter_mut().find(|(name, _)| matches!(&name.kind, UnityYamlNodeKind::Scalar(UnityYamlScalar::String(name) | UnityYamlScalar::PlainString(name)) if name == "m_ObjectHideFlags"))
        .ok_or_else(|| invalid_project("AnimatorController behaviour document has no hide flags"))?;
    flags.1 = UnityYamlNode::scalar(UnityYamlScalar::Signed(1));
    Ok(())
}
