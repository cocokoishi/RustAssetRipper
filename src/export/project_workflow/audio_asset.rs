use super::*;

pub(super) fn add_generated_audio_yaml_defaults(
    document: &mut crate::export::unity_yaml::UnityYamlDocument,
    generated_type: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let Some(generated_type) = generated_type else {
        return Ok(());
    };
    let body = match &mut document.root.kind {
        crate::export::unity_yaml::UnityYamlNodeKind::Mapping { entries, .. } => entries
            .iter_mut()
            .find_map(|(key, value)| audio_yaml_key_matches(key, "AudioClip").then_some(value)),
        _ => None,
    }
    .ok_or_else(|| invalid_project("AudioClip YAML document has no AudioClip mapping"))?;
    let fields = match &mut body.kind {
        crate::export::unity_yaml::UnityYamlNodeKind::Mapping { entries, .. } => entries,
        _ => return Err(invalid_project("AudioClip YAML body is not a mapping")),
    };
    let mut base_fields = vec![
        (
            "m_ObjectHideFlags",
            crate::export::unity_yaml::UnityYamlNode::scalar(
                crate::export::unity_yaml::UnityYamlScalar::Unsigned(0),
            ),
        ),
        ("m_CorrespondingSourceObject", audio_yaml_null_pointer()?),
        ("m_PrefabInstance", audio_yaml_null_pointer()?),
        ("m_PrefabAsset", audio_yaml_null_pointer()?),
    ];
    fields
        .try_reserve(base_fields.len() + 2)
        .map_err(|_| invalid_project("AudioClip generated defaults cannot be allocated"))?;
    for (name, value) in base_fields.drain(..).rev() {
        if !audio_yaml_mapping_has_field(fields, name) {
            fields.insert(
                0,
                (
                    crate::export::unity_yaml::UnityYamlNode::plain_string(name),
                    value,
                ),
            );
        }
    }

    if generated_type == "AudioClip_2018_3" {
        let has_editor_resource = audio_yaml_mapping_has_field(fields, "m_EditorResource");
        let has_editor_compression =
            audio_yaml_mapping_has_field(fields, "m_EditorCompressionFormat");
        if !has_editor_resource || !has_editor_compression {
            let mut editor_resource = crate::export::unity_yaml::UnityYamlNode::mapping(
                crate::export::unity_yaml::UnityYamlMappingStyle::Block,
            );
            editor_resource
                .insert_field(
                    "m_Source",
                    crate::export::unity_yaml::UnityYamlNode::scalar(
                        crate::export::unity_yaml::UnityYamlScalar::PlainString(String::new()),
                    ),
                )
                .and_then(|()| {
                    editor_resource.insert_field(
                        "m_Offset",
                        crate::export::unity_yaml::UnityYamlNode::scalar(
                            crate::export::unity_yaml::UnityYamlScalar::Unsigned(0),
                        ),
                    )
                })
                .and_then(|()| {
                    editor_resource.insert_field(
                        "m_Size",
                        crate::export::unity_yaml::UnityYamlNode::scalar(
                            crate::export::unity_yaml::UnityYamlScalar::Unsigned(0),
                        ),
                    )
                })
                .map_err(|error| {
                    invalid_project(format!(
                        "AudioClip editor resource cannot be built: {error}"
                    ))
                })?;
            let editor_compression = crate::export::unity_yaml::UnityYamlNode::scalar(
                crate::export::unity_yaml::UnityYamlScalar::Unsigned(0),
            );
            let insertion_index = fields
                .iter()
                .position(|(key, _)| audio_yaml_key_matches(key, "m_CompressionFormat"))
                .map_or(fields.len(), |index| index + 1);
            if !has_editor_resource {
                fields.insert(
                    insertion_index,
                    (
                        crate::export::unity_yaml::UnityYamlNode::plain_string("m_EditorResource"),
                        editor_resource,
                    ),
                );
            }
            if !has_editor_compression {
                let editor_index = fields
                    .iter()
                    .position(|(key, _)| audio_yaml_key_matches(key, "m_EditorResource"))
                    .map_or(fields.len(), |index| index + 1);
                fields.insert(
                    editor_index,
                    (
                        crate::export::unity_yaml::UnityYamlNode::plain_string(
                            "m_EditorCompressionFormat",
                        ),
                        editor_compression,
                    ),
                );
            }
        }
    }
    Ok(())
}

fn audio_yaml_key_matches(node: &crate::export::unity_yaml::UnityYamlNode, expected: &str) -> bool {
    matches!(
        &node.kind,
        crate::export::unity_yaml::UnityYamlNodeKind::Scalar(
            crate::export::unity_yaml::UnityYamlScalar::PlainString(value)
        ) if value == expected
    )
}

