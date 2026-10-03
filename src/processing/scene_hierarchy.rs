//! SceneDefinition-owned workspace views of recovered GameObject and Transform data.

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

const GAME_OBJECT_CLASS_ID: i32 = 1;
const TRANSFORM_CLASS_ID: i32 = 4;
const RECT_TRANSFORM_CLASS_ID: i32 = 224;
const MONO_BEHAVIOUR_CLASS_ID: i32 = 114;
const PREFAB_INSTANCE_CLASS_ID: i32 = 1001;
const LIGHTMAP_SETTINGS_CLASS_ID: i32 = 157;
const SCENE_ROOTS_CLASS_ID: i32 = 1_660_057_539;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyMembershipIssue {
    IncompleteCollectionGraph {
        file_index: usize,
    },
    IncompleteReferenceScans {
        file_index: usize,
    },
    InvalidSceneAssetIdentity {
        identity: WorkspaceSceneObjectIdentity,
        collection_file_index: usize,
        expected_class_id: Option<i32>,
    },
    DuplicateSceneAsset {
        identity: WorkspaceSceneObjectIdentity,
    },
    ConflictingSceneAssetIdentity {
        existing: WorkspaceSceneObjectIdentity,
        conflicting: WorkspaceSceneObjectIdentity,
    },
    ConflictingComponentIdentity {
        existing: WorkspaceSceneObjectIdentity,
        conflicting: WorkspaceSceneObjectIdentity,
    },
    ConflictingComponentGameObjectEvidence {
        identity: WorkspaceSceneObjectIdentity,
        existing: WorkspaceSceneComponentEvidence,
        conflicting: WorkspaceSceneComponentEvidence,
    },
    ComponentOutsideSceneCollections {
        identity: WorkspaceSceneObjectIdentity,
        evidence: WorkspaceSceneComponentEvidence,
    },
    ComponentClassIdUnavailable {
        identity: WorkspaceSceneObjectIdentity,
    },
    MonoBehaviourGameObjectReferenceUnavailable {
        identity: WorkspaceSceneObjectIdentity,
    },
    UnclassifiedComponentCandidate {
        identity: WorkspaceSceneObjectIdentity,
        evidence: WorkspaceSceneComponentEvidence,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyMembership {
    game_objects: Vec<WorkspaceSceneObjectIdentity>,
    components: Vec<WorkspaceSceneObjectIdentity>,
    prefab_instances: Vec<WorkspaceSceneObjectIdentity>,
    managers: Vec<WorkspaceSceneObjectIdentity>,
    scene_roots: Option<WorkspaceSceneObjectIdentity>,
    issues: Vec<WorkspaceSceneHierarchyMembershipIssue>,
}

impl WorkspaceSceneHierarchyMembership {
    pub fn game_objects(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.game_objects
    }

    pub fn components(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.components
    }

    pub fn prefab_instances(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.prefab_instances
    }

    pub fn managers(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.managers
    }

    pub fn scene_roots(&self) -> Option<WorkspaceSceneObjectIdentity> {
        self.scene_roots
    }

    pub fn issues(&self) -> &[WorkspaceSceneHierarchyMembershipIssue] {
        &self.issues
    }

    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyMembershipError {
    AssetLimitExceeded {
        requested: usize,
        limit: usize,
    },
    IssueLimitExceeded {
        requested: usize,
        limit: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    DuplicateCollectionFileIndex {
        file_index: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for WorkspaceSceneHierarchyMembershipError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetLimitExceeded { requested, limit } => write!(
                f,
                "Scene hierarchy membership needs {requested} assets/candidates, exceeding limit {limit}"
            ),
            Self::IssueLimitExceeded { requested, limit } => write!(
                f,
                "Scene hierarchy membership has {requested} issues, exceeding limit {limit}"
            ),
            Self::CountOverflow { resource } => {
                write!(f, "Scene hierarchy membership count overflowed for {resource}")
            }
            Self::DuplicateCollectionFileIndex { file_index } => write!(
                f,
                "SceneDefinition repeats workspace collection file index {file_index}"
            ),
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
        }
    }
}

impl Error for WorkspaceSceneHierarchyMembershipError {}

fn charge_membership_asset(
    count: &mut usize,
    limit: usize,
) -> Result<(), WorkspaceSceneHierarchyMembershipError> {
    let requested =
        count
            .checked_add(1)
            .ok_or(WorkspaceSceneHierarchyMembershipError::CountOverflow {
                resource: "ordered asset membership",
            })?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
            requested,
            limit,
        });
    }
    *count = requested;
    Ok(())
}

fn push_membership_asset(
    count: &mut usize,
    assets: &mut Vec<WorkspaceSceneObjectIdentity>,
    identity: WorkspaceSceneObjectIdentity,
    limit: usize,
) -> Result<(), WorkspaceSceneHierarchyMembershipError> {
    let requested =
        count
            .checked_add(1)
            .ok_or(WorkspaceSceneHierarchyMembershipError::CountOverflow {
                resource: "ordered asset membership",
            })?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
            requested,
            limit,
        });
    }
    assets.try_reserve(1).map_err(|_| {
        WorkspaceSceneHierarchyMembershipError::AllocationFailed {
            resource: "ordered Scene hierarchy category",
            requested,
        }
    })?;
    assets.push(identity);
    *count = requested;
    Ok(())
}

fn push_membership_issue(
    issues: &mut Vec<WorkspaceSceneHierarchyMembershipIssue>,
    issue: WorkspaceSceneHierarchyMembershipIssue,
    limit: usize,
) -> Result<(), WorkspaceSceneHierarchyMembershipError> {
    let requested = issues.len().checked_add(1).ok_or(
        WorkspaceSceneHierarchyMembershipError::CountOverflow {
            resource: "membership diagnostics",
        },
    )?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyMembershipError::IssueLimitExceeded {
            requested,
            limit,
        });
    }
    issues.try_reserve(1).map_err(|_| {
        WorkspaceSceneHierarchyMembershipError::AllocationFailed {
            resource: "Scene hierarchy membership diagnostics",
            requested,
        }
    })?;
    issues.push(issue);
    Ok(())
}

/// Scene graphs associated with source collection ownership, in SceneDefinition order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceSceneHierarchies {
    scenes: Vec<WorkspaceSceneHierarchyScene>,
    non_scene_collections: Vec<WorkspaceSceneCollectionHierarchy>,
    collection_graph_index: Vec<WorkspaceSceneCollectionGraphIndexEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkspaceSceneCollectionGraphIndexEntry {
    file_index: usize,
    scene_index: Option<usize>,
    collection_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct WorkspaceObjectKey {
    file_index: usize,
    object_index: usize,
}

impl From<WorkspaceSceneObjectIdentity> for WorkspaceObjectKey {
    fn from(identity: WorkspaceSceneObjectIdentity) -> Self {
        Self {
            file_index: identity.file_index,
            object_index: identity.object_index,
        }
    }
}

enum WorkspaceHierarchyWalkAction {
    Enter {
        game_object: WorkspaceSceneObjectIdentity,
        depth: usize,
    },
    Exit(WorkspaceObjectKey),
}

impl WorkspaceSceneHierarchies {
    pub fn scenes(&self) -> &[WorkspaceSceneHierarchyScene] {
        &self.scenes
    }

    pub fn non_scene_collections(&self) -> &[WorkspaceSceneCollectionHierarchy] {
        &self.non_scene_collections
    }

    fn collection_graph_for_file(
        &self,
        file_index: usize,
    ) -> Option<&WorkspaceSceneCollectionHierarchy> {
        let index = self
            .collection_graph_index
            .binary_search_by_key(&file_index, |entry| entry.file_index)
            .ok()?;
        let entry = self.collection_graph_index.get(index)?;
        match entry.scene_index {
            Some(scene_index) => self
                .scenes
                .get(scene_index)?
                .collections()
                .get(entry.collection_index),
            None => self.non_scene_collections.get(entry.collection_index),
        }
    }

    /// Walks the source `GameObjectExtensions.FetchHierarchy` order across loaded collection files.
    pub fn fetch_game_object_hierarchy(
        &self,
        root: WorkspaceSceneObjectIdentity,
        limits: WorkspaceGameObjectHierarchyWalkLimits,
    ) -> Result<WorkspaceGameObjectHierarchyWalk, WorkspaceGameObjectHierarchyWalkError> {
        if root.class_id != Some(GAME_OBJECT_CLASS_ID) {
            return Err(WorkspaceGameObjectHierarchyWalkError::RootIsNotGameObject {
                identity: root,
            });
        }

        let mut walk = WorkspaceGameObjectHierarchyWalk {
            root,
            entries: Vec::new(),
            issues: Vec::new(),
        };
        let mut actions = Vec::new();
        if limits.max_pending_actions == 0 {
            return Err(
                WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                    resource: "pending hierarchy actions",
                    requested: 1,
                    limit: limits.max_pending_actions,
                },
            );
        }
        actions.try_reserve_exact(1).map_err(|_| {
            WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
                resource: "GameObject hierarchy walk actions",
                requested: 1,
            }
        })?;
        actions.push(WorkspaceHierarchyWalkAction::Enter {
            game_object: root,
            depth: 0,
        });
        let mut active = HashSet::<WorkspaceObjectKey>::new();
        let mut visited_graphs = HashSet::<usize>::new();

        while let Some(action) = actions.pop() {
            match action {
                WorkspaceHierarchyWalkAction::Exit(key) => {
                    if !active.remove(&key) {
                        return Err(
                            WorkspaceGameObjectHierarchyWalkError::GraphInvariantFailed {
                                message: format!(
                                    "GameObject {}:{} was exited without an active entry",
                                    key.file_index, key.object_index
                                ),
                            },
                        );
                    }
                }
                WorkspaceHierarchyWalkAction::Enter { game_object, depth } => {
                    if depth > limits.max_depth {
                        return Err(
                            WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                                resource: "GameObject hierarchy depth",
                                requested: depth,
                                limit: limits.max_depth,
                            },
                        );
                    }
                    let key = WorkspaceObjectKey::from(game_object);
                    if active.contains(&key) {
                        push_hierarchy_walk_issue(
                            &mut walk,
                            WorkspaceGameObjectHierarchyIssue::CycleDetected { game_object },
                            limits,
                        )?;
                        continue;
                    }
                    let Some(graph) = self.collection_graph_for_file(game_object.file_index) else {
                        push_hierarchy_walk_issue(
                            &mut walk,
                            WorkspaceGameObjectHierarchyIssue::MissingCollectionGraph {
                                identity: game_object,
                            },
                            limits,
                        )?;
                        continue;
                    };
                    note_incomplete_collection_graph(
                        graph,
                        &mut visited_graphs,
                        &mut walk,
                        limits,
                    )?;
                    let Ok(hierarchy_index) = graph
                        .game_object_hierarchies
                        .binary_search_by_key(&game_object.object_index, |hierarchy| {
                            hierarchy.game_object.object_index
                        })
                    else {
                        push_hierarchy_walk_issue(
                            &mut walk,
                            WorkspaceGameObjectHierarchyIssue::MissingGameObjectHierarchy {
                                identity: game_object,
                            },
                            limits,
                        )?;
                        continue;
                    };
                    if graph.game_object_hierarchies[hierarchy_index].game_object != game_object {
                        push_hierarchy_walk_issue(
                            &mut walk,
                            WorkspaceGameObjectHierarchyIssue::MissingGameObjectHierarchy {
                                identity: game_object,
                            },
                            limits,
                        )?;
                        continue;
                    }
                    let requested_active = active.len().checked_add(1).ok_or(
                        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
                            resource: "active GameObject hierarchy path",
                        },
                    )?;
                    active.try_reserve(1).map_err(|_| {
                        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
                            resource: "active GameObject hierarchy path",
                            requested: requested_active,
                        }
                    })?;
                    active.insert(key);
                    push_hierarchy_walk_entry(
                        &mut walk,
                        WorkspaceGameObjectHierarchyEntry {
                            identity: game_object,
                            kind: WorkspaceGameObjectHierarchyEntryKind::GameObject,
                        },
                        limits,
                    )?;
                    let selected_transform = expand_game_object_components(
                        self,
                        graph,
                        game_object,
                        hierarchy_index,
                        &mut walk,
                        limits,
                    )?;
                    let children = if let Some(transform) = selected_transform {
                        collect_hierarchy_children(
                            self,
                            transform,
                            &mut walk,
                            &mut visited_graphs,
                            limits,
                        )?
                    } else {
                        Vec::new()
                    };

                    let next_action_count = actions
                        .len()
                        .checked_add(children.len())
                        .and_then(|count| count.checked_add(1))
                        .ok_or(WorkspaceGameObjectHierarchyWalkError::CountOverflow {
                            resource: "pending hierarchy actions",
                        })?;
                    if next_action_count > limits.max_pending_actions {
                        return Err(
                            WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                                resource: "pending hierarchy actions",
                                requested: next_action_count,
                                limit: limits.max_pending_actions,
                            },
                        );
                    }
                    let additional_actions = children.len().checked_add(1).ok_or(
                        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
                            resource: "pending hierarchy actions",
                        },
                    )?;
                    actions.try_reserve(additional_actions).map_err(|_| {
                        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
                            resource: "GameObject hierarchy walk actions",
                            requested: next_action_count,
                        }
                    })?;
                    actions.push(WorkspaceHierarchyWalkAction::Exit(key));
                    let child_depth = depth.checked_add(1).ok_or(
                        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
                            resource: "GameObject hierarchy depth",
                        },
                    )?;
                    for child in children.into_iter().rev() {
                        actions.push(WorkspaceHierarchyWalkAction::Enter {
                            game_object: child,
                            depth: child_depth,
                        });
                    }
                }
            }
        }
        Ok(walk)
    }

    /// Returns PrefabInstance identities in workspace file-index and serialized object order.
    pub fn prefab_instance_identities(
        &self,
    ) -> impl Iterator<Item = WorkspaceSceneObjectIdentity> + '_ {
        self.collection_graph_index.iter().flat_map(move |entry| {
            self.collection_graph_for_file(entry.file_index)
                .into_iter()
                .flat_map(|graph| graph.scene_assets.iter())
                .filter_map(|asset| {
                    (asset.category == WorkspaceSceneAssetCategory::PrefabInstance)
                        .then_some(asset.identity)
                })
        })
    }

    /// Returns recognized scene asset identities in caller-supplied collection traversal order.
    /// Assets inside each collection retain serialized object-table order.
    pub fn scene_asset_identities_in_collection_order<'a>(
        &'a self,
        collection_file_indices: &'a [usize],
    ) -> impl Iterator<Item = WorkspaceSceneAssetIdentity> + 'a {
        collection_file_indices.iter().flat_map(move |&file_index| {
            self.collection_graph_for_file(file_index)
                .into_iter()
                .flat_map(|graph| graph.scene_assets.iter().copied())
        })
    }

    /// Returns PrefabInstance identities in the supplied source collection traversal order.
    ///
    /// Each collection's assets retain serialized object-table order. The caller supplies the
    /// `GameBundle.FetchAssets()` collection order; workspace file indices alone do not encode
    /// that order for nested bundles.
    pub fn prefab_instance_identities_in_collection_order<'a>(
        &'a self,
        collection_file_indices: &'a [usize],
    ) -> impl Iterator<Item = WorkspaceSceneObjectIdentity> + 'a {
        self.scene_asset_identities_in_collection_order(collection_file_indices)
            .filter_map(|asset| {
                (asset.category == WorkspaceSceneAssetCategory::PrefabInstance)
                    .then_some(asset.identity)
            })
    }

    /// Follows the source `GameObjectExtensions.GetRoot` rule: select the first Transform in
    /// `m_Component`, then follow `m_Father` while its target has a non-null `m_GameObject`.
    /// A GameObject without a Transform resolves to itself, matching the state after the source
    /// `AddMissingTransforms` pass has supplied its generated Transform.
    pub fn get_root_game_object(
        &self,
        game_object: WorkspaceSceneObjectIdentity,
        limits: WorkspaceGetRootGameObjectLimits,
    ) -> Result<WorkspaceGetRootGameObjectWalk, WorkspaceGetRootGameObjectError> {
        let mut walk = WorkspaceGetRootGameObjectWalk {
            source_game_object: game_object,
            first_transform: None,
            transform_chain: Vec::new(),
            root_game_object: None,
            termination: None,
            issue: None,
        };
        if game_object.class_id != Some(GAME_OBJECT_CLASS_ID) {
            return Err(WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                message: format!(
                    "source identity {}:{} is not a GameObject",
                    game_object.file_index, game_object.object_index
                ),
            });
        }
        let Some(graph) = self.collection_graph_for_file(game_object.file_index) else {
            walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingCollectionGraph);
            return Ok(walk);
        };
        if !graph.source_graph_complete() || !graph.source_reference_scans_complete() {
            walk.issue = Some(WorkspaceGetRootGameObjectIssue::IncompleteCollectionGraph {
                file_index: game_object.file_index,
            });
            return Ok(walk);
        }
        let Ok(game_object_index) = graph
            .game_object_hierarchies
            .binary_search_by_key(&game_object.object_index, |hierarchy| {
                hierarchy.game_object.object_index
            })
        else {
            walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingGameObjectHierarchy);
            return Ok(walk);
        };
        if graph.game_object_hierarchies[game_object_index].game_object != game_object {
            walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingGameObjectHierarchy);
            return Ok(walk);
        }

        let component_relationships = graph
            .game_object_component_relationships(game_object_index)
            .map_err(
                |error| WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                    message: error.to_string(),
                },
            )?;
        let mut selected_transform = None;
        for (relationship_index, relationship) in component_relationships.enumerate() {
            let relationship_index = graph
                .game_object_component_relationship_indices
                .get(
                    graph.game_object_hierarchies[game_object_index].component_relationship_start
                        + relationship_index,
                )
                .copied()
                .ok_or_else(|| WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                    message: "GameObject component index range changed during iteration".to_owned(),
                })?;
            if relationship.kind != SceneRelationshipKind::GameObjectComponent
                || relationship.source_object_index != game_object.object_index
                || relationship.source_path_id != game_object.path_id
            {
                return Err(WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                    message: format!(
                        "GameObject {}:{} component index points to an unrelated relationship",
                        game_object.file_index, game_object.object_index
                    ),
                });
            }
            match resolve_relationship_identity(self, graph, relationship) {
                RelationshipIdentityResolution::Null => {}
                RelationshipIdentityResolution::Resolved(component)
                    if matches!(
                        component.class_id,
                        Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                    ) =>
                {
                    selected_transform = Some(component);
                    break;
                }
                RelationshipIdentityResolution::Resolved(component)
                    if component.class_id.is_some() => {}
                RelationshipIdentityResolution::Resolved(component) => {
                    walk.issue = Some(WorkspaceGetRootGameObjectIssue::UnknownComponentClass {
                        relationship_index,
                        component,
                    });
                    return Ok(walk);
                }
                RelationshipIdentityResolution::Unresolved(target) => {
                    walk.issue = Some(
                        WorkspaceGetRootGameObjectIssue::UnresolvedComponentReference {
                            relationship_index,
                            target,
                        },
                    );
                    return Ok(walk);
                }
                RelationshipIdentityResolution::Unavailable {
                    target_file_index,
                    target_object_index,
                    path_id,
                } => {
                    walk.issue = Some(
                        WorkspaceGetRootGameObjectIssue::ComponentIdentityUnavailable {
                            relationship_index,
                            target_file_index,
                            target_object_index,
                            path_id,
                        },
                    );
                    return Ok(walk);
                }
                RelationshipIdentityResolution::PathMismatch {
                    identity,
                    expected_path_id,
                } => {
                    walk.issue = Some(WorkspaceGetRootGameObjectIssue::ComponentPathIdMismatch {
                        relationship_index,
                        expected_path_id,
                        actual_path_id: identity.path_id,
                    });
                    return Ok(walk);
                }
            }
        }

        let Some(first_transform) = selected_transform else {
            walk.root_game_object = Some(game_object);
            walk.termination = Some(WorkspaceGetRootGameObjectTermination::NoTransformComponent);
            return Ok(walk);
        };
        walk.first_transform = Some(first_transform);
        walk.transform_chain.try_reserve(1).map_err(|_| {
            WorkspaceGetRootGameObjectError::AllocationFailed {
                resource: "GetRoot Transform chain",
                requested: 1,
            }
        })?;
        walk.transform_chain.push(first_transform);
        let mut visited = HashSet::new();
        visited
            .try_reserve(1)
            .map_err(|_| WorkspaceGetRootGameObjectError::AllocationFailed {
                resource: "GetRoot Transform cycle set",
                requested: 1,
            })?;
        visited.insert(WorkspaceObjectKey::from(first_transform));
        let mut current_transform = first_transform;

        loop {
            if walk.transform_chain.len() > limits.max_parent_depth {
                return Err(WorkspaceGetRootGameObjectError::ParentDepthLimitExceeded {
                    requested: walk.transform_chain.len(),
                    limit: limits.max_parent_depth,
                });
            }
            let Some(current_graph) = self.collection_graph_for_file(current_transform.file_index)
            else {
                walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingCollectionGraph);
                return Ok(walk);
            };
            if !current_graph.source_graph_complete()
                || !current_graph.source_reference_scans_complete()
            {
                walk.issue = Some(WorkspaceGetRootGameObjectIssue::IncompleteCollectionGraph {
                    file_index: current_transform.file_index,
                });
                return Ok(walk);
            }
            let Ok(transform_index) = current_graph
                .transform_hierarchies
                .binary_search_by_key(&current_transform.object_index, |hierarchy| {
                    hierarchy.transform.object_index
                })
            else {
                walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingTransformHierarchy {
                    transform: current_transform,
                });
                return Ok(walk);
            };
            if current_graph.transform_hierarchies[transform_index].transform != current_transform {
                walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingTransformHierarchy {
                    transform: current_transform,
                });
                return Ok(walk);
            }
            let parent_relationships = current_graph
                .transform_parent_relationships(transform_index)
                .map_err(
                    |error| WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                        message: error.to_string(),
                    },
                )?;
            let mut parent_relationships = parent_relationships.peekable();
            let Some(parent_relationship) = parent_relationships.next() else {
                walk.issue = Some(
                    WorkspaceGetRootGameObjectIssue::MissingTransformParentField {
                        transform: current_transform,
                    },
                );
                return Ok(walk);
            };
            let extra_parent_count = parent_relationships.count();
            if extra_parent_count > 0 {
                walk.issue = Some(
                    WorkspaceGetRootGameObjectIssue::DuplicateTransformParentFields {
                        transform: current_transform,
                        match_count: extra_parent_count + 1,
                    },
                );
                return Ok(walk);
            }
            if parent_relationship.kind != SceneRelationshipKind::TransformParent
                || parent_relationship.source_object_index != current_transform.object_index
                || parent_relationship.source_path_id != current_transform.path_id
            {
                return Err(WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                    message: format!(
                        "Transform {}:{} parent index points to an unrelated relationship",
                        current_transform.file_index, current_transform.object_index
                    ),
                });
            }
            let parent_transform =
                match resolve_relationship_identity(self, current_graph, parent_relationship) {
                    RelationshipIdentityResolution::Null => {
                        walk.termination = Some(WorkspaceGetRootGameObjectTermination::NullFather);
                        None
                    }
                    RelationshipIdentityResolution::Resolved(parent)
                        if matches!(
                            parent.class_id,
                            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                        ) =>
                    {
                        match resolve_transform_game_object(self, parent, &mut walk)? {
                            Some(_) => Some(parent),
                            None if walk.issue.is_none() => {
                                walk.termination = Some(
                                    WorkspaceGetRootGameObjectTermination::ParentHasNullGameObject,
                                );
                                None
                            }
                            None => return Ok(walk),
                        }
                    }
                    RelationshipIdentityResolution::Resolved(target) => {
                        walk.issue = Some(
                            WorkspaceGetRootGameObjectIssue::TransformParentClassMismatch {
                                target,
                            },
                        );
                        return Ok(walk);
                    }
                    RelationshipIdentityResolution::Unresolved(target) => {
                        walk.issue =
                            Some(WorkspaceGetRootGameObjectIssue::UnresolvedTransformParent {
                                transform: current_transform,
                                target,
                            });
                        return Ok(walk);
                    }
                    RelationshipIdentityResolution::Unavailable {
                        target_file_index,
                        target_object_index,
                        path_id,
                    } => {
                        walk.issue = Some(
                            WorkspaceGetRootGameObjectIssue::TransformParentIdentityUnavailable {
                                transform: current_transform,
                                target_file_index,
                                target_object_index,
                                path_id,
                            },
                        );
                        return Ok(walk);
                    }
                    RelationshipIdentityResolution::PathMismatch {
                        identity,
                        expected_path_id,
                    } => {
                        walk.issue = Some(
                            WorkspaceGetRootGameObjectIssue::TransformParentPathIdMismatch {
                                transform: current_transform,
                                expected_path_id,
                                actual_path_id: identity.path_id,
                            },
                        );
                        return Ok(walk);
                    }
                };

            let Some(parent_transform) = parent_transform else {
                break;
            };
            let key = WorkspaceObjectKey::from(parent_transform);
            if visited.contains(&key) {
                walk.issue = Some(WorkspaceGetRootGameObjectIssue::CycleDetected {
                    transform: parent_transform,
                });
                return Ok(walk);
            }
            let requested_depth = walk.transform_chain.len().checked_add(1).ok_or(
                WorkspaceGetRootGameObjectError::CountOverflow {
                    resource: "GetRoot Transform chain",
                },
            )?;
            if requested_depth > limits.max_parent_depth {
                return Err(WorkspaceGetRootGameObjectError::ParentDepthLimitExceeded {
                    requested: requested_depth,
                    limit: limits.max_parent_depth,
                });
            }
            walk.transform_chain.try_reserve(1).map_err(|_| {
                WorkspaceGetRootGameObjectError::AllocationFailed {
                    resource: "GetRoot Transform chain",
                    requested: requested_depth,
                }
            })?;
            visited.try_reserve(1).map_err(|_| {
                WorkspaceGetRootGameObjectError::AllocationFailed {
                    resource: "GetRoot Transform cycle set",
                    requested: visited.len().saturating_add(1),
                }
            })?;
            visited.insert(key);
            walk.transform_chain.push(parent_transform);
            current_transform = parent_transform;
        }

        if walk.issue.is_none() {
            if let Some(terminal_transform) = walk.transform_chain.last().copied() {
                walk.root_game_object =
                    resolve_transform_game_object(self, terminal_transform, &mut walk)?;
                if walk.root_game_object.is_none() && walk.issue.is_none() {
                    walk.issue = Some(WorkspaceGetRootGameObjectIssue::TransformGameObjectNull {
                        transform: terminal_transform,
                    });
                }
            }
        }
        Ok(walk)
    }

    /// Resolves one serialized `PrefabInstance.m_RootGameObject` edge against retained graphs.
    /// A missing or malformed field remains `FieldUnavailable`; it is never converted to null.
    pub fn resolve_prefab_instance_root(
        &self,
        prefab_instance: WorkspaceSceneObjectIdentity,
    ) -> Result<WorkspacePrefabInstanceRootResolution, WorkspacePrefabInstanceRootError> {
        if prefab_instance.class_id != Some(PREFAB_INSTANCE_CLASS_ID) {
            return Err(WorkspacePrefabInstanceRootError::NotPrefabInstance {
                identity: prefab_instance,
            });
        }
        let Some(graph) = self.collection_graph_for_file(prefab_instance.file_index) else {
            return Err(WorkspacePrefabInstanceRootError::MissingCollectionGraph {
                file_index: prefab_instance.file_index,
            });
        };
        let key = (prefab_instance.file_index, prefab_instance.object_index);
        let has_prefab_instance = graph
            .scene_assets
            .binary_search_by_key(&key, |asset| {
                (asset.identity.file_index, asset.identity.object_index)
            })
            .ok()
            .and_then(|index| graph.scene_assets.get(index))
            .filter(|asset| {
                asset.category == WorkspaceSceneAssetCategory::PrefabInstance
                    && asset.identity == prefab_instance
            })
            .is_some();
        if !has_prefab_instance {
            return Err(WorkspacePrefabInstanceRootError::MissingPrefabInstance {
                identity: prefab_instance,
            });
        }
        let relationships =
            graph.prefab_root_relationships_for_object(prefab_instance.object_index);
        if relationships.is_empty() {
            return Ok(WorkspacePrefabInstanceRootResolution::FieldUnavailable);
        }
        if relationships.len() > 1 {
            return Ok(
                WorkspacePrefabInstanceRootResolution::DuplicateRootReferences {
                    match_count: relationships.len(),
                },
            );
        }
        let relationship_index = relationships[0].relationship_index;
        let Some(relationship) = graph.relationships.get(relationship_index) else {
            return Err(WorkspacePrefabInstanceRootError::InvalidRelationshipIndex {
                relationship_index,
                relationship_count: graph.relationships.len(),
            });
        };
        if relationship.source_object_index != prefab_instance.object_index
            || relationship.kind != SceneRelationshipKind::PrefabInstanceRootGameObject
            || relationship.source_path_id != prefab_instance.path_id
        {
            return Err(
                WorkspacePrefabInstanceRootError::RelationshipIndexMismatch {
                    expected_object_index: prefab_instance.object_index,
                    relationship_index,
                    actual_object_index: relationship.source_object_index,
                    kind: relationship.kind,
                },
            );
        }
        let resolution = match resolve_relationship_identity(self, graph, relationship) {
            RelationshipIdentityResolution::Null => WorkspacePrefabInstanceRootResolution::Null,
            RelationshipIdentityResolution::Resolved(identity)
                if identity.class_id == Some(GAME_OBJECT_CLASS_ID) =>
            {
                WorkspacePrefabInstanceRootResolution::Resolved(identity)
            }
            RelationshipIdentityResolution::Resolved(identity) => {
                WorkspacePrefabInstanceRootResolution::TargetClassMismatch {
                    target: identity,
                    actual_class_id: identity.class_id,
                }
            }
            RelationshipIdentityResolution::Unresolved(target) => {
                WorkspacePrefabInstanceRootResolution::UnresolvedTarget(target)
            }
            RelationshipIdentityResolution::Unavailable {
                target_file_index,
                target_object_index,
                path_id,
            } => WorkspacePrefabInstanceRootResolution::TargetIdentityUnavailable {
                target_file_index,
                target_object_index,
                path_id,
            },
            RelationshipIdentityResolution::PathMismatch {
                identity,
                expected_path_id,
            } => WorkspacePrefabInstanceRootResolution::TargetIdentityPathMismatch {
                expected_path_id,
                actual_path_id: identity.path_id,
            },
        };
        Ok(resolution)
    }

    pub fn scene_for_collection(
        &self,
        definitions: &WorkspaceSceneDefinitions,
        collection_file_index: usize,
    ) -> Option<&WorkspaceSceneHierarchyScene> {
        definitions
            .scene_index_for_collection(collection_file_index)
            .and_then(|scene_index| self.scenes.get(scene_index))
    }
}

