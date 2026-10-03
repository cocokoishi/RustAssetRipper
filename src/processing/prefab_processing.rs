//! Identity-level reconstruction of AssetRipper's workspace PrefabProcessor order.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::scene::SceneRelationshipTarget;
use super::scene_hierarchy::{
    default_workspace_generated_transform_values, WorkspaceGameObjectHierarchyEntry,
    WorkspaceGameObjectHierarchyEntryKind, WorkspaceGameObjectHierarchyWalk,
    WorkspaceGameObjectHierarchyWalkError, WorkspaceGameObjectHierarchyWalkLimits,
    WorkspaceGeneratedTransformDefaults, WorkspaceGetRootGameObjectError,
    WorkspaceGetRootGameObjectLimits, WorkspaceGetRootGameObjectWalk,
    WorkspacePrefabInstanceRootResolution, WorkspaceSceneAssetCategory, WorkspaceSceneHierarchies,
    WorkspaceSceneObjectIdentity,
};

const TRANSFORM_CLASS_ID: i32 = 4;
const RECT_TRANSFORM_CLASS_ID: i32 = 224;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspacePrefabProcessingLimits {
    pub max_events: usize,
    pub max_processed_game_objects: usize,
    pub max_total_hierarchy_entries: usize,
    pub max_source_collection_file_indices: usize,
    pub get_root: WorkspaceGetRootGameObjectLimits,
    pub hierarchy_walk: WorkspaceGameObjectHierarchyWalkLimits,
}

