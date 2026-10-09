//! Identity-level reconstruction of AssetRipper's workspace PrefabProcessor order.

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

/// Replays the source-created collection and asset order after an identity-level PrefabProcessor
/// plan. PathIDs start at one independently in each ProcessedAssetCollection, as in the baseline.
pub fn plan_workspace_processed_prefab_assets(
    workspace: &WorkspaceSceneHierarchies,
    processing: &WorkspacePrefabProcessingPlan,
    source_external_identifier_counts: &[usize],
    animator_controller_inventory: &AnimatorControllerSourceDependencyInventory,
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
    let no_prefab_hierarchy_work = workspace.scenes().is_empty()
        && processing.processed_game_objects().is_empty()
        && processing.events().is_empty();
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
            if no_prefab_hierarchy_work {
                // Collections containing only non-hierarchy assets do not participate in
                // AddMissingTransforms, SceneHierarchyObject, or generated Prefab allocation.
                // Their absence from the typed hierarchy graph therefore creates no generated
                // PathID order to replay. Any scene, GameObject, PrefabInstance, or other
                // processor event keeps the original fail-closed behavior below.
                continue;
            }
            return Ok(incomplete_processed_asset_plan(&format!(
                "source collection file {file_index} has no recovered hierarchy graph; generated PathID order is indeterminate"
            )));
        };
        let has_hierarchy_inputs = graph.game_object_count() != 0
            || graph.transform_count() != 0
            || !graph.scene_assets().is_empty()
            || !graph.component_candidates().is_empty()
            || !graph.missing_transform_plans().is_empty();
        if !graph.is_complete() && has_hierarchy_inputs {
            return Ok(incomplete_processed_asset_plan(&format!(
                "collection file {file_index} has an incomplete scene hierarchy graph ({} GameObjects, {} Transforms, {} scene assets, {} component candidates, {} missing-Transform plans)",
                graph.game_object_count(),
                graph.transform_count(),
                graph.scene_assets().len(),
                graph.component_candidates().len(),
                graph.missing_transform_plans().len()
            )));
        }
        // The baseline's AddMissingTransforms pass creates no objects for a collection with no
        // GameObject, Transform, scene-asset, or component candidates. An opaque raw object in
        // such a collection cannot affect generated Prefab PathID order; keep the source scan
        // incomplete, but allow the empty processed-asset collections to be planned. If any
        // recognized hierarchy input exists, the fail-closed branch above still applies.
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
        modeled_animator_controller_dependency_append_count: 0,
        deferred_source_dependency_file_count: 0,
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
    let requested_source_dependency_keys = missing_transforms
        .len()
        .checked_add(workspace.scenes().len())
        .and_then(|count| count.checked_add(animator_controller_inventory.modeled_append_count()))
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "source processed-collection dependency append index",
        })?;
    source_dependency_append_keys
        .try_reserve(requested_source_dependency_keys)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "source processed-collection dependency append index",
            requested: requested_source_dependency_keys,
        })?;

    // AnimatorControllerProcessor runs before LightingDataProcessor and PrefabProcessor.
    plan_animator_controller_source_dependency_appends(
        &mut plan,
        &mut source_dependency_append_keys,
        animator_controller_inventory,
        limits,
    )?;

    // LightingDataProcessor runs before PrefabProcessor. It creates the first generated asset
    // collection referenced by a source collection and writes that PPtr to LightmapSettings.
    for scene in workspace.scenes() {
        let Some(owner) = scene.lighting_data_dependency_owner() else {
            continue;
        };
        plan_source_dependency_append(
            &mut plan,
            &mut source_dependency_append_keys,
            SourceDependencyAppendRequest {
                owner,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
                pointer_field:
                    WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
                target_asset: None,
                target_animator_state_machine: None,
            },
            limits,
        )?;
    }

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
            SourceDependencyAppendRequest {
                owner: game_object,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index,
                    },
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: Some(transform_asset),
                target_animator_state_machine: None,
            },
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

    assign_source_dependency_file_ids(
        &mut plan,
        source_external_identifier_counts,
        animator_controller_inventory,
        limits,
    )?;

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
        modeled_animator_controller_dependency_append_count: 0,
        deferred_source_dependency_file_count: 0,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceDependencyAppendRequest {
    owner: WorkspaceSceneObjectIdentity,
    target_collection: WorkspaceSourceDependencyAppendTarget,
    pointer_field: WorkspaceSourceDependencyPointerField,
    target_asset: Option<WorkspaceProcessedAssetIdentity>,
    target_animator_state_machine: Option<WorkspaceAnimatorControllerStateMachineIdentity>,
}

fn plan_source_dependency_append(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    seen: &mut HashSet<(usize, WorkspaceSourceDependencyAppendTarget)>,
    request: SourceDependencyAppendRequest,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    let key = (request.owner.file_index, request.target_collection);
    if seen.contains(&key) {
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
    seen.try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "source processed-collection dependency append index",
            requested,
        })?;
    seen.insert(key);
    plan.source_dependency_appends
        .push(WorkspaceSourceCollectionDependencyAppend {
            source_file_index: request.owner.file_index,
            target_collection: request.target_collection,
            pointer_field: request.pointer_field,
            first_source_owner: request.owner,
            target_asset: request.target_asset,
            target_animator_state_machine: request.target_animator_state_machine,
            file_id: None,
        });
    Ok(())
}

fn plan_animator_controller_source_dependency_appends(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    seen: &mut HashSet<(usize, WorkspaceSourceDependencyAppendTarget)>,
    inventory: &AnimatorControllerSourceDependencyInventory,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    for target in &inventory.append_targets {
        if inventory
            .deferred_file_indices
            .contains(&target.source_controller.file_index)
        {
            continue;
        }
        plan_source_dependency_append(
            plan,
            seen,
            SourceDependencyAppendRequest {
                owner: target.source_controller,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::GeneratedAnimatorControllerDependencies,
                pointer_field:
                    WorkspaceSourceDependencyPointerField::AnimatorControllerLayerStateMachine,
                target_asset: None,
                target_animator_state_machine: Some(*target),
            },
            limits,
        )?;
    }
    Ok(())
}

/// Ordered source-collection dependency effects from AnimatorControllerProcessor.
///
/// A source collection gets one dependency entry for the shared generated AnimatorController
/// collection when at least one decoded controller has a non-empty layer array. Files with any
/// controller whose relevant TypeTree data is unavailable or inconsistent remain deferred.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AnimatorControllerSourceDependencyInventory {
    append_file_indices: HashSet<usize>,
    deferred_file_indices: HashSet<usize>,
    append_targets: Vec<WorkspaceAnimatorControllerStateMachineIdentity>,
}

impl AnimatorControllerSourceDependencyInventory {
    pub fn modeled_append_count(&self) -> usize {
        self.append_file_indices.len()
    }

    pub fn deferred_file_count(&self) -> usize {
        self.deferred_file_indices.len()
    }
}

pub fn animator_controller_source_dependency_inventory(
    source_objects: &ObjectWorkspace,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<AnimatorControllerSourceDependencyInventory, WorkspacePrefabProcessingError> {
    const ANIMATOR_CONTROLLER_CLASS_ID: i32 = 91;
    let mut inventory = AnimatorControllerSourceDependencyInventory::default();
    for object in source_objects.objects() {
        if object.class_id.map(i32::from).unwrap_or(object.raw_type_id)
            == ANIMATOR_CONTROLLER_CLASS_ID
        {
            if inventory.deferred_file_indices.contains(&object.file_index) {
                continue;
            }
            let first_state_machine_index =
                object.payload.as_ref().ok().and_then(|payload| {
                    animator_controller_requires_source_dependency(&payload.root)
                });
            match first_state_machine_index {
                Some(Some(state_machine_index)) => {
                    let target = WorkspaceAnimatorControllerStateMachineIdentity {
                        source_controller: WorkspaceSceneObjectIdentity {
                            file_index: object.file_index,
                            object_index: object.object_index,
                            path_id: object.path_id,
                            class_id: Some(ANIMATOR_CONTROLLER_CLASS_ID),
                        },
                        state_machine_index,
                        source_layer_index: 0,
                    };
                    insert_animator_controller_source_append(&mut inventory, target, limits)?;
                }
                Some(None) => {}
                None => {
                    inventory.append_file_indices.remove(&object.file_index);
                    insert_animator_controller_source_file(
                        &mut inventory.deferred_file_indices,
                        &inventory.append_file_indices,
                        object.file_index,
                        limits,
                    )?;
                }
            }
        }
    }
    let deferred_files = &inventory.deferred_file_indices;
    inventory
        .append_targets
        .retain(|target| !deferred_files.contains(&target.source_controller.file_index));
    Ok(inventory)
}

fn insert_animator_controller_source_append(
    inventory: &mut AnimatorControllerSourceDependencyInventory,
    target: WorkspaceAnimatorControllerStateMachineIdentity,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    let file_index = target.source_controller.file_index;
    if inventory.append_file_indices.contains(&file_index) {
        return Ok(());
    }
    let requested = inventory
        .append_file_indices
        .len()
        .checked_add(inventory.deferred_file_indices.len())
        .and_then(|count| count.checked_add(1))
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "AnimatorController source dependency file indexes",
        })?;
    if requested > limits.max_collections {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "AnimatorController source dependency file indexes",
            requested,
            limit: limits.max_collections,
        });
    }
    inventory.append_file_indices.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "AnimatorController source dependency inventory",
            requested,
        }
    })?;
    inventory.append_targets.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "AnimatorController source StateMachine targets",
            requested,
        }
    })?;
    inventory.append_file_indices.insert(file_index);
    inventory.append_targets.push(target);
    Ok(())
}

