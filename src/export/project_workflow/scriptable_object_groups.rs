use super::*;

type SourceObjectKey = (usize, usize);

pub(super) struct PreparedScriptableObjectGroups {
    pub(super) roots: HashMap<SourceObjectKey, ScriptableObjectGroupPlan>,
    pub(super) member_keys: HashSet<SourceObjectKey>,
}

pub(super) struct ScriptableObjectGroupPlan {
    pub(super) extension: &'static str,
    pub(super) children: Vec<ScriptableObjectGroupMemberPlan>,
}

pub(super) struct ScriptableObjectGroupMemberPlan {
    pub(super) file_index: usize,
    pub(super) object_index: usize,
    pub(super) file_id: i64,
}

struct GroupRootCandidate {
    key: SourceObjectKey,
    extension: &'static str,
    children: Vec<SourceObjectKey>,
}

/// Mirrors the source ScriptableObjectProcessor for TimelineAsset and PostProcessProfile.
///
/// Timeline members are emitted into one `.playable` YAML stream. Children referenced from more
/// than one group remain standalone assets, matching the source processor's unique-child rule.
pub(super) fn prepare_scriptable_object_groups(
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    already_owned: &HashSet<ExportAssetObjectKey>,
) -> Result<PreparedScriptableObjectGroups, Box<dyn Error>> {
    let mut roots = Vec::new();
    let mut root_keys = HashSet::new();
    for object in objects
        .objects()
        .iter()
        .filter(|object| object.factory.class_id == 114)
    {
        let key = (object.file_index, object.object_index);
        if source_key_is_owned(key, already_owned) {
            continue;
        }
        let Some(payload) = object.payload.as_ref().ok() else {
            continue;
        };
        let has_tracks = payload
            .root
            .unique_field_in_base_chain("m_Tracks")
            .map_err(|error| group_field_error(object, "m_Tracks", error))?
            .is_some();
        let has_profile_settings = payload
            .root
            .unique_field_in_base_chain("settings")
            .map_err(|error| group_field_error(object, "settings", error))?
            .is_some();
        if !has_tracks && !has_profile_settings {
            continue;
        }
        let Some(script) = mono_behaviour_script_identity(object, objects, reference_graph)? else {
            continue;
        };
        let (extension, children) =
            if script.namespace == "UnityEngine.Timeline" && script.class == "TimelineAsset" {
                (
                    "playable",
                    find_timeline_children(object, objects, reference_graph)?,
                )
            } else if script.namespace == "UnityEngine.Rendering.PostProcessing"
                && script.class == "PostProcessProfile"
            {
                (
                    "asset",
                    find_post_process_profile_children(object, objects, reference_graph)?,
                )
            } else {
                continue;
            };
        root_keys.insert(key);
        roots.push(GroupRootCandidate {
            key,
            extension,
            children,
        });
    }

    let mut child_counts = HashMap::<SourceObjectKey, usize>::new();
    for root in &roots {
        for child in &root.children {
            if root_keys.contains(child) || source_key_is_owned(*child, already_owned) {
                continue;
            }
            let count = child_counts.entry(*child).or_default();
            *count = count.saturating_add(1);
        }
    }

    let mut prepared_roots = HashMap::new();
    let mut member_keys = HashSet::new();
    for root in roots {
        let mut children = Vec::new();
        let mut seen = HashSet::new();
        for child in root.children {
            if root_keys.contains(&child)
                || source_key_is_owned(child, already_owned)
                || child_counts.get(&child) != Some(&1)
                || !seen.insert(child)
            {
                continue;
            }
            let modifier = u32::try_from(children.len().checked_add(1).ok_or_else(|| {
                invalid_project("ScriptableObjectGroup child file ID modifier overflows")
            })?)
            .map_err(|_| invalid_project("ScriptableObjectGroup has too many child assets"))?;
            let file_id =
                crate::export::ids::get_main_export_id(114, modifier).map_err(|error| {
                    invalid_project(format!(
                        "ScriptableObjectGroup child file ID could not be planned: {error}"
                    ))
                })?;
            children.push(ScriptableObjectGroupMemberPlan {
                file_index: child.0,
                object_index: child.1,
                file_id,
            });
            member_keys.insert(child);
        }
        prepared_roots.insert(
            root.key,
            ScriptableObjectGroupPlan {
                extension: root.extension,
                children,
            },
        );
    }

    Ok(PreparedScriptableObjectGroups {
        roots: prepared_roots,
        member_keys,
    })
}

