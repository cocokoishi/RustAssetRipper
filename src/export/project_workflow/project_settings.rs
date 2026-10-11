use super::*;
use crate::export::value_yaml::{
    serialized_object_value_to_unity_yaml_document, UnityYamlObjectConversionHints,
    UnityYamlObjectFieldMode,
};
use crate::processing::project_settings::{EditorSettingsCapabilities, EditorSettingsModel};

const EDITOR_SETTINGS_PATH: &str = "ProjectSettings/EditorSettings.asset";

/// SceneDefinitionProcessor always creates EditorSettings, including imports with
/// no scene. It is a project configuration file without a GUID or importer sidecar.
pub(super) fn plan_generated_editor_settings(
    version_text: &str,
    project_version: SceneUnityVersion,
    staged: &mut StagedProjectFiles,
    limits: WorkspaceProjectExportLimits,
) -> Result<Option<PlannedProjectPostExportFile>, Box<dyn Error>> {
    let bytes = editor_settings_yaml(version_text, project_version, limits)?;
    if let Some(existing) = staged
        .files
        .iter_mut()
        .find(|file| file.relative_path == EDITOR_SETTINGS_PATH)
    {
        // Generated settings are the last collection in the source processing pass,
        // so their defaults replace any imported EditorSettings at this same path.
        // Keep the output plan unique instead of relying on a later overwrite.
        if existing.kind != ExportCollectionKind::Manager || existing.meta.is_some() {
            return Err(invalid_project(
                "EditorSettings output conflicts with a non-manager asset",
            ));
        }
        staged.total_bytes = staged
            .total_bytes
            .checked_sub(existing.bytes.len())
            .and_then(|count| count.checked_add(bytes.len()))
            .ok_or_else(|| invalid_project("EditorSettings output byte count overflows"))?;
        if staged.total_bytes > limits.max_total_asset_payload_bytes {
            return Err(invalid_project(
                "EditorSettings exceeds the project payload byte limit",
            ));
        }
        existing.bytes = bytes;
        Ok(None)
    } else {
        // Settings without input object identities join the generated project files,
        // like ProjectVersion.txt; they do not enter the asset pointer registry.
        let combined = staged
            .total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid_project("EditorSettings output byte count overflows"))?;
        if combined > limits.max_total_asset_payload_bytes {
            return Err(invalid_project(
                "EditorSettings exceeds the project payload byte limit",
            ));
        }
        Ok(Some(PlannedProjectPostExportFile {
            relative_path: EDITOR_SETTINGS_PATH.to_owned(),
            bytes,
        }))
    }
}

fn editor_settings_yaml(
    version_text: &str,
    project_version: SceneUnityVersion,
    limits: WorkspaceProjectExportLimits,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let version = FactoryUnityVersion::parse(version_text)
        .ok_or_else(|| invalid_project("EditorSettings requires a valid Unity version"))?;
    let (_, editor) = reference_class_layouts()
        .tpk_trees(159, &version)
        .map_err(|error| {
            invalid_project(format!(
                "EditorSettings layout lookup failed for Unity {version_text}: {error}"
            ))
        })?
        .ok_or_else(|| {
            invalid_project(format!("Unity {version_text} has no EditorSettings layout"))
        })?;
    let root_node = editor
        .nodes
        .first()
        .ok_or_else(|| invalid_project("EditorSettings layout is empty"))?;
    let mut root = default_value_for_type_tree_node(
        &editor,
        0,
        0,
        ObjectDefaultValueLimits {
            max_allocation_bytes: limits.max_total_asset_payload_bytes,
            ..ObjectDefaultValueLimits::default()
        },
    )?;
    apply_editor_settings_defaults(&mut root)?;
    let yaml_limits = limits.yaml_collection.object_conversion;
    let mut hints =
        UnityYamlObjectConversionHints::from_type_tree(&editor, project_version, yaml_limits)?;
    // Generated WalkEditor uses OriginalFieldName, not its renamed property names.
    hints.field_name_aliases.clear();
    hints.preserve_type_tree_field_names = true;
    let mut nonnull_pointer = None;
    let document = serialized_object_value_to_unity_yaml_document(
        ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: 0,
            },
            object_index: 0,
            path_id: 1,
            class_id: 159,
        },
        "EditorSettings",
        root_node.version,
        1,
        false,
        &root,
        &hints,
        UnityYamlObjectFieldMode::ProjectAsset,
        project_version,
        None,
        yaml_limits,
        |pointer| {
            if pointer.path_id != 0 {
                nonnull_pointer = Some(format!(
                    "{} ({}, {})",
                    pointer.field_path, pointer.file_id, pointer.path_id
                ));
            }
            ProjectPointerResolution::Null
        },
    )?;
    if let Some(pointer) = nonnull_pointer {
        return Err(invalid_project(format!(
            "generated EditorSettings contains an unmapped reference: {pointer}"
        )));
    }
    Ok(write_unity_yaml_documents(&[document], limits.yaml_write)?.into_bytes())
}