impl Default for WorkspacePrefabProcessingLimits {
    fn default() -> Self {
        Self {
            max_events: 20_000_000,
            max_processed_game_objects: 20_000_000,
            max_total_hierarchy_entries: 20_000_000,
            max_source_collection_file_indices: 1_000_000,
            get_root: WorkspaceGetRootGameObjectLimits::default(),
            hierarchy_walk: WorkspaceGameObjectHierarchyWalkLimits::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum WorkspacePrefabHierarchyPlanEntry {
    Source(WorkspaceGameObjectHierarchyEntry),
    GeneratedTransform {
        game_object: WorkspaceSceneObjectIdentity,
        default_values: WorkspaceGeneratedTransformDefaults,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspacePrefabHierarchyPlan {
    source_walk: WorkspaceGameObjectHierarchyWalk,
    entries: Vec<WorkspacePrefabHierarchyPlanEntry>,
}

impl WorkspacePrefabHierarchyPlan {
    pub fn source_walk(&self) -> &WorkspaceGameObjectHierarchyWalk {
        &self.source_walk
    }

    pub fn entries(&self) -> &[WorkspacePrefabHierarchyPlanEntry] {
        &self.entries
    }

    pub fn issues(&self) -> &[super::scene_hierarchy::WorkspaceGameObjectHierarchyIssue] {
        self.source_walk.issues()
    }

    pub fn is_complete(&self) -> bool {
        self.source_walk.is_complete()
    }

    pub fn generated_transform_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    WorkspacePrefabHierarchyPlanEntry::GeneratedTransform { .. }
                )
            })
            .count()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum WorkspacePrefabProcessingEvent {
    SceneGameObjectMarked {
        scene_index: usize,
        game_object: WorkspaceSceneObjectIdentity,
        newly_marked: bool,
    },
    ExistingPrefabInstanceNullRoot {
        prefab_instance: WorkspaceSceneObjectIdentity,
    },
    ExistingPrefabInstanceRootUnavailable {
        prefab_instance: WorkspaceSceneObjectIdentity,
        resolution: WorkspacePrefabInstanceRootResolution,
    },
    ExistingPrefabInstanceSkippedAlreadyProcessed {
        prefab_instance: WorkspaceSceneObjectIdentity,
        root_game_object: WorkspaceSceneObjectIdentity,
    },
    ExistingPrefabHierarchyCreated {
        prefab_instance: WorkspaceSceneObjectIdentity,
        root_game_object: WorkspaceSceneObjectIdentity,
        set_prefab_internal: bool,
        hierarchy: WorkspacePrefabHierarchyPlan,
    },
    ExistingPrefabHierarchyIncomplete {
        prefab_instance: WorkspaceSceneObjectIdentity,
        root_game_object: WorkspaceSceneObjectIdentity,
        set_prefab_internal: bool,
        hierarchy: WorkspaceGameObjectHierarchyWalk,
    },
    RemainingGameObjectSkippedAlreadyProcessed {
        game_object: WorkspaceSceneObjectIdentity,
    },
    RemainingGameObjectRootAlreadyProcessed {
        game_object: WorkspaceSceneObjectIdentity,
        root_walk: WorkspaceGetRootGameObjectWalk,
    },
    RemainingGameObjectRootUnavailable {
        game_object: WorkspaceSceneObjectIdentity,
        root_walk: WorkspaceGetRootGameObjectWalk,
    },
    GeneratedPrefabHierarchyCreated {
        source_game_object: WorkspaceSceneObjectIdentity,
        root_game_object: WorkspaceSceneObjectIdentity,
        root_walk: WorkspaceGetRootGameObjectWalk,
        hierarchy: WorkspacePrefabHierarchyPlan,
    },
    GeneratedPrefabHierarchyIncomplete {
        source_game_object: WorkspaceSceneObjectIdentity,
        root_game_object: WorkspaceSceneObjectIdentity,
        root_walk: WorkspaceGetRootGameObjectWalk,
        hierarchy: WorkspaceGameObjectHierarchyWalk,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspacePrefabProcessingPlan {
    events: Vec<WorkspacePrefabProcessingEvent>,
    processed_game_objects: Vec<WorkspaceSceneObjectIdentity>,
    source_collection_file_indices: Vec<usize>,
    hierarchy_entry_count: usize,
    complete: bool,
    stopped_on_incomplete_hierarchy: bool,
}

impl WorkspacePrefabProcessingPlan {
    pub fn events(&self) -> &[WorkspacePrefabProcessingEvent] {
        &self.events
    }

    /// Collection traversal order supplied to `GameBundle.FetchAssets()` replay.
    pub fn source_collection_file_indices(&self) -> &[usize] {
        &self.source_collection_file_indices
    }

    /// GameObjects enter this list in the order the source processor first adds them to its set.
    pub fn processed_game_objects(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.processed_game_objects
    }

    pub fn hierarchy_entry_count(&self) -> usize {
        self.hierarchy_entry_count
    }

    /// False when a root reference is indeterminate or a hierarchy traversal stopped early.
    pub fn is_complete(&self) -> bool {
        self.complete && !self.stopped_on_incomplete_hierarchy
    }

    pub fn stopped_on_incomplete_hierarchy(&self) -> bool {
        self.stopped_on_incomplete_hierarchy
    }

    pub fn report(&self) -> WorkspacePrefabProcessingReport {
        let mut report = WorkspacePrefabProcessingReport {
            processed_game_objects: self.processed_game_objects.len(),
            hierarchy_entries: self.hierarchy_entry_count,
            complete: self.is_complete(),
            stopped_on_incomplete_hierarchy: self.stopped_on_incomplete_hierarchy,
            ..WorkspacePrefabProcessingReport::default()
        };
        for event in &self.events {
            match event {
                WorkspacePrefabProcessingEvent::SceneGameObjectMarked { .. } => {
                    report.scene_game_object_records += 1;
                }
                WorkspacePrefabProcessingEvent::ExistingPrefabInstanceNullRoot { .. } => {
                    report.existing_prefab_instances += 1;
                }
                WorkspacePrefabProcessingEvent::ExistingPrefabInstanceRootUnavailable {
                    ..
                } => {
                    report.existing_prefab_instances += 1;
                    report.root_resolution_issues += 1;
                }
                WorkspacePrefabProcessingEvent::ExistingPrefabInstanceSkippedAlreadyProcessed {
                    ..
                } => {
                    report.existing_prefab_instances += 1;
                    report.existing_prefab_instances_skipped += 1;
                }
                WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyCreated {
                    hierarchy,
                    ..
                } => {
                    report.existing_prefab_instances += 1;
                    report.existing_prefab_hierarchies += 1;
                    report.generated_transform_components += hierarchy.generated_transform_count();
                }
                WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyIncomplete { .. } => {
                    report.existing_prefab_instances += 1;
                    report.existing_hierarchy_failures += 1;
                }
                WorkspacePrefabProcessingEvent::RemainingGameObjectSkippedAlreadyProcessed {
                    ..
                }
                | WorkspacePrefabProcessingEvent::RemainingGameObjectRootAlreadyProcessed {
                    ..
                } => report.remaining_game_objects_skipped += 1,
                WorkspacePrefabProcessingEvent::RemainingGameObjectRootUnavailable { .. } => {
                    report.root_resolution_issues += 1;
                }
                WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyCreated {
                    hierarchy,
                    ..
                } => {
                    report.generated_prefab_hierarchies += 1;
                    report.generated_transform_components += hierarchy.generated_transform_count();
                }
                WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyIncomplete { .. } => {
                    report.generated_hierarchy_failures += 1;
                }
            }
        }
        report
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkspacePrefabProcessingReport {
    pub scene_game_object_records: usize,
    pub existing_prefab_instances: usize,
    pub existing_prefab_hierarchies: usize,
    pub existing_prefab_instances_skipped: usize,
    pub generated_prefab_hierarchies: usize,
    pub remaining_game_objects_skipped: usize,
    pub root_resolution_issues: usize,
    pub existing_hierarchy_failures: usize,
    pub generated_hierarchy_failures: usize,
    pub generated_transform_components: usize,
    pub processed_game_objects: usize,
    pub hierarchy_entries: usize,
    pub complete: bool,
    pub stopped_on_incomplete_hierarchy: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspacePrefabProcessingError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    ResourceLimitExceeded {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    WorkspaceOperationFailed {
        operation: &'static str,
        message: String,
    },
}

impl Display for WorkspacePrefabProcessingError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::CountOverflow { resource } => write!(f, "{resource} count overflows usize"),
            Self::ResourceLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(f, "{resource} count {requested} exceeds limit {limit}"),
            Self::WorkspaceOperationFailed { operation, message } => {
                write!(f, "{operation} failed: {message}")
            }
        }
    }
}

impl Error for WorkspacePrefabProcessingError {}

/// Replays the three source identity-processing passes without constructing generated Unity
/// objects: scene membership, existing PrefabInstances, then remaining GameObjects.
///
/// Indeterminate root pointers and incomplete hierarchy walks remain visible in the plan. An
/// incomplete hierarchy walk stops later passes because the source creation path would fail while
/// enumerating `FetchHierarchy`; it must not be treated as a completed deduplication pass.
pub fn plan_workspace_prefab_processing(
    workspace: &WorkspaceSceneHierarchies,
    collection_file_indices: &[usize],
    limits: WorkspacePrefabProcessingLimits,
) -> Result<WorkspacePrefabProcessingPlan, WorkspacePrefabProcessingError> {
    if collection_file_indices.len() > limits.max_source_collection_file_indices {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "Prefab processing source collection indexes",
            requested: collection_file_indices.len(),
            limit: limits.max_source_collection_file_indices,
        });
    }
    let mut plan = WorkspacePrefabProcessingPlan {
        complete: true,
        ..WorkspacePrefabProcessingPlan::default()
    };
    plan.source_collection_file_indices
        .try_reserve_exact(collection_file_indices.len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "Prefab processing source collection indexes",
            requested: collection_file_indices.len(),
        })?;
    plan.source_collection_file_indices
        .extend_from_slice(collection_file_indices);
    let mut processed_set = HashSet::<(usize, usize)>::new();

    for (scene_index, scene) in workspace.scenes().iter().enumerate() {
        for collection in scene.collections() {
            for asset in collection.scene_assets() {
                if asset.category != WorkspaceSceneAssetCategory::GameObject {
                    continue;
                }
                let newly_marked =
                    mark_processed(asset.identity, &mut processed_set, &mut plan, limits)?;
                push_event(
                    &mut plan,
                    WorkspacePrefabProcessingEvent::SceneGameObjectMarked {
                        scene_index,
                        game_object: asset.identity,
                        newly_marked,
                    },
                    limits,
                )?;
            }
        }
    }

    for prefab_instance in
        workspace.prefab_instance_identities_in_collection_order(collection_file_indices)
    {
        let resolution = workspace
            .resolve_prefab_instance_root(prefab_instance)
            .map_err(|error| operation_error("PrefabInstance root resolution", error))?;
        match resolution {
            WorkspacePrefabInstanceRootResolution::Null => {
                push_event(
                    &mut plan,
                    WorkspacePrefabProcessingEvent::ExistingPrefabInstanceNullRoot {
                        prefab_instance,
                    },
                    limits,
                )?;
            }
            WorkspacePrefabInstanceRootResolution::Resolved(root_game_object) => {
                if is_processed(root_game_object, &processed_set) {
                    push_event(
                        &mut plan,
                        WorkspacePrefabProcessingEvent::ExistingPrefabInstanceSkippedAlreadyProcessed {
                            prefab_instance,
                            root_game_object,
                        },
                        limits,
                    )?;
                    continue;
                }
                let hierarchy_walk = fetch_hierarchy(workspace, root_game_object, &plan, limits)?;
                if !hierarchy_walk.is_complete() {
                    add_hierarchy_entry_count(&mut plan, hierarchy_walk.entries().len(), limits)?;
                    push_event(
                        &mut plan,
                        WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyIncomplete {
                            prefab_instance,
                            root_game_object,
                            set_prefab_internal: true,
                            hierarchy: hierarchy_walk,
                        },
                        limits,
                    )?;
                    plan.complete = false;
                    plan.stopped_on_incomplete_hierarchy = true;
                    return Ok(plan);
                }
                let remaining_entries = remaining_hierarchy_entries(&plan, limits)?;
                let hierarchy = plan_hierarchy_entries(hierarchy_walk, remaining_entries)?;
                add_hierarchy_entry_count(&mut plan, hierarchy.entries().len(), limits)?;
                mark_hierarchy_game_objects(&hierarchy, &mut processed_set, &mut plan, limits)?;
                push_event(
                    &mut plan,
                    WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyCreated {
                        prefab_instance,
                        root_game_object,
                        set_prefab_internal: true,
                        hierarchy,
                    },
                    limits,
                )?;
            }
            unavailable => {
                plan.complete = false;
                push_event(
                    &mut plan,
                    WorkspacePrefabProcessingEvent::ExistingPrefabInstanceRootUnavailable {
                        prefab_instance,
                        resolution: unavailable,
                    },
                    limits,
                )?;
            }
        }
    }

    for asset in workspace.scene_asset_identities_in_collection_order(collection_file_indices) {
        if asset.category != WorkspaceSceneAssetCategory::GameObject {
            continue;
        }
        let game_object = asset.identity;
        if is_processed(game_object, &processed_set) {
            push_event(
                &mut plan,
                WorkspacePrefabProcessingEvent::RemainingGameObjectSkippedAlreadyProcessed {
                    game_object,
                },
                limits,
            )?;
            continue;
        }

        let root_walk = workspace
            .get_root_game_object(game_object, limits.get_root)
            .map_err(root_walk_error)?;
        if !root_walk.is_complete() {
            plan.complete = false;
            push_event(
                &mut plan,
                WorkspacePrefabProcessingEvent::RemainingGameObjectRootUnavailable {
                    game_object,
                    root_walk,
                },
                limits,
            )?;
            return Ok(plan);
        }
        let root_game_object = root_walk.root_game_object.ok_or_else(|| {
            WorkspacePrefabProcessingError::WorkspaceOperationFailed {
                operation: "GameObject GetRoot",
                message: "a complete root walk had no root identity".to_owned(),
            }
        })?;
        if is_processed(root_game_object, &processed_set) {
            push_event(
                &mut plan,
                WorkspacePrefabProcessingEvent::RemainingGameObjectRootAlreadyProcessed {
                    game_object,
                    root_walk,
                },
                limits,
            )?;
            continue;
        }

        // The source inserts the root into its set before creating the generated PrefabInstance
        // and PrefabHierarchy. Preserve that ordering if hierarchy construction later fails.
        mark_processed(root_game_object, &mut processed_set, &mut plan, limits)?;
        let hierarchy_walk = fetch_hierarchy(workspace, root_game_object, &plan, limits)?;
        if !hierarchy_walk.is_complete() {
            add_hierarchy_entry_count(&mut plan, hierarchy_walk.entries().len(), limits)?;
            push_event(
                &mut plan,
                WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyIncomplete {
                    source_game_object: game_object,
                    root_game_object,
                    root_walk,
                    hierarchy: hierarchy_walk,
                },
                limits,
            )?;
            plan.complete = false;
            plan.stopped_on_incomplete_hierarchy = true;
            return Ok(plan);
        }
        let remaining_entries = remaining_hierarchy_entries(&plan, limits)?;
        let hierarchy = plan_hierarchy_entries(hierarchy_walk, remaining_entries)?;
        add_hierarchy_entry_count(&mut plan, hierarchy.entries().len(), limits)?;
        mark_hierarchy_game_objects(&hierarchy, &mut processed_set, &mut plan, limits)?;
        push_event(
            &mut plan,
            WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyCreated {
                source_game_object: game_object,
                root_game_object,
                root_walk,
                hierarchy,
            },
            limits,
        )?;
    }

    Ok(plan)
}

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

/// Source `GameObjectExtensions.AddComponent` insertion for a generated Transform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceGeneratedComponentAddition {
    pub game_object: WorkspaceSceneObjectIdentity,
    pub component_class_id: i32,
    pub appended_index: usize,
    pub component_asset: WorkspaceProcessedAssetIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSourceGameObjectComponentSlot {
    pub source_array_index: usize,
    pub file_id: i32,
    pub path_id: i64,
    pub target: SceneRelationshipTarget,
}

/// A non-mutating view of the source component slots and the generated Transform append.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceGameObjectComponentListOverlay {
    pub game_object: WorkspaceSceneObjectIdentity,
    pub source_slots: Vec<WorkspaceSourceGameObjectComponentSlot>,
    pub generated_component: WorkspaceGeneratedComponentAddition,
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

/// A source SerializedAssetCollection must reference the generated Transform collection.
/// The final FileID depends on dependency additions made by processors before PrefabProcessor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSourceCollectionDependencyAppend {
    pub source_file_index: usize,
    pub target_collection_index: usize,
    pub first_component_owner: WorkspaceSceneObjectIdentity,
    pub target_asset: WorkspaceProcessedAssetIdentity,
    /// Deferred until the source collection's pre-PrefabProcessor dependency list is replayed.
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
    complete: bool,
    incomplete_reason: Option<String>,
    collections: Vec<WorkspaceProcessedAssetCollectionPlan>,
    component_additions: Vec<WorkspaceGeneratedComponentAddition>,
    component_list_overlays: Vec<WorkspaceGameObjectComponentListOverlay>,
    component_overlay_slot_count: usize,
    constructor_pointers: Vec<WorkspaceGeneratedConstructorPointer>,
    dependency_entry_count: usize,
    source_dependency_appends: Vec<WorkspaceSourceCollectionDependencyAppend>,
    asset_count: usize,
    total_collection_name_bytes: usize,
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

    pub fn asset_count(&self) -> usize {
        self.asset_count
    }
}

/// Replays the source-created collection and asset order after an identity-level PrefabProcessor
/// plan. PathIDs start at one independently in each ProcessedAssetCollection, as in the baseline.
pub fn plan_workspace_processed_prefab_assets(
    workspace: &WorkspaceSceneHierarchies,
    processing: &WorkspacePrefabProcessingPlan,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<WorkspaceProcessedPrefabAssetPlan, WorkspacePrefabProcessingError> {
    if !processing.is_complete() {
        return Ok(incomplete_processed_asset_plan(
            "PrefabProcessor identity plan is incomplete; generated PathID order is indeterminate",
        ));
    }

    let mut graphs_by_file = HashMap::new();
    let graph_count = workspace
        .scenes()
        .iter()
        .flat_map(|scene| scene.collections())
        .count()
        .checked_add(workspace.non_scene_collections().len())
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "scene hierarchy collection graphs",
        })?;
    graphs_by_file.try_reserve(graph_count).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "Prefab allocation source collection index",
            requested: graph_count,
        }
    })?;
    let mut scene_index_by_file = HashMap::new();
    scene_index_by_file.try_reserve(graph_count).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "Prefab allocation scene ownership index",
            requested: graph_count,
        }
    })?;
    for (scene_index, scene) in workspace.scenes().iter().enumerate() {
        for graph in scene.collections() {
            let file_index = graph.collection_file_index();
            if graphs_by_file.insert(file_index, graph).is_some()
                || scene_index_by_file
                    .insert(file_index, scene_index)
                    .is_some()
            {
                return Err(processed_asset_operation_error(format!(
                    "collection file {file_index} belongs to more than one hierarchy graph"
                )));
            }
        }
    }
    for graph in workspace.non_scene_collections() {
        let file_index = graph.collection_file_index();
        if graphs_by_file.insert(file_index, graph).is_some() {
            return Err(processed_asset_operation_error(format!(
                "collection file {file_index} belongs to more than one hierarchy graph"
            )));
        }
    }

    let collection_file_indices = processing.source_collection_file_indices();
    let mut seen_collection_files = HashSet::new();
    seen_collection_files
        .try_reserve(collection_file_indices.len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "Prefab allocation collection membership set",
            requested: collection_file_indices.len(),
        })?;
    let mut missing_transforms = HashMap::new();
    for &file_index in collection_file_indices {
        if !seen_collection_files.insert(file_index) {
            return Err(processed_asset_operation_error(format!(
                "source collection order contains duplicate file index {file_index}"
            )));
        }
        let Some(graph) = graphs_by_file.get(&file_index).copied() else {
            return Ok(incomplete_processed_asset_plan(&format!(
                "source collection file {file_index} has no recovered hierarchy graph; generated PathID order is indeterminate"
            )));
        };
        if !graph.is_complete() {
            return Ok(incomplete_processed_asset_plan(&format!(
                "collection file {file_index} has an incomplete scene hierarchy graph"
            )));
        }
        let requested = missing_transforms
            .len()
            .checked_add(graph.missing_transform_plans().len())
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "missing Transform allocation candidates",
            })?;
        if requested > limits.max_missing_transform_candidates {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "missing Transform allocation candidates",
                requested,
                limit: limits.max_missing_transform_candidates,
            });
        }
        missing_transforms
            .try_reserve(graph.missing_transform_plans().len())
            .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
                resource: "missing Transform allocation candidates",
                requested,
            })?;
        for transform in graph.missing_transform_plans() {
            let key = (
                transform.game_object.file_index,
                transform.game_object.object_index,
            );
            if missing_transforms
                .insert(key, (transform.game_object, transform.default_values))
                .is_some()
            {
                return Err(processed_asset_operation_error(format!(
                    "GameObject {}:{} has duplicate missing-Transform plans",
                    key.0, key.1
                )));
            }
        }
    }

    let mut plan = WorkspaceProcessedPrefabAssetPlan {
        complete: true,
        incomplete_reason: None,
        collections: Vec::new(),
        component_additions: Vec::new(),
        component_list_overlays: Vec::new(),
        component_overlay_slot_count: 0,
        constructor_pointers: Vec::new(),
        dependency_entry_count: 0,
        source_dependency_appends: Vec::new(),
        asset_count: 0,
        total_collection_name_bytes: 0,
    };
    let prefab_hierarchies = add_processed_collection(
        &mut plan,
        WorkspaceProcessedCollectionKind::PrefabHierarchies,
        "Prefab Hierarchies",
        limits,
    )?;
    let generated_prefabs = add_processed_collection(
        &mut plan,
        WorkspaceProcessedCollectionKind::GeneratedPrefabs,
        "Generated Prefabs",
        limits,
    )?;
    let missing_prefab_transforms = add_processed_collection(
        &mut plan,
        WorkspaceProcessedCollectionKind::MissingPrefabTransforms,
        "Missing Prefab Transforms",
        limits,
    )?;

    let mut scene_collection_by_index = Vec::new();
    scene_collection_by_index
        .try_reserve_exact(workspace.scenes().len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "per-scene generated collection index",
            requested: workspace.scenes().len(),
        })?;
    scene_collection_by_index.resize(workspace.scenes().len(), None);
    let mut source_dependency_append_keys = HashSet::new();
    source_dependency_append_keys
        .try_reserve(missing_transforms.len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "source processed-collection dependency append index",
            requested: missing_transforms.len(),
        })?;

    // AddMissingTransforms runs before SceneHierarchyObject creation and follows FetchAssets order.
    for asset in workspace.scene_asset_identities_in_collection_order(collection_file_indices) {
        if asset.category != WorkspaceSceneAssetCategory::GameObject {
            continue;
        }
        let key = (asset.identity.file_index, asset.identity.object_index);
        let Some((game_object, default_values)) = missing_transforms.remove(&key) else {
            continue;
        };
        if game_object != asset.identity {
            return Err(processed_asset_operation_error(format!(
                "missing-Transform identity for {}:{} disagrees with source collection order",
                key.0, key.1
            )));
        }
        let collection_index = if let Some(&scene_index) = scene_index_by_file.get(&key.0) {
            let scene = workspace.scenes().get(scene_index).ok_or_else(|| {
                processed_asset_operation_error(format!(
                    "collection file {} maps to missing SceneDefinition {scene_index}",
                    key.0
                ))
            })?;
            ensure_scene_processed_collection(
                &mut plan,
                &mut scene_collection_by_index,
                scene_index,
                &scene.plan.name,
                limits,
            )?
        } else {
            missing_prefab_transforms
        };
        let transform_asset = add_processed_asset(
            &mut plan,
            collection_index,
            4,
            WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
                game_object,
                default_values,
            },
            limits,
        )?;
        let game_object_file_id = add_processed_source_file_dependency(
            &mut plan,
            collection_index,
            game_object.file_index,
            limits,
        )?;
        add_generated_constructor_pointer(
            &mut plan,
            WorkspaceGeneratedConstructorPointer {
                source_asset: transform_asset,
                field: WorkspaceGeneratedPointerField::TransformGameObject,
                file_id: game_object_file_id,
                path_id: game_object.path_id,
                target: game_object,
            },
            limits,
        )?;
        let graph = graphs_by_file.get(&key.0).copied().ok_or_else(|| {
            processed_asset_operation_error(format!(
                "missing-Transform GameObject collection {} disappeared during allocation",
                key.0
            ))
        })?;
        let appended_index = graph
            .game_object_component_slot_count(game_object)
            .map_err(|error| processed_asset_operation_error(error.to_string()))?;
        let mut source_slots = Vec::new();
        source_slots
            .try_reserve_exact(appended_index)
            .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
                resource: "source GameObject component slots for overlay",
                requested: appended_index,
            })?;
        for (source_array_index, relationship) in graph
            .game_object_component_relationships_for_identity(game_object)
            .map_err(|error| processed_asset_operation_error(error.to_string()))?
            .enumerate()
        {
            source_slots.push(WorkspaceSourceGameObjectComponentSlot {
                source_array_index,
                file_id: relationship.file_id,
                path_id: relationship.path_id,
                target: relationship.target.clone(),
            });
        }
        if source_slots.len() != appended_index {
            return Err(processed_asset_operation_error(format!(
                "GameObject {}:{} component slot count changed during overlay planning",
                game_object.file_index, game_object.object_index
            )));
        }
        let requested_overlay_slots = plan
            .component_overlay_slot_count
            .checked_add(appended_index)
            .and_then(|count| count.checked_add(1))
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "GameObject component overlay slots",
            })?;
        if requested_overlay_slots > limits.max_component_overlay_slots {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "GameObject component overlay slots",
                requested: requested_overlay_slots,
                limit: limits.max_component_overlay_slots,
            });
        }
        let requested_additions = plan.component_additions.len().checked_add(1).ok_or(
            WorkspacePrefabProcessingError::CountOverflow {
                resource: "generated GameObject component additions",
            },
        )?;
        if requested_additions > limits.max_component_additions {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "generated GameObject component additions",
                requested: requested_additions,
                limit: limits.max_component_additions,
            });
        }
        plan.component_additions.try_reserve(1).map_err(|_| {
            WorkspacePrefabProcessingError::AllocationFailed {
                resource: "generated GameObject component additions",
                requested: requested_additions,
            }
        })?;
        plan.component_list_overlays.try_reserve(1).map_err(|_| {
            WorkspacePrefabProcessingError::AllocationFailed {
                resource: "GameObject component list overlays",
                requested: requested_additions,
            }
        })?;
        let addition = WorkspaceGeneratedComponentAddition {
            game_object,
            component_class_id: 4,
            appended_index,
            component_asset: transform_asset,
        };
        plan_source_dependency_append(
            &mut plan,
            &mut source_dependency_append_keys,
            game_object,
            transform_asset,
            limits,
        )?;
        plan.component_additions.push(addition);
        plan.component_list_overlays
            .push(WorkspaceGameObjectComponentListOverlay {
                game_object,
                source_slots,
                generated_component: addition,
            });
        plan.component_overlay_slot_count = requested_overlay_slots;
    }
    if let Some((file_index, object_index)) = missing_transforms.keys().copied().min() {
        return Err(processed_asset_operation_error(format!(
            "missing-Transform GameObject {file_index}:{object_index} is absent from source FetchAssets order"
        )));
    }

    // SceneHierarchyObject.Create allocates one SceneAsset per SceneDefinition after missing transforms.
    for (scene_index, scene) in workspace.scenes().iter().enumerate() {
        let collection_index = ensure_scene_processed_collection(
            &mut plan,
            &mut scene_collection_by_index,
            scene_index,
            &scene.plan.name,
            limits,
        )?;
        add_processed_asset(
            &mut plan,
            collection_index,
            1032,
            WorkspaceProcessedPrefabAssetKind::SceneHierarchy { scene_index },
            limits,
        )?;
    }

    // Existing PrefabHierarchy objects and generated prefabs are interleaved by source events.
    for event in processing.events() {
        match event {
            WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyCreated {
                prefab_instance,
                root_game_object,
                ..
            } => {
                add_processed_asset(
                    &mut plan,
                    prefab_hierarchies,
                    1001,
                    WorkspaceProcessedPrefabAssetKind::PrefabHierarchy {
                        root_game_object: *root_game_object,
                        prefab: WorkspaceProcessedPrefabReference::Existing(*prefab_instance),
                    },
                    limits,
                )?;
            }
            WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyCreated {
                root_game_object,
                ..
            } => {
                let prefab_instance = add_processed_asset(
                    &mut plan,
                    generated_prefabs,
                    1001,
                    WorkspaceProcessedPrefabAssetKind::GeneratedPrefabInstance {
                        root_game_object: *root_game_object,
                    },
                    limits,
                )?;
                let root_file_id = add_processed_source_file_dependency(
                    &mut plan,
                    generated_prefabs,
                    root_game_object.file_index,
                    limits,
                )?;
                add_generated_constructor_pointer(
                    &mut plan,
                    WorkspaceGeneratedConstructorPointer {
                        source_asset: prefab_instance,
                        field: WorkspaceGeneratedPointerField::PrefabInstanceRootGameObject,
                        file_id: root_file_id,
                        path_id: root_game_object.path_id,
                        target: *root_game_object,
                    },
                    limits,
                )?;
                add_processed_asset(
                    &mut plan,
                    prefab_hierarchies,
                    1001,
                    WorkspaceProcessedPrefabAssetKind::PrefabHierarchy {
                        root_game_object: *root_game_object,
                        prefab: WorkspaceProcessedPrefabReference::Generated(prefab_instance),
                    },
                    limits,
                )?;
            }
            _ => {}
        }
    }

    Ok(plan)
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
        asset_count: 0,
        total_collection_name_bytes: 0,
    }
}