enum RelationshipIdentityResolution {
    Null,
    Resolved(WorkspaceSceneObjectIdentity),
    Unresolved(SceneRelationshipTarget),
    Unavailable {
        target_file_index: usize,
        target_object_index: usize,
        path_id: i64,
    },
    PathMismatch {
        identity: WorkspaceSceneObjectIdentity,
        expected_path_id: i64,
    },
}

fn note_incomplete_collection_graph(
    graph: &WorkspaceSceneCollectionHierarchy,
    visited_graphs: &mut HashSet<usize>,
    walk: &mut WorkspaceGameObjectHierarchyWalk,
    limits: WorkspaceGameObjectHierarchyWalkLimits,
) -> Result<(), WorkspaceGameObjectHierarchyWalkError> {
    let file_index = graph.collection_file_index();
    if visited_graphs.contains(&file_index) {
        return Ok(());
    }
    let requested_graphs = visited_graphs.len().checked_add(1).ok_or(
        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
            resource: "hierarchy graph visitation index",
        },
    )?;
    visited_graphs.try_reserve(1).map_err(|_| {
        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
            resource: "hierarchy graph visitation index",
            requested: requested_graphs,
        }
    })?;
    visited_graphs.insert(file_index);
    let root_discovery_complete = graph.roots().is_complete();
    let transform_audit_complete = graph.hierarchy_audit().is_complete();
    if graph.source_graph_complete()
        && graph.source_reference_scans_complete()
        && root_discovery_complete
        && transform_audit_complete
    {
        return Ok(());
    }
    push_hierarchy_walk_issue(
        walk,
        WorkspaceGameObjectHierarchyIssue::IncompleteCollectionGraph {
            file_index,
            source_graph_complete: graph.source_graph_complete(),
            reference_scans_complete: graph.source_reference_scans_complete(),
            root_discovery_complete,
            transform_audit_complete,
            graph_issue_count: graph.graph_issues().len(),
            root_issue_count: graph.roots().issues().len(),
            transform_audit_issue_count: graph.hierarchy_audit().issues().len(),
        },
        limits,
    )
}

fn push_hierarchy_walk_entry(
    walk: &mut WorkspaceGameObjectHierarchyWalk,
    entry: WorkspaceGameObjectHierarchyEntry,
    limits: WorkspaceGameObjectHierarchyWalkLimits,
) -> Result<(), WorkspaceGameObjectHierarchyWalkError> {
    let requested = walk.entries.len().checked_add(1).ok_or(
        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
            resource: "GameObject hierarchy entries",
        },
    )?;
    if requested > limits.max_entries {
        return Err(
            WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                resource: "GameObject hierarchy entries",
                requested,
                limit: limits.max_entries,
            },
        );
    }
    walk.entries.try_reserve(1).map_err(|_| {
        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
            resource: "GameObject hierarchy entries",
            requested,
        }
    })?;
    walk.entries.push(entry);
    Ok(())
}

fn push_hierarchy_walk_issue(
    walk: &mut WorkspaceGameObjectHierarchyWalk,
    issue: WorkspaceGameObjectHierarchyIssue,
    limits: WorkspaceGameObjectHierarchyWalkLimits,
) -> Result<(), WorkspaceGameObjectHierarchyWalkError> {
    let requested = walk.issues.len().checked_add(1).ok_or(
        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
            resource: "GameObject hierarchy issues",
        },
    )?;
    if requested > limits.max_issues {
        return Err(
            WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                resource: "GameObject hierarchy issues",
                requested,
                limit: limits.max_issues,
            },
        );
    }
    walk.issues.try_reserve(1).map_err(|_| {
        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
            resource: "GameObject hierarchy issues",
            requested,
        }
    })?;
    walk.issues.push(issue);
    Ok(())
}

fn workspace_object_identity(
    graph: &WorkspaceSceneCollectionHierarchy,
    file_index: usize,
    object_index: usize,
) -> Option<WorkspaceSceneObjectIdentity> {
    let key = (file_index, object_index);
    graph
        .scene_assets()
        .binary_search_by_key(&key, |asset| {
            (asset.identity.file_index, asset.identity.object_index)
        })
        .ok()
        .and_then(|index| graph.scene_assets().get(index))
        .map(|asset| asset.identity)
        .or_else(|| {
            graph
                .component_candidates()
                .binary_search_by_key(&key, |candidate| {
                    (
                        candidate.identity.file_index,
                        candidate.identity.object_index,
                    )
                })
                .ok()
                .and_then(|index| graph.component_candidates().get(index))
                .map(|candidate| candidate.identity)
        })
        .or_else(|| {
            if file_index != graph.collection_file_index() {
                return None;
            }
            graph
                .scene_objects()
                .binary_search_by_key(&object_index, |object| object.object_index)
                .ok()
                .and_then(|index| graph.scene_objects().get(index))
                .map(|object| WorkspaceSceneObjectIdentity {
                    file_index,
                    object_index,
                    path_id: object.path_id,
                    class_id: object.class_id,
                })
        })
}

fn resolve_relationship_identity(
    workspace: &WorkspaceSceneHierarchies,
    source_graph: &WorkspaceSceneCollectionHierarchy,
    relationship: &SceneRelationship,
) -> RelationshipIdentityResolution {
    let (target_file_index, target_object_index) = match &relationship.target {
        SceneRelationshipTarget::Null => return RelationshipIdentityResolution::Null,
        SceneRelationshipTarget::LocalObject { object_index } => {
            (source_graph.collection_file_index(), *object_index)
        }
        SceneRelationshipTarget::WorkspaceObject {
            file_index,
            object_index,
        } => (*file_index, *object_index),
        target => return RelationshipIdentityResolution::Unresolved(target.clone()),
    };
    let identity = workspace_object_identity(source_graph, target_file_index, target_object_index)
        .or_else(|| {
            workspace
                .collection_graph_for_file(target_file_index)
                .and_then(|graph| {
                    workspace_object_identity(graph, target_file_index, target_object_index)
                })
        });
    let Some(identity) = identity else {
        return RelationshipIdentityResolution::Unavailable {
            target_file_index,
            target_object_index,
            path_id: relationship.path_id,
        };
    };
    if identity.path_id != relationship.path_id {
        return RelationshipIdentityResolution::PathMismatch {
            identity,
            expected_path_id: relationship.path_id,
        };
    }
    RelationshipIdentityResolution::Resolved(identity)
}

fn graph_index_walk_error(
    error: WorkspaceSceneHierarchyError,
) -> WorkspaceGameObjectHierarchyWalkError {
    WorkspaceGameObjectHierarchyWalkError::GraphInvariantFailed {
        message: error.to_string(),
    }
}

fn expand_game_object_components(
    workspace: &WorkspaceSceneHierarchies,
    graph: &WorkspaceSceneCollectionHierarchy,
    game_object: WorkspaceSceneObjectIdentity,
    hierarchy_index: usize,
    walk: &mut WorkspaceGameObjectHierarchyWalk,
    limits: WorkspaceGameObjectHierarchyWalkLimits,
) -> Result<Option<WorkspaceSceneObjectIdentity>, WorkspaceGameObjectHierarchyWalkError> {
    let hierarchy = graph
        .game_object_hierarchies
        .get(hierarchy_index)
        .ok_or_else(|| {
            graph_index_walk_error(
                WorkspaceSceneHierarchyError::GameObjectHierarchyIndexOutOfBounds {
                    hierarchy_index,
                    hierarchy_count: graph.game_object_hierarchies.len(),
                },
            )
        })?;
    let indexes = graph
        .game_object_component_relationship_indices
        .get(hierarchy.component_relationship_start..hierarchy.component_relationship_end)
        .ok_or_else(|| {
            graph_index_walk_error(
                WorkspaceSceneHierarchyError::InvalidGameObjectComponentRelationshipRange {
                    hierarchy_index,
                    start: hierarchy.component_relationship_start,
                    end: hierarchy.component_relationship_end,
                    index_count: graph.game_object_component_relationship_indices.len(),
                },
            )
        })?;

    let mut selected_transform = None;
    for &relationship_index in indexes {
        let relationship = graph
            .relationships()
            .get(relationship_index)
            .ok_or_else(|| {
                graph_index_walk_error(
                    WorkspaceSceneHierarchyError::InvalidGameObjectComponentRelationshipIndex {
                        hierarchy_index,
                        relationship_index,
                        relationship_count: graph.relationships().len(),
                    },
                )
            })?;
        if relationship.kind != SceneRelationshipKind::GameObjectComponent {
            return Err(
                WorkspaceGameObjectHierarchyWalkError::GraphInvariantFailed {
                    message: format!(
                        "GameObject {}:{} index points at {:?}",
                        game_object.file_index, game_object.object_index, relationship.kind
                    ),
                },
            );
        }
        match resolve_relationship_identity(workspace, graph, relationship) {
            RelationshipIdentityResolution::Null => {}
            RelationshipIdentityResolution::Resolved(component) => {
                if component.class_id.is_none() {
                    push_hierarchy_walk_issue(
                        walk,
                        WorkspaceGameObjectHierarchyIssue::UnknownComponentClass {
                            game_object,
                            relationship_index,
                            component,
                        },
                        limits,
                    )?;
                }
                push_hierarchy_walk_entry(
                    walk,
                    WorkspaceGameObjectHierarchyEntry {
                        identity: component,
                        kind: WorkspaceGameObjectHierarchyEntryKind::Component,
                    },
                    limits,
                )?;
                if matches!(
                    component.class_id,
                    Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                ) {
                    selected_transform = Some(component);
                }
            }
            RelationshipIdentityResolution::Unresolved(target) => {
                push_hierarchy_walk_issue(
                    walk,
                    WorkspaceGameObjectHierarchyIssue::UnresolvedComponentReference {
                        game_object,
                        relationship_index,
                        target,
                    },
                    limits,
                )?;
            }
            RelationshipIdentityResolution::Unavailable {
                target_file_index,
                target_object_index,
                path_id,
            } => {
                push_hierarchy_walk_issue(
                    walk,
                    WorkspaceGameObjectHierarchyIssue::ComponentIdentityUnavailable {
                        game_object,
                        relationship_index,
                        target_file_index,
                        target_object_index,
                        path_id,
                    },
                    limits,
                )?;
            }
            RelationshipIdentityResolution::PathMismatch {
                identity,
                expected_path_id,
            } => {
                push_hierarchy_walk_issue(
                    walk,
                    WorkspaceGameObjectHierarchyIssue::TargetIdentityPathMismatch {
                        source: game_object,
                        relationship_index,
                        reference_kind: WorkspaceGameObjectHierarchyReferenceKind::Component,
                        expected_path_id,
                        actual_path_id: identity.path_id,
                    },
                    limits,
                )?;
            }
        }
    }
    Ok(selected_transform)
}

