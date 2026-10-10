use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::scene::{
    SceneGraph, SceneGraphIssue, SceneGraphIssueKind, SceneObjectRecord, SceneQuaternion,
    SceneRelationship, SceneRelationshipKind, SceneRelationshipTarget, SceneRootReason,
    SceneVector3, WorkspaceSceneHierarchyAudit, WorkspaceSceneRootDiscovery,
};
use super::scene_collections::{SceneDefinitionPlan, SceneManagerKind, WorkspaceSceneDefinitions};
use super::scene_naming::SceneUnityVersion;

const GAME_OBJECT_CLASS_ID: i32 = 1;
const TRANSFORM_CLASS_ID: i32 = 4;
const RECT_TRANSFORM_CLASS_ID: i32 = 224;
const MONO_BEHAVIOUR_CLASS_ID: i32 = 114;
const PREFAB_INSTANCE_CLASS_ID: i32 = 1001;
const LIGHTMAP_SETTINGS_CLASS_ID: i32 = 157;
const SCENE_ROOTS_CLASS_ID: i32 = 1_660_057_539;

/// `LightmapSettings.Has_LightingDataAsset` is present from Unity 5.0 onward in the frozen
/// generated class variants. The property is not part of every release payload TypeTree, so its
/// support must come from the collection version rather than a serialized PPtr occurrence.
pub const fn lightmap_settings_has_lighting_data_asset(version: SceneUnityVersion) -> bool {
    version.major >= 5
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneAssetCategory {
    GameObject,
    LevelGameManager(SceneManagerKind),
    PrefabInstance,
    SceneRoots,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WorkspaceSceneObjectIdentity {
    pub file_index: usize,
    pub object_index: usize,
    pub path_id: i64,
    pub class_id: Option<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneAssetIdentity {
    pub identity: WorkspaceSceneObjectIdentity,
    pub category: WorkspaceSceneAssetCategory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneComponentEvidence {
    TransformClassId,
    TypeTreeComponentBase,
    AmbiguousTypeTreeBase,
    GameObjectComponentReference,
    GameObjectPointerFieldNull,
    GameObjectPointerField,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneComponentCandidate {
    pub identity: WorkspaceSceneObjectIdentity,
    pub evidence: WorkspaceSceneComponentEvidence,
}

/// One source GameObject and the ordered range of its `m_Component` relationship indexes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneGameObjectHierarchy {
    game_object: WorkspaceSceneObjectIdentity,
    component_relationship_start: usize,
    component_relationship_end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkspacePrefabRootRelationshipIndexEntry {
    source_object_index: usize,
    relationship_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspacePrefabInstanceRootResolution {
    /// The decoded object did not expose one well-formed `m_RootGameObject` PPtr field.
    FieldUnavailable,
    Null,
    Resolved(WorkspaceSceneObjectIdentity),
    UnresolvedTarget(SceneRelationshipTarget),
    TargetIdentityUnavailable {
        target_file_index: usize,
        target_object_index: usize,
        path_id: i64,
    },
    TargetIdentityPathMismatch {
        expected_path_id: i64,
        actual_path_id: i64,
    },
    TargetClassMismatch {
        target: WorkspaceSceneObjectIdentity,
        actual_class_id: Option<i32>,
    },
    DuplicateRootReferences {
        match_count: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspacePrefabInstanceRootError {
    NotPrefabInstance {
        identity: WorkspaceSceneObjectIdentity,
    },
    MissingCollectionGraph {
        file_index: usize,
    },
    MissingPrefabInstance {
        identity: WorkspaceSceneObjectIdentity,
    },
    InvalidRelationshipIndex {
        relationship_index: usize,
        relationship_count: usize,
    },
    RelationshipIndexMismatch {
        expected_object_index: usize,
        relationship_index: usize,
        actual_object_index: usize,
        kind: SceneRelationshipKind,
    },
}

impl Display for WorkspacePrefabInstanceRootError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPrefabInstance { identity } => write!(
                f,
                "file {} object {} PathID {} is not a PrefabInstance",
                identity.file_index, identity.object_index, identity.path_id
            ),
            Self::MissingCollectionGraph { file_index } => {
                write!(f, "workspace file {file_index} has no retained hierarchy graph")
            }
            Self::MissingPrefabInstance { identity } => write!(
                f,
                "PrefabInstance file {} object {} PathID {} is absent from its collection graph",
                identity.file_index, identity.object_index, identity.path_id
            ),
            Self::InvalidRelationshipIndex {
                relationship_index,
                relationship_count,
            } => write!(
                f,
                "PrefabInstance root relationship index {relationship_index} is outside {relationship_count} relationships"
            ),
            Self::RelationshipIndexMismatch {
                expected_object_index,
                relationship_index,
                actual_object_index,
                kind,
            } => write!(
                f,
                "PrefabInstance object {expected_object_index} root relationship index {relationship_index} points to object {actual_object_index} ({kind:?})"
            ),
        }
    }
}

impl Error for WorkspacePrefabInstanceRootError {}

impl WorkspaceSceneGameObjectHierarchy {
    pub fn game_object(&self) -> WorkspaceSceneObjectIdentity {
        self.game_object
    }

    pub fn component_relationship_count(&self) -> usize {
        self.component_relationship_end - self.component_relationship_start
    }
}

/// One source Transform and ordered indexes into its `m_Father`, `m_Children`, and `m_GameObject` PPtrs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneTransformHierarchy {
    transform: WorkspaceSceneObjectIdentity,
    parent_relationship_start: usize,
    parent_relationship_end: usize,
    child_relationship_start: usize,
    child_relationship_end: usize,
    game_object_relationship_start: usize,
    game_object_relationship_end: usize,
}

struct WorkspaceSceneTransformHierarchyIndex {
    transforms: Vec<WorkspaceSceneTransformHierarchy>,
    parent_relationship_indices: Vec<usize>,
    child_relationship_indices: Vec<usize>,
    game_object_relationship_indices: Vec<usize>,
}

impl WorkspaceSceneTransformHierarchy {
    pub fn transform(&self) -> WorkspaceSceneObjectIdentity {
        self.transform
    }

    pub fn child_relationship_count(&self) -> usize {
        self.child_relationship_end - self.child_relationship_start
    }

    pub fn parent_relationship_count(&self) -> usize {
        self.parent_relationship_end - self.parent_relationship_start
    }

    pub fn game_object_relationship_count(&self) -> usize {
        self.game_object_relationship_end - self.game_object_relationship_start
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceGameObjectHierarchyWalkLimits {
    pub max_depth: usize,
    pub max_entries: usize,
    pub max_issues: usize,
    pub max_pending_actions: usize,
}

impl Default for WorkspaceGameObjectHierarchyWalkLimits {
    fn default() -> Self {
        Self {
            max_depth: 100_000,
            max_entries: 20_000_000,
            max_issues: 100_000,
            max_pending_actions: 5_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceGameObjectHierarchyEntryKind {
    GameObject,
    Component,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceGameObjectHierarchyReferenceKind {
    Component,
    ChildTransform,
    TransformGameObject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceGameObjectHierarchyEntry {
    pub identity: WorkspaceSceneObjectIdentity,
    pub kind: WorkspaceGameObjectHierarchyEntryKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceGameObjectHierarchyIssue {
    IncompleteCollectionGraph {
        file_index: usize,
        source_graph_complete: bool,
        reference_scans_complete: bool,
        root_discovery_complete: bool,
        transform_audit_complete: bool,
        graph_issue_count: usize,
        root_issue_count: usize,
        transform_audit_issue_count: usize,
    },
    MissingCollectionGraph {
        identity: WorkspaceSceneObjectIdentity,
    },
    MissingGameObjectHierarchy {
        identity: WorkspaceSceneObjectIdentity,
    },
    UnresolvedComponentReference {
        game_object: WorkspaceSceneObjectIdentity,
        relationship_index: usize,
        target: SceneRelationshipTarget,
    },
    ComponentIdentityUnavailable {
        game_object: WorkspaceSceneObjectIdentity,
        relationship_index: usize,
        target_file_index: usize,
        target_object_index: usize,
        path_id: i64,
    },
    TargetIdentityPathMismatch {
        source: WorkspaceSceneObjectIdentity,
        relationship_index: usize,
        reference_kind: WorkspaceGameObjectHierarchyReferenceKind,
        expected_path_id: i64,
        actual_path_id: i64,
    },
    UnknownComponentClass {
        game_object: WorkspaceSceneObjectIdentity,
        relationship_index: usize,
        component: WorkspaceSceneObjectIdentity,
    },
    UnresolvedChildTransformReference {
        transform: WorkspaceSceneObjectIdentity,
        relationship_index: usize,
        target: SceneRelationshipTarget,
    },
    ChildTransformIdentityUnavailable {
        transform: WorkspaceSceneObjectIdentity,
        relationship_index: usize,
        target_file_index: usize,
        target_object_index: usize,
        path_id: i64,
    },
    ChildTargetIsNotTransform {
        transform: WorkspaceSceneObjectIdentity,
        relationship_index: usize,
        child: WorkspaceSceneObjectIdentity,
    },
    MissingTransformHierarchy {
        transform: WorkspaceSceneObjectIdentity,
    },
    MissingTransformGameObjectReference {
        transform: WorkspaceSceneObjectIdentity,
    },
    DuplicateTransformGameObjectReferences {
        transform: WorkspaceSceneObjectIdentity,
        match_count: usize,
    },
    UnresolvedTransformGameObjectReference {
        transform: WorkspaceSceneObjectIdentity,
        relationship_index: usize,
        target: SceneRelationshipTarget,
    },
    GameObjectOwnerIdentityUnavailable {
        transform: WorkspaceSceneObjectIdentity,
        relationship_index: usize,
        target_file_index: usize,
        target_object_index: usize,
        path_id: i64,
    },
    TransformOwnerIsNotGameObject {
        transform: WorkspaceSceneObjectIdentity,
        relationship_index: usize,
        target: WorkspaceSceneObjectIdentity,
    },
    CycleDetected {
        game_object: WorkspaceSceneObjectIdentity,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceGameObjectHierarchyWalk {
    root: WorkspaceSceneObjectIdentity,
    entries: Vec<WorkspaceGameObjectHierarchyEntry>,
    issues: Vec<WorkspaceGameObjectHierarchyIssue>,
}

impl WorkspaceGameObjectHierarchyWalk {
    pub fn root(&self) -> WorkspaceSceneObjectIdentity {
        self.root
    }

    pub fn entries(&self) -> &[WorkspaceGameObjectHierarchyEntry] {
        &self.entries
    }

    pub fn issues(&self) -> &[WorkspaceGameObjectHierarchyIssue] {
        &self.issues
    }

    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceGetRootGameObjectLimits {
    pub max_parent_depth: usize,
}

impl Default for WorkspaceGetRootGameObjectLimits {
    fn default() -> Self {
        Self {
            max_parent_depth: 100_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceGetRootGameObjectTermination {
    NoTransformComponent,
    NullFather,
    ParentHasNullGameObject,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceGetRootGameObjectIssue {
    IncompleteCollectionGraph {
        file_index: usize,
    },
    MissingCollectionGraph,
    MissingGameObjectHierarchy,
    UnresolvedComponentReference {
        relationship_index: usize,
        target: SceneRelationshipTarget,
    },
    ComponentIdentityUnavailable {
        relationship_index: usize,
        target_file_index: usize,
        target_object_index: usize,
        path_id: i64,
    },
    ComponentPathIdMismatch {
        relationship_index: usize,
        expected_path_id: i64,
        actual_path_id: i64,
    },
    UnknownComponentClass {
        relationship_index: usize,
        component: WorkspaceSceneObjectIdentity,
    },
    MissingTransformHierarchy {
        transform: WorkspaceSceneObjectIdentity,
    },
    MissingTransformParentField {
        transform: WorkspaceSceneObjectIdentity,
    },
    DuplicateTransformParentFields {
        transform: WorkspaceSceneObjectIdentity,
        match_count: usize,
    },
    UnresolvedTransformParent {
        transform: WorkspaceSceneObjectIdentity,
        target: SceneRelationshipTarget,
    },
    TransformParentIdentityUnavailable {
        transform: WorkspaceSceneObjectIdentity,
        target_file_index: usize,
        target_object_index: usize,
        path_id: i64,
    },
    TransformParentPathIdMismatch {
        transform: WorkspaceSceneObjectIdentity,
        expected_path_id: i64,
        actual_path_id: i64,
    },
    TransformParentClassMismatch {
        target: WorkspaceSceneObjectIdentity,
    },
    MissingTransformGameObjectField {
        transform: WorkspaceSceneObjectIdentity,
    },
    DuplicateTransformGameObjectFields {
        transform: WorkspaceSceneObjectIdentity,
        match_count: usize,
    },
    UnresolvedTransformGameObject {
        transform: WorkspaceSceneObjectIdentity,
        target: SceneRelationshipTarget,
    },
    TransformGameObjectIdentityUnavailable {
        transform: WorkspaceSceneObjectIdentity,
        target_file_index: usize,
        target_object_index: usize,
        path_id: i64,
    },
    TransformGameObjectPathIdMismatch {
        transform: WorkspaceSceneObjectIdentity,
        expected_path_id: i64,
        actual_path_id: i64,
    },
    TransformGameObjectClassMismatch {
        transform: WorkspaceSceneObjectIdentity,
        target: WorkspaceSceneObjectIdentity,
    },
    TransformGameObjectNull {
        transform: WorkspaceSceneObjectIdentity,
    },
    CycleDetected {
        transform: WorkspaceSceneObjectIdentity,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceGetRootGameObjectWalk {
    pub source_game_object: WorkspaceSceneObjectIdentity,
    pub first_transform: Option<WorkspaceSceneObjectIdentity>,
    pub transform_chain: Vec<WorkspaceSceneObjectIdentity>,
    pub root_game_object: Option<WorkspaceSceneObjectIdentity>,
    pub termination: Option<WorkspaceGetRootGameObjectTermination>,
    pub issue: Option<WorkspaceGetRootGameObjectIssue>,
}

impl WorkspaceGetRootGameObjectWalk {
    pub fn is_complete(&self) -> bool {
        self.issue.is_none() && self.root_game_object.is_some() && self.termination.is_some()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceGetRootGameObjectError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    ParentDepthLimitExceeded {
        requested: usize,
        limit: usize,
    },
    GraphInvariantFailed {
        message: String,
    },
}

impl Display for WorkspaceGetRootGameObjectError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::CountOverflow { resource } => write!(f, "{resource} count overflows usize"),
            Self::ParentDepthLimitExceeded { requested, limit } => {
                write!(
                    f,
                    "Transform parent depth {requested} exceeds limit {limit}"
                )
            }
            Self::GraphInvariantFailed { message } => {
                write!(f, "scene graph invariant failed: {message}")
            }
        }
    }
}

impl Error for WorkspaceGetRootGameObjectError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkspaceGeneratedTransformDefaults {
    pub local_position: SceneVector3,
    pub local_rotation: SceneQuaternion,
    pub local_scale: SceneVector3,
    pub root_order: i32,
    /// Applied only when the selected Unity Transform schema has this optional field.
    pub local_euler_angles_hint_if_present: SceneVector3,
}

/// Source-derived default Transform values planned for a scene GameObject with no Transform.
///
/// This records the input-side processing action. It does not allocate a Unity object or PathID.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceMissingTransformPlan {
    pub game_object: WorkspaceSceneObjectIdentity,
    pub default_values: WorkspaceGeneratedTransformDefaults,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceGeneratedTransformPlanLimits {
    pub max_workspace_files: usize,
    pub max_missing_transform_plans: usize,
}

impl Default for WorkspaceGeneratedTransformPlanLimits {
    fn default() -> Self {
        Self {
            max_workspace_files: 1_000_000,
            max_missing_transform_plans: 20_000_000,
        }
    }
}

/// Missing-Transform plans routed to AssetRipper's non-scene "Missing Prefab Transforms" collection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceNonSceneTransformPlans {
    plans: Vec<WorkspaceMissingTransformPlan>,
}

impl WorkspaceNonSceneTransformPlans {
    pub fn plans(&self) -> &[WorkspaceMissingTransformPlan] {
        &self.plans
    }
}

/// Bounded workspace collection of missing-Transform identities before scene ownership is known.
pub struct WorkspaceGeneratedTransformPlanBuilder {
    file_plan_ranges: Vec<Option<(usize, usize)>>,
    game_objects: Vec<WorkspaceSceneObjectIdentity>,
    limits: WorkspaceGeneratedTransformPlanLimits,
}

/// Aggregate limits for SceneDefinition-owned recovered scene graphs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyLimits {
    pub max_workspace_files: usize,
    pub max_scene_definitions: usize,
    pub max_workspace_collection_graphs: usize,
    pub max_scene_objects: usize,
    pub max_scene_asset_records: usize,
    pub max_relationships: usize,
    pub max_issues: usize,
    pub max_total_name_bytes: usize,
    pub max_total_diagnostic_bytes: usize,
}

impl Default for WorkspaceSceneHierarchyLimits {
    fn default() -> Self {
        Self {
            max_workspace_files: 1_000_000,
            max_scene_definitions: 1_000_000,
            max_workspace_collection_graphs: 1_000_000,
            max_scene_objects: 20_000_000,
            max_scene_asset_records: 20_000_000,
            max_relationships: 5_000_000,
            max_issues: 500_000,
            max_total_name_bytes: 256 * 1024 * 1024,
            max_total_diagnostic_bytes: 256 * 1024 * 1024,
        }
    }
}

/// A recovered hierarchy graph attached to one loaded SerializedFile collection.
///
/// This retains the scene parser's known GameObject/Transform objects, every recognized scene
/// PPtr, root discovery, and hierarchy audit diagnostics. It does not claim that unrecognized
/// component types have been materialized as generated Unity objects.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSceneCollectionHierarchy {
    collection_file_index: usize,
    scene_objects: Vec<SceneObjectRecord>,
    relationships: Vec<SceneRelationship>,
    game_object_hierarchies: Vec<WorkspaceSceneGameObjectHierarchy>,
    game_object_component_relationship_indices: Vec<usize>,
    prefab_root_relationship_index: Vec<WorkspacePrefabRootRelationshipIndexEntry>,
    transform_hierarchies: Vec<WorkspaceSceneTransformHierarchy>,
    transform_parent_relationship_indices: Vec<usize>,
    transform_child_relationship_indices: Vec<usize>,
    transform_game_object_relationship_indices: Vec<usize>,
    graph_issues: Vec<SceneGraphIssue>,
    scene_assets: Vec<WorkspaceSceneAssetIdentity>,
    lighting_data_asset_field_object_indices: Vec<usize>,
    component_candidates: Vec<WorkspaceSceneComponentCandidate>,
    /// Missing Transform plans are retained for non-scene graphs so PrefabProcessor can route
    /// them into the workspace-wide `Missing Prefab Transforms` collection.
    missing_transform_plans: Vec<WorkspaceMissingTransformPlan>,
    source_graph_complete: bool,
    source_reference_scans_complete: bool,
    roots: WorkspaceSceneRootDiscovery,
    hierarchy_audit: WorkspaceSceneHierarchyAudit,
}

/// Recovered scene graph data for one of the source workspace's SceneDefinitions.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSceneHierarchyScene {
    pub plan: SceneDefinitionPlan,
    collections: Vec<WorkspaceSceneCollectionHierarchy>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkspaceSceneCollectionGraphIndexEntry {
    file_index: usize,
    scene_index: Option<usize>,
    collection_index: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceSceneHierarchies {
    scenes: Vec<WorkspaceSceneHierarchyScene>,
    non_scene_collections: Vec<WorkspaceSceneCollectionHierarchy>,
    collection_graph_index: Vec<WorkspaceSceneCollectionGraphIndexEntry>,
}
mod asset_classification;
mod builder;
mod collection;
mod collection_preparation;
mod collection_snapshot;
mod collection_validation;
mod errors;
mod hierarchy_walk;
mod membership;
mod prefab_roots;
mod reference_mapping;
mod relationship_indexes;
mod resource_limits;
mod transform_plan;
mod workspace_graph;

pub use self::builder::WorkspaceSceneHierarchyBuilder;
pub use self::errors::WorkspaceSceneHierarchyError;
pub use self::hierarchy_walk::WorkspaceGameObjectHierarchyWalkError;
pub use self::membership::{
    WorkspaceSceneHierarchyMembership, WorkspaceSceneHierarchyMembershipError,
    WorkspaceSceneHierarchyMembershipIssue, WorkspaceSceneHierarchyMembershipLimits,
};
pub use self::transform_plan::default_workspace_generated_transform_values;

#[cfg(test)]
#[path = "scene_hierarchy/tests/mod.rs"]
mod tests;