fn add_processed_collection(
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

fn add_processed_source_file_dependency(
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

fn add_generated_constructor_pointer(
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

fn plan_source_dependency_append(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    seen: &mut HashSet<(usize, usize)>,
    owner: WorkspaceSceneObjectIdentity,
    target_asset: WorkspaceProcessedAssetIdentity,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    let key = (owner.file_index, target_asset.collection_index);
    if !seen.insert(key) {
        return Ok(());
    }
    let requested = plan.source_dependency_appends.len().checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "source processed-collection dependency appends",
        },
    )?;
    if requested > limits.max_source_dependency_appends {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "source processed-collection dependency appends",
            requested,
            limit: limits.max_source_dependency_appends,
        });
    }
    plan.source_dependency_appends.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "source processed-collection dependency appends",
            requested,
        }
    })?;
    plan.source_dependency_appends
        .push(WorkspaceSourceCollectionDependencyAppend {
            source_file_index: owner.file_index,
            target_collection_index: target_asset.collection_index,
            first_component_owner: owner,
            target_asset,
            file_id: None,
        });
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

fn add_processed_asset(
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

fn processed_asset_operation_error(message: String) -> WorkspacePrefabProcessingError {
    WorkspacePrefabProcessingError::WorkspaceOperationFailed {
        operation: "processed prefab asset allocation planning",
        message,
    }
}

fn fetch_hierarchy(
    workspace: &WorkspaceSceneHierarchies,
    root_game_object: WorkspaceSceneObjectIdentity,
    plan: &WorkspacePrefabProcessingPlan,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<WorkspaceGameObjectHierarchyWalk, WorkspacePrefabProcessingError> {
    let remaining_entries = remaining_hierarchy_entries(plan, limits)?;
    let mut walk_limits = limits.hierarchy_walk;
    walk_limits.max_entries = walk_limits.max_entries.min(remaining_entries);
    workspace
        .fetch_game_object_hierarchy(root_game_object, walk_limits)
        .map_err(hierarchy_walk_error)
}

fn remaining_hierarchy_entries(
    plan: &WorkspacePrefabProcessingPlan,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<usize, WorkspacePrefabProcessingError> {
    limits
        .max_total_hierarchy_entries
        .checked_sub(plan.hierarchy_entry_count)
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "remaining prefab hierarchy entries",
        })
}

fn plan_hierarchy_entries(
    source_walk: WorkspaceGameObjectHierarchyWalk,
    entry_limit: usize,
) -> Result<WorkspacePrefabHierarchyPlan, WorkspacePrefabProcessingError> {
    let source_entries = source_walk.entries();
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(source_entries.len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "Prefab hierarchy plan entries",
            requested: source_entries.len(),
        })?;

    let mut cursor = 0usize;
    while cursor < source_entries.len() {
        let game_object_entry = source_entries[cursor];
        if game_object_entry.kind != WorkspaceGameObjectHierarchyEntryKind::GameObject {
            return Err(WorkspacePrefabProcessingError::WorkspaceOperationFailed {
                operation: "Prefab hierarchy planning",
                message: format!("entry {cursor} is a component without a preceding GameObject"),
            });
        }
        push_hierarchy_plan_entry(
            &mut entries,
            WorkspacePrefabHierarchyPlanEntry::Source(game_object_entry),
            entry_limit,
        )?;
        cursor += 1;

        let mut has_transform = false;
        while let Some(component_entry) = source_entries.get(cursor).copied() {
            if component_entry.kind != WorkspaceGameObjectHierarchyEntryKind::Component {
                break;
            }
            if matches!(
                component_entry.identity.class_id,
                Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
            ) {
                has_transform = true;
            }
            push_hierarchy_plan_entry(
                &mut entries,
                WorkspacePrefabHierarchyPlanEntry::Source(component_entry),
                entry_limit,
            )?;
            cursor += 1;
        }
        if !has_transform {
            push_hierarchy_plan_entry(
                &mut entries,
                WorkspacePrefabHierarchyPlanEntry::GeneratedTransform {
                    game_object: game_object_entry.identity,
                    default_values: default_workspace_generated_transform_values(),
                },
                entry_limit,
            )?;
        }
    }

    Ok(WorkspacePrefabHierarchyPlan {
        source_walk,
        entries,
    })
}

