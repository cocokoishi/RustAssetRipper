use super::*;

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
fn fetch_hierarchy_projects_unresolved_components_as_absent_and_reports_cycles() {
    let mut workspace = workspace_hierarchy_fixture(true);
    workspace.non_scene_collections[0].relationships[0].target =
        SceneRelationshipTarget::MissingLocalObject { path_id: 4 };
    let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
    let walk = workspace
        .fetch_game_object_hierarchy(root, WorkspaceGameObjectHierarchyWalkLimits::default())
        .unwrap();
    assert_eq!(walk.entries().len(), 5);
    assert_eq!(walk.issues().len(), 1);
    assert!(!walk.issues().iter().any(|issue| matches!(
        issue,
        WorkspaceGameObjectHierarchyIssue::UnresolvedComponentReference { .. }
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
