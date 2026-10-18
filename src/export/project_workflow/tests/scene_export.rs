use super::*;

#[test]
fn project_export_matches_source_for_a_scene_owned_no_tree_missing_transform() {
    let fixture =
        include_str!("../../../processing/scene_owned_missing_transform_source_fixture.txt");
    let hex = fixture
        .lines()
        .find_map(|line| line.strip_prefix("INPUT_HEX|"))
        .expect("Scene-owned fixture includes the source SerializedFile bytes");
    assert_eq!(hex.len() % 2, 0, "fixture hex has an odd length");
    let bytes = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                .expect("fixture contains ASCII hexadecimal bytes")
        })
        .collect::<Vec<_>>();
    assert_eq!(bytes.len(), 368);

    let (filesystem, workspace) =
        load_workspace_named_bytes_with_source_factory("SceneOwnedMissingTransform.unity", &bytes);
    assert_eq!(workspace.files().len(), 1);
    assert_eq!(workspace.files()[0].metadata.objects.len(), 2);
    assert!(workspace.files()[0]
        .metadata
        .types
        .iter()
        .all(|serialized_type| {
            serialized_type
                .type_tree
                .as_ref()
                .is_none_or(|tree| tree.nodes.is_empty())
        }));
    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    assert_eq!(objects.objects().len(), 2);
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(!reference_graph.is_complete());
    assert!(reference_graph.references().iter().any(|reference| {
        reference.source_path_id == 10
            && reference.field_path == "m_Component[0].component"
            && matches!(
                reference.resolution,
                MultiFilePPtrResolution::MissingDependency {
                    external_index: 0,
                    path_id: 99,
                }
            )
    }));

    let processing = crate::export::scene_wiring::process_workspace_scenes_and_prefabs(
        &filesystem,
        &workspace,
        &objects,
        &reference_graph,
        crate::export::scene_wiring::WorkspaceSceneProcessingLimits::default(),
    )
    .unwrap();
    assert_eq!(processing.definitions.definitions().len(), 1);
    assert_eq!(processing.scene_hierarchies.len(), 1);
    assert!(
        !processing.scene_hierarchies[0].incomplete,
        "{:?}",
        processing.scene_hierarchies
    );
    assert_eq!(processing.generated_transforms.len(), 1);
    assert_eq!(
        processing.generated_transforms[0].collection_name,
        "sceneownedmissingtransform.unity (Generated Assets)"
    );

    let root_catalog = RootResourceFileCatalog::default();
    let provider = PlatformResourceProvider::new(None, None);
    let settings = ExportSettingsFile::from(ExportDefaults::default());
    let output_root =
        crate::test_support::unique_test_directory("project-scene-owned-missing-transform-e2e");
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

    let scene_path = output_root.join("Assets/Scenes/sceneownedmissingtransform.unity.unity");
    let scene_meta_path =
        output_root.join("Assets/Scenes/sceneownedmissingtransform.unity.unity.meta");
    let scene_yaml = std::fs::read_to_string(&scene_path).unwrap();
    let source_yaml = include_str!("../no_tree_scene_owned_missing_transform_source_scene.yml");
    let transform_id = scene_yaml_document_file_id(&scene_yaml, 4);
    let source_transform_id = scene_yaml_document_file_id(source_yaml, 4);
    let normalize_transform_id = |yaml: &str, id: &str| {
        yaml.replace(
            &format!("--- !u!4 &{id}"),
            "--- !u!4 &<GENERATED_TRANSFORM_ID>",
        )
        .replace(
            &format!("{{fileID: {id}}}"),
            "{fileID: <GENERATED_TRANSFORM_ID>}",
        )
    };
    assert_eq!(
        normalize_transform_id(&scene_yaml, &transform_id),
        normalize_transform_id(source_yaml, &source_transform_id)
    );
    let scene_meta = std::fs::read_to_string(scene_meta_path).unwrap();
    let source_meta =
        include_str!("../no_tree_scene_owned_missing_transform_source_scene.meta.yml");
    let normalize_meta = |text: &str| {
        text.lines()
            .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(normalize_meta(&scene_meta), normalize_meta(source_meta));
    assert_eq!(report.asset_file_count, 1);
    assert_eq!(report.meta_file_count, 1);
    assert_eq!(report.exported_object_count, 3);

    std::fs::remove_dir_all(output_root).unwrap();
}

fn scene_yaml_document_file_id(yaml: &str, class_id: i32) -> String {
    let prefix = format!("--- !u!{class_id} &");
    yaml.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .and_then(|tail| tail.split_whitespace().next())
        .expect("the expected Unity YAML document exists")
        .to_owned()
}
