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

#[allow(clippy::too_many_arguments)]
fn append_struct_fields<F>(
    destination: &mut UnityYamlNode,
    fields: &[SerializedFieldValue],
    parent_path: &str,
    depth: usize,
    source: ExportAssetIdentity,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    stripped_fields: Option<&'static [&'static str]>,
    unity_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
    resolve_pointer: &mut F,
) -> Result<(), UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    for field in fields {
        let field_path = child_field_path(parent_path, &field.name, limits.max_field_path_bytes)?;
        if field_mode == UnityYamlObjectFieldMode::AssetImporter
            && importer_field_is_excluded(&field.name, &field_path, hints, limits)?
        {
            continue;
        }
        if field.name == "Base" {
            if let SerializedValue::Struct(base_fields) = &field.value {
                budget.visit(depth)?;
                append_struct_fields(
                    destination,
                    base_fields,
                    &field_path,
                    depth + 1,
                    source,
                    hints,
                    field_mode,
                    stripped_fields,
                    unity_version,
                    missing_reference_guid,
                    limits,
                    budget,
                    resolve_pointer,
                )?;
                continue;
            }
        }
        if stripped_fields.is_some_and(|allowed| !allowed.contains(&field.name.as_str())) {
            continue;
        }
        let value = convert_value(
            &field.value,
            &field_path,
            depth,
            source,
            hints,
            field_mode,
            unity_version,
            missing_reference_guid,
            limits,
            budget,
            resolve_pointer,
        )?;
        let contextual_field_name =
            field_name_alias_for_path(&field_path, hints, limits)?.unwrap_or(field.name.as_str());
        let field_name = if hints.preserve_type_tree_field_names {
            try_copy_string(contextual_field_name, "Unity TypeTree field name")?
        } else {
            baseline_yaml_field_name(contextual_field_name)?
        };
        destination
            .insert_field(field_name, value)
            .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn convert_value<F>(
    value: &SerializedValue,
    field_path: &str,
    depth: usize,
    source: ExportAssetIdentity,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    unity_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
    resolve_pointer: &mut F,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    budget.visit(depth)?;
    let node = match value {
        SerializedValue::Bool(value) => UnityYamlNode::scalar(UnityYamlScalar::Boolean(*value)),
        SerializedValue::I8(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(*value)))
        }
        SerializedValue::U8(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Unsigned(u64::from(*value)))
        }
        SerializedValue::I16(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(*value)))
        }
        SerializedValue::U16(value) | SerializedValue::Char(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Unsigned(u64::from(*value)))
        }
        SerializedValue::I32(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(*value)))
        }
        SerializedValue::U32(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Unsigned(u64::from(*value)))
        }
        SerializedValue::I64(value) => UnityYamlNode::scalar(UnityYamlScalar::Signed(*value)),
        SerializedValue::U64(value) => UnityYamlNode::scalar(UnityYamlScalar::Unsigned(*value)),
        SerializedValue::F32(value) => UnityYamlNode::scalar(UnityYamlScalar::Float32(*value)),
        SerializedValue::F64(value) => UnityYamlNode::scalar(UnityYamlScalar::Float64(*value)),
        SerializedValue::StringBytes(bytes) => {
            let value = str::from_utf8(bytes).map_err(|_| {
                UnityYamlObjectConversionError::InvalidUtf8String {
                    field_path: field_path.to_owned(),
                }
            })?;
            UnityYamlNode::string(try_copy_string(value, "Unity YAML string")?)
        }
        SerializedValue::Bytes(bytes) => {
            let hex_length = bytes.len().checked_mul(2).ok_or(
                UnityYamlObjectConversionError::HexScalarLimitExceeded {
                    requested: usize::MAX,
                    limit: limits.max_generated_scalar_bytes,
                    field_path: field_path.to_owned(),
                },
            )?;
            let hex = encode_byte_hex(bytes, hex_length, field_path, limits)?;
            UnityYamlNode::plain_string(hex)
        }
        SerializedValue::Array(items) => convert_array(
            items,
            field_path,
            depth,
            source,
            hints,
            field_mode,
            unity_version,
            missing_reference_guid,
            limits,
            budget,
            resolve_pointer,
        )?,
        SerializedValue::Struct(fields) => {
            let scene_object_identifier_schema =
                if field_mode == UnityYamlObjectFieldMode::ProjectAsset {
                    scene_object_identifier_schema_for_path(field_path, hints, limits)?
                } else {
                    None
                };
            if let Some(schema) = scene_object_identifier_schema {
                convert_scene_object_identifier_value(fields, schema, field_path, depth, budget)?
            } else if let Some(schema) = property_name_schema_for_path(field_path, hints, limits)? {
                convert_property_name_value(fields, schema, field_path, depth, budget)?
            } else if let Some(schema) = hash128_schema_for_path(field_path, hints, limits)? {
                convert_hash128_value(fields, schema, field_path, depth, limits, budget)?
            } else if let Some(schema) = guid_schema_for_path(field_path, hints, limits)? {
                convert_guid_value(fields, schema, field_path, depth, budget)?
            } else {
                let mut mapping =
                    UnityYamlNode::mapping(if is_flow_mapped_path(field_path, hints, limits)? {
                        UnityYamlMappingStyle::Flow
                    } else {
                        UnityYamlMappingStyle::Block
                    });
                if let Some(version) = serialized_version_for_path(field_path, hints, limits)? {
                    mapping
                        .insert_field_begin(
                            "serializedVersion",
                            UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(version))),
                        )
                        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                }
                append_struct_fields(
                    &mut mapping,
                    fields,
                    field_path,
                    depth + 1,
                    source,
                    hints,
                    field_mode,
                    None,
                    unity_version,
                    missing_reference_guid,
                    limits,
                    budget,
                    resolve_pointer,
                )?;
                mapping
            }
        }
        SerializedValue::PPtr { file_id, path_id } => {
            let resolution = if *path_id == 0 {
                ProjectPointerResolution::Null
            } else {
                resolve_pointer(UnityYamlPPtrRequest {
                    source,
                    field_path,
                    file_id: *file_id,
                    path_id: *path_id,
                })
            };
            project_pointer_to_unity_yaml(&resolution, unity_version, missing_reference_guid)
                .map_err(|source| UnityYamlObjectConversionError::PointerConversion {
                    field_path: field_path.to_owned(),
                    source: Box::new(source),
                })?
        }
        SerializedValue::ManagedReferenceId {
            reference_id,
            indexed,
        } => {
            let identifier_name = if *indexed { "id" } else { "rid" };
            let child_path =
                child_field_path(field_path, identifier_name, limits.max_field_path_bytes)?;
            budget.visit(depth + 1)?;
            budget.check_field_path(&child_path)?;
            let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
            mapping
                .insert_field(
                    identifier_name,
                    UnityYamlNode::scalar(UnityYamlScalar::Signed(*reference_id)),
                )
                .map_err(UnityYamlObjectConversionError::NodeBuild)?;
            mapping
        }
        SerializedValue::ManagedReferenceRegistry(registry) => convert_managed_reference_registry(
            registry,
            field_path,
            depth,
            source,
            hints,
            field_mode,
            unity_version,
            missing_reference_guid,
            limits,
            budget,
            resolve_pointer,
        )?,
        SerializedValue::Raw { type_name, .. } => {
            return Err(UnityYamlObjectConversionError::UnsupportedRawValue {
                field_path: field_path.to_owned(),
                type_name: type_name.clone(),
            });
        }
    };
    Ok(node)
}

