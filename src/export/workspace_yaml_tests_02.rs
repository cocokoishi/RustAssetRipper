    #[test]
    fn workspace_yaml_export_reports_missing_streamed_resource_without_null_or_empty_data() {
        let source_bytes = make_streamed_asset_file(189, true);
        let (filesystem, workspace) = load_workspace_bytes(&source_bytes);
        let empty_inputs: [ExplicitRootResourceInput<'_, '_>; 0] = [];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &empty_inputs,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let identity = asset(0, 77, 189);
        let pointer_map = pointer_map_for_asset(identity);
        let provider = PlatformResourceProvider::new(None, None);
        let context = WorkspaceStreamedYamlResourceContext {
            filesystem: &filesystem,
            root_catalog: &catalog,
            provider: &provider,
            content_limits: StreamedResourceContentLimits::default(),
            staging_limits: StreamedYamlAssetLimits::default(),
        };

        let no_context_error = workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            identity,
            77,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap_err();
        match no_context_error {
            UnityYamlWorkspaceExportError::StreamedAssetPreparation { source, .. } => {
                assert!(matches!(
                    *source,
                    StreamedYamlAssetError::StreamContentRequired { resource }
                        if resource.path == "texture.resS" && resource.offset == 2 && resource.size == 3
                ))
            }
            other => panic!("expected missing resource context error, got {other}"),
        }

        let copy_limited_context = WorkspaceStreamedYamlResourceContext {
            staging_limits: StreamedYamlAssetLimits {
                max_source_value_copy_bytes: 1,
                ..StreamedYamlAssetLimits::default()
            },
            ..context
        };
        let copy_limit_error = workspace_object_to_unity_yaml_document_with_streamed_resources(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            identity,
            77,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
            &copy_limited_context,
        )
        .unwrap_err();
        match copy_limit_error {
            UnityYamlWorkspaceExportError::StreamedAssetPreparation { source, .. } => {
                assert!(matches!(
                    *source,
                    StreamedYamlAssetError::SourceValueCopyLimitExceeded {
                        size,
                        limit: 1
                    } if size > 1
                ))
            }
            other => panic!("expected source-copy bound, got {other}"),
        }

        assert!(matches!(
            workspace_object_to_unity_yaml_document_with_streamed_resources(
                &workspace,
                &reference_graph,
                &pointer_map,
                &objects.objects()[0],
                identity,
                77,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
                &context,
            ),
            Err(UnityYamlWorkspaceExportError::StreamedResourceMissing {
                path,
                unavailable_data_paths,
                ..
            }) if path == "texture.resS" && unavailable_data_paths.is_empty()
        ));
        assert_eq!(
            objects.objects()[0]
                .payload
                .as_ref()
                .unwrap()
                .root
                .unique_field_in_base_chain("m_ImageData_C189")
                .unwrap()
                .unwrap(),
            &crate::serialized_file::object::SerializedValue::Bytes(Vec::new())
        );
    }

    #[test]
    fn exports_workspace_gameobject_component_and_transform_back_reference() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        assert_eq!(
            reference_graph.references()[0].field_path,
            "m_Component[0].component"
        );
        assert_eq!(
            reference_graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 1,
            }
        );

        let pointer_map = pointer_map();
        let game_object = workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            asset(0, GAME_OBJECT_PATH_ID, 1),
            GAME_OBJECT_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let transform = workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[1],
            asset(1, TRANSFORM_PATH_ID, 4),
            TRANSFORM_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[game_object, transform],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("--- !u!1 &42"));
        assert!(output.contains("serializedVersion: 6"));
        assert!(output.contains("component: {fileID: 43}"));
        assert!(output.contains("m_GameObject: {fileID: 42}"));
    }

    #[test]
    fn collection_executor_writes_documents_in_supplied_yaml_order() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
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
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
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
    fn scene_component_membership_ids_flow_through_workspace_yaml_export() {
        const COMPONENT_CLASS_ID: i32 = 23;
        const COMPONENT_PATH_ID: i64 = 9001;

        let bytes = make_mesh_renderer_component_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let assignments = plan_single_file_scene_assignments(&bytes, &reference_graph);
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].file_id, COMPONENT_PATH_ID);
        assert_eq!(assignments[0].asset.class_id, COMPONENT_CLASS_ID);

        let mut pointer_map_builder =
            ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments)
            .unwrap();
        let pointer_map = pointer_map_builder.build().unwrap();
        let output = workspace_scene_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments,
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();

        assert!(output.contains("--- !u!23 &9001"));
        assert!(output.contains("MeshRenderer:"));
        assert!(output.contains("m_GameObject: {fileID: 0}"));
    }

    #[test]
    fn scene_gameobject_component_nonnull_pptrs_follow_ids_into_yaml() {
        let bytes = make_game_object_mesh_renderer_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        assert_eq!(reference_graph.file_object_count(0), Some(2));
        assert_eq!(
            reference_graph.references()[0].resolution,
            crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 1,
            }
        );
        assert_eq!(
            reference_graph.references()[1].resolution,
            crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 0,
            }
        );

        let assignments = plan_single_file_scene_assignments(&bytes, &reference_graph);
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| (assignment.asset.object_index, assignment.file_id))
                .collect::<Vec<_>>(),
            [(0, GAME_OBJECT_PATH_ID), (1, 9001)]
        );
        let mut pointer_map_builder =
            ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments)
            .unwrap();
        let pointer_map = pointer_map_builder.build().unwrap();
        let output = workspace_scene_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments,
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();

        assert!(output.find("--- !u!1 &42").unwrap() < output.find("--- !u!23 &9001").unwrap());
        assert!(output.contains("component: {fileID: 9001}"));
        assert!(output.contains("m_GameObject: {fileID: 42}"));
    }

    #[test]
    fn modern_prefab_executor_omits_hidden_marker_and_preserves_hierarchy_order() {
        const PREFAB_COLLECTION_INDEX: usize = 13;

        let bytes = make_game_object_mesh_renderer_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
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

    #[test]
    fn prefab_executor_writes_a_planned_generated_transform_document() {
        const PREFAB_COLLECTION_INDEX: usize = 14;

        let bytes = make_game_object_without_transform_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());

        let hierarchies =
            non_scene_hierarchies_for_workspace("main.assets", &bytes, &reference_graph);
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
                crate::processing::prefab_processing::WorkspaceProcessedPrefabAssetPlanLimits::
                    default(),
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
    fn scene_component_pointers_to_other_collections_emit_guid_and_type() {
        let bytes = make_game_object_mesh_renderer_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let assignments = plan_single_file_scene_assignments(&bytes, &reference_graph);
        assert_eq!(assignments.len(), 2);

        let mut pointer_map_builder =
            ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments[..1])
            .unwrap();
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX + 1, [0x82; 16], &assignments[1..])
            .unwrap();
        let pointer_map = pointer_map_builder.build().unwrap();
        let game_object_yaml = workspace_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments[..1],
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let mesh_renderer_yaml = workspace_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments[1..],
            COLLECTION_INDEX + 1,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();

        assert!(
            game_object_yaml.contains(
                "component: {fileID: 9001, guid: 28282828282828282828282828282828, type: 2}"
            ),
            "{game_object_yaml}"
        );
        assert!(
            mesh_renderer_yaml.contains(
                "m_GameObject: {fileID: 42, guid: 17171717171717171717171717171717, type: 2}"
            ),
            "{mesh_renderer_yaml}"
        );
    }

    #[test]
    fn cross_file_component_pptrs_reach_separate_scene_yaml_collections() {
        let game_object_bytes = make_game_object_with_external_component();
        let mesh_renderer_bytes = make_mesh_renderer_with_external_game_object();
        let source_files = [
            ("main.assets", game_object_bytes.as_slice()),
            ("shared.assets", mesh_renderer_bytes.as_slice()),
        ];
        let (filesystem, workspace) = load_workspace_files(&source_files);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        assert_eq!(reference_graph.file_object_count(0), Some(1));
        assert_eq!(reference_graph.file_object_count(1), Some(1));
        assert_eq!(reference_graph.references().len(), 2);
        assert_eq!(
            reference_graph.references()[0].resolution,
            crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
        assert_eq!(
            reference_graph.references()[1].resolution,
            crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 0,
            }
        );

        let assignments = plan_scene_assignments_for_workspace(&source_files, &reference_graph);
        assert_eq!(assignments.len(), 2);
        assert_eq!(assignments[0].asset.path_id, 42);
        assert_eq!(assignments[1].asset.path_id, 9001);
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| assignment.file_id)
                .collect::<Vec<_>>(),
            [42, 9001]
        );

        let mut pointer_map_builder =
            ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments[..1])
            .unwrap();
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX + 1, [0x82; 16], &assignments[1..])
            .unwrap();
        let pointer_map = pointer_map_builder.build().unwrap();
        let game_object_yaml = workspace_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments[..1],
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let mesh_renderer_yaml = workspace_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments[1..],
            COLLECTION_INDEX + 1,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();

        assert!(game_object_yaml.contains("--- !u!1 &42"));
        assert!(mesh_renderer_yaml.contains("--- !u!23 &9001"));
        assert!(game_object_yaml.contains(
            "component: {fileID: 9001, guid: 28282828282828282828282828282828, type: 2}"
        ));
        assert!(mesh_renderer_yaml.contains(
            "m_GameObject: {fileID: 42, guid: 17171717171717171717171717171717, type: 2}"
        ));
    }

    #[test]
    fn pptr_dictionary_values_resolve_and_emit_exported_collection_pointer() {
        const SOURCE_PATH_ID: i64 = 51;
        const TARGET_COLLECTION_INDEX: usize = COLLECTION_INDEX + 1;
        const TARGET_FILE_ID: i64 = 142;

        let dictionary_bytes = make_game_object_with_external_pptr_dictionary();
        let target_bytes = make_game_object_and_transform_file(TRANSFORM_PATH_ID);
        let source_files = [
            ("main.assets", dictionary_bytes.as_slice()),
            ("target.assets", target_bytes.as_slice()),
        ];
        let (filesystem, workspace) = load_workspace_files(&source_files);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete());

        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let dictionary_edges: Vec<_> = reference_graph.references_from_object(0, 0).collect();
        assert_eq!(dictionary_edges.len(), 1);
        assert_eq!(dictionary_edges[0].field_path, "m_Targets[0].second");
        assert_eq!(dictionary_edges[0].file_id, 1);
        assert_eq!(dictionary_edges[0].path_id, GAME_OBJECT_PATH_ID);
        assert_eq!(
            dictionary_edges[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );

        let target_asset = ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 1 },
            object_index: 0,
            path_id: GAME_OBJECT_PATH_ID,
            class_id: 1,
        };
        let pointer_map = ProjectExportPointerMap::new(
            &[ExportedAssetPointerEntry {
                asset: target_asset,
                file_id: TARGET_FILE_ID,
                collection_index: TARGET_COLLECTION_INDEX,
            }],
            &[ExportCollectionPointerMetadata {
                collection_index: TARGET_COLLECTION_INDEX,
                guid: [0x82; 16],
                asset_type: UnityProjectAssetType::Serialized,
            }],
            ExportPointerMapLimits::default(),
        )
        .unwrap();
        let document = workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            asset(0, SOURCE_PATH_ID, 1),
            SOURCE_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let yaml = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();

        assert!(yaml.contains("--- !u!1 &51"), "{yaml}");
        assert!(
            yaml.contains("target: {fileID: 142, guid: 28282828282828282828282828282828, type: 2}"),
            "{yaml}"
        );
    }

    #[test]
    fn collection_executor_reports_processed_assets_without_a_generated_walker() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
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
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());

        let hierarchies =
            scene_hierarchies_for_workspace(&[("main.assets", &bytes)], &reference_graph);
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
                crate::processing::prefab_processing::WorkspaceProcessedPrefabAssetPlanLimits::default(),
            )
            .unwrap();
        assert!(
            processed_assets.is_complete(),
            "{:?}",
            processed_assets.incomplete_reason()
        );

        let assignments = crate::export::scene_collection::
            plan_workspace_scene_export_ids_with_processed_assets(
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
        let generated_context = WorkspaceGeneratedTransformYamlContext::new(
            &processed_assets,
            0,
            &transform_metadata,
            0,
        );
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

    #[test]
    fn refuses_workspace_payload_bytes_or_reference_edges_that_do_not_match() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let pointer_map = pointer_map();

        let mut with_trailing_bytes = objects.objects()[0].clone();
        with_trailing_bytes
            .payload
            .as_mut()
            .unwrap()
            .trailing_bytes
            .push(0xaa);
        assert!(matches!(
            workspace_object_to_unity_yaml_document(
                &workspace,
                &reference_graph,
                &pointer_map,
                &with_trailing_bytes,
                asset(0, GAME_OBJECT_PATH_ID, 1),
                GAME_OBJECT_PATH_ID,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlWorkspaceExportError::UnparsedTrailingPayload { byte_count: 1, .. })
        ));

        let mut with_wrong_pointer = objects.objects()[0].clone();
        let payload = with_wrong_pointer.payload.as_mut().unwrap();
        let crate::serialized_file::object::SerializedValue::Struct(fields) = &mut payload.root
        else {
            panic!("fixture root is a struct")
        };
        let crate::serialized_file::object::SerializedValue::Array(components) =
            &mut fields[0].value
        else {
            panic!("fixture component field is an array")
        };
        let crate::serialized_file::object::SerializedValue::Struct(pair_fields) =
            &mut components[0]
        else {
            panic!("fixture component value is a struct")
        };
        pair_fields[0].value = crate::serialized_file::object::SerializedValue::PPtr {
            file_id: 0,
            path_id: 999,
        };
        assert!(matches!(
            workspace_object_to_unity_yaml_document(
                &workspace,
                &reference_graph,
                &pointer_map,
                &with_wrong_pointer,
                asset(0, GAME_OBJECT_PATH_ID, 1),
                GAME_OBJECT_PATH_ID,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlWorkspaceExportError::ReferenceEdgeMissing { path_id: 999, .. })
        ));
    }

    #[test]
    fn missing_local_component_reference_remains_an_export_error() {
        let (filesystem, workspace) = load_workspaces(999);
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let pointer_map = pointer_map();
        assert!(matches!(
            workspace_object_to_unity_yaml_document(
                &workspace,
                &reference_graph,
                &pointer_map,
                &objects.objects()[0],
                asset(0, GAME_OBJECT_PATH_ID, 1),
                GAME_OBJECT_PATH_ID,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlWorkspaceExportError::Conversion(
                UnityYamlObjectConversionError::PointerConversion { .. }
            ))
        ));
    }

    #[test]
    fn stripped_marker_needs_hierarchy_and_set_membership() {
        // `StrippedAssetExtensions.IsStripped`: both facts must hold.
        assert!(is_stripped_document(true, true));
        assert!(!is_stripped_document(true, false));
        assert!(!is_stripped_document(false, true));
        assert!(!is_stripped_document(false, false));
    }
