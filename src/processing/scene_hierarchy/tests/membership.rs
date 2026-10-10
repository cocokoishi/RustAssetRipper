use super::*;

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
        Err(
            crate::export::scene_collection::WorkspaceSceneExportPlanError::IncompleteMembership { .. }
        )
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
