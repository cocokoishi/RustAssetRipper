use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::metadata::TypeTree;
use crate::serialized_file::reference_graph::MultiFileReferenceGraph;

use super::scene::{
    SceneGraph, SceneGraphIssue, SceneObjectRecord, SceneQuaternion, SceneRelationship,
    SceneRelationshipKind, SceneRelationshipTarget, SceneRootReason, SceneVector3,
    WorkspaceSceneHierarchyAudit, WorkspaceSceneRootDiscovery,
};
use super::scene_collections::{
    scene_manager_kind, SceneDefinitionPlan, SceneManagerKind, WorkspaceSceneDefinitions,
};
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

fn resolve_transform_game_object(
    workspace: &WorkspaceSceneHierarchies,
    transform: WorkspaceSceneObjectIdentity,
    walk: &mut WorkspaceGetRootGameObjectWalk,
) -> Result<Option<WorkspaceSceneObjectIdentity>, WorkspaceGetRootGameObjectError> {
    let Some(graph) = workspace.collection_graph_for_file(transform.file_index) else {
        walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingCollectionGraph);
        return Ok(None);
    };
    if !graph.source_graph_complete() || !graph.source_reference_scans_complete() {
        walk.issue = Some(WorkspaceGetRootGameObjectIssue::IncompleteCollectionGraph {
            file_index: transform.file_index,
        });
        return Ok(None);
    }
    let Ok(transform_index) = graph
        .transform_hierarchies
        .binary_search_by_key(&transform.object_index, |hierarchy| {
            hierarchy.transform.object_index
        })
    else {
        walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingTransformHierarchy { transform });
        return Ok(None);
    };
    if graph.transform_hierarchies[transform_index].transform != transform {
        walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingTransformHierarchy { transform });
        return Ok(None);
    }
    let owners = graph
        .transform_game_object_relationships(transform_index)
        .map_err(
            |error| WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                message: error.to_string(),
            },
        )?;
    let mut owners = owners.peekable();
    let Some(owner) = owners.next() else {
        walk.issue =
            Some(WorkspaceGetRootGameObjectIssue::MissingTransformGameObjectField { transform });
        return Ok(None);
    };
    let extra_owner_count = owners.count();
    if extra_owner_count > 0 {
        walk.issue = Some(
            WorkspaceGetRootGameObjectIssue::DuplicateTransformGameObjectFields {
                transform,
                match_count: extra_owner_count + 1,
            },
        );
        return Ok(None);
    }
    if owner.kind != SceneRelationshipKind::ComponentGameObject
        || owner.source_object_index != transform.object_index
        || owner.source_path_id != transform.path_id
    {
        return Err(WorkspaceGetRootGameObjectError::GraphInvariantFailed {
            message: format!(
                "Transform {}:{} owner index points to an unrelated relationship",
                transform.file_index, transform.object_index
            ),
        });
    }
    match resolve_relationship_identity(workspace, graph, owner) {
        RelationshipIdentityResolution::Null => Ok(None),
        RelationshipIdentityResolution::Resolved(game_object)
            if game_object.class_id == Some(GAME_OBJECT_CLASS_ID) =>
        {
            Ok(Some(game_object))
        }
        RelationshipIdentityResolution::Resolved(target) => {
            walk.issue = Some(
                WorkspaceGetRootGameObjectIssue::TransformGameObjectClassMismatch {
                    transform,
                    target,
                },
            );
            Ok(None)
        }
        RelationshipIdentityResolution::Unresolved(target) => {
            walk.issue = Some(
                WorkspaceGetRootGameObjectIssue::UnresolvedTransformGameObject {
                    transform,
                    target,
                },
            );
            Ok(None)
        }
        RelationshipIdentityResolution::Unavailable {
            target_file_index,
            target_object_index,
            path_id,
        } => {
            walk.issue = Some(
                WorkspaceGetRootGameObjectIssue::TransformGameObjectIdentityUnavailable {
                    transform,
                    target_file_index,
                    target_object_index,
                    path_id,
                },
            );
            Ok(None)
        }
        RelationshipIdentityResolution::PathMismatch {
            identity,
            expected_path_id,
        } => {
            walk.issue = Some(
                WorkspaceGetRootGameObjectIssue::TransformGameObjectPathIdMismatch {
                    transform,
                    expected_path_id,
                    actual_path_id: identity.path_id,
                },
            );
            Ok(None)
        }
    }
}

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