fn insert_animator_controller_source_file(
    destination: &mut HashSet<usize>,
    other_effects: &HashSet<usize>,
    file_index: usize,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    if destination.contains(&file_index) {
        return Ok(());
    }
    let requested = destination
        .len()
        .checked_add(other_effects.len())
        .and_then(|count| count.checked_add(1))
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "AnimatorController source dependency file indexes",
        })?;
    if requested > limits.max_collections {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "AnimatorController source dependency file indexes",
            requested,
            limit: limits.max_collections,
        });
    }
    destination
        .try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "AnimatorController source dependency inventory",
            requested,
        })?;
    destination.insert(file_index);
    Ok(())
}

fn animator_controller_requires_source_dependency(root: &SerializedValue) -> Option<Option<usize>> {
    let controller_constant = root.unique_field_in_base_chain("m_Controller").ok()??;
    let layer_array =
        unique_field_from_aliases(controller_constant, &["m_LayerArray", "m_HumanLayerArray"])?;
    let layer_values = layer_array.array_elements()?;
    let state_machine_array = controller_constant
        .unique_field("m_StateMachineArray")
        .ok()??;
    let state_machine_values = state_machine_array.array_elements()?;

    let mut first_state_machine_index = None;
    for (layer_index, layer_entry) in layer_values.iter().enumerate() {
        let layer = unwrap_animator_layer_constant(layer_entry)?;
        let state_machine_index = layer
            .unique_field_in_base_chain("m_StateMachineIndex")
            .ok()??;
        let state_machine_index = serialized_nonnegative_index(state_machine_index)?;
        if state_machine_index >= state_machine_values.len() {
            return None;
        }
        if layer_index == 0 {
            first_state_machine_index = Some(state_machine_index);
        }
    }

    Some(first_state_machine_index)
}

fn unique_field_from_aliases<'a>(
    value: &'a SerializedValue,
    aliases: &[&str],
) -> Option<&'a SerializedValue> {
    let mut found = None;
    for alias in aliases {
        if let Some(candidate) = value.unique_field(alias).ok()? {
            if found.is_some() {
                return None;
            }
            found = Some(candidate);
        }
    }
    found
}

fn unwrap_animator_layer_constant(value: &SerializedValue) -> Option<&SerializedValue> {
    let capitalized_data = value.unique_field("Data").ok()?;
    let lowercase_data = value.unique_field("data").ok()?;
    let wrapped = match (capitalized_data, lowercase_data) {
        (Some(data), None) | (None, Some(data)) => Some(data),
        (None, None) => None,
        (Some(_), Some(_)) => return None,
    };
    if let Some(layer) = wrapped {
        if value
            .unique_field_in_base_chain("m_StateMachineIndex")
            .ok()?
            .is_some()
        {
            return None;
        }
        Some(layer)
    } else {
        Some(value)
    }
}

fn serialized_nonnegative_index(value: &SerializedValue) -> Option<usize> {
    match value {
        SerializedValue::U8(index) => Some(usize::from(*index)),
        SerializedValue::U16(index) => Some(usize::from(*index)),
        SerializedValue::U32(index) => usize::try_from(*index).ok(),
        SerializedValue::U64(index) => usize::try_from(*index).ok(),
        SerializedValue::I8(index) => usize::try_from(*index).ok(),
        SerializedValue::I16(index) => usize::try_from(*index).ok(),
        SerializedValue::I32(index) => usize::try_from(*index).ok(),
        SerializedValue::I64(index) => usize::try_from(*index).ok(),
        _ => None,
    }
}

fn assign_source_dependency_file_ids(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    source_external_identifier_counts: &[usize],
    animator_controller_inventory: &AnimatorControllerSourceDependencyInventory,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    plan.modeled_animator_controller_dependency_append_count =
        animator_controller_inventory.modeled_append_count();

    let mut appended_count_by_file = HashMap::<usize, usize>::new();
    appended_count_by_file
        .try_reserve(plan.source_dependency_appends.len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "source dependency append order index",
            requested: plan.source_dependency_appends.len(),
        })?;

    let mut deferred_source_files = HashSet::new();
    for append in &mut plan.source_dependency_appends {
        let Some(&external_identifier_count) =
            source_external_identifier_counts.get(append.source_file_index)
        else {
            return Err(WorkspacePrefabProcessingError::WorkspaceOperationFailed {
                operation: "source dependency FileID assignment",
                message: format!(
                    "external identifier count is missing for source file {}",
                    append.source_file_index
                ),
            });
        };
        if animator_controller_inventory
            .deferred_file_indices
            .contains(&append.source_file_index)
        {
            if !deferred_source_files.contains(&append.source_file_index) {
                let requested = deferred_source_files.len().checked_add(1).ok_or(
                    WorkspacePrefabProcessingError::CountOverflow {
                        resource: "deferred source dependency files",
                    },
                )?;
                deferred_source_files.try_reserve(1).map_err(|_| {
                    WorkspacePrefabProcessingError::AllocationFailed {
                        resource: "deferred source dependency file index",
                        requested,
                    }
                })?;
                deferred_source_files.insert(append.source_file_index);
            }
            continue;
        }
        let prior_append_count = appended_count_by_file
            .get(&append.source_file_index)
            .copied()
            .unwrap_or(0);
        let initial_dependency_count = external_identifier_count.checked_add(1).ok_or(
            WorkspacePrefabProcessingError::CountOverflow {
                resource: "source collection dependency entries",
            },
        )?;
        let dependency_file_id = initial_dependency_count
            .checked_add(prior_append_count)
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "source collection dependency FileID",
            })?;
        let resulting_dependency_count = dependency_file_id.checked_add(1).ok_or(
            WorkspacePrefabProcessingError::CountOverflow {
                resource: "source collection dependency entries",
            },
        )?;
        if resulting_dependency_count > limits.max_source_dependency_entries_per_collection {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "source collection dependency entries",
                requested: resulting_dependency_count,
                limit: limits.max_source_dependency_entries_per_collection,
            });
        }
        append.file_id = Some(i32::try_from(dependency_file_id).map_err(|_| {
            WorkspacePrefabProcessingError::CountOverflow {
                resource: "source collection dependency FileID",
            }
        })?);
        appended_count_by_file.insert(
            append.source_file_index,
            prior_append_count.checked_add(1).ok_or(
                WorkspacePrefabProcessingError::CountOverflow {
                    resource: "source collection dependency append order",
                },
            )?,
        );
    }
    plan.deferred_source_dependency_file_count = deferred_source_files.len();
    if !deferred_source_files.is_empty() {
        plan.complete = false;
        plan.incomplete_reason = Some(format!(
            "AnimatorController source dependency effects remain unknown in {} collection files that also require source dependency FileIDs; those FileIDs are deferred",
            deferred_source_files.len()
        ));
    }
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

