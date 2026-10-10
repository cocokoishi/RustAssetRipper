/// The asset kinds `SceneHierarchyObject.Create` sorts on, in source arm order.
///
/// Interface checks (`IGameObject`, `ILevelGameManager`, `ISceneRoots`, â€¦) and the
/// `IsComponentOnGameObject` pointer test need live objects and stay caller-side;
/// the caller reports the winning arm, including which side of the MonoBehaviour
/// gate the asset falls on.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SceneHierarchyAssetKind {
    /// `IGameObject`.
    GameObject,
    /// `IMonoBehaviour` with `IsComponentOnGameObject`.
    AttachedMonoBehaviour,
    /// `IMonoBehaviour` without a GameObject: matched by no arm, ignored.
    DetachedMonoBehaviour,
    /// Any other `IComponent`.
    Component,
    /// `ILevelGameManager`.
    LevelGameManager,
    /// `IPrefabInstance`.
    PrefabInstance,
    /// `ISceneRoots`.
    SceneRoots,
    /// Anything else: matched by no arm, ignored.
    Other,
}

/// Which list `SceneHierarchyObject.Create` files the asset into.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SceneHierarchySlot {
    /// `GameObjects`.
    GameObjects,
    /// `Components`.
    Components,
    /// `Managers`.
    Managers,
    /// `PrefabInstances`.
    PrefabInstances,
    /// `SceneRoots` (a single assignment: repeated roots overwrite, caller keeps last).
    SceneRoots,
    /// Matched by no arm; the asset lands in no list.
    Ignored,
}

/// `SceneHierarchyObject.Create` (`AssetRipper/Source/AssetRipper.Processing/Prefabs/SceneHierarchyObject.cs`
/// lines 82-108): GameObjects, attached MonoBehaviours, and other components file into
/// their lists; managers, prefab instances, and scene roots file into theirs; detached
/// MonoBehaviours and anything else are ignored.
pub const fn classify_scene_hierarchy_asset(kind: SceneHierarchyAssetKind) -> SceneHierarchySlot {
    match kind {
        SceneHierarchyAssetKind::GameObject => SceneHierarchySlot::GameObjects,
        SceneHierarchyAssetKind::AttachedMonoBehaviour | SceneHierarchyAssetKind::Component => {
            SceneHierarchySlot::Components
        }
        SceneHierarchyAssetKind::LevelGameManager => SceneHierarchySlot::Managers,
        SceneHierarchyAssetKind::PrefabInstance => SceneHierarchySlot::PrefabInstances,
        SceneHierarchyAssetKind::SceneRoots => SceneHierarchySlot::SceneRoots,
        SceneHierarchyAssetKind::DetachedMonoBehaviour | SceneHierarchyAssetKind::Other => {
            SceneHierarchySlot::Ignored
        }
    }
}

/// `SceneHierarchyObject.FetchDependencies` edge labels in walk order
/// (`AssetRipper/Source/AssetRipper.Processing/Prefabs/SceneHierarchyObject.cs` lines 43-54
/// with `GameObjectHierarchyObject.FetchDependencies` first): one `{List}[]` label per
/// member of `GameObjects`, `Components`, `PrefabInstances`, and `Managers`, then a single
/// `SceneRoots` label when a root block applies. Counts arrive as caller facts; pointer
/// resolution stays caller-side.
pub fn scene_hierarchy_dependency_fields(
    game_objects: usize,
    components: usize,
    prefab_instances: usize,
    managers: usize,
    has_scene_roots: bool,
) -> Vec<String> {
    let mut fields = Vec::new();
    for _ in 0..game_objects {
        fields.push("GameObjects[]".to_owned());
    }
    for _ in 0..components {
        fields.push("Components[]".to_owned());
    }
    for _ in 0..prefab_instances {
        fields.push("PrefabInstances[]".to_owned());
    }
    for _ in 0..managers {
        fields.push("Managers[]".to_owned());
    }
    if has_scene_roots {
        fields.push("SceneRoots".to_owned());
    }
    fields
}

/// Caller-reported `GameObjectExtensions.IsRoot` facts for one hierarchy GameObject.
///
/// Facts arrive in `GameObjects` order; the component lookup and the father-pointer
/// resolution (including cross-file targets) stay caller-side.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneHierarchyRootFact {
    /// Whether the GameObject carries a Transform component.
    pub has_transform: bool,
    /// Whether that Transform's father pointer resolved to null.
    /// Ignored when there is no Transform.
    pub father_is_null: bool,
}

/// `GameObjectExtensions.IsRoot` (`AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/GameObjectExtensions.cs`
/// lines 117-120) over caller facts: a GameObject is a root when it has no Transform
/// (the source `?.` yields null) or its Transform father is null.
pub const fn is_scene_hierarchy_root(fact: SceneHierarchyRootFact) -> bool {
    !fact.has_transform || fact.father_is_null
}

/// `SceneHierarchyObject.GetRoots` (`AssetRipper/Source/AssetRipper.Processing/Prefabs/SceneHierarchyObject.cs`
/// lines 73-76): hierarchy-order indexes of the GameObjects that are roots.
///
/// This is the export-time filter over the assembled hierarchy; `discover_local_roots`
/// runs the same rule during graph discovery with diagnostics for unresolved links.
pub fn scene_hierarchy_root_indexes(facts: &[SceneHierarchyRootFact]) -> Vec<usize> {
    let mut roots = Vec::new();
    for (index, fact) in facts.iter().enumerate() {
        if is_scene_hierarchy_root(*fact) {
            roots.push(index);
        }
    }
    roots
}
