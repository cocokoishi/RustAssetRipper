    use super::*;
    use crate::io::binary::Endian;
    use crate::serialized_file::metadata::{SerializedType, TypeTree, TypeTreeNode};
    use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
    use crate::serialized_file::object::SerializedFieldValue;

    fn empty_plan() -> WorkspaceProcessedPrefabAssetPlan {
        WorkspaceProcessedPrefabAssetPlan {
            complete: true,
            incomplete_reason: None,
            collections: Vec::new(),
            component_additions: Vec::new(),
            component_list_overlays: Vec::new(),
            component_overlay_slot_count: 0,
            constructor_pointers: Vec::new(),
            dependency_entry_count: 0,
            source_dependency_appends: Vec::new(),
            modeled_animator_controller_dependency_append_count: 0,
            deferred_source_dependency_file_count: 0,
            asset_count: 0,
            total_collection_name_bytes: 0,
        }
    }

    fn tree_node(level: u8, name: &str, type_name: &str, byte_size: i32) -> TypeTreeNode {
        TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size,
            index: 0,
            meta_flags: 0,
            reference_type_hash: None,
        }
    }

    fn transform_metadata(
        include_euler_hint: bool,
        include_root_order: bool,
    ) -> SerializedFileMetadata {
        let mut nodes = vec![
            tree_node(0, "", "Transform", -1),
            tree_node(1, "Base", "Component", -1),
            tree_node(2, "m_GameObject", "PPtr<GameObject>", 12),
            tree_node(3, "m_FileID", "SInt32", 4),
            tree_node(3, "m_PathID", "SInt64", 8),
            tree_node(1, "m_Father", "PPtr<Transform>", 12),
            tree_node(2, "m_FileID", "SInt32", 4),
            tree_node(2, "m_PathID", "SInt64", 8),
            tree_node(1, "m_Children", "vector", -1),
            tree_node(2, "Array", "Array", -1),
            tree_node(3, "size", "int", 4),
            tree_node(3, "data", "PPtr<Transform>", 12),
            tree_node(4, "m_FileID", "SInt32", 4),
            tree_node(4, "m_PathID", "SInt64", 8),
        ];
        if include_root_order {
            nodes.push(tree_node(1, "m_RootOrder", "int", 4));
        }
        if include_euler_hint {
            nodes.extend([
                tree_node(1, "m_LocalEulerAnglesHint", "Vector3f", -1),
                tree_node(2, "x", "float", 4),
                tree_node(2, "y", "float", 4),
                tree_node(2, "z", "float", 4),
            ]);
        }
        nodes.extend([
            tree_node(1, "m_LocalPosition", "Vector3f", -1),
            tree_node(2, "x", "float", 4),
            tree_node(2, "y", "float", 4),
            tree_node(2, "z", "float", 4),
            tree_node(1, "m_LocalRotation", "Quaternionf", -1),
            tree_node(2, "x", "float", 4),
            tree_node(2, "y", "float", 4),
            tree_node(2, "z", "float", 4),
            tree_node(2, "w", "float", 4),
            tree_node(1, "m_LocalScale", "Vector3f", -1),
            tree_node(2, "x", "float", 4),
            tree_node(2, "y", "float", 4),
            tree_node(2, "z", "float", 4),
        ]);
        SerializedFileMetadata {
            prefix: SerializedFileMetadataPrefix {
                metadata_offset: 0,
                metadata_size: 0,
                body_endian: Endian::Little,
                unity_version_text: Some("2019.4.31f1".to_owned()),
                target_platform: Some(0),
                has_type_tree: true,
                consumed_bytes: 0,
            },
            types: vec![SerializedType {
                raw_type_id: TRANSFORM_CLASS_ID,
                type_id_for_hash: TRANSFORM_CLASS_ID,
                is_stripped_type: false,
                script_type_index: -1,
                script_id: None,
                old_type_hash: None,
                type_tree: Some(TypeTree {
                    nodes,
                    string_buffer: Vec::new(),
                }),
                type_dependencies: Vec::new(),
            }],
            long_file_id: None,
            objects: Vec::new(),
            script_types: Vec::new(),
            externals: Vec::new(),
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        }
    }

    fn generated_transform_plan() -> (
        WorkspaceProcessedPrefabAssetPlan,
        WorkspaceProcessedAssetIdentity,
        WorkspaceSceneObjectIdentity,
    ) {
        let mut plan = empty_plan();
        let limits = WorkspaceProcessedPrefabAssetPlanLimits::default();
        let collection_index = add_processed_collection(
            &mut plan,
            WorkspaceProcessedCollectionKind::MissingPrefabTransforms,
            "Missing Prefab Transforms",
            limits,
        )
        .unwrap();
        let game_object = WorkspaceSceneObjectIdentity {
            file_index: 3,
            object_index: 7,
            path_id: 99,
            class_id: Some(1),
        };
        let asset = add_processed_asset(
            &mut plan,
            collection_index,
            TRANSFORM_CLASS_ID,
            WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
                game_object,
                default_values: default_workspace_generated_transform_values(),
            },
            limits,
        )
        .unwrap();
        let file_id = add_processed_source_file_dependency(
            &mut plan,
            collection_index,
            game_object.file_index,
            limits,
        )
        .unwrap();
        add_generated_constructor_pointer(
            &mut plan,
            WorkspaceGeneratedConstructorPointer {
                source_asset: asset,
                field: WorkspaceGeneratedPointerField::TransformGameObject,
                file_id,
                path_id: game_object.path_id,
                target: game_object,
            },
            limits,
        )
        .unwrap();
        (plan, asset, game_object)
    }

    #[test]
    fn processed_collection_dependency_indexes_follow_first_touch_order_and_deduplicate() {
        let mut plan = empty_plan();
        let limits = WorkspaceProcessedPrefabAssetPlanLimits::default();
        let collection_index = add_processed_collection(
            &mut plan,
            WorkspaceProcessedCollectionKind::GeneratedPrefabs,
            "Generated Prefabs",
            limits,
        )
        .unwrap();

        assert_eq!(
            add_processed_source_file_dependency(&mut plan, collection_index, 9, limits).unwrap(),
            1
        );
        assert_eq!(
            add_processed_source_file_dependency(&mut plan, collection_index, 4, limits).unwrap(),
            2
        );
        assert_eq!(
            add_processed_source_file_dependency(&mut plan, collection_index, 9, limits).unwrap(),
            1
        );
        assert_eq!(
            plan.collections[collection_index].dependencies,
            vec![
                WorkspaceProcessedCollectionDependency {
                    file_id: 0,
                    target: WorkspaceProcessedDependencyTarget::ProcessedCollection {
                        collection_index,
                    },
                },
                WorkspaceProcessedCollectionDependency {
                    file_id: 1,
                    target: WorkspaceProcessedDependencyTarget::SerializedFileCollection {
                        file_index: 9,
                    },
                },
                WorkspaceProcessedCollectionDependency {
                    file_id: 2,
                    target: WorkspaceProcessedDependencyTarget::SerializedFileCollection {
                        file_index: 4,
                    },
                },
            ]
        );
    }

    #[test]
    fn processed_collection_dependency_index_budget_is_enforced() {
        let mut plan = empty_plan();
        let limits = WorkspaceProcessedPrefabAssetPlanLimits {
            max_dependencies_per_collection: 2,
            ..Default::default()
        };
        let collection_index = add_processed_collection(
            &mut plan,
            WorkspaceProcessedCollectionKind::MissingPrefabTransforms,
            "Missing Prefab Transforms",
            limits,
        )
        .unwrap();
        add_processed_source_file_dependency(&mut plan, collection_index, 0, limits).unwrap();

        assert!(matches!(
            add_processed_source_file_dependency(&mut plan, collection_index, 1, limits),
            Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "processed collection dependency entries",
                requested: 3,
                limit: 2,
            })
        ));
    }

    #[test]
    fn source_dependency_append_requests_deduplicate_by_file_and_processed_collection() {
        let mut plan = empty_plan();
        let mut seen = HashSet::new();
        let first_owner = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 0,
            path_id: 10,
            class_id: Some(1),
        };
        let later_owner = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 1,
            path_id: 11,
            class_id: Some(1),
        };
        let target_asset = WorkspaceProcessedAssetIdentity {
            collection_index: 2,
            path_id: 1,
            class_id: 4,
        };
        let target_collection = WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
            collection_index: 2,
        };
        let limits = WorkspaceProcessedPrefabAssetPlanLimits::default();

        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner: first_owner,
                target_collection,
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: Some(target_asset),
                target_animator_state_machine: None,
            },
            limits,
        )
        .unwrap();
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner: later_owner,
                target_collection,
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: Some(target_asset),
                target_animator_state_machine: None,
            },
            limits,
        )
        .unwrap();

        assert_eq!(plan.source_dependency_appends.len(), 1);
        assert_eq!(
            plan.source_dependency_appends[0].first_source_owner,
            first_owner
        );
        assert_eq!(
            plan.source_dependency_appends[0].target_collection,
            target_collection
        );
        assert_eq!(plan.source_dependency_appends[0].file_id, None);
    }

    #[test]
    fn source_dependency_file_ids_follow_prior_processor_appends_and_external_identifiers() {
        let mut plan = empty_plan();
        let mut seen = HashSet::new();
        let lightmap_settings = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 0,
            path_id: 10,
            class_id: Some(157),
        };
        let game_object = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 1,
            path_id: 11,
            class_id: Some(1),
        };
        let limits = WorkspaceProcessedPrefabAssetPlanLimits::default();

        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner: lightmap_settings,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
                pointer_field:
                    WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
                target_asset: None,
                target_animator_state_machine: None,
            },
            limits,
        )
        .unwrap();
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner: game_object,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index: 2,
                    },
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: Some(WorkspaceProcessedAssetIdentity {
                    collection_index: 2,
                    path_id: 1,
                    class_id: 4,
                }),
                target_animator_state_machine: None,
            },
            limits,
        )
        .unwrap();

        assign_source_dependency_file_ids(
            &mut plan,
            &[3],
            &AnimatorControllerSourceDependencyInventory::default(),
            limits,
        )
        .unwrap();

        assert_eq!(plan.source_dependency_appends[0].file_id, Some(4));
        assert_eq!(plan.source_dependency_appends[1].file_id, Some(5));
    }

    #[test]
    fn unknown_animator_controller_effect_defers_only_requested_file_ids() {
        let mut plan = empty_plan();
        let mut seen = HashSet::new();
        let owner = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 1,
            path_id: 11,
            class_id: Some(1),
        };
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
                pointer_field:
                    WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
                target_asset: None,
                target_animator_state_machine: None,
            },
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index: 2,
                    },
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: None,
                target_animator_state_machine: None,
            },
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        let mut animator_controller_inventory =
            AnimatorControllerSourceDependencyInventory::default();
        animator_controller_inventory
            .deferred_file_indices
            .insert(0);

        assign_source_dependency_file_ids(
            &mut plan,
            &[3],
            &animator_controller_inventory,
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();

        assert!(!plan.is_complete());
        assert_eq!(plan.deferred_source_dependency_file_count(), 1);
        assert!(plan
            .source_dependency_appends
            .iter()
            .all(|append| append.file_id.is_none()));
        assert!(plan
            .incomplete_reason()
            .unwrap()
            .contains("AnimatorController source dependency effects remain unknown"));
    }

    #[test]
    fn animator_controller_dependency_appends_precede_lighting_data_and_prefab_file_ids() {
        let mut plan = empty_plan();
        let mut seen = HashSet::new();
        let controller = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 3,
            path_id: 31,
            class_id: Some(91),
        };
        let owner = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 1,
            path_id: 11,
            class_id: Some(1),
        };
        let target = WorkspaceAnimatorControllerStateMachineIdentity {
            source_controller: controller,
            state_machine_index: 2,
            source_layer_index: 0,
        };
        let mut animator_controller_inventory =
            AnimatorControllerSourceDependencyInventory::default();
        animator_controller_inventory.append_file_indices.insert(0);
        animator_controller_inventory.append_targets.push(target);
        plan_animator_controller_source_dependency_appends(
            &mut plan,
            &mut seen,
            &animator_controller_inventory,
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
                pointer_field:
                    WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
                target_asset: None,
                target_animator_state_machine: None,
            },
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index: 2,
                    },
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: None,
                target_animator_state_machine: None,
            },
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();

        assign_source_dependency_file_ids(
            &mut plan,
            &[3],
            &animator_controller_inventory,
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();

        assert_eq!(
            plan.modeled_animator_controller_dependency_append_count(),
            1
        );
        assert_eq!(plan.source_dependency_appends.len(), 3);
        assert_eq!(
            plan.source_dependency_appends[0].target_collection,
            WorkspaceSourceDependencyAppendTarget::GeneratedAnimatorControllerDependencies
        );
        assert_eq!(
            plan.source_dependency_appends[0].pointer_field,
            WorkspaceSourceDependencyPointerField::AnimatorControllerLayerStateMachine
        );
        assert_eq!(
            plan.source_dependency_appends[0].target_animator_state_machine,
            Some(target)
        );
        assert_eq!(plan.source_dependency_appends[0].file_id, Some(4));
        assert_eq!(plan.source_dependency_appends[1].file_id, Some(5));
        assert_eq!(plan.source_dependency_appends[2].file_id, Some(6));
        assert!(plan.is_complete());
    }

    #[test]
    fn animator_controller_layer_inventory_checks_array_aliases_and_indices() {
        let controller =
            |layer_array_name: &str, layer_indices: &[u32], state_machine_count: usize| {
                let layers = layer_indices
                    .iter()
                    .map(|index| {
                        SerializedValue::Struct(vec![SerializedFieldValue {
                            name: "Data".to_owned(),
                            value: SerializedValue::Struct(vec![SerializedFieldValue {
                                name: "m_StateMachineIndex".to_owned(),
                                value: SerializedValue::U32(*index),
                            }]),
                        }])
                    })
                    .collect();
                SerializedValue::Struct(vec![SerializedFieldValue {
                    name: "m_Controller".to_owned(),
                    value: SerializedValue::Struct(vec![
                        SerializedFieldValue {
                            name: layer_array_name.to_owned(),
                            value: SerializedValue::Array(layers),
                        },
                        SerializedFieldValue {
                            name: "m_StateMachineArray".to_owned(),
                            value: SerializedValue::Array(
                                (0..state_machine_count)
                                    .map(|_| SerializedValue::Struct(Vec::new()))
                                    .collect(),
                            ),
                        },
                    ]),
                }])
            };

        assert_eq!(
            animator_controller_requires_source_dependency(&controller("m_LayerArray", &[0, 1], 2)),
            Some(Some(0))
        );
        let direct_layer_value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Controller".to_owned(),
            value: SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "m_LayerArray".to_owned(),
                    value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                        SerializedFieldValue {
                            name: "m_StateMachineIndex".to_owned(),
                            value: SerializedValue::U32(0),
                        },
                    ])]),
                },
                SerializedFieldValue {
                    name: "m_StateMachineArray".to_owned(),
                    value: SerializedValue::Array(vec![SerializedValue::Struct(Vec::new())]),
                },
            ]),
        }]);
        assert_eq!(
            animator_controller_requires_source_dependency(&direct_layer_value),
            Some(Some(0))
        );
        assert_eq!(
            animator_controller_requires_source_dependency(&controller(
                "m_HumanLayerArray",
                &[],
                1
            )),
            Some(None)
        );
        assert_eq!(
            animator_controller_requires_source_dependency(&controller("m_LayerArray", &[1], 1)),
            None
        );
    }

    #[test]
    fn animator_controller_layer_inventory_rejects_missing_ambiguous_and_mistyped_fields() {
        let both_layer_arrays = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Controller".to_owned(),
            value: SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "m_LayerArray".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                },
                SerializedFieldValue {
                    name: "m_HumanLayerArray".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                },
                SerializedFieldValue {
                    name: "m_StateMachineArray".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                },
            ]),
        }]);
        assert_eq!(
            animator_controller_requires_source_dependency(&both_layer_arrays),
            None
        );

        let mistyped_layer_index = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Controller".to_owned(),
            value: SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "m_LayerArray".to_owned(),
                    value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                        SerializedFieldValue {
                            name: "Data".to_owned(),
                            value: SerializedValue::Struct(vec![SerializedFieldValue {
                                name: "m_StateMachineIndex".to_owned(),
                                value: SerializedValue::StringBytes(b"0".to_vec()),
                            }]),
                        },
                    ])]),
                },
                SerializedFieldValue {
                    name: "m_StateMachineArray".to_owned(),
                    value: SerializedValue::Array(vec![SerializedValue::Struct(Vec::new())]),
                },
            ]),
        }]);
        assert_eq!(
            animator_controller_requires_source_dependency(&mistyped_layer_index),
            None
        );
    }

    #[test]
    fn materializes_game_object_component_append_without_changing_source_slots() {
        let game_object = WorkspaceSceneObjectIdentity {
            file_index: 3,
            object_index: 2,
            path_id: 10,
            class_id: Some(1),
        };
        let original_elements = vec![
            SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            },
            SerializedValue::PPtr {
                file_id: 2,
                path_id: 88,
            },
        ];
        let source_slots = vec![
            WorkspaceSourceGameObjectComponentSlot {
                source_array_index: 0,
                file_id: 0,
                path_id: 0,
                target: SceneRelationshipTarget::Null,
            },
            WorkspaceSourceGameObjectComponentSlot {
                source_array_index: 1,
                file_id: 2,
                path_id: 88,
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 88 },
            },
        ];
        let generated_component = WorkspaceGeneratedComponentAddition {
            game_object,
            component_class_id: 4,
            appended_index: 2,
            component_asset: WorkspaceProcessedAssetIdentity {
                collection_index: 5,
                path_id: 200,
                class_id: 4,
            },
        };
        let overlay = WorkspaceGameObjectComponentListOverlay {
            game_object,
            source_slots,
            generated_component,
        };
        let source_payload = ParsedObjectPayload {
            object_index: game_object.object_index,
            path_id: game_object.path_id,
            type_name: "GameObject".to_owned(),
            root: SerializedValue::Struct(vec![SerializedFieldValue {
                name: "Base".to_owned(),
                value: SerializedValue::Struct(vec![SerializedFieldValue {
                    name: "m_Component".to_owned(),
                    value: SerializedValue::Array(original_elements.clone()),
                }]),
            }]),
            consumed_bytes: 16,
            trailing_bytes: vec![0xaa],
            allocated_bytes: 64,
        };

        let edited =
            materialize_game_object_component_append(&source_payload, &overlay, 3, None, 3)
                .unwrap();
        let component_array = edited
            .root
            .unique_field_in_base_chain("m_Component")
            .unwrap()
            .unwrap()
            .array_elements()
            .unwrap();
        assert_eq!(&component_array[..2], original_elements);
        assert_eq!(
            component_array[2].pptr(),
            Some((3, generated_component.component_asset.path_id))
        );
        assert_eq!(edited.trailing_bytes, [0xaa]);

        let wrapper_payload = ParsedObjectPayload {
            root: SerializedValue::Struct(vec![SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    SerializedFieldValue {
                        name: "component".to_owned(),
                        value: SerializedValue::PPtr {
                            file_id: 0,
                            path_id: 13,
                        },
                    },
                ])]),
            }]),
            ..source_payload.clone()
        };
        let wrapper_overlay = WorkspaceGameObjectComponentListOverlay {
            source_slots: vec![WorkspaceSourceGameObjectComponentSlot {
                source_array_index: 0,
                file_id: 0,
                path_id: 13,
                target: SceneRelationshipTarget::LocalObject { object_index: 4 },
            }],
            generated_component: WorkspaceGeneratedComponentAddition {
                appended_index: 1,
                ..generated_component
            },
            ..overlay.clone()
        };
        let wrapper_edited = materialize_game_object_component_append(
            &wrapper_payload,
            &wrapper_overlay,
            3,
            None,
            2,
        )
        .unwrap();
        assert_eq!(
            wrapper_edited
                .root
                .unique_field_in_base_chain("m_Component")
                .unwrap()
                .unwrap()
                .array_elements()
                .unwrap()[1]
                .unique_field("component")
                .unwrap()
                .unwrap()
                .pptr(),
            Some((3, generated_component.component_asset.path_id))
        );
    }

    #[test]
    fn planned_materialization_uses_only_assigned_source_dependency_file_ids() {
        let game_object = WorkspaceSceneObjectIdentity {
            file_index: 3,
            object_index: 1,
            path_id: 12,
            class_id: Some(1),
        };
        let generated_component = WorkspaceGeneratedComponentAddition {
            game_object,
            component_class_id: 4,
            appended_index: 1,
            component_asset: WorkspaceProcessedAssetIdentity {
                collection_index: 5,
                path_id: 201,
                class_id: 4,
            },
        };
        let overlay = WorkspaceGameObjectComponentListOverlay {
            game_object,
            source_slots: vec![WorkspaceSourceGameObjectComponentSlot {
                source_array_index: 0,
                file_id: 0,
                path_id: 0,
                target: SceneRelationshipTarget::Null,
            }],
            generated_component,
        };
        let source_payload = ParsedObjectPayload {
            object_index: game_object.object_index,
            path_id: game_object.path_id,
            type_name: "GameObject".to_owned(),
            root: SerializedValue::Struct(vec![SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(vec![SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                }]),
            }]),
            consumed_bytes: 12,
            trailing_bytes: Vec::new(),
            allocated_bytes: 32,
        };
        let mut plan = empty_plan();
        plan.source_dependency_appends
            .push(WorkspaceSourceCollectionDependencyAppend {
                source_file_index: game_object.file_index,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index: generated_component.component_asset.collection_index,
                    },
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                first_source_owner: game_object,
                target_asset: Some(generated_component.component_asset),
                target_animator_state_machine: None,
                file_id: Some(7),
            });

        let edited = materialize_planned_game_object_component_append(
            &plan,
            &source_payload,
            &overlay,
            None,
            2,
        )
        .unwrap();
        assert_eq!(
            edited
                .root
                .unique_field_in_base_chain("m_Component")
                .unwrap()
                .unwrap()
                .array_elements()
                .unwrap()[1]
                .pptr(),
            Some((7, 201))
        );

        plan.source_dependency_appends[0].file_id = None;
        let deferred = materialize_planned_game_object_component_append(
            &plan,
            &source_payload,
            &overlay,
            None,
            2,
        )
        .unwrap_err();
        assert!(deferred
            .to_string()
            .contains("FileID is deferred or unavailable"));
    }

    #[test]
    fn requires_a_typed_template_for_empty_component_arrays_and_checks_limits() {
        let game_object = WorkspaceSceneObjectIdentity {
            file_index: 1,
            object_index: 0,
            path_id: 22,
            class_id: Some(1),
        };
        let overlay = WorkspaceGameObjectComponentListOverlay {
            game_object,
            source_slots: Vec::new(),
            generated_component: WorkspaceGeneratedComponentAddition {
                game_object,
                component_class_id: 4,
                appended_index: 0,
                component_asset: WorkspaceProcessedAssetIdentity {
                    collection_index: 2,
                    path_id: 1,
                    class_id: 4,
                },
            },
        };
        let payload = ParsedObjectPayload {
            object_index: 0,
            path_id: 22,
            type_name: "GameObject".to_owned(),
            root: SerializedValue::Struct(vec![SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(Vec::new()),
            }]),
            consumed_bytes: 0,
            trailing_bytes: Vec::new(),
            allocated_bytes: 0,
        };
        let missing_template =
            materialize_game_object_component_append(&payload, &overlay, 1, None, 1).unwrap_err();
        assert!(missing_template
            .to_string()
            .contains("TypeTree-derived element template"));

        let too_many = materialize_game_object_component_append(
            &payload,
            &overlay,
            1,
            Some(&SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            }),
            0,
        )
        .unwrap_err();
        assert!(too_many.to_string().contains("exceeding the limit of 0"));
        assert_eq!(
            payload
                .root
                .unique_field_in_base_chain("m_Component")
                .unwrap()
                .unwrap()
                .array_elements()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn materializes_generated_transform_defaults_and_planned_game_object_pointer() {
        let (plan, asset, game_object) = generated_transform_plan();
        let metadata = transform_metadata(true, true);

        let generated = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &metadata,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap();

        assert_eq!(generated.asset, asset);
        assert_eq!(generated.type_index, 0);
        assert_eq!(
            generated
                .root
                .unique_field_in_base_chain("m_GameObject")
                .unwrap()
                .unwrap()
                .pptr(),
            Some((1, game_object.path_id))
        );
        assert_eq!(
            generated.root.unique_field("m_Father").unwrap(),
            Some(&SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            })
        );
        assert_eq!(
            generated.root.unique_field("m_Children").unwrap(),
            Some(&SerializedValue::Array(Vec::new()))
        );
        assert_eq!(
            generated.root.unique_field("m_RootOrder").unwrap(),
            Some(&SerializedValue::I32(0))
        );
        assert_eq!(
            generated
                .root
                .unique_field("m_LocalEulerAnglesHint")
                .unwrap(),
            Some(&vector3_value(0.0, 0.0, 0.0))
        );
        assert_eq!(
            generated.root.unique_field("m_LocalPosition").unwrap(),
            Some(&vector3_value(0.0, 0.0, 0.0))
        );
        assert_eq!(
            generated.root.unique_field("m_LocalRotation").unwrap(),
            Some(&SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "x".to_owned(),
                    value: SerializedValue::F32(0.0),
                },
                SerializedFieldValue {
                    name: "y".to_owned(),
                    value: SerializedValue::F32(0.0),
                },
                SerializedFieldValue {
                    name: "z".to_owned(),
                    value: SerializedValue::F32(0.0),
                },
                SerializedFieldValue {
                    name: "w".to_owned(),
                    value: SerializedValue::F32(1.0),
                },
            ]))
        );
        assert_eq!(
            generated.root.unique_field("m_LocalScale").unwrap(),
            Some(&vector3_value(1.0, 1.0, 1.0))
        );

        let mut expected = Vec::new();
        expected.extend_from_slice(&1i32.to_le_bytes());
        expected.extend_from_slice(&game_object.path_id.to_le_bytes());
        expected.extend_from_slice(&0i32.to_le_bytes());
        expected.extend_from_slice(&0i64.to_le_bytes());
        expected.extend_from_slice(&0i32.to_le_bytes()); // m_Children size
        expected.extend_from_slice(&0i32.to_le_bytes()); // m_RootOrder
        for value in [0.0f32, 0.0, 0.0] {
            // Euler hint
            expected.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0f32, 0.0, 0.0] {
            // Local position
            expected.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0f32, 0.0, 0.0, 1.0] {
            // Local rotation
            expected.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1.0f32, 1.0, 1.0] {
            // Local scale
            expected.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(generated.bytes, expected);
    }

    #[test]
    fn generated_transform_yaml_resolves_its_constructor_pointer_to_a_local_scene_id() {
        let (plan, asset, game_object) = generated_transform_plan();
        let metadata = transform_metadata(true, true);
        let generated = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &metadata,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap();
        let constructor_pointer = plan.constructor_pointers()[0];
        let target_asset = crate::export::scene_collection::ExportAssetIdentity {
            collection:
                crate::export::scene_collection::ExportAssetCollectionIdentity::SerializedFile {
                    file_index: game_object.file_index,
                },
            object_index: game_object.object_index,
            path_id: game_object.path_id,
            class_id: 1,
        };
        let generated_asset = crate::export::scene_collection::ExportAssetIdentity {
            collection: crate::export::scene_collection::ExportAssetCollectionIdentity::
                ProcessedCollection {
                    collection_index: asset.collection_index,
                },
            object_index: 0,
            path_id: asset.path_id,
            class_id: asset.class_id,
        };
        let collection_index = 55;
        let pointer_map = crate::export::pointer_map::ProjectExportPointerMap::new(
            &[
                crate::export::pointer_map::ExportedAssetPointerEntry {
                    asset: target_asset,
                    file_id: game_object.path_id,
                    collection_index,
                },
                crate::export::pointer_map::ExportedAssetPointerEntry {
                    asset: generated_asset,
                    file_id: 123,
                    collection_index,
                },
            ],
            &[
                crate::export::pointer_map::ExportCollectionPointerMetadata {
                    collection_index,
                    guid: [0x55; 16],
                    asset_type: crate::export::pointer_map::UnityProjectAssetType::Serialized,
                },
            ],
            crate::export::pointer_map::ExportPointerMapLimits::default(),
        )
        .unwrap();

        let document =
            crate::export::workspace_yaml::generated_transform_payload_to_unity_yaml_document(
                &generated,
                &metadata,
                generated_asset,
                123,
                constructor_pointer,
                &pointer_map,
                collection_index,
                None,
                crate::export::value_yaml::UnityYamlObjectConversionLimits::default(),
            )
            .unwrap();
        let yaml = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();

        assert!(yaml.contains("--- !u!4 &123"), "{yaml}");
        assert!(
            yaml.contains(&format!(
                "m_GameObject: {{fileID: {}}}",
                game_object.path_id
            )),
            "{yaml}"
        );
        assert!(yaml.contains("m_LocalPosition:"), "{yaml}");
        assert!(yaml.contains("m_LocalScale:"), "{yaml}");
        assert!(!yaml.contains("guid:"), "{yaml}");

        let mut wrong_class_metadata = metadata.clone();
        wrong_class_metadata.types[0]
            .type_tree
            .as_mut()
            .unwrap()
            .nodes[0]
            .type_name = Some("NotTransform".to_owned());
        assert!(matches!(
            crate::export::workspace_yaml::generated_transform_payload_to_unity_yaml_document(
                &generated,
                &wrong_class_metadata,
                generated_asset,
                123,
                constructor_pointer,
                &pointer_map,
                collection_index,
                None,
                crate::export::value_yaml::UnityYamlObjectConversionLimits::default(),
            ),
            Err(crate::export::workspace_yaml::UnityYamlWorkspaceExportError::
                GeneratedTransformInvariant { .. })
        ));

        let mut wrong_pointer = generated.clone();
        *wrong_pointer
            .root
            .unique_field_in_base_chain_mut("m_Father")
            .unwrap()
            .unwrap() = SerializedValue::PPtr {
            file_id: constructor_pointer.file_id,
            path_id: constructor_pointer.path_id,
        };
        wrong_pointer.bytes = metadata
            .encode_object_payload_for_type(
                metadata.objects.len(),
                wrong_pointer.type_index,
                &wrong_pointer.root,
                &[],
                ObjectEncodeLimits::default(),
            )
            .unwrap();
        assert!(matches!(
            crate::export::workspace_yaml::generated_transform_payload_to_unity_yaml_document(
                &wrong_pointer,
                &metadata,
                generated_asset,
                123,
                constructor_pointer,
                &pointer_map,
                collection_index,
                None,
                crate::export::value_yaml::UnityYamlObjectConversionLimits::default(),
            ),
            Err(crate::export::workspace_yaml::UnityYamlWorkspaceExportError::
                GeneratedTransformInvariant { message, .. })
                if message.contains("unexpected non-null Transform PPtr")
        ));
    }

    fn vector3_value(x: f32, y: f32, z: f32) -> SerializedValue {
        SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "x".to_owned(),
                value: SerializedValue::F32(x),
            },
            SerializedFieldValue {
                name: "y".to_owned(),
                value: SerializedValue::F32(y),
            },
            SerializedFieldValue {
                name: "z".to_owned(),
                value: SerializedValue::F32(z),
            },
        ])
    }

    #[test]
    fn generated_transform_accepts_optional_fields_and_requires_transform_vectors() {
        let (plan, asset, _) = generated_transform_plan();
        let without_euler_hint = transform_metadata(false, true);
        let generated = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &without_euler_hint,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap();
        assert!(generated
            .root
            .unique_field("m_LocalEulerAnglesHint")
            .unwrap()
            .is_none());

        // 2019.4's class-4 TPK omits m_RootOrder; the source setter treats it as optional.
        let without_root_order = transform_metadata(false, false);
        let generated = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &without_root_order,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap();
        assert!(generated
            .root
            .unique_field("m_RootOrder")
            .unwrap()
            .is_none());

        // The position, rotation, and scale fields are unconditional factory writes.
        let mut without_scale = without_root_order;
        let nodes = &mut without_scale.types[0].type_tree.as_mut().unwrap().nodes;
        let scale_start = nodes
            .iter()
            .position(|node| node.name.as_deref() == Some("m_LocalScale"))
            .expect("the fixture has m_LocalScale");
        let mut scale_end = scale_start + 1;
        while scale_end < nodes.len() && nodes[scale_end].level > nodes[scale_start].level {
            scale_end += 1;
        }
        nodes.drain(scale_start..scale_end);
        let error = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &without_scale,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("no m_LocalScale field"));
    }

    #[test]
    fn generated_transform_rejects_wrong_type_and_dependency_identity() {
        let (plan, asset, _) = generated_transform_plan();
        let mut wrong_type = transform_metadata(true, true);
        wrong_type.types[0].raw_type_id = 224;
        let wrong_type_error = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &wrong_type,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap_err();
        assert!(wrong_type_error.to_string().contains("expected 4"));

        let mut wrong_dependency = plan.clone();
        wrong_dependency.collections[asset.collection_index].dependencies[1].target =
            WorkspaceProcessedDependencyTarget::SerializedFileCollection { file_index: 4 };
        let dependency_error = materialize_planned_generated_transform_payload(
            &wrong_dependency,
            asset,
            &transform_metadata(true, true),
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap_err();
        assert!(dependency_error
            .to_string()
            .contains("does not uniquely target its source SerializedFile"));
    }

    #[test]
    fn scene_hierarchy_bundle_name_stays_unset_without_a_serialized_bundle() {
        assert_eq!(
            scene_hierarchy_asset_bundle_name(false, "level0", None),
            WorkspaceSceneHierarchyBundleName::Unset
        );
        // The bundle lookup gates everything: even a resolved asset name is
        // ignored when no SerializedBundle was selected.
        assert_eq!(
            scene_hierarchy_asset_bundle_name(false, "level0", Some("scenes")),
            WorkspaceSceneHierarchyBundleName::Unset
        );
    }

    #[test]
    fn scene_hierarchy_bundle_name_falls_back_to_the_bundle_name() {
        assert_eq!(
            scene_hierarchy_asset_bundle_name(true, "level0", None),
            WorkspaceSceneHierarchyBundleName::BundleName("level0".to_owned())
        );
    }

    #[test]
    fn scene_hierarchy_bundle_name_prefers_the_asset_bundle_asset() {
        assert_eq!(
            scene_hierarchy_asset_bundle_name(true, "level0", Some("scenes")),
            WorkspaceSceneHierarchyBundleName::AssetName("scenes".to_owned())
        );
        // The resolved name wins even when it differs from the bundle name.
        assert_eq!(
            scene_hierarchy_asset_bundle_name(true, "shared", Some("shared")),
            WorkspaceSceneHierarchyBundleName::AssetName("shared".to_owned())
        );
    }

    fn bundle_source(bundle_index: usize, bundle_name: &str) -> WorkspaceSceneBundleSource {
        WorkspaceSceneBundleSource {
            bundle_index,
            bundle_name: bundle_name.to_owned(),
            is_serialized_bundle: true,
        }
    }

    fn asset_fact(
        resolved_name: &str,
        has_streamed_scene_flag: bool,
        is_streamed_scene_asset_bundle: bool,
    ) -> WorkspaceBundleAssetNameFact {
        WorkspaceBundleAssetNameFact {
            resolved_name: resolved_name.to_owned(),
            has_streamed_scene_flag,
            is_streamed_scene_asset_bundle,
        }
    }

    fn asset_bundle_root_with_name(name: &str) -> SerializedValue {
        SerializedValue::Struct(vec![crate::serialized_file::object::SerializedFieldValue {
            name: "m_AssetBundleName_R".to_owned(),
            value: SerializedValue::StringBytes(name.as_bytes().to_vec()),
        }])
    }

    #[test]
    fn scene_bundle_plan_composes_traversal_fetch_order_and_get_asset_bundle_name() {
        // The resolved names below come from the real `get_asset_bundle_name`
        // resolver, not invented strings: an explicit serialized name wins, an
        // empty serialized name falls back to the owning bundle name.
        let resolved_explicit = crate::processing::asset_bundle::get_asset_bundle_name(
            &asset_bundle_root_with_name("scenes"),
            "level0",
        )
        .unwrap();
        assert_eq!(resolved_explicit, "scenes");
        let resolved_fallback = crate::processing::asset_bundle::get_asset_bundle_name(
            &asset_bundle_root_with_name(""),
            "level0",
        )
        .unwrap();
        assert_eq!(resolved_fallback, "level0");

        // Scene 0 has two collections: the first collection belongs to bundle 7
        // and the second to bundle 9. Source `FirstOrDefault` must select bundle 7.
        // Scene 1 has no serialized bundle and stays unset. Scene 2 selects a
        // bundle without assets and falls back to the bundle name.
        let scene_collections = vec![vec![0usize, 1usize], vec![2usize], vec![3usize]];
        let file_bundles = vec![
            Some(bundle_source(7, "level0")),
            Some(bundle_source(9, "other")),
            None,
            Some(bundle_source(11, "empty")),
        ];
        let mut fetch_order = HashMap::new();
        // Bundle 7 holds two assets in FetchAssets() order: the first wins even
        // though the second sorts earlier. Bundle 11 has no entry at all.
        fetch_order.insert(
            7usize,
            vec![
                asset_fact(&resolved_explicit, false, false),
                asset_fact("aaa-first-sorted-but-second", false, false),
            ],
        );
        fetch_order.insert(11usize, Vec::new());
        let assignments = plan_workspace_scene_hierarchy_bundle_names(
            &scene_collections,
            &file_bundles,
            &fetch_order,
            WorkspaceSceneHierarchyBundlePlanLimits::default(),
        )
        .unwrap();
        assert_eq!(assignments.len(), 3);
        assert_eq!(assignments[0].scene_index, 0);
        assert_eq!(assignments[0].effective_asset_bundle_name, None);
        assert_eq!(
            assignments[0].name,
            WorkspaceSceneHierarchyBundleName::AssetName("scenes".to_owned())
        );
        assert_eq!(assignments[0].streamed_scene_assert_warning, None);
        assert_eq!(
            assignments[1].name,
            WorkspaceSceneHierarchyBundleName::Unset
        );
        assert_eq!(assignments[1].effective_asset_bundle_name, None);
        assert_eq!(
            assignments[2].name,
            WorkspaceSceneHierarchyBundleName::BundleName("empty".to_owned())
        );
        assert_eq!(assignments[2].effective_asset_bundle_name, None);
    }

    #[test]
    fn scene_bundle_plan_reports_the_streamed_scene_debug_assert_without_changing_the_name() {
        let scene_collections = vec![vec![0usize]];
        let file_bundles = vec![Some(bundle_source(3, "level0"))];
        let mut fetch_order = HashMap::new();
        // The source Debug.Assert fires when the version carries
        // IsStreamedSceneAssetBundle but the value is false. Release builds
        // continue with the resolved name; Rust records an explicit warning.
        fetch_order.insert(3usize, vec![asset_fact("scenes", true, false)]);
        let assignments = plan_workspace_scene_hierarchy_bundle_names(
            &scene_collections,
            &file_bundles,
            &fetch_order,
            WorkspaceSceneHierarchyBundlePlanLimits::default(),
        )
        .unwrap();
        assert_eq!(assignments.len(), 1);
        assert_eq!(
            assignments[0].name,
            WorkspaceSceneHierarchyBundleName::AssetName("scenes".to_owned())
        );
        let warning = assignments[0]
            .streamed_scene_assert_warning
            .as_ref()
            .expect("assert warning is recorded");
        assert!(warning.contains("IsStreamedSceneAssetBundle=false"));
        assert!(warning.contains("release continues"));

        // A streamed true value and a version without the field stay silent.
        let mut silent = HashMap::new();
        silent.insert(3usize, vec![asset_fact("scenes", true, true)]);
        let assignments = plan_workspace_scene_hierarchy_bundle_names(
            &scene_collections,
            &file_bundles,
            &silent,
            WorkspaceSceneHierarchyBundlePlanLimits::default(),
        )
        .unwrap();
        assert_eq!(assignments[0].streamed_scene_assert_warning, None);
    }

    #[test]
    fn scene_bundle_plan_rejects_invalid_indexes_and_enforces_limits() {
        // Unknown collection file indexes fail instead of selecting a bundle.
        let error = plan_workspace_scene_hierarchy_bundle_names(
            &[vec![5usize]],
            &[Some(bundle_source(1, "level0"))],
            &HashMap::new(),
            WorkspaceSceneHierarchyBundlePlanLimits::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("outside 1 files"));

        // Limits fail closed with explicit resources.
        let error = plan_workspace_scene_hierarchy_bundle_names(
            &[vec![0usize], vec![0usize]],
            &[Some(bundle_source(1, "level0"))],
            &HashMap::new(),
            WorkspaceSceneHierarchyBundlePlanLimits {
                max_scenes: 1,
                ..WorkspaceSceneHierarchyBundlePlanLimits::default()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("scene hierarchy bundle scenes"));
    }