/// Candidate asset-bundle name that the source processor tries to assign to a scene hierarchy.
///
/// Mirrors the `PrefabProcessor.Process` scene pass
/// (`AssetRipper/Source/AssetRipper.Processing/Prefabs/PrefabProcessor.cs`,
/// lines 28-48): the first `SerializedBundle` among the scene collections
/// supplies the candidate; its first `IAssetBundle` asset's `GetAssetBundleName`
/// wins when present, otherwise the bundle name is used. With no `SerializedBundle`
/// the source does not attempt an assignment. `GeneratedObjectBase.AssetBundleName`
/// has a no-op setter, so the effective generated hierarchy property remains null.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyBundleName {
    /// No `SerializedBundle` among the scene collections: no assignment candidate.
    Unset,
    /// A `SerializedBundle` without an `IAssetBundle` asset: `bundle.Name`.
    BundleName(String),
    /// The first `IAssetBundle` asset's `GetAssetBundleName()`.
    AssetName(String),
}

/// Selects the candidate that `PrefabProcessor` attempts to assign to the hierarchy.
///
/// `has_serialized_bundle` records the `FirstOrDefault(b => b is SerializedBundle)`
/// lookup and gates everything, matching the nested `if (bundle is not null)`.
/// `bundle_name` is that bundle's `Name`; `asset_bundle_name` carries the first
/// `IAssetBundle` asset's resolved `GetAssetBundleName()` (`Some` when such an
/// asset exists, `None` otherwise). Collection traversal order, the
/// `FetchAssets` order, and the `GetAssetBundleName` resolution itself (see
/// `asset_bundle.rs`) stay caller-side, as does the debug-only streamed-scene
/// assertion, which is intentionally not modeled. The generated `SceneHierarchyObject` inherits
/// a no-op `AssetBundleName` setter from `GeneratedObjectBase`, so the candidate is discarded and
/// the effective property remains unset.
pub fn scene_hierarchy_asset_bundle_name(
    has_serialized_bundle: bool,
    bundle_name: &str,
    asset_bundle_name: Option<&str>,
) -> WorkspaceSceneHierarchyBundleName {
    if !has_serialized_bundle {
        return WorkspaceSceneHierarchyBundleName::Unset;
    }
    match asset_bundle_name {
        Some(name) => WorkspaceSceneHierarchyBundleName::AssetName(name.to_owned()),
        None => WorkspaceSceneHierarchyBundleName::BundleName(bundle_name.to_owned()),
    }
}

/// One source bundle as seen by the `PrefabProcessor` scene pass.
///
/// `bundle_index` is the workspace container-bundle identity. `bundle_name`
/// is that bundle's `Name` (`FileContainer.Name` after
/// `SpecialFileNames.FixFileIdentifier`). `is_serialized_bundle` records the
/// `b is SerializedBundle` filter from
/// `scene.Collections.Select(c => c.Bundle).FirstOrDefault(b => b is SerializedBundle)`:
/// only serialized container bundles participate; standalone files and generated
/// processed collections never supply a hierarchy bundle name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneBundleSource {
    pub bundle_index: usize,
    pub bundle_name: String,
    pub is_serialized_bundle: bool,
}

/// One `IAssetBundle` asset in `Bundle.FetchAssets()` order.
///
/// `resolved_name` must already be the `AssetBundleExtensions.GetAssetBundleName()`
/// result for that asset (non-empty serialized `AssetBundleName_R` wins, otherwise
/// the owning bundle name), resolved with `asset_bundle::get_asset_bundle_name`.
/// The list for each bundle must follow `Bundle.FetchAssets()`:
/// direct collections in order, then child bundles recursively in source entry
/// order (`AssetRipper/Source/AssetRipper.Assets/Bundles/Bundle.cs`, lines 368-384).
/// `has_streamed_scene_flag` records `Has_IsStreamedSceneAssetBundle()` and
/// `is_streamed_scene_asset_bundle` records `IsStreamedSceneAssetBundle`; together
/// they replay the scene-pass `Debug.Assert`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceBundleAssetNameFact {
    pub resolved_name: String,
    pub has_streamed_scene_flag: bool,
    pub is_streamed_scene_asset_bundle: bool,
}

/// Per-scene hierarchy bundle assignment from the integrated scene pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyBundleAssignment {
    pub scene_index: usize,
    /// Candidate value that `PrefabProcessor` passes to the generated hierarchy setter.
    pub name: WorkspaceSceneHierarchyBundleName,
    /// Effective `SceneHierarchyObject.AssetBundleName` read-back value. The generated hierarchy
    /// inherits a no-op setter from `GeneratedObjectBase`, so the source leaves this unset.
    pub effective_asset_bundle_name: Option<String>,
    /// Present when the selected asset carries the streamed-scene field with a
    /// false value, which trips the source `Debug.Assert` in debug builds while
    /// release builds continue with the resolved name. Rust always continues with
    /// the resolved name and records this explicit warning instead of asserting.
    pub streamed_scene_assert_warning: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyBundlePlanLimits {
    pub max_scenes: usize,
    pub max_collections_per_scene: usize,
    pub max_bundle_name_bytes: usize,
    pub max_assets_per_bundle: usize,
}

impl Default for WorkspaceSceneHierarchyBundlePlanLimits {
    fn default() -> Self {
        Self {
            max_scenes: 1_000_000,
            max_collections_per_scene: 1_000_000,
            max_bundle_name_bytes: 16 * 1024 * 1024,
            max_assets_per_bundle: 1_000_000,
        }
    }
}

