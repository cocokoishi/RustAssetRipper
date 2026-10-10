use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::metadata::SerializedFileMetadata;
use crate::serialized_file::object::{
    ObjectDefaultValueLimits, ObjectEncodeLimits, ParsedObjectPayload, SerializedValue,
};
use crate::serialized_file::object_workspace::ObjectWorkspace;

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

/// Typed root and encoded bytes for one planned, newly generated Transform.
///
/// The caller can place `root` into a `SerializedObjectInsertion` when creating the generated
/// collection file. The bytes are returned as direct evidence that the selected existing type
/// entry can encode this value; they do not create the collection or its object record.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceGeneratedTransformPayload {
    pub asset: WorkspaceProcessedAssetIdentity,
    pub type_index: usize,
    pub root: SerializedValue,
    pub bytes: Vec<u8>,
}

/// Materialize the planned Transform constructor values against an existing serialized type.
///
/// This applies the source `TransformExtensions.InitializeDefault` values and the checked
/// `GameObject_C4P` dependency from the processed-asset plan. The selected type must be an
/// existing class-ID-4 TypeTree; this does not synthesize a missing serialized type entry.
pub fn materialize_planned_generated_transform_payload(
    plan: &WorkspaceProcessedPrefabAssetPlan,
    asset_identity: WorkspaceProcessedAssetIdentity,
    metadata: &SerializedFileMetadata,
    type_index: usize,
    default_limits: ObjectDefaultValueLimits,
    encode_limits: ObjectEncodeLimits,
) -> Result<WorkspaceGeneratedTransformPayload, WorkspacePrefabProcessingError> {
    if !plan.is_complete() {
        return Err(processed_asset_operation_error(
            "PrefabProcessor asset allocation plan is incomplete".to_owned(),
        ));
    }
    if asset_identity.class_id != TRANSFORM_CLASS_ID {
        return Err(processed_asset_operation_error(format!(
            "generated asset class ID {} is not Transform class ID {TRANSFORM_CLASS_ID}",
            asset_identity.class_id
        )));
    }
    let collection = plan
        .collections
        .get(asset_identity.collection_index)
        .ok_or_else(|| {
            processed_asset_operation_error(format!(
                "generated Transform collection {} is unavailable",
                asset_identity.collection_index
            ))
        })?;
    let mut matching_assets = collection
        .assets
        .iter()
        .filter(|asset| asset.identity == asset_identity);
    let asset = matching_assets.next().ok_or_else(|| {
        processed_asset_operation_error(format!(
            "generated Transform {:?} is absent from its planned collection",
            asset_identity
        ))
    })?;
    if matching_assets.next().is_some() {
        return Err(processed_asset_operation_error(format!(
            "generated Transform {:?} occurs more than once in its planned collection",
            asset_identity
        )));
    }
    let WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
        game_object,
        default_values,
    } = &asset.kind
    else {
        return Err(processed_asset_operation_error(format!(
            "planned asset {:?} is not a generated Transform",
            asset_identity
        )));
    };

    let mut matching_pointers = plan.constructor_pointers.iter().filter(|pointer| {
        pointer.source_asset == asset_identity
            && pointer.field == WorkspaceGeneratedPointerField::TransformGameObject
    });
    let constructor_pointer = matching_pointers.next().ok_or_else(|| {
        generated_transform_materialization_error(
            *game_object,
            "planned Transform has no GameObject constructor PPtr",
        )
    })?;
    if matching_pointers.next().is_some() {
        return Err(generated_transform_materialization_error(
            *game_object,
            "planned Transform has duplicate GameObject constructor PPtrs",
        ));
    }
    if constructor_pointer.target != *game_object
        || constructor_pointer.path_id != game_object.path_id
        || constructor_pointer.file_id <= 0
    {
        return Err(generated_transform_materialization_error(
            *game_object,
            "planned GameObject constructor PPtr disagrees with the generated asset kind",
        ));
    }
    let matching_dependencies = collection
        .dependencies
        .iter()
        .filter(|dependency| dependency.file_id == constructor_pointer.file_id);
    let mut matching_dependencies = matching_dependencies;
    let Some(dependency) = matching_dependencies.next() else {
        return Err(generated_transform_materialization_error(
            *game_object,
            "planned GameObject FileID is absent from the generated collection dependency table",
        ));
    };
    if matching_dependencies.next().is_some()
        || dependency.target
            != (WorkspaceProcessedDependencyTarget::SerializedFileCollection {
                file_index: game_object.file_index,
            })
    {
        return Err(generated_transform_materialization_error(
            *game_object,
            "planned GameObject FileID does not uniquely target its source SerializedFile",
        ));
    }

    let serialized_type = metadata.types.get(type_index).ok_or_else(|| {
        generated_transform_materialization_error(
            *game_object,
            &format!(
                "Transform type index {type_index} is outside the {}-entry type table",
                metadata.types.len()
            ),
        )
    })?;
    if serialized_type.raw_type_id != TRANSFORM_CLASS_ID {
        return Err(generated_transform_materialization_error(
            *game_object,
            &format!(
                "serialized type {type_index} has class ID {}, expected {TRANSFORM_CLASS_ID}",
                serialized_type.raw_type_id
            ),
        ));
    }
    let object_index = metadata.objects.len();
    let mut root = metadata
        .default_object_value_for_type(object_index, type_index, default_limits)
        .map_err(|error| {
            generated_transform_materialization_error(*game_object, &error.to_string())
        })?;

    set_transform_vector3(
        &mut root,
        *game_object,
        "m_LocalPosition",
        default_values.local_position,
    )?;
    set_transform_quaternion(
        &mut root,
        *game_object,
        "m_LocalRotation",
        default_values.local_rotation,
    )?;
    set_transform_vector3(
        &mut root,
        *game_object,
        "m_LocalScale",
        default_values.local_scale,
    )?;
    // The 2019.4 class-4 TPK omits `m_RootOrder`; the baseline setter is a silent no-op there,
    // so a missing field must not fail the generated Transform.
    set_optional_transform_i32(
        &mut root,
        *game_object,
        "m_RootOrder",
        default_values.root_order,
    )?;
    if let Some(euler_hint) = root
        .unique_field_in_base_chain_mut("m_LocalEulerAnglesHint")
        .map_err(|error| {
            generated_transform_materialization_error(*game_object, &error.to_string())
        })?
    {
        set_vector3_components(
            euler_hint,
            *game_object,
            "m_LocalEulerAnglesHint",
            default_values.local_euler_angles_hint_if_present,
        )?;
    }
    let game_object_field = root
        .unique_field_in_base_chain_mut("m_GameObject")
        .map_err(|error| {
            generated_transform_materialization_error(*game_object, &error.to_string())
        })?
        .ok_or_else(|| {
            generated_transform_materialization_error(
                *game_object,
                "Transform TypeTree has no m_GameObject field",
            )
        })?;
    match game_object_field {
        SerializedValue::PPtr { file_id, path_id } => {
            *file_id = constructor_pointer.file_id;
            *path_id = constructor_pointer.path_id;
        }
        other => {
            return Err(generated_transform_materialization_error(
                *game_object,
                &format!(
                    "Transform m_GameObject has value shape {} instead of PPtr",
                    component_value_kind(other)
                ),
            ));
        }
    }

    let bytes = metadata
        .encode_object_payload_for_type(object_index, type_index, &root, &[], encode_limits)
        .map_err(|error| {
            generated_transform_materialization_error(*game_object, &error.to_string())
        })?;
    Ok(WorkspaceGeneratedTransformPayload {
        asset: asset_identity,
        type_index,
        root,
        bytes,
    })
}