#[allow(clippy::too_many_arguments)]
fn convert_managed_reference_registry<F>(
    registry: &super::super::serialized_file::managed_registry::ManagedRegistryRead,
    field_path: &str,
    depth: usize,
    source: ExportAssetIdentity,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    unity_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
    resolve_pointer: &mut F,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    for (name, value) in [
        ("version", SerializedValue::I32(registry.version)),
        ("RefIds", SerializedValue::Array(Vec::new())),
    ] {
        let path = child_field_path(field_path, name, limits.max_field_path_bytes)?;
        budget.visit(depth + 1)?;
        budget.check_field_path(&path)?;
        let converted = if name == "version" {
            convert_value(
                &value,
                &path,
                depth + 1,
                source,
                hints,
                field_mode,
                unity_version,
                missing_reference_guid,
                limits,
                budget,
                resolve_pointer,
            )?
        } else {
            let mut sequence = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
            for (entry_index, entry) in registry.entries.iter().enumerate() {
                let entry_path =
                    indexed_field_path(&path, entry_index, limits.max_field_path_bytes)?;
                budget.visit(depth + 2)?;
                budget.check_field_path(&entry_path)?;
                let mut entry_mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);

                let reference_path =
                    child_field_path(&entry_path, "rid", limits.max_field_path_bytes)?;
                budget.visit(depth + 3)?;
                budget.check_field_path(&reference_path)?;
                entry_mapping
                    .insert_field(
                        "rid",
                        UnityYamlNode::scalar(UnityYamlScalar::Signed(entry.reference_id)),
                    )
                    .map_err(UnityYamlObjectConversionError::NodeBuild)?;

                let type_path = child_field_path(&entry_path, "type", limits.max_field_path_bytes)?;
                budget.visit(depth + 3)?;
                budget.check_field_path(&type_path)?;
                let mut type_mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
                for (name, text) in [
                    ("class", entry.class_name.as_str()),
                    ("ns", entry.namespace_name.as_str()),
                    ("asm", entry.assembly_name.as_str()),
                ] {
                    let path = child_field_path(&type_path, name, limits.max_field_path_bytes)?;
                    budget.visit(depth + 4)?;
                    budget.check_field_path(&path)?;
                    type_mapping
                        .insert_field(
                            name,
                            UnityYamlNode::string(try_copy_string(
                                text,
                                "managed reference type name",
                            )?),
                        )
                        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                }
                entry_mapping
                    .insert_field("type", type_mapping)
                    .map_err(UnityYamlObjectConversionError::NodeBuild)?;

                let data_path = child_field_path(&entry_path, "data", limits.max_field_path_bytes)?;
                budget.visit(depth + 3)?;
                budget.check_field_path(&data_path)?;
                let data = match &entry.data {
                    super::super::serialized_file::managed_registry::ManagedRegistryEntryData::Empty => {
                        UnityYamlNode::mapping(UnityYamlMappingStyle::Block)
                    }
                    super::super::serialized_file::managed_registry::ManagedRegistryEntryData::Content(value) => {
                        convert_value(
                            value,
                            &data_path,
                            depth + 3,
                            source,
                            hints,
                            field_mode,
                            unity_version,
                            missing_reference_guid,
                            limits,
                            budget,
                            resolve_pointer,
                        )?
                    }
                };
                entry_mapping
                    .insert_field("data", data)
                    .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                sequence
                    .push(entry_mapping)
                    .map_err(UnityYamlObjectConversionError::NodeBuild)?;
            }
            sequence
        };
        mapping
            .insert_field(name, converted)
            .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    }
    Ok(mapping)
}

fn hash128_schema_for_path<'a>(
    field_path: &str,
    hints: &'a UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<&'a UnityYamlHash128Schema>, UnityYamlObjectConversionError> {
    if let Some(schema) = hints.hash128_schemas.get(field_path) {
        return Ok(Some(schema));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.hash128_schemas.get(&canonical_path))
}