/// Plan every scene hierarchy's asset-bundle name in source order.
///
/// `scene_collection_file_indices` holds one entry per scene in
/// `GameBundle.Scenes` order; each inner list is that scene's
/// `SceneDefinition.Collections` order. `file_bundles` is indexed by workspace
/// file index; `Some` carries the owning bundle source and `None` means the file
/// has no `SerializedBundle` (standalone input). `bundle_assets_in_fetch_order`
/// maps a bundle index to its `IAssetBundle` facts in `FetchAssets()` order.
///
/// For each scene the first collection whose bundle is a serialized bundle wins
/// (`FirstOrDefault`), then that bundle's first `IAssetBundle` asset wins
/// (`FetchAssets().OfType<IAssetBundle>().FirstOrDefault()`): its resolved
/// `GetAssetBundleName()` becomes the candidate `AssetName`, while a bundle without such an
/// asset falls back to the candidate `bundle.Name`. A scene without any serialized bundle
/// stays `Unset`. All candidates are discarded by `GeneratedObjectBase.AssetBundleName`'s
/// no-op setter, leaving the effective property unset. The streamed-scene `Debug.Assert` becomes
/// an explicit warning; it never changes the candidate and never fails the plan.
pub fn plan_workspace_scene_hierarchy_bundle_names(
    scene_collection_file_indices: &[Vec<usize>],
    file_bundles: &[Option<WorkspaceSceneBundleSource>],
    bundle_assets_in_fetch_order: &HashMap<usize, Vec<WorkspaceBundleAssetNameFact>>,
    limits: WorkspaceSceneHierarchyBundlePlanLimits,
) -> Result<Vec<WorkspaceSceneHierarchyBundleAssignment>, WorkspacePrefabProcessingError> {
    if scene_collection_file_indices.len() > limits.max_scenes {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "scene hierarchy bundle scenes",
            requested: scene_collection_file_indices.len(),
            limit: limits.max_scenes,
        });
    }
    let mut assignments = Vec::new();
    assignments
        .try_reserve_exact(scene_collection_file_indices.len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "scene hierarchy bundle assignments",
            requested: scene_collection_file_indices.len(),
        })?;
    for (scene_index, collection_file_indices) in scene_collection_file_indices.iter().enumerate() {
        if collection_file_indices.len() > limits.max_collections_per_scene {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "scene hierarchy bundle collections",
                requested: collection_file_indices.len(),
                limit: limits.max_collections_per_scene,
            });
        }
        let mut selected_bundle: Option<&WorkspaceSceneBundleSource> = None;
        for &file_index in collection_file_indices {
            let bundle = file_bundles.get(file_index).ok_or_else(|| {
                WorkspacePrefabProcessingError::WorkspaceOperationFailed {
                    operation: "scene hierarchy bundle selection",
                    message: format!(
                        "scene {scene_index} collection file {file_index} is outside {} files",
                        file_bundles.len()
                    ),
                }
            })?;
            if let Some(source) = bundle {
                if source.bundle_name.len() > limits.max_bundle_name_bytes {
                    return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                        resource: "scene hierarchy bundle name bytes",
                        requested: source.bundle_name.len(),
                        limit: limits.max_bundle_name_bytes,
                    });
                }
                if source.is_serialized_bundle {
                    selected_bundle = Some(source);
                    break;
                }
            }
        }
        let Some(bundle) = selected_bundle else {
            assignments.push(WorkspaceSceneHierarchyBundleAssignment {
                scene_index,
                name: WorkspaceSceneHierarchyBundleName::Unset,
                effective_asset_bundle_name: None,
                streamed_scene_assert_warning: None,
            });
            continue;
        };
        let assets = bundle_assets_in_fetch_order.get(&bundle.bundle_index);
        let Some(assets) = assets else {
            assignments.push(WorkspaceSceneHierarchyBundleAssignment {
                scene_index,
                name: WorkspaceSceneHierarchyBundleName::BundleName(bundle.bundle_name.clone()),
                effective_asset_bundle_name: None,
                streamed_scene_assert_warning: None,
            });
            continue;
        };
        if assets.len() > limits.max_assets_per_bundle {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "scene hierarchy bundle assets",
                requested: assets.len(),
                limit: limits.max_assets_per_bundle,
            });
        }
        let Some(first) = assets.first() else {
            assignments.push(WorkspaceSceneHierarchyBundleAssignment {
                scene_index,
                name: WorkspaceSceneHierarchyBundleName::BundleName(bundle.bundle_name.clone()),
                effective_asset_bundle_name: None,
                streamed_scene_assert_warning: None,
            });
            continue;
        };
        if first.resolved_name.len() > limits.max_bundle_name_bytes {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "scene hierarchy asset bundle name bytes",
                requested: first.resolved_name.len(),
                limit: limits.max_bundle_name_bytes,
            });
        }
        let warning = if first.has_streamed_scene_flag && !first.is_streamed_scene_asset_bundle {
            Some(format!(
                "scene {scene_index} bundle '{}' asset '{:?}' carries IsStreamedSceneAssetBundle=false; source Debug.Assert would fire in debug builds, release continues with the resolved name",
                bundle.bundle_name, first.resolved_name
            ))
        } else {
            None
        };
        assignments.push(WorkspaceSceneHierarchyBundleAssignment {
            scene_index,
            name: WorkspaceSceneHierarchyBundleName::AssetName(first.resolved_name.clone()),
            effective_asset_bundle_name: None,
            streamed_scene_assert_warning: warning,
        });
    }
    Ok(assignments)
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
    use crate::io::binary::Endian;
    use crate::serialized_file::metadata::{SerializedType, TypeTree, TypeTreeNode};
    use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
    use crate::serialized_file::object::SerializedFieldValue;

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
            modeled_animator_controller_dependency_append_count: 0,
            deferred_source_dependency_file_count: 0,
            asset_count: 0,
            total_collection_name_bytes: 0,
        }
    }

    fn tree_node(level: u8, name: &str, type_name: &str, byte_size: i32) -> TypeTreeNode {
        TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size,
            index: 0,
            meta_flags: 0,
            reference_type_hash: None,
        }
    }

    fn transform_metadata(
        include_euler_hint: bool,
        include_root_order: bool,
    ) -> SerializedFileMetadata {
        let mut nodes = vec![
            tree_node(0, "", "Transform", -1),
            tree_node(1, "Base", "Component", -1),
            tree_node(2, "m_GameObject", "PPtr<GameObject>", 12),
            tree_node(3, "m_FileID", "SInt32", 4),
            tree_node(3, "m_PathID", "SInt64", 8),
            tree_node(1, "m_Father", "PPtr<Transform>", 12),
            tree_node(2, "m_FileID", "SInt32", 4),
            tree_node(2, "m_PathID", "SInt64", 8),
            tree_node(1, "m_Children", "vector", -1),
            tree_node(2, "Array", "Array", -1),
            tree_node(3, "size", "int", 4),
            tree_node(3, "data", "PPtr<Transform>", 12),
            tree_node(4, "m_FileID", "SInt32", 4),
            tree_node(4, "m_PathID", "SInt64", 8),
        ];
        if include_root_order {
            nodes.push(tree_node(1, "m_RootOrder", "int", 4));
        }
        if include_euler_hint {
            nodes.extend([
                tree_node(1, "m_LocalEulerAnglesHint", "Vector3f", -1),
                tree_node(2, "x", "float", 4),
                tree_node(2, "y", "float", 4),
                tree_node(2, "z", "float", 4),
            ]);
        }
        nodes.extend([
            tree_node(1, "m_LocalPosition", "Vector3f", -1),
            tree_node(2, "x", "float", 4),
            tree_node(2, "y", "float", 4),
            tree_node(2, "z", "float", 4),
            tree_node(1, "m_LocalRotation", "Quaternionf", -1),
            tree_node(2, "x", "float", 4),
            tree_node(2, "y", "float", 4),
            tree_node(2, "z", "float", 4),
            tree_node(2, "w", "float", 4),
            tree_node(1, "m_LocalScale", "Vector3f", -1),
            tree_node(2, "x", "float", 4),
            tree_node(2, "y", "float", 4),
            tree_node(2, "z", "float", 4),
        ]);
        SerializedFileMetadata {
            prefix: SerializedFileMetadataPrefix {
                metadata_offset: 0,
                metadata_size: 0,
                body_endian: Endian::Little,
                unity_version_text: Some("2019.4.31f1".to_owned()),
                target_platform: Some(0),
                has_type_tree: true,
                consumed_bytes: 0,
            },
            types: vec![SerializedType {
                raw_type_id: TRANSFORM_CLASS_ID,
                type_id_for_hash: TRANSFORM_CLASS_ID,
                is_stripped_type: false,
                script_type_index: -1,
                script_id: None,
                old_type_hash: None,
                type_tree: Some(TypeTree {
                    nodes,
                    string_buffer: Vec::new(),
                }),
                type_dependencies: Vec::new(),
            }],
            long_file_id: None,
            objects: Vec::new(),
            script_types: Vec::new(),
            externals: Vec::new(),
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        }
    }

    fn generated_transform_plan() -> (
        WorkspaceProcessedPrefabAssetPlan,
        WorkspaceProcessedAssetIdentity,
        WorkspaceSceneObjectIdentity,
    ) {
        let mut plan = empty_plan();
        let limits = WorkspaceProcessedPrefabAssetPlanLimits::default();
        let collection_index = add_processed_collection(
            &mut plan,
            WorkspaceProcessedCollectionKind::MissingPrefabTransforms,
            "Missing Prefab Transforms",
            limits,
        )
        .unwrap();
        let game_object = WorkspaceSceneObjectIdentity {
            file_index: 3,
            object_index: 7,
            path_id: 99,
            class_id: Some(1),
        };
        let asset = add_processed_asset(
            &mut plan,
            collection_index,
            TRANSFORM_CLASS_ID,
            WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
                game_object,
                default_values: default_workspace_generated_transform_values(),
            },
            limits,
        )
        .unwrap();
        let file_id = add_processed_source_file_dependency(
            &mut plan,
            collection_index,
            game_object.file_index,
            limits,
        )
        .unwrap();
        add_generated_constructor_pointer(
            &mut plan,
            WorkspaceGeneratedConstructorPointer {
                source_asset: asset,
                field: WorkspaceGeneratedPointerField::TransformGameObject,
                file_id,
                path_id: game_object.path_id,
                target: game_object,
            },
            limits,
        )
        .unwrap();
        (plan, asset, game_object)
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
        let target_collection = WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
            collection_index: 2,
        };
        let limits = WorkspaceProcessedPrefabAssetPlanLimits::default();

        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner: first_owner,
                target_collection,
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: Some(target_asset),
                target_animator_state_machine: None,
            },
            limits,
        )
        .unwrap();
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner: later_owner,
                target_collection,
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: Some(target_asset),
                target_animator_state_machine: None,
            },
            limits,
        )
        .unwrap();

        assert_eq!(plan.source_dependency_appends.len(), 1);
        assert_eq!(
            plan.source_dependency_appends[0].first_source_owner,
            first_owner
        );
        assert_eq!(
            plan.source_dependency_appends[0].target_collection,
            target_collection
        );
        assert_eq!(plan.source_dependency_appends[0].file_id, None);
    }

    #[test]
    fn source_dependency_file_ids_follow_prior_processor_appends_and_external_identifiers() {
        let mut plan = empty_plan();
        let mut seen = HashSet::new();
        let lightmap_settings = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 0,
            path_id: 10,
            class_id: Some(157),
        };
        let game_object = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 1,
            path_id: 11,
            class_id: Some(1),
        };
        let limits = WorkspaceProcessedPrefabAssetPlanLimits::default();

        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner: lightmap_settings,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
                pointer_field:
                    WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
                target_asset: None,
                target_animator_state_machine: None,
            },
            limits,
        )
        .unwrap();
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner: game_object,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index: 2,
                    },
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: Some(WorkspaceProcessedAssetIdentity {
                    collection_index: 2,
                    path_id: 1,
                    class_id: 4,
                }),
                target_animator_state_machine: None,
            },
            limits,
        )
        .unwrap();

        assign_source_dependency_file_ids(
            &mut plan,
            &[3],
            &AnimatorControllerSourceDependencyInventory::default(),
            limits,
        )
        .unwrap();

        assert_eq!(plan.source_dependency_appends[0].file_id, Some(4));
        assert_eq!(plan.source_dependency_appends[1].file_id, Some(5));
    }

    #[test]
    fn unknown_animator_controller_effect_defers_only_requested_file_ids() {
        let mut plan = empty_plan();
        let mut seen = HashSet::new();
        let owner = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 1,
            path_id: 11,
            class_id: Some(1),
        };
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
                pointer_field:
                    WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
                target_asset: None,
                target_animator_state_machine: None,
            },
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index: 2,
                    },
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: None,
                target_animator_state_machine: None,
            },
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        let mut animator_controller_inventory =
            AnimatorControllerSourceDependencyInventory::default();
        animator_controller_inventory
            .deferred_file_indices
            .insert(0);

        assign_source_dependency_file_ids(
            &mut plan,
            &[3],
            &animator_controller_inventory,
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();

        assert!(!plan.is_complete());
        assert_eq!(plan.deferred_source_dependency_file_count(), 1);
        assert!(plan
            .source_dependency_appends
            .iter()
            .all(|append| append.file_id.is_none()));
        assert!(plan
            .incomplete_reason()
            .unwrap()
            .contains("AnimatorController source dependency effects remain unknown"));
    }

    #[test]
    fn animator_controller_dependency_appends_precede_lighting_data_and_prefab_file_ids() {
        let mut plan = empty_plan();
        let mut seen = HashSet::new();
        let controller = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 3,
            path_id: 31,
            class_id: Some(91),
        };
        let owner = WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 1,
            path_id: 11,
            class_id: Some(1),
        };
        let target = WorkspaceAnimatorControllerStateMachineIdentity {
            source_controller: controller,
            state_machine_index: 2,
            source_layer_index: 0,
        };
        let mut animator_controller_inventory =
            AnimatorControllerSourceDependencyInventory::default();
        animator_controller_inventory.append_file_indices.insert(0);
        animator_controller_inventory.append_targets.push(target);
        plan_animator_controller_source_dependency_appends(
            &mut plan,
            &mut seen,
            &animator_controller_inventory,
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
                pointer_field:
                    WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
                target_asset: None,
                target_animator_state_machine: None,
            },
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        plan_source_dependency_append(
            &mut plan,
            &mut seen,
            SourceDependencyAppendRequest {
                owner,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index: 2,
                    },
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                target_asset: None,
                target_animator_state_machine: None,
            },
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();

        assign_source_dependency_file_ids(
            &mut plan,
            &[3],
            &animator_controller_inventory,
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();

        assert_eq!(
            plan.modeled_animator_controller_dependency_append_count(),
            1
        );
        assert_eq!(plan.source_dependency_appends.len(), 3);
        assert_eq!(
            plan.source_dependency_appends[0].target_collection,
            WorkspaceSourceDependencyAppendTarget::GeneratedAnimatorControllerDependencies
        );
        assert_eq!(
            plan.source_dependency_appends[0].pointer_field,
            WorkspaceSourceDependencyPointerField::AnimatorControllerLayerStateMachine
        );
        assert_eq!(
            plan.source_dependency_appends[0].target_animator_state_machine,
            Some(target)
        );
        assert_eq!(plan.source_dependency_appends[0].file_id, Some(4));
        assert_eq!(plan.source_dependency_appends[1].file_id, Some(5));
        assert_eq!(plan.source_dependency_appends[2].file_id, Some(6));
        assert!(plan.is_complete());
    }

    #[test]
    fn animator_controller_layer_inventory_checks_array_aliases_and_indices() {
        let controller =
            |layer_array_name: &str, layer_indices: &[u32], state_machine_count: usize| {
                let layers = layer_indices
                    .iter()
                    .map(|index| {
                        SerializedValue::Struct(vec![SerializedFieldValue {
                            name: "Data".to_owned(),
                            value: SerializedValue::Struct(vec![SerializedFieldValue {
                                name: "m_StateMachineIndex".to_owned(),
                                value: SerializedValue::U32(*index),
                            }]),
                        }])
                    })
                    .collect();
                SerializedValue::Struct(vec![SerializedFieldValue {
                    name: "m_Controller".to_owned(),
                    value: SerializedValue::Struct(vec![
                        SerializedFieldValue {
                            name: layer_array_name.to_owned(),
                            value: SerializedValue::Array(layers),
                        },
                        SerializedFieldValue {
                            name: "m_StateMachineArray".to_owned(),
                            value: SerializedValue::Array(
                                (0..state_machine_count)
                                    .map(|_| SerializedValue::Struct(Vec::new()))
                                    .collect(),
                            ),
                        },
                    ]),
                }])
            };

        assert_eq!(
            animator_controller_requires_source_dependency(&controller("m_LayerArray", &[0, 1], 2)),
            Some(Some(0))
        );
        let direct_layer_value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Controller".to_owned(),
            value: SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "m_LayerArray".to_owned(),
                    value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                        SerializedFieldValue {
                            name: "m_StateMachineIndex".to_owned(),
                            value: SerializedValue::U32(0),
                        },
                    ])]),
                },
                SerializedFieldValue {
                    name: "m_StateMachineArray".to_owned(),
                    value: SerializedValue::Array(vec![SerializedValue::Struct(Vec::new())]),
                },
            ]),
        }]);
        assert_eq!(
            animator_controller_requires_source_dependency(&direct_layer_value),
            Some(Some(0))
        );
        assert_eq!(
            animator_controller_requires_source_dependency(&controller(
                "m_HumanLayerArray",
                &[],
                1
            )),
            Some(None)
        );
        assert_eq!(
            animator_controller_requires_source_dependency(&controller("m_LayerArray", &[1], 1)),
            None
        );
    }

    #[test]
    fn animator_controller_layer_inventory_rejects_missing_ambiguous_and_mistyped_fields() {
        let both_layer_arrays = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Controller".to_owned(),
            value: SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "m_LayerArray".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                },
                SerializedFieldValue {
                    name: "m_HumanLayerArray".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                },
                SerializedFieldValue {
                    name: "m_StateMachineArray".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                },
            ]),
        }]);
        assert_eq!(
            animator_controller_requires_source_dependency(&both_layer_arrays),
            None
        );

        let mistyped_layer_index = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Controller".to_owned(),
            value: SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "m_LayerArray".to_owned(),
                    value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                        SerializedFieldValue {
                            name: "Data".to_owned(),
                            value: SerializedValue::Struct(vec![SerializedFieldValue {
                                name: "m_StateMachineIndex".to_owned(),
                                value: SerializedValue::StringBytes(b"0".to_vec()),
                            }]),
                        },
                    ])]),
                },
                SerializedFieldValue {
                    name: "m_StateMachineArray".to_owned(),
                    value: SerializedValue::Array(vec![SerializedValue::Struct(Vec::new())]),
                },
            ]),
        }]);
        assert_eq!(
            animator_controller_requires_source_dependency(&mistyped_layer_index),
            None
        );
    }

    #[test]
    fn materializes_game_object_component_append_without_changing_source_slots() {
        let game_object = WorkspaceSceneObjectIdentity {
            file_index: 3,
            object_index: 2,
            path_id: 10,
            class_id: Some(1),
        };
        let original_elements = vec![
            SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            },
            SerializedValue::PPtr {
                file_id: 2,
                path_id: 88,
            },
        ];
        let source_slots = vec![
            WorkspaceSourceGameObjectComponentSlot {
                source_array_index: 0,
                file_id: 0,
                path_id: 0,
                target: SceneRelationshipTarget::Null,
            },
            WorkspaceSourceGameObjectComponentSlot {
                source_array_index: 1,
                file_id: 2,
                path_id: 88,
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 88 },
            },
        ];
        let generated_component = WorkspaceGeneratedComponentAddition {
            game_object,
            component_class_id: 4,
            appended_index: 2,
            component_asset: WorkspaceProcessedAssetIdentity {
                collection_index: 5,
                path_id: 200,
                class_id: 4,
            },
        };
        let overlay = WorkspaceGameObjectComponentListOverlay {
            game_object,
            source_slots,
            generated_component,
        };
        let source_payload = ParsedObjectPayload {
            object_index: game_object.object_index,
            path_id: game_object.path_id,
            type_name: "GameObject".to_owned(),
            root: SerializedValue::Struct(vec![SerializedFieldValue {
                name: "Base".to_owned(),
                value: SerializedValue::Struct(vec![SerializedFieldValue {
                    name: "m_Component".to_owned(),
                    value: SerializedValue::Array(original_elements.clone()),
                }]),
            }]),
            consumed_bytes: 16,
            trailing_bytes: vec![0xaa],
            allocated_bytes: 64,
        };

        let edited =
            materialize_game_object_component_append(&source_payload, &overlay, 3, None, 3)
                .unwrap();
        let component_array = edited
            .root
            .unique_field_in_base_chain("m_Component")
            .unwrap()
            .unwrap()
            .array_elements()
            .unwrap();
        assert_eq!(&component_array[..2], original_elements);
        assert_eq!(
            component_array[2].pptr(),
            Some((3, generated_component.component_asset.path_id))
        );
        assert_eq!(edited.trailing_bytes, [0xaa]);

        let wrapper_payload = ParsedObjectPayload {
            root: SerializedValue::Struct(vec![SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    SerializedFieldValue {
                        name: "component".to_owned(),
                        value: SerializedValue::PPtr {
                            file_id: 0,
                            path_id: 13,
                        },
                    },
                ])]),
            }]),
            ..source_payload.clone()
        };
        let wrapper_overlay = WorkspaceGameObjectComponentListOverlay {
            source_slots: vec![WorkspaceSourceGameObjectComponentSlot {
                source_array_index: 0,
                file_id: 0,
                path_id: 13,
                target: SceneRelationshipTarget::LocalObject { object_index: 4 },
            }],
            generated_component: WorkspaceGeneratedComponentAddition {
                appended_index: 1,
                ..generated_component
            },
            ..overlay.clone()
        };
        let wrapper_edited = materialize_game_object_component_append(
            &wrapper_payload,
            &wrapper_overlay,
            3,
            None,
            2,
        )
        .unwrap();
        assert_eq!(
            wrapper_edited
                .root
                .unique_field_in_base_chain("m_Component")
                .unwrap()
                .unwrap()
                .array_elements()
                .unwrap()[1]
                .unique_field("component")
                .unwrap()
                .unwrap()
                .pptr(),
            Some((3, generated_component.component_asset.path_id))
        );
    }

    #[test]
    fn planned_materialization_uses_only_assigned_source_dependency_file_ids() {
        let game_object = WorkspaceSceneObjectIdentity {
            file_index: 3,
            object_index: 1,
            path_id: 12,
            class_id: Some(1),
        };
        let generated_component = WorkspaceGeneratedComponentAddition {
            game_object,
            component_class_id: 4,
            appended_index: 1,
            component_asset: WorkspaceProcessedAssetIdentity {
                collection_index: 5,
                path_id: 201,
                class_id: 4,
            },
        };
        let overlay = WorkspaceGameObjectComponentListOverlay {
            game_object,
            source_slots: vec![WorkspaceSourceGameObjectComponentSlot {
                source_array_index: 0,
                file_id: 0,
                path_id: 0,
                target: SceneRelationshipTarget::Null,
            }],
            generated_component,
        };
        let source_payload = ParsedObjectPayload {
            object_index: game_object.object_index,
            path_id: game_object.path_id,
            type_name: "GameObject".to_owned(),
            root: SerializedValue::Struct(vec![SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(vec![SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                }]),
            }]),
            consumed_bytes: 12,
            trailing_bytes: Vec::new(),
            allocated_bytes: 32,
        };
        let mut plan = empty_plan();
        plan.source_dependency_appends
            .push(WorkspaceSourceCollectionDependencyAppend {
                source_file_index: game_object.file_index,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index: generated_component.component_asset.collection_index,
                    },
                pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                first_source_owner: game_object,
                target_asset: Some(generated_component.component_asset),
                target_animator_state_machine: None,
                file_id: Some(7),
            });

        let edited = materialize_planned_game_object_component_append(
            &plan,
            &source_payload,
            &overlay,
            None,
            2,
        )
        .unwrap();
        assert_eq!(
            edited
                .root
                .unique_field_in_base_chain("m_Component")
                .unwrap()
                .unwrap()
                .array_elements()
                .unwrap()[1]
                .pptr(),
            Some((7, 201))
        );

        plan.source_dependency_appends[0].file_id = None;
        let deferred = materialize_planned_game_object_component_append(
            &plan,
            &source_payload,
            &overlay,
            None,
            2,
        )
        .unwrap_err();
        assert!(deferred
            .to_string()
            .contains("FileID is deferred or unavailable"));
    }

    #[test]
    fn requires_a_typed_template_for_empty_component_arrays_and_checks_limits() {
        let game_object = WorkspaceSceneObjectIdentity {
            file_index: 1,
            object_index: 0,
            path_id: 22,
            class_id: Some(1),
        };
        let overlay = WorkspaceGameObjectComponentListOverlay {
            game_object,
            source_slots: Vec::new(),
            generated_component: WorkspaceGeneratedComponentAddition {
                game_object,
                component_class_id: 4,
                appended_index: 0,
                component_asset: WorkspaceProcessedAssetIdentity {
                    collection_index: 2,
                    path_id: 1,
                    class_id: 4,
                },
            },
        };
        let payload = ParsedObjectPayload {
            object_index: 0,
            path_id: 22,
            type_name: "GameObject".to_owned(),
            root: SerializedValue::Struct(vec![SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(Vec::new()),
            }]),
            consumed_bytes: 0,
            trailing_bytes: Vec::new(),
            allocated_bytes: 0,
        };
        let missing_template =
            materialize_game_object_component_append(&payload, &overlay, 1, None, 1).unwrap_err();
        assert!(missing_template
            .to_string()
            .contains("TypeTree-derived element template"));

        let too_many = materialize_game_object_component_append(
            &payload,
            &overlay,
            1,
            Some(&SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            }),
            0,
        )
        .unwrap_err();
        assert!(too_many.to_string().contains("exceeding the limit of 0"));
        assert_eq!(
            payload
                .root
                .unique_field_in_base_chain("m_Component")
                .unwrap()
                .unwrap()
                .array_elements()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn materializes_generated_transform_defaults_and_planned_game_object_pointer() {
        let (plan, asset, game_object) = generated_transform_plan();
        let metadata = transform_metadata(true, true);

        let generated = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &metadata,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap();

        assert_eq!(generated.asset, asset);
        assert_eq!(generated.type_index, 0);
        assert_eq!(
            generated
                .root
                .unique_field_in_base_chain("m_GameObject")
                .unwrap()
                .unwrap()
                .pptr(),
            Some((1, game_object.path_id))
        );
        assert_eq!(
            generated.root.unique_field("m_Father").unwrap(),
            Some(&SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            })
        );
        assert_eq!(
            generated.root.unique_field("m_Children").unwrap(),
            Some(&SerializedValue::Array(Vec::new()))
        );
        assert_eq!(
            generated.root.unique_field("m_RootOrder").unwrap(),
            Some(&SerializedValue::I32(0))
        );
        assert_eq!(
            generated
                .root
                .unique_field("m_LocalEulerAnglesHint")
                .unwrap(),
            Some(&vector3_value(0.0, 0.0, 0.0))
        );
        assert_eq!(
            generated.root.unique_field("m_LocalPosition").unwrap(),
            Some(&vector3_value(0.0, 0.0, 0.0))
        );
        assert_eq!(
            generated.root.unique_field("m_LocalRotation").unwrap(),
            Some(&SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "x".to_owned(),
                    value: SerializedValue::F32(0.0),
                },
                SerializedFieldValue {
                    name: "y".to_owned(),
                    value: SerializedValue::F32(0.0),
                },
                SerializedFieldValue {
                    name: "z".to_owned(),
                    value: SerializedValue::F32(0.0),
                },
                SerializedFieldValue {
                    name: "w".to_owned(),
                    value: SerializedValue::F32(1.0),
                },
            ]))
        );
        assert_eq!(
            generated.root.unique_field("m_LocalScale").unwrap(),
            Some(&vector3_value(1.0, 1.0, 1.0))
        );

        let mut expected = Vec::new();
        expected.extend_from_slice(&1i32.to_le_bytes());
        expected.extend_from_slice(&game_object.path_id.to_le_bytes());
        expected.extend_from_slice(&0i32.to_le_bytes());
        expected.extend_from_slice(&0i64.to_le_bytes());
        expected.extend_from_slice(&0i32.to_le_bytes()); // m_Children size
        expected.extend_from_slice(&0i32.to_le_bytes()); // m_RootOrder
        for value in [0.0f32, 0.0, 0.0] {
            // Euler hint
            expected.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0f32, 0.0, 0.0] {
            // Local position
            expected.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0f32, 0.0, 0.0, 1.0] {
            // Local rotation
            expected.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1.0f32, 1.0, 1.0] {
            // Local scale
            expected.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(generated.bytes, expected);
    }

    #[test]
    fn generated_transform_yaml_resolves_its_constructor_pointer_to_a_local_scene_id() {
        let (plan, asset, game_object) = generated_transform_plan();
        let metadata = transform_metadata(true, true);
        let generated = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &metadata,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap();
        let constructor_pointer = plan.constructor_pointers()[0];
        let target_asset = crate::export::scene_collection::ExportAssetIdentity {
            collection:
                crate::export::scene_collection::ExportAssetCollectionIdentity::SerializedFile {
                    file_index: game_object.file_index,
                },
            object_index: game_object.object_index,
            path_id: game_object.path_id,
            class_id: 1,
        };
        let generated_asset = crate::export::scene_collection::ExportAssetIdentity {
            collection: crate::export::scene_collection::ExportAssetCollectionIdentity::
                ProcessedCollection {
                    collection_index: asset.collection_index,
                },
            object_index: 0,
            path_id: asset.path_id,
            class_id: asset.class_id,
        };
        let collection_index = 55;
        let pointer_map = crate::export::pointer_map::ProjectExportPointerMap::new(
            &[
                crate::export::pointer_map::ExportedAssetPointerEntry {
                    asset: target_asset,
                    file_id: game_object.path_id,
                    collection_index,
                },
                crate::export::pointer_map::ExportedAssetPointerEntry {
                    asset: generated_asset,
                    file_id: 123,
                    collection_index,
                },
            ],
            &[
                crate::export::pointer_map::ExportCollectionPointerMetadata {
                    collection_index,
                    guid: [0x55; 16],
                    asset_type: crate::export::pointer_map::UnityProjectAssetType::Serialized,
                },
            ],
            crate::export::pointer_map::ExportPointerMapLimits::default(),
        )
        .unwrap();

        let document =
            crate::export::workspace_yaml::generated_transform_payload_to_unity_yaml_document(
                &generated,
                &metadata,
                generated_asset,
                123,
                constructor_pointer,
                &pointer_map,
                collection_index,
                None,
                crate::export::value_yaml::UnityYamlObjectConversionLimits::default(),
            )
            .unwrap();
        let yaml = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();

        assert!(yaml.contains("--- !u!4 &123"), "{yaml}");
        assert!(
            yaml.contains(&format!(
                "m_GameObject: {{fileID: {}}}",
                game_object.path_id
            )),
            "{yaml}"
        );
        assert!(yaml.contains("m_LocalPosition:"), "{yaml}");
        assert!(yaml.contains("m_LocalScale:"), "{yaml}");
        assert!(!yaml.contains("guid:"), "{yaml}");

        let mut wrong_class_metadata = metadata.clone();
        wrong_class_metadata.types[0]
            .type_tree
            .as_mut()
            .unwrap()
            .nodes[0]
            .type_name = Some("NotTransform".to_owned());
        assert!(matches!(
            crate::export::workspace_yaml::generated_transform_payload_to_unity_yaml_document(
                &generated,
                &wrong_class_metadata,
                generated_asset,
                123,
                constructor_pointer,
                &pointer_map,
                collection_index,
                None,
                crate::export::value_yaml::UnityYamlObjectConversionLimits::default(),
            ),
            Err(crate::export::workspace_yaml::UnityYamlWorkspaceExportError::
                GeneratedTransformInvariant { .. })
        ));

        let mut wrong_pointer = generated.clone();
        *wrong_pointer
            .root
            .unique_field_in_base_chain_mut("m_Father")
            .unwrap()
            .unwrap() = SerializedValue::PPtr {
            file_id: constructor_pointer.file_id,
            path_id: constructor_pointer.path_id,
        };
        wrong_pointer.bytes = metadata
            .encode_object_payload_for_type(
                metadata.objects.len(),
                wrong_pointer.type_index,
                &wrong_pointer.root,
                &[],
                ObjectEncodeLimits::default(),
            )
            .unwrap();
        assert!(matches!(
            crate::export::workspace_yaml::generated_transform_payload_to_unity_yaml_document(
                &wrong_pointer,
                &metadata,
                generated_asset,
                123,
                constructor_pointer,
                &pointer_map,
                collection_index,
                None,
                crate::export::value_yaml::UnityYamlObjectConversionLimits::default(),
            ),
            Err(crate::export::workspace_yaml::UnityYamlWorkspaceExportError::
                GeneratedTransformInvariant { message, .. })
                if message.contains("unexpected non-null Transform PPtr")
        ));
    }

    fn vector3_value(x: f32, y: f32, z: f32) -> SerializedValue {
        SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "x".to_owned(),
                value: SerializedValue::F32(x),
            },
            SerializedFieldValue {
                name: "y".to_owned(),
                value: SerializedValue::F32(y),
            },
            SerializedFieldValue {
                name: "z".to_owned(),
                value: SerializedValue::F32(z),
            },
        ])
    }

    #[test]
    fn generated_transform_accepts_optional_fields_and_requires_transform_vectors() {
        let (plan, asset, _) = generated_transform_plan();
        let without_euler_hint = transform_metadata(false, true);
        let generated = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &without_euler_hint,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap();
        assert!(generated
            .root
            .unique_field("m_LocalEulerAnglesHint")
            .unwrap()
            .is_none());

        // 2019.4's class-4 TPK omits m_RootOrder; the source setter treats it as optional.
        let without_root_order = transform_metadata(false, false);
        let generated = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &without_root_order,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap();
        assert!(generated
            .root
            .unique_field("m_RootOrder")
            .unwrap()
            .is_none());

        // The position, rotation, and scale fields are unconditional factory writes.
        let mut without_scale = without_root_order;
        let nodes = &mut without_scale.types[0].type_tree.as_mut().unwrap().nodes;
        let scale_start = nodes
            .iter()
            .position(|node| node.name.as_deref() == Some("m_LocalScale"))
            .expect("the fixture has m_LocalScale");
        let mut scale_end = scale_start + 1;
        while scale_end < nodes.len() && nodes[scale_end].level > nodes[scale_start].level {
            scale_end += 1;
        }
        nodes.drain(scale_start..scale_end);
        let error = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &without_scale,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("no m_LocalScale field"));
    }

    #[test]
    fn generated_transform_rejects_wrong_type_and_dependency_identity() {
        let (plan, asset, _) = generated_transform_plan();
        let mut wrong_type = transform_metadata(true, true);
        wrong_type.types[0].raw_type_id = 224;
        let wrong_type_error = materialize_planned_generated_transform_payload(
            &plan,
            asset,
            &wrong_type,
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap_err();
        assert!(wrong_type_error.to_string().contains("expected 4"));

        let mut wrong_dependency = plan.clone();
        wrong_dependency.collections[asset.collection_index].dependencies[1].target =
            WorkspaceProcessedDependencyTarget::SerializedFileCollection { file_index: 4 };
        let dependency_error = materialize_planned_generated_transform_payload(
            &wrong_dependency,
            asset,
            &transform_metadata(true, true),
            0,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        )
        .unwrap_err();
        assert!(dependency_error
            .to_string()
            .contains("does not uniquely target its source SerializedFile"));
    }

    #[test]
    fn scene_hierarchy_bundle_name_stays_unset_without_a_serialized_bundle() {
        assert_eq!(
            scene_hierarchy_asset_bundle_name(false, "level0", None),
            WorkspaceSceneHierarchyBundleName::Unset
        );
        // The bundle lookup gates everything: even a resolved asset name is
        // ignored when no SerializedBundle was selected.
        assert_eq!(
            scene_hierarchy_asset_bundle_name(false, "level0", Some("scenes")),
            WorkspaceSceneHierarchyBundleName::Unset
        );
    }

    #[test]
    fn scene_hierarchy_bundle_name_falls_back_to_the_bundle_name() {
        assert_eq!(
            scene_hierarchy_asset_bundle_name(true, "level0", None),
            WorkspaceSceneHierarchyBundleName::BundleName("level0".to_owned())
        );
    }

    #[test]
    fn scene_hierarchy_bundle_name_prefers_the_asset_bundle_asset() {
        assert_eq!(
            scene_hierarchy_asset_bundle_name(true, "level0", Some("scenes")),
            WorkspaceSceneHierarchyBundleName::AssetName("scenes".to_owned())
        );
        // The resolved name wins even when it differs from the bundle name.
        assert_eq!(
            scene_hierarchy_asset_bundle_name(true, "shared", Some("shared")),
            WorkspaceSceneHierarchyBundleName::AssetName("shared".to_owned())
        );
    }

    fn bundle_source(bundle_index: usize, bundle_name: &str) -> WorkspaceSceneBundleSource {
        WorkspaceSceneBundleSource {
            bundle_index,
            bundle_name: bundle_name.to_owned(),
            is_serialized_bundle: true,
        }
    }

    fn asset_fact(
        resolved_name: &str,
        has_streamed_scene_flag: bool,
        is_streamed_scene_asset_bundle: bool,
    ) -> WorkspaceBundleAssetNameFact {
        WorkspaceBundleAssetNameFact {
            resolved_name: resolved_name.to_owned(),
            has_streamed_scene_flag,
            is_streamed_scene_asset_bundle,
        }
    }

    fn asset_bundle_root_with_name(name: &str) -> SerializedValue {
        SerializedValue::Struct(vec![crate::serialized_file::object::SerializedFieldValue {
            name: "m_AssetBundleName_R".to_owned(),
            value: SerializedValue::StringBytes(name.as_bytes().to_vec()),
        }])
    }

    #[test]
    fn scene_bundle_plan_composes_traversal_fetch_order_and_get_asset_bundle_name() {
        // The resolved names below come from the real `get_asset_bundle_name`
        // resolver, not invented strings: an explicit serialized name wins, an
        // empty serialized name falls back to the owning bundle name.
        let resolved_explicit = crate::processing::asset_bundle::get_asset_bundle_name(
            &asset_bundle_root_with_name("scenes"),
            "level0",
        )
        .unwrap();
        assert_eq!(resolved_explicit, "scenes");
        let resolved_fallback = crate::processing::asset_bundle::get_asset_bundle_name(
            &asset_bundle_root_with_name(""),
            "level0",
        )
        .unwrap();
        assert_eq!(resolved_fallback, "level0");

        // Scene 0 has two collections: the first collection belongs to bundle 7
        // and the second to bundle 9. Source `FirstOrDefault` must select bundle 7.
        // Scene 1 has no serialized bundle and stays unset. Scene 2 selects a
        // bundle without assets and falls back to the bundle name.
        let scene_collections = vec![vec![0usize, 1usize], vec![2usize], vec![3usize]];
        let file_bundles = vec![
            Some(bundle_source(7, "level0")),
            Some(bundle_source(9, "other")),
            None,
            Some(bundle_source(11, "empty")),
        ];
        let mut fetch_order = HashMap::new();
        // Bundle 7 holds two assets in FetchAssets() order: the first wins even
        // though the second sorts earlier. Bundle 11 has no entry at all.
        fetch_order.insert(
            7usize,
            vec![
                asset_fact(&resolved_explicit, false, false),
                asset_fact("aaa-first-sorted-but-second", false, false),
            ],
        );
        fetch_order.insert(11usize, Vec::new());
        let assignments = plan_workspace_scene_hierarchy_bundle_names(
            &scene_collections,
            &file_bundles,
            &fetch_order,
            WorkspaceSceneHierarchyBundlePlanLimits::default(),
        )
        .unwrap();
        assert_eq!(assignments.len(), 3);
        assert_eq!(assignments[0].scene_index, 0);
        assert_eq!(assignments[0].effective_asset_bundle_name, None);
        assert_eq!(
            assignments[0].name,
            WorkspaceSceneHierarchyBundleName::AssetName("scenes".to_owned())
        );
        assert_eq!(assignments[0].streamed_scene_assert_warning, None);
        assert_eq!(
            assignments[1].name,
            WorkspaceSceneHierarchyBundleName::Unset
        );
        assert_eq!(assignments[1].effective_asset_bundle_name, None);
        assert_eq!(
            assignments[2].name,
            WorkspaceSceneHierarchyBundleName::BundleName("empty".to_owned())
        );
        assert_eq!(assignments[2].effective_asset_bundle_name, None);
    }

    #[test]
    fn scene_bundle_plan_reports_the_streamed_scene_debug_assert_without_changing_the_name() {
        let scene_collections = vec![vec![0usize]];
        let file_bundles = vec![Some(bundle_source(3, "level0"))];
        let mut fetch_order = HashMap::new();
        // The source Debug.Assert fires when the version carries
        // IsStreamedSceneAssetBundle but the value is false. Release builds
        // continue with the resolved name; Rust records an explicit warning.
        fetch_order.insert(3usize, vec![asset_fact("scenes", true, false)]);
        let assignments = plan_workspace_scene_hierarchy_bundle_names(
            &scene_collections,
            &file_bundles,
            &fetch_order,
            WorkspaceSceneHierarchyBundlePlanLimits::default(),
        )
        .unwrap();
        assert_eq!(assignments.len(), 1);
        assert_eq!(
            assignments[0].name,
            WorkspaceSceneHierarchyBundleName::AssetName("scenes".to_owned())
        );
        let warning = assignments[0]
            .streamed_scene_assert_warning
            .as_ref()
            .expect("assert warning is recorded");
        assert!(warning.contains("IsStreamedSceneAssetBundle=false"));
        assert!(warning.contains("release continues"));

        // A streamed true value and a version without the field stay silent.
        let mut silent = HashMap::new();
        silent.insert(3usize, vec![asset_fact("scenes", true, true)]);
        let assignments = plan_workspace_scene_hierarchy_bundle_names(
            &scene_collections,
            &file_bundles,
            &silent,
            WorkspaceSceneHierarchyBundlePlanLimits::default(),
        )
        .unwrap();
        assert_eq!(assignments[0].streamed_scene_assert_warning, None);
    }

    #[test]
    fn scene_bundle_plan_rejects_invalid_indexes_and_enforces_limits() {
        // Unknown collection file indexes fail instead of selecting a bundle.
        let error = plan_workspace_scene_hierarchy_bundle_names(
            &[vec![5usize]],
            &[Some(bundle_source(1, "level0"))],
            &HashMap::new(),
            WorkspaceSceneHierarchyBundlePlanLimits::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("outside 1 files"));

        // Limits fail closed with explicit resources.
        let error = plan_workspace_scene_hierarchy_bundle_names(
            &[vec![0usize], vec![0usize]],
            &[Some(bundle_source(1, "level0"))],
            &HashMap::new(),
            WorkspaceSceneHierarchyBundlePlanLimits {
                max_scenes: 1,
                ..WorkspaceSceneHierarchyBundlePlanLimits::default()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("scene hierarchy bundle scenes"));
    }
}