pub(super) fn rewrite_audio_yaml_resource(
    document: &mut crate::export::unity_yaml::UnityYamlDocument,
    resource: &crate::resources::audio::AudioClipResourceInfo,
    sidecar_source: Option<&str>,
    sidecar_bytes: Option<&[u8]>,
) -> Result<(), Box<dyn Error>> {
    let audio_body = match &mut document.root.kind {
        crate::export::unity_yaml::UnityYamlNodeKind::Mapping { entries, .. } => {
            let mut audio_body_index = None;
            for (index, (key, _)) in entries.iter().enumerate() {
                if audio_yaml_key_matches(key, "AudioClip")
                    && audio_body_index.replace(index).is_some()
                {
                    return Err(invalid_project(
                        "AudioClip YAML document has duplicate AudioClip mappings",
                    ));
                }
            }
            let index = audio_body_index.ok_or_else(|| {
                invalid_project("AudioClip YAML document has no AudioClip mapping")
            })?;
            &mut entries[index].1
        }
        _ => {
            return Err(invalid_project(
                "AudioClip YAML document root is not a mapping",
            ))
        }
    };
    let resource_mapping = match &mut audio_body.kind {
        crate::export::unity_yaml::UnityYamlNodeKind::Mapping { entries, .. } => {
            let mut resource_index = None;
            for (index, (key, _)) in entries.iter().enumerate() {
                if audio_yaml_key_matches(key, "m_Resource")
                    && resource_index.replace(index).is_some()
                {
                    return Err(invalid_project(
                        "AudioClip YAML body has duplicate m_Resource mappings",
                    ));
                }
            }
            let index = resource_index
                .ok_or_else(|| invalid_project("AudioClip YAML body has no m_Resource mapping"))?;
            &mut entries[index].1
        }
        _ => return Err(invalid_project("AudioClip YAML body is not a mapping")),
    };
    let fields = match &mut resource_mapping.kind {
        crate::export::unity_yaml::UnityYamlNodeKind::Mapping { entries, .. } => entries,
        _ => {
            return Err(invalid_project(
                "AudioClip m_Resource value is not a mapping",
            ))
        }
    };
    let (source, size) = match (resource.size, sidecar_source, sidecar_bytes) {
        (0, None, None) => ("", 0),
        (0, Some(_), _) => {
            return Err(invalid_project(
                "AudioClip sidecar source path exists without streamed bytes",
            ));
        }
        (_, None, _) => {
            return Err(invalid_project(
                "AudioClip streamed bytes have no planned sidecar source path",
            ));
        }
        (_, Some(_), None) => {
            return Err(invalid_project(
                "AudioClip sidecar source path exists without resolved streamed bytes",
            ));
        }
        (source_size, Some(path), Some(bytes)) => {
            let sidecar_size = u64::try_from(bytes.len()).map_err(|_| {
                invalid_project("AudioClip sidecar length does not fit its serialized size")
            })?;
            if sidecar_size != source_size {
                return Err(invalid_project(format!(
                    "AudioClip source range declares {} bytes but the sidecar contains {sidecar_size}",
                    source_size
                )));
            }
            (path, sidecar_size)
        }
    };
    set_audio_yaml_resource_field(
        fields,
        &["m_Source", "path"],
        "source path",
        crate::export::unity_yaml::UnityYamlNode::plain_string(source),
    )?;
    set_audio_yaml_resource_field(
        fields,
        &["m_Offset", "offset"],
        "offset",
        crate::export::unity_yaml::UnityYamlNode::scalar(
            crate::export::unity_yaml::UnityYamlScalar::Unsigned(0),
        ),
    )?;
    set_audio_yaml_resource_field(
        fields,
        &["m_Size", "size"],
        "size",
        crate::export::unity_yaml::UnityYamlNode::scalar(
            crate::export::unity_yaml::UnityYamlScalar::Unsigned(size),
        ),
    )?;
    Ok(())
}

fn set_audio_yaml_resource_field(
    fields: &mut [(
        crate::export::unity_yaml::UnityYamlNode,
        crate::export::unity_yaml::UnityYamlNode,
    )],
    aliases: &[&str],
    field_label: &str,
    value: crate::export::unity_yaml::UnityYamlNode,
) -> Result<(), Box<dyn Error>> {
    let mut matching_index = None;
    for (index, (key, _)) in fields.iter().enumerate() {
        if aliases
            .iter()
            .any(|alias| audio_yaml_key_matches(key, alias))
            && matching_index.replace(index).is_some()
        {
            return Err(invalid_project(format!(
                "AudioClip m_Resource has duplicate {field_label} fields"
            )));
        }
    }
    let index = matching_index.ok_or_else(|| {
        invalid_project(format!(
            "AudioClip m_Resource has no serialized {field_label} field"
        ))
    })?;
    fields[index].1 = value;
    Ok(())
}

pub(super) fn project_relative_path_for_host(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_owned()
    }
}

fn audio_yaml_mapping_has_field(
    fields: &[(
        crate::export::unity_yaml::UnityYamlNode,
        crate::export::unity_yaml::UnityYamlNode,
    )],
    name: &str,
) -> bool {
    fields
        .iter()
        .any(|(key, _)| audio_yaml_key_matches(key, name))
}

fn audio_yaml_null_pointer() -> Result<crate::export::unity_yaml::UnityYamlNode, Box<dyn Error>> {
    let mut pointer = crate::export::unity_yaml::UnityYamlNode::mapping(
        crate::export::unity_yaml::UnityYamlMappingStyle::Flow,
    );
    pointer
        .insert_field(
            "fileID",
            crate::export::unity_yaml::UnityYamlNode::scalar(
                crate::export::unity_yaml::UnityYamlScalar::Unsigned(0),
            ),
        )
        .map_err(|error| {
            invalid_project(format!("AudioClip base PPtr cannot be built: {error}"))
        })?;
    Ok(pointer)
}