fn dictionary_representation_for_path(
    field_path: &str,
    hints: &UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<UnityYamlDictionaryRepresentation>, UnityYamlObjectConversionError> {
    if let Some(representation) = hints.dictionary_representations.get(field_path) {
        return Ok(Some(*representation));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints
        .dictionary_representations
        .get(&canonical_path)
        .copied())
}

fn force_block_sequence_for_path(
    field_path: &str,
    hints: &UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<bool, UnityYamlObjectConversionError> {
    if hints.force_block_sequence_paths.contains(field_path) {
        return Ok(true);
    }
    if !field_path.contains('[') {
        return Ok(false);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.force_block_sequence_paths.contains(&canonical_path))
}

fn guid_schema_for_path<'a>(
    field_path: &str,
    hints: &'a UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<&'a UnityYamlGuidSchema>, UnityYamlObjectConversionError> {
    if let Some(schema) = hints.guid_schemas.get(field_path) {
        return Ok(Some(schema));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.guid_schemas.get(&canonical_path))
}

fn property_name_schema_for_path<'a>(
    field_path: &str,
    hints: &'a UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<&'a UnityYamlPropertyNameSchema>, UnityYamlObjectConversionError> {
    if let Some(schema) = hints.property_name_schemas.get(field_path) {
        return Ok(Some(schema));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.property_name_schemas.get(&canonical_path))
}

fn field_name_alias_for_path<'a>(
    field_path: &str,
    hints: &'a UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<&'a str>, UnityYamlObjectConversionError> {
    if let Some(alias) = hints.field_name_aliases.get(field_path) {
        return Ok(Some(alias));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints
        .field_name_aliases
        .get(&canonical_path)
        .map(String::as_str))
}

fn scene_object_identifier_schema_for_path<'a>(
    field_path: &str,
    hints: &'a UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<&'a UnityYamlSceneObjectIdentifierSchema>, UnityYamlObjectConversionError> {
    if let Some(schema) = hints.scene_object_identifier_schemas.get(field_path) {
        return Ok(Some(schema));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.scene_object_identifier_schemas.get(&canonical_path))
}

fn convert_hash128_value(
    fields: &[SerializedFieldValue],
    schema: &UnityYamlHash128Schema,
    field_path: &str,
    depth: usize,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    const HASH128_BYTE_COUNT: usize = 16;
    if schema.byte_field_names.len() != HASH128_BYTE_COUNT || fields.len() != HASH128_BYTE_COUNT {
        return Err(UnityYamlObjectConversionError::Hash128FieldCount {
            field_path: field_path.to_owned(),
            expected: HASH128_BYTE_COUNT,
            actual: fields.len(),
        });
    }

    let mut bytes = [0u8; HASH128_BYTE_COUNT];
    for (field_index, field_name) in schema.byte_field_names.iter().enumerate() {
        budget.visit(depth + 1)?;
        let mut matching_value = None;
        for field in fields.iter().filter(|field| field.name == *field_name) {
            if matching_value.is_some() {
                return Err(UnityYamlObjectConversionError::InvalidHash128Byte {
                    field_path: field_path.to_owned(),
                    field_name: field_name.clone(),
                    field_index,
                });
            }
            matching_value = Some(&field.value);
        }
        let Some(SerializedValue::U8(value)) = matching_value else {
            return Err(UnityYamlObjectConversionError::InvalidHash128Byte {
                field_path: field_path.to_owned(),
                field_name: field_name.clone(),
                field_index,
            });
        };
        bytes[field_index] = *value;
    }

    let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    mapping
        .insert_field(
            "serializedVersion",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(
                schema.serialized_version,
            ))),
        )
        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    if schema.serialized_version > 1 {
        const HASH_STRING_LENGTH: usize = HASH128_BYTE_COUNT * 2;
        if HASH_STRING_LENGTH > limits.max_generated_scalar_bytes {
            return Err(UnityYamlObjectConversionError::HexScalarLimitExceeded {
                requested: HASH_STRING_LENGTH,
                limit: limits.max_generated_scalar_bytes,
                field_path: field_path.to_owned(),
            });
        }
        let hash =
            format_hash128(bytes).map_err(|_| UnityYamlObjectConversionError::Hash128Overflow {
                field_path: field_path.to_owned(),
            })?;
        mapping
            .insert_field("Hash", UnityYamlNode::string(hash))
            .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    } else {
        for (field_index, value) in bytes.into_iter().enumerate() {
            mapping
                .insert_field(
                    format!("bytes[{field_index}]"),
                    UnityYamlNode::scalar(UnityYamlScalar::Unsigned(u64::from(value))),
                )
                .map_err(UnityYamlObjectConversionError::NodeBuild)?;
        }
    }
    Ok(mapping)
}

fn convert_guid_value(
    fields: &[SerializedFieldValue],
    schema: &UnityYamlGuidSchema,
    field_path: &str,
    depth: usize,
    budget: &mut ConversionBudget,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    if fields.len() != schema.word_field_names.len() {
        return Err(UnityYamlObjectConversionError::GuidFieldCount {
            field_path: field_path.to_owned(),
            expected: schema.word_field_names.len(),
            actual: fields.len(),
        });
    }

    let mut words = [0u32; 4];
    for (word_index, field_name) in schema.word_field_names.iter().enumerate() {
        budget.visit(depth + 1)?;
        let mut matching_value = None;
        for field in fields.iter().filter(|field| field.name == *field_name) {
            if matching_value.is_some() {
                return Err(UnityYamlObjectConversionError::InvalidGuidWord {
                    field_path: field_path.to_owned(),
                    field_name: field_name.clone(),
                    word_index,
                });
            }
            matching_value = Some(&field.value);
        }
        let Some(SerializedValue::U32(value)) = matching_value else {
            return Err(UnityYamlObjectConversionError::InvalidGuidWord {
                field_path: field_path.to_owned(),
                field_name: field_name.clone(),
                word_index,
            });
        };
        words[word_index] = *value;
    }

    let mut serialized_guid = [0u8; 16];
    for (word_index, word) in words.into_iter().enumerate() {
        let start = word_index * 4;
        serialized_guid[start..start + 4].copy_from_slice(&word.to_le_bytes());
    }
    Ok(UnityYamlNode::string(format_unity_guid_words(
        serialized_guid,
    )))
}

