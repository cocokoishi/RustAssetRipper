use super::*;

#[derive(Clone, Copy)]
pub(super) enum FontDataView<'data> {
    Bytes(&'data [u8]),
    ByteArray(&'data [SerializedValue]),
}

impl FontDataView<'_> {
    pub(super) fn len(self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            Self::ByteArray(values) => values.len(),
        }
    }

    pub(super) fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub(super) fn extension(self) -> Result<&'static str, Box<dyn Error>> {
        match self {
            Self::Bytes(bytes) => Ok(super::super::resource_extensions::font_extension(bytes)),
            Self::ByteArray(values) => {
                let prefix_length = values.len().min(4);
                let mut prefix = [0u8; 4];
                for (index, value) in values.iter().take(prefix_length).enumerate() {
                    let SerializedValue::U8(value) = value else {
                        return Err(invalid_project(format!(
                            "Font m_FontData[{index}] is not a byte"
                        )));
                    };
                    prefix[index] = *value;
                }
                Ok(super::super::resource_extensions::font_extension(
                    &prefix[..prefix_length],
                ))
            }
        }
    }

    pub(super) fn to_vec(self) -> Result<Vec<u8>, Box<dyn Error>> {
        match self {
            Self::Bytes(bytes) => {
                let mut output = Vec::new();
                output
                    .try_reserve_exact(bytes.len())
                    .map_err(|_| invalid_project("could not allocate Font export bytes"))?;
                output.extend_from_slice(bytes);
                Ok(output)
            }
            Self::ByteArray(values) => {
                let mut output = Vec::new();
                output
                    .try_reserve_exact(values.len())
                    .map_err(|_| invalid_project("could not allocate Font export bytes"))?;
                for (index, value) in values.iter().enumerate() {
                    let SerializedValue::U8(value) = value else {
                        return Err(invalid_project(format!(
                            "Font m_FontData[{index}] is not a byte"
                        )));
                    };
                    output.push(*value);
                }
                Ok(output)
            }
        }
    }
}

pub(super) struct FontImporterValues {
    pub(super) font_size: i32,
    pub(super) font_names: Vec<String>,
    pub(super) fallback_fonts: Vec<(i32, i64)>,
    pub(super) character_spacing: i32,
    pub(super) character_padding: i32,
    pub(super) font_rendering_mode: i32,
    pub(super) use_legacy_bounds_calculation: bool,
    pub(super) should_round_advance_value: bool,
    pub(super) asset_bundle_name: Option<String>,
}

pub(super) fn font_asset_data(
    root: &SerializedValue,
) -> Result<Option<FontDataView<'_>>, Box<dyn Error>> {
    let value = root
        .unique_field_in_base_chain("m_FontData")
        .map_err(|error| invalid_project(format!("Font m_FontData lookup failed: {error}")))?;
    match value {
        Some(SerializedValue::Bytes(bytes)) => Ok(Some(FontDataView::Bytes(bytes))),
        Some(SerializedValue::Array(values)) => Ok(Some(FontDataView::ByteArray(values))),
        Some(_) => Err(invalid_project(
            "Font m_FontData is neither bytes nor an array of bytes",
        )),
        None => Ok(None),
    }
}

