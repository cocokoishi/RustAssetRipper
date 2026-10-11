use super::*;

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
        source_prefab_processor_reference_scans_complete: true,
        source_scene_membership_reference_scans_complete: true,
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
        source_prefab_processor_reference_scans_complete: true,
        source_scene_membership_reference_scans_complete: true,
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
    let (null_workspace, prefab) = prefab_root_workspace(Some((0, SceneRelationshipTarget::Null)));
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