fn convert_property_name_value(
    fields: &[SerializedFieldValue],
    schema: &UnityYamlPropertyNameSchema,
    field_path: &str,
    depth: usize,
    budget: &mut ConversionBudget,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    if fields.len() != 1 {
        return Err(UnityYamlObjectConversionError::PropertyNameFieldCount {
            field_path: field_path.to_owned(),
            expected: 1,
            actual: fields.len(),
        });
    }

    budget.visit(depth + 1)?;
    let mut matching_value = None;
    for field in fields
        .iter()
        .filter(|field| field.name == schema.value_field_name)
    {
        if matching_value.is_some() {
            return Err(UnityYamlObjectConversionError::InvalidPropertyNameValue {
                field_path: field_path.to_owned(),
                field_name: schema.value_field_name.clone(),
            });
        }
        matching_value = Some(&field.value);
    }

    match (schema.value_kind, matching_value) {
        (UnityYamlPropertyNameValueKind::Int32, Some(SerializedValue::I32(value))) => {
            Ok(UnityYamlNode::string(value.to_string()))
        }
        (UnityYamlPropertyNameValueKind::Utf8String, Some(SerializedValue::StringBytes(bytes))) => {
            let value = str::from_utf8(bytes).map_err(|_| {
                UnityYamlObjectConversionError::InvalidUtf8String {
                    field_path: field_path.to_owned(),
                }
            })?;
            Ok(UnityYamlNode::string(try_copy_string(
                value,
                "Unity PropertyName string",
            )?))
        }
        _ => Err(UnityYamlObjectConversionError::InvalidPropertyNameValue {
            field_path: field_path.to_owned(),
            field_name: schema.value_field_name.clone(),
        }),
    }
}

fn convert_scene_object_identifier_value(
    fields: &[SerializedFieldValue],
    schema: &UnityYamlSceneObjectIdentifierSchema,
    field_path: &str,
    depth: usize,
    budget: &mut ConversionBudget,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    let target_object = scene_object_identifier_file_id(
        fields,
        &schema.target_object_field_name,
        field_path,
        depth,
        budget,
    )?;
    let target_prefab = scene_object_identifier_file_id(
        fields,
        &schema.target_prefab_field_name,
        field_path,
        depth,
        budget,
    )?;
    let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    mapping
        .insert_field(
            "targetObject",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(target_object)),
        )
        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    mapping
        .insert_field(
            "targetPrefab",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(target_prefab)),
        )
        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    Ok(mapping)
}

fn scene_object_identifier_file_id(
    fields: &[SerializedFieldValue],
    field_name: &str,
    field_path: &str,
    depth: usize,
    budget: &mut ConversionBudget,
) -> Result<i64, UnityYamlObjectConversionError> {
    let mut matching_value = None;
    for field in fields.iter().filter(|field| field.name == field_name) {
        if matching_value.is_some() {
            return Err(
                UnityYamlObjectConversionError::InvalidSceneObjectIdentifierValue {
                    field_path: field_path.to_owned(),
                    field_name: field_name.to_owned(),
                },
            );
        }
        matching_value = Some(&field.value);
    }
    budget.visit(depth + 1)?;
    match matching_value {
        Some(SerializedValue::I64(value)) => Ok(*value),
        _ => Err(
            UnityYamlObjectConversionError::InvalidSceneObjectIdentifierValue {
                field_path: field_path.to_owned(),
                field_name: field_name.to_owned(),
            },
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn convert_array<F>(
    items: &[SerializedValue],
    field_path: &str,
    depth: usize,
    source: ExportAssetIdentity,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    unity_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
    resolve_pointer: &mut F,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    if !force_block_sequence_for_path(field_path, hints, limits)? {
        if let Some(representation) = dictionary_representation_for_path(field_path, hints, limits)?
        {
            return convert_dictionary(
                items,
                field_path,
                depth,
                representation,
                source,
                hints,
                field_mode,
                unity_version,
                missing_reference_guid,
                limits,
                budget,
                resolve_pointer,
            );
        }
    }

    if items.is_empty() {
        let kind = hints
            .empty_array_kinds
            .get(field_path)
            .copied()
            .ok_or_else(|| UnityYamlObjectConversionError::MissingEmptyArrayHint {
                field_path: field_path.to_owned(),
            })?;
        return match kind {
            UnityYamlEmptyArrayKind::IntegerHex => Ok(UnityYamlNode::plain_string(String::new())),
            UnityYamlEmptyArrayKind::BlockSequence => {
                Ok(UnityYamlNode::sequence(UnityYamlSequenceStyle::Block))
            }
            UnityYamlEmptyArrayKind::BlockCurveSequence => {
                Ok(UnityYamlNode::sequence(UnityYamlSequenceStyle::BlockCurve))
            }
            UnityYamlEmptyArrayKind::FlowSequence => {
                Ok(UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow))
            }
        };
    }

    if let Some(kind) = hints.empty_array_kinds.get(field_path) {
        if *kind == UnityYamlEmptyArrayKind::IntegerHex {
            return convert_integer_array(items, field_path, depth, limits, budget);
        }
    } else if integer_array_width(&items[0]).is_some() {
        return convert_integer_array(items, field_path, depth, limits, budget);
    }

    let style = match hints.empty_array_kinds.get(field_path) {
        Some(UnityYamlEmptyArrayKind::BlockCurveSequence) => UnityYamlSequenceStyle::BlockCurve,
        Some(UnityYamlEmptyArrayKind::FlowSequence) => UnityYamlSequenceStyle::Flow,
        _ => UnityYamlSequenceStyle::Block,
    };
    let mut sequence = UnityYamlNode::sequence(style);
    for (index, item) in items.iter().enumerate() {
        let element_path = indexed_field_path(field_path, index, limits.max_field_path_bytes)?;
        sequence
            .push(convert_value(
                item,
                &element_path,
                depth + 1,
                source,
                hints,
                field_mode,
                unity_version,
                missing_reference_guid,
                limits,
                budget,
                resolve_pointer,
            )?)
            .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    }
    Ok(sequence)
}

#[allow(clippy::too_many_arguments)]
fn convert_dictionary<F>(
    items: &[SerializedValue],
    field_path: &str,
    depth: usize,
    representation: UnityYamlDictionaryRepresentation,
    source: ExportAssetIdentity,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    unity_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
    resolve_pointer: &mut F,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    let mut output = match representation {
        UnityYamlDictionaryRepresentation::KeyedMapping
        | UnityYamlDictionaryRepresentation::LegacyPairMapping => {
            UnityYamlNode::mapping(UnityYamlMappingStyle::Block)
        }
        UnityYamlDictionaryRepresentation::PairSequence => {
            UnityYamlNode::sequence(UnityYamlSequenceStyle::BlockCurve)
        }
    };

    for (element_index, item) in items.iter().enumerate() {
        let element_path =
            indexed_field_path(field_path, element_index, limits.max_field_path_bytes)?;
        let SerializedValue::Struct(pair_fields) = item else {
            return Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                field_path: field_path.to_owned(),
                element_index,
            });
        };
        let mut first = None;
        let mut second = None;
        for pair_field in pair_fields {
            match pair_field.name.as_str() {
                "first" if first.is_none() => first = Some(&pair_field.value),
                "second" if second.is_none() => second = Some(&pair_field.value),
                "first" | "second" => {
                    return Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                        field_path: field_path.to_owned(),
                        element_index,
                    });
                }
                _ => {}
            }
        }
        if pair_fields.len() != 2 || first.is_none() || second.is_none() {
            return Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                field_path: field_path.to_owned(),
                element_index,
            });
        }
        budget.visit(depth + 1)?;
        let Some(first) = first else {
            return Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                field_path: field_path.to_owned(),
                element_index,
            });
        };
        let Some(second) = second else {
            return Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                field_path: field_path.to_owned(),
                element_index,
            });
        };

        let first_path = child_field_path(&element_path, "first", limits.max_field_path_bytes)?;
        let second_path = child_field_path(&element_path, "second", limits.max_field_path_bytes)?;
        let first_node = if representation == UnityYamlDictionaryRepresentation::KeyedMapping {
            if let Some(key) = primitive_dictionary_key(first, field_path, element_index)? {
                budget.visit(depth + 2)?;
                key
            } else {
                convert_value(
                    first,
                    &first_path,
                    depth + 2,
                    source,
                    hints,
                    field_mode,
                    unity_version,
                    missing_reference_guid,
                    limits,
                    budget,
                    resolve_pointer,
                )?
            }
        } else {
            convert_value(
                first,
                &first_path,
                depth + 2,
                source,
                hints,
                field_mode,
                unity_version,
                missing_reference_guid,
                limits,
                budget,
                resolve_pointer,
            )?
        };
        let second_node = convert_value(
            second,
            &second_path,
            depth + 2,
            source,
            hints,
            field_mode,
            unity_version,
            missing_reference_guid,
            limits,
            budget,
            resolve_pointer,
        )?;

        match representation {
            UnityYamlDictionaryRepresentation::KeyedMapping => {
                if !matches!(&first_node.kind, UnityYamlNodeKind::Scalar(_)) {
                    return Err(UnityYamlObjectConversionError::InvalidDictionaryKey {
                        field_path: field_path.to_owned(),
                        element_index,
                    });
                }
                output
                    .insert(first_node, second_node)
                    .map_err(UnityYamlObjectConversionError::NodeBuild)?;
            }
            UnityYamlDictionaryRepresentation::LegacyPairMapping
            | UnityYamlDictionaryRepresentation::PairSequence => {
                let mut pair = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
                pair.insert_field("first", first_node)
                    .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                pair.insert_field("second", second_node)
                    .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                if representation == UnityYamlDictionaryRepresentation::LegacyPairMapping {
                    output
                        .insert_field("data", pair)
                        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                } else {
                    output
                        .push(pair)
                        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                }
            }
        }
    }

    Ok(output)
}