fn collect_hierarchy_children(
    workspace: &WorkspaceSceneHierarchies,
    transform: WorkspaceSceneObjectIdentity,
    walk: &mut WorkspaceGameObjectHierarchyWalk,
    visited_graphs: &mut HashSet<usize>,
    limits: WorkspaceGameObjectHierarchyWalkLimits,
) -> Result<Vec<WorkspaceSceneObjectIdentity>, WorkspaceGameObjectHierarchyWalkError> {
    let mut children = Vec::new();
    let Some(graph) = workspace.collection_graph_for_file(transform.file_index) else {
        push_hierarchy_walk_issue(
            walk,
            WorkspaceGameObjectHierarchyIssue::MissingCollectionGraph {
                identity: transform,
            },
            limits,
        )?;
        return Ok(children);
    };
    note_incomplete_collection_graph(graph, visited_graphs, walk, limits)?;
    let Ok(transform_index) = graph
        .transform_hierarchies
        .binary_search_by_key(&transform.object_index, |hierarchy| {
            hierarchy.transform.object_index
        })
    else {
        push_hierarchy_walk_issue(
            walk,
            WorkspaceGameObjectHierarchyIssue::MissingTransformHierarchy { transform },
            limits,
        )?;
        return Ok(children);
    };
    if graph.transform_hierarchies[transform_index].transform != transform {
        push_hierarchy_walk_issue(
            walk,
            WorkspaceGameObjectHierarchyIssue::MissingTransformHierarchy { transform },
            limits,
        )?;
        return Ok(children);
    }
    let transform_hierarchy = &graph.transform_hierarchies[transform_index];
    let child_indexes = graph
        .transform_child_relationship_indices
        .get(
            transform_hierarchy.child_relationship_start
                ..transform_hierarchy.child_relationship_end,
        )
        .ok_or_else(|| {
            graph_index_walk_error(
                WorkspaceSceneHierarchyError::InvalidTransformChildRelationshipRange {
                    hierarchy_index: transform_index,
                    start: transform_hierarchy.child_relationship_start,
                    end: transform_hierarchy.child_relationship_end,
                    index_count: graph.transform_child_relationship_indices.len(),
                },
            )
        })?;
    children
        .try_reserve_exact(child_indexes.len())
        .map_err(
            |_| WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
                resource: "ordered GameObject hierarchy children",
                requested: child_indexes.len(),
            },
        )?;

    for &child_relationship_index in child_indexes {
        let child_relationship = graph
            .relationships()
            .get(child_relationship_index)
            .ok_or_else(|| {
                graph_index_walk_error(
                    WorkspaceSceneHierarchyError::InvalidTransformRelationshipIndex {
                        hierarchy_index: transform_index,
                        relationship_index: child_relationship_index,
                        relationship_count: graph.relationships().len(),
                    },
                )
            })?;
        if child_relationship.kind != SceneRelationshipKind::TransformChild {
            return Err(
                WorkspaceGameObjectHierarchyWalkError::GraphInvariantFailed {
                    message: format!(
                        "Transform {}:{} child index points at {:?}",
                        transform.file_index, transform.object_index, child_relationship.kind
                    ),
                },
            );
        }
        let child_transform =
            match resolve_relationship_identity(workspace, graph, child_relationship) {
                RelationshipIdentityResolution::Null => continue,
                RelationshipIdentityResolution::Resolved(identity) => identity,
                RelationshipIdentityResolution::Unresolved(target) => {
                    push_hierarchy_walk_issue(
                        walk,
                        WorkspaceGameObjectHierarchyIssue::UnresolvedChildTransformReference {
                            transform,
                            relationship_index: child_relationship_index,
                            target,
                        },
                        limits,
                    )?;
                    continue;
                }
                RelationshipIdentityResolution::Unavailable {
                    target_file_index,
                    target_object_index,
                    path_id,
                } => {
                    push_hierarchy_walk_issue(
                        walk,
                        WorkspaceGameObjectHierarchyIssue::ChildTransformIdentityUnavailable {
                            transform,
                            relationship_index: child_relationship_index,
                            target_file_index,
                            target_object_index,
                            path_id,
                        },
                        limits,
                    )?;
                    continue;
                }
                RelationshipIdentityResolution::PathMismatch {
                    identity,
                    expected_path_id,
                } => {
                    push_hierarchy_walk_issue(
                        walk,
                        WorkspaceGameObjectHierarchyIssue::TargetIdentityPathMismatch {
                            source: transform,
                            relationship_index: child_relationship_index,
                            reference_kind:
                                WorkspaceGameObjectHierarchyReferenceKind::ChildTransform,
                            expected_path_id,
                            actual_path_id: identity.path_id,
                        },
                        limits,
                    )?;
                    continue;
                }
            };
        if !matches!(
            child_transform.class_id,
            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
        ) {
            push_hierarchy_walk_issue(
                walk,
                WorkspaceGameObjectHierarchyIssue::ChildTargetIsNotTransform {
                    transform,
                    relationship_index: child_relationship_index,
                    child: child_transform,
                },
                limits,
            )?;
            continue;
        }
        let Some(child_graph) = workspace.collection_graph_for_file(child_transform.file_index)
        else {
            push_hierarchy_walk_issue(
                walk,
                WorkspaceGameObjectHierarchyIssue::MissingCollectionGraph {
                    identity: child_transform,
                },
                limits,
            )?;
            continue;
        };
        note_incomplete_collection_graph(child_graph, visited_graphs, walk, limits)?;
        let Ok(child_transform_index) = child_graph
            .transform_hierarchies
            .binary_search_by_key(&child_transform.object_index, |hierarchy| {
                hierarchy.transform.object_index
            })
        else {
            push_hierarchy_walk_issue(
                walk,
                WorkspaceGameObjectHierarchyIssue::MissingTransformHierarchy {
                    transform: child_transform,
                },
                limits,
            )?;
            continue;
        };
        if child_graph.transform_hierarchies[child_transform_index].transform != child_transform {
            push_hierarchy_walk_issue(
                walk,
                WorkspaceGameObjectHierarchyIssue::MissingTransformHierarchy {
                    transform: child_transform,
                },
                limits,
            )?;
            continue;
        }
        let child_transform_hierarchy = &child_graph.transform_hierarchies[child_transform_index];
        let owner_indexes = child_graph
            .transform_game_object_relationship_indices
            .get(
                child_transform_hierarchy.game_object_relationship_start
                    ..child_transform_hierarchy.game_object_relationship_end,
            )
            .ok_or_else(|| {
                graph_index_walk_error(
                    WorkspaceSceneHierarchyError::InvalidTransformGameObjectRelationshipRange {
                        hierarchy_index: child_transform_index,
                        start: child_transform_hierarchy.game_object_relationship_start,
                        end: child_transform_hierarchy.game_object_relationship_end,
                        index_count: child_graph.transform_game_object_relationship_indices.len(),
                    },
                )
            })?;
        match owner_indexes {
            [] => {
                push_hierarchy_walk_issue(
                    walk,
                    WorkspaceGameObjectHierarchyIssue::MissingTransformGameObjectReference {
                        transform: child_transform,
                    },
                    limits,
                )?;
            }
            [owner_relationship_index] => {
                let owner_relationship = child_graph
                    .relationships()
                    .get(*owner_relationship_index)
                    .ok_or_else(|| {
                        graph_index_walk_error(
                            WorkspaceSceneHierarchyError::InvalidTransformRelationshipIndex {
                                hierarchy_index: child_transform_index,
                                relationship_index: *owner_relationship_index,
                                relationship_count: child_graph.relationships().len(),
                            },
                        )
                    })?;
                if owner_relationship.kind != SceneRelationshipKind::ComponentGameObject {
                    return Err(
                        WorkspaceGameObjectHierarchyWalkError::GraphInvariantFailed {
                            message: format!(
                                "Transform {}:{} owner index points at {:?}",
                                child_transform.file_index,
                                child_transform.object_index,
                                owner_relationship.kind
                            ),
                        },
                    );
                }
                match resolve_relationship_identity(workspace, child_graph, owner_relationship) {
                    RelationshipIdentityResolution::Null => {}
                    RelationshipIdentityResolution::Resolved(owner) => {
                        if owner.class_id != Some(GAME_OBJECT_CLASS_ID) {
                            push_hierarchy_walk_issue(
                                walk,
                                WorkspaceGameObjectHierarchyIssue::TransformOwnerIsNotGameObject {
                                    transform: child_transform,
                                    relationship_index: *owner_relationship_index,
                                    target: owner,
                                },
                                limits,
                            )?;
                        } else {
                            children.push(owner);
                        }
                    }
                    RelationshipIdentityResolution::Unresolved(target) => {
                        push_hierarchy_walk_issue(
                            walk,
                            WorkspaceGameObjectHierarchyIssue::UnresolvedTransformGameObjectReference {
                                transform: child_transform,
                                relationship_index: *owner_relationship_index,
                                target,
                            },
                            limits,
                        )?;
                    }
                    RelationshipIdentityResolution::Unavailable {
                        target_file_index,
                        target_object_index,
                        path_id,
                    } => {
                        push_hierarchy_walk_issue(
                            walk,
                            WorkspaceGameObjectHierarchyIssue::GameObjectOwnerIdentityUnavailable {
                                transform: child_transform,
                                relationship_index: *owner_relationship_index,
                                target_file_index,
                                target_object_index,
                                path_id,
                            },
                            limits,
                        )?;
                    }
                    RelationshipIdentityResolution::PathMismatch {
                        identity,
                        expected_path_id,
                    } => {
                        push_hierarchy_walk_issue(
                            walk,
                            WorkspaceGameObjectHierarchyIssue::TargetIdentityPathMismatch {
                                source: child_transform,
                                relationship_index: *owner_relationship_index,
                                reference_kind:
                                    WorkspaceGameObjectHierarchyReferenceKind::TransformGameObject,
                                expected_path_id,
                                actual_path_id: identity.path_id,
                            },
                            limits,
                        )?;
                    }
                }
            }
            relationships => {
                push_hierarchy_walk_issue(
                    walk,
                    WorkspaceGameObjectHierarchyIssue::DuplicateTransformGameObjectReferences {
                        transform: child_transform,
                        match_count: relationships.len(),
                    },
                    limits,
                )?;
            }
        }
    }
    Ok(children)
}

fn build_collection_graph_index(
    scenes: &[WorkspaceSceneHierarchyScene],
    non_scene_collections: &[WorkspaceSceneCollectionHierarchy],
    expected_count: usize,
) -> Result<Vec<WorkspaceSceneCollectionGraphIndexEntry>, WorkspaceSceneHierarchyError> {
    let mut index = Vec::new();
    index.try_reserve_exact(expected_count).map_err(|_| {
        WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "workspace collection graph lookup index",
            requested: expected_count,
        }
    })?;
    for (scene_index, scene) in scenes.iter().enumerate() {
        for (collection_index, collection) in scene.collections.iter().enumerate() {
            index.push(WorkspaceSceneCollectionGraphIndexEntry {
                file_index: collection.collection_file_index(),
                scene_index: Some(scene_index),
                collection_index,
            });
        }
    }
    for (collection_index, collection) in non_scene_collections.iter().enumerate() {
        index.push(WorkspaceSceneCollectionGraphIndexEntry {
            file_index: collection.collection_file_index(),
            scene_index: None,
            collection_index,
        });
    }
    if index.len() != expected_count {
        return Err(
            WorkspaceSceneHierarchyError::WorkspaceCollectionGraphIndexCountMismatch {
                expected: expected_count,
                actual: index.len(),
            },
        );
    }
    index.sort_unstable_by_key(|entry| entry.file_index);
    for pair in index.windows(2) {
        if pair[0].file_index == pair[1].file_index {
            return Err(WorkspaceSceneHierarchyError::DuplicateCollectionGraph {
                file_index: pair[0].file_index,
            });
        }
    }
    Ok(index)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceGameObjectHierarchyWalkError {
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
    RootIsNotGameObject {
        identity: WorkspaceSceneObjectIdentity,
    },
    GraphInvariantFailed {
        message: String,
    },
}

impl Display for WorkspaceGameObjectHierarchyWalkError {
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
            Self::RootIsNotGameObject { identity } => write!(
                f,
                "hierarchy root file {} object {} PathID {} is not a GameObject",
                identity.file_index, identity.object_index, identity.path_id
            ),
            Self::GraphInvariantFailed { message } => {
                write!(f, "hierarchy graph index is inconsistent: {message}")
            }
        }
    }
}

impl Error for WorkspaceGameObjectHierarchyWalkError {}

/// Incrementally compacts hierarchy graphs so inspect does not retain full per-file indexes.
pub struct WorkspaceSceneHierarchyBuilder {
    collection_graphs: Vec<Option<WorkspaceSceneCollectionHierarchy>>,
    limits: WorkspaceSceneHierarchyLimits,
    collection_graph_count: usize,
    scene_object_count: usize,
    scene_asset_record_count: usize,
    relationship_count: usize,
    issue_count: usize,
    name_bytes: usize,
    diagnostic_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyError {
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
    WorkspaceFileCountMismatch {
        count: usize,
        limit: usize,
    },
    WorkspaceDefinitionCollectionCountMismatch {
        expected: usize,
        actual: usize,
    },
    WorkspaceCollectionGraphIndexCountMismatch {
        expected: usize,
        actual: usize,
    },
    SceneCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    CollectionFileIndexOutOfBounds {
        file_index: usize,
        file_count: usize,
    },
    DuplicateCollectionGraph {
        file_index: usize,
    },
    DuplicateGeneratedTransformSource {
        file_index: usize,
    },
    MissingCollectionGraph {
        scene_index: usize,
        file_index: usize,
    },
    InvalidSourceIdentity {
        file_index: usize,
        object_index: usize,
        path_id: i64,
    },
    InvalidRelationshipSource {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    PrefabRootRelationshipSourceMissing {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    DuplicateGameObjectHierarchy {
        file_index: usize,
        object_index: usize,
    },
    GameObjectComponentRelationshipSourceMissing {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    GameObjectHierarchyIndexOutOfBounds {
        hierarchy_index: usize,
        hierarchy_count: usize,
    },
    MissingGameObjectHierarchy {
        file_index: usize,
        object_index: usize,
    },
    InvalidGameObjectComponentRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidGameObjectComponentRelationshipIndex {
        hierarchy_index: usize,
        relationship_index: usize,
        relationship_count: usize,
    },
    DuplicateTransformHierarchy {
        file_index: usize,
        object_index: usize,
    },
    InvalidTransformHierarchySource {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    TransformHierarchyIndexOutOfBounds {
        hierarchy_index: usize,
        hierarchy_count: usize,
    },
    InvalidTransformParentRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidTransformChildRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidTransformGameObjectRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidTransformRelationshipIndex {
        hierarchy_index: usize,
        relationship_index: usize,
        relationship_count: usize,
    },
    InvalidComponentTarget {
        source_file_index: usize,
        target_file_index: usize,
        object_index: usize,
    },
    InvalidRootIdentity {
        file_index: usize,
        object_index: usize,
    },
    InvalidRootTransformIdentity {
        file_index: usize,
        transform_file_index: usize,
        transform_index: usize,
    },
    RootSourceFileMismatch {
        expected: usize,
        actual: usize,
    },
    AuditSourceFileMismatch {
        expected: usize,
        actual: usize,
    },
}

impl Display for WorkspaceSceneHierarchyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::CountOverflow { resource } => write!(f, "{resource} count overflows usize"),
            Self::ResourceLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(f, "{resource} count {requested} exceeds limit {limit}"),
            Self::WorkspaceFileCountMismatch { count, limit } => {
                write!(f, "workspace file count {count} exceeds limit {limit}")
            }
            Self::WorkspaceDefinitionCollectionCountMismatch { expected, actual } => write!(
                f,
                "generated-transform planning expected {expected} workspace collections, definitions contain {actual}"
            ),
            Self::WorkspaceCollectionGraphIndexCountMismatch { expected, actual } => write!(
                f,
                "workspace collection graph index expected {expected} entries, contains {actual}"
            ),
            Self::SceneCountLimitExceeded { count, limit } => {
                write!(f, "SceneDefinition count {count} exceeds limit {limit}")
            }
            Self::CollectionFileIndexOutOfBounds {
                file_index,
                file_count,
            } => write!(f, "scene collection file index {file_index} is outside {file_count} files"),
            Self::DuplicateCollectionGraph { file_index } => {
                write!(f, "scene collection file {file_index} has more than one graph")
            }
            Self::DuplicateGeneratedTransformSource { file_index } => write!(
                f,
                "workspace file {file_index} supplied generated-transform candidates more than once"
            ),
            Self::MissingCollectionGraph {
                scene_index,
                file_index,
            } => write!(
                f,
                "SceneDefinition {scene_index} has no recovered graph for collection file {file_index}"
            ),
            Self::InvalidSourceIdentity {
                file_index,
                object_index,
                path_id,
            } => write!(
                f,
                "scene graph object {file_index}:{object_index} PathID {path_id} does not match the joined workspace"
            ),
            Self::InvalidRelationshipSource {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "scene relationship {relationship_index} source object {file_index}:{object_index} is invalid"
            ),
            Self::PrefabRootRelationshipSourceMissing {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "PrefabInstance root relationship {relationship_index} in collection {file_index} has no matching PrefabInstance at object {object_index}"
            ),
            Self::DuplicateGameObjectHierarchy {
                file_index,
                object_index,
            } => write!(
                f,
                "scene collection {file_index} contains GameObject {object_index} more than once"
            ),
            Self::GameObjectComponentRelationshipSourceMissing {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "GameObject component relationship {relationship_index} in collection {file_index} has no indexed GameObject source at object {object_index}"
            ),
            Self::GameObjectHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count,
            } => write!(
                f,
                "GameObject hierarchy index {hierarchy_index} is outside {hierarchy_count} entries"
            ),
            Self::MissingGameObjectHierarchy {
                file_index,
                object_index,
            } => write!(
                f,
                "collection {file_index} has no component list for GameObject {object_index}"
            ),
            Self::InvalidGameObjectComponentRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "GameObject hierarchy {hierarchy_index} component relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidGameObjectComponentRelationshipIndex {
                hierarchy_index,
                relationship_index,
                relationship_count,
            } => write!(
                f,
                "GameObject hierarchy {hierarchy_index} references relationship {relationship_index}, but only {relationship_count} relationships exist"
            ),
            Self::DuplicateTransformHierarchy {
                file_index,
                object_index,
            } => write!(
                f,
                "scene collection {file_index} contains Transform {object_index} more than once"
            ),
            Self::InvalidTransformHierarchySource {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "Transform hierarchy relationship {relationship_index} in collection {file_index} has no indexed Transform source at object {object_index}"
            ),
            Self::TransformHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count,
            } => write!(
                f,
                "Transform hierarchy index {hierarchy_index} is outside {hierarchy_count} entries"
            ),
            Self::InvalidTransformParentRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} parent relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidTransformChildRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} child relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidTransformGameObjectRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} GameObject relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidTransformRelationshipIndex {
                hierarchy_index,
                relationship_index,
                relationship_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} references relationship {relationship_index}, but only {relationship_count} relationships exist"
            ),
            Self::InvalidComponentTarget {
                source_file_index,
                target_file_index,
                object_index,
            } => write!(
                f,
                "scene collection {source_file_index} component edge targets invalid object {target_file_index}:{object_index}"
            ),
            Self::InvalidRootIdentity {
                file_index,
                object_index,
            } => write!(
                f,
                "scene root candidate GameObject {file_index}:{object_index} is invalid"
            ),
            Self::InvalidRootTransformIdentity {
                file_index,
                transform_file_index,
                transform_index,
            } => write!(
                f,
                "scene root candidate Transform {transform_file_index}:{transform_index} from file {file_index} is invalid"
            ),
            Self::RootSourceFileMismatch { expected, actual } => write!(
                f,
                "root discovery belongs to source file {actual}, expected {expected}"
            ),
            Self::AuditSourceFileMismatch { expected, actual } => write!(
                f,
                "hierarchy audit contains source file {actual}, expected {expected}"
            ),
        }
    }
}

impl Error for WorkspaceSceneHierarchyError {}

