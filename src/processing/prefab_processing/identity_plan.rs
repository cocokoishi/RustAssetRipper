use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::processing::scene_hierarchy::{
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

    pub fn issues(
        &self,
    ) -> &[crate::processing::scene_hierarchy::WorkspaceGameObjectHierarchyIssue] {
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
