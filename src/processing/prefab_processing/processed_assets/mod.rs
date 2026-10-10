mod dependencies;
mod planner;
mod scene_bundle_names;

use std::collections::{HashMap, HashSet};

use crate::serialized_file::object::SerializedValue;
use crate::serialized_file::object_workspace::ObjectWorkspace;

use super::identity_plan::WorkspacePrefabProcessingError;
use super::*;
use crate::processing::scene_hierarchy::{
    WorkspaceGeneratedTransformDefaults, WorkspaceSceneHierarchies, WorkspaceSceneObjectIdentity,
};

pub use self::dependencies::animator_controller_source_dependency_inventory;
pub use self::dependencies::AnimatorControllerSourceDependencyInventory;
pub use self::scene_bundle_names::{
    plan_workspace_scene_hierarchy_bundle_names, scene_hierarchy_asset_bundle_name,
    WorkspaceBundleAssetNameFact, WorkspaceSceneBundleSource,
    WorkspaceSceneHierarchyBundleAssignment, WorkspaceSceneHierarchyBundleName,
    WorkspaceSceneHierarchyBundlePlanLimits,
};
/// The source `ProcessedBundle.AddNewProcessedCollection` order used by PrefabProcessor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceProcessedCollectionKind {
    PrefabHierarchies,
    GeneratedPrefabs,
    MissingPrefabTransforms,
    SceneGeneratedAssets { scene_index: usize },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WorkspaceProcessedAssetIdentity {
    pub collection_index: usize,
    pub path_id: i64,
    pub class_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorkspaceProcessedPrefabReference {
    Existing(WorkspaceSceneObjectIdentity),
    Generated(WorkspaceProcessedAssetIdentity),
}

#[derive(Clone, Debug, PartialEq)]
pub enum WorkspaceProcessedPrefabAssetKind {
    SceneHierarchy {
        scene_index: usize,
    },
    GeneratedTransform {
        game_object: WorkspaceSceneObjectIdentity,
        default_values: WorkspaceGeneratedTransformDefaults,
    },
    GeneratedPrefabInstance {
        root_game_object: WorkspaceSceneObjectIdentity,
    },
    PrefabHierarchy {
        root_game_object: WorkspaceSceneObjectIdentity,
        prefab: WorkspaceProcessedPrefabReference,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceProcessedPrefabAsset {
    pub identity: WorkspaceProcessedAssetIdentity,
    pub kind: WorkspaceProcessedPrefabAssetKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceProcessedAssetCollectionPlan {
    pub collection_index: usize,
    pub kind: WorkspaceProcessedCollectionKind,
    pub name: String,
    pub assets: Vec<WorkspaceProcessedPrefabAsset>,
    /// Dependency indexes known from generated constructor-time PPtr assignments, including self at fileID 0.
    pub dependencies: Vec<WorkspaceProcessedCollectionDependency>,
    /// False when this collection has other baseline PPtr fields not yet replayed by Rust.
    pub dependency_indices_complete: bool,
    next_path_id: i64,
    source_dependency_index: HashMap<usize, i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceProcessedDependencyTarget {
    ProcessedCollection { collection_index: usize },
    SerializedFileCollection { file_index: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceProcessedCollectionDependency {
    pub file_id: i32,
    pub target: WorkspaceProcessedDependencyTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceGeneratedPointerField {
    TransformGameObject,
    PrefabInstanceRootGameObject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceGeneratedConstructorPointer {
    pub source_asset: WorkspaceProcessedAssetIdentity,
    pub field: WorkspaceGeneratedPointerField,
    pub file_id: i32,
    pub path_id: i64,
    pub target: WorkspaceSceneObjectIdentity,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WorkspaceSourceDependencyAppendTarget {
    GeneratedAnimatorControllerDependencies,
    GeneratedLightingDataAssets,
    PrefabProcessedCollection { collection_index: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceSourceDependencyPointerField {
    AnimatorControllerLayerStateMachine,
    LightmapSettingsLightingDataAsset,
    GameObjectComponent,
}

/// Source-derived identity for a StateMachine generated from one AnimatorController constant.
/// The eventual generated collection PathID has not yet been allocated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceAnimatorControllerStateMachineIdentity {
    pub source_controller: WorkspaceSceneObjectIdentity,
    pub state_machine_index: usize,
    pub source_layer_index: usize,
}

/// One ordered dependency-table append performed while processing a source SerializedAssetCollection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSourceCollectionDependencyAppend {
    pub source_file_index: usize,
    pub target_collection: WorkspaceSourceDependencyAppendTarget,
    pub pointer_field: WorkspaceSourceDependencyPointerField,
    pub first_source_owner: WorkspaceSceneObjectIdentity,
    /// Present when the generated asset identity is part of the Prefab plan.
    pub target_asset: Option<WorkspaceProcessedAssetIdentity>,
    /// Present for the first generated StateMachine referenced by an AnimatorController layer.
    pub target_animator_state_machine: Option<WorkspaceAnimatorControllerStateMachineIdentity>,
    /// Dependency FileIDs start after self and the original external table.
    pub file_id: Option<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceProcessedPrefabAssetPlanLimits {
    pub max_collections: usize,
    pub max_assets: usize,
    pub max_component_additions: usize,
    pub max_component_overlay_slots: usize,
    pub max_dependency_entries: usize,
    pub max_dependencies_per_collection: usize,
    pub max_constructor_pointers: usize,
    pub max_source_dependency_appends: usize,
    pub max_source_dependency_entries_per_collection: usize,
    pub max_missing_transform_candidates: usize,
    pub max_total_collection_name_bytes: usize,
}

impl Default for WorkspaceProcessedPrefabAssetPlanLimits {
    fn default() -> Self {
        Self {
            max_collections: 1_000_000,
            max_assets: 20_000_000,
            max_component_additions: 20_000_000,
            max_component_overlay_slots: 40_000_000,
            max_dependency_entries: 20_000_000,
            max_dependencies_per_collection: 1_000_000,
            max_constructor_pointers: 20_000_000,
            max_source_dependency_appends: 20_000_000,
            max_source_dependency_entries_per_collection: 25_000_001,
            max_missing_transform_candidates: 20_000_000,
            max_total_collection_name_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Ordered, identity-only PathID assignments for PrefabProcessor's virtual collections.
///
/// This does not create Unity objects, mutate the source graph, or serialize generated assets.
/// If source processing or any selected collection graph is incomplete, no PathIDs are claimed.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceProcessedPrefabAssetPlan {
    pub(super) complete: bool,
    pub(super) incomplete_reason: Option<String>,
    pub(super) collections: Vec<WorkspaceProcessedAssetCollectionPlan>,
    pub(super) component_additions: Vec<WorkspaceGeneratedComponentAddition>,
    pub(super) component_list_overlays: Vec<WorkspaceGameObjectComponentListOverlay>,
    pub(super) component_overlay_slot_count: usize,
    pub(super) constructor_pointers: Vec<WorkspaceGeneratedConstructorPointer>,
    pub(super) dependency_entry_count: usize,
    pub(super) source_dependency_appends: Vec<WorkspaceSourceCollectionDependencyAppend>,
    pub(super) modeled_animator_controller_dependency_append_count: usize,
    pub(super) deferred_source_dependency_file_count: usize,
    pub(super) asset_count: usize,
    pub(super) total_collection_name_bytes: usize,
}

impl WorkspaceProcessedPrefabAssetPlan {
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn incomplete_reason(&self) -> Option<&str> {
        self.incomplete_reason.as_deref()
    }

    pub fn collections(&self) -> &[WorkspaceProcessedAssetCollectionPlan] {
        &self.collections
    }

    pub fn component_additions(&self) -> &[WorkspaceGeneratedComponentAddition] {
        &self.component_additions
    }

    pub fn component_list_overlays(&self) -> &[WorkspaceGameObjectComponentListOverlay] {
        &self.component_list_overlays
    }

    pub fn constructor_pointers(&self) -> &[WorkspaceGeneratedConstructorPointer] {
        &self.constructor_pointers
    }

    pub fn source_dependency_appends(&self) -> &[WorkspaceSourceCollectionDependencyAppend] {
        &self.source_dependency_appends
    }

    pub fn deferred_source_dependency_file_count(&self) -> usize {
        self.deferred_source_dependency_file_count
    }

    pub fn modeled_animator_controller_dependency_append_count(&self) -> usize {
        self.modeled_animator_controller_dependency_append_count
    }

    pub fn asset_count(&self) -> usize {
        self.asset_count
    }
}
/// Replays source-created collection and asset order after the identity-level PrefabProcessor plan.
pub fn plan_workspace_processed_prefab_assets(
    workspace: &WorkspaceSceneHierarchies,
    processing: &WorkspacePrefabProcessingPlan,
    source_external_identifier_counts: &[usize],
    animator_controller_inventory: &AnimatorControllerSourceDependencyInventory,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<WorkspaceProcessedPrefabAssetPlan, WorkspacePrefabProcessingError> {
    planner::plan_workspace_processed_prefab_assets(
        workspace,
        processing,
        source_external_identifier_counts,
        animator_controller_inventory,
        limits,
    )
}
fn incomplete_processed_asset_plan(reason: &str) -> WorkspaceProcessedPrefabAssetPlan {
    WorkspaceProcessedPrefabAssetPlan {
        complete: false,
        incomplete_reason: Some(reason.to_owned()),
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

pub(super) fn add_processed_collection(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    kind: WorkspaceProcessedCollectionKind,
    name: &str,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<usize, WorkspacePrefabProcessingError> {
    let requested = plan.collections.len().checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed asset collections",
        },
    )?;
    if requested > limits.max_collections {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed asset collections",
            requested,
            limit: limits.max_collections,
        });
    }
    let requested_name_bytes = plan
        .total_collection_name_bytes
        .checked_add(name.len())
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed asset collection names",
        })?;
    if requested_name_bytes > limits.max_total_collection_name_bytes {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed asset collection name bytes",
            requested: requested_name_bytes,
            limit: limits.max_total_collection_name_bytes,
        });
    }
    plan.collections.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed asset collections",
            requested,
        }
    })?;
    if limits.max_dependencies_per_collection == 0 {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed collection dependency entries",
            requested: 1,
            limit: limits.max_dependencies_per_collection,
        });
    }
    let requested_dependency_entries = plan.dependency_entry_count.checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed collection dependency entries",
        },
    )?;
    if requested_dependency_entries > limits.max_dependency_entries {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed collection dependency entries",
            requested: requested_dependency_entries,
            limit: limits.max_dependency_entries,
        });
    }
    let mut owned_name = String::new();
    owned_name.try_reserve_exact(name.len()).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed asset collection name",
            requested: name.len(),
        }
    })?;
    owned_name.push_str(name);
    let collection_index = plan.collections.len();
    let mut dependencies = Vec::new();
    dependencies.try_reserve_exact(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed collection dependency entries",
            requested: 1,
        }
    })?;
    dependencies.push(WorkspaceProcessedCollectionDependency {
        file_id: 0,
        target: WorkspaceProcessedDependencyTarget::ProcessedCollection { collection_index },
    });
    plan.collections
        .push(WorkspaceProcessedAssetCollectionPlan {
            collection_index,
            dependency_indices_complete: kind
                != WorkspaceProcessedCollectionKind::PrefabHierarchies,
            kind,
            name: owned_name,
            assets: Vec::new(),
            dependencies,
            next_path_id: 0,
            source_dependency_index: HashMap::new(),
        });
    plan.total_collection_name_bytes = requested_name_bytes;
    plan.dependency_entry_count = requested_dependency_entries;
    Ok(collection_index)
}

