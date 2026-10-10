//! Identity planning and generated-asset allocation for source PrefabProcessor passes.

mod generated_objects;
mod identity_plan;
mod processed_assets;

pub use self::generated_objects::{
    materialize_game_object_component_append, materialize_planned_game_object_component_append,
    materialize_planned_generated_transform_payload, WorkspaceGameObjectComponentListOverlay,
    WorkspaceGeneratedComponentAddition, WorkspaceGeneratedTransformPayload,
    WorkspaceSourceGameObjectComponentSlot,
};
pub use self::identity_plan::{
    plan_workspace_prefab_processing, WorkspacePrefabHierarchyPlan,
    WorkspacePrefabHierarchyPlanEntry, WorkspacePrefabProcessingError,
    WorkspacePrefabProcessingEvent, WorkspacePrefabProcessingLimits, WorkspacePrefabProcessingPlan,
    WorkspacePrefabProcessingReport,
};
pub use self::processed_assets::{
    animator_controller_source_dependency_inventory, plan_workspace_processed_prefab_assets,
    plan_workspace_scene_hierarchy_bundle_names, scene_hierarchy_asset_bundle_name,
    AnimatorControllerSourceDependencyInventory, WorkspaceAnimatorControllerStateMachineIdentity,
    WorkspaceBundleAssetNameFact, WorkspaceGeneratedConstructorPointer,
    WorkspaceGeneratedPointerField, WorkspaceProcessedAssetCollectionPlan,
    WorkspaceProcessedAssetIdentity, WorkspaceProcessedCollectionDependency,
    WorkspaceProcessedCollectionKind, WorkspaceProcessedDependencyTarget,
    WorkspaceProcessedPrefabAsset, WorkspaceProcessedPrefabAssetKind,
    WorkspaceProcessedPrefabAssetPlan, WorkspaceProcessedPrefabAssetPlanLimits,
    WorkspaceProcessedPrefabReference, WorkspaceSceneBundleSource,
    WorkspaceSceneHierarchyBundleAssignment, WorkspaceSceneHierarchyBundleName,
    WorkspaceSceneHierarchyBundlePlanLimits, WorkspaceSourceCollectionDependencyAppend,
    WorkspaceSourceDependencyAppendTarget, WorkspaceSourceDependencyPointerField,
};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