pub(super) fn font_importer_values(
    payload: &ParsedObjectPayload,
) -> Result<FontImporterValues, Box<dyn Error>> {
    let root = &payload.root;
    let font_size = match required_font_field(root, "m_FontSize")? {
        SerializedValue::F32(value) => *value as i32,
        value => font_integer(value, "m_FontSize")?,
    };
    let font_names = match optional_font_field(root, "m_FontNames")? {
        None => Vec::new(),
        Some(SerializedValue::Array(values)) => {
            let mut names = Vec::new();
            names
                .try_reserve_exact(values.len())
                .map_err(|_| invalid_project("could not allocate Font m_FontNames"))?;
            for (index, value) in values.iter().enumerate() {
                let SerializedValue::StringBytes(bytes) = value else {
                    return Err(invalid_project(format!(
                        "Font m_FontNames[{index}] is not a serialized string"
                    )));
                };
                let name = std::str::from_utf8(bytes).map_err(|error| {
                    invalid_project(format!(
                        "Font m_FontNames[{index}] is not valid UTF-8: {error}"
                    ))
                })?;
                names.push(name.to_owned());
            }
            names
        }
        Some(other) => {
            return Err(invalid_project(format!(
                "Font m_FontNames is not an array: {other:?}"
            )))
        }
    };
    let fallback_fonts = match optional_font_field(root, "m_FallbackFonts")? {
        None => Vec::new(),
        Some(SerializedValue::Array(values)) => {
            let mut fonts = Vec::new();
            fonts
                .try_reserve_exact(values.len())
                .map_err(|_| invalid_project("could not allocate Font m_FallbackFonts"))?;
            for (index, value) in values.iter().enumerate() {
                let SerializedValue::PPtr { file_id, path_id } = value else {
                    return Err(invalid_project(format!(
                        "Font m_FallbackFonts[{index}] is not a PPtr: {value:?}"
                    )));
                };
                fonts.push((*file_id, *path_id));
            }
            fonts
        }
        Some(other) => {
            return Err(invalid_project(format!(
                "Font m_FallbackFonts is not an array: {other:?}"
            )))
        }
    };
    Ok(FontImporterValues {
        font_size,
        font_names,
        fallback_fonts,
        character_spacing: font_integer(
            required_font_field(root, "m_CharacterSpacing")?,
            "m_CharacterSpacing",
        )?,
        character_padding: font_integer(
            required_font_field(root, "m_CharacterPadding")?,
            "m_CharacterPadding",
        )?,
        font_rendering_mode: font_integer(
            required_font_field(root, "m_FontRenderingMode")?,
            "m_FontRenderingMode",
        )?,
        use_legacy_bounds_calculation: font_bool(
            required_font_field(root, "m_UseLegacyBoundsCalculation")?,
            "m_UseLegacyBoundsCalculation",
        )?,
        should_round_advance_value: font_bool(
            required_font_field(root, "m_ShouldRoundAdvanceValue")?,
            "m_ShouldRoundAdvanceValue",
        )?,
        asset_bundle_name: object_asset_bundle_name(payload),
    })
}

pub(super) fn font_importer_yaml(
    values: &FontImporterValues,
    fallback_references: UnityYamlNode,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    use crate::export::unity_yaml::UnityYamlScalar;

    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    importer.insert_field(
        "serializedVersion",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(4)),
    )?;
    importer.insert_field(
        "externalObjects",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field(
        "fontSize",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(values.font_size))),
    )?;
    importer.insert_field(
        "forceTextureCase",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(-2)),
    )?;
    importer.insert_field(
        "characterSpacing",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(values.character_spacing))),
    )?;
    importer.insert_field(
        "characterPadding",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(values.character_padding))),
    )?;
    importer.insert_field(
        "includeFontData",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(1)),
    )?;
    let mut font_names = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for name in &values.font_names {
        font_names.push(UnityYamlNode::string(name.clone()))?;
    }
    importer.insert_field("fontNames", font_names)?;
    importer.insert_field("fallbackFontReferences", fallback_references)?;
    importer.insert_field("customCharacters", UnityYamlNode::string(""))?;
    importer.insert_field(
        "fontRenderingMode",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(
            values.font_rendering_mode,
        ))),
    )?;
    let mut output = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    output.insert_field(
        "hasEmptyFontData",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
    )?;
    importer.insert_field("output", output)?;
    importer.insert_field(
        "ascentCalculationMode",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(1)),
    )?;
    importer.insert_field(
        "useLegacyBoundsCalculation",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(
            if values.use_legacy_bounds_calculation {
                1
            } else {
                0
            },
        )),
    )?;
    importer.insert_field(
        "shouldRoundAdvanceValue",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(
            if values.should_round_advance_value {
                1
            } else {
                0
            },
        )),
    )?;
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field(
        "assetBundleName",
        UnityYamlNode::string(values.asset_bundle_name.as_deref().unwrap_or("")),
    )?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