pub(super) fn add_processed_source_file_dependency(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    collection_index: usize,
    source_file_index: usize,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<i32, WorkspacePrefabProcessingError> {
    let collection = plan.collections.get_mut(collection_index).ok_or_else(|| {
        processed_asset_operation_error(format!(
            "processed asset collection index {collection_index} is unavailable"
        ))
    })?;
    if let Some(&file_id) = collection.source_dependency_index.get(&source_file_index) {
        return Ok(file_id);
    }
    let requested_collection_entries = collection.dependencies.len().checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed collection dependency entries",
        },
    )?;
    if requested_collection_entries > limits.max_dependencies_per_collection {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed collection dependency entries",
            requested: requested_collection_entries,
            limit: limits.max_dependencies_per_collection,
        });
    }
    let requested_total_entries = plan.dependency_entry_count.checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed collection dependency entries",
        },
    )?;
    if requested_total_entries > limits.max_dependency_entries {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed collection dependency entries",
            requested: requested_total_entries,
            limit: limits.max_dependency_entries,
        });
    }
    let file_id = i32::try_from(collection.dependencies.len()).map_err(|_| {
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed collection PPtr FileID",
        }
    })?;
    collection.dependencies.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed collection dependency entries",
            requested: requested_collection_entries,
        }
    })?;
    collection
        .source_dependency_index
        .try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed collection dependency lookup",
            requested: requested_collection_entries - 1,
        })?;
    collection
        .dependencies
        .push(WorkspaceProcessedCollectionDependency {
            file_id,
            target: WorkspaceProcessedDependencyTarget::SerializedFileCollection {
                file_index: source_file_index,
            },
        });
    collection
        .source_dependency_index
        .insert(source_file_index, file_id);
    plan.dependency_entry_count = requested_total_entries;
    Ok(file_id)
}

