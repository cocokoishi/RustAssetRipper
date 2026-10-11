//! File-scoped GameObject and Transform relationship recovery.
//!
//! Also ports the `SceneHierarchyObject` assembly decisions: the `Create` asset-sort
//! dispatch, the `FetchDependencies` edge order, and the `GetRoots` root filter over
//! caller-reported `IsRoot` facts. Live interface checks, pointer
//! resolution, generated `SceneAsset` allocation, and `SetMainAsset` plumbing stay
//! caller-side; repeated `SceneRoots` assignments overwrite, so callers keep the last.
//! The root facts arrive post-resolution, so this filter never invents roots from
//! unresolved links: the caller reports only whether each father pointer is null.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::metadata::{ObjectInfo, SerializedFileMetadata};
use crate::serialized_file::object::{
    ObjectDecodeLimits, ParsedObjectPayload, SerializedValue, SerializedValueLookupError,
};
use crate::serialized_file::object_workspace::WorkspaceObject;
use crate::serialized_file::reference_graph::{
    MultiFileObjectReference, MultiFilePPtrResolution, MultiFileReferenceGraph,
    PredefinedEngineResourceKind,
};

const GAME_OBJECT_CLASS_ID: i32 = 1;
const TRANSFORM_CLASS_ID: i32 = 4;
const RECT_TRANSFORM_CLASS_ID: i32 = 224;
const PREFAB_INSTANCE_CLASS_ID: i32 = 1001;

/// Bounds for building one file-scoped scene relationship graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneGraphLimits {
    pub max_objects: usize,
    pub max_relationships: usize,
    pub max_issues: usize,
    pub max_total_decoded_bytes: usize,
    pub max_total_name_bytes: usize,
    pub max_total_field_path_bytes: usize,
    pub object_decode: ObjectDecodeLimits,
}

impl Default for SceneGraphLimits {
    fn default() -> Self {
        Self {
            max_objects: 4_000_000,
            max_relationships: 2_000_000,
            max_issues: 100_000,
            max_total_decoded_bytes: 2 * 1024 * 1024 * 1024,
            max_total_name_bytes: 64 * 1024 * 1024,
            max_total_field_path_bytes: 64 * 1024 * 1024,
            object_decode: ObjectDecodeLimits::default(),
        }
    }
}

/// One object-table identity, retaining the source PathID and table index.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneObjectRecord {
    pub object_index: usize,
    pub path_id: i64,
    pub class_id: Option<i32>,
    /// Raw serialized UTF-8 bytes are kept so malformed input is not silently rewritten.
    pub name_bytes: Option<Vec<u8>>,
    pub is_active: Option<bool>,
    /// Serialized local-space transform values for Transform and RectTransform objects.
    pub transform: Option<SceneTransformValues>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneVector2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneVector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneQuaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// Values serialized by Transform, with optional fields reflecting the active TypeTree layout.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneTransformValues {
    pub local_position: Option<SceneVector3>,
    pub local_rotation: Option<SceneQuaternion>,
    pub local_scale: Option<SceneVector3>,
    pub root_order: Option<i32>,
    pub local_euler_angles_hint: Option<SceneVector3>,
    pub rect_transform: Option<SceneRectTransformValues>,
}

/// Persisted layout data found on RectTransform; all values remain in their source coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneRectTransformValues {
    pub anchor_min: Option<SceneVector2>,
    pub anchor_max: Option<SceneVector2>,
    pub anchored_position: Option<SceneVector2>,
    pub size_delta: Option<SceneVector2>,
    pub pivot: Option<SceneVector2>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneRelationshipKind {
    GameObjectComponent,
    ComponentGameObject,
    TransformParent,
    TransformChild,
    PrefabInstanceRootGameObject,
}

impl SceneRelationshipKind {
    const fn sort_rank(self) -> u8 {
        match self {
            Self::GameObjectComponent => 0,
            Self::ComponentGameObject => 1,
            Self::TransformParent => 2,
            Self::TransformChild => 3,
            Self::PrefabInstanceRootGameObject => 4,
        }
    }
}