fn primitive_dictionary_key(
    value: &SerializedValue,
    field_path: &str,
    element_index: usize,
) -> Result<Option<UnityYamlNode>, UnityYamlObjectConversionError> {
    let key = match value {
        SerializedValue::Bool(value) => Some(try_copy_string(
            if *value { "True" } else { "False" },
            "Unity dictionary boolean key",
        )?),
        SerializedValue::Char(value) => {
            let Some(character) = char::from_u32(u32::from(*value)) else {
                return Err(UnityYamlObjectConversionError::InvalidDictionaryKey {
                    field_path: field_path.to_owned(),
                    element_index,
                });
            };
            let mut key = String::new();
            key.try_reserve_exact(character.len_utf8()).map_err(|_| {
                UnityYamlObjectConversionError::AllocationFailed {
                    resource: "Unity dictionary character key",
                    requested: character.len_utf8(),
                }
            })?;
            key.push(character);
            Some(key)
        }
        SerializedValue::I8(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::U8(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::I16(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::U16(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::I32(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::U32(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::I64(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::U64(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::F32(value) => Some(format_dictionary_float32_key(*value)?),
        SerializedValue::F64(value) => Some(format_dictionary_float64_key(*value)?),
        _ => None,
    };
    Ok(key.map(UnityYamlNode::string))
}

fn format_dictionary_numeric_key<T: Display>(
    value: T,
    field_path: &str,
    element_index: usize,
) -> Result<String, UnityYamlObjectConversionError> {
    let mut key = String::new();
    key.try_reserve_exact(64)
        .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity dictionary numeric key",
            requested: 64,
        })?;
    write!(&mut key, "{value}").map_err(|_| {
        UnityYamlObjectConversionError::InvalidDictionaryKey {
            field_path: field_path.to_owned(),
            element_index,
        }
    })?;
    Ok(key)
}

/// Render a `float` dictionary key exactly like the source `ToString`: the same
/// invariant-culture shortest form the YAML value scalars use, not Rust `Display`
/// (which disagrees on fixed/scientific notation choices and exponent padding).
fn format_dictionary_float32_key(value: f32) -> Result<String, UnityYamlObjectConversionError> {
    if value.is_nan() {
        try_copy_string("NaN", "Unity dictionary floating-point key")
    } else if value == f32::INFINITY {
        try_copy_string("Infinity", "Unity dictionary floating-point key")
    } else if value == f32::NEG_INFINITY {
        try_copy_string("-Infinity", "Unity dictionary floating-point key")
    } else {
        try_copy_string(
            &format_dotnet_f32(value),
            "Unity dictionary floating-point key",
        )
    }
}

/// Render a `double` dictionary key exactly like the source `ToString`.
fn format_dictionary_float64_key(value: f64) -> Result<String, UnityYamlObjectConversionError> {
    if value.is_nan() {
        try_copy_string("NaN", "Unity dictionary floating-point key")
    } else if value == f64::INFINITY {
        try_copy_string("Infinity", "Unity dictionary floating-point key")
    } else if value == f64::NEG_INFINITY {
        try_copy_string("-Infinity", "Unity dictionary floating-point key")
    } else {
        try_copy_string(
            &format_dotnet_f64(value),
            "Unity dictionary floating-point key",
        )
    }
}

fn importer_field_is_excluded(
    field_name: &str,
    field_path: &str,
    hints: &UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<bool, UnityYamlObjectConversionError> {
    if IMPORTER_FIELDS_TO_SKIP.contains(&field_name)
        || hints.importer_excluded_field_paths.contains(field_path)
    {
        return Ok(true);
    }
    if field_path.contains('[') {
        let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
        return Ok(hints
            .importer_excluded_field_paths
            .contains(&canonical_path));
    }
    Ok(false)
}

fn baseline_type_scoped_field_aliases(type_name: &str) -> &'static [(&'static str, &'static str)] {
    const ANIMATION_CLIP_ALIASES: &[(&str, &str)] =
        &[("m_AnimationClipSettings", "m_MuscleClipInfo")];
    const GENERIC_BINDING_ALIASES: &[(&str, &str)] = &[("m_TypeID", "m_ClassID")];
    const ANIMATOR_CONTROLLER_ALIASES: &[(&str, &str)] = &[
        ("m_AnimatorEvents", "m_AnimatorParameters"),
        ("m_Layers", "m_AnimatorLayers"),
    ];
    const ANIMATOR_CONTROLLER_LAYER_ALIASES: &[(&str, &str)] = &[("m_HumanMask", "m_Mask")];
    const BLEND_TREE_ALIASES: &[(&str, &str)] = &[
        ("m_BlendEvent", "m_BlendParameter"),
        ("m_BlendEventY", "m_BlendParameterY"),
    ];
    const LIGHTMAP_SETTINGS_ALIASES: &[(&str, &str)] =
        &[("m_LightmapSnapshot", "m_LightingDataAsset")];
    const TEXTURE_3D_ALIASES: &[(&str, &str)] = &[
        ("m_DataSize", "m_CompleteImageSize"),
        ("m_ImageCount", "m_Depth"),
    ];
    const MESH_RENDERER_ALIASES: &[(&str, &str)] = &[("m_StitchSeams", "m_StitchLightmapSeams")];
    const PREFAB_ALIASES: &[(&str, &str)] = &[
        ("m_IsPrefabParent", "m_IsPrefabAsset"),
        ("m_ParentPrefab", "m_SourcePrefab"),
    ];
    const LAYER_CONSTANT_ALIASES: &[(&str, &str)] = &[(
        "m_StateMachineMotionSetIndex",
        "m_StateMachineSynchronizedLayerIndex",
    )];
    const CONTROLLER_CONSTANT_ALIASES: &[(&str, &str)] = &[("m_HumanLayerArray", "m_LayerArray")];
    const STATE_MACHINE_CONSTANT_ALIASES: &[(&str, &str)] =
        &[("m_MotionSetCount", "m_SynchronizedLayerCount")];
    const BUILD_SETTINGS_ALIASES: &[(&str, &str)] = &[("m_Levels", "m_Scenes")];
    const HEIGHTMAP_ALIASES: &[(&str, &str)] = &[
        (
            "m_EnableSurfaceMaskTextureCompression",
            "m_EnableHolesTextureCompression",
        ),
        ("m_SurfaceMask", "m_Holes"),
        ("m_SurfaceMaskLOD", "m_HolesLOD"),
    ];
    const GRAPHICS_SETTINGS_ALIASES: &[(&str, &str)] = &[
        ("m_BuildTargetShaderSettings", "m_TierSettings"),
        ("m_AlbedoSwatchInfos", "m_AlbedoSwitchInfos"),
    ];
    const TEXTURE_IMPORTER_ALIASES: &[(&str, &str)] =
        &[("m_BuildTargetSettings", "m_PlatformSettings")];
    const BUILD_TARGET_SHADER_SETTINGS_ALIASES: &[(&str, &str)] =
        &[("m_ShaderSettings", "m_Settings")];
    const BINDING_ALIASES: &[(&str, &str)] = &[("m_Set", "m_EncodedData")];
    const CONSTANT_BUFFER_PARAMETER_ALIASES: &[(&str, &str)] =
        &[("m_Index", "m_OffsetInConstantBuffer")];
    const VERTEX_DATA_ALIASES: &[(&str, &str)] = &[("m_DataSize", "m_Data")];
    const COMPUTE_SHADER_VARIANT_ALIASES: &[(&str, &str)] = &[("m_Kernels", "m_KernelParents")];
    const SHADER_ALIASES: &[(&str, &str)] = &[("m_SubProgramBlob", "m_CompressedBlob")];
    const GAME_OBJECT_ALIASES: &[(&str, &str)] = &[("m_Component", "m_Components")];

    match type_name {
        "AnimationClip" => ANIMATION_CLIP_ALIASES,
        "GenericBinding" => GENERIC_BINDING_ALIASES,
        "AnimatorController" => ANIMATOR_CONTROLLER_ALIASES,
        "AnimatorControllerLayer" => ANIMATOR_CONTROLLER_LAYER_ALIASES,
        "BlendTree" => BLEND_TREE_ALIASES,
        "LightmapSettings" => LIGHTMAP_SETTINGS_ALIASES,
        "Texture3D" => TEXTURE_3D_ALIASES,
        "MeshRenderer" => MESH_RENDERER_ALIASES,
        "Prefab" => PREFAB_ALIASES,
        "HumanLayerConstant" | "LayerConstant" => LAYER_CONSTANT_ALIASES,
        "ControllerConstant" => CONTROLLER_CONSTANT_ALIASES,
        "StateMachineConstant" => STATE_MACHINE_CONSTANT_ALIASES,
        "BuildSettings" => BUILD_SETTINGS_ALIASES,
        "Heightmap" => HEIGHTMAP_ALIASES,
        "GraphicsSettings" => GRAPHICS_SETTINGS_ALIASES,
        "TextureImporter" => TEXTURE_IMPORTER_ALIASES,
        "BuildTargetShaderSettings" => BUILD_TARGET_SHADER_SETTINGS_ALIASES,
        "Binding" => BINDING_ALIASES,
        "MatrixParameter" | "StructParameter" | "VectorParameter" => {
            CONSTANT_BUFFER_PARAMETER_ALIASES
        }
        "VertexData" => VERTEX_DATA_ALIASES,
        "ComputeShaderPlatformVariant" => COMPUTE_SHADER_VARIANT_ALIASES,
        "Shader" => SHADER_ALIASES,
        "GameObject" => GAME_OBJECT_ALIASES,
        _ => &[],
    }
}

fn has_conditional_field_aliases(type_name: &str) -> bool {
    matches!(
        type_name,
        "ParticleSystem" | "ShapeModule" | "ClipMuscleConstant" | "Xform" | "ValueArray" | "Mesh"
    )
}

fn baseline_type_scoped_field_alias(
    parent_type: &str,
    field_name: &str,
    field_type: &str,
) -> Option<&'static str> {
    match (parent_type, field_name) {
        ("ClipMuscleConstant", "m_AverageSpeed") => match field_type {
            "Vector4Float" | "float4" => Some("m_AverageSpeed4"),
            "Vector3Float" | "float3" => Some("m_AverageSpeed3"),
            _ => None,
        },
        ("Xform", "m_T") => match field_type {
            "Vector4Float" | "float4" => Some("m_T4"),
            "Vector3Float" | "float3" => Some("m_T3"),
            _ => None,
        },
        ("Xform", "m_S") => match field_type {
            "Vector4Float" | "float4" => Some("m_S4"),
            "Vector3Float" | "float3" => Some("m_S3"),
            _ => None,
        },
        ("ParticleSystem", "m_StartDelay") => Some(if field_type == "float" {
            "m_StartDelay_Single"
        } else {
            "m_StartDelay_MinMaxCurve"
        }),
        ("ShapeModule", "m_Arc") if field_type != "float" => Some("m_ArcParameter"),
        ("ShapeModule", "m_Radius") if field_type != "float" => Some("m_RadiusParameter"),
        ("Mesh", "m_Shapes") if field_type == "vector" => Some("m_ShapesList"),
        _ => baseline_type_scoped_field_aliases(parent_type)
            .iter()
            .find(|(source_name, _)| *source_name == field_name)
            .map(|(_, alias)| *alias),
    }
}

fn unsupported_contextual_field_alias_reason(
    parent_type: &str,
    field_name: &str,
    field_type: &str,
) -> Option<&'static str> {
    let recognized_type = match (parent_type, field_name) {
        ("ClipMuscleConstant", "m_AverageSpeed") => {
            matches!(
                field_type,
                "Vector4Float" | "Vector3Float" | "float4" | "float3"
            )
        }
        ("Xform", "m_T" | "m_S") => {
            matches!(
                field_type,
                "Vector4Float" | "Vector3Float" | "float4" | "float3"
            )
        }
        _ => true,
    };
    (!recognized_type).then_some("unsupported child TypeTree type for baseline field alias")
}

fn baseline_yaml_field_name(field_name: &str) -> Result<String, UnityYamlObjectConversionError> {
    let replacement = match field_name {
        "m_Image_data" => Some("m_ImageData"),
        "m_TextureFormat" => Some("m_Format"),
        "m_TextureDimension" => Some("m_Dimension"),
        "m_ObjectHideFlags" => Some("m_HideFlags"),
        "m_Val" => Some("m_Value"),
        _ => None,
    };
    if let Some(replacement) = replacement {
        return try_copy_string(replacement, "Unity baseline YAML field name");
    }

    for (source_prefix, output_prefix) in [
        ("m_Dst", "m_Destination"),
        ("m_Dest", "m_Destination"),
        ("m_Src", "m_Source"),
    ] {
        let Some(suffix) = field_name.strip_prefix(source_prefix) else {
            continue;
        };
        if !baseline_prefix_has_uppercase_initial(suffix) {
            continue;
        }
        let requested = output_prefix.len().checked_add(suffix.len()).ok_or(
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "Unity baseline YAML field name",
                requested: usize::MAX,
            },
        )?;
        let mut output_name = String::new();
        output_name.try_reserve_exact(requested).map_err(|_| {
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "Unity baseline YAML field name",
                requested,
            }
        })?;
        output_name.push_str(output_prefix);
        output_name.push_str(suffix);
        return Ok(output_name);
    }

    try_copy_string(field_name, "Unity baseline YAML field name")
}

fn baseline_prefix_has_uppercase_initial(suffix: &str) -> bool {
    suffix
        .encode_utf16()
        .next()
        .and_then(|unit| char::from_u32(u32::from(unit)))
        .is_some_and(char::is_uppercase)
}

fn stripped_field_allowlist(class_id: i32, stripped: bool) -> Option<&'static [&'static str]> {
    stripped.then_some(stripped_fields_for_class_id(class_id))
}

fn stripped_fields_for_class_id(class_id: i32) -> &'static [&'static str] {
    if class_id == 114 {
        STRIPPED_MONO_BEHAVIOUR_ALLOWED_FIELDS
    } else {
        STRIPPED_ASSET_ALLOWED_FIELDS
    }
}

