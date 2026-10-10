use super::*;

#[test]
fn project_export_writes_a_processed_prefab_collection_with_generated_transform() {
    let bytes = make_game_object_without_transform_file();
    let (filesystem, workspace) = load_workspace_bytes(&bytes);
    let objects = ObjectWorkspace::load_from_vfs_with_layout_source(
        &filesystem,
        &workspace,
        ObjectWorkspaceLimits::default(),
        &crate::processing::asset_factory::FileTypeTreeTestLayouts,
    )
    .unwrap();
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    let root_catalog = RootResourceFileCatalog::default();
    let provider = PlatformResourceProvider::new(None, None);
    let settings = ExportSettingsFile::from(ExportDefaults::default());
    let output_root = crate::test_support::unique_test_directory("project-prefab-e2e");

    let report = export_workspace_yaml_project_with_settings(
        &output_root,
        false,
        &filesystem,
        &root_catalog,
        &provider,
        &workspace,
        &objects,
        &reference_graph,
        &settings,
        WorkspaceProjectExportLimits::default(),
    )
    .unwrap();

    let prefab_path = output_root.join("Assets/GameObject/Root.prefab");
    let prefab_meta_path = output_root.join("Assets/GameObject/Root.prefab.meta");
    let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
    let prefab_meta = std::fs::read_to_string(&prefab_meta_path).unwrap();
    let game_object_id = yaml_document_file_id(&prefab_yaml, 1);
    let transform_id = yaml_document_file_id(&prefab_yaml, 4);
    assert!(prefab_yaml.contains("m_Name: Root"), "{prefab_yaml}");
    assert!(prefab_yaml.contains("m_LocalPosition:"), "{prefab_yaml}");
    assert!(
        prefab_yaml.contains(&format!("component: {{fileID: {transform_id}}}")),
        "{prefab_yaml}"
    );
    assert!(
        prefab_yaml.contains(&format!("m_GameObject: {{fileID: {game_object_id}}}")),
        "{prefab_yaml}"
    );
    assert!(!prefab_yaml.contains("!u!1001"), "{prefab_yaml}");
    assert!(prefab_meta.contains("PrefabImporter:"), "{prefab_meta}");
    assert!(prefab_meta.contains("externalObjects: {}"), "{prefab_meta}");
    assert!(prefab_meta.contains("userData:\n"), "{prefab_meta}");
    assert!(prefab_meta.contains("assetBundleName:\n"), "{prefab_meta}");
    assert!(
        prefab_meta.contains("assetBundleVariant:\n"),
        "{prefab_meta}"
    );
    assert!(!output_root.join("Assets/PrefabInstance").exists());
    assert_eq!(report.asset_file_count, 1);
    assert_eq!(report.meta_file_count, 1);
    assert_eq!(report.collection_count, 1);
    assert_eq!(report.exported_object_count, 2);

    std::fs::remove_dir_all(output_root).unwrap();
}

#[test]
fn project_export_matches_source_for_a_generated_legacy_prefab() {
    let bytes = include_bytes!("../../legacy_prefab_2017_4_input.unity");
    let (filesystem, workspace) =
        load_workspace_named_bytes_with_source_factory("LegacyRoot.unity", bytes);
    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    assert_eq!(objects.objects().len(), 2);
    assert!(objects.objects().iter().all(|object| matches!(
        &object.factory.kind,
        crate::processing::asset_factory::FactoryAssetKind::Generated(_)
    )));
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());
    let root_catalog = RootResourceFileCatalog::default();
    let provider = PlatformResourceProvider::new(None, None);
    let settings = ExportSettingsFile::from(ExportDefaults::default());
    let output_root = crate::test_support::unique_test_directory("project-legacy-prefab-e2e");

    let report = export_workspace_yaml_project_with_settings(
        &output_root,
        false,
        &filesystem,
        &root_catalog,
        &provider,
        &workspace,
        &objects,
        &reference_graph,
        &settings,
        WorkspaceProjectExportLimits::default(),
    )
    .unwrap();

    let prefab_path = output_root.join("Assets/GameObject/LegacyRoot.prefab");
    let prefab_meta_path = output_root.join("Assets/GameObject/LegacyRoot.prefab.meta");
    let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
    let expected_yaml = include_str!("../../legacy_prefab_source_2017_4_fixture.yaml");
    assert_eq!(
        prefab_yaml.replace("\r\n", "\n"),
        expected_yaml.replace("\r\n", "\n")
    );
    let prefab_meta = std::fs::read_to_string(&prefab_meta_path).unwrap();
    assert!(
        prefab_meta.contains("NativeFormatImporter:"),
        "{prefab_meta}"
    );
    assert!(
        prefab_meta.contains("mainObjectFileID: 100100000"),
        "{prefab_meta}"
    );
    assert!(!prefab_meta.contains("PrefabImporter:"), "{prefab_meta}");
    assert_eq!(report.asset_file_count, 1);
    assert_eq!(report.meta_file_count, 1);
    assert_eq!(report.post_export_file_count, 1);
    assert_eq!(report.collection_count, 1);
    assert_eq!(report.exported_object_count, 3);
    assert!(report.processing_warnings.is_empty());

    let path_id_map = std::fs::read(output_root.join("AuxiliaryFiles/path_id_map.json")).unwrap();
    let expected_path_id_map = include_str!("../../legacy_prefab_source_2017_4_probe_fixture.txt")
        .lines()
        .find_map(|line| line.strip_prefix("FILE|AuxiliaryFiles/path_id_map.json|"))
        .expect("frozen source fixture records the PathIdMap output")
        .split('|')
        .collect::<Vec<_>>();
    assert_eq!(path_id_map.len().to_string(), expected_path_id_map[0]);
    let expected_empty_map = crate::export::path_id_map::serialize_path_id_map(
        &crate::export::path_id_map::SerializedGameInfo {
            files: vec![crate::export::path_id_map::SerializedFileInfo {
                name: Some("legacyroot.unity".to_owned()),
                assets: Vec::new(),
            }],
        },
    );
    assert_eq!(
        path_id_map,
        expected_empty_map.as_bytes(),
        "empty PathIdMap body differs from the source fixture's collection name and shape"
    );

    std::fs::remove_dir_all(output_root).unwrap();
}