pub(super) fn add_generated_constructor_pointer(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    pointer: WorkspaceGeneratedConstructorPointer,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    if pointer.path_id == 0 || pointer.target.path_id != pointer.path_id {
        return Err(processed_asset_operation_error(format!(
            "generated {:?} pointer for collection {} PathID {} has a null or mismatched target",
            pointer.field, pointer.source_asset.collection_index, pointer.source_asset.path_id
        )));
    }
    let requested = plan.constructor_pointers.len().checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "generated constructor-time PPtrs",
        },
    )?;
    if requested > limits.max_constructor_pointers {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "generated constructor-time PPtrs",
            requested,
            limit: limits.max_constructor_pointers,
        });
    }
    plan.constructor_pointers.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "generated constructor-time PPtrs",
            requested,
        }
    })?;
    plan.constructor_pointers.push(pointer);
    Ok(())
}

fn ensure_scene_processed_collection(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    scene_collection_by_index: &mut [Option<usize>],
    scene_index: usize,
    scene_name: &str,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<usize, WorkspacePrefabProcessingError> {
    let scene_count = scene_collection_by_index.len();
    let slot = scene_collection_by_index
        .get_mut(scene_index)
        .ok_or_else(|| {
            processed_asset_operation_error(format!(
                "SceneDefinition index {scene_index} is outside {} scenes",
                scene_count
            ))
        })?;
    if let Some(collection_index) = *slot {
        return Ok(collection_index);
    }
    const SUFFIX: &str = " (Generated Assets)";
    let name_length = scene_name.len().checked_add(SUFFIX.len()).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "scene generated collection name",
        },
    )?;
    let mut name = String::new();
    name.try_reserve_exact(name_length).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "scene generated collection name",
            requested: name_length,
        }
    })?;
    name.push_str(scene_name);
    name.push_str(SUFFIX);
    let collection_index = add_processed_collection(
        plan,
        WorkspaceProcessedCollectionKind::SceneGeneratedAssets { scene_index },
        &name,
        limits,
    )?;
    *slot = Some(collection_index);
    Ok(collection_index)
}