fn apply_editor_settings_defaults(root: &mut SerializedValue) -> Result<(), Box<dyn Error>> {
    let defaults = EditorSettingsModel::source_defaults(EditorSettingsCapabilities {
        collab_editor_settings: root
            .unique_field_in_base_chain("m_CollabEditorSettings")?
            .is_some(),
        asset_pipeline_mode: root
            .unique_field_in_base_chain("m_AssetPipelineMode")?
            .is_some(),
    });
    if let Some(support) = root.unique_field_in_base_chain_mut("m_ExternalVersionControlSupport")? {
        match support {
            SerializedValue::StringBytes(value) => {
                *value = defaults
                    .external_version_control_support_text
                    .as_bytes()
                    .to_vec()
            }
            // Frozen generated ExternalVersionControl.Generic is 1.
            SerializedValue::I32(value) => *value = 1,
            SerializedValue::U32(value) => *value = 1,
            _ => {
                return Err(invalid_project(
                    "EditorSettings version-control field has an unexpected type",
                ))
            }
        }
    }
    for (name, value) in [
        ("m_SerializationMode", 2), // SerializationMode.ForceText
        (
            "m_SpritePackerPaddingPower",
            defaults.sprite_packer_padding_power,
        ),
        (
            "m_EtcTextureCompressorBehavior",
            defaults.etc_texture_compressor_behavior,
        ),
        (
            "m_EtcTextureFastCompressor",
            defaults.etc_texture_fast_compressor,
        ),
        (
            "m_EtcTextureNormalCompressor",
            defaults.etc_texture_normal_compressor,
        ),
        (
            "m_EtcTextureBestCompressor",
            defaults.etc_texture_best_compressor,
        ),
        ("m_CacheServerMode", 0), // CacheServerMode.AsPreferences
        (
            "m_EnterPlayModeOptions",
            i32::from(defaults.enter_play_mode_options.disable_domain_reload)
                | (i32::from(defaults.enter_play_mode_options.disable_scene_reload) << 1),
        ),
        (
            "m_SerializeInlineMappingsOnOneLine",
            i32::from(defaults.serialize_inline_mappings_on_one_line),
        ),
        (
            "m_EnableTextureStreamingInEditMode",
            i32::from(defaults.enable_texture_streaming_in_edit_mode),
        ),
        (
            "m_EnableTextureStreamingInPlayMode",
            i32::from(defaults.enable_texture_streaming_in_play_mode),
        ),
        (
            "m_AsyncShaderCompilation",
            i32::from(defaults.async_shader_compilation),
        ),
        (
            "m_CacheServerEnableDownload",
            i32::from(defaults.cache_server_enable_download),
        ),
        (
            "m_CacheServerEnableUpload",
            i32::from(defaults.cache_server_enable_upload),
        ),
        (
            "m_ShowLightmapResolutionOverlay",
            i32::from(defaults.show_lightmap_resolution_overlay),
        ),
        (
            "m_UseLegacyProbeSampleCount",
            i32::from(defaults.use_legacy_probe_sample_count),
        ),
    ] {
        set_number(root, name, value)?;
    }
    if defaults.asset_pipeline_mode.is_some() {
        set_number(root, "m_AssetPipelineMode", 1)?; // AssetPipelineMode.Version2
    }
    for (name, value) in [
        (
            "m_ProjectGenerationIncludedExtensions",
            defaults.project_generation_included_extensions,
        ),
        (
            "m_ProjectGenerationRootNamespace",
            defaults.project_generation_root_namespace,
        ),
        (
            "m_UserGeneratedProjectSuffix",
            defaults.user_generated_project_suffix,
        ),
        ("m_CacheServerEndpoint", defaults.cache_server_endpoint),
        (
            "m_CacheServerNamespacePrefix",
            defaults.cache_server_namespace_prefix,
        ),
    ] {
        if let Some(field) = root.unique_field_in_base_chain_mut(name)? {
            let SerializedValue::StringBytes(bytes) = field else {
                return Err(invalid_project(format!(
                    "EditorSettings {name} is not a string"
                )));
            };
            *bytes = value.as_bytes().to_vec();
        }
    }
    if let Some(collab) = defaults.collab_editor_settings {
        let field = root
            .unique_field_in_base_chain_mut("m_CollabEditorSettings")?
            .ok_or_else(|| invalid_project("EditorSettings collaboration field disappeared"))?;
        set_number(
            field,
            "inProgressEnabled",
            i32::from(collab.in_progress_enabled),
        )?;
    }
    Ok(())
}