impl WorkspaceGeneratedTransformPlanBuilder {
    pub fn new(
        workspace_file_count: usize,
        limits: WorkspaceGeneratedTransformPlanLimits,
    ) -> Result<Self, WorkspaceSceneHierarchyError> {
        if workspace_file_count > limits.max_workspace_files {
            return Err(WorkspaceSceneHierarchyError::WorkspaceFileCountMismatch {
                count: workspace_file_count,
                limit: limits.max_workspace_files,
            });
        }
        let mut file_plan_ranges = Vec::new();
        file_plan_ranges
            .try_reserve_exact(workspace_file_count)
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "workspace missing-Transform source index",
                requested: workspace_file_count,
            })?;
        file_plan_ranges.resize(workspace_file_count, None);
        Ok(Self {
            file_plan_ranges,
            game_objects: Vec::new(),
            limits,
        })
    }

    pub fn contains_file(&self, file_index: usize) -> bool {
        self.file_plan_ranges
            .get(file_index)
            .is_some_and(Option::is_some)
    }

    pub fn add_file(
        &mut self,
        file_index: usize,
        roots: &WorkspaceSceneRootDiscovery,
        references: &MultiFileReferenceGraph<'_>,
    ) -> Result<(), WorkspaceSceneHierarchyError> {
        let Some(existing_range) = self.file_plan_ranges.get(file_index) else {
            return Err(
                WorkspaceSceneHierarchyError::CollectionFileIndexOutOfBounds {
                    file_index,
                    file_count: self.file_plan_ranges.len(),
                },
            );
        };
        if existing_range.is_some() {
            return Err(
                WorkspaceSceneHierarchyError::DuplicateGeneratedTransformSource { file_index },
            );
        }

        let candidates = missing_transform_game_objects(file_index, roots, references)?;
        let next_count = checked_charge(
            self.game_objects.len(),
            candidates.len(),
            self.limits.max_missing_transform_plans,
            "workspace missing-Transform plans",
        )?;
        self.game_objects
            .try_reserve_exact(candidates.len())
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "workspace missing-Transform identities",
                requested: next_count,
            })?;

        let start = self.game_objects.len();
        self.game_objects.extend(candidates);
        self.file_plan_ranges[file_index] = Some((start, next_count));
        Ok(())
    }

    pub fn finish(
        self,
        definitions: &WorkspaceSceneDefinitions,
    ) -> Result<WorkspaceNonSceneTransformPlans, WorkspaceSceneHierarchyError> {
        if definitions.workspace_collection_count() != self.file_plan_ranges.len() {
            return Err(
                WorkspaceSceneHierarchyError::WorkspaceDefinitionCollectionCountMismatch {
                    expected: self.file_plan_ranges.len(),
                    actual: definitions.workspace_collection_count(),
                },
            );
        }

        let plan_count = self
            .game_objects
            .iter()
            .filter(|identity| {
                definitions
                    .scene_index_for_collection(identity.file_index)
                    .is_none()
            })
            .count();
        let mut plans = Vec::new();
        plans.try_reserve_exact(plan_count).map_err(|_| {
            WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "non-scene missing-Transform plans",
                requested: plan_count,
            }
        })?;
        for game_object in self.game_objects.into_iter().filter(|identity| {
            definitions
                .scene_index_for_collection(identity.file_index)
                .is_none()
        }) {
            plans.push(WorkspaceMissingTransformPlan {
                game_object,
                default_values: default_injected_transform_values(),
            });
        }
        Ok(WorkspaceNonSceneTransformPlans { plans })
    }
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

impl WorkspaceSceneCollectionHierarchy {
    pub fn collection_file_index(&self) -> usize {
        self.collection_file_index
    }

    pub fn scene_objects(&self) -> &[SceneObjectRecord] {
        &self.scene_objects
    }

    pub fn game_object_count(&self) -> usize {
        self.scene_objects
            .iter()
            .filter(|object| object.class_id == Some(GAME_OBJECT_CLASS_ID))
            .count()
    }