#[test]
fn project_export_matches_source_for_a_no_tree_generated_prefab_hierarchy() {
    let fixture = include_str!("../../no_tree_prefab_source_2019_4_input.hex");
    let hex = fixture
        .lines()
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .expect("source-authored input fixture has a hex payload");
    assert_eq!(hex.len() % 2, 0, "source fixture hex has an odd length");
    let bytes = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                .expect("source fixture contains ASCII hexadecimal bytes")
        })
        .collect::<Vec<_>>();
    assert_eq!(bytes.len(), 360);

    let (filesystem, workspace) =
        load_workspace_named_bytes_with_source_factory("NoTreeRoot.unity", &bytes);
    let serialized_file = workspace.files().first().expect("one source file");
    assert_eq!(serialized_file.metadata.objects.len(), 2);
    assert!(serialized_file
        .metadata
        .types
        .iter()
        .all(|serialized_type| serialized_type
            .type_tree
            .as_ref()
            .is_none_or(|tree| tree.nodes.is_empty())));
    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    assert_eq!(objects.objects().len(), 2);
    assert!(objects.objects().iter().all(|object| {
        matches!(
            &object.factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(_)
        ) && object.recovered_type_tree.is_some()
            && object.payload.is_ok()
    }));
    assert_eq!(objects.get(0, 0).unwrap().factory.name, "NoTreeRoot");

    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());
    assert!(reference_graph.references().iter().any(|reference| {
        reference.source_path_id == 10
            && reference.path_id == 11
            && reference.field_path == "m_Component[0].component"
            && matches!(
                reference.resolution,
                MultiFilePPtrResolution::Object {
                    target_file_index: 0,
                    target_object_index: 1,
                }
            )
    }));
    assert!(reference_graph.references().iter().any(|reference| {
        reference.source_path_id == 11
            && reference.path_id == 10
            && reference.field_path == "m_GameObject"
            && matches!(
                reference.resolution,
                MultiFilePPtrResolution::Object {
                    target_file_index: 0,
                    target_object_index: 0,
                }
            )
    }));

    let root_catalog = RootResourceFileCatalog::default();
    let provider = PlatformResourceProvider::new(None, None);
    let settings = ExportSettingsFile::from(ExportDefaults::default());
    let output_root = crate::test_support::unique_test_directory("project-no-tree-prefab-e2e");
    let report = export_workspace_yaml_project_with_settings(
        &output_root,
        false,
        &filesystem,
        &root_catalog,
        &provider,
        &workspace,
        &objects,
        &reference_graph,
        &settings,
        WorkspaceProjectExportLimits::default(),
    )
    .unwrap();

    let prefab_path = output_root.join("Assets/GameObject/NoTreeRoot.prefab");
    let prefab_meta_path = output_root.join("Assets/GameObject/NoTreeRoot.prefab.meta");
    let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
    let source_yaml = include_str!("../../no_tree_prefab_source_2019_4.yaml");
    assert_eq!(
        prefab_yaml.replace("\r\n", "\n"),
        source_yaml.replace("\r\n", "\n")
    );
    assert!(!output_root.join("Assets/PrefabInstance").exists());
    let prefab_meta = std::fs::read_to_string(prefab_meta_path).unwrap();
    let source_meta = include_str!("../../no_tree_prefab_source_2019_4.meta.yaml");
    let normalize_meta = |text: &str| {
        text.lines()
            .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(normalize_meta(&prefab_meta), normalize_meta(source_meta));
    assert_eq!(report.asset_file_count, 1);
    assert_eq!(report.meta_file_count, 1);
    assert_eq!(report.collection_count, 1);
    assert_eq!(report.exported_object_count, 2);
    assert!(report.processing_warnings.is_empty());

    std::fs::remove_dir_all(output_root).unwrap();
}

