use super::super::bundle_names::{
    asset_bundle_container_target, asset_bundle_name_for_asset, get_asset_bundle_name,
    AssetBundleContainerTarget, AssetBundleNameDecodeError,
};
use super::super::path_application::{
    bundle_container_original_path, calculated_original_path, ensure_starts_with_assets,
    initialize_shader_overrides, original_path_details, resource_manager_path_should_replace,
    restore_original_name, sort_workspace_original_path_events, BundleOriginalPathEffect,
    OriginalPathDetails, WorkspaceOriginalPathEvent, WorkspaceOriginalPathEventKind,
};
use super::super::path_model::{
    BundledAssetsExportMode, WorkspaceAssetExportPath, WorkspaceAssetExportPathReport,
};
use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
use crate::serialized_file::references::PPtrResolution;

fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
    SerializedFieldValue {
        name: name.to_owned(),
        value,
    }
}

fn root(fields: Vec<SerializedFieldValue>) -> SerializedValue {
    SerializedValue::Struct(fields)
}

#[test]
fn asset_bundle_name_prefers_nonempty_serialized_value_then_uses_bundle_name() {
    assert_eq!(
        get_asset_bundle_name(
            &root(vec![field(
                "m_AssetBundleName_R",
                SerializedValue::StringBytes(b"custom.bundle".to_vec()),
            )]),
            "source.bundle",
        )
        .unwrap(),
        "custom.bundle"
    );
    assert_eq!(
        get_asset_bundle_name(
            &root(vec![field(
                "AssetBundleName_R",
                SerializedValue::StringBytes(Vec::new()),
            )]),
            "source.bundle",
        )
        .unwrap(),
        "source.bundle"
    );
    assert_eq!(
        get_asset_bundle_name(&root(Vec::new()), "source.bundle").unwrap(),
        "source.bundle"
    );
}

#[test]
fn asset_bundle_name_rejects_conflicting_aliases_and_malformed_values() {
    let conflict = root(vec![
        field(
            "m_AssetBundleName_R",
            SerializedValue::StringBytes(b"one".to_vec()),
        ),
        field(
            "AssetBundleName_R",
            SerializedValue::StringBytes(b"two".to_vec()),
        ),
    ]);
    assert!(matches!(
        get_asset_bundle_name(&conflict, "fallback"),
        Err(AssetBundleNameDecodeError::DuplicateAliases { .. })
    ));
    assert!(matches!(
        get_asset_bundle_name(
            &root(vec![field("m_AssetBundleName_R", SerializedValue::I32(3))]),
            "fallback",
        ),
        Err(AssetBundleNameDecodeError::WrongType { .. })
    ));
    assert!(matches!(
        get_asset_bundle_name(
            &root(vec![field(
                "m_AssetBundleName_R",
                SerializedValue::StringBytes(vec![0xff]),
            )]),
            "fallback",
        ),
        Err(AssetBundleNameDecodeError::InvalidUtf8 { .. })
    ));
}

#[test]
fn asset_bundle_asset_name_removes_only_a_trailing_bundle_extension() {
    assert_eq!(
        asset_bundle_name_for_asset("avatars.bundle").unwrap(),
        "avatars"
    );
    assert_eq!(
        asset_bundle_name_for_asset("Avatars.BUNDLE").unwrap(),
        "Avatars"
    );
    assert_eq!(
        asset_bundle_name_for_asset("bundle.avatars").unwrap(),
        "bundle.avatars"
    );
}

#[test]
fn direct_asset_paths_keep_verbatim_correct_case_and_prepend_assets() {
    // `OriginalPathHelper.EnsureStartsWithAssets`: exact, case-corrected, prepended.
    assert_eq!(
        ensure_starts_with_assets("Assets/Textures/wood.mat").unwrap(),
        "Assets/Textures/wood.mat"
    );
    assert_eq!(
        ensure_starts_with_assets("aSSETS/Textures/wood.mat").unwrap(),
        "Assets/Textures/wood.mat"
    );
    assert_eq!(
        ensure_starts_with_assets("Textures/wood.mat").unwrap(),
        "Assets/Textures/wood.mat"
    );
}