    pub fn transform_count(&self) -> usize {
        self.scene_objects
            .iter()
            .filter(|object| {
                matches!(
                    object.class_id,
                    Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                )
            })
            .count()
    }

    pub fn relationships(&self) -> &[SceneRelationship] {
        &self.relationships
    }

    pub fn game_object_hierarchies(&self) -> &[WorkspaceSceneGameObjectHierarchy] {
        &self.game_object_hierarchies
    }

    fn prefab_root_relationships_for_object(
        &self,
        object_index: usize,
    ) -> &[WorkspacePrefabRootRelationshipIndexEntry] {
        let start = self
            .prefab_root_relationship_index
            .partition_point(|entry| entry.source_object_index < object_index);
        let end = self
            .prefab_root_relationship_index
            .partition_point(|entry| entry.source_object_index <= object_index);
        &self.prefab_root_relationship_index[start..end]
    }

    /// Return a GameObject's `m_Component` edges in serialized array order.
    pub fn game_object_component_relationships<'a>(
        &'a self,
        hierarchy_index: usize,
    ) -> Result<impl Iterator<Item = &'a SceneRelationship> + 'a, WorkspaceSceneHierarchyError>
    {
        let hierarchy = self.game_object_hierarchies.get(hierarchy_index).ok_or(
            WorkspaceSceneHierarchyError::GameObjectHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count: self.game_object_hierarchies.len(),
            },
        )?;
        let range = self
            .game_object_component_relationship_indices
            .get(hierarchy.component_relationship_start..hierarchy.component_relationship_end)
            .ok_or(
                WorkspaceSceneHierarchyError::InvalidGameObjectComponentRelationshipRange {
                    hierarchy_index,
                    start: hierarchy.component_relationship_start,
                    end: hierarchy.component_relationship_end,
                    index_count: self.game_object_component_relationship_indices.len(),
                },
            )?;
        for &relationship_index in range {
            if self.relationships.get(relationship_index).is_none() {
                return Err(
                    WorkspaceSceneHierarchyError::InvalidGameObjectComponentRelationshipIndex {
                        hierarchy_index,
                        relationship_index,
                        relationship_count: self.relationships.len(),
                    },
                );
            }
        }
        Ok(range
            .iter()
            .map(|&relationship_index| &self.relationships[relationship_index]))
    }

    /// Returns one source GameObject's ordered component-array relationships after identity checks.
    pub fn game_object_component_relationships_for_identity<'a>(
        &'a self,
        game_object: WorkspaceSceneObjectIdentity,
    ) -> Result<impl Iterator<Item = &'a SceneRelationship> + 'a, WorkspaceSceneHierarchyError>
    {
        let hierarchy_index = self.game_object_hierarchy_index(game_object)?;
        self.game_object_component_relationships(hierarchy_index)
    }

    /// Returns the serialized GameObject component-array length for a verified source identity.
    pub fn game_object_component_slot_count(
        &self,
        game_object: WorkspaceSceneObjectIdentity,
    ) -> Result<usize, WorkspaceSceneHierarchyError> {
        Ok(self
            .game_object_component_relationships_for_identity(game_object)?
            .count())
    }

    fn game_object_hierarchy_index(
        &self,
        game_object: WorkspaceSceneObjectIdentity,
    ) -> Result<usize, WorkspaceSceneHierarchyError> {
        if game_object.file_index != self.collection_file_index
            || game_object.class_id != Some(GAME_OBJECT_CLASS_ID)
        {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index: game_object.file_index,
                object_index: game_object.object_index,
                path_id: game_object.path_id,
            });
        }
        let hierarchy_index = self
            .game_object_hierarchies
            .binary_search_by_key(&game_object.object_index, |hierarchy| {
                hierarchy.game_object.object_index
            })
            .map_err(
                |_| WorkspaceSceneHierarchyError::MissingGameObjectHierarchy {
                    file_index: game_object.file_index,
                    object_index: game_object.object_index,
                },
            )?;
        let hierarchy = &self.game_object_hierarchies[hierarchy_index];
        if hierarchy.game_object != game_object {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index: game_object.file_index,
                object_index: game_object.object_index,
                path_id: game_object.path_id,
            });
        }
        Ok(hierarchy_index)
    }

    pub fn transform_hierarchies(&self) -> &[WorkspaceSceneTransformHierarchy] {
        &self.transform_hierarchies
    }

    pub fn transform_parent_relationships<'a>(
        &'a self,
        hierarchy_index: usize,
    ) -> Result<impl Iterator<Item = &'a SceneRelationship> + 'a, WorkspaceSceneHierarchyError>
    {
        let hierarchy = self.transform_hierarchies.get(hierarchy_index).ok_or(
            WorkspaceSceneHierarchyError::TransformHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count: self.transform_hierarchies.len(),
            },
        )?;
        let range = self
            .transform_parent_relationship_indices
            .get(hierarchy.parent_relationship_start..hierarchy.parent_relationship_end)
            .ok_or(
                WorkspaceSceneHierarchyError::InvalidTransformParentRelationshipRange {
                    hierarchy_index,
                    start: hierarchy.parent_relationship_start,
                    end: hierarchy.parent_relationship_end,
                    index_count: self.transform_parent_relationship_indices.len(),
                },
            )?;
        self.validate_transform_relationship_indexes(hierarchy_index, range)?;
        Ok(range
            .iter()
            .map(|&relationship_index| &self.relationships[relationship_index]))
    }

    pub fn transform_child_relationships<'a>(
        &'a self,
        hierarchy_index: usize,
    ) -> Result<impl Iterator<Item = &'a SceneRelationship> + 'a, WorkspaceSceneHierarchyError>
    {
        let hierarchy = self.transform_hierarchies.get(hierarchy_index).ok_or(
            WorkspaceSceneHierarchyError::TransformHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count: self.transform_hierarchies.len(),
            },
        )?;
        let range = self
            .transform_child_relationship_indices
            .get(hierarchy.child_relationship_start..hierarchy.child_relationship_end)
            .ok_or(
                WorkspaceSceneHierarchyError::InvalidTransformChildRelationshipRange {
                    hierarchy_index,
                    start: hierarchy.child_relationship_start,
                    end: hierarchy.child_relationship_end,
                    index_count: self.transform_child_relationship_indices.len(),
                },
            )?;
        self.validate_transform_relationship_indexes(hierarchy_index, range)?;
        Ok(range
            .iter()
            .map(|&relationship_index| &self.relationships[relationship_index]))
    }

    pub fn transform_game_object_relationships<'a>(
        &'a self,
        hierarchy_index: usize,
    ) -> Result<impl Iterator<Item = &'a SceneRelationship> + 'a, WorkspaceSceneHierarchyError>
    {
        let hierarchy = self.transform_hierarchies.get(hierarchy_index).ok_or(
            WorkspaceSceneHierarchyError::TransformHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count: self.transform_hierarchies.len(),
            },
        )?;
        let range = self
            .transform_game_object_relationship_indices
            .get(hierarchy.game_object_relationship_start..hierarchy.game_object_relationship_end)
            .ok_or(
                WorkspaceSceneHierarchyError::InvalidTransformGameObjectRelationshipRange {
                    hierarchy_index,
                    start: hierarchy.game_object_relationship_start,
                    end: hierarchy.game_object_relationship_end,
                    index_count: self.transform_game_object_relationship_indices.len(),
                },
            )?;
        self.validate_transform_relationship_indexes(hierarchy_index, range)?;
        Ok(range
            .iter()
            .map(|&relationship_index| &self.relationships[relationship_index]))
    }

    fn validate_transform_relationship_indexes(
        &self,
        hierarchy_index: usize,
        indexes: &[usize],
    ) -> Result<(), WorkspaceSceneHierarchyError> {
        for &relationship_index in indexes {
            if self.relationships.get(relationship_index).is_none() {
                return Err(
                    WorkspaceSceneHierarchyError::InvalidTransformRelationshipIndex {
                        hierarchy_index,
                        relationship_index,
                        relationship_count: self.relationships.len(),
                    },
                );
            }
        }
        Ok(())
    }

    pub fn graph_issues(&self) -> &[SceneGraphIssue] {
        &self.graph_issues
    }

    pub fn scene_assets(&self) -> &[WorkspaceSceneAssetIdentity] {
        &self.scene_assets
    }

    pub fn has_lighting_data_asset_field(&self, object_index: usize) -> bool {
        self.lighting_data_asset_field_object_indices
            .binary_search(&object_index)
            .is_ok()
    }

    pub fn component_candidates(&self) -> &[WorkspaceSceneComponentCandidate] {
        &self.component_candidates
    }

    pub fn missing_transform_plans(&self) -> &[WorkspaceMissingTransformPlan] {
        &self.missing_transform_plans
    }

    pub fn source_graph_complete(&self) -> bool {
        self.source_graph_complete
    }

    pub fn source_reference_scans_complete(&self) -> bool {
        self.source_reference_scans_complete
    }

    pub fn roots(&self) -> &WorkspaceSceneRootDiscovery {
        &self.roots
    }

    pub fn hierarchy_audit(&self) -> &WorkspaceSceneHierarchyAudit {
        &self.hierarchy_audit
    }

    pub fn is_complete(&self) -> bool {
        self.source_graph_complete
            && self.source_reference_scans_complete
            && self.graph_issues.is_empty()
            && self.roots.is_complete()
            && self.hierarchy_audit.is_complete()
    }
}

