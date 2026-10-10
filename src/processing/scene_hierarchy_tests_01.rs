    use super::*;
    use crate::processing::prefab_processing::{
        plan_workspace_prefab_processing, plan_workspace_processed_prefab_assets,
        WorkspacePrefabProcessingEvent, WorkspaceProcessedCollectionKind,
        WorkspaceProcessedPrefabAssetKind, WorkspaceProcessedPrefabAssetPlanLimits,
        WorkspaceProcessedPrefabReference, WorkspaceSourceDependencyAppendTarget,
        WorkspaceSourceDependencyPointerField,
    };
    use crate::processing::scene_collections::{
        SceneDefinitionGuidPlan, SceneDefinitionPathSource, SceneDefinitionPlan,
        SceneGuidGenerationReason, SceneManagerKind,
    };
    use crate::serialized_file::file::SerializedFile;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::metadata::TypeTreeNode;
    use crate::serialized_file::reference_graph::{
        MultiFileReferenceGraph, NamedObjectReferenceGraph, ObjectReferenceGraph,
    };

    fn push_generated_scene_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn put_generated_scene_type_tree_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        output.extend_from_slice(&1_u16.to_le_bytes());
        output.push(level);
        output.push(0);
        output.extend_from_slice(&type_offset.to_le_bytes());
        output.extend_from_slice(&name_offset.to_le_bytes());
        output.extend_from_slice(&byte_size.to_le_bytes());
        output.extend_from_slice(&index.to_le_bytes());
        output.extend_from_slice(&0_u32.to_le_bytes());
    }

    fn generated_mesh_renderer_serialized_file() -> Vec<u8> {
        const CLASS_ID: i32 = 23;

        let mut strings = Vec::new();
        let mesh_renderer_type =
            push_generated_scene_type_tree_string(&mut strings, "MeshRenderer");
        let empty_name = push_generated_scene_type_tree_string(&mut strings, "");
        let renderer_type = push_generated_scene_type_tree_string(&mut strings, "Renderer");
        let component_type = push_generated_scene_type_tree_string(&mut strings, "Component");
        let base_name = push_generated_scene_type_tree_string(&mut strings, "Base");
        let game_object_pointer_type =
            push_generated_scene_type_tree_string(&mut strings, "PPtr<GameObject>");
        let game_object_field = push_generated_scene_type_tree_string(&mut strings, "m_GameObject");
        let int_type = push_generated_scene_type_tree_string(&mut strings, "int");
        let file_id_field = push_generated_scene_type_tree_string(&mut strings, "m_FileID");
        let path_id_field = push_generated_scene_type_tree_string(&mut strings, "m_PathID");

        let mut metadata = b"2019.4.0f1\0".to_vec();
        metadata.extend_from_slice(&0_u32.to_le_bytes());
        metadata.extend_from_slice(&1_i32.to_le_bytes());
        metadata.extend_from_slice(&CLASS_ID.to_le_bytes());
        metadata.extend_from_slice(&6_i32.to_le_bytes());
        metadata.extend_from_slice(&i32::try_from(strings.len()).unwrap().to_le_bytes());
        put_generated_scene_type_tree_node(&mut metadata, 0, mesh_renderer_type, empty_name, -1, 0);
        put_generated_scene_type_tree_node(&mut metadata, 1, renderer_type, base_name, -1, 1);
        put_generated_scene_type_tree_node(&mut metadata, 2, component_type, base_name, -1, 2);
        put_generated_scene_type_tree_node(
            &mut metadata,
            3,
            game_object_pointer_type,
            game_object_field,
            -1,
            3,
        );
        put_generated_scene_type_tree_node(&mut metadata, 4, int_type, file_id_field, 4, 4);
        put_generated_scene_type_tree_node(&mut metadata, 4, int_type, path_id_field, 4, 5);
        metadata.extend_from_slice(&strings);

        metadata.extend_from_slice(&0_u32.to_le_bytes());
        metadata.extend_from_slice(&1_i32.to_le_bytes());
        metadata.extend_from_slice(&9001_i32.to_le_bytes());
        metadata.extend_from_slice(&0_u32.to_le_bytes());
        metadata.extend_from_slice(&8_i32.to_le_bytes());
        metadata.extend_from_slice(&CLASS_ID.to_le_bytes());
        metadata.extend_from_slice(&(CLASS_ID as i16).to_le_bytes());
        metadata.extend_from_slice(&0_u16.to_le_bytes());
        metadata.extend_from_slice(&0_i32.to_le_bytes());
        metadata.push(0);

        let header_size = 20_usize;
        let data_offset = header_size + metadata.len();
        let file_size = data_offset + 8;
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&FormatVersion::Unknown10.raw().to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&[0; 8]);
        file
    }

    fn identity(
        file_index: usize,
        object_index: usize,
        path_id: i64,
        class_id: i32,
    ) -> WorkspaceSceneObjectIdentity {
        WorkspaceSceneObjectIdentity {
            file_index,
            object_index,
            path_id,
            class_id: Some(class_id),
        }
    }

    fn relationship(
        source: WorkspaceSceneObjectIdentity,
        kind: SceneRelationshipKind,
        field_path: &str,
        path_id: i64,
        target: SceneRelationshipTarget,
    ) -> SceneRelationship {
        SceneRelationship {
            source_object_index: source.object_index,
            source_path_id: source.path_id,
            kind,
            field_path: field_path.to_owned(),
            file_id: 0,
            path_id,
            target,
        }
    }

    fn empty_collection_graph(file_index: usize) -> WorkspaceSceneCollectionHierarchy {
        WorkspaceSceneCollectionHierarchy {
            collection_file_index: file_index,
            scene_objects: Vec::new(),
            relationships: Vec::new(),
            game_object_hierarchies: Vec::new(),
            game_object_component_relationship_indices: Vec::new(),
            prefab_root_relationship_index: Vec::new(),
            transform_hierarchies: Vec::new(),
            transform_parent_relationship_indices: Vec::new(),
            transform_child_relationship_indices: Vec::new(),
            transform_game_object_relationship_indices: Vec::new(),
            graph_issues: Vec::new(),
            scene_assets: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            component_candidates: Vec::new(),
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        }
    }

    fn scene_definition_plan(file_index: usize) -> SceneDefinitionPlan {
        SceneDefinitionPlan {
            collection_file_index: file_index,
            collection_name: "scene".to_owned(),
            name: "scene".to_owned(),
            path: "Assets/Scenes/scene".to_owned(),
            path_source: SceneDefinitionPathSource::CollectionNameFallback,
            guid: SceneDefinitionGuidPlan::GenerateAtExport(
                SceneGuidGenerationReason::MissingFromInput,
            ),
        }
    }

    #[test]
    fn component_candidate_normalization_keeps_distinct_evidence_for_one_object() {
        let mono_behaviour = identity(0, 1, 11, MONO_BEHAVIOUR_CLASS_ID);
        let direct = WorkspaceSceneComponentCandidate {
            identity: mono_behaviour,
            evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
        };
        let back_reference = WorkspaceSceneComponentCandidate {
            identity: mono_behaviour,
            evidence: WorkspaceSceneComponentEvidence::GameObjectPointerField,
        };
        let null_back_reference = WorkspaceSceneComponentCandidate {
            identity: mono_behaviour,
            evidence: WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull,
        };
        let mut candidates = vec![back_reference, direct, null_back_reference, direct];

        normalize_component_candidates(&mut candidates);

        assert_eq!(
            candidates,
            vec![direct, null_back_reference, back_reference]
        );
    }

    #[test]
    fn type_tree_base_chain_classifies_component_inheritance_without_guessing() {
        let tree = |nodes: &[(u8, &str, Option<&str>)]| TypeTree {
            nodes: nodes
                .iter()
                .map(|(level, name, type_name)| TypeTreeNode {
                    level: *level,
                    version: 1,
                    type_flags: 0,
                    type_string_offset: None,
                    name_string_offset: None,
                    type_name: type_name.map(str::to_owned),
                    name: Some((*name).to_owned()),
                    byte_size: -1,
                    index: 0,
                    meta_flags: 0,
                    reference_type_hash: None,
                })
                .collect(),
            string_buffer: Vec::new(),
        };
        let mesh_renderer = tree(&[
            (0, "", Some("MeshRenderer")),
            (1, "Base", Some("Renderer")),
            (2, "Base", Some("Component")),
            (3, "m_GameObject", Some("PPtr<GameObject>")),
        ]);
        let scriptable_object = tree(&[
            (0, "", Some("ExampleAsset")),
            (1, "Base", Some("ScriptableObject")),
            (2, "Base", Some("Object")),
        ]);
        let duplicate_base = tree(&[
            (0, "", Some("MalformedAsset")),
            (1, "Base", Some("Component")),
            (1, "Base", Some("Object")),
        ]);
        let missing_base_type = tree(&[(0, "", Some("MalformedAsset")), (1, "Base", None)]);
        let empty_tree = TypeTree {
            nodes: Vec::new(),
            string_buffer: Vec::new(),
        };

        assert_eq!(
            classify_type_tree_component_base(Some(&mesh_renderer)),
            TypeTreeComponentBaseClassification::Component
        );
        assert_eq!(
            classify_type_tree_component_base(Some(&scriptable_object)),
            TypeTreeComponentBaseClassification::NotComponent
        );
        assert_eq!(
            classify_type_tree_component_base(Some(&duplicate_base)),
            TypeTreeComponentBaseClassification::Ambiguous
        );
        assert_eq!(
            classify_type_tree_component_base(Some(&missing_base_type)),
            TypeTreeComponentBaseClassification::Ambiguous
        );
        assert_eq!(
            classify_type_tree_component_base(Some(&empty_tree)),
            TypeTreeComponentBaseClassification::Unavailable
        );
        assert_eq!(
            classify_type_tree_component_base(None),
            TypeTreeComponentBaseClassification::Unavailable
        );
    }

    #[test]
    fn generated_serialized_file_type_tree_component_flows_into_scene_ids() {
        const MESH_RENDERER_CLASS_ID: i32 = 23;

        let bytes = generated_mesh_renderer_serialized_file();
        let file = SerializedFile::parse(&bytes).unwrap();
        assert_eq!(
            file.metadata.objects[0].class_id,
            Some(MESH_RENDERER_CLASS_ID as i16)
        );
        let object_graph = ObjectReferenceGraph::build(&file).unwrap();
        let named_files = [NamedObjectReferenceGraph {
            file_name: "scene.assets",
            metadata: &file.metadata,
            graph: &object_graph,
        }];
        let references = MultiFileReferenceGraph::build(&named_files).unwrap();
        assert!(
            references.is_complete(),
            "object scan: {:?}; issues: {:?}",
            references.object_scan_is_complete(0, 0),
            object_graph.issues()
        );
        assert_eq!(references.object_type_tree(0, 0).unwrap().nodes.len(), 6);

        let mut scene_graph =
            SceneGraph::build(&file, crate::processing::scene::SceneGraphLimits::default())
                .unwrap();
        let roots = scene_graph
            .discover_workspace_roots(
                0,
                "scene.assets",
                &references,
                crate::processing::scene::SceneRootDiscoveryLimits::default(),
            )
            .unwrap();
        let audit = scene_graph
            .audit_workspace_transform_links(
                0,
                "scene.assets",
                &references,
                crate::processing::scene::SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        let plan = crate::processing::scene_collections::plan_scene_definition(
            0,
            "scene.assets",
            &crate::processing::scene_collections::SceneCollectionPathStatus::BuildSettingsMissing,
            None,
            None,
            crate::processing::scene_collections::SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        let definitions =
            crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
                vec![plan],
                1,
                crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
            )
            .unwrap();
        let mut builder =
            WorkspaceSceneHierarchyBuilder::new(1, WorkspaceSceneHierarchyLimits::default())
                .unwrap();
        builder
            .add_collection_graph(0, &scene_graph, roots, audit, &references)
            .unwrap();
        let hierarchies = builder.finish(&definitions).unwrap();
        let scene = &hierarchies.scenes()[0];
        let candidates = scene.collections()[0].component_candidates();
        assert_eq!(
            candidates,
            &[
                WorkspaceSceneComponentCandidate {
                    identity: identity(0, 0, 9001, MESH_RENDERER_CLASS_ID),
                    evidence: WorkspaceSceneComponentEvidence::TypeTreeComponentBase,
                },
                WorkspaceSceneComponentCandidate {
                    identity: identity(0, 0, 9001, MESH_RENDERER_CLASS_ID),
                    evidence: WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull,
                },
            ]
        );

        let membership = scene
            .asset_membership(WorkspaceSceneHierarchyMembershipLimits::default())
            .unwrap();
        assert!(
            membership.is_complete(),
            "issues: {:?}",
            membership.issues()
        );
        assert_eq!(
            membership.components(),
            &[identity(0, 0, 9001, MESH_RENDERER_CLASS_ID)]
        );
        let assignments = crate::export::scene_collection::plan_workspace_scene_export_ids(
            scene,
            crate::processing::scene_naming::SceneUnityVersion {
                major: 2019,
                minor: 4,
                patch: 0,
            },
            WorkspaceSceneHierarchyMembershipLimits::default(),
            crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
            crate::export::scene_collection::SceneExportIdPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(assignments.len(), 1);
        assert_eq!(
            assignments[0].asset,
            crate::export::scene_collection::ExportAssetIdentity {
                collection:
                    crate::export::scene_collection::ExportAssetCollectionIdentity::SerializedFile {
                        file_index: 0
                    },
                object_index: 0,
                path_id: 9001,
                class_id: MESH_RENDERER_CLASS_ID,
            }
        );
        assert_eq!(assignments[0].file_id, 9001);
    }

    #[test]
    fn scene_asset_membership_preserves_source_order_and_reports_weak_component_evidence() {
        let mut first = empty_collection_graph(8);
        let first_game_object = identity(8, 0, 80, GAME_OBJECT_CLASS_ID);
        let another_first_collection_game_object = identity(8, 4, 84, GAME_OBJECT_CLASS_ID);
        let first_prefab_instance = identity(8, 5, 85, PREFAB_INSTANCE_CLASS_ID);
        let first_manager = identity(8, 6, 86, SceneManagerKind::RenderSettings.class_id());
        let first_roots = identity(8, 7, 87, SCENE_ROOTS_CLASS_ID);
        let second_roots = identity(8, 8, 88, SCENE_ROOTS_CLASS_ID);
        first.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: first_game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: first_prefab_instance,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            },
            WorkspaceSceneAssetIdentity {
                identity: first_manager,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::RenderSettings,
                ),
            },
            WorkspaceSceneAssetIdentity {
                identity: first_roots,
                category: WorkspaceSceneAssetCategory::SceneRoots,
            },
            WorkspaceSceneAssetIdentity {
                identity: another_first_collection_game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: second_roots,
                category: WorkspaceSceneAssetCategory::SceneRoots,
            },
        ];
        first.component_candidates = vec![
            WorkspaceSceneComponentCandidate {
                identity: identity(3, 5, 35, 23),
                evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            },
            WorkspaceSceneComponentCandidate {
                identity: identity(8, 2, 82, TRANSFORM_CLASS_ID),
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            },
        ];

        let mut second = empty_collection_graph(3);
        let second_game_object = identity(3, 1, 31, GAME_OBJECT_CLASS_ID);
        let second_prefab_instance = identity(3, 2, 32, PREFAB_INSTANCE_CLASS_ID);
        let second_manager = identity(3, 3, 33, SceneManagerKind::LightmapSettings.class_id());
        second.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: second_game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: second_prefab_instance,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            },
            WorkspaceSceneAssetIdentity {
                identity: second_manager,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::LightmapSettings,
                ),
            },
        ];
        second.component_candidates = vec![
            WorkspaceSceneComponentCandidate {
                identity: identity(8, 1, 81, 23),
                evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            },
            WorkspaceSceneComponentCandidate {
                identity: identity(3, 4, 34, TRANSFORM_CLASS_ID),
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            },
            WorkspaceSceneComponentCandidate {
                identity: identity(3, 6, 36, 23),
                evidence: WorkspaceSceneComponentEvidence::GameObjectPointerField,
            },
        ];
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(8),
            collections: vec![first, second],
        };

        let membership = scene
            .asset_membership(WorkspaceSceneHierarchyMembershipLimits::default())
            .unwrap();
        assert_eq!(
            membership.game_objects(),
            &[
                first_game_object,
                another_first_collection_game_object,
                second_game_object
            ]
        );
        assert_eq!(
            membership.prefab_instances(),
            &[first_prefab_instance, second_prefab_instance]
        );
        assert_eq!(membership.managers(), &[first_manager, second_manager]);
        assert_eq!(membership.scene_roots(), Some(second_roots));
        assert_eq!(
            membership.components(),
            &[
                identity(8, 1, 81, 23),
                identity(8, 2, 82, TRANSFORM_CLASS_ID),
                identity(3, 4, 34, TRANSFORM_CLASS_ID),
                identity(3, 5, 35, 23),
            ]
        );
        assert!(!membership.is_complete());
        assert!(membership.issues().contains(
            &WorkspaceSceneHierarchyMembershipIssue::UnclassifiedComponentCandidate {
                identity: identity(3, 6, 36, 23),
                evidence: WorkspaceSceneComponentEvidence::GameObjectPointerField,
            }
        ));
        assert!(matches!(
            crate::export::scene_collection::plan_workspace_scene_export_ids(
                &scene,
                crate::processing::scene_naming::SceneUnityVersion {
                    major: 2019,
                    minor: 4,
                    patch: 0,
                },
                WorkspaceSceneHierarchyMembershipLimits::default(),
                crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
                crate::export::scene_collection::SceneExportIdPlanLimits::default(),
            ),
            Err(crate::export::scene_collection::WorkspaceSceneExportPlanError::IncompleteMembership { .. })
        ));
    }

    #[test]
    fn scene_asset_membership_marks_incomplete_scans_and_enforces_asset_limit() {
        let mut collection = empty_collection_graph(0);
        collection.source_reference_scans_complete = false;
        collection.scene_assets.push(WorkspaceSceneAssetIdentity {
            identity: identity(0, 0, 10, GAME_OBJECT_CLASS_ID),
            category: WorkspaceSceneAssetCategory::GameObject,
        });
        collection.scene_assets.push(WorkspaceSceneAssetIdentity {
            identity: identity(0, 1, 11, GAME_OBJECT_CLASS_ID),
            category: WorkspaceSceneAssetCategory::GameObject,
        });
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![collection],
        };
        let membership = scene
            .asset_membership(WorkspaceSceneHierarchyMembershipLimits::default())
            .unwrap();
        assert_eq!(
            membership.issues(),
            &[WorkspaceSceneHierarchyMembershipIssue::IncompleteReferenceScans { file_index: 0 }]
        );
        assert!(matches!(
            scene.asset_membership(WorkspaceSceneHierarchyMembershipLimits {
                max_assets: 1,
                max_issues: 10,
            }),
            Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
                requested: 2,
                limit: 1,
            })
        ));
    }

    #[test]
    fn workspace_scene_export_plan_uses_baseline_category_order_before_yaml_sorting() {
        let game_object = identity(5, 0, 50, GAME_OBJECT_CLASS_ID);
        let prefab_instance = identity(5, 1, 51, PREFAB_INSTANCE_CLASS_ID);
        let manager = identity(5, 2, 52, SceneManagerKind::RenderSettings.class_id());
        let scene_roots = identity(5, 3, 53, SCENE_ROOTS_CLASS_ID);
        let transform = identity(5, 4, 54, TRANSFORM_CLASS_ID);
        let renderer = identity(5, 5, 55, 23);
        let mono_behaviour = identity(5, 6, 56, MONO_BEHAVIOUR_CLASS_ID);
        let mut collection = empty_collection_graph(5);
        collection.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: prefab_instance,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            },
            WorkspaceSceneAssetIdentity {
                identity: manager,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::RenderSettings,
                ),
            },
            WorkspaceSceneAssetIdentity {
                identity: scene_roots,
                category: WorkspaceSceneAssetCategory::SceneRoots,
            },
        ];
        collection.component_candidates = vec![
            WorkspaceSceneComponentCandidate {
                identity: transform,
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            },
            WorkspaceSceneComponentCandidate {
                identity: renderer,
                evidence: WorkspaceSceneComponentEvidence::TypeTreeComponentBase,
            },
            WorkspaceSceneComponentCandidate {
                identity: mono_behaviour,
                evidence: WorkspaceSceneComponentEvidence::GameObjectPointerField,
            },
        ];
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(5),
            collections: vec![collection],
        };
        let assignments = crate::export::scene_collection::plan_workspace_scene_export_ids(
            &scene,
            crate::processing::scene_naming::SceneUnityVersion {
                major: 2019,
                minor: 4,
                patch: 0,
            },
            WorkspaceSceneHierarchyMembershipLimits::default(),
            crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
            crate::export::scene_collection::SceneExportIdPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| assignment.asset.object_index)
                .collect::<Vec<_>>(),
            [0, 4, 5, 6, 1, 2, 3]
        );
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| assignment.file_id)
                .collect::<Vec<_>>(),
            [50, 54, 55, 56, 51, 52, 53]
        );
        let yaml_order = crate::export::scene_collection::order_scene_yaml_assignments(
            &assignments,
            crate::export::scene_collection::SceneYamlAssetOrderLimits::default(),
        )
        .unwrap();
        assert_eq!(
            yaml_order
                .iter()
                .map(|assignment| assignment.asset.object_index)
                .collect::<Vec<_>>(),
            [2, 0, 4, 5, 6, 1, 3]
        );
    }

    #[test]
    fn mono_behaviour_requires_baseline_non_null_game_object_evidence() {
        let game_object = identity(0, 0, 10, GAME_OBJECT_CLASS_ID);
        let mono_behaviour = identity(0, 1, 11, MONO_BEHAVIOUR_CLASS_ID);
        let mut collection = empty_collection_graph(0);
        collection.scene_assets.push(WorkspaceSceneAssetIdentity {
            identity: game_object,
            category: WorkspaceSceneAssetCategory::GameObject,
        });
        collection
            .component_candidates
            .push(WorkspaceSceneComponentCandidate {
                identity: mono_behaviour,
                evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            });
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![collection],
        };

        let membership = scene
            .asset_membership(WorkspaceSceneHierarchyMembershipLimits::default())
            .unwrap();
        assert!(!membership.components().contains(&mono_behaviour));
        assert!(membership.issues().contains(
            &WorkspaceSceneHierarchyMembershipIssue::MonoBehaviourGameObjectReferenceUnavailable {
                identity: mono_behaviour,
            }
        ));
        assert!(!membership.is_complete());

        let mut null_pointer_collection = empty_collection_graph(0);
        null_pointer_collection
            .scene_assets
            .push(WorkspaceSceneAssetIdentity {
                identity: game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            });
        null_pointer_collection.component_candidates = vec![
            WorkspaceSceneComponentCandidate {
                identity: mono_behaviour,
                evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            },
            WorkspaceSceneComponentCandidate {
                identity: mono_behaviour,
                evidence: WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull,
            },
        ];
        let null_pointer_scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![null_pointer_collection],
        };
        let null_pointer_membership = null_pointer_scene
            .asset_membership(WorkspaceSceneHierarchyMembershipLimits::default())
            .unwrap();
        assert!(!null_pointer_membership
            .components()
            .contains(&mono_behaviour));
        assert!(null_pointer_membership.issues().is_empty());
        assert!(null_pointer_membership.is_complete());
    }

    #[test]
    fn lighting_data_dependency_owner_matches_first_lightmap_settings_and_render_settings() {
        let first = identity(0, 0, 10, LIGHTMAP_SETTINGS_CLASS_ID);
        let second = identity(0, 1, 11, LIGHTMAP_SETTINGS_CLASS_ID);
        let render = identity(0, 2, 12, SceneManagerKind::RenderSettings.class_id());
        let mut collection = empty_collection_graph(0);
        collection.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: first,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::LightmapSettings,
                ),
            },
            WorkspaceSceneAssetIdentity {
                identity: second,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::LightmapSettings,
                ),
            },
            WorkspaceSceneAssetIdentity {
                identity: render,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::RenderSettings,
                ),
            },
        ];
        collection.lighting_data_asset_field_object_indices = vec![second.object_index];
        let mut scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![collection],
        };

        // The source selects the first LightmapSettings; a later field-bearing instance is not used.
        assert_eq!(scene.lighting_data_dependency_owner(), None);
        scene.collections[0].lighting_data_asset_field_object_indices =
            vec![first.object_index, second.object_index];
        assert_eq!(scene.lighting_data_dependency_owner(), Some(first));
        scene.collections[0].scene_assets.retain(|asset| {
            asset.category
                != WorkspaceSceneAssetCategory::LevelGameManager(SceneManagerKind::RenderSettings)
        });
        assert_eq!(scene.lighting_data_dependency_owner(), None);
    }

    #[test]
    fn lightmap_settings_lighting_data_asset_capability_matches_source_version_fixture() {
        let mut row_count = 0usize;
        for line in include_str!("lightmap_settings_capability_fixture.txt").lines() {
            let fields = line.split('|').collect::<Vec<_>>();
            assert_eq!(fields.len(), 4, "invalid source fixture row {line:?}");
            assert_eq!(fields[0], "VERSION", "invalid source fixture row {line:?}");
            let version_text = fields[1];
            assert!(
                fields[2].starts_with("LightmapSettings_"),
                "missing generated class in source fixture row {line:?}"
            );
            let expected = match fields[3] {
                "True" => true,
                "False" => false,
                value => panic!("invalid Has_LightingDataAsset value {value:?}"),
            };
            let version = SceneUnityVersion::parse(version_text)
                .unwrap_or_else(|| panic!("invalid Unity version in source fixture row {line:?}"));
            assert_eq!(
                lightmap_settings_has_lighting_data_asset(version),
                expected,
                "Unity {version_text} Has_LightingDataAsset"
            );
            row_count += 1;
        }
        assert_eq!(row_count, 32, "source version fixture row count changed");
    }

    #[test]
    fn lighting_data_asset_field_match_uses_the_terminal_typed_field_name() {
        assert!(is_lighting_data_asset_field_path("m_LightingDataAsset"));
        assert!(is_lighting_data_asset_field_path(
            "LightmapSettings.m_LightingDataAsset"
        ));
        assert!(!is_lighting_data_asset_field_path(
            "m_LightingDataAssetBackup"
        ));
        assert!(!is_lighting_data_asset_field_path("m_LightmapData"));
    }

    fn workspace_hierarchy_fixture(cycle: bool) -> WorkspaceSceneHierarchies {
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let first_transform = identity(0, 1, 2, TRANSFORM_CLASS_ID);
        let last_transform = identity(0, 2, 3, RECT_TRANSFORM_CLASS_ID);
        let renderer = identity(0, 3, 4, 23);
        let child_transform = identity(1, 0, 10, TRANSFORM_CLASS_ID);
        let child_game_object = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);

        let root_relationships = vec![
            relationship(
                root,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[0]",
                renderer.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 3 },
            ),
            relationship(
                root,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[1]",
                0,
                SceneRelationshipTarget::Null,
            ),
            relationship(
                root,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[2]",
                first_transform.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 1 },
            ),
            relationship(
                root,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[3]",
                last_transform.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 2 },
            ),
            relationship(
                last_transform,
                SceneRelationshipKind::TransformChild,
                "m_Children[0]",
                child_transform.path_id,
                SceneRelationshipTarget::WorkspaceObject {
                    file_index: 1,
                    object_index: 0,
                },
            ),
            relationship(
                last_transform,
                SceneRelationshipKind::ComponentGameObject,
                "m_GameObject",
                root.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 0 },
            ),
            relationship(
                first_transform,
                SceneRelationshipKind::ComponentGameObject,
                "m_GameObject",
                root.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 0 },
            ),
            relationship(
                first_transform,
                SceneRelationshipKind::TransformParent,
                "m_Father",
                0,
                SceneRelationshipTarget::Null,
            ),
            relationship(
                last_transform,
                SceneRelationshipKind::TransformParent,
                "m_Father",
                0,
                SceneRelationshipTarget::Null,
            ),
        ];
        let root_collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 0,
            scene_objects: Vec::new(),
            relationships: root_relationships,
            game_object_hierarchies: vec![WorkspaceSceneGameObjectHierarchy {
                game_object: root,
                component_relationship_start: 0,
                component_relationship_end: 4,
            }],
            game_object_component_relationship_indices: vec![0, 1, 2, 3],
            prefab_root_relationship_index: Vec::new(),
            transform_hierarchies: vec![
                WorkspaceSceneTransformHierarchy {
                    transform: first_transform,
                    parent_relationship_start: 0,
                    parent_relationship_end: 1,
                    child_relationship_start: 0,
                    child_relationship_end: 0,
                    game_object_relationship_start: 0,
                    game_object_relationship_end: 1,
                },
                WorkspaceSceneTransformHierarchy {
                    transform: last_transform,
                    parent_relationship_start: 1,
                    parent_relationship_end: 2,
                    child_relationship_start: 0,
                    child_relationship_end: 1,
                    game_object_relationship_start: 1,
                    game_object_relationship_end: 1,
                },
            ],
            transform_parent_relationship_indices: vec![7, 8],
            transform_child_relationship_indices: vec![4],
            transform_game_object_relationship_indices: vec![6, 5],
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![WorkspaceSceneAssetIdentity {
                identity: root,
                category: WorkspaceSceneAssetCategory::GameObject,
            }],
            component_candidates: vec![
                WorkspaceSceneComponentCandidate {
                    identity: first_transform,
                    evidence: WorkspaceSceneComponentEvidence::TransformClassId,
                },
                WorkspaceSceneComponentCandidate {
                    identity: last_transform,
                    evidence: WorkspaceSceneComponentEvidence::TransformClassId,
                },
                WorkspaceSceneComponentCandidate {
                    identity: renderer,
                    evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
                },
            ],
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };

        let mut child_relationships = vec![
            relationship(
                child_transform,
                SceneRelationshipKind::ComponentGameObject,
                "m_GameObject",
                child_game_object.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 1 },
            ),
            relationship(
                child_game_object,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[0]",
                child_transform.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 0 },
            ),
            relationship(
                child_transform,
                SceneRelationshipKind::TransformParent,
                "m_Father",
                first_transform.path_id,
                SceneRelationshipTarget::WorkspaceObject {
                    file_index: 0,
                    object_index: 1,
                },
            ),
        ];
        let child_transform_child_range = if cycle {
            child_relationships.push(relationship(
                child_transform,
                SceneRelationshipKind::TransformChild,
                "m_Children[0]",
                child_transform.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 0 },
            ));
            0..1
        } else {
            0..0
        };
        let child_transform_child_indexes = if cycle { vec![3] } else { Vec::new() };
        let child_collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 1,
            scene_objects: Vec::new(),
            relationships: child_relationships,
            game_object_hierarchies: vec![WorkspaceSceneGameObjectHierarchy {
                game_object: child_game_object,
                component_relationship_start: 0,
                component_relationship_end: 1,
            }],
            game_object_component_relationship_indices: vec![1],
            prefab_root_relationship_index: Vec::new(),
            transform_hierarchies: vec![WorkspaceSceneTransformHierarchy {
                transform: child_transform,
                parent_relationship_start: 0,
                parent_relationship_end: 1,
                child_relationship_start: child_transform_child_range.start,
                child_relationship_end: child_transform_child_range.end,
                game_object_relationship_start: 0,
                game_object_relationship_end: 1,
            }],
            transform_parent_relationship_indices: vec![2],
            transform_child_relationship_indices: child_transform_child_indexes,
            transform_game_object_relationship_indices: vec![0],
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![WorkspaceSceneAssetIdentity {
                identity: child_game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            }],
            component_candidates: vec![WorkspaceSceneComponentCandidate {
                identity: child_transform,
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            }],
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };
        WorkspaceSceneHierarchies {
            scenes: Vec::new(),
            non_scene_collections: vec![root_collection, child_collection],
            collection_graph_index: vec![
                WorkspaceSceneCollectionGraphIndexEntry {
                    file_index: 0,
                    scene_index: None,
                    collection_index: 0,
                },
                WorkspaceSceneCollectionGraphIndexEntry {
                    file_index: 1,
                    scene_index: None,
                    collection_index: 1,
                },
            ],
        }
    }

    fn prefab_root_workspace(
        target: Option<(i64, SceneRelationshipTarget)>,
    ) -> (WorkspaceSceneHierarchies, WorkspaceSceneObjectIdentity) {
        let prefab = identity(0, 0, 100, PREFAB_INSTANCE_CLASS_ID);
        let root = identity(0, 1, 200, GAME_OBJECT_CLASS_ID);
        let relationships = target
            .map(|(path_id, target)| {
                vec![relationship(
                    prefab,
                    SceneRelationshipKind::PrefabInstanceRootGameObject,
                    "m_RootGameObject",
                    path_id,
                    target,
                )]
            })
            .unwrap_or_default();
        let prefab_root_relationship_index = if relationships.is_empty() {
            Vec::new()
        } else {
            vec![WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: prefab.object_index,
                relationship_index: 0,
            }]
        };
        let collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 0,
            scene_objects: Vec::new(),
            relationships,
            game_object_hierarchies: Vec::new(),
            game_object_component_relationship_indices: Vec::new(),
            prefab_root_relationship_index,
            transform_hierarchies: Vec::new(),
            transform_parent_relationship_indices: Vec::new(),
            transform_child_relationship_indices: Vec::new(),
            transform_game_object_relationship_indices: Vec::new(),
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![
                WorkspaceSceneAssetIdentity {
                    identity: prefab,
                    category: WorkspaceSceneAssetCategory::PrefabInstance,
                },
                WorkspaceSceneAssetIdentity {
                    identity: root,
                    category: WorkspaceSceneAssetCategory::GameObject,
                },
            ],
            component_candidates: Vec::new(),
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };
        (
            WorkspaceSceneHierarchies {
                scenes: Vec::new(),
                non_scene_collections: vec![collection],
                collection_graph_index: vec![WorkspaceSceneCollectionGraphIndexEntry {
                    file_index: 0,
                    scene_index: None,
                    collection_index: 0,
                }],
            },
            prefab,
        )
    }

    #[test]
    fn recognizes_source_scene_hierarchy_asset_class_ids() {
        assert_eq!(
            scene_asset_category(Some(1)),
            Some(WorkspaceSceneAssetCategory::GameObject)
        );
        assert_eq!(
            scene_asset_category(Some(29)),
            Some(WorkspaceSceneAssetCategory::LevelGameManager(
                SceneManagerKind::OcclusionCullingSettings
            ))
        );
        assert_eq!(
            scene_asset_category(Some(104)),
            Some(WorkspaceSceneAssetCategory::LevelGameManager(
                SceneManagerKind::RenderSettings
            ))
        );
        assert_eq!(
            scene_asset_category(Some(157)),
            Some(WorkspaceSceneAssetCategory::LevelGameManager(
                SceneManagerKind::LightmapSettings
            ))
        );
        assert_eq!(
            scene_asset_category(Some(196)),
            Some(WorkspaceSceneAssetCategory::LevelGameManager(
                SceneManagerKind::NavMeshSettings
            ))
        );
        assert_eq!(
            scene_asset_category(Some(1001)),
            Some(WorkspaceSceneAssetCategory::PrefabInstance)
        );
        assert_eq!(
            scene_asset_category(Some(1_660_057_539)),
            Some(WorkspaceSceneAssetCategory::SceneRoots)
        );
        assert_eq!(scene_asset_category(Some(114)), None);
    }

    #[test]
    fn indexes_prefab_root_edges_only_for_matching_prefab_instances() {
        let prefab = identity(0, 3, 30, PREFAB_INSTANCE_CLASS_ID);
        let root = identity(0, 4, 40, GAME_OBJECT_CLASS_ID);
        let relationships = vec![relationship(
            prefab,
            SceneRelationshipKind::PrefabInstanceRootGameObject,
            "m_RootGameObject",
            root.path_id,
            SceneRelationshipTarget::LocalObject {
                object_index: root.object_index,
            },
        )];
        let assets = [WorkspaceSceneAssetIdentity {
            identity: prefab,
            category: WorkspaceSceneAssetCategory::PrefabInstance,
        }];
        assert_eq!(
            build_prefab_root_relationship_index(0, &assets, &relationships).unwrap(),
            vec![WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: prefab.object_index,
                relationship_index: 0,
            }]
        );
        assert!(matches!(
            build_prefab_root_relationship_index(0, &[], &relationships),
            Err(
                WorkspaceSceneHierarchyError::PrefabRootRelationshipSourceMissing {
                    file_index: 0,
                    relationship_index: 0,
                    object_index: 3,
                }
            )
        ));
    }

    #[test]
    fn resolves_prefab_instance_root_in_workspace_file_order() {
        let prefab = identity(0, 0, 100, PREFAB_INSTANCE_CLASS_ID);
        let root = identity(1, 0, 200, GAME_OBJECT_CLASS_ID);
        let prefab_relationship = relationship(
            prefab,
            SceneRelationshipKind::PrefabInstanceRootGameObject,
            "m_RootGameObject",
            root.path_id,
            SceneRelationshipTarget::WorkspaceObject {
                file_index: root.file_index,
                object_index: root.object_index,
            },
        );
        let prefab_collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 0,
            scene_objects: Vec::new(),
            relationships: vec![prefab_relationship],
            game_object_hierarchies: Vec::new(),
            game_object_component_relationship_indices: Vec::new(),
            prefab_root_relationship_index: vec![WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: prefab.object_index,
                relationship_index: 0,
            }],
            transform_hierarchies: Vec::new(),
            transform_parent_relationship_indices: Vec::new(),
            transform_child_relationship_indices: Vec::new(),
            transform_game_object_relationship_indices: Vec::new(),
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![WorkspaceSceneAssetIdentity {
                identity: prefab,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            }],
            component_candidates: Vec::new(),
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };
        let root_collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 1,
            scene_objects: Vec::new(),
            relationships: Vec::new(),
            game_object_hierarchies: Vec::new(),
            game_object_component_relationship_indices: Vec::new(),
            prefab_root_relationship_index: Vec::new(),
            transform_hierarchies: Vec::new(),
            transform_parent_relationship_indices: Vec::new(),
            transform_child_relationship_indices: Vec::new(),
            transform_game_object_relationship_indices: Vec::new(),
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![WorkspaceSceneAssetIdentity {
                identity: root,
                category: WorkspaceSceneAssetCategory::GameObject,
            }],
            component_candidates: Vec::new(),
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };
        let workspace = WorkspaceSceneHierarchies {
            scenes: Vec::new(),
            non_scene_collections: vec![prefab_collection, root_collection],
            collection_graph_index: vec![
                WorkspaceSceneCollectionGraphIndexEntry {
                    file_index: 0,
                    scene_index: None,
                    collection_index: 0,
                },
                WorkspaceSceneCollectionGraphIndexEntry {
                    file_index: 1,
                    scene_index: None,
                    collection_index: 1,
                },
            ],
        };

        assert_eq!(
            workspace.prefab_instance_identities().collect::<Vec<_>>(),
            vec![prefab]
        );
        assert_eq!(
            workspace.resolve_prefab_instance_root(prefab).unwrap(),
            WorkspacePrefabInstanceRootResolution::Resolved(root)
        );
    }

    #[test]
    fn enumerates_prefab_instances_in_supplied_source_collection_order() {
        let (mut workspace, prefab_in_file_zero) =
            prefab_root_workspace(Some((0, SceneRelationshipTarget::Null)));
        let mut prefab_in_file_one = prefab_in_file_zero;
        prefab_in_file_one.file_index = 1;
        prefab_in_file_one.path_id = 101;

        let mut file_one_collection = workspace.non_scene_collections[0].clone();
        file_one_collection.collection_file_index = 1;
        file_one_collection.scene_assets = vec![WorkspaceSceneAssetIdentity {
            identity: prefab_in_file_one,
            category: WorkspaceSceneAssetCategory::PrefabInstance,
        }];
        file_one_collection.relationships.clear();
        file_one_collection.prefab_root_relationship_index.clear();
        workspace.non_scene_collections.push(file_one_collection);
        workspace
            .collection_graph_index
            .push(WorkspaceSceneCollectionGraphIndexEntry {
                file_index: 1,
                scene_index: None,
                collection_index: 1,
            });

        assert_eq!(
            workspace
                .prefab_instance_identities_in_collection_order(&[1, 0])
                .collect::<Vec<_>>(),
            vec![prefab_in_file_one, prefab_in_file_zero]
        );
    }

    #[test]
    fn keeps_null_unresolved_and_missing_prefab_roots_distinct() {
        let (null_workspace, prefab) =
            prefab_root_workspace(Some((0, SceneRelationshipTarget::Null)));
        assert_eq!(
            null_workspace.resolve_prefab_instance_root(prefab).unwrap(),
            WorkspacePrefabInstanceRootResolution::Null
        );

        let (unresolved_workspace, prefab) = prefab_root_workspace(Some((
            404,
            SceneRelationshipTarget::MissingLocalObject { path_id: 404 },
        )));
        assert_eq!(
            unresolved_workspace
                .resolve_prefab_instance_root(prefab)
                .unwrap(),
            WorkspacePrefabInstanceRootResolution::UnresolvedTarget(
                SceneRelationshipTarget::MissingLocalObject { path_id: 404 }
            )
        );

        let (missing_workspace, prefab) = prefab_root_workspace(None);
        assert_eq!(
            missing_workspace
                .resolve_prefab_instance_root(prefab)
                .unwrap(),
            WorkspacePrefabInstanceRootResolution::FieldUnavailable
        );
    }

    #[test]
    fn keeps_wrong_class_path_mismatch_and_duplicate_prefab_roots_explicit() {
        let (mut wrong_class_workspace, prefab) = prefab_root_workspace(Some((
            200,
            SceneRelationshipTarget::LocalObject { object_index: 1 },
        )));
        wrong_class_workspace.non_scene_collections[0].scene_assets[1]
            .identity
            .class_id = Some(23);
        assert_eq!(
            wrong_class_workspace
                .resolve_prefab_instance_root(prefab)
                .unwrap(),
            WorkspacePrefabInstanceRootResolution::TargetClassMismatch {
                target: identity(0, 1, 200, 23),
                actual_class_id: Some(23),
            }
        );

        let (path_mismatch_workspace, prefab) = prefab_root_workspace(Some((
            201,
            SceneRelationshipTarget::LocalObject { object_index: 1 },
        )));
        assert_eq!(
            path_mismatch_workspace
                .resolve_prefab_instance_root(prefab)
                .unwrap(),
            WorkspacePrefabInstanceRootResolution::TargetIdentityPathMismatch {
                expected_path_id: 201,
                actual_path_id: 200,
            }
        );

        let (mut duplicate_workspace, prefab) = prefab_root_workspace(Some((
            200,
            SceneRelationshipTarget::LocalObject { object_index: 1 },
        )));
        duplicate_workspace.non_scene_collections[0]
            .relationships
            .push(relationship(
                prefab,
                SceneRelationshipKind::PrefabInstanceRootGameObject,
                "m_RootGameObject",
                200,
                SceneRelationshipTarget::LocalObject { object_index: 1 },
            ));
        duplicate_workspace.non_scene_collections[0]
            .prefab_root_relationship_index
            .push(WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: prefab.object_index,
                relationship_index: 1,
            });
        assert_eq!(
            duplicate_workspace
                .resolve_prefab_instance_root(prefab)
                .unwrap(),
            WorkspacePrefabInstanceRootResolution::DuplicateRootReferences { match_count: 2 }
        );
    }

    #[test]
    fn component_pointer_field_match_is_limited_to_inherited_game_object_field() {
        assert!(is_game_object_pointer_path("m_GameObject"));
        assert!(is_game_object_pointer_path("Base.m_GameObject"));
        assert!(is_game_object_pointer_path("Base.Base.m_GameObject"));
        assert!(!is_game_object_pointer_path("Other.m_GameObject"));
        assert!(!is_game_object_pointer_path("m_GameObject.child"));
        assert!(!is_game_object_pointer_path("m_GameObject[0]"));
    }

    #[test]
    fn fetch_hierarchy_preserves_component_and_child_order_and_uses_last_transform() {
        let workspace = workspace_hierarchy_fixture(false);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let walk = workspace
            .fetch_game_object_hierarchy(root, WorkspaceGameObjectHierarchyWalkLimits::default())
            .unwrap();
        assert!(walk.is_complete(), "{:?}", walk.issues());
        assert_eq!(
            walk.entries()
                .iter()
                .map(|entry| (entry.identity, entry.kind))
                .collect::<Vec<_>>(),
            vec![
                (root, WorkspaceGameObjectHierarchyEntryKind::GameObject),
                (
                    identity(0, 3, 4, 23),
                    WorkspaceGameObjectHierarchyEntryKind::Component,
                ),
                (
                    identity(0, 1, 2, TRANSFORM_CLASS_ID),
                    WorkspaceGameObjectHierarchyEntryKind::Component,
                ),
                (
                    identity(0, 2, 3, RECT_TRANSFORM_CLASS_ID),
                    WorkspaceGameObjectHierarchyEntryKind::Component,
                ),
                (
                    identity(1, 1, 11, GAME_OBJECT_CLASS_ID),
                    WorkspaceGameObjectHierarchyEntryKind::GameObject,
                ),
                (
                    identity(1, 0, 10, TRANSFORM_CLASS_ID),
                    WorkspaceGameObjectHierarchyEntryKind::Component,
                ),
            ]
        );
    }

    #[test]
    fn fetch_hierarchy_reports_cycles_and_unresolved_component_edges() {
        let mut workspace = workspace_hierarchy_fixture(true);
        workspace.non_scene_collections[0].relationships[0].target =
            SceneRelationshipTarget::MissingLocalObject { path_id: 4 };
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let walk = workspace
            .fetch_game_object_hierarchy(root, WorkspaceGameObjectHierarchyWalkLimits::default())
            .unwrap();
        assert_eq!(walk.entries().len(), 5);
        assert_eq!(walk.issues().len(), 2);
        assert!(walk.issues().iter().any(|issue| matches!(
            issue,
            WorkspaceGameObjectHierarchyIssue::UnresolvedComponentReference {
                game_object,
                relationship_index: 0,
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 4 },
            } if *game_object == root
        )));
        assert!(walk.issues().iter().any(|issue| matches!(
            issue,
            WorkspaceGameObjectHierarchyIssue::CycleDetected { game_object }
                if *game_object == identity(1, 1, 11, GAME_OBJECT_CLASS_ID)
        )));
        assert!(!walk.is_complete());
    }

    #[test]
    fn fetch_hierarchy_fails_when_depth_or_entry_bounds_are_exceeded() {
        let workspace = workspace_hierarchy_fixture(false);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        assert!(matches!(
            workspace.fetch_game_object_hierarchy(
                root,
                WorkspaceGameObjectHierarchyWalkLimits {
                    max_depth: 0,
                    ..WorkspaceGameObjectHierarchyWalkLimits::default()
                }
            ),
            Err(
                WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                    resource: "GameObject hierarchy depth",
                    requested: 1,
                    limit: 0,
                }
            )
        ));
        assert!(matches!(
            workspace.fetch_game_object_hierarchy(
                root,
                WorkspaceGameObjectHierarchyWalkLimits {
                    max_entries: 2,
                    ..WorkspaceGameObjectHierarchyWalkLimits::default()
                }
            ),
            Err(
                WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                    resource: "GameObject hierarchy entries",
                    requested: 3,
                    limit: 2,
                }
            )
        ));
    }

    #[test]
    fn get_root_uses_first_transform_and_follows_cross_file_fathers() {
        let workspace = workspace_hierarchy_fixture(false);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let first_transform = identity(0, 1, 2, TRANSFORM_CLASS_ID);
        let child = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);
        let child_transform = identity(1, 0, 10, TRANSFORM_CLASS_ID);

        let root_walk = workspace
            .get_root_game_object(root, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(root_walk.is_complete());
        assert_eq!(root_walk.first_transform, Some(first_transform));
        assert_eq!(root_walk.transform_chain, vec![first_transform]);
        assert_eq!(root_walk.root_game_object, Some(root));
        assert_eq!(
            root_walk.termination,
            Some(WorkspaceGetRootGameObjectTermination::NullFather)
        );

        let child_walk = workspace
            .get_root_game_object(child, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(child_walk.is_complete());
        assert_eq!(child_walk.first_transform, Some(child_transform));
        assert_eq!(
            child_walk.transform_chain,
            vec![child_transform, first_transform]
        );
        assert_eq!(child_walk.root_game_object, Some(root));
    }

    #[test]
    fn get_root_handles_added_transform_and_reports_unresolved_or_cyclic_fathers() {
        let mut no_transform_workspace = workspace_hierarchy_fixture(false);
        let root_graph = &mut no_transform_workspace.non_scene_collections[0];
        root_graph.game_object_hierarchies[0].component_relationship_end = 2;
        root_graph
            .game_object_component_relationship_indices
            .truncate(2);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let no_transform = no_transform_workspace
            .get_root_game_object(root, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(no_transform.is_complete());
        assert_eq!(no_transform.root_game_object, Some(root));
        assert_eq!(
            no_transform.termination,
            Some(WorkspaceGetRootGameObjectTermination::NoTransformComponent)
        );

        let mut unresolved_workspace = workspace_hierarchy_fixture(false);
        unresolved_workspace.non_scene_collections[1].relationships[2].path_id = 404;
        unresolved_workspace.non_scene_collections[1].relationships[2].target =
            SceneRelationshipTarget::MissingLocalObject { path_id: 404 };
        let child = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);
        let unresolved = unresolved_workspace
            .get_root_game_object(child, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(matches!(
            unresolved.issue,
            Some(WorkspaceGetRootGameObjectIssue::UnresolvedTransformParent {
                transform,
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 404 },
            }) if transform == identity(1, 0, 10, TRANSFORM_CLASS_ID)
        ));

        let mut null_parent_owner_workspace = workspace_hierarchy_fixture(false);
        null_parent_owner_workspace.non_scene_collections[0].relationships[6].target =
            SceneRelationshipTarget::Null;
        let null_parent_owner = null_parent_owner_workspace
            .get_root_game_object(child, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(null_parent_owner.is_complete());
        assert_eq!(null_parent_owner.root_game_object, Some(child));
        assert_eq!(
            null_parent_owner.termination,
            Some(WorkspaceGetRootGameObjectTermination::ParentHasNullGameObject)
        );

        let mut cycle_workspace = workspace_hierarchy_fixture(false);
        let first_transform = identity(0, 1, 2, TRANSFORM_CLASS_ID);
        cycle_workspace.non_scene_collections[0].relationships[7].path_id = 10;
        cycle_workspace.non_scene_collections[0].relationships[7].target =
            SceneRelationshipTarget::WorkspaceObject {
                file_index: 1,
                object_index: 0,
            };
        let cycle = cycle_workspace
            .get_root_game_object(child, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(matches!(
            cycle.issue,
            Some(WorkspaceGetRootGameObjectIssue::CycleDetected { transform })
                if transform == identity(1, 0, 10, TRANSFORM_CLASS_ID)
        ));
        assert!(cycle.transform_chain.contains(&first_transform));
    }

    #[test]
    fn get_root_enforces_parent_depth_limit() {
        let workspace = workspace_hierarchy_fixture(false);
        let child = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);
        assert!(matches!(
            workspace.get_root_game_object(
                child,
                WorkspaceGetRootGameObjectLimits {
                    max_parent_depth: 1,
                }
            ),
            Err(WorkspaceGetRootGameObjectError::ParentDepthLimitExceeded {
                requested: 2,
                limit: 1,
            })
        ));
    }

    #[test]
    fn prefab_processing_seeds_scene_objects_before_existing_instances() {
        let mut workspace = workspace_hierarchy_fixture(false);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let child = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);
        let prefab = identity(1, 2, 12, PREFAB_INSTANCE_CLASS_ID);
        let null_prefab = identity(1, 3, 13, PREFAB_INSTANCE_CLASS_ID);
        let missing_root_prefab = identity(1, 4, 14, PREFAB_INSTANCE_CLASS_ID);

        let child_collection = &mut workspace.non_scene_collections[1];
        child_collection.relationships.push(relationship(
            prefab,
            SceneRelationshipKind::PrefabInstanceRootGameObject,
            "m_RootGameObject",
            child.path_id,
            SceneRelationshipTarget::LocalObject {
                object_index: child.object_index,
            },
        ));
        child_collection.relationships.push(relationship(
            null_prefab,
            SceneRelationshipKind::PrefabInstanceRootGameObject,
            "m_RootGameObject",
            0,
            SceneRelationshipTarget::Null,
        ));
        child_collection.prefab_root_relationship_index = vec![
            WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: prefab.object_index,
                relationship_index: 3,
            },
            WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: null_prefab.object_index,
                relationship_index: 4,
            },
        ];
        child_collection
            .scene_assets
            .push(WorkspaceSceneAssetIdentity {
                identity: prefab,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            });
        child_collection
            .scene_assets
            .push(WorkspaceSceneAssetIdentity {
                identity: null_prefab,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            });
        child_collection
            .scene_assets
            .push(WorkspaceSceneAssetIdentity {
                identity: missing_root_prefab,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            });

        let scene_collection = workspace.non_scene_collections.remove(0);
        workspace.scenes.push(WorkspaceSceneHierarchyScene {
            plan: SceneDefinitionPlan {
                collection_file_index: 0,
                collection_name: "scene".to_owned(),
                name: "scene".to_owned(),
                path: "Assets/Scenes/scene".to_owned(),
                path_source: SceneDefinitionPathSource::CollectionNameFallback,
                guid: SceneDefinitionGuidPlan::GenerateAtExport(
                    SceneGuidGenerationReason::MissingFromInput,
                ),
            },
            collections: vec![scene_collection],
        });
        workspace.collection_graph_index[0] = WorkspaceSceneCollectionGraphIndexEntry {
            file_index: 0,
            scene_index: Some(0),
            collection_index: 0,
        };
        workspace.collection_graph_index[1].collection_index = 0;

        let plan =
            plan_workspace_prefab_processing(&workspace, &[1, 0], Default::default()).unwrap();
        assert!(!plan.is_complete());
        assert_eq!(plan.processed_game_objects(), &[root, child]);
        assert!(matches!(
            plan.events()[0],
            WorkspacePrefabProcessingEvent::SceneGameObjectMarked {
                scene_index: 0,
                game_object,
                newly_marked: true,
            } if game_object == root
        ));
        assert!(matches!(
            plan.events()[1],
            WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyCreated {
                prefab_instance,
                root_game_object,
                set_prefab_internal: true,
                ..
            } if prefab_instance == prefab && root_game_object == child
        ));
        assert!(matches!(
            plan.events()[2],
            WorkspacePrefabProcessingEvent::ExistingPrefabInstanceNullRoot {
                prefab_instance,
            } if prefab_instance == null_prefab
        ));
        assert!(matches!(
            plan.events()[3],
            WorkspacePrefabProcessingEvent::ExistingPrefabInstanceRootUnavailable {
                prefab_instance,
                resolution: WorkspacePrefabInstanceRootResolution::FieldUnavailable,
            } if prefab_instance == missing_root_prefab
        ));
        assert!(matches!(
            plan.events()[4],
            WorkspacePrefabProcessingEvent::RemainingGameObjectSkippedAlreadyProcessed {
                game_object,
            } if game_object == child
        ));
        assert!(matches!(
            plan.events()[5],
            WorkspacePrefabProcessingEvent::RemainingGameObjectSkippedAlreadyProcessed {
                game_object,
            } if game_object == root
        ));
        let report = plan.report();
        assert_eq!(report.existing_prefab_instances, 3);
        assert_eq!(report.existing_prefab_hierarchies, 1);
        assert_eq!(report.root_resolution_issues, 1);
    }

    #[test]
    fn prefab_processing_uses_source_order_and_deduplicates_hierarchy_game_objects() {
        let workspace = workspace_hierarchy_fixture(false);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let child = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);
        let plan =
            plan_workspace_prefab_processing(&workspace, &[1, 0], Default::default()).unwrap();

        assert!(plan.is_complete());
        assert_eq!(plan.processed_game_objects(), &[root, child]);
        assert!(matches!(
            &plan.events()[0],
            WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyCreated {
                source_game_object,
                root_game_object,
                hierarchy,
                ..
            } if *source_game_object == child
                && *root_game_object == root
                && hierarchy.is_complete()
        ));
        assert!(matches!(
            plan.events()[1],
            WorkspacePrefabProcessingEvent::RemainingGameObjectSkippedAlreadyProcessed {
                game_object,
            } if game_object == root
        ));
        assert_eq!(plan.hierarchy_entry_count(), 6);
    }