fn push_hierarchy_plan_entry(
    entries: &mut Vec<WorkspacePrefabHierarchyPlanEntry>,
    entry: WorkspacePrefabHierarchyPlanEntry,
    limit: usize,
) -> Result<(), WorkspacePrefabProcessingError> {
    let requested =
        entries
            .len()
            .checked_add(1)
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "Prefab hierarchy plan entries",
            })?;
    if requested > limit {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "Prefab hierarchy plan entries",
            requested,
            limit,
        });
    }
    entries
        .try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "Prefab hierarchy plan entries",
            requested,
        })?;
    entries.push(entry);
    Ok(())
}

fn add_hierarchy_entry_count(
    plan: &mut WorkspacePrefabProcessingPlan,
    count: usize,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    let requested = plan.hierarchy_entry_count.checked_add(count).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "prefab hierarchy entries",
        },
    )?;
    if requested > limits.max_total_hierarchy_entries {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "prefab hierarchy entries",
            requested,
            limit: limits.max_total_hierarchy_entries,
        });
    }
    plan.hierarchy_entry_count = requested;
    Ok(())
}

fn mark_hierarchy_game_objects(
    hierarchy: &WorkspacePrefabHierarchyPlan,
    processed_set: &mut HashSet<(usize, usize)>,
    plan: &mut WorkspacePrefabProcessingPlan,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    for entry in hierarchy.source_walk().entries() {
        if entry.kind == WorkspaceGameObjectHierarchyEntryKind::GameObject {
            mark_processed(entry.identity, processed_set, plan, limits)?;
        }
    }
    Ok(())
}