/// Recovered scene graph data for one of the source workspace's SceneDefinitions.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSceneHierarchyScene {
    pub plan: SceneDefinitionPlan,
    collections: Vec<WorkspaceSceneCollectionHierarchy>,
}

impl WorkspaceSceneHierarchyScene {
    pub fn collections(&self) -> &[WorkspaceSceneCollectionHierarchy] {
        &self.collections
    }

    /// Reconstructs the category membership exposed by the baseline
    /// `SceneHierarchyObject.Assets`, retaining source collection and object order.
    ///
    /// Components are included when backed by a Transform class ID, a decoded GameObject
    /// `m_Component` reference, or the baseline MonoBehaviour class with a non-null
    /// `m_GameObject` field. A null MonoBehaviour pointer excludes it; other `m_GameObject`-only
    /// candidates remain ambiguous. Callers must not treat partial membership as export-ready.
    pub fn asset_membership(
        &self,
        limits: WorkspaceSceneHierarchyMembershipLimits,
    ) -> Result<WorkspaceSceneHierarchyMembership, WorkspaceSceneHierarchyMembershipError> {
        if self.collections.len() > limits.max_assets {
            return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
                requested: self.collections.len(),
                limit: limits.max_assets,
            });
        }

        let mut collection_ranks = HashMap::new();
        collection_ranks
            .try_reserve(self.collections.len())
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "SceneDefinition collection-order index",
                    requested: self.collections.len(),
                },
            )?;
        for (rank, collection) in self.collections.iter().enumerate() {
            if collection_ranks
                .insert(collection.collection_file_index(), rank)
                .is_some()
            {
                return Err(
                    WorkspaceSceneHierarchyMembershipError::DuplicateCollectionFileIndex {
                        file_index: collection.collection_file_index(),
                    },
                );
            }
        }

        let scene_asset_count = self
            .collections
            .iter()
            .try_fold(0usize, |count, collection| {
                count.checked_add(collection.scene_assets().len())
            })
            .ok_or(WorkspaceSceneHierarchyMembershipError::CountOverflow {
                resource: "SceneDefinition scene assets",
            })?;
        if scene_asset_count > limits.max_assets {
            return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
                requested: scene_asset_count,
                limit: limits.max_assets,
            });
        }
        let component_candidate_count = self
            .collections
            .iter()
            .try_fold(0usize, |count, collection| {
                count.checked_add(collection.component_candidates().len())
            })
            .ok_or(WorkspaceSceneHierarchyMembershipError::CountOverflow {
                resource: "SceneDefinition component candidates",
            })?;
        if component_candidate_count > limits.max_assets {
            return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
                requested: component_candidate_count,
                limit: limits.max_assets,
            });
        }

        let mut membership = WorkspaceSceneHierarchyMembership {
            game_objects: Vec::new(),
            components: Vec::new(),
            prefab_instances: Vec::new(),
            managers: Vec::new(),
            scene_roots: None,
            issues: Vec::new(),
        };
        let mut membership_asset_count = 0usize;
        let mut seen_scene_assets = HashMap::new();
        seen_scene_assets
            .try_reserve(scene_asset_count)
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "SceneDefinition asset identity index",
                    requested: scene_asset_count,
                },
            )?;
        let mut seen_components = HashMap::new();
        seen_components
            .try_reserve(component_candidate_count)
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "SceneDefinition component identity index",
                    requested: component_candidate_count,
                },
            )?;
        let mut weak_components = Vec::new();
        weak_components
            .try_reserve_exact(component_candidate_count)
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "ambiguous SceneDefinition component candidates",
                    requested: component_candidate_count,
                },
            )?;
        let mut seen_weak_components = HashMap::new();
        seen_weak_components
            .try_reserve(component_candidate_count)
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "ambiguous SceneDefinition component identity index",
                    requested: component_candidate_count,
                },
            )?;
        let mut ordered_components = Vec::new();
        ordered_components
            .try_reserve(component_candidate_count)
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "ordered SceneDefinition components",
                    requested: component_candidate_count,
                },
            )?;

        for collection in &self.collections {
            let file_index = collection.collection_file_index();
            if !collection.source_graph_complete() {
                push_membership_issue(
                    &mut membership.issues,
                    WorkspaceSceneHierarchyMembershipIssue::IncompleteCollectionGraph {
                        file_index,
                    },
                    limits.max_issues,
                )?;
            }
            if !collection.source_reference_scans_complete() {
                push_membership_issue(
                    &mut membership.issues,
                    WorkspaceSceneHierarchyMembershipIssue::IncompleteReferenceScans { file_index },
                    limits.max_issues,
                )?;
            }

            for asset in collection.scene_assets() {
                let expected_class_id = match asset.category {
                    WorkspaceSceneAssetCategory::GameObject => Some(GAME_OBJECT_CLASS_ID),
                    WorkspaceSceneAssetCategory::PrefabInstance => Some(PREFAB_INSTANCE_CLASS_ID),
                    WorkspaceSceneAssetCategory::SceneRoots => Some(SCENE_ROOTS_CLASS_ID),
                    WorkspaceSceneAssetCategory::LevelGameManager(kind) => Some(kind.class_id()),
                };
                if asset.identity.file_index != file_index
                    || asset.identity.class_id != expected_class_id
                {
                    push_membership_issue(
                        &mut membership.issues,
                        WorkspaceSceneHierarchyMembershipIssue::InvalidSceneAssetIdentity {
                            identity: asset.identity,
                            collection_file_index: file_index,
                            expected_class_id,
                        },
                        limits.max_issues,
                    )?;
                    continue;
                }
                let asset_key = WorkspaceObjectKey::from(asset.identity);
                if let Some(&existing) = seen_scene_assets.get(&asset_key) {
                    let issue = if existing == asset.identity {
                        WorkspaceSceneHierarchyMembershipIssue::DuplicateSceneAsset {
                            identity: asset.identity,
                        }
                    } else {
                        WorkspaceSceneHierarchyMembershipIssue::ConflictingSceneAssetIdentity {
                            existing,
                            conflicting: asset.identity,
                        }
                    };
                    push_membership_issue(&mut membership.issues, issue, limits.max_issues)?;
                    continue;
                }
                seen_scene_assets.insert(asset_key, asset.identity);
                match asset.category {
                    WorkspaceSceneAssetCategory::GameObject => push_membership_asset(
                        &mut membership_asset_count,
                        &mut membership.game_objects,
                        asset.identity,
                        limits.max_assets,
                    )?,
                    WorkspaceSceneAssetCategory::PrefabInstance => push_membership_asset(
                        &mut membership_asset_count,
                        &mut membership.prefab_instances,
                        asset.identity,
                        limits.max_assets,
                    )?,
                    WorkspaceSceneAssetCategory::LevelGameManager(_) => push_membership_asset(
                        &mut membership_asset_count,
                        &mut membership.managers,
                        asset.identity,
                        limits.max_assets,
                    )?,
                    WorkspaceSceneAssetCategory::SceneRoots => {
                        if membership.scene_roots.is_none() {
                            charge_membership_asset(
                                &mut membership_asset_count,
                                limits.max_assets,
                            )?;
                        }
                        // SceneHierarchyObject.Create overwrites its SceneRoots property for
                        // every matching source asset, so the last one in traversal order wins.
                        membership.scene_roots = Some(asset.identity);
                    }
                }
            }

            for candidate in collection.component_candidates() {
                match candidate.evidence {
                    WorkspaceSceneComponentEvidence::TransformClassId
                    | WorkspaceSceneComponentEvidence::TypeTreeComponentBase
                    | WorkspaceSceneComponentEvidence::GameObjectComponentReference => {
                        let Some(&rank) = collection_ranks.get(&candidate.identity.file_index)
                        else {
                            push_membership_issue(
                                &mut membership.issues,
                                WorkspaceSceneHierarchyMembershipIssue::ComponentOutsideSceneCollections {
                                    identity: candidate.identity,
                                    evidence: candidate.evidence,
                                },
                                limits.max_issues,
                            )?;
                            continue;
                        };
                        let component_key = WorkspaceObjectKey::from(candidate.identity);
                        if let Some(&existing) = seen_components.get(&component_key) {
                            if existing != candidate.identity {
                                push_membership_issue(
                                    &mut membership.issues,
                                    WorkspaceSceneHierarchyMembershipIssue::ConflictingComponentIdentity {
                                        existing,
                                        conflicting: candidate.identity,
                                    },
                                    limits.max_issues,
                                )?;
                            }
                        } else {
                            seen_components.insert(component_key, candidate.identity);
                            ordered_components.try_reserve(1).map_err(|_| {
                                WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                                    resource: "ordered SceneDefinition components",
                                    requested: ordered_components.len().saturating_add(1),
                                }
                            })?;
                            ordered_components.push((rank, candidate.identity));
                        }
                    }
                    WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull
                    | WorkspaceSceneComponentEvidence::GameObjectPointerField => {
                        let weak_key = WorkspaceObjectKey::from(candidate.identity);
                        if let Some(&(existing, existing_evidence)) =
                            seen_weak_components.get(&weak_key)
                        {
                            if existing != candidate.identity {
                                push_membership_issue(
                                    &mut membership.issues,
                                    WorkspaceSceneHierarchyMembershipIssue::ConflictingComponentIdentity {
                                        existing,
                                        conflicting: candidate.identity,
                                    },
                                    limits.max_issues,
                                )?;
                            } else if existing_evidence != candidate.evidence {
                                push_membership_issue(
                                    &mut membership.issues,
                                    WorkspaceSceneHierarchyMembershipIssue::ConflictingComponentGameObjectEvidence {
                                        identity: candidate.identity,
                                        existing: existing_evidence,
                                        conflicting: candidate.evidence,
                                    },
                                    limits.max_issues,
                                )?;
                            }
                        } else {
                            seen_weak_components
                                .insert(weak_key, (candidate.identity, candidate.evidence));
                            weak_components.try_reserve(1).map_err(|_| {
                                WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                                    resource: "ambiguous SceneDefinition component candidates",
                                    requested: weak_components.len().saturating_add(1),
                                }
                            })?;
                            weak_components.push((candidate.identity, candidate.evidence));
                        }
                    }
                    WorkspaceSceneComponentEvidence::AmbiguousTypeTreeBase => {
                        push_membership_issue(
                            &mut membership.issues,
                            WorkspaceSceneHierarchyMembershipIssue::UnclassifiedComponentCandidate {
                                identity: candidate.identity,
                                evidence: candidate.evidence,
                            },
                            limits.max_issues,
                        )?;
                    }
                }
            }
        }

        for (identity, evidence) in weak_components {
            if let Some(&existing) = seen_components.get(&WorkspaceObjectKey::from(identity)) {
                if existing != identity {
                    push_membership_issue(
                        &mut membership.issues,
                        WorkspaceSceneHierarchyMembershipIssue::ConflictingComponentIdentity {
                            existing,
                            conflicting: identity,
                        },
                        limits.max_issues,
                    )?;
                }
                continue;
            }
            if identity.class_id == Some(MONO_BEHAVIOUR_CLASS_ID) {
                if evidence == WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull {
                    continue;
                }
                let Some(&rank) = collection_ranks.get(&identity.file_index) else {
                    push_membership_issue(
                        &mut membership.issues,
                        WorkspaceSceneHierarchyMembershipIssue::ComponentOutsideSceneCollections {
                            identity,
                            evidence,
                        },
                        limits.max_issues,
                    )?;
                    continue;
                };
                seen_components.insert(WorkspaceObjectKey::from(identity), identity);
                ordered_components.try_reserve(1).map_err(|_| {
                    WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                        resource: "ordered SceneDefinition components",
                        requested: ordered_components.len().saturating_add(1),
                    }
                })?;
                ordered_components.push((rank, identity));
                continue;
            }
            push_membership_issue(
                &mut membership.issues,
                WorkspaceSceneHierarchyMembershipIssue::UnclassifiedComponentCandidate {
                    identity,
                    evidence,
                },
                limits.max_issues,
            )?;
        }
        ordered_components.sort_unstable_by_key(|(rank, identity)| (*rank, identity.object_index));
        for (_, identity) in ordered_components {
            if identity.class_id == Some(MONO_BEHAVIOUR_CLASS_ID) {
                match seen_weak_components.get(&WorkspaceObjectKey::from(identity)) {
                    Some(&(
                        weak_identity,
                        WorkspaceSceneComponentEvidence::GameObjectPointerField,
                    )) if weak_identity == identity => {}
                    Some(&(
                        weak_identity,
                        WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull,
                    )) if weak_identity == identity => continue,
                    Some(&(weak_identity, _)) if weak_identity != identity => {
                        push_membership_issue(
                            &mut membership.issues,
                            WorkspaceSceneHierarchyMembershipIssue::ConflictingComponentIdentity {
                                existing: weak_identity,
                                conflicting: identity,
                            },
                            limits.max_issues,
                        )?;
                        continue;
                    }
                    _ => {
                        push_membership_issue(
                            &mut membership.issues,
                            WorkspaceSceneHierarchyMembershipIssue::MonoBehaviourGameObjectReferenceUnavailable {
                                identity,
                            },
                            limits.max_issues,
                        )?;
                        continue;
                    }
                }
            }
            if identity.class_id.is_none() {
                push_membership_issue(
                    &mut membership.issues,
                    WorkspaceSceneHierarchyMembershipIssue::ComponentClassIdUnavailable {
                        identity,
                    },
                    limits.max_issues,
                )?;
            }
            membership_asset_count = membership_asset_count.checked_add(1).ok_or(
                WorkspaceSceneHierarchyMembershipError::CountOverflow {
                    resource: "ordered component membership",
                },
            )?;
            if membership_asset_count > limits.max_assets {
                return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
                    requested: membership_asset_count,
                    limit: limits.max_assets,
                });
            }
            membership.components.try_reserve(1).map_err(|_| {
                WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "SceneDefinition component identities",
                    requested: membership_asset_count,
                }
            })?;
            membership.components.push(identity);
        }
        Ok(membership)
    }

    /// Returns the first LightmapSettings object selected by LightingDataProcessor when it has
    /// the versioned LightingDataAsset field and the scene also contains RenderSettings.
    pub fn lighting_data_dependency_owner(&self) -> Option<WorkspaceSceneObjectIdentity> {
        let mut has_render_settings = false;
        let mut first_lightmap_settings = None;
        for collection in &self.collections {
            for asset in collection.scene_assets() {
                match asset.category {
                    WorkspaceSceneAssetCategory::LevelGameManager(
                        SceneManagerKind::RenderSettings,
                    ) => has_render_settings = true,
                    WorkspaceSceneAssetCategory::LevelGameManager(
                        SceneManagerKind::LightmapSettings,
                    ) if first_lightmap_settings.is_none() => {
                        first_lightmap_settings = Some((
                            asset.identity,
                            collection.has_lighting_data_asset_field(asset.identity.object_index),
                        ));
                    }
                    _ => {}
                }
            }
        }
        if has_render_settings {
            first_lightmap_settings
                .filter(|(_, has_field)| *has_field)
                .map(|(identity, _)| identity)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyMembershipLimits {
    pub max_assets: usize,
    pub max_issues: usize,
}

impl Default for WorkspaceSceneHierarchyMembershipLimits {
    fn default() -> Self {
        Self {
            max_assets: 20_000_000,
            max_issues: 100_000,
        }
    }
}