pub(super) fn font_fallback_references_yaml(
    entry: &RoutedObject<'_>,
    values: &FontImporterValues,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    project_version: SceneUnityVersion,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut references = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for (index, (file_id, path_id)) in values.fallback_fonts.iter().copied().enumerate() {
        let field_path = format!("m_FallbackFonts[{index}]");
        let mut matching = reference_graph
            .references_from_object(entry.object.file_index, entry.object.object_index)
            .filter(|edge| {
                edge.field_path == field_path && edge.file_id == file_id && edge.path_id == path_id
            });
        let edge = matching.next().ok_or_else(|| {
            invalid_project(format!(
                "Font {}:{} fallback at index {index} ({file_id}, {path_id}) has no matching reference-graph edge at {field_path:?}",
                entry.object.file_index, entry.object.path_id
            ))
        })?;
        if matching.next().is_some() {
            return Err(invalid_project(format!(
                "Font {}:{} fallback at index {index} has duplicate reference-graph edges",
                entry.object.file_index, entry.object.path_id
            )));
        }
        let resolution = match &edge.resolution {
            MultiFilePPtrResolution::Null => ProjectPointerResolution::Null,
            MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index,
            } => {
                let target = objects
                    .get(*target_file_index, *target_object_index)
                    .ok_or_else(|| {
                        invalid_project(format!(
                            "Font {}:{} fallback at index {index} resolves to absent workspace object {target_file_index}:{target_object_index}",
                            entry.object.file_index, entry.object.path_id
                        ))
                    })?;
                if target.factory.class_id != 128 {
                    return Err(invalid_project(format!(
                        "Font {}:{} fallback at index {index} resolves to class {}, expected Font (128)",
                        entry.object.file_index, entry.object.path_id, target.factory.class_id
                    )));
                }
                let target_identity = ExportAssetIdentity {
                    collection: ExportAssetCollectionIdentity::SerializedFile {
                        file_index: target.file_index,
                    },
                    object_index: target.object_index,
                    path_id: target.path_id,
                    class_id: target.factory.class_id,
                };
                pointer_map.resolve(
                    ProjectPointerRequest::ResolvedAsset(target_identity),
                    entry.collection_index,
                )?
            }
            MultiFilePPtrResolution::PredefinedEngineAsset {
                resource, path_id, ..
            } => pointer_map.resolve(
                ProjectPointerRequest::PredefinedEngineAsset {
                    path_id: *path_id,
                    resource: *resource,
                },
                entry.collection_index,
            )?,
            unresolved => {
                return Err(invalid_project(format!(
                    "Font {}:{} fallback at index {index} ({file_id}, {path_id}) is unresolved: {unresolved:?}",
                    entry.object.file_index, entry.object.path_id
                )))
            }
        };
        if let ProjectPointerResolution::MissingExportedTarget { asset, .. } = &resolution {
            return Err(invalid_project(format!(
                "Font {}:{} fallback at index {index} targets Font {asset:?}, which has no exported project identity",
                entry.object.file_index, entry.object.path_id
            )));
        }
        let pointer = project_pointer_to_unity_yaml(
            &resolution,
            project_version,
            Some(crate::export::script_hashing::UnityGuid::MISSING_REFERENCE.to_byte_array()),
        )
        .map_err(|error| {
            invalid_project(format!(
                "Font {}:{} fallback at index {index} could not be represented in importer metadata: {error}",
                entry.object.file_index, entry.object.path_id
            ))
        })?;
        references.push(pointer)?;
    }
    Ok(references)
}

fn required_font_field<'value>(
    root: &'value SerializedValue,
    name: &str,
) -> Result<&'value SerializedValue, Box<dyn Error>> {
    optional_font_field(root, name)?
        .ok_or_else(|| invalid_project(format!("Font is missing required serialized field {name}")))
}

fn optional_font_field<'value>(
    root: &'value SerializedValue,
    name: &str,
) -> Result<Option<&'value SerializedValue>, Box<dyn Error>> {
    root.unique_field_in_base_chain(name)
        .map_err(|error| invalid_project(format!("Font {name} lookup failed: {error}")))
}

fn font_integer(value: &SerializedValue, field: &str) -> Result<i32, Box<dyn Error>> {
    let value = match value {
        SerializedValue::I8(value) => i64::from(*value),
        SerializedValue::U8(value) => i64::from(*value),
        SerializedValue::I16(value) => i64::from(*value),
        SerializedValue::U16(value) | SerializedValue::Char(value) => i64::from(*value),
        SerializedValue::I32(value) => i64::from(*value),
        SerializedValue::U32(value) => i64::from(*value),
        SerializedValue::I64(value) => *value,
        SerializedValue::U64(value) => i64::try_from(*value).map_err(|_| {
            invalid_project(format!(
                "Font {field} value {value} exceeds importer integer range"
            ))
        })?,
        other => {
            return Err(invalid_project(format!(
                "Font {field} is not an integer: {other:?}"
            )))
        }
    };
    i32::try_from(value).map_err(|_| {
        invalid_project(format!(
            "Font {field} value {value} exceeds importer integer range"
        ))
    })
}

