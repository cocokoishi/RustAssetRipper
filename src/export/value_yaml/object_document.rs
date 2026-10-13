use crate::processing::scene_naming::SceneUnityVersion;
use crate::serialized_file::object::SerializedValue;

use super::super::pointer_map::ProjectPointerResolution;
use super::super::scene_collection::ExportAssetIdentity;
use super::super::unity_yaml::{
    UnityYamlDocument, UnityYamlMappingStyle, UnityYamlNode, UnityYamlNodeBuildError,
    UnityYamlNodeKind, UnityYamlScalar,
};
use super::conversion::convert_object_body;
use super::model::{
    UnityYamlObjectConversionError, UnityYamlObjectConversionHints,
    UnityYamlObjectConversionLimits, UnityYamlObjectFieldMode, UnityYamlPPtrRequest,
};
/// Convert one already-decoded object value into a Unity YAML object document.
///
/// `SerializedValue` retains ordered fields and PPtrs but not the TypeTree schema. Callers supply
/// element-kind hints for empty arrays, where the serialized value alone cannot distinguish a
/// numeric hex list from an object sequence. Non-null PPtrs are resolved by the caller from the
/// multi-file graph and project export map. The generic conversion deliberately rejects raw
/// unknown leaves instead of replacing them with hex dumps.
#[allow(clippy::too_many_arguments)]
pub fn serialized_object_value_to_unity_yaml_document<F>(
    source: ExportAssetIdentity,
    class_name: &str,
    serialized_version: i32,
    file_id: i64,
    stripped: bool,
    root: &SerializedValue,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    unity_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    mut resolve_pointer: F,
) -> Result<UnityYamlDocument, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    let mut body = convert_object_body(
        source,
        serialized_version,
        stripped,
        root,
        hints,
        field_mode,
        unity_version,
        missing_reference_guid,
        limits,
        &mut resolve_pointer,
    )?;
    if source.class_id == 114 {
        if field_mode == UnityYamlObjectFieldMode::ProjectAsset
            && !stripped
            && has_mono_behaviour_yaml_header(&body)
        {
            flatten_mapping_field(&mut body, "m_Structure")?;
            insert_material_editor_fields(
                &mut body,
                &[
                    "m_ObjectHideFlags",
                    "m_CorrespondingSourceObject",
                    "m_PrefabInstance",
                    "m_PrefabAsset",
                ],
            )?;
            insert_mapping_field_before(
                &mut body,
                "m_EditorHideFlags",
                "m_Script",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
            )?;
            insert_material_field_after(
                &mut body,
                "m_EditorClassIdentifier",
                "m_Name",
                UnityYamlNode::string(String::new()),
            )?;
        } else {
            move_mapping_field_to_end(&mut body, "m_Structure")?;
        }
    }
    if source.class_id == 21 && field_mode == UnityYamlObjectFieldMode::ProjectAsset && !stripped {
        match (unity_version.major, unity_version.minor) {
            (2017, 4) => insert_material_editor_fields(
                &mut body,
                &[
                    "m_ObjectHideFlags",
                    "m_PrefabParentObject",
                    "m_PrefabInternal",
                ],
            )?,
            (2018, 2) => insert_material_editor_fields(
                &mut body,
                &[
                    "m_ObjectHideFlags",
                    "m_CorrespondingSourceObject",
                    "m_PrefabInternal",
                ],
            )?,
            (2018, 3) | (2019, 4) => insert_material_editor_fields(
                &mut body,
                &[
                    "m_ObjectHideFlags",
                    "m_CorrespondingSourceObject",
                    "m_PrefabInstance",
                    "m_PrefabAsset",
                ],
            )?,
            (2022, 3) => insert_late_material_editor_fields(&mut body, false)?,
            (6000, 4) => insert_late_material_editor_fields(&mut body, true)?,
            _ => {}
        }
    }
    // The source walker labels native documents with asset.ClassName, not the
    // TypeTree root. Some native classes reuse an abstract base layout: both
    // FlareLayer and HaloLayer have a Behaviour root even in their editor tree.
    // Keep that schema for the fields, but preserve the declared native identity
    // in project YAML. Unknown classes and explicit/script names stay intact.
    let class_name = if field_mode == UnityYamlObjectFieldMode::ProjectAsset
        && matches!(
            class_name,
            "Object" | "EditorExtension" | "Component" | "Behaviour"
        ) {
        hints.native_class_name.as_deref().unwrap_or(class_name)
    } else {
        class_name
    };
    UnityYamlDocument::object(source.class_id, file_id, class_name, body, stripped)
        .map_err(UnityYamlObjectConversionError::NodeBuild)
}