/// Explicit result of resolving one recovered scene pointer within its source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneRelationshipTarget {
    Null,
    LocalObject {
        object_index: usize,
    },
    MissingLocalObject {
        path_id: i64,
    },
    AmbiguousLocalObject {
        path_id: i64,
        match_count: usize,
    },
    WorkspaceObject {
        file_index: usize,
        object_index: usize,
    },
    PredefinedEngineAsset {
        external_index: usize,
        resource: PredefinedEngineResourceKind,
        path_id: i64,
        class_id: i32,
    },
    ExternalObject {
        file_id: i32,
        external_index: usize,
        path_id: i64,
    },
    MissingDependency {
        external_index: usize,
        path_id: i64,
    },
    AmbiguousDependency {
        external_index: usize,
        path_id: i64,
        candidate_count: usize,
    },
    MissingExternalObject {
        file_index: usize,
        path_id: i64,
    },
    AmbiguousExternalObject {
        file_index: usize,
        path_id: i64,
        match_count: usize,
    },
    InvalidFileId {
        file_id: i32,
        external_count: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneRelationship {
    pub source_object_index: usize,
    pub source_path_id: i64,
    pub kind: SceneRelationshipKind,
    /// Field path includes zero-based array indexes where the source value was an array.
    pub field_path: String,
    pub file_id: i32,
    pub path_id: i64,
    pub target: SceneRelationshipTarget,
}

struct ScenePointerField {
    file_id: i32,
    path_id: i64,
    field_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneGraphIssue {
    pub object_index: usize,
    pub path_id: i64,
    pub field_path: Option<String>,
    pub kind: SceneGraphIssueKind,
}

/// A known contradiction in local Transform `m_Father`/`m_Children` relationships.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneHierarchyIssue {
    pub relationship_index: usize,
    pub source_object_index: usize,
    pub target_object_index: usize,
    pub field_path: String,
    pub kind: SceneHierarchyIssueKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyIssue {
    pub source_file_index: usize,
    pub relationship_index: usize,
    pub source_object_index: usize,
    pub source_path_id: i64,
    pub target_file_index: Option<usize>,
    pub target_object_index: Option<usize>,
    pub field_path: String,
    pub kind: SceneHierarchyIssueKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneHierarchyIssueKind {
    TargetClassUnknown,
    TargetIsNotTransform {
        class_id: i32,
    },
    MissingReciprocal {
        counterpart_kind: SceneRelationshipKind,
    },
    DuplicateReciprocal {
        counterpart_kind: SceneRelationshipKind,
        match_count: usize,
    },
    MismatchedReciprocalTarget {
        counterpart_kind: SceneRelationshipKind,
        actual_target: SceneRelationshipTarget,
    },
    UnresolvedTarget {
        target: SceneRelationshipTarget,
    },
    SourceScanIncomplete,
    CounterpartScanIncomplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneHierarchyAuditLimits {
    pub max_relationships: usize,
    pub max_issues: usize,
}

impl Default for SceneHierarchyAuditLimits {
    fn default() -> Self {
        Self {
            max_relationships: 2_000_000,
            max_issues: 100_000,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyAudit {
    relationships_checked: usize,
    issues: Vec<WorkspaceSceneHierarchyIssue>,
}

impl WorkspaceSceneHierarchyAudit {
    pub fn relationships_checked(&self) -> usize {
        self.relationships_checked
    }

    pub fn issues(&self) -> &[WorkspaceSceneHierarchyIssue] {
        &self.issues
    }

    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IndexedHierarchyRelationship {
    source_object_index: usize,
    kind: SceneRelationshipKind,
    target_local_object_index: Option<usize>,
    relationship_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IndexedSceneRelationship {
    source_object_index: usize,
    kind: SceneRelationshipKind,
    relationship_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneRootDiscoveryLimits {
    pub max_roots: usize,
    pub max_issues: usize,
}

impl Default for SceneRootDiscoveryLimits {
    fn default() -> Self {
        Self {
            max_roots: 4_000_000,
            max_issues: 100_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneRootReason {
    NoTransformComponent,
    NullFather,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneRootCandidate {
    pub game_object_index: usize,
    pub transform_index: Option<usize>,
    pub reason: SceneRootReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneRootDiscoveryIssue {
    pub game_object_index: usize,
    pub transform_index: Option<usize>,
    pub kind: SceneRootDiscoveryIssueKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneRootDiscoveryIssueKind {
    ComponentListIncomplete,
    UnresolvedComponent {
        target: SceneRelationshipTarget,
    },
    MultipleTransformComponents {
        selected_transform_index: usize,
        match_count: usize,
    },
    MissingTransformParent,
    DuplicateTransformParent {
        match_count: usize,
    },
    UnresolvedTransformParent {
        target: SceneRelationshipTarget,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneRootDiscovery {
    roots: Vec<SceneRootCandidate>,
    issues: Vec<SceneRootDiscoveryIssue>,
}

impl SceneRootDiscovery {
    pub fn roots(&self) -> &[SceneRootCandidate] {
        &self.roots
    }

    pub fn issues(&self) -> &[SceneRootDiscoveryIssue] {
        &self.issues
    }

    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

/// A scene root whose component or Transform links may cross SerializedFile boundaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneRootCandidate {
    pub source_file_index: usize,
    pub game_object_index: usize,
    pub transform_file_index: Option<usize>,
    pub transform_index: Option<usize>,
    pub reason: SceneRootReason,
}

/// A scene-root issue with file-scoped identities for both the GameObject and Transform.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneRootDiscoveryIssue {
    pub source_file_index: usize,
    pub game_object_index: usize,
    pub transform_file_index: Option<usize>,
    pub transform_index: Option<usize>,
    pub kind: WorkspaceSceneRootDiscoveryIssueKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneRootDiscoveryIssueKind {
    ComponentListIncomplete,
    UnresolvedComponent {
        target: SceneRelationshipTarget,
    },
    MultipleTransformComponents {
        selected_transform_file_index: usize,
        selected_transform_index: usize,
        match_count: usize,
    },
    MissingTransformParent,
    DuplicateTransformParent {
        match_count: usize,
    },
    UnresolvedTransformParent {
        target: SceneRelationshipTarget,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceSceneRootDiscovery {
    roots: Vec<WorkspaceSceneRootCandidate>,
    issues: Vec<WorkspaceSceneRootDiscoveryIssue>,
}

impl WorkspaceSceneRootDiscovery {
    pub fn roots(&self) -> &[WorkspaceSceneRootCandidate] {
        &self.roots
    }

    pub fn issues(&self) -> &[WorkspaceSceneRootDiscoveryIssue] {
        &self.issues
    }

    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneRootDiscoveryError {
    CountOverflow {
        resource: &'static str,
    },
    RootCountLimitExceeded {
        requested: usize,
        limit: usize,
    },
    IssueCountLimitExceeded {
        requested: usize,
        limit: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    InvalidRelationshipIndex {
        relationship_index: usize,
        relationship_count: usize,
    },
    ReferenceGraphMismatch {
        message: String,
    },
}

impl Display for SceneRootDiscoveryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountOverflow { resource } => {
                write!(f, "scene root discovery count overflows for {resource}")
            }
            Self::RootCountLimitExceeded { requested, limit } => write!(
                f,
                "scene root candidate count {requested} exceeds configured limit {limit}"
            ),
            Self::IssueCountLimitExceeded { requested, limit } => write!(
                f,
                "scene root issue count {requested} exceeds configured limit {limit}"
            ),
            Self::AllocationFailed { resource, requested } => write!(
                f,
                "could not reserve {requested} entries for scene root discovery {resource}"
            ),
            Self::InvalidRelationshipIndex {
                relationship_index,
                relationship_count,
            } => write!(
                f,
                "scene root relationship index {relationship_index} is outside the {relationship_count} relationships"
            ),
            Self::ReferenceGraphMismatch { message } => {
                write!(f, "scene root reference graph does not match its source: {message}")
            }
        }
    }
}

impl Error for SceneRootDiscoveryError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneGraphAuditError {
    CountOverflow {
        resource: &'static str,
    },
    RelationshipLimitExceeded {
        count: usize,
        limit: usize,
    },
    IssueLimitExceeded {
        count: usize,
        limit: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    InvalidRelationshipIndex {
        relationship_index: usize,
        relationship_count: usize,
    },
    ReferenceGraphMismatch {
        message: String,
    },
}

impl Display for SceneGraphAuditError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountOverflow { resource } => {
                write!(f, "scene graph audit count overflows for {resource}")
            }
            Self::RelationshipLimitExceeded { count, limit } => write!(
                f,
                "scene graph audit has {count} relationships, exceeding limit {limit}"
            ),
            Self::IssueLimitExceeded { count, limit } => write!(
                f,
                "scene graph audit has {count} issues, exceeding limit {limit}"
            ),
            Self::AllocationFailed { resource, requested } => write!(
                f,
                "could not reserve {requested} entries for scene graph audit {resource}"
            ),
            Self::InvalidRelationshipIndex {
                relationship_index,
                relationship_count,
            } => write!(
                f,
                "scene graph audit index {relationship_index} is outside the {relationship_count} relationships"
            ),
            Self::ReferenceGraphMismatch { message } => {
                write!(f, "scene graph audit reference graph mismatch: {message}")
            }
        }
    }
}

impl Error for SceneGraphAuditError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneGraphIssueKind {
    ObjectDecodeFailed {
        message: String,
    },
    MissingField {
        field_name: String,
    },
    DuplicateField {
        field_name: String,
        match_count: usize,
    },
    UnexpectedValueType {
        expected: &'static str,
        actual: &'static str,
    },
}

/// File-scoped scene nodes and relationships recovered from GameObject, Transform, and
/// PrefabInstance TypeTrees.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneGraph {
    objects: Vec<SceneObjectRecord>,
    relationships: Vec<SceneRelationship>,
    issues: Vec<SceneGraphIssue>,
}

struct SceneGraphBuilder<'a> {
    metadata: &'a SerializedFileMetadata,
    limits: SceneGraphLimits,
    objects: Vec<SceneObjectRecord>,
    path_id_index: Vec<(i64, usize)>,
    relationships: Vec<SceneRelationship>,
    issues: Vec<SceneGraphIssue>,
    total_decoded_bytes: usize,
    total_name_bytes: usize,
    total_field_path_bytes: usize,
}

mod accessors;
mod builder;
mod errors;
mod hierarchy_audit;
mod reference_resolution;
mod root_discovery;
mod scene_hierarchy;

pub use self::errors::SceneGraphBuildError;
pub use self::reference_resolution::SceneGraphResolveError;
pub use self::scene_hierarchy::{
    classify_scene_hierarchy_asset, is_scene_hierarchy_root, scene_hierarchy_dependency_fields,
    scene_hierarchy_root_indexes, SceneHierarchyAssetKind, SceneHierarchyRootFact,
    SceneHierarchySlot,
};

#[cfg(test)]
#[path = "scene/tests/mod.rs"]
mod tests;