fn font_bool(value: &SerializedValue, field: &str) -> Result<bool, Box<dyn Error>> {
    match value {
        SerializedValue::Bool(value) => Ok(*value),
        other => Err(invalid_project(format!(
            "Font {field} is not a boolean: {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{font_asset_data, font_importer_values, font_importer_yaml};
    use crate::export::unity_yaml::{
        write_unity_yaml_mapping, UnityYamlMappingStyle, UnityYamlNode, UnityYamlScalar,
        UnityYamlSequenceStyle, UnityYamlWriteLimits,
    };
    use crate::serialized_file::object::{
        ParsedObjectPayload, SerializedFieldValue, SerializedValue,
    };

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn source_font_payload() -> ParsedObjectPayload {
        ParsedObjectPayload {
            object_index: 0,
            path_id: 7,
            type_name: "Font".to_owned(),
            root: SerializedValue::Struct(vec![
                field(
                    "m_FontData",
                    SerializedValue::Array(
                        b"OTTOfont"
                            .iter()
                            .copied()
                            .map(SerializedValue::U8)
                            .collect(),
                    ),
                ),
                field("m_FontSize", SerializedValue::F32(16.0)),
                field(
                    "m_FontNames",
                    SerializedValue::Array(vec![SerializedValue::StringBytes(
                        b"Noto Serif JP".to_vec(),
                    )]),
                ),
                field(
                    "m_FallbackFonts",
                    SerializedValue::Array(vec![SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 42,
                    }]),
                ),
                field("m_CharacterSpacing", SerializedValue::I32(0)),
                field("m_CharacterPadding", SerializedValue::I32(1)),
                field("m_FontRenderingMode", SerializedValue::I32(0)),
                field("m_UseLegacyBoundsCalculation", SerializedValue::Bool(false)),
                field("m_ShouldRoundAdvanceValue", SerializedValue::Bool(true)),
            ]),
            consumed_bytes: 0,
            trailing_bytes: Vec::new(),
            allocated_bytes: 0,
        }
    }

    #[test]
    fn source_font_data_and_importer_fields_are_preserved_for_binary_export() {
        let payload = source_font_payload();
        let data = font_asset_data(&payload.root).unwrap().unwrap();
        assert_eq!(data.to_vec().unwrap(), b"OTTOfont");
        assert_eq!(data.extension().unwrap(), "otf");

        let importer = font_importer_values(&payload).unwrap();
        assert_eq!(importer.font_size, 16);
        assert_eq!(importer.font_names, ["Noto Serif JP"]);
        assert_eq!(importer.fallback_fonts, [(0, 42)]);
        assert_eq!(importer.character_spacing, 0);
        assert_eq!(importer.character_padding, 1);
        assert_eq!(importer.font_rendering_mode, 0);
        assert!(!importer.use_legacy_bounds_calculation);
        assert!(importer.should_round_advance_value);
    }

    #[test]
    fn true_type_font_importer_emits_source_import_fields_and_fallback_pptrs() {
        let values = font_importer_values(&source_font_payload()).unwrap();
        let mut fallback = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
        fallback
            .insert_field(
                "fileID",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(12800000)),
            )
            .unwrap();
        fallback
            .insert_field(
                "guid",
                UnityYamlNode::string("11111111111111111111111111111111"),
            )
            .unwrap();
        fallback
            .insert_field("type", UnityYamlNode::scalar(UnityYamlScalar::Signed(3)))
            .unwrap();
        let mut references = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
        references.push(fallback).unwrap();

        let yaml = write_unity_yaml_mapping(
            &font_importer_yaml(&values, references).unwrap(),
            UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for expected in [
            "serializedVersion: 4",
            "fontSize: 16",
            "forceTextureCase: -2",
            "characterSpacing: 0",
            "characterPadding: 1",
            "includeFontData: 1",
            "- Noto Serif JP",
            "- {fileID: 12800000, guid: 11111111111111111111111111111111, type: 3}",
            "fontRenderingMode: 0",
            "hasEmptyFontData: 0",
            "ascentCalculationMode: 1",
            "useLegacyBoundsCalculation: 0",
            "shouldRoundAdvanceValue: 1",
        ] {
            assert!(yaml.contains(expected), "missing {expected:?} in:\n{yaml}");
        }
    }
}
