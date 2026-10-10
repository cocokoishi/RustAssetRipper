use super::*;

#[test]
fn hierarchy_create_sorts_every_arm_into_its_list() {
    assert_eq!(
        classify_scene_hierarchy_asset(SceneHierarchyAssetKind::GameObject),
        SceneHierarchySlot::GameObjects
    );
    // Only MonoBehaviours on a GameObject join Components; detached ones are ignored.
    assert_eq!(
        classify_scene_hierarchy_asset(SceneHierarchyAssetKind::AttachedMonoBehaviour),
        SceneHierarchySlot::Components
    );
    assert_eq!(
        classify_scene_hierarchy_asset(SceneHierarchyAssetKind::Component),
        SceneHierarchySlot::Components
    );
    assert_eq!(
        classify_scene_hierarchy_asset(SceneHierarchyAssetKind::DetachedMonoBehaviour),
        SceneHierarchySlot::Ignored
    );
    assert_eq!(
        classify_scene_hierarchy_asset(SceneHierarchyAssetKind::LevelGameManager),
        SceneHierarchySlot::Managers
    );
    assert_eq!(
        classify_scene_hierarchy_asset(SceneHierarchyAssetKind::PrefabInstance),
        SceneHierarchySlot::PrefabInstances
    );
    assert_eq!(
        classify_scene_hierarchy_asset(SceneHierarchyAssetKind::SceneRoots),
        SceneHierarchySlot::SceneRoots
    );
    assert_eq!(
        classify_scene_hierarchy_asset(SceneHierarchyAssetKind::Other),
        SceneHierarchySlot::Ignored
    );
}

#[test]
fn hierarchy_dependency_fields_follow_the_fetch_order() {
    assert_eq!(
        scene_hierarchy_dependency_fields(2, 1, 1, 1, true),
        vec![
            "GameObjects[]",
            "GameObjects[]",
            "Components[]",
            "PrefabInstances[]",
            "Managers[]",
            "SceneRoots",
        ]
    );
    // The SceneRoots edge exists only when a root block applies.
    assert_eq!(
        scene_hierarchy_dependency_fields(1, 0, 0, 0, false),
        vec!["GameObjects[]"]
    );
    assert!(scene_hierarchy_dependency_fields(0, 0, 0, 0, false).is_empty());
}

#[test]
fn hierarchy_root_predicate_matches_the_is_root_rule() {
    use super::SceneHierarchyRootFact;
    // No Transform: the source `?.` yields null, so the GameObject is a root.
    assert!(super::is_scene_hierarchy_root(SceneHierarchyRootFact {
        has_transform: false,
        father_is_null: false,
    }));
    // A Transform with a null father is a root; a set father is not.
    assert!(super::is_scene_hierarchy_root(SceneHierarchyRootFact {
        has_transform: true,
        father_is_null: true,
    }));
    assert!(!super::is_scene_hierarchy_root(SceneHierarchyRootFact {
        has_transform: true,
        father_is_null: false,
    }));
}

#[test]
fn get_roots_keeps_hierarchy_order_and_reports_no_roots_explicitly() {
    use super::SceneHierarchyRootFact;
    let facts = [
        SceneHierarchyRootFact {
            has_transform: true,
            father_is_null: false,
        },
        SceneHierarchyRootFact {
            has_transform: true,
            father_is_null: true,
        },
        SceneHierarchyRootFact {
            has_transform: false,
            father_is_null: false,
        },
        SceneHierarchyRootFact {
            has_transform: true,
            father_is_null: false,
        },
    ];
    assert_eq!(super::scene_hierarchy_root_indexes(&facts), vec![1, 2]);
    assert!(super::scene_hierarchy_root_indexes(&facts[0..1]).is_empty());
    assert!(super::scene_hierarchy_root_indexes(&[]).is_empty());
}
