use super::*;

#[test]
fn collection_executor_writes_documents_in_supplied_yaml_order() {
    let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert_eq!(objects.get(0, 0).unwrap().path_id, GAME_OBJECT_PATH_ID);
    assert_eq!(objects.get(0, 1).unwrap().path_id, TRANSFORM_PATH_ID);
    assert!(objects.get(1, 0).is_none());
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    let assignments = [
        SceneExportIdAssignment {
            asset: asset(1, TRANSFORM_PATH_ID, 4),
            file_id: TRANSFORM_PATH_ID,
        },
        SceneExportIdAssignment {
            asset: asset(0, GAME_OBJECT_PATH_ID, 1),
            file_id: GAME_OBJECT_PATH_ID,
        },
    ];
    let output = workspace_assignments_to_unity_yaml(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map(),
        &assignments,
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
    )
    .unwrap();
    let transform_position = output.find("--- !u!4 &43").unwrap();
    let game_object_position = output.find("--- !u!1 &42").unwrap();
    assert!(transform_position < game_object_position);
    assert!(output.contains("m_GameObject: {fileID: 42}"));
    assert!(output.contains("component: {fileID: 43}"));
}

#[test]
fn scene_executor_applies_scene_ordering_before_writing() {
    let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    let assignments = [
        SceneExportIdAssignment {
            asset: asset(0, GAME_OBJECT_PATH_ID, 1),
            file_id: GAME_OBJECT_PATH_ID,
        },
        SceneExportIdAssignment {
            asset: asset(1, TRANSFORM_PATH_ID, 4),
            file_id: TRANSFORM_PATH_ID,
        },
    ];
    let output = workspace_scene_assignments_to_unity_yaml(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map(),
        &assignments,
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
    )
    .unwrap();
    assert!(output.find("--- !u!1 &42").unwrap() < output.find("--- !u!4 &43").unwrap());
}

#[test]
fn modern_prefab_executor_omits_hidden_marker_and_preserves_hierarchy_order() {
    const PREFAB_COLLECTION_INDEX: usize = 13;

    let bytes = make_game_object_mesh_renderer_file();
    let (filesystem, workspace) = load_workspace_bytes(&bytes);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete());
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());

    let unity_version = SceneUnityVersion::parse("2019.4.0f1").unwrap();
    let game_objects = [crate::export::prefab_collection::VersionedPrefabAsset {
        identity: asset(0, GAME_OBJECT_PATH_ID, 1),
        unity_version,
    }];
    let components = [crate::export::prefab_collection::VersionedPrefabAsset {
        identity: asset(1, 9001, 23),
        unity_version,
    }];
    let prefab_asset = crate::export::prefab_collection::VersionedPrefabAsset {
        identity: ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: 8,
            },
            object_index: 0,
            path_id: 1,
            class_id: 1001,
        },
        unity_version,
    };
    let hierarchy_asset = crate::export::prefab_collection::VersionedPrefabAsset {
        identity: ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: 9,
            },
            object_index: 0,
            path_id: 1,
            class_id: 1001,
        },
        unity_version,
    };
    let make_plan =
        |representation,
         hidden_assets: &[crate::export::scene_collection::ExportAssetObjectKey]| {
            crate::export::prefab_collection::plan_prefab_export_ids(
                crate::export::prefab_collection::PrefabHierarchyAssetGroups {
                    game_objects: &game_objects,
                    components: &components,
                    prefab_instances: &[],
                    prefab_asset,
                    hierarchy_asset,
                    representation,
                },
                hidden_assets,
                crate::export::prefab_collection::PrefabExportPlanLimits::default(),
            )
            .unwrap()
        };
    let modern_plan = make_plan(
        PrefabAssetRepresentation::PrefabInstanceMarker,
        &[prefab_asset.identity.object_key()],
    );
    assert!(!modern_plan.emit_prefab_asset);
    assert_eq!(
        modern_plan.exportable_assets,
        [game_objects[0].identity, components[0].identity]
    );

    let mut pointer_map_builder =
        ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
    pointer_map_builder
        .register_prefab_collection(
            PREFAB_COLLECTION_INDEX,
            [0x7d; 16],
            UnityProjectAssetType::Meta,
            &modern_plan.registered_assets,
        )
        .unwrap();
    let pointer_map = pointer_map_builder.build().unwrap();
    let output = workspace_prefab_hierarchy_to_unity_yaml(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &modern_plan,
        PREFAB_COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
    )
    .unwrap();

    let game_object_file_id = modern_plan
        .registered_assets
        .iter()
        .find(|assignment| assignment.asset == game_objects[0].identity)
        .unwrap()
        .file_id;
    let component_file_id = modern_plan
        .registered_assets
        .iter()
        .find(|assignment| assignment.asset == components[0].identity)
        .unwrap()
        .file_id;
    assert!(
        output
            .find(&format!("--- !u!1 &{game_object_file_id}"))
            .unwrap()
            < output
                .find(&format!("--- !u!23 &{component_file_id}"))
                .unwrap()
    );
    assert!(output.contains(&format!("component: {{fileID: {component_file_id}}}")));
    assert!(output.contains(&format!("m_GameObject: {{fileID: {game_object_file_id}}}")));
    assert!(!output.contains("!u!1001"), "{output}");

    let legacy_plan = make_plan(PrefabAssetRepresentation::LegacyPrefabMarker, &[]);
    assert!(legacy_plan.emit_prefab_asset);
    assert!(matches!(
        workspace_prefab_hierarchy_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &legacy_plan,
            PREFAB_COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        ),
        Err(
            UnityYamlWorkspaceCollectionError::ProcessedAssetRequiresGeneratedWalker {
                assignment_index: 2,
                asset: ExportAssetIdentity {
                    collection: ExportAssetCollectionIdentity::ProcessedCollection { .. },
                    class_id: 1001,
                    ..
                },
            }
        )
    ));
}
