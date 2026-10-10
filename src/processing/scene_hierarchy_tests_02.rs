    #[test]
    fn prefab_hierarchy_plan_inserts_missing_transform_after_source_components() {
        let mut workspace = workspace_hierarchy_fixture(false);
        let root_graph = &mut workspace.non_scene_collections[0];
        root_graph.game_object_hierarchies[0].component_relationship_end = 2;
        root_graph
            .game_object_component_relationship_indices
            .truncate(2);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let plan =
            plan_workspace_prefab_processing(&workspace, &[0, 1], Default::default()).unwrap();

        assert!(plan.is_complete());
        assert_eq!(plan.report().generated_transform_components, 1);
        let WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyCreated { hierarchy, .. } =
            &plan.events()[0]
        else {
            panic!("expected a generated hierarchy event")
        };
        assert_eq!(hierarchy.entries().len(), 3);
        assert!(matches!(
            hierarchy.entries()[0],
            crate::processing::prefab_processing::WorkspacePrefabHierarchyPlanEntry::Source(
                WorkspaceGameObjectHierarchyEntry {
                    identity,
                    kind: WorkspaceGameObjectHierarchyEntryKind::GameObject,
                }
            ) if identity == root
        ));
        assert!(matches!(
            hierarchy.entries()[1],
            crate::processing::prefab_processing::WorkspacePrefabHierarchyPlanEntry::Source(
                WorkspaceGameObjectHierarchyEntry {
                    identity: component_identity,
                    kind: WorkspaceGameObjectHierarchyEntryKind::Component,
                }
            ) if component_identity == identity(0, 3, 4, 23)
        ));
        assert!(matches!(
            &hierarchy.entries()[2],
            crate::processing::prefab_processing::WorkspacePrefabHierarchyPlanEntry::GeneratedTransform {
                game_object,
                default_values,
            } if *game_object == root
                && default_values.local_scale.x == 1.0
                && default_values.local_rotation.w == 1.0
        ));
    }

    #[test]
    fn prefab_processing_stops_after_an_incomplete_hierarchy_walk() {
        let mut workspace = workspace_hierarchy_fixture(false);
        workspace.non_scene_collections[1].source_reference_scans_complete = false;
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let plan =
            plan_workspace_prefab_processing(&workspace, &[0, 1], Default::default()).unwrap();

        assert!(!plan.is_complete());
        assert!(plan.stopped_on_incomplete_hierarchy());
        assert_eq!(plan.processed_game_objects(), &[root]);
        assert!(matches!(
            &plan.events()[0],
            WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyIncomplete {
                root_game_object,
                hierarchy,
                ..
            } if *root_game_object == root && !hierarchy.is_complete()
        ));
        assert_eq!(plan.events().len(), 1);
    }

    #[test]
    fn prefab_processing_enforces_event_and_hierarchy_entry_limits() {
        let workspace = workspace_hierarchy_fixture(false);
        assert!(matches!(
            plan_workspace_prefab_processing(
                &workspace,
                &[0, 1],
                crate::processing::prefab_processing::WorkspacePrefabProcessingLimits {
                    max_events: 0,
                    ..Default::default()
                }
            ),
            Err(crate::processing::prefab_processing::WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "Prefab processing events",
                requested: 1,
                limit: 0,
            })
        ));
        assert!(matches!(
            plan_workspace_prefab_processing(
                &workspace,
                &[0, 1],
                crate::processing::prefab_processing::WorkspacePrefabProcessingLimits {
                    max_total_hierarchy_entries: 1,
                    ..Default::default()
                }
            ),
            Err(crate::processing::prefab_processing::WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "GameObject hierarchy entries",
                requested: 2,
                limit: 1,
            })
        ));

        let mut missing_transform_workspace = workspace_hierarchy_fixture(false);
        missing_transform_workspace.non_scene_collections[0].game_object_hierarchies[0]
            .component_relationship_end = 2;
        missing_transform_workspace.non_scene_collections[0]
            .game_object_component_relationship_indices
            .truncate(2);
        assert!(matches!(
            plan_workspace_prefab_processing(
                &missing_transform_workspace,
                &[0, 1],
                crate::processing::prefab_processing::WorkspacePrefabProcessingLimits {
                    max_total_hierarchy_entries: 2,
                    ..Default::default()
                }
            ),
            Err(crate::processing::prefab_processing::WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "Prefab hierarchy plan entries",
                requested: 3,
                limit: 2,
            })
        ));
    }

    #[test]
    fn processed_prefab_asset_plan_uses_independent_collection_path_id_sequences() {
        let workspace = workspace_hierarchy_fixture(false);
        let processing = plan_workspace_prefab_processing(
            &workspace,
            &[0, 1],
            crate::processing::prefab_processing::WorkspacePrefabProcessingLimits::default(),
        )
        .unwrap();
        assert!(processing.is_complete());
        assert_eq!(processing.source_collection_file_indices(), &[0, 1]);

        let allocations = plan_workspace_processed_prefab_assets(
            &workspace,
            &processing,
            &[0, 0],
            &Default::default(),
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        assert!(
            allocations.is_complete(),
            "{:?}",
            allocations.incomplete_reason()
        );
        assert_eq!(allocations.asset_count(), 2);
        assert_eq!(allocations.collections().len(), 3);
        assert_eq!(
            allocations.collections()[0].kind,
            WorkspaceProcessedCollectionKind::PrefabHierarchies
        );
        assert_eq!(
            allocations.collections()[1].kind,
            WorkspaceProcessedCollectionKind::GeneratedPrefabs
        );
        assert_eq!(
            allocations.collections()[2].kind,
            WorkspaceProcessedCollectionKind::MissingPrefabTransforms
        );

        let generated_prefab = &allocations.collections()[1].assets[0];
        assert_eq!(generated_prefab.identity.path_id, 1);
        assert_eq!(generated_prefab.identity.class_id, 1001);
        let hierarchy = &allocations.collections()[0].assets[0];
        assert_eq!(hierarchy.identity.path_id, 1);
        assert_eq!(hierarchy.identity.class_id, 1001);
        assert!(matches!(
            hierarchy.kind,
            WorkspaceProcessedPrefabAssetKind::PrefabHierarchy {
                prefab: WorkspaceProcessedPrefabReference::Generated(identity),
                ..
            } if identity == generated_prefab.identity
        ));
    }

    #[test]
    fn incomplete_prefab_processing_does_not_claim_generated_path_ids() {
        let workspace = workspace_hierarchy_fixture(true);
        let processing = plan_workspace_prefab_processing(
            &workspace,
            &[0, 1],
            crate::processing::prefab_processing::WorkspacePrefabProcessingLimits::default(),
        )
        .unwrap();
        assert!(!processing.is_complete());

        let allocations = plan_workspace_processed_prefab_assets(
            &workspace,
            &processing,
            &[0, 0],
            &Default::default(),
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        assert!(!allocations.is_complete());
        assert!(allocations.collections().is_empty());
        assert_eq!(allocations.asset_count(), 0);
        assert!(allocations
            .incomplete_reason()
            .unwrap()
            .contains("identity plan is incomplete"));
    }

    #[test]
    fn generated_transform_gets_its_source_collection_path_id_before_prefab_assets() {
        let game_object = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 0,
            scene_objects: Vec::new(),
            relationships: vec![relationship(
                game_object,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[0]",
                0,
                SceneRelationshipTarget::Null,
            )],
            game_object_hierarchies: vec![WorkspaceSceneGameObjectHierarchy {
                game_object,
                component_relationship_start: 0,
                component_relationship_end: 1,
            }],
            game_object_component_relationship_indices: vec![0],
            prefab_root_relationship_index: Vec::new(),
            transform_hierarchies: Vec::new(),
            transform_parent_relationship_indices: Vec::new(),
            transform_child_relationship_indices: Vec::new(),
            transform_game_object_relationship_indices: Vec::new(),
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![WorkspaceSceneAssetIdentity {
                identity: game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            }],
            component_candidates: Vec::new(),
            missing_transform_plans: vec![WorkspaceMissingTransformPlan {
                game_object,
                default_values: default_workspace_generated_transform_values(),
            }],
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };
        let workspace = WorkspaceSceneHierarchies {
            scenes: Vec::new(),
            non_scene_collections: vec![collection],
            collection_graph_index: vec![WorkspaceSceneCollectionGraphIndexEntry {
                file_index: 0,
                scene_index: None,
                collection_index: 0,
            }],
        };
        let processing = plan_workspace_prefab_processing(
            &workspace,
            &[0],
            crate::processing::prefab_processing::WorkspacePrefabProcessingLimits::default(),
        )
        .unwrap();
        assert!(processing.is_complete());

        let allocations = plan_workspace_processed_prefab_assets(
            &workspace,
            &processing,
            &[0],
            &Default::default(),
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        assert!(
            allocations.is_complete(),
            "{:?}",
            allocations.incomplete_reason()
        );
        assert_eq!(allocations.asset_count(), 3);
        let missing_transform_collection = &allocations.collections()[2];
        assert_eq!(missing_transform_collection.assets.len(), 1);
        assert_eq!(missing_transform_collection.assets[0].identity.path_id, 1);
        assert_eq!(missing_transform_collection.assets[0].identity.class_id, 4);
        assert!(matches!(
            missing_transform_collection.assets[0].kind,
            WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
                game_object: actual,
                default_values,
            } if actual == game_object
                && default_values == default_workspace_generated_transform_values()
        ));
        assert_eq!(allocations.component_additions().len(), 1);
        let component_addition = allocations.component_additions()[0];
        assert_eq!(component_addition.game_object, game_object);
        assert_eq!(component_addition.component_class_id, 4);
        assert_eq!(component_addition.appended_index, 1);
        assert_eq!(
            component_addition.component_asset,
            missing_transform_collection.assets[0].identity
        );
        let component_overlay = &allocations.component_list_overlays()[0];
        assert_eq!(component_overlay.game_object, game_object);
        assert_eq!(component_overlay.source_slots.len(), 1);
        assert_eq!(component_overlay.source_slots[0].source_array_index, 0);
        assert_eq!(component_overlay.source_slots[0].file_id, 0);
        assert_eq!(component_overlay.source_slots[0].path_id, 0);
        assert_eq!(
            component_overlay.source_slots[0].target,
            SceneRelationshipTarget::Null
        );
        assert_eq!(component_overlay.generated_component, component_addition);
        assert_eq!(allocations.source_dependency_appends().len(), 1);
        let source_dependency_append = allocations.source_dependency_appends()[0];
        assert_eq!(source_dependency_append.source_file_index, 0);
        assert_eq!(
            source_dependency_append.target_collection,
            crate::processing::prefab_processing::WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                collection_index: 2
            }
        );
        assert_eq!(
            source_dependency_append.pointer_field,
            crate::processing::prefab_processing::WorkspaceSourceDependencyPointerField::GameObjectComponent
        );
        assert_eq!(source_dependency_append.first_source_owner, game_object);
        assert_eq!(
            source_dependency_append.target_asset,
            Some(missing_transform_collection.assets[0].identity)
        );
        assert_eq!(source_dependency_append.file_id, Some(1));
        assert_eq!(allocations.constructor_pointers().len(), 2);
        assert_eq!(
            allocations.constructor_pointers()[0].field,
            crate::processing::prefab_processing::WorkspaceGeneratedPointerField::TransformGameObject
        );
        assert_eq!(allocations.constructor_pointers()[0].file_id, 1);
        assert_eq!(
            allocations.constructor_pointers()[0].path_id,
            game_object.path_id
        );
        assert_eq!(allocations.constructor_pointers()[0].target, game_object);
        assert_eq!(
            allocations.constructor_pointers()[1].field,
            crate::processing::prefab_processing::WorkspaceGeneratedPointerField::PrefabInstanceRootGameObject
        );
        assert_eq!(allocations.constructor_pointers()[1].file_id, 1);
        assert_eq!(
            allocations.constructor_pointers()[1].path_id,
            game_object.path_id
        );
        assert_eq!(allocations.constructor_pointers()[1].target, game_object);
        for collection_index in [1, 2] {
            let dependencies = &allocations.collections()[collection_index].dependencies;
            assert_eq!(dependencies.len(), 2);
            assert_eq!(dependencies[0].file_id, 0);
            assert_eq!(
                dependencies[0].target,
                crate::processing::prefab_processing::WorkspaceProcessedDependencyTarget::ProcessedCollection {
                    collection_index
                }
            );
            assert_eq!(dependencies[1].file_id, 1);
            assert_eq!(
                dependencies[1].target,
                crate::processing::prefab_processing::WorkspaceProcessedDependencyTarget::SerializedFileCollection {
                    file_index: 0
                }
            );
        }
        assert!(!allocations.collections()[0].dependency_indices_complete);
        assert_eq!(allocations.collections()[1].assets[0].identity.path_id, 1);
        assert_eq!(allocations.collections()[0].assets[0].identity.path_id, 1);
    }

    #[test]
    fn lighting_data_dependency_file_id_precedes_missing_transform_component_file_id() {
        let game_object = identity(0, 0, 100, GAME_OBJECT_CLASS_ID);
        let lightmap_settings = identity(0, 1, 101, LIGHTMAP_SETTINGS_CLASS_ID);
        let render_settings = identity(0, 2, 102, SceneManagerKind::RenderSettings.class_id());
        let mut collection = empty_collection_graph(0);
        collection.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: lightmap_settings,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::LightmapSettings,
                ),
            },
            WorkspaceSceneAssetIdentity {
                identity: render_settings,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::RenderSettings,
                ),
            },
        ];
        collection.lighting_data_asset_field_object_indices = vec![lightmap_settings.object_index];
        collection.game_object_hierarchies = vec![WorkspaceSceneGameObjectHierarchy {
            game_object,
            component_relationship_start: 0,
            component_relationship_end: 0,
        }];
        collection.missing_transform_plans = vec![WorkspaceMissingTransformPlan {
            game_object,
            default_values: default_workspace_generated_transform_values(),
        }];
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![collection],
        };
        let workspace = WorkspaceSceneHierarchies {
            scenes: vec![scene],
            non_scene_collections: Vec::new(),
            collection_graph_index: vec![WorkspaceSceneCollectionGraphIndexEntry {
                file_index: 0,
                scene_index: Some(0),
                collection_index: 0,
            }],
        };
        let processing =
            plan_workspace_prefab_processing(&workspace, &[0], Default::default()).unwrap();
        assert!(processing.is_complete());
        let allocations = plan_workspace_processed_prefab_assets(
            &workspace,
            &processing,
            &[3],
            &Default::default(),
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        assert!(allocations.is_complete());

        let appends = allocations.source_dependency_appends();
        assert_eq!(appends.len(), 2);
        assert_eq!(appends[0].source_file_index, 0);
        assert_eq!(
            appends[0].target_collection,
            WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets
        );
        assert_eq!(
            appends[0].pointer_field,
            WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset
        );
        assert_eq!(appends[0].first_source_owner, lightmap_settings);
        assert_eq!(appends[0].file_id, Some(4));
        assert_eq!(
            appends[1].target_collection,
            WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                collection_index: 3
            }
        );
        assert_eq!(appends[1].first_source_owner, game_object);
        assert_eq!(appends[1].file_id, Some(5));
    }

    #[test]
    fn scene_export_plan_includes_generated_transforms_and_keeps_yaml_sort_separate() {
        let game_object = identity(0, 0, 100, GAME_OBJECT_CLASS_ID);
        let render_settings = identity(0, 1, 102, SceneManagerKind::RenderSettings.class_id());
        let mut collection = empty_collection_graph(0);
        collection.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: render_settings,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::RenderSettings,
                ),
            },
        ];
        collection.game_object_hierarchies = vec![WorkspaceSceneGameObjectHierarchy {
            game_object,
            component_relationship_start: 0,
            component_relationship_end: 0,
        }];
        collection.missing_transform_plans = vec![WorkspaceMissingTransformPlan {
            game_object,
            default_values: default_workspace_generated_transform_values(),
        }];
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![collection],
        };
        let workspace = WorkspaceSceneHierarchies {
            scenes: vec![scene],
            non_scene_collections: Vec::new(),
            collection_graph_index: vec![WorkspaceSceneCollectionGraphIndexEntry {
                file_index: 0,
                scene_index: Some(0),
                collection_index: 0,
            }],
        };
        let processing =
            plan_workspace_prefab_processing(&workspace, &[0], Default::default()).unwrap();
        assert!(processing.is_complete());
        let processed_assets = plan_workspace_processed_prefab_assets(
            &workspace,
            &processing,
            &[3],
            &Default::default(),
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        assert!(processed_assets.is_complete());

        let unity_version = crate::processing::scene_naming::SceneUnityVersion {
            major: 2019,
            minor: 4,
            patch: 0,
        };
        let hierarchy_order =
            crate::export::scene_collection::plan_workspace_scene_export_ids_with_processed_assets(
                &workspace.scenes()[0],
                0,
                &processed_assets,
                unity_version,
                WorkspaceSceneHierarchyMembershipLimits::default(),
                crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
                crate::export::scene_collection::SceneExportIdPlanLimits::default(),
            )
            .unwrap();
        assert_eq!(hierarchy_order.len(), 3);
        assert_eq!(hierarchy_order[0].asset.path_id, game_object.path_id);
        assert_eq!(
            hierarchy_order[1].asset.collection,
            crate::export::scene_collection::ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: 3
            }
        );
        assert_eq!(hierarchy_order[1].asset.object_index, 0);
        assert_eq!(hierarchy_order[1].asset.class_id, TRANSFORM_CLASS_ID);
        assert_eq!(
            hierarchy_order[1].file_id,
            crate::export::ids::get_pseudo_random_export_id(TRANSFORM_CLASS_ID, 0, unity_version)
        );
        assert_eq!(hierarchy_order[2].asset.path_id, render_settings.path_id);

        let yaml_order = crate::export::scene_collection::order_scene_yaml_assignments(
            &hierarchy_order,
            crate::export::scene_collection::SceneYamlAssetOrderLimits::default(),
        )
        .unwrap();
        assert_eq!(yaml_order.len(), 3);
        assert_eq!(yaml_order[0].asset.path_id, render_settings.path_id);
        assert_eq!(yaml_order[1].asset.path_id, game_object.path_id);
        assert_eq!(
            yaml_order[2].asset.collection,
            hierarchy_order[1].asset.collection
        );
        assert_eq!(yaml_order[2].file_id, hierarchy_order[1].file_id);
    }