fn retain_stripped_yaml_fields(
    body: &mut UnityYamlNode,
    class_id: i32,
) -> Result<(), UnityYamlObjectConversionError> {
    let allowed_fields = stripped_fields_for_class_id(class_id);
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut body.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    entries.retain(|(key, _)| match &key.kind {
        UnityYamlNodeKind::Scalar(
            UnityYamlScalar::String(name) | UnityYamlScalar::PlainString(name),
        ) => allowed_fields.contains(&name.as_str()),
        _ => false,
    });
    Ok(())
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

fn convert_integer_array(
    items: &[SerializedValue],
    field_path: &str,
    depth: usize,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    let width = integer_array_width(&items[0]).ok_or_else(|| {
        UnityYamlObjectConversionError::InvalidIntegerArray {
            field_path: field_path.to_owned(),
            element_index: 0,
        }
    })?;
    let hex_length = items
        .len()
        .checked_mul(width)
        .and_then(|bytes| bytes.checked_mul(2))
        .ok_or_else(|| UnityYamlObjectConversionError::HexScalarLimitExceeded {
            requested: usize::MAX,
            limit: limits.max_generated_scalar_bytes,
            field_path: field_path.to_owned(),
        })?;
    if hex_length > limits.max_generated_scalar_bytes {
        return Err(UnityYamlObjectConversionError::HexScalarLimitExceeded {
            requested: hex_length,
            limit: limits.max_generated_scalar_bytes,
            field_path: field_path.to_owned(),
        });
    }
    for (index, value) in items.iter().enumerate() {
        if integer_array_width(value) != Some(width) {
            return Err(UnityYamlObjectConversionError::InvalidIntegerArray {
                field_path: field_path.to_owned(),
                element_index: index,
            });
        }
        let element_path = indexed_field_path(field_path, index, limits.max_field_path_bytes)?;
        budget.visit(depth + 1)?;
        budget.check_field_path(&element_path)?;
    }

    let mut hex = String::new();
    hex.try_reserve_exact(hex_length).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity integer-list hex scalar",
            requested: hex_length,
        }
    })?;
    for value in items {
        let raw = integer_array_raw(value).ok_or_else(|| {
            UnityYamlObjectConversionError::InvalidIntegerArray {
                field_path: field_path.to_owned(),
                element_index: 0,
            }
        })?;
        for byte_index in 0..width {
            let byte = ((raw >> (byte_index * 8)) & 0xff) as u8;
            hex.push(hex_digit(byte >> 4));
            hex.push(hex_digit(byte & 0x0f));
        }
    }
    Ok(UnityYamlNode::plain_string(hex))
}

