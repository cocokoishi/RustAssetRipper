use super::*;
use crate::processing::prefab_processing::tests::support::*;
use crate::serialized_file::object::SerializedFieldValue;

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
            target_collection: WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
            pointer_field: WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
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
            target_collection: WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
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
            target_collection: WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
            pointer_field: WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
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
            target_collection: WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                collection_index: 2,
            },
            pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
            target_asset: None,
            target_animator_state_machine: None,
        },
        WorkspaceProcessedPrefabAssetPlanLimits::default(),
    )
    .unwrap();
    let mut animator_controller_inventory = AnimatorControllerSourceDependencyInventory::default();
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
    let mut animator_controller_inventory = AnimatorControllerSourceDependencyInventory::default();
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
            target_collection: WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
            pointer_field: WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
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
            target_collection: WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
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
    let controller = |layer_array_name: &str, layer_indices: &[u32], state_machine_count: usize| {
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
        animator_controller_requires_source_dependency(&controller("m_HumanLayerArray", &[], 1)),
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