impl WorkspaceSceneHierarchyBuilder {
    pub fn new(
        workspace_file_count: usize,
        limits: WorkspaceSceneHierarchyLimits,
    ) -> Result<Self, WorkspaceSceneHierarchyError> {
        if workspace_file_count > limits.max_workspace_files {
            return Err(WorkspaceSceneHierarchyError::WorkspaceFileCountMismatch {
                count: workspace_file_count,
                limit: limits.max_workspace_files,
            });
        }
        let mut collection_graphs = Vec::new();
        collection_graphs
            .try_reserve_exact(workspace_file_count)
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "workspace scene collection graph index",
                requested: workspace_file_count,
            })?;
        collection_graphs.resize_with(workspace_file_count, || None);
        Ok(Self {
            collection_graphs,
            limits,
            collection_graph_count: 0,
            scene_object_count: 0,
            scene_asset_record_count: 0,
            relationship_count: 0,
            issue_count: 0,
            name_bytes: 0,
            diagnostic_bytes: 0,
        })
    }

    pub fn contains_collection_graph(&self, file_index: usize) -> bool {
        self.collection_graphs
            .get(file_index)
            .is_some_and(Option::is_some)
    }

    pub fn add_collection_graph(
        &mut self,
        file_index: usize,
        scene_graph: &SceneGraph,
        roots: WorkspaceSceneRootDiscovery,
        hierarchy_audit: WorkspaceSceneHierarchyAudit,
        references: &MultiFileReferenceGraph<'_>,
    ) -> Result<(), WorkspaceSceneHierarchyError> {
        let Some(slot) = self.collection_graphs.get(file_index) else {
            return Err(
                WorkspaceSceneHierarchyError::CollectionFileIndexOutOfBounds {
                    file_index,
                    file_count: self.collection_graphs.len(),
                },
            );
        };
        if slot.is_some() {
            return Err(WorkspaceSceneHierarchyError::DuplicateCollectionGraph { file_index });
        }

        validate_collection_graph(
            file_index,
            scene_graph,
            &roots,
            &hierarchy_audit,
            references,
        )?;
        let source_reference_scans_complete =
            (0..scene_graph.objects().len()).all(|object_index| {
                references.object_scan_is_complete(file_index, object_index) == Some(true)
            });
        let classification =
            classify_scene_collection_objects(file_index, scene_graph, references)?;
        let ClassifiedSceneCollectionObjects {
            scene_assets,
            lighting_data_asset_field_object_indices,
            component_candidates,
        } = classification;
        let missing_transform_plans =
            build_missing_transform_plans(file_index, &roots, references)?;
        let scene_objects = scene_graph
            .objects()
            .iter()
            .filter(|object| {
                matches!(
                    object.class_id,
                    Some(GAME_OBJECT_CLASS_ID | TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                )
            })
            .count();
        let issue_count = scene_graph
            .issues()
            .len()
            .checked_add(roots.issues().len())
            .and_then(|count| count.checked_add(hierarchy_audit.issues().len()))
            .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
                resource: "scene graph diagnostics",
            })?;
        let name_bytes = scene_graph
            .objects()
            .iter()
            .filter(|object| {
                matches!(
                    object.class_id,
                    Some(GAME_OBJECT_CLASS_ID | TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                )
            })
            .try_fold(0usize, |total, object| {
                total.checked_add(object.name_bytes.as_ref().map_or(0, Vec::len))
            })
            .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
                resource: "scene object names",
            })?;
        let diagnostic_bytes = scene_graph
            .relationships()
            .iter()
            .try_fold(0usize, |total, relationship| {
                total.checked_add(relationship.field_path.len())
            })
            .and_then(|total| {
                scene_graph.issues().iter().try_fold(total, |total, issue| {
                    let field_path_bytes = issue.field_path.as_ref().map_or(0, String::len);
                    let kind_bytes = match &issue.kind {
                        super::scene::SceneGraphIssueKind::ObjectDecodeFailed { message } => {
                            message.len()
                        }
                        super::scene::SceneGraphIssueKind::MissingField { field_name } => {
                            field_name.len()
                        }
                        super::scene::SceneGraphIssueKind::DuplicateField {
                            field_name, ..
                        } => field_name.len(),
                        super::scene::SceneGraphIssueKind::UnexpectedValueType { .. } => 0,
                    };
                    total.checked_add(field_path_bytes)?.checked_add(kind_bytes)
                })
            })
            .and_then(|total| {
                hierarchy_audit
                    .issues()
                    .iter()
                    .try_fold(total, |total, issue| {
                        total.checked_add(issue.field_path.len())
                    })
            })
            .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
                resource: "scene diagnostics",
            })?;

        let next_collection_graph_count = checked_charge(
            self.collection_graph_count,
            1,
            self.limits.max_workspace_collection_graphs,
            "workspace hierarchy collection graphs",
        )?;
        let next_scene_object_count = checked_charge(
            self.scene_object_count,
            scene_objects,
            self.limits.max_scene_objects,
            "scene objects",
        )?;
        let scene_asset_record_count = scene_assets
            .len()
            .checked_add(component_candidates.len())
            .and_then(|count| count.checked_add(missing_transform_plans.len()))
            .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
                resource: "scene asset and generated-transform records",
            })?;
        let next_scene_asset_record_count = checked_charge(
            self.scene_asset_record_count,
            scene_asset_record_count,
            self.limits.max_scene_asset_records,
            "scene asset and generated-transform records",
        )?;
        let next_relationship_count = checked_charge(
            self.relationship_count,
            scene_graph.relationships().len(),
            self.limits.max_relationships,
            "scene relationships",
        )?;
        let next_issue_count = checked_charge(
            self.issue_count,
            issue_count,
            self.limits.max_issues,
            "scene graph issues",
        )?;
        let next_name_bytes = checked_charge(
            self.name_bytes,
            name_bytes,
            self.limits.max_total_name_bytes,
            "scene object name bytes",
        )?;
        let next_diagnostic_bytes = checked_charge(
            self.diagnostic_bytes,
            diagnostic_bytes,
            self.limits.max_total_diagnostic_bytes,
            "scene graph diagnostic bytes",
        )?;

        let mut objects = Vec::new();
        objects.try_reserve_exact(scene_objects).map_err(|_| {
            WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "SceneDefinition scene objects",
                requested: scene_objects,
            }
        })?;
        for object in scene_graph.objects().iter().filter(|object| {
            matches!(
                object.class_id,
                Some(GAME_OBJECT_CLASS_ID | TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
            )
        }) {
            objects.push(copy_scene_object(object)?);
        }
        objects.sort_unstable_by_key(|object| object.object_index);

        let mut relationships = Vec::new();
        relationships
            .try_reserve_exact(scene_graph.relationships().len())
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "SceneDefinition scene relationships",
                requested: scene_graph.relationships().len(),
            })?;
        for relationship in scene_graph.relationships() {
            relationships.push(copy_scene_relationship(relationship)?);
        }

        let (game_object_hierarchies, game_object_component_relationship_indices) =
            build_game_object_component_relationship_index(
                file_index,
                &scene_assets,
                &relationships,
            )?;
        let transform_index =
            build_transform_hierarchy_index(file_index, &component_candidates, &relationships)?;
        let prefab_root_relationship_index =
            build_prefab_root_relationship_index(file_index, &scene_assets, &relationships)?;

        let mut graph_issues = Vec::new();
        graph_issues
            .try_reserve_exact(scene_graph.issues().len())
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "SceneDefinition scene graph diagnostics",
                requested: scene_graph.issues().len(),
            })?;
        for issue in scene_graph.issues() {
            graph_issues.push(copy_scene_graph_issue(issue)?);
        }

        let snapshot = WorkspaceSceneCollectionHierarchy {
            collection_file_index: file_index,
            scene_objects: objects,
            relationships,
            game_object_hierarchies,
            game_object_component_relationship_indices,
            prefab_root_relationship_index,
            transform_hierarchies: transform_index.transforms,
            transform_parent_relationship_indices: transform_index.parent_relationship_indices,
            transform_child_relationship_indices: transform_index.child_relationship_indices,
            transform_game_object_relationship_indices: transform_index
                .game_object_relationship_indices,
            graph_issues,
            scene_assets,
            lighting_data_asset_field_object_indices,
            component_candidates,
            missing_transform_plans,
            source_graph_complete: scene_graph.is_complete(),
            source_reference_scans_complete,
            roots,
            hierarchy_audit,
        };
        self.collection_graphs[file_index] = Some(snapshot);
        self.collection_graph_count = next_collection_graph_count;
        self.scene_object_count = next_scene_object_count;
        self.scene_asset_record_count = next_scene_asset_record_count;
        self.relationship_count = next_relationship_count;
        self.issue_count = next_issue_count;
        self.name_bytes = next_name_bytes;
        self.diagnostic_bytes = next_diagnostic_bytes;
        Ok(())
    }

    pub fn finish(
        mut self,
        definitions: &WorkspaceSceneDefinitions,
    ) -> Result<WorkspaceSceneHierarchies, WorkspaceSceneHierarchyError> {
        if definitions.workspace_collection_count() != self.collection_graphs.len() {
            return Err(
                WorkspaceSceneHierarchyError::WorkspaceDefinitionCollectionCountMismatch {
                    expected: self.collection_graphs.len(),
                    actual: definitions.workspace_collection_count(),
                },
            );
        }
        if definitions.definitions().len() > self.limits.max_scene_definitions {
            return Err(WorkspaceSceneHierarchyError::SceneCountLimitExceeded {
                count: definitions.definitions().len(),
                limit: self.limits.max_scene_definitions,
            });
        }

        let mut scenes = Vec::new();
        scenes
            .try_reserve_exact(definitions.definitions().len())
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "SceneDefinition hierarchy records",
                requested: definitions.definitions().len(),
            })?;
        for definition in definitions.definitions() {
            let mut collections = Vec::new();
            collections
                .try_reserve_exact(definition.collection_file_indices().len())
                .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                    resource: "SceneDefinition collection hierarchy records",
                    requested: definition.collection_file_indices().len(),
                })?;
            for &file_index in definition.collection_file_indices() {
                let Some(slot) = self.collection_graphs.get_mut(file_index) else {
                    return Err(
                        WorkspaceSceneHierarchyError::CollectionFileIndexOutOfBounds {
                            file_index,
                            file_count: self.collection_graphs.len(),
                        },
                    );
                };
                let Some(collection_graph) = slot.take() else {
                    return Err(WorkspaceSceneHierarchyError::MissingCollectionGraph {
                        scene_index: scenes.len(),
                        file_index,
                    });
                };
                collections.push(collection_graph);
            }
            scenes.push(WorkspaceSceneHierarchyScene {
                plan: definition.plan.clone(),
                collections,
            });
        }
        let non_scene_collection_count = self
            .collection_graphs
            .iter()
            .filter(|graph| graph.is_some())
            .count();
        let mut non_scene_collections = Vec::new();
        non_scene_collections
            .try_reserve_exact(non_scene_collection_count)
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "non-scene hierarchy collection graphs",
                requested: non_scene_collection_count,
            })?;
        for graph in self.collection_graphs.iter_mut().flatten() {
            // Non-scene generated-transform plans are exposed through the workspace-wide
            // Missing Prefab Transforms result, not duplicated in this input graph snapshot.
            graph.missing_transform_plans.clear();
        }
        for graph in self.collection_graphs.into_iter().flatten() {
            non_scene_collections.push(graph);
        }
        let collection_graph_index = build_collection_graph_index(
            &scenes,
            &non_scene_collections,
            self.collection_graph_count,
        )?;
        Ok(WorkspaceSceneHierarchies {
            scenes,
            non_scene_collections,
            collection_graph_index,
        })
    }
}

fn build_game_object_component_relationship_index(
    collection_file_index: usize,
    scene_assets: &[WorkspaceSceneAssetIdentity],
    relationships: &[SceneRelationship],
) -> Result<(Vec<WorkspaceSceneGameObjectHierarchy>, Vec<usize>), WorkspaceSceneHierarchyError> {
    let game_object_count = scene_assets
        .iter()
        .filter(|asset| asset.category == WorkspaceSceneAssetCategory::GameObject)
        .count();
    let mut game_objects = Vec::new();
    game_objects
        .try_reserve_exact(game_object_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "SceneDefinition GameObject hierarchy indexes",
            requested: game_object_count,
        })?;
    for asset in scene_assets
        .iter()
        .filter(|asset| asset.category == WorkspaceSceneAssetCategory::GameObject)
    {
        game_objects.push(WorkspaceSceneGameObjectHierarchy {
            game_object: asset.identity,
            component_relationship_start: 0,
            component_relationship_end: 0,
        });
    }
    game_objects.sort_unstable_by_key(|hierarchy| hierarchy.game_object.object_index);
    for pair in game_objects.windows(2) {
        if pair[0].game_object.object_index == pair[1].game_object.object_index {
            return Err(WorkspaceSceneHierarchyError::DuplicateGameObjectHierarchy {
                file_index: collection_file_index,
                object_index: pair[0].game_object.object_index,
            });
        }
    }

    let component_relationship_count = relationships
        .iter()
        .filter(|relationship| relationship.kind == SceneRelationshipKind::GameObjectComponent)
        .count();
    let mut component_relationship_indices = Vec::new();
    component_relationship_indices
        .try_reserve_exact(component_relationship_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "GameObject component relationship indexes",
            requested: component_relationship_count,
        })?;
    component_relationship_indices.extend(relationships.iter().enumerate().filter_map(
        |(relationship_index, relationship)| {
            (relationship.kind == SceneRelationshipKind::GameObjectComponent)
                .then_some(relationship_index)
        },
    ));
    component_relationship_indices.sort_unstable_by_key(|&relationship_index| {
        (
            relationships[relationship_index].source_object_index,
            relationship_index,
        )
    });

    let mut relationship_cursor = 0usize;
    for hierarchy in &mut game_objects {
        let object_index = hierarchy.game_object.object_index;
        if let Some(&relationship_index) = component_relationship_indices.get(relationship_cursor) {
            let relationship = &relationships[relationship_index];
            if relationship.source_object_index < object_index {
                return Err(
                    WorkspaceSceneHierarchyError::GameObjectComponentRelationshipSourceMissing {
                        file_index: collection_file_index,
                        relationship_index,
                        object_index: relationship.source_object_index,
                    },
                );
            }
        }

        hierarchy.component_relationship_start = relationship_cursor;
        while let Some(&relationship_index) =
            component_relationship_indices.get(relationship_cursor)
        {
            if relationships[relationship_index].source_object_index != object_index {
                break;
            }
            relationship_cursor += 1;
        }
        hierarchy.component_relationship_end = relationship_cursor;
    }
    if let Some(&relationship_index) = component_relationship_indices.get(relationship_cursor) {
        return Err(
            WorkspaceSceneHierarchyError::GameObjectComponentRelationshipSourceMissing {
                file_index: collection_file_index,
                relationship_index,
                object_index: relationships[relationship_index].source_object_index,
            },
        );
    }

    Ok((game_objects, component_relationship_indices))
}

fn build_prefab_root_relationship_index(
    collection_file_index: usize,
    scene_assets: &[WorkspaceSceneAssetIdentity],
    relationships: &[SceneRelationship],
) -> Result<Vec<WorkspacePrefabRootRelationshipIndexEntry>, WorkspaceSceneHierarchyError> {
    let relationship_count = relationships
        .iter()
        .filter(|relationship| {
            relationship.kind == SceneRelationshipKind::PrefabInstanceRootGameObject
        })
        .count();
    let mut index = Vec::new();
    index.try_reserve_exact(relationship_count).map_err(|_| {
        WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "PrefabInstance root relationship index",
            requested: relationship_count,
        }
    })?;

    for (relationship_index, relationship) in relationships.iter().enumerate() {
        if relationship.kind != SceneRelationshipKind::PrefabInstanceRootGameObject {
            continue;
        }
        let key = (collection_file_index, relationship.source_object_index);
        let source = scene_assets
            .binary_search_by_key(&key, |asset| {
                (asset.identity.file_index, asset.identity.object_index)
            })
            .ok()
            .and_then(|source_index| scene_assets.get(source_index));
        if !source.is_some_and(|asset| {
            asset.category == WorkspaceSceneAssetCategory::PrefabInstance
                && asset.identity.path_id == relationship.source_path_id
        }) {
            return Err(
                WorkspaceSceneHierarchyError::PrefabRootRelationshipSourceMissing {
                    file_index: collection_file_index,
                    relationship_index,
                    object_index: relationship.source_object_index,
                },
            );
        }
        index.push(WorkspacePrefabRootRelationshipIndexEntry {
            source_object_index: relationship.source_object_index,
            relationship_index,
        });
    }
    index.sort_unstable_by_key(|entry| (entry.source_object_index, entry.relationship_index));
    Ok(index)
}

fn build_transform_hierarchy_index(
    collection_file_index: usize,
    component_candidates: &[WorkspaceSceneComponentCandidate],
    relationships: &[SceneRelationship],
) -> Result<WorkspaceSceneTransformHierarchyIndex, WorkspaceSceneHierarchyError> {
    let transform_count = component_candidates
        .iter()
        .filter(|candidate| candidate.evidence == WorkspaceSceneComponentEvidence::TransformClassId)
        .count();
    let mut transforms = Vec::new();
    transforms.try_reserve_exact(transform_count).map_err(|_| {
        WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "SceneDefinition Transform hierarchy indexes",
            requested: transform_count,
        }
    })?;
    for candidate in component_candidates
        .iter()
        .filter(|candidate| candidate.evidence == WorkspaceSceneComponentEvidence::TransformClassId)
    {
        if candidate.identity.file_index != collection_file_index
            || !matches!(
                candidate.identity.class_id,
                Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
            )
        {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index: candidate.identity.file_index,
                object_index: candidate.identity.object_index,
                path_id: candidate.identity.path_id,
            });
        }
        transforms.push(WorkspaceSceneTransformHierarchy {
            transform: candidate.identity,
            parent_relationship_start: 0,
            parent_relationship_end: 0,
            child_relationship_start: 0,
            child_relationship_end: 0,
            game_object_relationship_start: 0,
            game_object_relationship_end: 0,
        });
    }
    transforms.sort_unstable_by_key(|hierarchy| hierarchy.transform.object_index);
    for pair in transforms.windows(2) {
        if pair[0].transform.object_index == pair[1].transform.object_index {
            return Err(WorkspaceSceneHierarchyError::DuplicateTransformHierarchy {
                file_index: collection_file_index,
                object_index: pair[0].transform.object_index,
            });
        }
    }

    let parent_relationship_count = relationships
        .iter()
        .filter(|relationship| relationship.kind == SceneRelationshipKind::TransformParent)
        .count();
    let child_relationship_count = relationships
        .iter()
        .filter(|relationship| relationship.kind == SceneRelationshipKind::TransformChild)
        .count();
    let game_object_relationship_count = relationships
        .iter()
        .filter(|relationship| {
            relationship.kind == SceneRelationshipKind::ComponentGameObject
                && transforms
                    .binary_search_by_key(&relationship.source_object_index, |hierarchy| {
                        hierarchy.transform.object_index
                    })
                    .is_ok()
        })
        .count();

    let mut parent_relationship_indices = Vec::new();
    parent_relationship_indices
        .try_reserve_exact(parent_relationship_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "Transform parent relationship indexes",
            requested: parent_relationship_count,
        })?;
    let mut child_relationship_indices = Vec::new();
    child_relationship_indices
        .try_reserve_exact(child_relationship_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "Transform child relationship indexes",
            requested: child_relationship_count,
        })?;
    let mut game_object_relationship_indices = Vec::new();
    game_object_relationship_indices
        .try_reserve_exact(game_object_relationship_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "Transform GameObject relationship indexes",
            requested: game_object_relationship_count,
        })?;

    for (relationship_index, relationship) in relationships.iter().enumerate() {
        match relationship.kind {
            SceneRelationshipKind::TransformParent => {
                if transforms
                    .binary_search_by_key(&relationship.source_object_index, |hierarchy| {
                        hierarchy.transform.object_index
                    })
                    .is_err()
                {
                    return Err(
                        WorkspaceSceneHierarchyError::InvalidTransformHierarchySource {
                            file_index: collection_file_index,
                            relationship_index,
                            object_index: relationship.source_object_index,
                        },
                    );
                }
                parent_relationship_indices.push(relationship_index);
            }
            SceneRelationshipKind::TransformChild => {
                if transforms
                    .binary_search_by_key(&relationship.source_object_index, |hierarchy| {
                        hierarchy.transform.object_index
                    })
                    .is_err()
                {
                    return Err(
                        WorkspaceSceneHierarchyError::InvalidTransformHierarchySource {
                            file_index: collection_file_index,
                            relationship_index,
                            object_index: relationship.source_object_index,
                        },
                    );
                }
                child_relationship_indices.push(relationship_index);
            }
            SceneRelationshipKind::ComponentGameObject
                if transforms
                    .binary_search_by_key(&relationship.source_object_index, |hierarchy| {
                        hierarchy.transform.object_index
                    })
                    .is_ok() =>
            {
                game_object_relationship_indices.push(relationship_index)
            }
            _ => {}
        }
    }

    parent_relationship_indices.sort_unstable_by_key(|&relationship_index| {
        (
            relationships[relationship_index].source_object_index,
            relationship_index,
        )
    });
    child_relationship_indices.sort_unstable_by_key(|&relationship_index| {
        (
            relationships[relationship_index].source_object_index,
            relationship_index,
        )
    });
    game_object_relationship_indices.sort_unstable_by_key(|&relationship_index| {
        (
            relationships[relationship_index].source_object_index,
            relationship_index,
        )
    });
    assign_transform_relationship_ranges(
        collection_file_index,
        &mut transforms,
        &parent_relationship_indices,
        relationships,
        TransformRelationshipRange::Parent,
    )?;
    assign_transform_relationship_ranges(
        collection_file_index,
        &mut transforms,
        &child_relationship_indices,
        relationships,
        TransformRelationshipRange::Children,
    )?;
    assign_transform_relationship_ranges(
        collection_file_index,
        &mut transforms,
        &game_object_relationship_indices,
        relationships,
        TransformRelationshipRange::GameObject,
    )?;

    Ok(WorkspaceSceneTransformHierarchyIndex {
        transforms,
        parent_relationship_indices,
        child_relationship_indices,
        game_object_relationship_indices,
    })
}