fn integer_array_width(value: &SerializedValue) -> Option<usize> {
    match value {
        SerializedValue::Bool(_) | SerializedValue::I8(_) | SerializedValue::U8(_) => Some(1),
        SerializedValue::I16(_) | SerializedValue::U16(_) | SerializedValue::Char(_) => Some(2),
        SerializedValue::I32(_) | SerializedValue::U32(_) => Some(4),
        SerializedValue::I64(_) | SerializedValue::U64(_) => Some(8),
        _ => None,
    }
}

fn integer_array_raw(value: &SerializedValue) -> Option<u64> {
    match value {
        SerializedValue::Bool(value) => Some(u64::from(*value)),
        SerializedValue::I8(value) => Some(u64::from(*value as u8)),
        SerializedValue::U8(value) => Some(u64::from(*value)),
        SerializedValue::I16(value) => Some(u64::from(*value as u16)),
        SerializedValue::U16(value) | SerializedValue::Char(value) => Some(u64::from(*value)),
        SerializedValue::I32(value) => Some(u64::from(*value as u32)),
        SerializedValue::U32(value) => Some(u64::from(*value)),
        SerializedValue::I64(value) => Some(*value as u64),
        SerializedValue::U64(value) => Some(*value),
        _ => None,
    }
}

fn encode_byte_hex(
    bytes: &[u8],
    requested: usize,
    field_path: &str,
    limits: UnityYamlObjectConversionLimits,
) -> Result<String, UnityYamlObjectConversionError> {
    if requested > limits.max_generated_scalar_bytes {
        return Err(UnityYamlObjectConversionError::HexScalarLimitExceeded {
            requested,
            limit: limits.max_generated_scalar_bytes,
            field_path: field_path.to_owned(),
        });
    }
    let mut hex = String::new();
    hex.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity byte-array hex scalar",
            requested,
        }
    })?;
    for &byte in bytes {
        hex.push(hex_digit(byte >> 4));
        hex.push(hex_digit(byte & 0x0f));
    }
    Ok(hex)
}