pub(super) fn add_processed_asset(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    collection_index: usize,
    class_id: i32,
    kind: WorkspaceProcessedPrefabAssetKind,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<WorkspaceProcessedAssetIdentity, WorkspacePrefabProcessingError> {
    let requested_count =
        plan.asset_count
            .checked_add(1)
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "generated processed assets",
            })?;
    if requested_count > limits.max_assets {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "generated processed assets",
            requested: requested_count,
            limit: limits.max_assets,
        });
    }
    let collection = plan.collections.get_mut(collection_index).ok_or_else(|| {
        processed_asset_operation_error(format!(
            "processed asset collection index {collection_index} is unavailable"
        ))
    })?;
    let path_id = collection.next_path_id.checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed collection PathID",
        },
    )?;
    collection.assets.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed collection assets",
            requested: collection.assets.len().saturating_add(1),
        }
    })?;
    let identity = WorkspaceProcessedAssetIdentity {
        collection_index,
        path_id,
        class_id,
    };
    collection
        .assets
        .push(WorkspaceProcessedPrefabAsset { identity, kind });
    collection.next_path_id = path_id;
    plan.asset_count = requested_count;
    Ok(identity)
}

pub(super) fn processed_asset_operation_error(message: String) -> WorkspacePrefabProcessingError {
    WorkspacePrefabProcessingError::WorkspaceOperationFailed {
        operation: "processed prefab asset allocation planning",
        message,
    }
}