fn assign_transform_relationship_ranges(
    collection_file_index: usize,
    transforms: &mut [WorkspaceSceneTransformHierarchy],
    relationship_indices: &[usize],
    relationships: &[SceneRelationship],
    range_kind: TransformRelationshipRange,
) -> Result<(), WorkspaceSceneHierarchyError> {
    let mut relationship_cursor = 0usize;
    for hierarchy in transforms {
        let object_index = hierarchy.transform.object_index;
        if let Some(&relationship_index) = relationship_indices.get(relationship_cursor) {
            let source_object_index = relationships[relationship_index].source_object_index;
            if source_object_index < object_index {
                return Err(
                    WorkspaceSceneHierarchyError::InvalidTransformHierarchySource {
                        file_index: collection_file_index,
                        relationship_index,
                        object_index: source_object_index,
                    },
                );
            }
        }
        let start = relationship_cursor;
        while let Some(&relationship_index) = relationship_indices.get(relationship_cursor) {
            if relationships[relationship_index].source_object_index != object_index {
                break;
            }
            relationship_cursor += 1;
        }
        match range_kind {
            TransformRelationshipRange::Parent => {
                hierarchy.parent_relationship_start = start;
                hierarchy.parent_relationship_end = relationship_cursor;
            }
            TransformRelationshipRange::Children => {
                hierarchy.child_relationship_start = start;
                hierarchy.child_relationship_end = relationship_cursor;
            }
            TransformRelationshipRange::GameObject => {
                hierarchy.game_object_relationship_start = start;
                hierarchy.game_object_relationship_end = relationship_cursor;
            }
        }
    }
    if let Some(&relationship_index) = relationship_indices.get(relationship_cursor) {
        return Err(
            WorkspaceSceneHierarchyError::InvalidTransformHierarchySource {
                file_index: collection_file_index,
                relationship_index,
                object_index: relationships[relationship_index].source_object_index,
            },
        );
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum TransformRelationshipRange {
    Parent,
    Children,
    GameObject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TypeTreeComponentBaseClassification {
    Component,
    NotComponent,
    Unavailable,
    Ambiguous,
}

fn classify_type_tree_component_base(
    type_tree: Option<&TypeTree>,
) -> TypeTreeComponentBaseClassification {
    let Some(type_tree) = type_tree else {
        return TypeTreeComponentBaseClassification::Unavailable;
    };
    let Some(root) = type_tree.nodes.first() else {
        return TypeTreeComponentBaseClassification::Ambiguous;
    };
    if root.level != 0 {
        return TypeTreeComponentBaseClassification::Ambiguous;
    }
    if root.type_name.as_deref() == Some("Component") {
        return TypeTreeComponentBaseClassification::Component;
    }

    let mut parent_index = 0usize;
    for _ in 0..type_tree.nodes.len() {
        let parent = &type_tree.nodes[parent_index];
        let Some(child_level) = parent.level.checked_add(1) else {
            return TypeTreeComponentBaseClassification::Ambiguous;
        };
        let mut base_index = None;
        for child_index in (parent_index + 1)..type_tree.nodes.len() {
            let child = &type_tree.nodes[child_index];
            if child.level <= parent.level {
                break;
            }
            if child.level == child_level
                && child.name.as_deref() == Some("Base")
                && base_index.replace(child_index).is_some()
            {
                return TypeTreeComponentBaseClassification::Ambiguous;
            }
        }
        let Some(base_index) = base_index else {
            return TypeTreeComponentBaseClassification::NotComponent;
        };
        let Some(base_type_name) = type_tree.nodes[base_index].type_name.as_deref() else {
            return TypeTreeComponentBaseClassification::Ambiguous;
        };
        if base_type_name == "Component" {
            return TypeTreeComponentBaseClassification::Component;
        }
        parent_index = base_index;
    }
    TypeTreeComponentBaseClassification::Ambiguous
}

struct ClassifiedSceneCollectionObjects {
    scene_assets: Vec<WorkspaceSceneAssetIdentity>,
    lighting_data_asset_field_object_indices: Vec<usize>,
    component_candidates: Vec<WorkspaceSceneComponentCandidate>,
}

fn classify_scene_collection_objects(
    file_index: usize,
    scene_graph: &SceneGraph,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<ClassifiedSceneCollectionObjects, WorkspaceSceneHierarchyError> {
    let scene_asset_count = scene_graph
        .objects()
        .iter()
        .filter(|object| {
            scene_asset_category(
                references
                    .object_class_id(file_index, object.object_index)
                    .or(object.class_id),
            )
            .is_some()
        })
        .count();
    let mut scene_assets = Vec::new();
    scene_assets
        .try_reserve_exact(scene_asset_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "SceneHierarchyObject asset identities",
            requested: scene_asset_count,
        })?;

    let lighting_data_asset_field_object_count = scene_graph
        .objects()
        .iter()
        .filter(|object| {
            references
                .object_class_id(file_index, object.object_index)
                .or(object.class_id)
                == Some(LIGHTMAP_SETTINGS_CLASS_ID)
                && references
                    .references_from_object(file_index, object.object_index)
                    .any(|reference| is_lighting_data_asset_field_path(reference.field_path))
        })
        .count();
    let mut lighting_data_asset_field_object_indices = Vec::new();
    lighting_data_asset_field_object_indices
        .try_reserve_exact(lighting_data_asset_field_object_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "LightmapSettings LightingDataAsset field index",
            requested: lighting_data_asset_field_object_count,
        })?;

    let mut component_candidates = Vec::new();
    let mut candidate_upper_bound = scene_graph
        .objects()
        .iter()
        .filter(|object| {
            matches!(
                references
                    .object_class_id(file_index, object.object_index)
                    .or(object.class_id),
                Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
            )
        })
        .count();
    candidate_upper_bound = scene_graph
        .relationships()
        .iter()
        .filter(|relationship| relationship.kind == SceneRelationshipKind::GameObjectComponent)
        .try_fold(candidate_upper_bound, |count, _| count.checked_add(1))
        .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
            resource: "scene component candidate",
        })?;
    candidate_upper_bound = scene_graph
        .objects()
        .iter()
        .try_fold(candidate_upper_bound, |count, object| {
            let class_id = references
                .object_class_id(file_index, object.object_index)
                .or(object.class_id);
            if scene_asset_category(class_id).is_some()
                || matches!(class_id, Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID))
            {
                return Some(count);
            }
            let pointer_field_count = references
                .references_from_object(file_index, object.object_index)
                .filter(|reference| is_game_object_pointer_path(reference.field_path))
                .count();
            let type_tree_candidate_count = usize::from(matches!(
                classify_type_tree_component_base(
                    references.object_type_tree(file_index, object.object_index,)
                ),
                TypeTreeComponentBaseClassification::Component
                    | TypeTreeComponentBaseClassification::Ambiguous
            ));
            pointer_field_count
                .checked_add(type_tree_candidate_count)
                .and_then(|additional| count.checked_add(additional))
        })
        .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
            resource: "scene component candidate",
        })?;
    component_candidates
        .try_reserve_exact(candidate_upper_bound)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "scene component candidate identities",
            requested: candidate_upper_bound,
        })?;

    for object in scene_graph.objects() {
        let class_id = references
            .object_class_id(file_index, object.object_index)
            .or(object.class_id);
        if class_id == Some(LIGHTMAP_SETTINGS_CLASS_ID)
            && references
                .references_from_object(file_index, object.object_index)
                .any(|reference| is_lighting_data_asset_field_path(reference.field_path))
        {
            lighting_data_asset_field_object_indices.push(object.object_index);
        }
        if let Some(category) = scene_asset_category(class_id) {
            let identity = scene_object_identity(
                file_index,
                object.object_index,
                object.path_id,
                class_id,
                references,
            )?;
            scene_assets.push(WorkspaceSceneAssetIdentity { identity, category });
        }
        if matches!(class_id, Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)) {
            let identity = scene_object_identity(
                file_index,
                object.object_index,
                object.path_id,
                class_id,
                references,
            )?;
            component_candidates.push(WorkspaceSceneComponentCandidate {
                identity,
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            });
        }

        if scene_asset_category(class_id).is_none()
            && !matches!(class_id, Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID))
        {
            let type_tree_evidence = match classify_type_tree_component_base(
                references.object_type_tree(file_index, object.object_index),
            ) {
                TypeTreeComponentBaseClassification::Component => {
                    Some(WorkspaceSceneComponentEvidence::TypeTreeComponentBase)
                }
                TypeTreeComponentBaseClassification::Ambiguous => {
                    Some(WorkspaceSceneComponentEvidence::AmbiguousTypeTreeBase)
                }
                TypeTreeComponentBaseClassification::NotComponent
                | TypeTreeComponentBaseClassification::Unavailable => None,
            };
            if let Some(evidence) = type_tree_evidence {
                let identity = scene_object_identity(
                    file_index,
                    object.object_index,
                    object.path_id,
                    class_id,
                    references,
                )?;
                component_candidates.push(WorkspaceSceneComponentCandidate { identity, evidence });
            }

            for reference in references.references_from_object(file_index, object.object_index) {
                if !is_game_object_pointer_path(reference.field_path) {
                    continue;
                }
                let identity = scene_object_identity(
                    file_index,
                    object.object_index,
                    object.path_id,
                    class_id,
                    references,
                )?;
                let evidence = if reference.path_id == 0 {
                    WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull
                } else {
                    WorkspaceSceneComponentEvidence::GameObjectPointerField
                };
                component_candidates.push(WorkspaceSceneComponentCandidate { identity, evidence });
            }
        }
    }

    for relationship in scene_graph
        .relationships()
        .iter()
        .filter(|relationship| relationship.kind == SceneRelationshipKind::GameObjectComponent)
    {
        let (target_file_index, target_object_index) = match &relationship.target {
            SceneRelationshipTarget::LocalObject { object_index } => (file_index, *object_index),
            SceneRelationshipTarget::WorkspaceObject {
                file_index,
                object_index,
            } => (*file_index, *object_index),
            _ => continue,
        };
        let class_id = references.object_class_id(target_file_index, target_object_index);
        if target_file_index == file_index
            && matches!(class_id, Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID))
        {
            continue;
        }
        let identity = scene_object_identity(
            target_file_index,
            target_object_index,
            relationship.path_id,
            class_id,
            references,
        )
        .map_err(|error| match error {
            WorkspaceSceneHierarchyError::InvalidSourceIdentity { .. } => {
                WorkspaceSceneHierarchyError::InvalidComponentTarget {
                    source_file_index: file_index,
                    target_file_index,
                    object_index: target_object_index,
                }
            }
            other => other,
        })?;
        if identity.path_id != relationship.path_id {
            return Err(WorkspaceSceneHierarchyError::InvalidComponentTarget {
                source_file_index: file_index,
                target_file_index,
                object_index: target_object_index,
            });
        }
        component_candidates.push(WorkspaceSceneComponentCandidate {
            identity,
            evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
        });
    }

    scene_assets
        .sort_unstable_by_key(|asset| (asset.identity.file_index, asset.identity.object_index));
    lighting_data_asset_field_object_indices.sort_unstable();
    lighting_data_asset_field_object_indices.dedup();
    normalize_component_candidates(&mut component_candidates);
    Ok(ClassifiedSceneCollectionObjects {
        scene_assets,
        lighting_data_asset_field_object_indices,
        component_candidates,
    })
}

fn is_lighting_data_asset_field_path(field_path: &str) -> bool {
    field_path
        .rsplit('.')
        .next()
        .is_some_and(|field_name| field_name == "m_LightingDataAsset")
}

fn scene_asset_category(class_id: Option<i32>) -> Option<WorkspaceSceneAssetCategory> {
    match class_id {
        Some(GAME_OBJECT_CLASS_ID) => Some(WorkspaceSceneAssetCategory::GameObject),
        Some(PREFAB_INSTANCE_CLASS_ID) => Some(WorkspaceSceneAssetCategory::PrefabInstance),
        Some(SCENE_ROOTS_CLASS_ID) => Some(WorkspaceSceneAssetCategory::SceneRoots),
        manager => scene_manager_kind(manager).map(WorkspaceSceneAssetCategory::LevelGameManager),
    }
}

fn scene_object_identity(
    file_index: usize,
    object_index: usize,
    path_id: i64,
    class_id: Option<i32>,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<WorkspaceSceneObjectIdentity, WorkspaceSceneHierarchyError> {
    let Some(identity) = references.object_identity(file_index, object_index) else {
        return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
            file_index,
            object_index,
            path_id,
        });
    };
    if identity.path_id != path_id {
        return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
            file_index,
            object_index,
            path_id,
        });
    }
    Ok(WorkspaceSceneObjectIdentity {
        file_index,
        object_index,
        path_id: identity.path_id,
        class_id,
    })
}

fn build_missing_transform_plans(
    file_index: usize,
    roots: &WorkspaceSceneRootDiscovery,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<Vec<WorkspaceMissingTransformPlan>, WorkspaceSceneHierarchyError> {
    let game_objects = missing_transform_game_objects(file_index, roots, references)?;
    let mut plans = Vec::new();
    plans.try_reserve_exact(game_objects.len()).map_err(|_| {
        WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "missing scene Transform plans",
            requested: game_objects.len(),
        }
    })?;
    for game_object in game_objects {
        plans.push(WorkspaceMissingTransformPlan {
            game_object,
            default_values: default_injected_transform_values(),
        });
    }
    Ok(plans)
}

fn missing_transform_game_objects(
    file_index: usize,
    roots: &WorkspaceSceneRootDiscovery,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<Vec<WorkspaceSceneObjectIdentity>, WorkspaceSceneHierarchyError> {
    let game_object_count = roots
        .roots()
        .iter()
        .filter(|root| root.reason == SceneRootReason::NoTransformComponent)
        .count();
    let mut game_objects = Vec::new();
    game_objects
        .try_reserve_exact(game_object_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "missing-Transform GameObject identities",
            requested: game_object_count,
        })?;
    for root in roots
        .roots()
        .iter()
        .filter(|root| root.reason == SceneRootReason::NoTransformComponent)
    {
        let is_game_object = references
            .object_class_id(file_index, root.game_object_index)
            .is_some_and(|class_id| class_id == GAME_OBJECT_CLASS_ID);
        if root.source_file_index != file_index
            || !is_game_object
            || root.transform_file_index.is_some()
            || root.transform_index.is_some()
        {
            return Err(WorkspaceSceneHierarchyError::InvalidRootIdentity {
                file_index,
                object_index: root.game_object_index,
            });
        }
        let Some(source_identity) = references.object_identity(file_index, root.game_object_index)
        else {
            return Err(WorkspaceSceneHierarchyError::InvalidRootIdentity {
                file_index,
                object_index: root.game_object_index,
            });
        };
        game_objects.push(scene_object_identity(
            file_index,
            root.game_object_index,
            source_identity.path_id,
            Some(GAME_OBJECT_CLASS_ID),
            references,
        )?);
    }
    Ok(game_objects)
}

fn default_injected_transform_values() -> WorkspaceGeneratedTransformDefaults {
    WorkspaceGeneratedTransformDefaults {
        local_position: SceneVector3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        local_rotation: SceneQuaternion {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        local_scale: SceneVector3 {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
        root_order: 0,
        local_euler_angles_hint_if_present: SceneVector3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    }
}

/// Returns the baseline `TransformExtensions.InitializeDefault` values for missing-Transform
/// planning. No generated Unity object or PathID is allocated by this function.
pub fn default_workspace_generated_transform_values() -> WorkspaceGeneratedTransformDefaults {
    default_injected_transform_values()
}

fn is_game_object_pointer_path(field_path: &str) -> bool {
    let mut path = field_path;
    loop {
        if path == "m_GameObject" {
            return true;
        }
        let Some(base) = path.strip_prefix("Base.") else {
            return false;
        };
        path = base;
    }
}

const fn component_evidence_order(evidence: WorkspaceSceneComponentEvidence) -> u8 {
    match evidence {
        WorkspaceSceneComponentEvidence::TransformClassId => 0,
        WorkspaceSceneComponentEvidence::TypeTreeComponentBase => 1,
        WorkspaceSceneComponentEvidence::GameObjectComponentReference => 2,
        WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull => 3,
        WorkspaceSceneComponentEvidence::GameObjectPointerField => 4,
        WorkspaceSceneComponentEvidence::AmbiguousTypeTreeBase => 5,
    }
}

fn normalize_component_candidates(candidates: &mut Vec<WorkspaceSceneComponentCandidate>) {
    candidates.sort_unstable_by_key(|candidate| {
        (
            candidate.identity.file_index,
            candidate.identity.object_index,
            component_evidence_order(candidate.evidence),
        )
    });
    // Keep distinct evidence kinds for the same object. In particular, baseline Scene
    // membership requires the non-null m_GameObject evidence for MonoBehaviour assets.
    candidates.dedup_by_key(|candidate| {
        (
            candidate.identity.file_index,
            candidate.identity.object_index,
            candidate.evidence,
        )
    });
}

fn validate_collection_graph(
    file_index: usize,
    scene_graph: &SceneGraph,
    roots: &WorkspaceSceneRootDiscovery,
    hierarchy_audit: &WorkspaceSceneHierarchyAudit,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<(), WorkspaceSceneHierarchyError> {
    for object in scene_graph.objects() {
        let Some(identity) = references.object_identity(file_index, object.object_index) else {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index,
                object_index: object.object_index,
                path_id: object.path_id,
            });
        };
        if identity.path_id != object.path_id {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index,
                object_index: object.object_index,
                path_id: object.path_id,
            });
        }
    }
    for (relationship_index, relationship) in scene_graph.relationships().iter().enumerate() {
        let Some(source) = scene_graph.objects().get(relationship.source_object_index) else {
            return Err(WorkspaceSceneHierarchyError::InvalidRelationshipSource {
                file_index,
                relationship_index,
                object_index: relationship.source_object_index,
            });
        };
        if source.path_id != relationship.source_path_id {
            return Err(WorkspaceSceneHierarchyError::InvalidRelationshipSource {
                file_index,
                relationship_index,
                object_index: relationship.source_object_index,
            });
        }
        let Some(identity) =
            references.object_identity(file_index, relationship.source_object_index)
        else {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index,
                object_index: relationship.source_object_index,
                path_id: relationship.source_path_id,
            });
        };
        if identity.path_id != relationship.source_path_id {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index,
                object_index: relationship.source_object_index,
                path_id: relationship.source_path_id,
            });
        }
    }

    for root in roots.roots() {
        if root.source_file_index != file_index {
            return Err(WorkspaceSceneHierarchyError::RootSourceFileMismatch {
                expected: file_index,
                actual: root.source_file_index,
            });
        }
        let is_game_object = scene_graph
            .objects()
            .get(root.game_object_index)
            .is_some_and(|object| object.class_id == Some(GAME_OBJECT_CLASS_ID));
        if !is_game_object
            || references
                .object_identity(file_index, root.game_object_index)
                .is_none()
        {
            return Err(WorkspaceSceneHierarchyError::InvalidRootIdentity {
                file_index,
                object_index: root.game_object_index,
            });
        }
        if let (Some(transform_file_index), Some(transform_index)) =
            (root.transform_file_index, root.transform_index)
        {
            let is_transform = references
                .object_class_id(transform_file_index, transform_index)
                .is_some_and(|class_id| {
                    matches!(class_id, TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                });
            if !is_transform {
                return Err(WorkspaceSceneHierarchyError::InvalidRootTransformIdentity {
                    file_index,
                    transform_file_index,
                    transform_index,
                });
            }
        } else if root.reason != SceneRootReason::NoTransformComponent {
            return Err(WorkspaceSceneHierarchyError::InvalidRootIdentity {
                file_index,
                object_index: root.game_object_index,
            });
        }
    }
    for issue in roots.issues() {
        if issue.source_file_index != file_index {
            return Err(WorkspaceSceneHierarchyError::RootSourceFileMismatch {
                expected: file_index,
                actual: issue.source_file_index,
            });
        }
    }
    for issue in hierarchy_audit.issues() {
        if issue.source_file_index != file_index {
            return Err(WorkspaceSceneHierarchyError::AuditSourceFileMismatch {
                expected: file_index,
                actual: issue.source_file_index,
            });
        }
    }
    Ok(())
}

fn checked_charge(
    current: usize,
    amount: usize,
    limit: usize,
    resource: &'static str,
) -> Result<usize, WorkspaceSceneHierarchyError> {
    let requested = current
        .checked_add(amount)
        .ok_or(WorkspaceSceneHierarchyError::CountOverflow { resource })?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyError::ResourceLimitExceeded {
            resource,
            requested,
            limit,
        });
    }
    Ok(requested)
}

fn copy_scene_object(
    object: &SceneObjectRecord,
) -> Result<SceneObjectRecord, WorkspaceSceneHierarchyError> {
    let name_bytes = if let Some(source) = &object.name_bytes {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(source.len()).map_err(|_| {
            WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "scene object name bytes",
                requested: source.len(),
            }
        })?;
        bytes.extend_from_slice(source);
        Some(bytes)
    } else {
        None
    };
    Ok(SceneObjectRecord {
        object_index: object.object_index,
        path_id: object.path_id,
        class_id: object.class_id,
        name_bytes,
        is_active: object.is_active,
        transform: object.transform.clone(),
    })
}

fn copy_scene_relationship(
    relationship: &SceneRelationship,
) -> Result<SceneRelationship, WorkspaceSceneHierarchyError> {
    Ok(SceneRelationship {
        source_object_index: relationship.source_object_index,
        source_path_id: relationship.source_path_id,
        kind: relationship.kind,
        field_path: try_copy_string(&relationship.field_path, "scene relationship field path")?,
        file_id: relationship.file_id,
        path_id: relationship.path_id,
        target: relationship.target.clone(),
    })
}

fn copy_scene_graph_issue(
    issue: &SceneGraphIssue,
) -> Result<SceneGraphIssue, WorkspaceSceneHierarchyError> {
    use super::scene::SceneGraphIssueKind;

    let kind = match &issue.kind {
        SceneGraphIssueKind::ObjectDecodeFailed { message } => {
            SceneGraphIssueKind::ObjectDecodeFailed {
                message: try_copy_string(message, "scene object decode diagnostic")?,
            }
        }
        SceneGraphIssueKind::MissingField { field_name } => SceneGraphIssueKind::MissingField {
            field_name: try_copy_string(field_name, "scene missing-field diagnostic")?,
        },
        SceneGraphIssueKind::DuplicateField {
            field_name,
            match_count,
        } => SceneGraphIssueKind::DuplicateField {
            field_name: try_copy_string(field_name, "scene duplicate-field diagnostic")?,
            match_count: *match_count,
        },
        SceneGraphIssueKind::UnexpectedValueType { expected, actual } => {
            SceneGraphIssueKind::UnexpectedValueType { expected, actual }
        }
    };
    Ok(SceneGraphIssue {
        object_index: issue.object_index,
        path_id: issue.path_id,
        field_path: issue
            .field_path
            .as_ref()
            .map(|path| try_copy_string(path, "scene diagnostic field path"))
            .transpose()?,
        kind,
    })
}