#[test]
fn original_path_details_cover_trailing_separators_and_bare_names() {
    assert_eq!(
        original_path_details("Assets/Textures/").unwrap(),
        OriginalPathDetails {
            directory: Some("Assets".to_owned()),
            name: None,
            extension: None,
        }
    );
    assert_eq!(
        original_path_details("wood.png").unwrap(),
        OriginalPathDetails {
            directory: None,
            name: Some("wood".to_owned()),
            extension: Some("png".to_owned()),
        }
    );
}

#[test]
fn asset_bundle_original_paths_follow_all_export_modes() {
    assert_eq!(
        bundle_container_original_path(
            "Textures/wood.mat",
            "Avatars",
            BundledAssetsExportMode::DirectExport,
        )
        .unwrap()
        .unwrap()
        .original_path
        .as_deref(),
        Some("Assets/Textures/wood.mat")
    );
    assert_eq!(
        bundle_container_original_path(
            "/export/assets/Textures/wood.mat",
            "Avatars",
            BundledAssetsExportMode::DirectExport,
        )
        .unwrap()
        .unwrap()
        .original_path
        .as_deref(),
        Some("Assets/Textures/wood.mat")
    );
    assert_eq!(
        bundle_container_original_path(
            "/export/Outside/wood.mat",
            "Avatars",
            BundledAssetsExportMode::DirectExport,
        )
        .unwrap(),
        None
    );
    let expected_group_path = format!(
        "Assets/AssetBundles{}Avatars{}Textures/wood.mat",
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR,
    );
    assert_eq!(
        bundle_container_original_path(
            "assets/aVaTaRs/Textures/wood.mat",
            "Avatars",
            BundledAssetsExportMode::GroupByBundleName,
        )
        .unwrap(),
        Some(BundleOriginalPathEffect {
            original_path: Some(expected_group_path),
        })
    );
    assert_eq!(
        bundle_container_original_path(
            "Textures/wood.mat",
            "Avatars",
            BundledAssetsExportMode::GroupByAssetType,
        )
        .unwrap(),
        Some(BundleOriginalPathEffect {
            original_path: None,
        })
    );
}

#[test]
fn original_path_details_split_directory_name_and_extension() {
    assert_eq!(
        original_path_details("Assets/Textures/Wood.Albedo.PNG").unwrap(),
        OriginalPathDetails {
            directory: Some("Assets/Textures".to_owned()),
            name: Some("Wood.Albedo".to_owned()),
            extension: Some("PNG".to_owned()),
        }
    );
    assert_eq!(
        original_path_details("Assets/Textures/README").unwrap(),
        OriginalPathDetails {
            directory: Some("Assets/Textures".to_owned()),
            name: Some("README".to_owned()),
            extension: None,
        }
    );
}

#[test]
fn original_name_restoration_matches_ascii_case_and_utf16_length() {
    let mut record = WorkspaceAssetExportPath {
        original_path: Some("Assets/textures/wood.png".to_owned()),
        original_directory: Some("Assets/textures".to_owned()),
        original_name: Some("wood".to_owned()),
        original_extension: Some("png".to_owned()),
        ..WorkspaceAssetExportPath::default()
    };
    let mut report = WorkspaceAssetExportPathReport::default();
    assert!(restore_original_name(&mut record, "Wood", &mut report).unwrap());
    assert_eq!(record.original_name.as_deref(), Some("Wood"));
    record.original_path = calculated_original_path(&record).unwrap();
    assert_eq!(
        record.original_path.as_deref(),
        Some(format!("Assets/textures{}Wood.png", std::path::MAIN_SEPARATOR).as_str())
    );
    assert_eq!(report.restored_original_names, 1);

    assert!(!restore_original_name(&mut record, "Longer", &mut report).unwrap());
    assert_eq!(report.restored_original_names, 1);
}

#[test]
fn original_name_restoration_reports_non_ascii_case_as_unverified() {
    let mut record = WorkspaceAssetExportPath {
        original_name: Some("über".to_owned()),
        ..WorkspaceAssetExportPath::default()
    };
    let mut report = WorkspaceAssetExportPathReport::default();
    assert!(!restore_original_name(&mut record, "Über", &mut report).unwrap());
    assert_eq!(record.original_name.as_deref(), Some("über"));
    assert_eq!(report.case_restoration_unverified, 1);
}