#[test]
fn project_export_routes_an_existing_modern_prefab_instance_collection() {
    let bytes = make_existing_prefab_instance_file();
    if let Ok(path) = std::env::var("ASSET_RIPPER_REFERENCE_PREFAB_FIXTURE") {
        std::fs::write(path, &bytes).unwrap();
    }
    let (filesystem, workspace) = load_workspace_bytes_with_source_factory(&bytes);
    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(
        objects
            .objects()
            .iter()
            .all(|object| object.payload.is_ok()),
        "undecoded Prefab fixture objects: {:?}",
        objects
            .objects()
            .iter()
            .filter_map(|object| object.payload.as_ref().err())
            .collect::<Vec<_>>()
    );
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    assert_eq!(objects.objects().len(), 3);
    assert_eq!(
        objects.get(0, 0).unwrap().factory.kind,
        crate::processing::asset_factory::FactoryAssetKind::Generated(
            "GameObject_2018_3".to_owned()
        )
    );
    assert_eq!(objects.get(0, 0).unwrap().factory.name, "Root");
    assert_eq!(
        objects.get(0, 1).unwrap().factory.kind,
        crate::processing::asset_factory::FactoryAssetKind::Generated(
            "Transform_2018_3".to_owned()
        )
    );
    assert_eq!(
        objects.get(0, 2).unwrap().factory.kind,
        crate::processing::asset_factory::FactoryAssetKind::Generated(
            "PrefabInstance_2018_3".to_owned()
        )
    );
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(
        reference_graph.is_complete(),
        "reference graph is incomplete"
    );
    let resolves_to = |source_path_id, target_path_id| {
        reference_graph.references().iter().any(|reference| {
            reference.source_path_id == source_path_id
                && reference.path_id == target_path_id
                && reference.file_id == 0
                && matches!(
                    reference.resolution,
                    MultiFilePPtrResolution::Object {
                        target_file_index: 0,
                        ..
                    }
                )
        })
    };
    assert!(
        resolves_to(10, 11),
        "GameObject component PPtr was not retained"
    );
    assert!(
        resolves_to(11, 10),
        "Transform-to-GameObject PPtr was not retained"
    );
    assert!(
        resolves_to(12, 10),
        "PrefabInstance root PPtr was not retained"
    );
    let root_catalog = RootResourceFileCatalog::default();
    let provider = PlatformResourceProvider::new(None, None);
    let settings = ExportSettingsFile::from(ExportDefaults::default());
    let output_root = crate::test_support::unique_test_directory("project-existing-prefab-e2e");

    let report = export_workspace_yaml_project_with_settings(
        &output_root,
        false,
        &filesystem,
        &root_catalog,
        &provider,
        &workspace,
        &objects,
        &reference_graph,
        &settings,
        WorkspaceProjectExportLimits::default(),
    )
    .unwrap();

    let prefab_path = output_root.join("Assets/PrefabInstance/Root.prefab");
    let prefab_meta_path = output_root.join("Assets/PrefabInstance/Root.prefab.meta");
    let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
    let prefab_meta = std::fs::read_to_string(&prefab_meta_path).unwrap();
    let game_object_id = yaml_document_file_id(&prefab_yaml, 1);
    let transform_id = yaml_document_file_id(&prefab_yaml, 4);
    assert!(prefab_yaml.contains("m_Name: Root"), "{prefab_yaml}");
    assert!(
        prefab_yaml.contains(&format!("component: {{fileID: {transform_id}}}")),
        "{prefab_yaml}"
    );
    assert!(
        prefab_yaml.contains(&format!("m_GameObject: {{fileID: {game_object_id}}}")),
        "{prefab_yaml}"
    );
    assert!(!prefab_yaml.contains("!u!1001"), "{prefab_yaml}");
    assert!(prefab_meta.contains("PrefabImporter:"), "{prefab_meta}");
    assert_eq!(report.asset_file_count, 1);
    assert_eq!(report.meta_file_count, 1);
    assert_eq!(report.collection_count, 1);
    assert_eq!(report.exported_object_count, 2);
    assert!(report.processing_warnings.is_empty());

    let source_rows = include_str!("../../../processing/prefab_instance_runtime_fixture.txt");
    assert!(source_rows.lines().any(|line| {
            line == "PREFAB-HIER|root=10:Root|prefab=12|gameObjects=GameObject_2018_3#10:Root|components=Transform_2018_3#11:null|prefabInstances=|exportable=GameObject_2018_3#10:Root,Transform_2018_3#11:null|hidden=PrefabInstance_2018_3#12:null"
        }));

    std::fs::remove_dir_all(output_root).unwrap();
}

fn yaml_document_file_id(yaml: &str, class_id: i32) -> String {
    let prefix = format!("--- !u!{class_id} &");
    yaml.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .and_then(|tail| tail.split_whitespace().next())
        .expect("the expected Unity YAML document exists")
        .to_owned()
}
