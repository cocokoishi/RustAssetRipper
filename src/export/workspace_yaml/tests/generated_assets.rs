use super::*;

#[test]
fn prefab_executor_writes_a_planned_generated_transform_document() {
    const PREFAB_COLLECTION_INDEX: usize = 14;

    let bytes = make_game_object_without_transform_file();
    let (filesystem, workspace) = load_workspace_bytes(&bytes);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete());
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());

    let hierarchies = non_scene_hierarchies_for_workspace("main.assets", &bytes, &reference_graph);
    assert!(hierarchies.scenes().is_empty());
    assert_eq!(hierarchies.non_scene_collections().len(), 1);
    let processing = crate::processing::prefab_processing::plan_workspace_prefab_processing(
        &hierarchies,
        &[0],
        Default::default(),
    )
    .unwrap();
    assert!(processing.is_complete());
    let hierarchy = processing
        .events()
        .iter()
        .find_map(|event| {
            match event {
            crate::processing::prefab_processing::WorkspacePrefabProcessingEvent::
                GeneratedPrefabHierarchyCreated { hierarchy, .. } => Some(hierarchy),
            _ => None,
        }
        })
        .expect("non-scene root should create a generated Prefab hierarchy");
    let processed_assets =
        crate::processing::prefab_processing::plan_workspace_processed_prefab_assets(
            &hierarchies,
            &processing,
            &[0],
            &Default::default(),
            crate::processing::prefab_processing::WorkspaceProcessedPrefabAssetPlanLimits::default(
            ),
        )
        .unwrap();
    assert!(
        processed_assets.is_complete(),
        "{:?}",
        processed_assets.incomplete_reason()
    );

    let unity_version = SceneUnityVersion::parse("2019.4.0f1").unwrap();
    let source_asset =
        |identity: crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity| {
            ExportAssetIdentity {
                collection: ExportAssetCollectionIdentity::SerializedFile {
                    file_index: identity.file_index,
                },
                object_index: identity.object_index,
                path_id: identity.path_id,
                class_id: identity.class_id.unwrap(),
            }
        };
    let mut game_objects = Vec::new();
    let mut components = Vec::new();
    let missing_transforms = processed_assets
        .collections()
        .iter()
        .find(|collection| {
            collection.kind
                == crate::processing::prefab_processing::WorkspaceProcessedCollectionKind::
                    MissingPrefabTransforms
        })
        .expect("processed plan includes the Missing Prefab Transforms collection");
    for entry in hierarchy.entries() {
        match entry {
            crate::processing::prefab_processing::WorkspacePrefabHierarchyPlanEntry::Source(
                entry,
            ) => {
                let versioned = crate::export::prefab_collection::VersionedPrefabAsset {
                    identity: source_asset(entry.identity),
                    unity_version,
                };
                match entry.kind {
                    crate::processing::scene_hierarchy::
                        WorkspaceGameObjectHierarchyEntryKind::GameObject => {
                        game_objects.push(versioned);
                    }
                    crate::processing::scene_hierarchy::
                        WorkspaceGameObjectHierarchyEntryKind::Component => {
                        components.push(versioned);
                    }
                }
            }
            crate::processing::prefab_processing::WorkspacePrefabHierarchyPlanEntry::
                GeneratedTransform { game_object, .. } => {
                let (object_index, asset) = missing_transforms
                    .assets
                    .iter()
                    .enumerate()
                    .find(|(_, asset)| {
                        matches!(
                            asset.kind,
                            crate::processing::prefab_processing::
                                WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
                                    game_object: candidate,
                                    ..
                                } if candidate == *game_object
                        )
                    })
                    .expect("planned Prefab Transform has a materialized collection identity");
                components.push(crate::export::prefab_collection::VersionedPrefabAsset {
                    identity: ExportAssetIdentity {
                        collection: ExportAssetCollectionIdentity::ProcessedCollection {
                            collection_index: missing_transforms.collection_index,
                        },
                        object_index,
                        path_id: asset.identity.path_id,
                        class_id: asset.identity.class_id,
                    },
                    unity_version,
                });
            }
        }
    }

    let generated_prefabs = processed_assets
        .collections()
        .iter()
        .find(|collection| {
            collection.kind
                == crate::processing::prefab_processing::WorkspaceProcessedCollectionKind::
                    GeneratedPrefabs
        })
        .expect("processed plan includes the Generated Prefabs collection");
    let (prefab_object_index, prefab_asset) = generated_prefabs
        .assets
        .iter()
        .enumerate()
        .find(|(_, asset)| {
            matches!(
                asset.kind,
                crate::processing::prefab_processing::WorkspaceProcessedPrefabAssetKind::
                    GeneratedPrefabInstance { .. }
            )
        })
        .expect("generated Prefab hierarchy owns a PrefabInstance marker");
    let versioned_prefab_asset = crate::export::prefab_collection::VersionedPrefabAsset {
        identity: ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: generated_prefabs.collection_index,
            },
            object_index: prefab_object_index,
            path_id: prefab_asset.identity.path_id,
            class_id: prefab_asset.identity.class_id,
        },
        unity_version,
    };
    let prefab_hierarchies = processed_assets
        .collections()
        .iter()
        .find(|collection| {
            collection.kind
                == crate::processing::prefab_processing::WorkspaceProcessedCollectionKind::
                    PrefabHierarchies
        })
        .expect("processed plan includes the Prefab Hierarchies collection");
    let (hierarchy_object_index, hierarchy_asset) = prefab_hierarchies
        .assets
        .iter()
        .enumerate()
        .find(|(_, asset)| {
            matches!(
                asset.kind,
                crate::processing::prefab_processing::WorkspaceProcessedPrefabAssetKind::
                    PrefabHierarchy { .. }
            )
        })
        .expect("generated Prefab hierarchy has a registered helper asset");
    let versioned_hierarchy_asset = crate::export::prefab_collection::VersionedPrefabAsset {
        identity: ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: prefab_hierarchies.collection_index,
            },
            object_index: hierarchy_object_index,
            path_id: hierarchy_asset.identity.path_id,
            class_id: hierarchy_asset.identity.class_id,
        },
        unity_version,
    };
    let hidden_assets = [versioned_prefab_asset.identity.object_key()];
    let plan = crate::export::prefab_collection::plan_prefab_export_ids(
        crate::export::prefab_collection::PrefabHierarchyAssetGroups {
            game_objects: &game_objects,
            components: &components,
            prefab_instances: &[],
            prefab_asset: versioned_prefab_asset,
            hierarchy_asset: versioned_hierarchy_asset,
            representation: PrefabAssetRepresentation::PrefabInstanceMarker,
        },
        &hidden_assets,
        crate::export::prefab_collection::PrefabExportPlanLimits::default(),
    )
    .unwrap();
    assert!(!plan.emit_prefab_asset);

    let mut pointer_map_builder =
        ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
    pointer_map_builder
        .register_prefab_collection(
            PREFAB_COLLECTION_INDEX,
            [0x72; 16],
            crate::export::pointer_map::UnityProjectAssetType::Meta,
            &plan.registered_assets,
        )
        .unwrap();
    let pointer_map = pointer_map_builder.build().unwrap();
    let transform_metadata = generated_transform_metadata();
    let generated_context = WorkspaceGeneratedTransformYamlContext::for_prefabs(
        &processed_assets,
        &transform_metadata,
        0,
    );
    let output = workspace_prefab_hierarchy_to_unity_yaml_with_generated_transforms(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &plan,
        PREFAB_COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
        &generated_context,
    )
    .unwrap();

    let game_object_file_id = plan
        .registered_assets
        .iter()
        .find(|assignment| assignment.asset.class_id == 1)
        .unwrap()
        .file_id;
    let transform_file_id = plan
        .registered_assets
        .iter()
        .find(|assignment| assignment.asset.class_id == 4)
        .unwrap()
        .file_id;
    assert!(output.contains(&format!("--- !u!1 &{game_object_file_id}")));
    assert!(output.contains(&format!("--- !u!4 &{transform_file_id}")));
    assert!(output.contains(&format!("component: {{fileID: {transform_file_id}}}")));
    assert!(output.contains(&format!("m_GameObject: {{fileID: {game_object_file_id}}}")));
    assert!(!output.contains("!u!1001"), "{output}");
}