fn set_transform_vector3(
    root: &mut SerializedValue,
    game_object: WorkspaceSceneObjectIdentity,
    field_name: &str,
    value: super::scene::SceneVector3,
) -> Result<(), WorkspacePrefabProcessingError> {
    let vector = root
        .unique_field_in_base_chain_mut(field_name)
        .map_err(|error| {
            generated_transform_materialization_error(game_object, &error.to_string())
        })?
        .ok_or_else(|| {
            generated_transform_materialization_error(
                game_object,
                &format!("Transform TypeTree has no {field_name} field"),
            )
        })?;
    set_vector3_components(vector, game_object, field_name, value)
}

fn set_vector3_components(
    vector: &mut SerializedValue,
    game_object: WorkspaceSceneObjectIdentity,
    field_name: &str,
    value: super::scene::SceneVector3,
) -> Result<(), WorkspacePrefabProcessingError> {
    for (name, component) in [("x", value.x), ("y", value.y), ("z", value.z)] {
        let field_path = format!("{field_name}.{name}");
        let field = vector
            .unique_field_in_base_chain_mut(name)
            .map_err(|error| {
                generated_transform_materialization_error(game_object, &error.to_string())
            })?
            .ok_or_else(|| {
                generated_transform_materialization_error(
                    game_object,
                    &format!("Transform TypeTree has no {field_path} field"),
                )
            })?;
        match field {
            SerializedValue::F32(current) => *current = component,
            other => {
                return Err(generated_transform_materialization_error(
                    game_object,
                    &format!(
                        "Transform {field_path} has value shape {} instead of f32",
                        component_value_kind(other)
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn set_transform_quaternion(
    root: &mut SerializedValue,
    game_object: WorkspaceSceneObjectIdentity,
    field_name: &str,
    value: super::scene::SceneQuaternion,
) -> Result<(), WorkspacePrefabProcessingError> {
    let quaternion = root
        .unique_field_in_base_chain_mut(field_name)
        .map_err(|error| {
            generated_transform_materialization_error(game_object, &error.to_string())
        })?
        .ok_or_else(|| {
            generated_transform_materialization_error(
                game_object,
                &format!("Transform TypeTree has no {field_name} field"),
            )
        })?;
    for (name, component) in [
        ("x", value.x),
        ("y", value.y),
        ("z", value.z),
        ("w", value.w),
    ] {
        let field_path = format!("{field_name}.{name}");
        let field = quaternion
            .unique_field_in_base_chain_mut(name)
            .map_err(|error| {
                generated_transform_materialization_error(game_object, &error.to_string())
            })?
            .ok_or_else(|| {
                generated_transform_materialization_error(
                    game_object,
                    &format!("Transform TypeTree has no {field_path} field"),
                )
            })?;
        match field {
            SerializedValue::F32(current) => *current = component,
            other => {
                return Err(generated_transform_materialization_error(
                    game_object,
                    &format!(
                        "Transform {field_path} has value shape {} instead of f32",
                        component_value_kind(other)
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Set an optional Transform integer field; an absent field is the baseline's no-op setter.
fn set_optional_transform_i32(
    root: &mut SerializedValue,
    game_object: WorkspaceSceneObjectIdentity,
    field_name: &str,
    value: i32,
) -> Result<(), WorkspacePrefabProcessingError> {
    let Some(field) = root
        .unique_field_in_base_chain_mut(field_name)
        .map_err(|error| {
            generated_transform_materialization_error(game_object, &error.to_string())
        })?
    else {
        return Ok(());
    };
    match field {
        SerializedValue::I32(current) => {
            *current = value;
            Ok(())
        }
        other => Err(generated_transform_materialization_error(
            game_object,
            &format!(
                "Transform {field_name} has value shape {} instead of i32",
                component_value_kind(other)
            ),
        )),
    }
}

fn generated_transform_materialization_error(
    game_object: WorkspaceSceneObjectIdentity,
    message: &str,
) -> WorkspacePrefabProcessingError {
    WorkspacePrefabProcessingError::WorkspaceOperationFailed {
        operation: "generated Transform payload materialization",
        message: format!(
            "source GameObject file {} object {} PathID {}: {message}",
            game_object.file_index, game_object.object_index, game_object.path_id
        ),
    }
}

/// Materialize one checked component-list overlay as an edited GameObject payload.
///
/// Existing component values and their order are retained. The appended element reuses the
/// source element shape, or an explicit TypeTree-derived template when the source array is empty.
/// This edits only the decoded payload; callers must also write the planned external dependency
/// entry before treating the resulting PPtr as a complete SerializedFile edit.
pub fn materialize_game_object_component_append(
    source_payload: &ParsedObjectPayload,
    overlay: &WorkspaceGameObjectComponentListOverlay,
    component_file_id: i32,
    empty_element_template: Option<&SerializedValue>,
    max_component_elements: usize,
) -> Result<ParsedObjectPayload, WorkspacePrefabProcessingError> {
    let game_object = overlay.game_object;
    if source_payload.object_index != game_object.object_index
        || source_payload.path_id != game_object.path_id
        || overlay.generated_component.game_object != game_object
        || overlay.generated_component.component_class_id != 4
        || overlay.generated_component.component_asset.class_id != 4
        || overlay.generated_component.appended_index != overlay.source_slots.len()
    {
        return Err(component_materialization_error(
            game_object,
            "payload or generated Transform identity does not match the component overlay",
        ));
    }

    let component_array = source_payload
        .root
        .unique_field_in_base_chain("m_Component")
        .map_err(|error| component_materialization_error(game_object, &error.to_string()))?
        .ok_or_else(|| {
            component_materialization_error(game_object, "GameObject has no m_Component field")
        })?;
    let source_elements = component_array.array_elements().ok_or_else(|| {
        component_materialization_error(game_object, "m_Component is not an array")
    })?;
    if source_elements.len() != overlay.source_slots.len() {
        return Err(component_materialization_error(
            game_object,
            &format!(
                "m_Component has {} entries, overlay describes {}",
                source_elements.len(),
                overlay.source_slots.len()
            ),
        ));
    }
    for (index, (element, slot)) in source_elements
        .iter()
        .zip(&overlay.source_slots)
        .enumerate()
    {
        if slot.source_array_index != index {
            return Err(component_materialization_error(
                game_object,
                &format!(
                    "overlay slot {} is stored at array index {index}",
                    slot.source_array_index
                ),
            ));
        }
        let pointer = component_pair_pointer(element).map_err(|message| {
            component_materialization_error(
                game_object,
                &format!("source m_Component[{index}] {message}"),
            )
        })?;
        if pointer != (slot.file_id, slot.path_id) {
            return Err(component_materialization_error(
                game_object,
                &format!(
                    "source m_Component[{index}] PPtr ({}, {}) differs from overlay PPtr ({}, {})",
                    pointer.0, pointer.1, slot.file_id, slot.path_id
                ),
            ));
        }
    }

    let mut appended_element = if let Some(last) = source_elements.last() {
        last.clone()
    } else {
        empty_element_template.cloned().ok_or_else(|| {
            component_materialization_error(
                game_object,
                "empty m_Component array requires a TypeTree-derived element template",
            )
        })?
    };
    replace_component_pair_pointer(
        &mut appended_element,
        component_file_id,
        overlay.generated_component.component_asset.path_id,
    )
    .map_err(|message| component_materialization_error(game_object, &message))?;

    let mut edited = source_payload.clone();
    let array = edited
        .root
        .unique_field_in_base_chain_mut("m_Component")
        .map_err(|error| component_materialization_error(game_object, &error.to_string()))?
        .ok_or_else(|| {
            component_materialization_error(game_object, "GameObject has no m_Component field")
        })?;
    array
        .push_array_element_bounded(appended_element, max_component_elements)
        .map_err(|error| component_materialization_error(game_object, &error.to_string()))?;
    Ok(edited)
}

/// Materialize a component append using the FileID assigned by the completed Prefab plan.
pub fn materialize_planned_game_object_component_append(
    plan: &WorkspaceProcessedPrefabAssetPlan,
    source_payload: &ParsedObjectPayload,
    overlay: &WorkspaceGameObjectComponentListOverlay,
    empty_element_template: Option<&SerializedValue>,
    max_component_elements: usize,
) -> Result<ParsedObjectPayload, WorkspacePrefabProcessingError> {
    let game_object = overlay.game_object;
    if !plan.is_complete() {
        return Err(component_materialization_error(
            game_object,
            "PrefabProcessor asset allocation plan is incomplete",
        ));
    }
    let collection_index = overlay.generated_component.component_asset.collection_index;
    let mut matching_appends = plan.source_dependency_appends.iter().filter(|append| {
        append.source_file_index == game_object.file_index
            && append.pointer_field == WorkspaceSourceDependencyPointerField::GameObjectComponent
            && append.target_collection
                == (WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                    collection_index,
                })
    });
    let append = matching_appends.next().ok_or_else(|| {
        component_materialization_error(
            game_object,
            "Prefab plan has no source dependency append for the generated collection",
        )
    })?;
    if matching_appends.next().is_some() {
        return Err(component_materialization_error(
            game_object,
            "Prefab plan has duplicate source dependency appends for the generated collection",
        ));
    }
    let component_file_id = append.file_id.ok_or_else(|| {
        component_materialization_error(
            game_object,
            "source dependency FileID is deferred or unavailable",
        )
    })?;
    materialize_game_object_component_append(
        source_payload,
        overlay,
        component_file_id,
        empty_element_template,
        max_component_elements,
    )
}

fn component_pair_pointer(value: &SerializedValue) -> Result<(i32, i64), String> {
    if let Some(pointer) = value.pptr() {
        return Ok(pointer);
    }
    let fields = value.struct_fields().ok_or_else(|| {
        format!(
            "has unsupported value shape {}",
            component_value_kind(value)
        )
    })?;
    if fields.len() != 1 {
        return Err("has a ComponentPair value with fields besides component".to_owned());
    }
    let mut component_fields = fields.iter().filter(|field| field.name == "component");
    let component = component_fields
        .next()
        .ok_or_else(|| "has no component field".to_owned())?;
    if component_fields.next().is_some() {
        return Err("has duplicate component fields".to_owned());
    }
    component
        .value
        .pptr()
        .ok_or_else(|| "component field is not a PPtr".to_owned())
}

fn replace_component_pair_pointer(
    value: &mut SerializedValue,
    file_id: i32,
    path_id: i64,
) -> Result<(), String> {
    if matches!(value, SerializedValue::PPtr { .. }) {
        *value = SerializedValue::PPtr { file_id, path_id };
        return Ok(());
    }
    let fields = match value {
        SerializedValue::Struct(fields) => fields,
        _ => {
            return Err(format!(
                "template has unsupported value shape {}",
                component_value_kind(value)
            ));
        }
    };
    if fields.len() != 1 {
        return Err("template has fields besides ComponentPair.component".to_owned());
    }
    let mut component_fields = fields.iter_mut().filter(|field| field.name == "component");
    let component = component_fields
        .next()
        .ok_or_else(|| "template has no component field".to_owned())?;
    if component_fields.next().is_some() {
        return Err("template has duplicate component fields".to_owned());
    }
    if !matches!(component.value, SerializedValue::PPtr { .. }) {
        return Err("template component field is not a PPtr".to_owned());
    }
    component.value = SerializedValue::PPtr { file_id, path_id };
    Ok(())
}

fn component_value_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "i8",
        SerializedValue::U8(_) => "u8",
        SerializedValue::I16(_) => "i16",
        SerializedValue::U16(_) => "u16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "i32",
        SerializedValue::U32(_) => "u32",
        SerializedValue::I64(_) => "i64",
        SerializedValue::U64(_) => "u64",
        SerializedValue::F32(_) => "f32",
        SerializedValue::F64(_) => "f64",
        SerializedValue::StringBytes(_) => "string",
        SerializedValue::Bytes(_) => "bytes",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::ManagedReferenceId { .. } => "managed reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
        SerializedValue::Raw { .. } => "raw",
    }
}

fn component_materialization_error(
    game_object: WorkspaceSceneObjectIdentity,
    message: &str,
) -> WorkspacePrefabProcessingError {
    WorkspacePrefabProcessingError::WorkspaceOperationFailed {
        operation: "GameObject component-list materialization",
        message: format!(
            "file {} object {} PathID {}: {message}",
            game_object.file_index, game_object.object_index, game_object.path_id
        ),
    }
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
    complete: bool,
    incomplete_reason: Option<String>,
    collections: Vec<WorkspaceProcessedAssetCollectionPlan>,
    component_additions: Vec<WorkspaceGeneratedComponentAddition>,
    component_list_overlays: Vec<WorkspaceGameObjectComponentListOverlay>,
    component_overlay_slot_count: usize,
    constructor_pointers: Vec<WorkspaceGeneratedConstructorPointer>,
    dependency_entry_count: usize,
    source_dependency_appends: Vec<WorkspaceSourceCollectionDependencyAppend>,
    modeled_animator_controller_dependency_append_count: usize,
    deferred_source_dependency_file_count: usize,
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