#[test]
fn shader_override_initialization_preserves_existing_values() {
    let mut record = WorkspaceAssetExportPath {
        original_directory: Some("Assets/Shaders".to_owned()),
        original_name: Some("Surface".to_owned()),
        original_extension: Some("shader".to_owned()),
        override_name: Some("CustomName".to_owned()),
        ..WorkspaceAssetExportPath::default()
    };
    let mut report = WorkspaceAssetExportPathReport::default();
    initialize_shader_overrides(&mut record, &mut report).unwrap();
    assert_eq!(record.override_directory.as_deref(), Some("Assets/Shaders"));
    assert_eq!(record.override_name.as_deref(), Some("CustomName"));
    assert_eq!(record.override_extension.as_deref(), Some("shader"));
    assert_eq!(report.initialized_shader_overrides, 1);

    initialize_shader_overrides(&mut record, &mut report).unwrap();
    assert_eq!(report.initialized_shader_overrides, 1);
}

#[test]
fn resource_manager_path_precedence_uses_utf16_length_and_keeps_first_tie() {
    assert!(resource_manager_path_should_replace(
        None,
        "Assets/Resources/wood"
    ));
    assert!(!resource_manager_path_should_replace(
        Some("Assets/Resources/é"),
        "Assets/Resources/x"
    ));
    assert!(resource_manager_path_should_replace(
        Some("Assets/Resources/wood"),
        "Assets/Resources/inner/wood"
    ));
}

#[test]
fn original_path_events_follow_collection_object_and_container_order() {
    let mut events = [
        WorkspaceOriginalPathEvent {
            collection_index: 1,
            source_object_index: 0,
            container_entry_index: 0,
            kind: WorkspaceOriginalPathEventKind::AssetBundle(0),
        },
        WorkspaceOriginalPathEvent {
            collection_index: 0,
            source_object_index: 3,
            container_entry_index: 1,
            kind: WorkspaceOriginalPathEventKind::ResourceManager(0),
        },
        WorkspaceOriginalPathEvent {
            collection_index: 0,
            source_object_index: 2,
            container_entry_index: 0,
            kind: WorkspaceOriginalPathEventKind::AssetBundle(1),
        },
        WorkspaceOriginalPathEvent {
            collection_index: 0,
            source_object_index: 3,
            container_entry_index: 0,
            kind: WorkspaceOriginalPathEventKind::ResourceManager(1),
        },
    ];

    sort_workspace_original_path_events(&mut events);

    assert_eq!(
        events.map(|event| event.kind),
        [
            WorkspaceOriginalPathEventKind::AssetBundle(1),
            WorkspaceOriginalPathEventKind::ResourceManager(1),
            WorkspaceOriginalPathEventKind::ResourceManager(0),
            WorkspaceOriginalPathEventKind::AssetBundle(0),
        ]
    );
}

#[test]
fn container_pointer_resolution_keeps_null_local_shared_and_invalid_distinct() {
    assert_eq!(
        asset_bundle_container_target(PPtrResolution::Null),
        AssetBundleContainerTarget::Null
    );
    assert_eq!(
        asset_bundle_container_target(PPtrResolution::LocalObject { object_index: 4 }),
        AssetBundleContainerTarget::LocalObject { object_index: 4 }
    );
    assert_eq!(
        asset_bundle_container_target(PPtrResolution::ExternalDependency {
            file_id: 2,
            external_index: 1,
            path_id: 9,
            identifier: &crate::serialized_file::metadata::FileIdentifier {
                asset_path: None,
                guid: None,
                file_type: None,
                path_name_origin: String::new(),
            },
        }),
        AssetBundleContainerTarget::SharedFile {
            file_id: 2,
            external_index: 1,
            path_id: 9,
        }
    );
    assert_eq!(
        asset_bundle_container_target(PPtrResolution::InvalidFileId {
            file_id: -1,
            external_count: 0,
        }),
        AssetBundleContainerTarget::InvalidFileId {
            file_id: -1,
            external_count: 0,
        }
    );
}