#[test]
fn collection_executor_reports_processed_assets_without_a_generated_walker() {
    let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    let processed = SceneExportIdAssignment {
        asset: ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: 5,
            },
            object_index: 0,
            path_id: 1,
            class_id: 4,
        },
        file_id: 123,
    };
    assert!(matches!(
        workspace_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map(),
            &[processed],
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        ),
        Err(
            UnityYamlWorkspaceCollectionError::ProcessedAssetRequiresGeneratedWalker {
                assignment_index: 0,
                ..
            }
        )
    ));
}

#[test]
fn scene_executor_writes_a_planned_generated_transform_document() {
    let bytes = make_game_object_without_transform_file();
    let (filesystem, workspace) = load_workspace_bytes(&bytes);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete());
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());

    let hierarchies = scene_hierarchies_for_workspace(&[("main.assets", &bytes)], &reference_graph);
    let processing = crate::processing::prefab_processing::plan_workspace_prefab_processing(
        &hierarchies,
        &[0],
        Default::default(),
    )
    .unwrap();
    assert!(processing.is_complete());
    let processed_assets =
        crate::processing::prefab_processing::plan_workspace_processed_prefab_assets(
            &hierarchies,
            &processing,
            &[0],
            &Default::default(),
            crate::processing::prefab_processing::WorkspaceProcessedPrefabAssetPlanLimits::default(
            ),
        )
        .unwrap();
    assert!(
        processed_assets.is_complete(),
        "{:?}",
        processed_assets.incomplete_reason()
    );

    let assignments =
        crate::export::scene_collection::plan_workspace_scene_export_ids_with_processed_assets(
            &hierarchies.scenes()[0],
            0,
            &processed_assets,
            SceneUnityVersion {
                major: 2019,
                minor: 4,
                patch: 0,
            },
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyMembershipLimits::default(),
            crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
            crate::export::scene_collection::SceneExportIdPlanLimits::default(),
        )
        .unwrap();
    let generated_transform = assignments
        .iter()
        .find(|assignment| {
            assignment.asset.class_id == 4
                && matches!(
                    assignment.asset.collection,
                    ExportAssetCollectionIdentity::ProcessedCollection { .. }
                )
        })
        .copied()
        .expect("scene export planner should include the synthesized Transform");

    let mut pointer_map_builder =
        ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
    pointer_map_builder
        .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments)
        .unwrap();
    let pointer_map = pointer_map_builder.build().unwrap();
    let transform_metadata = generated_transform_metadata();
    let generated_context =
        WorkspaceGeneratedTransformYamlContext::new(&processed_assets, 0, &transform_metadata, 0);
    let output = workspace_scene_assignments_to_unity_yaml_with_generated_transforms(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments,
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
        &generated_context,
    )
    .unwrap();

    assert!(output.contains(&format!("--- !u!4 &{}", generated_transform.file_id)));
    assert!(output.contains("m_GameObject: {fileID: 42}"), "{output}");
    assert!(
        output.contains(&format!(
            "component: {{fileID: {}}}",
            generated_transform.file_id
        )),
        "{output}"
    );
    assert!(
        !output.contains("m_GameObject: {fileID: 42, guid:"),
        "{output}"
    );
    assert!(
        output.contains("m_LocalScale:\n    x: 1\n    y: 1\n    z: 1"),
        "{output}"
    );
}