fn insert_material_editor_fields(
    body: &mut UnityYamlNode,
    editor_fields: &[&str],
) -> Result<(), UnityYamlObjectConversionError> {
    let UnityYamlNodeKind::Mapping { style, .. } = &body.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let style = *style;
    let previous = std::mem::replace(
        &mut body.kind,
        UnityYamlNodeKind::Mapping {
            style,
            entries: Vec::new(),
        },
    );
    let UnityYamlNodeKind::Mapping { style, mut entries } = previous else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let requested = entries.len().checked_add(editor_fields.len()).ok_or(
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Material Editor YAML fields",
            requested: usize::MAX,
        },
    )?;
    let mut ordered = Vec::new();
    ordered.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Material Editor YAML fields",
            requested,
        }
    })?;

    if let Some(entry) = take_yaml_mapping_field(&mut entries, "serializedVersion") {
        ordered.push(entry);
    }
    for &field in editor_fields {
        ordered.push(take_yaml_mapping_field(&mut entries, field).unwrap_or((
            UnityYamlNode::plain_string(field),
            match field {
                "m_ObjectHideFlags" => UnityYamlNode::scalar(UnityYamlScalar::Unsigned(0)),
                _ => {
                    let mut pointer = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
                    pointer
                        .insert_field("fileID", UnityYamlNode::scalar(UnityYamlScalar::Signed(0)))
                        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                    pointer
                }
            },
        )));
    }
    ordered.append(&mut entries);
    body.kind = UnityYamlNodeKind::Mapping {
        style,
        entries: ordered,
    };
    Ok(())
}

fn insert_late_material_editor_fields(
    body: &mut UnityYamlNode,
    include_allow_locking: bool,
) -> Result<(), UnityYamlObjectConversionError> {
    insert_material_editor_fields(
        body,
        &[
            "m_ObjectHideFlags",
            "m_CorrespondingSourceObject",
            "m_PrefabInstance",
            "m_PrefabAsset",
        ],
    )?;
    insert_material_field_after(body, "m_Parent", "m_Shader", material_null_pointer()?)?;
    insert_material_field_after(
        body,
        "m_ModifiedSerializedProperties",
        "m_Parent",
        UnityYamlNode::scalar(UnityYamlScalar::Unsigned(0)),
    )?;
    insert_material_field_after(
        body,
        "m_LockedProperties",
        "disabledShaderPasses",
        UnityYamlNode::plain_string(""),
    )?;
    if include_allow_locking {
        insert_material_field_after(
            body,
            "m_AllowLocking",
            "m_BuildTextureStacks",
            UnityYamlNode::scalar(UnityYamlScalar::Boolean(false)),
        )?;
    }
    Ok(())
}

fn material_null_pointer() -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    let mut pointer = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    pointer
        .insert_field("fileID", UnityYamlNode::scalar(UnityYamlScalar::Signed(0)))
        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    Ok(pointer)
}

fn insert_material_field_after(
    body: &mut UnityYamlNode,
    field_name: &str,
    anchor_name: &str,
    default_value: UnityYamlNode,
) -> Result<(), UnityYamlObjectConversionError> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut body.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let field = take_yaml_mapping_field(entries, field_name)
        .unwrap_or((UnityYamlNode::plain_string(field_name), default_value));
    let Some(anchor_index) = entries.iter().position(|(key, _)| {
        matches!(
            &key.kind,
            UnityYamlNodeKind::Scalar(
                UnityYamlScalar::PlainString(value) | UnityYamlScalar::String(value)
            ) if value == anchor_name
        )
    }) else {
        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
            node_index: 0,
            reason: "missing Unity YAML editor-field insertion anchor",
        });
    };
    entries
        .try_reserve(1)
        .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
            resource: "Material Editor YAML fields",
            requested: entries.len().saturating_add(1),
        })?;
    entries.insert(anchor_index + 1, field);
    Ok(())
}