/// Optional generated properties have no effect in versions where the field does
/// not exist. A present field must still have the exact integer/bool layout.
fn set_number(root: &mut SerializedValue, name: &str, number: i32) -> Result<(), Box<dyn Error>> {
    let Some(field) = root.unique_field_in_base_chain_mut(name)? else {
        return Ok(());
    };
    match field {
        SerializedValue::Bool(value) if matches!(number, 0 | 1) => *value = number != 0,
        SerializedValue::I32(value) => *value = number,
        SerializedValue::U32(value) => {
            *value = u32::try_from(number).map_err(|_| {
                invalid_project(format!(
                    "EditorSettings {name} cannot store a negative value"
                ))
            })?
        }
        _ => {
            return Err(invalid_project(format!(
                "EditorSettings {name} has an unexpected numeric type"
            )))
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_editor_settings_matches_existing_independent_project_outputs() {
        for (version, fixture) in [
            (
                "2019.4.40f1",
                include_str!("../audio_project_stream_source_2019_4_fixture.txt"),
            ),
            (
                "2022.3.22f1",
                include_str!("../texture_project_source_2022_3_fixture.txt"),
            ),
        ] {
            let hex = fixture
                .lines()
                .find_map(|line| {
                    line.strip_prefix(
                        "SOURCE_OUTPUT|ExportedProject/ProjectSettings/EditorSettings.asset|",
                    )
                })
                .unwrap();
            let expected = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>();
            let actual = editor_settings_yaml(
                version,
                SceneUnityVersion::parse(version).unwrap(),
                WorkspaceProjectExportLimits::default(),
            )
            .unwrap();
            assert_eq!(
                String::from_utf8(actual).unwrap(),
                String::from_utf8(expected).unwrap(),
                "Unity {version}"
            );
        }
    }

    #[test]
    fn generated_editor_settings_replaces_imported_settings_without_duplicate_paths() {
        let mut staged = StagedProjectFiles {
            files: vec![PlannedProjectFile {
                collection_name: "ManagerExportCollection",
                kind: ExportCollectionKind::Manager,
                relative_path: EDITOR_SETTINGS_PATH.to_owned(),
                guid: [0; 16],
                bytes: b"imported settings".to_vec(),
                meta: None,
                label: "EditorSettings".to_owned(),
            }],
            support_files: Vec::new(),
            total_bytes: b"imported settings".len(),
            exported_object_count: 1,
        };
        let version = SceneUnityVersion::parse("2022.3.22f1").unwrap();
        let result = plan_generated_editor_settings(
            "2022.3.22f1",
            version,
            &mut staged,
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();
        assert!(result.is_none());
        assert_eq!(staged.files.len(), 1);
        assert_eq!(staged.total_bytes, staged.files[0].bytes.len());
        assert!(std::str::from_utf8(&staged.files[0].bytes)
            .unwrap()
            .contains("m_SerializationMode: 2\n"));
        staged.files[0].kind = ExportCollectionKind::Asset;
        assert!(plan_generated_editor_settings(
            "2022.3.22f1",
            version,
            &mut staged,
            WorkspaceProjectExportLimits::default()
        )
        .is_err());
    }
}