fn try_copy_string(
    source: &str,
    resource: &'static str,
) -> Result<String, WorkspaceSceneHierarchyError> {
    let mut copy = String::new();
    copy.try_reserve_exact(source.len()).map_err(|_| {
        WorkspaceSceneHierarchyError::AllocationFailed {
            resource,
            requested: source.len(),
        }
    })?;
    copy.push_str(source);
    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::prefab_processing::{
        plan_workspace_prefab_processing, plan_workspace_processed_prefab_assets,
        WorkspacePrefabProcessingEvent, WorkspaceProcessedCollectionKind,
        WorkspaceProcessedPrefabAssetKind, WorkspaceProcessedPrefabAssetPlanLimits,
        WorkspaceProcessedPrefabReference, WorkspaceSourceDependencyAppendTarget,
        WorkspaceSourceDependencyPointerField,
    };
    use crate::processing::scene_collections::{
        SceneDefinitionGuidPlan, SceneDefinitionPathSource, SceneDefinitionPlan,
        SceneGuidGenerationReason, SceneManagerKind,
    };
    use crate::serialized_file::metadata::TypeTreeNode;

    fn identity(
        file_index: usize,
        object_index: usize,
        path_id: i64,
        class_id: i32,
    ) -> WorkspaceSceneObjectIdentity {
        WorkspaceSceneObjectIdentity {
            file_index,
            object_index,
            path_id,
            class_id: Some(class_id),
        }
    }

    fn relationship(
        source: WorkspaceSceneObjectIdentity,
        kind: SceneRelationshipKind,
        field_path: &str,
        path_id: i64,
        target: SceneRelationshipTarget,
    ) -> SceneRelationship {
        SceneRelationship {
            source_object_index: source.object_index,
            source_path_id: source.path_id,
            kind,
            field_path: field_path.to_owned(),
            file_id: 0,
            path_id,
            target,
        }
    }

    fn empty_collection_graph(file_index: usize) -> WorkspaceSceneCollectionHierarchy {
        WorkspaceSceneCollectionHierarchy {
            collection_file_index: file_index,
            scene_objects: Vec::new(),
            relationships: Vec::new(),
            game_object_hierarchies: Vec::new(),
            game_object_component_relationship_indices: Vec::new(),
            prefab_root_relationship_index: Vec::new(),
            transform_hierarchies: Vec::new(),
            transform_parent_relationship_indices: Vec::new(),
            transform_child_relationship_indices: Vec::new(),
            transform_game_object_relationship_indices: Vec::new(),
            graph_issues: Vec::new(),
            scene_assets: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            component_candidates: Vec::new(),
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        }
    }

    fn scene_definition_plan(file_index: usize) -> SceneDefinitionPlan {
        SceneDefinitionPlan {
            collection_file_index: file_index,
            collection_name: "scene".to_owned(),
            name: "scene".to_owned(),
            path: "Assets/Scenes/scene".to_owned(),
            path_source: SceneDefinitionPathSource::CollectionNameFallback,
            guid: SceneDefinitionGuidPlan::GenerateAtExport(
                SceneGuidGenerationReason::MissingFromInput,
            ),
        }
    }

    #[test]
    fn component_candidate_normalization_keeps_distinct_evidence_for_one_object() {
        let mono_behaviour = identity(0, 1, 11, MONO_BEHAVIOUR_CLASS_ID);
        let direct = WorkspaceSceneComponentCandidate {
            identity: mono_behaviour,
            evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
        };
        let back_reference = WorkspaceSceneComponentCandidate {
            identity: mono_behaviour,
            evidence: WorkspaceSceneComponentEvidence::GameObjectPointerField,
        };
        let null_back_reference = WorkspaceSceneComponentCandidate {
            identity: mono_behaviour,
            evidence: WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull,
        };
        let mut candidates = vec![back_reference, direct, null_back_reference, direct];

        normalize_component_candidates(&mut candidates);

        assert_eq!(
            candidates,
            vec![direct, null_back_reference, back_reference]
        );
    }

    #[test]
    fn type_tree_base_chain_classifies_component_inheritance_without_guessing() {
        let tree = |nodes: &[(u8, &str, Option<&str>)]| TypeTree {
            nodes: nodes
                .iter()
                .map(|(level, name, type_name)| TypeTreeNode {
                    level: *level,
                    version: 1,
                    type_flags: 0,
                    type_string_offset: None,
                    name_string_offset: None,
                    type_name: type_name.map(str::to_owned),
                    name: Some((*name).to_owned()),
                    byte_size: -1,
                    index: 0,
                    meta_flags: 0,
                    reference_type_hash: None,
                })
                .collect(),
            string_buffer: Vec::new(),
        };
        let mesh_renderer = tree(&[
            (0, "", Some("MeshRenderer")),
            (1, "Base", Some("Renderer")),
            (2, "Base", Some("Component")),
            (3, "m_GameObject", Some("PPtr<GameObject>")),
        ]);
        let scriptable_object = tree(&[
            (0, "", Some("ExampleAsset")),
            (1, "Base", Some("ScriptableObject")),
            (2, "Base", Some("Object")),
        ]);
        let duplicate_base = tree(&[
            (0, "", Some("MalformedAsset")),
            (1, "Base", Some("Component")),
            (1, "Base", Some("Object")),
        ]);
        let missing_base_type = tree(&[(0, "", Some("MalformedAsset")), (1, "Base", None)]);

        assert_eq!(
            classify_type_tree_component_base(Some(&mesh_renderer)),
            TypeTreeComponentBaseClassification::Component
        );
        assert_eq!(
            classify_type_tree_component_base(Some(&scriptable_object)),
            TypeTreeComponentBaseClassification::NotComponent
        );
        assert_eq!(
            classify_type_tree_component_base(Some(&duplicate_base)),
            TypeTreeComponentBaseClassification::Ambiguous
        );
        assert_eq!(
            classify_type_tree_component_base(Some(&missing_base_type)),
            TypeTreeComponentBaseClassification::Ambiguous
        );
        assert_eq!(
            classify_type_tree_component_base(None),
            TypeTreeComponentBaseClassification::Unavailable
        );
    }

    #[test]
    fn scene_asset_membership_preserves_source_order_and_reports_weak_component_evidence() {
        let mut first = empty_collection_graph(8);
        let first_game_object = identity(8, 0, 80, GAME_OBJECT_CLASS_ID);
        let another_first_collection_game_object = identity(8, 4, 84, GAME_OBJECT_CLASS_ID);
        let first_prefab_instance = identity(8, 5, 85, PREFAB_INSTANCE_CLASS_ID);
        let first_manager = identity(8, 6, 86, SceneManagerKind::RenderSettings.class_id());
        let first_roots = identity(8, 7, 87, SCENE_ROOTS_CLASS_ID);
        let second_roots = identity(8, 8, 88, SCENE_ROOTS_CLASS_ID);
        first.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: first_game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: first_prefab_instance,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            },
            WorkspaceSceneAssetIdentity {
                identity: first_manager,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::RenderSettings,
                ),
            },
            WorkspaceSceneAssetIdentity {
                identity: first_roots,
                category: WorkspaceSceneAssetCategory::SceneRoots,
            },
            WorkspaceSceneAssetIdentity {
                identity: another_first_collection_game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: second_roots,
                category: WorkspaceSceneAssetCategory::SceneRoots,
            },
        ];
        first.component_candidates = vec![
            WorkspaceSceneComponentCandidate {
                identity: identity(3, 5, 35, 23),
                evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            },
            WorkspaceSceneComponentCandidate {
                identity: identity(8, 2, 82, TRANSFORM_CLASS_ID),
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            },
        ];

        let mut second = empty_collection_graph(3);
        let second_game_object = identity(3, 1, 31, GAME_OBJECT_CLASS_ID);
        let second_prefab_instance = identity(3, 2, 32, PREFAB_INSTANCE_CLASS_ID);
        let second_manager = identity(3, 3, 33, SceneManagerKind::LightmapSettings.class_id());
        second.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: second_game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: second_prefab_instance,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            },
            WorkspaceSceneAssetIdentity {
                identity: second_manager,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::LightmapSettings,
                ),
            },
        ];
        second.component_candidates = vec![
            WorkspaceSceneComponentCandidate {
                identity: identity(8, 1, 81, 23),
                evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            },
            WorkspaceSceneComponentCandidate {
                identity: identity(3, 4, 34, TRANSFORM_CLASS_ID),
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            },
            WorkspaceSceneComponentCandidate {
                identity: identity(3, 6, 36, 23),
                evidence: WorkspaceSceneComponentEvidence::GameObjectPointerField,
            },
        ];
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(8),
            collections: vec![first, second],
        };

        let membership = scene
            .asset_membership(WorkspaceSceneHierarchyMembershipLimits::default())
            .unwrap();
        assert_eq!(
            membership.game_objects(),
            &[
                first_game_object,
                another_first_collection_game_object,
                second_game_object
            ]
        );
        assert_eq!(
            membership.prefab_instances(),
            &[first_prefab_instance, second_prefab_instance]
        );
        assert_eq!(membership.managers(), &[first_manager, second_manager]);
        assert_eq!(membership.scene_roots(), Some(second_roots));
        assert_eq!(
            membership.components(),
            &[
                identity(8, 1, 81, 23),
                identity(8, 2, 82, TRANSFORM_CLASS_ID),
                identity(3, 4, 34, TRANSFORM_CLASS_ID),
                identity(3, 5, 35, 23),
            ]
        );
        assert!(!membership.is_complete());
        assert!(membership.issues().contains(
            &WorkspaceSceneHierarchyMembershipIssue::UnclassifiedComponentCandidate {
                identity: identity(3, 6, 36, 23),
                evidence: WorkspaceSceneComponentEvidence::GameObjectPointerField,
            }
        ));
        assert!(matches!(
            crate::export::scene_collection::plan_workspace_scene_export_ids(
                &scene,
                crate::processing::scene_naming::SceneUnityVersion {
                    major: 2019,
                    minor: 4,
                    patch: 0,
                },
                WorkspaceSceneHierarchyMembershipLimits::default(),
                crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
                crate::export::scene_collection::SceneExportIdPlanLimits::default(),
            ),
            Err(crate::export::scene_collection::WorkspaceSceneExportPlanError::IncompleteMembership { .. })
        ));
    }

    #[test]
    fn scene_asset_membership_marks_incomplete_scans_and_enforces_asset_limit() {
        let mut collection = empty_collection_graph(0);
        collection.source_reference_scans_complete = false;
        collection.scene_assets.push(WorkspaceSceneAssetIdentity {
            identity: identity(0, 0, 10, GAME_OBJECT_CLASS_ID),
            category: WorkspaceSceneAssetCategory::GameObject,
        });
        collection.scene_assets.push(WorkspaceSceneAssetIdentity {
            identity: identity(0, 1, 11, GAME_OBJECT_CLASS_ID),
            category: WorkspaceSceneAssetCategory::GameObject,
        });
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![collection],
        };
        let membership = scene
            .asset_membership(WorkspaceSceneHierarchyMembershipLimits::default())
            .unwrap();
        assert_eq!(
            membership.issues(),
            &[WorkspaceSceneHierarchyMembershipIssue::IncompleteReferenceScans { file_index: 0 }]
        );
        assert!(matches!(
            scene.asset_membership(WorkspaceSceneHierarchyMembershipLimits {
                max_assets: 1,
                max_issues: 10,
            }),
            Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
                requested: 2,
                limit: 1,
            })
        ));
    }

    #[test]
    fn workspace_scene_export_plan_uses_baseline_category_order_before_yaml_sorting() {
        let game_object = identity(5, 0, 50, GAME_OBJECT_CLASS_ID);
        let prefab_instance = identity(5, 1, 51, PREFAB_INSTANCE_CLASS_ID);
        let manager = identity(5, 2, 52, SceneManagerKind::RenderSettings.class_id());
        let scene_roots = identity(5, 3, 53, SCENE_ROOTS_CLASS_ID);
        let transform = identity(5, 4, 54, TRANSFORM_CLASS_ID);
        let renderer = identity(5, 5, 55, 23);
        let mono_behaviour = identity(5, 6, 56, MONO_BEHAVIOUR_CLASS_ID);
        let mut collection = empty_collection_graph(5);
        collection.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: prefab_instance,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            },
            WorkspaceSceneAssetIdentity {
                identity: manager,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::RenderSettings,
                ),
            },
            WorkspaceSceneAssetIdentity {
                identity: scene_roots,
                category: WorkspaceSceneAssetCategory::SceneRoots,
            },
        ];
        collection.component_candidates = vec![
            WorkspaceSceneComponentCandidate {
                identity: transform,
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            },
            WorkspaceSceneComponentCandidate {
                identity: renderer,
                evidence: WorkspaceSceneComponentEvidence::TypeTreeComponentBase,
            },
            WorkspaceSceneComponentCandidate {
                identity: mono_behaviour,
                evidence: WorkspaceSceneComponentEvidence::GameObjectPointerField,
            },
        ];
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(5),
            collections: vec![collection],
        };
        let assignments = crate::export::scene_collection::plan_workspace_scene_export_ids(
            &scene,
            crate::processing::scene_naming::SceneUnityVersion {
                major: 2019,
                minor: 4,
                patch: 0,
            },
            WorkspaceSceneHierarchyMembershipLimits::default(),
            crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
            crate::export::scene_collection::SceneExportIdPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| assignment.asset.object_index)
                .collect::<Vec<_>>(),
            [0, 4, 5, 6, 1, 2, 3]
        );
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| assignment.file_id)
                .collect::<Vec<_>>(),
            [50, 54, 55, 56, 51, 52, 53]
        );
        let yaml_order = crate::export::scene_collection::order_scene_yaml_assignments(
            &assignments,
            crate::export::scene_collection::SceneYamlAssetOrderLimits::default(),
        )
        .unwrap();
        assert_eq!(
            yaml_order
                .iter()
                .map(|assignment| assignment.asset.object_index)
                .collect::<Vec<_>>(),
            [2, 0, 4, 5, 6, 1, 3]
        );
    }

    #[test]
    fn mono_behaviour_requires_baseline_non_null_game_object_evidence() {
        let game_object = identity(0, 0, 10, GAME_OBJECT_CLASS_ID);
        let mono_behaviour = identity(0, 1, 11, MONO_BEHAVIOUR_CLASS_ID);
        let mut collection = empty_collection_graph(0);
        collection.scene_assets.push(WorkspaceSceneAssetIdentity {
            identity: game_object,
            category: WorkspaceSceneAssetCategory::GameObject,
        });
        collection
            .component_candidates
            .push(WorkspaceSceneComponentCandidate {
                identity: mono_behaviour,
                evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            });
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![collection],
        };

        let membership = scene
            .asset_membership(WorkspaceSceneHierarchyMembershipLimits::default())
            .unwrap();
        assert!(!membership.components().contains(&mono_behaviour));
        assert!(membership.issues().contains(
            &WorkspaceSceneHierarchyMembershipIssue::MonoBehaviourGameObjectReferenceUnavailable {
                identity: mono_behaviour,
            }
        ));
        assert!(!membership.is_complete());

        let mut null_pointer_collection = empty_collection_graph(0);
        null_pointer_collection
            .scene_assets
            .push(WorkspaceSceneAssetIdentity {
                identity: game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            });
        null_pointer_collection.component_candidates = vec![
            WorkspaceSceneComponentCandidate {
                identity: mono_behaviour,
                evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            },
            WorkspaceSceneComponentCandidate {
                identity: mono_behaviour,
                evidence: WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull,
            },
        ];
        let null_pointer_scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![null_pointer_collection],
        };
        let null_pointer_membership = null_pointer_scene
            .asset_membership(WorkspaceSceneHierarchyMembershipLimits::default())
            .unwrap();
        assert!(!null_pointer_membership
            .components()
            .contains(&mono_behaviour));
        assert!(null_pointer_membership.issues().is_empty());
        assert!(null_pointer_membership.is_complete());
    }

    #[test]
    fn lighting_data_dependency_owner_matches_first_lightmap_settings_and_render_settings() {
        let first = identity(0, 0, 10, LIGHTMAP_SETTINGS_CLASS_ID);
        let second = identity(0, 1, 11, LIGHTMAP_SETTINGS_CLASS_ID);
        let render = identity(0, 2, 12, SceneManagerKind::RenderSettings.class_id());
        let mut collection = empty_collection_graph(0);
        collection.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: first,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::LightmapSettings,
                ),
            },
            WorkspaceSceneAssetIdentity {
                identity: second,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::LightmapSettings,
                ),
            },
            WorkspaceSceneAssetIdentity {
                identity: render,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::RenderSettings,
                ),
            },
        ];
        collection.lighting_data_asset_field_object_indices = vec![second.object_index];
        let mut scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![collection],
        };

        // The source selects the first LightmapSettings; a later field-bearing instance is not used.
        assert_eq!(scene.lighting_data_dependency_owner(), None);
        scene.collections[0].lighting_data_asset_field_object_indices =
            vec![first.object_index, second.object_index];
        assert_eq!(scene.lighting_data_dependency_owner(), Some(first));
        scene.collections[0].scene_assets.retain(|asset| {
            asset.category
                != WorkspaceSceneAssetCategory::LevelGameManager(SceneManagerKind::RenderSettings)
        });
        assert_eq!(scene.lighting_data_dependency_owner(), None);
    }

    #[test]
    fn lighting_data_asset_field_match_uses_the_terminal_typed_field_name() {
        assert!(is_lighting_data_asset_field_path("m_LightingDataAsset"));
        assert!(is_lighting_data_asset_field_path(
            "LightmapSettings.m_LightingDataAsset"
        ));
        assert!(!is_lighting_data_asset_field_path(
            "m_LightingDataAssetBackup"
        ));
        assert!(!is_lighting_data_asset_field_path("m_LightmapData"));
    }

    fn workspace_hierarchy_fixture(cycle: bool) -> WorkspaceSceneHierarchies {
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let first_transform = identity(0, 1, 2, TRANSFORM_CLASS_ID);
        let last_transform = identity(0, 2, 3, RECT_TRANSFORM_CLASS_ID);
        let renderer = identity(0, 3, 4, 23);
        let child_transform = identity(1, 0, 10, TRANSFORM_CLASS_ID);
        let child_game_object = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);

        let root_relationships = vec![
            relationship(
                root,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[0]",
                renderer.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 3 },
            ),
            relationship(
                root,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[1]",
                0,
                SceneRelationshipTarget::Null,
            ),
            relationship(
                root,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[2]",
                first_transform.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 1 },
            ),
            relationship(
                root,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[3]",
                last_transform.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 2 },
            ),
            relationship(
                last_transform,
                SceneRelationshipKind::TransformChild,
                "m_Children[0]",
                child_transform.path_id,
                SceneRelationshipTarget::WorkspaceObject {
                    file_index: 1,
                    object_index: 0,
                },
            ),
            relationship(
                last_transform,
                SceneRelationshipKind::ComponentGameObject,
                "m_GameObject",
                root.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 0 },
            ),
            relationship(
                first_transform,
                SceneRelationshipKind::ComponentGameObject,
                "m_GameObject",
                root.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 0 },
            ),
            relationship(
                first_transform,
                SceneRelationshipKind::TransformParent,
                "m_Father",
                0,
                SceneRelationshipTarget::Null,
            ),
            relationship(
                last_transform,
                SceneRelationshipKind::TransformParent,
                "m_Father",
                0,
                SceneRelationshipTarget::Null,
            ),
        ];
        let root_collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 0,
            scene_objects: Vec::new(),
            relationships: root_relationships,
            game_object_hierarchies: vec![WorkspaceSceneGameObjectHierarchy {
                game_object: root,
                component_relationship_start: 0,
                component_relationship_end: 4,
            }],
            game_object_component_relationship_indices: vec![0, 1, 2, 3],
            prefab_root_relationship_index: Vec::new(),
            transform_hierarchies: vec![
                WorkspaceSceneTransformHierarchy {
                    transform: first_transform,
                    parent_relationship_start: 0,
                    parent_relationship_end: 1,
                    child_relationship_start: 0,
                    child_relationship_end: 0,
                    game_object_relationship_start: 0,
                    game_object_relationship_end: 1,
                },
                WorkspaceSceneTransformHierarchy {
                    transform: last_transform,
                    parent_relationship_start: 1,
                    parent_relationship_end: 2,
                    child_relationship_start: 0,
                    child_relationship_end: 1,
                    game_object_relationship_start: 1,
                    game_object_relationship_end: 1,
                },
            ],
            transform_parent_relationship_indices: vec![7, 8],
            transform_child_relationship_indices: vec![4],
            transform_game_object_relationship_indices: vec![6, 5],
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![WorkspaceSceneAssetIdentity {
                identity: root,
                category: WorkspaceSceneAssetCategory::GameObject,
            }],
            component_candidates: vec![
                WorkspaceSceneComponentCandidate {
                    identity: first_transform,
                    evidence: WorkspaceSceneComponentEvidence::TransformClassId,
                },
                WorkspaceSceneComponentCandidate {
                    identity: last_transform,
                    evidence: WorkspaceSceneComponentEvidence::TransformClassId,
                },
                WorkspaceSceneComponentCandidate {
                    identity: renderer,
                    evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
                },
            ],
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };

        let mut child_relationships = vec![
            relationship(
                child_transform,
                SceneRelationshipKind::ComponentGameObject,
                "m_GameObject",
                child_game_object.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 1 },
            ),
            relationship(
                child_game_object,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[0]",
                child_transform.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 0 },
            ),
            relationship(
                child_transform,
                SceneRelationshipKind::TransformParent,
                "m_Father",
                first_transform.path_id,
                SceneRelationshipTarget::WorkspaceObject {
                    file_index: 0,
                    object_index: 1,
                },
            ),
        ];
        let child_transform_child_range = if cycle {
            child_relationships.push(relationship(
                child_transform,
                SceneRelationshipKind::TransformChild,
                "m_Children[0]",
                child_transform.path_id,
                SceneRelationshipTarget::LocalObject { object_index: 0 },
            ));
            0..1
        } else {
            0..0
        };
        let child_transform_child_indexes = if cycle { vec![3] } else { Vec::new() };
        let child_collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 1,
            scene_objects: Vec::new(),
            relationships: child_relationships,
            game_object_hierarchies: vec![WorkspaceSceneGameObjectHierarchy {
                game_object: child_game_object,
                component_relationship_start: 0,
                component_relationship_end: 1,
            }],
            game_object_component_relationship_indices: vec![1],
            prefab_root_relationship_index: Vec::new(),
            transform_hierarchies: vec![WorkspaceSceneTransformHierarchy {
                transform: child_transform,
                parent_relationship_start: 0,
                parent_relationship_end: 1,
                child_relationship_start: child_transform_child_range.start,
                child_relationship_end: child_transform_child_range.end,
                game_object_relationship_start: 0,
                game_object_relationship_end: 1,
            }],
            transform_parent_relationship_indices: vec![2],
            transform_child_relationship_indices: child_transform_child_indexes,
            transform_game_object_relationship_indices: vec![0],
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![WorkspaceSceneAssetIdentity {
                identity: child_game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            }],
            component_candidates: vec![WorkspaceSceneComponentCandidate {
                identity: child_transform,
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            }],
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };
        WorkspaceSceneHierarchies {
            scenes: Vec::new(),
            non_scene_collections: vec![root_collection, child_collection],
            collection_graph_index: vec![
                WorkspaceSceneCollectionGraphIndexEntry {
                    file_index: 0,
                    scene_index: None,
                    collection_index: 0,
                },
                WorkspaceSceneCollectionGraphIndexEntry {
                    file_index: 1,
                    scene_index: None,
                    collection_index: 1,
                },
            ],
        }
    }

    fn prefab_root_workspace(
        target: Option<(i64, SceneRelationshipTarget)>,
    ) -> (WorkspaceSceneHierarchies, WorkspaceSceneObjectIdentity) {
        let prefab = identity(0, 0, 100, PREFAB_INSTANCE_CLASS_ID);
        let root = identity(0, 1, 200, GAME_OBJECT_CLASS_ID);
        let relationships = target
            .map(|(path_id, target)| {
                vec![relationship(
                    prefab,
                    SceneRelationshipKind::PrefabInstanceRootGameObject,
                    "m_RootGameObject",
                    path_id,
                    target,
                )]
            })
            .unwrap_or_default();
        let prefab_root_relationship_index = if relationships.is_empty() {
            Vec::new()
        } else {
            vec![WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: prefab.object_index,
                relationship_index: 0,
            }]
        };
        let collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 0,
            scene_objects: Vec::new(),
            relationships,
            game_object_hierarchies: Vec::new(),
            game_object_component_relationship_indices: Vec::new(),
            prefab_root_relationship_index,
            transform_hierarchies: Vec::new(),
            transform_parent_relationship_indices: Vec::new(),
            transform_child_relationship_indices: Vec::new(),
            transform_game_object_relationship_indices: Vec::new(),
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![
                WorkspaceSceneAssetIdentity {
                    identity: prefab,
                    category: WorkspaceSceneAssetCategory::PrefabInstance,
                },
                WorkspaceSceneAssetIdentity {
                    identity: root,
                    category: WorkspaceSceneAssetCategory::GameObject,
                },
            ],
            component_candidates: Vec::new(),
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };
        (
            WorkspaceSceneHierarchies {
                scenes: Vec::new(),
                non_scene_collections: vec![collection],
                collection_graph_index: vec![WorkspaceSceneCollectionGraphIndexEntry {
                    file_index: 0,
                    scene_index: None,
                    collection_index: 0,
                }],
            },
            prefab,
        )
    }

    #[test]
    fn recognizes_source_scene_hierarchy_asset_class_ids() {
        assert_eq!(
            scene_asset_category(Some(1)),
            Some(WorkspaceSceneAssetCategory::GameObject)
        );
        assert_eq!(
            scene_asset_category(Some(29)),
            Some(WorkspaceSceneAssetCategory::LevelGameManager(
                SceneManagerKind::OcclusionCullingSettings
            ))
        );
        assert_eq!(
            scene_asset_category(Some(104)),
            Some(WorkspaceSceneAssetCategory::LevelGameManager(
                SceneManagerKind::RenderSettings
            ))
        );
        assert_eq!(
            scene_asset_category(Some(157)),
            Some(WorkspaceSceneAssetCategory::LevelGameManager(
                SceneManagerKind::LightmapSettings
            ))
        );
        assert_eq!(
            scene_asset_category(Some(196)),
            Some(WorkspaceSceneAssetCategory::LevelGameManager(
                SceneManagerKind::NavMeshSettings
            ))
        );
        assert_eq!(
            scene_asset_category(Some(1001)),
            Some(WorkspaceSceneAssetCategory::PrefabInstance)
        );
        assert_eq!(
            scene_asset_category(Some(1_660_057_539)),
            Some(WorkspaceSceneAssetCategory::SceneRoots)
        );
        assert_eq!(scene_asset_category(Some(114)), None);
    }

    #[test]
    fn indexes_prefab_root_edges_only_for_matching_prefab_instances() {
        let prefab = identity(0, 3, 30, PREFAB_INSTANCE_CLASS_ID);
        let root = identity(0, 4, 40, GAME_OBJECT_CLASS_ID);
        let relationships = vec![relationship(
            prefab,
            SceneRelationshipKind::PrefabInstanceRootGameObject,
            "m_RootGameObject",
            root.path_id,
            SceneRelationshipTarget::LocalObject {
                object_index: root.object_index,
            },
        )];
        let assets = [WorkspaceSceneAssetIdentity {
            identity: prefab,
            category: WorkspaceSceneAssetCategory::PrefabInstance,
        }];
        assert_eq!(
            build_prefab_root_relationship_index(0, &assets, &relationships).unwrap(),
            vec![WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: prefab.object_index,
                relationship_index: 0,
            }]
        );
        assert!(matches!(
            build_prefab_root_relationship_index(0, &[], &relationships),
            Err(
                WorkspaceSceneHierarchyError::PrefabRootRelationshipSourceMissing {
                    file_index: 0,
                    relationship_index: 0,
                    object_index: 3,
                }
            )
        ));
    }

    #[test]
    fn resolves_prefab_instance_root_in_workspace_file_order() {
        let prefab = identity(0, 0, 100, PREFAB_INSTANCE_CLASS_ID);
        let root = identity(1, 0, 200, GAME_OBJECT_CLASS_ID);
        let prefab_relationship = relationship(
            prefab,
            SceneRelationshipKind::PrefabInstanceRootGameObject,
            "m_RootGameObject",
            root.path_id,
            SceneRelationshipTarget::WorkspaceObject {
                file_index: root.file_index,
                object_index: root.object_index,
            },
        );
        let prefab_collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 0,
            scene_objects: Vec::new(),
            relationships: vec![prefab_relationship],
            game_object_hierarchies: Vec::new(),
            game_object_component_relationship_indices: Vec::new(),
            prefab_root_relationship_index: vec![WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: prefab.object_index,
                relationship_index: 0,
            }],
            transform_hierarchies: Vec::new(),
            transform_parent_relationship_indices: Vec::new(),
            transform_child_relationship_indices: Vec::new(),
            transform_game_object_relationship_indices: Vec::new(),
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![WorkspaceSceneAssetIdentity {
                identity: prefab,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            }],
            component_candidates: Vec::new(),
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };
        let root_collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 1,
            scene_objects: Vec::new(),
            relationships: Vec::new(),
            game_object_hierarchies: Vec::new(),
            game_object_component_relationship_indices: Vec::new(),
            prefab_root_relationship_index: Vec::new(),
            transform_hierarchies: Vec::new(),
            transform_parent_relationship_indices: Vec::new(),
            transform_child_relationship_indices: Vec::new(),
            transform_game_object_relationship_indices: Vec::new(),
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![WorkspaceSceneAssetIdentity {
                identity: root,
                category: WorkspaceSceneAssetCategory::GameObject,
            }],
            component_candidates: Vec::new(),
            missing_transform_plans: Vec::new(),
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };
        let workspace = WorkspaceSceneHierarchies {
            scenes: Vec::new(),
            non_scene_collections: vec![prefab_collection, root_collection],
            collection_graph_index: vec![
                WorkspaceSceneCollectionGraphIndexEntry {
                    file_index: 0,
                    scene_index: None,
                    collection_index: 0,
                },
                WorkspaceSceneCollectionGraphIndexEntry {
                    file_index: 1,
                    scene_index: None,
                    collection_index: 1,
                },
            ],
        };

        assert_eq!(
            workspace.prefab_instance_identities().collect::<Vec<_>>(),
            vec![prefab]
        );
        assert_eq!(
            workspace.resolve_prefab_instance_root(prefab).unwrap(),
            WorkspacePrefabInstanceRootResolution::Resolved(root)
        );
    }

    #[test]
    fn enumerates_prefab_instances_in_supplied_source_collection_order() {
        let (mut workspace, prefab_in_file_zero) =
            prefab_root_workspace(Some((0, SceneRelationshipTarget::Null)));
        let mut prefab_in_file_one = prefab_in_file_zero;
        prefab_in_file_one.file_index = 1;
        prefab_in_file_one.path_id = 101;

        let mut file_one_collection = workspace.non_scene_collections[0].clone();
        file_one_collection.collection_file_index = 1;
        file_one_collection.scene_assets = vec![WorkspaceSceneAssetIdentity {
            identity: prefab_in_file_one,
            category: WorkspaceSceneAssetCategory::PrefabInstance,
        }];
        file_one_collection.relationships.clear();
        file_one_collection.prefab_root_relationship_index.clear();
        workspace.non_scene_collections.push(file_one_collection);
        workspace
            .collection_graph_index
            .push(WorkspaceSceneCollectionGraphIndexEntry {
                file_index: 1,
                scene_index: None,
                collection_index: 1,
            });

        assert_eq!(
            workspace
                .prefab_instance_identities_in_collection_order(&[1, 0])
                .collect::<Vec<_>>(),
            vec![prefab_in_file_one, prefab_in_file_zero]
        );
    }

    #[test]
    fn keeps_null_unresolved_and_missing_prefab_roots_distinct() {
        let (null_workspace, prefab) =
            prefab_root_workspace(Some((0, SceneRelationshipTarget::Null)));
        assert_eq!(
            null_workspace.resolve_prefab_instance_root(prefab).unwrap(),
            WorkspacePrefabInstanceRootResolution::Null
        );

        let (unresolved_workspace, prefab) = prefab_root_workspace(Some((
            404,
            SceneRelationshipTarget::MissingLocalObject { path_id: 404 },
        )));
        assert_eq!(
            unresolved_workspace
                .resolve_prefab_instance_root(prefab)
                .unwrap(),
            WorkspacePrefabInstanceRootResolution::UnresolvedTarget(
                SceneRelationshipTarget::MissingLocalObject { path_id: 404 }
            )
        );

        let (missing_workspace, prefab) = prefab_root_workspace(None);
        assert_eq!(
            missing_workspace
                .resolve_prefab_instance_root(prefab)
                .unwrap(),
            WorkspacePrefabInstanceRootResolution::FieldUnavailable
        );
    }

    #[test]
    fn keeps_wrong_class_path_mismatch_and_duplicate_prefab_roots_explicit() {
        let (mut wrong_class_workspace, prefab) = prefab_root_workspace(Some((
            200,
            SceneRelationshipTarget::LocalObject { object_index: 1 },
        )));
        wrong_class_workspace.non_scene_collections[0].scene_assets[1]
            .identity
            .class_id = Some(23);
        assert_eq!(
            wrong_class_workspace
                .resolve_prefab_instance_root(prefab)
                .unwrap(),
            WorkspacePrefabInstanceRootResolution::TargetClassMismatch {
                target: identity(0, 1, 200, 23),
                actual_class_id: Some(23),
            }
        );

        let (path_mismatch_workspace, prefab) = prefab_root_workspace(Some((
            201,
            SceneRelationshipTarget::LocalObject { object_index: 1 },
        )));
        assert_eq!(
            path_mismatch_workspace
                .resolve_prefab_instance_root(prefab)
                .unwrap(),
            WorkspacePrefabInstanceRootResolution::TargetIdentityPathMismatch {
                expected_path_id: 201,
                actual_path_id: 200,
            }
        );

        let (mut duplicate_workspace, prefab) = prefab_root_workspace(Some((
            200,
            SceneRelationshipTarget::LocalObject { object_index: 1 },
        )));
        duplicate_workspace.non_scene_collections[0]
            .relationships
            .push(relationship(
                prefab,
                SceneRelationshipKind::PrefabInstanceRootGameObject,
                "m_RootGameObject",
                200,
                SceneRelationshipTarget::LocalObject { object_index: 1 },
            ));
        duplicate_workspace.non_scene_collections[0]
            .prefab_root_relationship_index
            .push(WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: prefab.object_index,
                relationship_index: 1,
            });
        assert_eq!(
            duplicate_workspace
                .resolve_prefab_instance_root(prefab)
                .unwrap(),
            WorkspacePrefabInstanceRootResolution::DuplicateRootReferences { match_count: 2 }
        );
    }

    #[test]
    fn component_pointer_field_match_is_limited_to_inherited_game_object_field() {
        assert!(is_game_object_pointer_path("m_GameObject"));
        assert!(is_game_object_pointer_path("Base.m_GameObject"));
        assert!(is_game_object_pointer_path("Base.Base.m_GameObject"));
        assert!(!is_game_object_pointer_path("Other.m_GameObject"));
        assert!(!is_game_object_pointer_path("m_GameObject.child"));
        assert!(!is_game_object_pointer_path("m_GameObject[0]"));
    }

    #[test]
    fn fetch_hierarchy_preserves_component_and_child_order_and_uses_last_transform() {
        let workspace = workspace_hierarchy_fixture(false);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let walk = workspace
            .fetch_game_object_hierarchy(root, WorkspaceGameObjectHierarchyWalkLimits::default())
            .unwrap();
        assert!(walk.is_complete(), "{:?}", walk.issues());
        assert_eq!(
            walk.entries()
                .iter()
                .map(|entry| (entry.identity, entry.kind))
                .collect::<Vec<_>>(),
            vec![
                (root, WorkspaceGameObjectHierarchyEntryKind::GameObject),
                (
                    identity(0, 3, 4, 23),
                    WorkspaceGameObjectHierarchyEntryKind::Component,
                ),
                (
                    identity(0, 1, 2, TRANSFORM_CLASS_ID),
                    WorkspaceGameObjectHierarchyEntryKind::Component,
                ),
                (
                    identity(0, 2, 3, RECT_TRANSFORM_CLASS_ID),
                    WorkspaceGameObjectHierarchyEntryKind::Component,
                ),
                (
                    identity(1, 1, 11, GAME_OBJECT_CLASS_ID),
                    WorkspaceGameObjectHierarchyEntryKind::GameObject,
                ),
                (
                    identity(1, 0, 10, TRANSFORM_CLASS_ID),
                    WorkspaceGameObjectHierarchyEntryKind::Component,
                ),
            ]
        );
    }

    #[test]
    fn fetch_hierarchy_reports_cycles_and_unresolved_component_edges() {
        let mut workspace = workspace_hierarchy_fixture(true);
        workspace.non_scene_collections[0].relationships[0].target =
            SceneRelationshipTarget::MissingLocalObject { path_id: 4 };
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let walk = workspace
            .fetch_game_object_hierarchy(root, WorkspaceGameObjectHierarchyWalkLimits::default())
            .unwrap();
        assert_eq!(walk.entries().len(), 5);
        assert_eq!(walk.issues().len(), 2);
        assert!(walk.issues().iter().any(|issue| matches!(
            issue,
            WorkspaceGameObjectHierarchyIssue::UnresolvedComponentReference {
                game_object,
                relationship_index: 0,
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 4 },
            } if *game_object == root
        )));
        assert!(walk.issues().iter().any(|issue| matches!(
            issue,
            WorkspaceGameObjectHierarchyIssue::CycleDetected { game_object }
                if *game_object == identity(1, 1, 11, GAME_OBJECT_CLASS_ID)
        )));
        assert!(!walk.is_complete());
    }

    #[test]
    fn fetch_hierarchy_fails_when_depth_or_entry_bounds_are_exceeded() {
        let workspace = workspace_hierarchy_fixture(false);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        assert!(matches!(
            workspace.fetch_game_object_hierarchy(
                root,
                WorkspaceGameObjectHierarchyWalkLimits {
                    max_depth: 0,
                    ..WorkspaceGameObjectHierarchyWalkLimits::default()
                }
            ),
            Err(
                WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                    resource: "GameObject hierarchy depth",
                    requested: 1,
                    limit: 0,
                }
            )
        ));
        assert!(matches!(
            workspace.fetch_game_object_hierarchy(
                root,
                WorkspaceGameObjectHierarchyWalkLimits {
                    max_entries: 2,
                    ..WorkspaceGameObjectHierarchyWalkLimits::default()
                }
            ),
            Err(
                WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                    resource: "GameObject hierarchy entries",
                    requested: 3,
                    limit: 2,
                }
            )
        ));
    }

    #[test]
    fn get_root_uses_first_transform_and_follows_cross_file_fathers() {
        let workspace = workspace_hierarchy_fixture(false);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let first_transform = identity(0, 1, 2, TRANSFORM_CLASS_ID);
        let child = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);
        let child_transform = identity(1, 0, 10, TRANSFORM_CLASS_ID);

        let root_walk = workspace
            .get_root_game_object(root, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(root_walk.is_complete());
        assert_eq!(root_walk.first_transform, Some(first_transform));
        assert_eq!(root_walk.transform_chain, vec![first_transform]);
        assert_eq!(root_walk.root_game_object, Some(root));
        assert_eq!(
            root_walk.termination,
            Some(WorkspaceGetRootGameObjectTermination::NullFather)
        );

        let child_walk = workspace
            .get_root_game_object(child, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(child_walk.is_complete());
        assert_eq!(child_walk.first_transform, Some(child_transform));
        assert_eq!(
            child_walk.transform_chain,
            vec![child_transform, first_transform]
        );
        assert_eq!(child_walk.root_game_object, Some(root));
    }

    #[test]
    fn get_root_handles_added_transform_and_reports_unresolved_or_cyclic_fathers() {
        let mut no_transform_workspace = workspace_hierarchy_fixture(false);
        let root_graph = &mut no_transform_workspace.non_scene_collections[0];
        root_graph.game_object_hierarchies[0].component_relationship_end = 2;
        root_graph
            .game_object_component_relationship_indices
            .truncate(2);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let no_transform = no_transform_workspace
            .get_root_game_object(root, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(no_transform.is_complete());
        assert_eq!(no_transform.root_game_object, Some(root));
        assert_eq!(
            no_transform.termination,
            Some(WorkspaceGetRootGameObjectTermination::NoTransformComponent)
        );

        let mut unresolved_workspace = workspace_hierarchy_fixture(false);
        unresolved_workspace.non_scene_collections[1].relationships[2].path_id = 404;
        unresolved_workspace.non_scene_collections[1].relationships[2].target =
            SceneRelationshipTarget::MissingLocalObject { path_id: 404 };
        let child = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);
        let unresolved = unresolved_workspace
            .get_root_game_object(child, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(matches!(
            unresolved.issue,
            Some(WorkspaceGetRootGameObjectIssue::UnresolvedTransformParent {
                transform,
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 404 },
            }) if transform == identity(1, 0, 10, TRANSFORM_CLASS_ID)
        ));

        let mut null_parent_owner_workspace = workspace_hierarchy_fixture(false);
        null_parent_owner_workspace.non_scene_collections[0].relationships[6].target =
            SceneRelationshipTarget::Null;
        let null_parent_owner = null_parent_owner_workspace
            .get_root_game_object(child, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(null_parent_owner.is_complete());
        assert_eq!(null_parent_owner.root_game_object, Some(child));
        assert_eq!(
            null_parent_owner.termination,
            Some(WorkspaceGetRootGameObjectTermination::ParentHasNullGameObject)
        );

        let mut cycle_workspace = workspace_hierarchy_fixture(false);
        let first_transform = identity(0, 1, 2, TRANSFORM_CLASS_ID);
        cycle_workspace.non_scene_collections[0].relationships[7].path_id = 10;
        cycle_workspace.non_scene_collections[0].relationships[7].target =
            SceneRelationshipTarget::WorkspaceObject {
                file_index: 1,
                object_index: 0,
            };
        let cycle = cycle_workspace
            .get_root_game_object(child, WorkspaceGetRootGameObjectLimits::default())
            .unwrap();
        assert!(matches!(
            cycle.issue,
            Some(WorkspaceGetRootGameObjectIssue::CycleDetected { transform })
                if transform == identity(1, 0, 10, TRANSFORM_CLASS_ID)
        ));
        assert!(cycle.transform_chain.contains(&first_transform));
    }

    #[test]
    fn get_root_enforces_parent_depth_limit() {
        let workspace = workspace_hierarchy_fixture(false);
        let child = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);
        assert!(matches!(
            workspace.get_root_game_object(
                child,
                WorkspaceGetRootGameObjectLimits {
                    max_parent_depth: 1,
                }
            ),
            Err(WorkspaceGetRootGameObjectError::ParentDepthLimitExceeded {
                requested: 2,
                limit: 1,
            })
        ));
    }

    #[test]
    fn prefab_processing_seeds_scene_objects_before_existing_instances() {
        let mut workspace = workspace_hierarchy_fixture(false);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let child = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);
        let prefab = identity(1, 2, 12, PREFAB_INSTANCE_CLASS_ID);
        let null_prefab = identity(1, 3, 13, PREFAB_INSTANCE_CLASS_ID);
        let missing_root_prefab = identity(1, 4, 14, PREFAB_INSTANCE_CLASS_ID);

        let child_collection = &mut workspace.non_scene_collections[1];
        child_collection.relationships.push(relationship(
            prefab,
            SceneRelationshipKind::PrefabInstanceRootGameObject,
            "m_RootGameObject",
            child.path_id,
            SceneRelationshipTarget::LocalObject {
                object_index: child.object_index,
            },
        ));
        child_collection.relationships.push(relationship(
            null_prefab,
            SceneRelationshipKind::PrefabInstanceRootGameObject,
            "m_RootGameObject",
            0,
            SceneRelationshipTarget::Null,
        ));
        child_collection.prefab_root_relationship_index = vec![
            WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: prefab.object_index,
                relationship_index: 3,
            },
            WorkspacePrefabRootRelationshipIndexEntry {
                source_object_index: null_prefab.object_index,
                relationship_index: 4,
            },
        ];
        child_collection
            .scene_assets
            .push(WorkspaceSceneAssetIdentity {
                identity: prefab,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            });
        child_collection
            .scene_assets
            .push(WorkspaceSceneAssetIdentity {
                identity: null_prefab,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            });
        child_collection
            .scene_assets
            .push(WorkspaceSceneAssetIdentity {
                identity: missing_root_prefab,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            });

        let scene_collection = workspace.non_scene_collections.remove(0);
        workspace.scenes.push(WorkspaceSceneHierarchyScene {
            plan: SceneDefinitionPlan {
                collection_file_index: 0,
                collection_name: "scene".to_owned(),
                name: "scene".to_owned(),
                path: "Assets/Scenes/scene".to_owned(),
                path_source: SceneDefinitionPathSource::CollectionNameFallback,
                guid: SceneDefinitionGuidPlan::GenerateAtExport(
                    SceneGuidGenerationReason::MissingFromInput,
                ),
            },
            collections: vec![scene_collection],
        });
        workspace.collection_graph_index[0] = WorkspaceSceneCollectionGraphIndexEntry {
            file_index: 0,
            scene_index: Some(0),
            collection_index: 0,
        };
        workspace.collection_graph_index[1].collection_index = 0;

        let plan =
            plan_workspace_prefab_processing(&workspace, &[1, 0], Default::default()).unwrap();
        assert!(!plan.is_complete());
        assert_eq!(plan.processed_game_objects(), &[root, child]);
        assert!(matches!(
            plan.events()[0],
            WorkspacePrefabProcessingEvent::SceneGameObjectMarked {
                scene_index: 0,
                game_object,
                newly_marked: true,
            } if game_object == root
        ));
        assert!(matches!(
            plan.events()[1],
            WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyCreated {
                prefab_instance,
                root_game_object,
                set_prefab_internal: true,
                ..
            } if prefab_instance == prefab && root_game_object == child
        ));
        assert!(matches!(
            plan.events()[2],
            WorkspacePrefabProcessingEvent::ExistingPrefabInstanceNullRoot {
                prefab_instance,
            } if prefab_instance == null_prefab
        ));
        assert!(matches!(
            plan.events()[3],
            WorkspacePrefabProcessingEvent::ExistingPrefabInstanceRootUnavailable {
                prefab_instance,
                resolution: WorkspacePrefabInstanceRootResolution::FieldUnavailable,
            } if prefab_instance == missing_root_prefab
        ));
        assert!(matches!(
            plan.events()[4],
            WorkspacePrefabProcessingEvent::RemainingGameObjectSkippedAlreadyProcessed {
                game_object,
            } if game_object == child
        ));
        assert!(matches!(
            plan.events()[5],
            WorkspacePrefabProcessingEvent::RemainingGameObjectSkippedAlreadyProcessed {
                game_object,
            } if game_object == root
        ));
        let report = plan.report();
        assert_eq!(report.existing_prefab_instances, 3);
        assert_eq!(report.existing_prefab_hierarchies, 1);
        assert_eq!(report.root_resolution_issues, 1);
    }

    #[test]
    fn prefab_processing_uses_source_order_and_deduplicates_hierarchy_game_objects() {
        let workspace = workspace_hierarchy_fixture(false);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let child = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);
        let plan =
            plan_workspace_prefab_processing(&workspace, &[1, 0], Default::default()).unwrap();

        assert!(plan.is_complete());
        assert_eq!(plan.processed_game_objects(), &[root, child]);
        assert!(matches!(
            &plan.events()[0],
            WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyCreated {
                source_game_object,
                root_game_object,
                hierarchy,
                ..
            } if *source_game_object == child
                && *root_game_object == root
                && hierarchy.is_complete()
        ));
        assert!(matches!(
            plan.events()[1],
            WorkspacePrefabProcessingEvent::RemainingGameObjectSkippedAlreadyProcessed {
                game_object,
            } if game_object == root
        ));
        assert_eq!(plan.hierarchy_entry_count(), 6);
    }

    #[test]
    fn prefab_hierarchy_plan_inserts_missing_transform_after_source_components() {
        let mut workspace = workspace_hierarchy_fixture(false);
        let root_graph = &mut workspace.non_scene_collections[0];
        root_graph.game_object_hierarchies[0].component_relationship_end = 2;
        root_graph
            .game_object_component_relationship_indices
            .truncate(2);
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let plan =
            plan_workspace_prefab_processing(&workspace, &[0, 1], Default::default()).unwrap();

        assert!(plan.is_complete());
        assert_eq!(plan.report().generated_transform_components, 1);
        let WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyCreated { hierarchy, .. } =
            &plan.events()[0]
        else {
            panic!("expected a generated hierarchy event")
        };
        assert_eq!(hierarchy.entries().len(), 3);
        assert!(matches!(
            hierarchy.entries()[0],
            crate::processing::prefab_processing::WorkspacePrefabHierarchyPlanEntry::Source(
                WorkspaceGameObjectHierarchyEntry {
                    identity,
                    kind: WorkspaceGameObjectHierarchyEntryKind::GameObject,
                }
            ) if identity == root
        ));
        assert!(matches!(
            hierarchy.entries()[1],
            crate::processing::prefab_processing::WorkspacePrefabHierarchyPlanEntry::Source(
                WorkspaceGameObjectHierarchyEntry {
                    identity: component_identity,
                    kind: WorkspaceGameObjectHierarchyEntryKind::Component,
                }
            ) if component_identity == identity(0, 3, 4, 23)
        ));
        assert!(matches!(
            &hierarchy.entries()[2],
            crate::processing::prefab_processing::WorkspacePrefabHierarchyPlanEntry::GeneratedTransform {
                game_object,
                default_values,
            } if *game_object == root
                && default_values.local_scale.x == 1.0
                && default_values.local_rotation.w == 1.0
        ));
    }

    #[test]
    fn prefab_processing_stops_after_an_incomplete_hierarchy_walk() {
        let mut workspace = workspace_hierarchy_fixture(false);
        workspace.non_scene_collections[1].source_reference_scans_complete = false;
        let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let plan =
            plan_workspace_prefab_processing(&workspace, &[0, 1], Default::default()).unwrap();

        assert!(!plan.is_complete());
        assert!(plan.stopped_on_incomplete_hierarchy());
        assert_eq!(plan.processed_game_objects(), &[root]);
        assert!(matches!(
            &plan.events()[0],
            WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyIncomplete {
                root_game_object,
                hierarchy,
                ..
            } if *root_game_object == root && !hierarchy.is_complete()
        ));
        assert_eq!(plan.events().len(), 1);
    }

    #[test]
    fn prefab_processing_enforces_event_and_hierarchy_entry_limits() {
        let workspace = workspace_hierarchy_fixture(false);
        assert!(matches!(
            plan_workspace_prefab_processing(
                &workspace,
                &[0, 1],
                crate::processing::prefab_processing::WorkspacePrefabProcessingLimits {
                    max_events: 0,
                    ..Default::default()
                }
            ),
            Err(crate::processing::prefab_processing::WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "Prefab processing events",
                requested: 1,
                limit: 0,
            })
        ));
        assert!(matches!(
            plan_workspace_prefab_processing(
                &workspace,
                &[0, 1],
                crate::processing::prefab_processing::WorkspacePrefabProcessingLimits {
                    max_total_hierarchy_entries: 1,
                    ..Default::default()
                }
            ),
            Err(crate::processing::prefab_processing::WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "GameObject hierarchy entries",
                requested: 2,
                limit: 1,
            })
        ));

        let mut missing_transform_workspace = workspace_hierarchy_fixture(false);
        missing_transform_workspace.non_scene_collections[0].game_object_hierarchies[0]
            .component_relationship_end = 2;
        missing_transform_workspace.non_scene_collections[0]
            .game_object_component_relationship_indices
            .truncate(2);
        assert!(matches!(
            plan_workspace_prefab_processing(
                &missing_transform_workspace,
                &[0, 1],
                crate::processing::prefab_processing::WorkspacePrefabProcessingLimits {
                    max_total_hierarchy_entries: 2,
                    ..Default::default()
                }
            ),
            Err(crate::processing::prefab_processing::WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "Prefab hierarchy plan entries",
                requested: 3,
                limit: 2,
            })
        ));
    }

    #[test]
    fn processed_prefab_asset_plan_uses_independent_collection_path_id_sequences() {
        let workspace = workspace_hierarchy_fixture(false);
        let processing = plan_workspace_prefab_processing(
            &workspace,
            &[0, 1],
            crate::processing::prefab_processing::WorkspacePrefabProcessingLimits::default(),
        )
        .unwrap();
        assert!(processing.is_complete());
        assert_eq!(processing.source_collection_file_indices(), &[0, 1]);

        let allocations = plan_workspace_processed_prefab_assets(
            &workspace,
            &processing,
            &[0, 0],
            &Default::default(),
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        assert!(
            allocations.is_complete(),
            "{:?}",
            allocations.incomplete_reason()
        );
        assert_eq!(allocations.asset_count(), 2);
        assert_eq!(allocations.collections().len(), 3);
        assert_eq!(
            allocations.collections()[0].kind,
            WorkspaceProcessedCollectionKind::PrefabHierarchies
        );
        assert_eq!(
            allocations.collections()[1].kind,
            WorkspaceProcessedCollectionKind::GeneratedPrefabs
        );
        assert_eq!(
            allocations.collections()[2].kind,
            WorkspaceProcessedCollectionKind::MissingPrefabTransforms
        );

        let generated_prefab = &allocations.collections()[1].assets[0];
        assert_eq!(generated_prefab.identity.path_id, 1);
        assert_eq!(generated_prefab.identity.class_id, 1001);
        let hierarchy = &allocations.collections()[0].assets[0];
        assert_eq!(hierarchy.identity.path_id, 1);
        assert_eq!(hierarchy.identity.class_id, 1001);
        assert!(matches!(
            hierarchy.kind,
            WorkspaceProcessedPrefabAssetKind::PrefabHierarchy {
                prefab: WorkspaceProcessedPrefabReference::Generated(identity),
                ..
            } if identity == generated_prefab.identity
        ));
    }

    #[test]
    fn incomplete_prefab_processing_does_not_claim_generated_path_ids() {
        let workspace = workspace_hierarchy_fixture(true);
        let processing = plan_workspace_prefab_processing(
            &workspace,
            &[0, 1],
            crate::processing::prefab_processing::WorkspacePrefabProcessingLimits::default(),
        )
        .unwrap();
        assert!(!processing.is_complete());

        let allocations = plan_workspace_processed_prefab_assets(
            &workspace,
            &processing,
            &[0, 0],
            &Default::default(),
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        assert!(!allocations.is_complete());
        assert!(allocations.collections().is_empty());
        assert_eq!(allocations.asset_count(), 0);
        assert!(allocations
            .incomplete_reason()
            .unwrap()
            .contains("identity plan is incomplete"));
    }

    #[test]
    fn generated_transform_gets_its_source_collection_path_id_before_prefab_assets() {
        let game_object = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
        let collection = WorkspaceSceneCollectionHierarchy {
            collection_file_index: 0,
            scene_objects: Vec::new(),
            relationships: vec![relationship(
                game_object,
                SceneRelationshipKind::GameObjectComponent,
                "m_Component[0]",
                0,
                SceneRelationshipTarget::Null,
            )],
            game_object_hierarchies: vec![WorkspaceSceneGameObjectHierarchy {
                game_object,
                component_relationship_start: 0,
                component_relationship_end: 1,
            }],
            game_object_component_relationship_indices: vec![0],
            prefab_root_relationship_index: Vec::new(),
            transform_hierarchies: Vec::new(),
            transform_parent_relationship_indices: Vec::new(),
            transform_child_relationship_indices: Vec::new(),
            transform_game_object_relationship_indices: Vec::new(),
            graph_issues: Vec::new(),
            lighting_data_asset_field_object_indices: Vec::new(),
            scene_assets: vec![WorkspaceSceneAssetIdentity {
                identity: game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            }],
            component_candidates: Vec::new(),
            missing_transform_plans: vec![WorkspaceMissingTransformPlan {
                game_object,
                default_values: default_workspace_generated_transform_values(),
            }],
            source_graph_complete: true,
            source_reference_scans_complete: true,
            roots: WorkspaceSceneRootDiscovery::default(),
            hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
        };
        let workspace = WorkspaceSceneHierarchies {
            scenes: Vec::new(),
            non_scene_collections: vec![collection],
            collection_graph_index: vec![WorkspaceSceneCollectionGraphIndexEntry {
                file_index: 0,
                scene_index: None,
                collection_index: 0,
            }],
        };
        let processing = plan_workspace_prefab_processing(
            &workspace,
            &[0],
            crate::processing::prefab_processing::WorkspacePrefabProcessingLimits::default(),
        )
        .unwrap();
        assert!(processing.is_complete());

        let allocations = plan_workspace_processed_prefab_assets(
            &workspace,
            &processing,
            &[0],
            &Default::default(),
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        assert!(
            allocations.is_complete(),
            "{:?}",
            allocations.incomplete_reason()
        );
        assert_eq!(allocations.asset_count(), 3);
        let missing_transform_collection = &allocations.collections()[2];
        assert_eq!(missing_transform_collection.assets.len(), 1);
        assert_eq!(missing_transform_collection.assets[0].identity.path_id, 1);
        assert_eq!(missing_transform_collection.assets[0].identity.class_id, 4);
        assert!(matches!(
            missing_transform_collection.assets[0].kind,
            WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
                game_object: actual,
                default_values,
            } if actual == game_object
                && default_values == default_workspace_generated_transform_values()
        ));
        assert_eq!(allocations.component_additions().len(), 1);
        let component_addition = allocations.component_additions()[0];
        assert_eq!(component_addition.game_object, game_object);
        assert_eq!(component_addition.component_class_id, 4);
        assert_eq!(component_addition.appended_index, 1);
        assert_eq!(
            component_addition.component_asset,
            missing_transform_collection.assets[0].identity
        );
        let component_overlay = &allocations.component_list_overlays()[0];
        assert_eq!(component_overlay.game_object, game_object);
        assert_eq!(component_overlay.source_slots.len(), 1);
        assert_eq!(component_overlay.source_slots[0].source_array_index, 0);
        assert_eq!(component_overlay.source_slots[0].file_id, 0);
        assert_eq!(component_overlay.source_slots[0].path_id, 0);
        assert_eq!(
            component_overlay.source_slots[0].target,
            SceneRelationshipTarget::Null
        );
        assert_eq!(component_overlay.generated_component, component_addition);
        assert_eq!(allocations.source_dependency_appends().len(), 1);
        let source_dependency_append = allocations.source_dependency_appends()[0];
        assert_eq!(source_dependency_append.source_file_index, 0);
        assert_eq!(
            source_dependency_append.target_collection,
            crate::processing::prefab_processing::WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                collection_index: 2
            }
        );
        assert_eq!(
            source_dependency_append.pointer_field,
            crate::processing::prefab_processing::WorkspaceSourceDependencyPointerField::GameObjectComponent
        );
        assert_eq!(source_dependency_append.first_source_owner, game_object);
        assert_eq!(
            source_dependency_append.target_asset,
            Some(missing_transform_collection.assets[0].identity)
        );
        assert_eq!(source_dependency_append.file_id, Some(1));
        assert_eq!(allocations.constructor_pointers().len(), 2);
        assert_eq!(
            allocations.constructor_pointers()[0].field,
            crate::processing::prefab_processing::WorkspaceGeneratedPointerField::TransformGameObject
        );
        assert_eq!(allocations.constructor_pointers()[0].file_id, 1);
        assert_eq!(
            allocations.constructor_pointers()[0].path_id,
            game_object.path_id
        );
        assert_eq!(allocations.constructor_pointers()[0].target, game_object);
        assert_eq!(
            allocations.constructor_pointers()[1].field,
            crate::processing::prefab_processing::WorkspaceGeneratedPointerField::PrefabInstanceRootGameObject
        );
        assert_eq!(allocations.constructor_pointers()[1].file_id, 1);
        assert_eq!(
            allocations.constructor_pointers()[1].path_id,
            game_object.path_id
        );
        assert_eq!(allocations.constructor_pointers()[1].target, game_object);
        for collection_index in [1, 2] {
            let dependencies = &allocations.collections()[collection_index].dependencies;
            assert_eq!(dependencies.len(), 2);
            assert_eq!(dependencies[0].file_id, 0);
            assert_eq!(
                dependencies[0].target,
                crate::processing::prefab_processing::WorkspaceProcessedDependencyTarget::ProcessedCollection {
                    collection_index
                }
            );
            assert_eq!(dependencies[1].file_id, 1);
            assert_eq!(
                dependencies[1].target,
                crate::processing::prefab_processing::WorkspaceProcessedDependencyTarget::SerializedFileCollection {
                    file_index: 0
                }
            );
        }
        assert!(!allocations.collections()[0].dependency_indices_complete);
        assert_eq!(allocations.collections()[1].assets[0].identity.path_id, 1);
        assert_eq!(allocations.collections()[0].assets[0].identity.path_id, 1);
    }

    #[test]
    fn lighting_data_dependency_file_id_precedes_missing_transform_component_file_id() {
        let game_object = identity(0, 0, 100, GAME_OBJECT_CLASS_ID);
        let lightmap_settings = identity(0, 1, 101, LIGHTMAP_SETTINGS_CLASS_ID);
        let render_settings = identity(0, 2, 102, SceneManagerKind::RenderSettings.class_id());
        let mut collection = empty_collection_graph(0);
        collection.scene_assets = vec![
            WorkspaceSceneAssetIdentity {
                identity: game_object,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
            WorkspaceSceneAssetIdentity {
                identity: lightmap_settings,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::LightmapSettings,
                ),
            },
            WorkspaceSceneAssetIdentity {
                identity: render_settings,
                category: WorkspaceSceneAssetCategory::LevelGameManager(
                    SceneManagerKind::RenderSettings,
                ),
            },
        ];
        collection.lighting_data_asset_field_object_indices = vec![lightmap_settings.object_index];
        collection.game_object_hierarchies = vec![WorkspaceSceneGameObjectHierarchy {
            game_object,
            component_relationship_start: 0,
            component_relationship_end: 0,
        }];
        collection.missing_transform_plans = vec![WorkspaceMissingTransformPlan {
            game_object,
            default_values: default_workspace_generated_transform_values(),
        }];
        let scene = WorkspaceSceneHierarchyScene {
            plan: scene_definition_plan(0),
            collections: vec![collection],
        };
        let workspace = WorkspaceSceneHierarchies {
            scenes: vec![scene],
            non_scene_collections: Vec::new(),
            collection_graph_index: vec![WorkspaceSceneCollectionGraphIndexEntry {
                file_index: 0,
                scene_index: Some(0),
                collection_index: 0,
            }],
        };
        let processing =
            plan_workspace_prefab_processing(&workspace, &[0], Default::default()).unwrap();
        assert!(processing.is_complete());
        let allocations = plan_workspace_processed_prefab_assets(
            &workspace,
            &processing,
            &[3],
            &Default::default(),
            WorkspaceProcessedPrefabAssetPlanLimits::default(),
        )
        .unwrap();
        assert!(allocations.is_complete());

        let appends = allocations.source_dependency_appends();
        assert_eq!(appends.len(), 2);
        assert_eq!(appends[0].source_file_index, 0);
        assert_eq!(
            appends[0].target_collection,
            WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets
        );
        assert_eq!(
            appends[0].pointer_field,
            WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset
        );
        assert_eq!(appends[0].first_source_owner, lightmap_settings);
        assert_eq!(appends[0].file_id, Some(4));
        assert_eq!(
            appends[1].target_collection,
            WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                collection_index: 3
            }
        );
        assert_eq!(appends[1].first_source_owner, game_object);
        assert_eq!(appends[1].file_id, Some(5));
    }
}