fn source_key_is_owned(key: SourceObjectKey, owned: &HashSet<ExportAssetObjectKey>) -> bool {
    owned.contains(&ExportAssetObjectKey {
        collection: ExportAssetCollectionIdentity::SerializedFile { file_index: key.0 },
        object_index: key.1,
    })
}

fn mono_behaviour_script_identity(
    object: &WorkspaceObject,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Option<crate::processing::managed_assembly::MonoScriptExportIdentity>, Box<dyn Error>> {
    let payload = object.payload.as_ref().map_err(|error| {
        invalid_project(format!(
            "MonoBehaviour {}:{} could not be decoded while identifying its ScriptableObject group: {error}",
            object.file_index, object.path_id
        ))
    })?;
    let Some(pointer) = payload
        .root
        .unique_field_in_base_chain("m_Script")
        .map_err(|error| group_field_error(object, "m_Script", error))?
    else {
        return Ok(None);
    };
    let Some((target_file_index, target_object_index)) =
        resolve_group_pointer(object, pointer, reference_graph)?
    else {
        return Ok(None);
    };
    if reference_graph.object_class_id(target_file_index, target_object_index) != Some(115) {
        return Ok(None);
    }
    let script = objects
        .get(target_file_index, target_object_index)
        .ok_or_else(|| {
            invalid_project(format!(
                "MonoBehaviour {}:{} m_Script resolved outside ObjectWorkspace to {}:{}",
                object.file_index, object.path_id, target_file_index, target_object_index
            ))
        })?;
    let identity = mono_script_export_identity(script).map_err(|error| {
        invalid_project(format!(
            "MonoBehaviour {}:{} references MonoScript {}:{} whose type identity is unreadable: {error}",
            object.file_index,
            object.path_id,
            target_file_index,
            script.path_id
        ))
    })?;
    Ok(Some(identity))
}

fn find_timeline_children(
    root: &WorkspaceObject,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Vec<SourceObjectKey>, Box<dyn Error>> {
    let payload = root.payload.as_ref().map_err(|error| {
        invalid_project(format!(
            "TimelineAsset {}:{} could not be decoded: {error}",
            root.file_index, root.path_id
        ))
    })?;
    let mut children = Vec::new();
    let mut unique = HashSet::new();
    if let Some(tracks) = payload
        .root
        .unique_field_in_base_chain("m_Tracks")
        .map_err(|error| group_field_error(root, "m_Tracks", error))?
    {
        for track_pointer in array_values(root, "m_Tracks", tracks)? {
            let Some(track_key) =
                group_object_pointer(root, track_pointer, objects, reference_graph)?
            else {
                continue;
            };
            let track = objects.get(track_key.0, track_key.1).ok_or_else(|| {
                invalid_project(format!(
                    "TimelineAsset {}:{} track {}:{} is absent from ObjectWorkspace",
                    root.file_index, root.path_id, track_key.0, track_key.1
                ))
            })?;
            let track_payload = track.payload.as_ref().map_err(|error| {
                invalid_project(format!(
                    "TimelineAsset {}:{} track {}:{} could not be decoded: {error}",
                    root.file_index, root.path_id, track.file_index, track.path_id
                ))
            })?;
            let Some(parent) = track_payload
                .root
                .unique_field_in_base_chain("m_Parent")
                .map_err(|error| group_field_error(track, "m_Parent", error))?
            else {
                continue;
            };
            if resolve_group_pointer(track, parent, reference_graph)?
                != Some((root.file_index, root.object_index))
            {
                continue;
            }
            push_unique_child(&mut children, &mut unique, track_key);

            if let Some(clips) = track_payload
                .root
                .unique_field_in_base_chain("m_Clips")
                .map_err(|error| group_field_error(track, "m_Clips", error))?
            {
                for (clip_index, clip) in array_values(track, "m_Clips", clips)?.iter().enumerate()
                {
                    let SerializedValue::Struct(_) = clip else {
                        return Err(invalid_project(format!(
                            "Timeline track {}:{} m_Clips[{clip_index}] is not a structure",
                            track.file_index, track.path_id
                        )));
                    };
                    let Some(asset) = clip
                        .unique_field("m_Asset")
                        .map_err(|error| group_field_error(track, "m_Clips[].m_Asset", error))?
                    else {
                        continue;
                    };
                    if let Some(asset_key) =
                        group_object_pointer(track, asset, objects, reference_graph)?
                    {
                        push_unique_child(&mut children, &mut unique, asset_key);
                    }
                }
            }

            if let Some(markers) = track_payload
                .root
                .unique_field_in_base_chain("m_Markers")
                .map_err(|error| group_field_error(track, "m_Markers", error))?
            {
                let marker_objects = markers
                    .unique_field("m_Objects")
                    .map_err(|error| group_field_error(track, "m_Markers.m_Objects", error))?;
                if let Some(marker_objects) = marker_objects {
                    for marker_pointer in
                        array_values(track, "m_Markers.m_Objects", marker_objects)?
                    {
                        if let Some(marker_key) =
                            group_object_pointer(track, marker_pointer, objects, reference_graph)?
                        {
                            push_unique_child(&mut children, &mut unique, marker_key);
                        }
                    }
                }
            }
        }
    }
    if let Some(marker_track) = payload
        .root
        .unique_field_in_base_chain("m_MarkerTrack")
        .map_err(|error| group_field_error(root, "m_MarkerTrack", error))?
    {
        if let Some(marker_key) =
            group_object_pointer(root, marker_track, objects, reference_graph)?
        {
            push_unique_child(&mut children, &mut unique, marker_key);
        }
    }
    Ok(children)
}

fn find_post_process_profile_children(
    root: &WorkspaceObject,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Vec<SourceObjectKey>, Box<dyn Error>> {
    let payload = root.payload.as_ref().map_err(|error| {
        invalid_project(format!(
            "PostProcessProfile {}:{} could not be decoded: {error}",
            root.file_index, root.path_id
        ))
    })?;
    let mut children = Vec::new();
    let mut unique = HashSet::new();
    if let Some(settings) = payload
        .root
        .unique_field_in_base_chain("settings")
        .map_err(|error| group_field_error(root, "settings", error))?
    {
        for setting_pointer in array_values(root, "settings", settings)? {
            if let Some(setting_key) =
                group_object_pointer(root, setting_pointer, objects, reference_graph)?
            {
                push_unique_child(&mut children, &mut unique, setting_key);
            }
        }
    }
    Ok(children)
}

fn array_values<'value>(
    object: &WorkspaceObject,
    field_name: &str,
    value: &'value SerializedValue,
) -> Result<&'value [SerializedValue], Box<dyn Error>> {
    match value {
        SerializedValue::Array(values) => Ok(values),
        _ => Err(invalid_project(format!(
            "ScriptableObject {}:{} field {field_name:?} is not an array",
            object.file_index, object.path_id
        ))),
    }
}

fn group_object_pointer(
    source: &WorkspaceObject,
    pointer: &SerializedValue,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Option<SourceObjectKey>, Box<dyn Error>> {
    let Some((file_index, object_index)) = resolve_group_pointer(source, pointer, reference_graph)?
    else {
        return Ok(None);
    };
    if reference_graph.object_class_id(file_index, object_index) != Some(114) {
        return Ok(None);
    }
    if objects.get(file_index, object_index).is_none() {
        return Err(invalid_project(format!(
            "ScriptableObject {}:{} resolves a MonoBehaviour pointer outside ObjectWorkspace to {file_index}:{object_index}",
            source.file_index, source.path_id
        )));
    }
    Ok(Some((file_index, object_index)))
}

fn resolve_group_pointer(
    source: &WorkspaceObject,
    pointer: &SerializedValue,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Option<SourceObjectKey>, Box<dyn Error>> {
    let SerializedValue::PPtr { file_id, path_id } = pointer else {
        return Err(invalid_project(format!(
            "ScriptableObject {}:{} expected a PPtr while preparing generated group membership",
            source.file_index, source.path_id
        )));
    };
    if *path_id == 0 {
        return Ok(None);
    }
    let edge = reference_graph
        .references_from_object(source.file_index, source.object_index)
        .find(|edge| edge.file_id == *file_id && edge.path_id == *path_id);
    let Some(edge) = edge else {
        return Err(invalid_project(format!(
            "ScriptableObject {}:{} PPtr ({file_id}, {path_id}) has no workspace reference-graph edge",
            source.file_index, source.path_id
        )));
    };
    match edge.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => Ok(Some((target_file_index, target_object_index))),
        _ => Ok(None),
    }
}

fn push_unique_child(
    children: &mut Vec<SourceObjectKey>,
    unique: &mut HashSet<SourceObjectKey>,
    child: SourceObjectKey,
) {
    if unique.insert(child) {
        children.push(child);
    }
}

fn group_field_error(
    object: &WorkspaceObject,
    field_name: &str,
    error: impl std::fmt::Display,
) -> Box<dyn Error> {
    invalid_project(format!(
        "ScriptableObject {}:{} field {field_name:?} could not be read while preparing generated groups: {error}",
        object.file_index, object.path_id
    ))
}