const fn hex_digit(nibble: u8) -> char {
    if nibble < 10 {
        (b'0' + nibble) as char
    } else {
        (b'a' + (nibble - 10)) as char
    }
}

fn is_string_type_tree_node(
    layout: &UnityYamlTypeTreeLayout<'_>,
    node_index: usize,
) -> Result<bool, UnityYamlObjectConversionError> {
    let children = layout.child_indices(node_index)?;
    if children.len() != 1 {
        return Ok(false);
    }
    let Some(data_node) = layout.array_shape_data_node(children[0])? else {
        return Ok(false);
    };
    Ok(matches!(layout.type_name(data_node)?, "char" | "UInt8")
        && layout.first_child[data_node].is_none())
}

fn is_integer_type_tree_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "bool"
            | "char"
            | "SInt8"
            | "UInt8"
            | "SInt16"
            | "UInt16"
            | "SInt32"
            | "UInt32"
            | "SInt64"
            | "UInt64"
            | "FileSize"
            | "signed char"
            | "unsigned char"
            | "short"
            | "unsigned short"
            | "int"
            | "unsigned int"
            | "long long"
            | "unsigned long long"
            | "Type*"
    )
}

fn is_signed_file_id_type_tree_type(type_name: &str) -> bool {
    matches!(type_name, "SInt64" | "long long")
}

fn is_valid_dictionary_key_type_tree_type(type_name: &str) -> bool {
    type_name == "GUID"
        || matches!(type_name, "string" | "Utf8String" | "float" | "double")
        || is_integer_type_tree_type(type_name)
}

fn is_unsigned_byte_type_tree_type(type_name: &str) -> bool {
    matches!(type_name, "UInt8" | "byte" | "unsigned char")
}

fn is_unsigned_word_type_tree_type(type_name: &str) -> bool {
    matches!(type_name, "UInt32" | "uint" | "unsigned int")
}

fn wildcard_element_path(
    parent: &str,
    limits: UnityYamlObjectConversionLimits,
) -> Result<String, UnityYamlObjectConversionError> {
    let requested = parent.len().checked_add(3).ok_or(
        UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested: usize::MAX,
            limit: limits.max_field_path_bytes,
        },
    )?;
    if requested > limits.max_field_path_bytes {
        return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested,
            limit: limits.max_field_path_bytes,
        });
    }
    let mut path = String::new();
    path.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML array schema path",
            requested,
        }
    })?;
    path.push_str(parent);
    path.push_str("[*]");
    Ok(path)
}

fn is_flow_mapped_path(
    path: &str,
    hints: &UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<bool, UnityYamlObjectConversionError> {
    if hints.flow_mapped_struct_paths.contains(path) {
        return Ok(true);
    }
    if !path.contains('[') {
        return Ok(false);
    }
    let canonical_path = canonical_array_path(path, limits.max_field_path_bytes)?;
    Ok(hints.flow_mapped_struct_paths.contains(&canonical_path))
}