fn is_processed(
    game_object: WorkspaceSceneObjectIdentity,
    processed_set: &HashSet<(usize, usize)>,
) -> bool {
    processed_set.contains(&(game_object.file_index, game_object.object_index))
}

fn mark_processed(
    game_object: WorkspaceSceneObjectIdentity,
    processed_set: &mut HashSet<(usize, usize)>,
    plan: &mut WorkspacePrefabProcessingPlan,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<bool, WorkspacePrefabProcessingError> {
    if is_processed(game_object, processed_set) {
        return Ok(false);
    }
    let requested = plan.processed_game_objects.len().checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed GameObjects",
        },
    )?;
    if requested > limits.max_processed_game_objects {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed GameObjects",
            requested,
            limit: limits.max_processed_game_objects,
        });
    }
    plan.processed_game_objects.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "ordered processed GameObjects",
            requested,
        }
    })?;
    processed_set
        .try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed GameObject membership set",
            requested,
        })?;
    processed_set.insert((game_object.file_index, game_object.object_index));
    plan.processed_game_objects.push(game_object);
    Ok(true)
}

fn push_event(
    plan: &mut WorkspacePrefabProcessingPlan,
    event: WorkspacePrefabProcessingEvent,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    let requested =
        plan.events
            .len()
            .checked_add(1)
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "Prefab processing events",
            })?;
    if requested > limits.max_events {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "Prefab processing events",
            requested,
            limit: limits.max_events,
        });
    }
    plan.events
        .try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "Prefab processing events",
            requested,
        })?;
    plan.events.push(event);
    Ok(())
}