fn insert_mapping_field_before(
    body: &mut UnityYamlNode,
    field_name: &str,
    anchor_name: &str,
    default_value: UnityYamlNode,
) -> Result<(), UnityYamlObjectConversionError> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut body.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let field = take_yaml_mapping_field(entries, field_name)
        .unwrap_or((UnityYamlNode::plain_string(field_name), default_value));
    let Some(anchor_index) = entries.iter().position(|(key, _)| {
        matches!(
            &key.kind,
            UnityYamlNodeKind::Scalar(
                UnityYamlScalar::PlainString(value) | UnityYamlScalar::String(value)
            ) if value == anchor_name
        )
    }) else {
        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
            node_index: 0,
            reason: "missing Unity YAML editor-field insertion anchor",
        });
    };
    entries
        .try_reserve(1)
        .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML editor fields",
            requested: entries.len().saturating_add(1),
        })?;
    entries.insert(anchor_index, field);
    Ok(())
}

fn has_mono_behaviour_yaml_header(body: &UnityYamlNode) -> bool {
    let UnityYamlNodeKind::Mapping { entries, .. } = &body.kind else {
        return false;
    };
    ["m_GameObject", "m_Script"].into_iter().all(|field_name| {
        entries.iter().any(|(key, value)| {
            matches!(
                &key.kind,
                UnityYamlNodeKind::Scalar(
                    UnityYamlScalar::PlainString(name) | UnityYamlScalar::String(name)
                ) if name == field_name
            ) && matches!(&value.kind, UnityYamlNodeKind::Mapping { .. })
        })
    })
}

fn flatten_mapping_field(
    body: &mut UnityYamlNode,
    field_name: &str,
) -> Result<(), UnityYamlObjectConversionError> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut body.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let mut field_index = None;
    for (index, (key, _)) in entries.iter().enumerate() {
        if matches!(
            &key.kind,
            UnityYamlNodeKind::Scalar(
                UnityYamlScalar::PlainString(name) | UnityYamlScalar::String(name)
            ) if name == field_name
        ) && field_index.replace(index).is_some()
        {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: 0,
                reason: "MonoBehaviour contains multiple m_Structure fields",
            });
        }
    }
    let Some(field_index) = field_index else {
        return Ok(());
    };
    let (_, structure) = entries.remove(field_index);
    let UnityYamlNodeKind::Mapping {
        entries: structure_fields,
        ..
    } = structure.kind
    else {
        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
            node_index: 0,
            reason: "MonoBehaviour m_Structure is not a structure",
        });
    };
    entries
        .try_reserve(structure_fields.len().saturating_sub(1))
        .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
            resource: "flattened MonoBehaviour YAML fields",
            requested: entries.len().saturating_add(structure_fields.len()),
        })?;
    entries.splice(field_index..field_index, structure_fields);
    Ok(())
}

fn take_yaml_mapping_field(
    entries: &mut Vec<(UnityYamlNode, UnityYamlNode)>,
    field_name: &str,
) -> Option<(UnityYamlNode, UnityYamlNode)> {
    let index = entries.iter().position(|(key, _)| {
        matches!(
            &key.kind,
            UnityYamlNodeKind::Scalar(
                UnityYamlScalar::PlainString(value) | UnityYamlScalar::String(value)
            ) if value == field_name
        )
    })?;
    Some(entries.remove(index))
}

fn move_mapping_field_to_end(
    mapping: &mut UnityYamlNode,
    field_name: &str,
) -> Result<(), UnityYamlObjectConversionError> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut mapping.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let field_index = entries.iter().position(|(key, _)| {
        matches!(
            &key.kind,
            UnityYamlNodeKind::Scalar(
                UnityYamlScalar::String(name) | UnityYamlScalar::PlainString(name)
            ) if name == field_name
        )
    });
    if let Some(field_index) = field_index {
        let field = entries.remove(field_index);
        entries.push(field);
    }
    Ok(())
}