fn operation_error(operation: &'static str, error: impl Display) -> WorkspacePrefabProcessingError {
    WorkspacePrefabProcessingError::WorkspaceOperationFailed {
        operation,
        message: error.to_string(),
    }
}

fn root_walk_error(error: WorkspaceGetRootGameObjectError) -> WorkspacePrefabProcessingError {
    match error {
        WorkspaceGetRootGameObjectError::AllocationFailed {
            resource,
            requested,
        } => WorkspacePrefabProcessingError::AllocationFailed {
            resource,
            requested,
        },
        WorkspaceGetRootGameObjectError::CountOverflow { resource } => {
            WorkspacePrefabProcessingError::CountOverflow { resource }
        }
        WorkspaceGetRootGameObjectError::ParentDepthLimitExceeded { requested, limit } => {
            WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "Transform parent depth",
                requested,
                limit,
            }
        }
        WorkspaceGetRootGameObjectError::GraphInvariantFailed { message } => {
            WorkspacePrefabProcessingError::WorkspaceOperationFailed {
                operation: "GameObject GetRoot",
                message,
            }
        }
    }
}

fn hierarchy_walk_error(
    error: WorkspaceGameObjectHierarchyWalkError,
) -> WorkspacePrefabProcessingError {
    match error {
        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
            resource,
            requested,
        } => WorkspacePrefabProcessingError::AllocationFailed {
            resource,
            requested,
        },
        WorkspaceGameObjectHierarchyWalkError::CountOverflow { resource } => {
            WorkspacePrefabProcessingError::CountOverflow { resource }
        }
        WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
            resource,
            requested,
            limit,
        } => WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource,
            requested,
            limit,
        },
        other => operation_error("GameObject FetchHierarchy", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            asset_count: 0,
            total_collection_name_bytes: 0,
        }
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
        let limits = WorkspaceProcessedPrefabAssetPlanLimits::default();

        plan_source_dependency_append(&mut plan, &mut seen, first_owner, target_asset, limits)
            .unwrap();
        plan_source_dependency_append(&mut plan, &mut seen, later_owner, target_asset, limits)
            .unwrap();

        assert_eq!(plan.source_dependency_appends.len(), 1);
        assert_eq!(
            plan.source_dependency_appends[0].first_component_owner,
            first_owner
        );
        assert_eq!(plan.source_dependency_appends[0].file_id, None);
    }
}
