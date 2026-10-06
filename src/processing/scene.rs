//! File-scoped GameObject and Transform relationship recovery.
//!
//! Also ports the `SceneHierarchyObject` assembly decisions: the `Create` asset-sort
//! dispatch and the `FetchDependencies` edge order. Live interface checks, pointer
//! resolution, generated `SceneAsset` allocation, and `SetMainAsset` plumbing stay
//! caller-side; repeated `SceneRoots` assignments overwrite, so callers keep the last.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::metadata::{ObjectInfo, SerializedFileMetadata};
use crate::serialized_file::object::{
    ObjectDecodeLimits, ParsedObjectPayload, SerializedValue, SerializedValueLookupError,
};
use crate::serialized_file::reference_graph::{
    MultiFileObjectReference, MultiFilePPtrResolution, MultiFileReferenceGraph,
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

impl SceneGraph {
    /// Decode GameObject, Transform, RectTransform, and PrefabInstance payloads from one file.
    ///
    /// Other object records remain in `objects` as potential relationship targets. This stage
    /// does not load external files or synthesize reciprocal relationships.
    pub fn build(
        file: &SerializedFile<'_>,
        limits: SceneGraphLimits,
    ) -> Result<Self, SceneGraphBuildError> {
        let mut builder = SceneGraphBuilder::new(&file.metadata, limits)?;
        for (object_index, object) in file.metadata.objects.iter().enumerate() {
            if !is_scene_object_class(object_scene_class_id(object)) {
                continue;
            }
            match file.decode_object_payload_with_limits(object_index, &limits.object_decode) {
                Ok(payload) => builder.add_payload(&payload)?,
                Err(error) => builder.push_issue(SceneGraphIssue {
                    object_index,
                    path_id: object.path_id,
                    field_path: None,
                    kind: SceneGraphIssueKind::ObjectDecodeFailed {
                        message: error.to_string(),
                    },
                })?,
            }
        }
        builder.finish()
    }

    /// Build from already decoded roots, useful to processors that share an object decode pass.
    /// Every supplied root must correspond to the matching object-table index.
    pub fn from_decoded_roots(
        metadata: &SerializedFileMetadata,
        roots: &[(usize, SerializedValue)],
        limits: SceneGraphLimits,
    ) -> Result<Self, SceneGraphBuildError> {
        let mut builder = SceneGraphBuilder::new(metadata, limits)?;
        let expected_root_count = metadata
            .objects
            .iter()
            .filter(|object| is_scene_object_class(object_scene_class_id(object)))
            .count();
        if roots.len() != expected_root_count {
            return Err(SceneGraphBuildError::DecodedRootCountMismatch {
                expected: expected_root_count,
                actual: roots.len(),
            });
        }
        let mut seen = Vec::new();
        seen.try_reserve_exact(metadata.objects.len())
            .map_err(|_| SceneGraphBuildError::AllocationFailed {
                resource: "decoded scene root index",
                requested: metadata.objects.len(),
            })?;
        seen.resize(metadata.objects.len(), false);
        for (object_index, root) in roots {
            let object = metadata.objects.get(*object_index).ok_or(
                SceneGraphBuildError::InvalidObjectIndex {
                    object_index: *object_index,
                    object_count: metadata.objects.len(),
                },
            )?;
            if !is_scene_object_class(object_scene_class_id(object)) {
                return Err(SceneGraphBuildError::UnexpectedDecodedRoot {
                    object_index: *object_index,
                    class_id: object_scene_class_id(object),
                });
            }
            if std::mem::replace(&mut seen[*object_index], true) {
                return Err(SceneGraphBuildError::DuplicateDecodedRoot {
                    object_index: *object_index,
                });
            }
            builder.add_root(
                *object_index,
                object.path_id,
                object_scene_class_id(object),
                root,
            )?;
        }
        builder.finish()
    }

    pub fn objects(&self) -> &[SceneObjectRecord] {
        &self.objects
    }

    pub fn relationships(&self) -> &[SceneRelationship] {
        &self.relationships
    }

    pub fn issues(&self) -> &[SceneGraphIssue] {
        &self.issues
    }

    /// True when relevant TypeTrees decoded and every pointer uniquely resolves in the joined set.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
            && self.relationships.iter().all(|relationship| {
                matches!(
                    &relationship.target,
                    SceneRelationshipTarget::Null
                        | SceneRelationshipTarget::LocalObject { .. }
                        | SceneRelationshipTarget::WorkspaceObject { .. }
                )
            })
    }

    /// Find local GameObject root candidates using the source SceneHierarchyObject rule.
    ///
    /// A GameObject is a root when it has no resolved Transform component or its first resolved
    /// Transform component has a null father. Missing, ambiguous, or cross-file component and
    /// father pointers are reported as issues and do not become roots by assumption. This returns
    /// file-local candidates; assigning them to a SceneDefinition still requires collection and
    /// scene metadata that this graph does not hold.
    pub fn discover_local_roots(
        &self,
        limits: SceneRootDiscoveryLimits,
    ) -> Result<SceneRootDiscovery, SceneRootDiscoveryError> {
        let indexed_relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::GameObjectComponent
                        | SceneRelationshipKind::TransformParent
                )
            })
            .count();
        let mut relation_index = Vec::new();
        relation_index
            .try_reserve_exact(indexed_relationship_count)
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "relationship lookup index",
                requested: indexed_relationship_count,
            })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            if matches!(
                relationship.kind,
                SceneRelationshipKind::GameObjectComponent | SceneRelationshipKind::TransformParent
            ) {
                relation_index.push(IndexedSceneRelationship {
                    source_object_index: relationship.source_object_index,
                    kind: relationship.kind,
                    relationship_index,
                });
            }
        }
        relation_index.sort_unstable_by_key(|entry| {
            (
                entry.source_object_index,
                hierarchy_kind_rank(entry.kind),
                entry.relationship_index,
            )
        });

        let mut component_issue_objects = Vec::new();
        component_issue_objects
            .try_reserve_exact(self.issues.len())
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "GameObject component issue index",
                requested: self.issues.len(),
            })?;
        for issue in &self.issues {
            if issue.field_path.as_deref().is_some_and(|field_path| {
                field_path == "m_Component" || field_path.starts_with("m_Component[")
            }) {
                component_issue_objects.push(issue.object_index);
            }
        }
        component_issue_objects.sort_unstable();
        component_issue_objects.dedup();

        let mut discovery = SceneRootDiscovery {
            roots: Vec::new(),
            issues: Vec::new(),
        };
        for (game_object_index, object) in self.objects.iter().enumerate() {
            if object.class_id != Some(GAME_OBJECT_CLASS_ID) {
                continue;
            }
            let mut component_list_incomplete = component_issue_objects
                .binary_search(&game_object_index)
                .is_ok();
            if component_list_incomplete {
                push_scene_root_issue(
                    &mut discovery.issues,
                    limits.max_issues,
                    SceneRootDiscoveryIssue {
                        game_object_index,
                        transform_index: None,
                        kind: SceneRootDiscoveryIssueKind::ComponentListIncomplete,
                    },
                )?;
            }

            let mut selected_transform: Option<usize> = None;
            let mut transform_match_count = 0usize;
            let component_range = scene_relationship_range(
                &relation_index,
                game_object_index,
                SceneRelationshipKind::GameObjectComponent,
            );
            for entry in &relation_index[component_range] {
                let relationship = self.relationships.get(entry.relationship_index).ok_or(
                    SceneRootDiscoveryError::InvalidRelationshipIndex {
                        relationship_index: entry.relationship_index,
                        relationship_count: self.relationships.len(),
                    },
                )?;
                match &relationship.target {
                    SceneRelationshipTarget::Null => {}
                    SceneRelationshipTarget::LocalObject { object_index } => {
                        let Some(component) = self.objects.get(*object_index) else {
                            component_list_incomplete = true;
                            push_scene_root_issue(
                                &mut discovery.issues,
                                limits.max_issues,
                                SceneRootDiscoveryIssue {
                                    game_object_index,
                                    transform_index: selected_transform,
                                    kind: SceneRootDiscoveryIssueKind::UnresolvedComponent {
                                        target: relationship.target.clone(),
                                    },
                                },
                            )?;
                            continue;
                        };
                        match component.class_id {
                            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {
                                transform_match_count = transform_match_count
                                    .checked_add(1)
                                    .ok_or(SceneRootDiscoveryError::CountOverflow {
                                        resource: "Transform components",
                                    })?;
                                if selected_transform.is_none() {
                                    selected_transform = Some(*object_index);
                                }
                            }
                            Some(_) => {}
                            None => {
                                component_list_incomplete = true;
                                push_scene_root_issue(
                                    &mut discovery.issues,
                                    limits.max_issues,
                                    SceneRootDiscoveryIssue {
                                        game_object_index,
                                        transform_index: selected_transform,
                                        kind: SceneRootDiscoveryIssueKind::UnresolvedComponent {
                                            target: relationship.target.clone(),
                                        },
                                    },
                                )?;
                            }
                        }
                    }
                    target => {
                        component_list_incomplete = true;
                        push_scene_root_issue(
                            &mut discovery.issues,
                            limits.max_issues,
                            SceneRootDiscoveryIssue {
                                game_object_index,
                                transform_index: selected_transform,
                                kind: SceneRootDiscoveryIssueKind::UnresolvedComponent {
                                    target: target.clone(),
                                },
                            },
                        )?;
                    }
                }
            }
            if transform_match_count > 1 {
                if let Some(selected_transform_index) = selected_transform {
                    push_scene_root_issue(
                        &mut discovery.issues,
                        limits.max_issues,
                        SceneRootDiscoveryIssue {
                            game_object_index,
                            transform_index: Some(selected_transform_index),
                            kind: SceneRootDiscoveryIssueKind::MultipleTransformComponents {
                                selected_transform_index,
                                match_count: transform_match_count,
                            },
                        },
                    )?;
                }
            }

            if component_list_incomplete {
                continue;
            }

            let Some(transform_index) = selected_transform else {
                push_scene_root_candidate(
                    &mut discovery.roots,
                    limits.max_roots,
                    SceneRootCandidate {
                        game_object_index,
                        transform_index: None,
                        reason: SceneRootReason::NoTransformComponent,
                    },
                )?;
                continue;
            };

            let father_range = scene_relationship_range(
                &relation_index,
                transform_index,
                SceneRelationshipKind::TransformParent,
            );
            let father_entries = &relation_index[father_range];
            match father_entries {
                [] => push_scene_root_issue(
                    &mut discovery.issues,
                    limits.max_issues,
                    SceneRootDiscoveryIssue {
                        game_object_index,
                        transform_index: Some(transform_index),
                        kind: SceneRootDiscoveryIssueKind::MissingTransformParent,
                    },
                )?,
                [entry] => {
                    let father = self.relationships.get(entry.relationship_index).ok_or(
                        SceneRootDiscoveryError::InvalidRelationshipIndex {
                            relationship_index: entry.relationship_index,
                            relationship_count: self.relationships.len(),
                        },
                    )?;
                    match &father.target {
                        SceneRelationshipTarget::Null => push_scene_root_candidate(
                            &mut discovery.roots,
                            limits.max_roots,
                            SceneRootCandidate {
                                game_object_index,
                                transform_index: Some(transform_index),
                                reason: SceneRootReason::NullFather,
                            },
                        )?,
                        SceneRelationshipTarget::LocalObject { object_index } => {
                            let parent_is_transform =
                                self.objects.get(*object_index).is_some_and(|parent| {
                                    matches!(
                                        parent.class_id,
                                        Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                                    )
                                });
                            if !parent_is_transform {
                                push_scene_root_issue(
                                    &mut discovery.issues,
                                    limits.max_issues,
                                    SceneRootDiscoveryIssue {
                                        game_object_index,
                                        transform_index: Some(transform_index),
                                        kind:
                                            SceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                                                target: father.target.clone(),
                                            },
                                    },
                                )?;
                            }
                        }
                        target => push_scene_root_issue(
                            &mut discovery.issues,
                            limits.max_issues,
                            SceneRootDiscoveryIssue {
                                game_object_index,
                                transform_index: Some(transform_index),
                                kind: SceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                                    target: target.clone(),
                                },
                            },
                        )?,
                    }
                }
                entries => push_scene_root_issue(
                    &mut discovery.issues,
                    limits.max_issues,
                    SceneRootDiscoveryIssue {
                        game_object_index,
                        transform_index: Some(transform_index),
                        kind: SceneRootDiscoveryIssueKind::DuplicateTransformParent {
                            match_count: entries.len(),
                        },
                    },
                )?,
            }
        }
        Ok(discovery)
    }

    /// Discover roots while resolving component and father targets across loaded files.
    ///
    /// The scene graph is resolved against the supplied graph before discovery; source file and
    /// object identities are checked by `resolve_with_multi_file_graph`. The graph supplies target
    /// class IDs and the `m_Father` edge for external Transforms. Missing or undecodable targets
    /// remain issues, never roots.
    pub fn discover_workspace_roots(
        &mut self,
        source_file_index: usize,
        source_file_name: &str,
        graph: &MultiFileReferenceGraph<'_>,
        limits: SceneRootDiscoveryLimits,
    ) -> Result<WorkspaceSceneRootDiscovery, SceneRootDiscoveryError> {
        self.resolve_with_multi_file_graph(source_file_index, source_file_name, graph)
            .map_err(|source| SceneRootDiscoveryError::ReferenceGraphMismatch {
                message: source.to_string(),
            })?;
        let indexed_relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::GameObjectComponent
                        | SceneRelationshipKind::TransformParent
                )
            })
            .count();
        let mut relation_index = Vec::new();
        relation_index
            .try_reserve_exact(indexed_relationship_count)
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "workspace root relationship lookup index",
                requested: indexed_relationship_count,
            })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            if matches!(
                relationship.kind,
                SceneRelationshipKind::GameObjectComponent | SceneRelationshipKind::TransformParent
            ) {
                relation_index.push(IndexedSceneRelationship {
                    source_object_index: relationship.source_object_index,
                    kind: relationship.kind,
                    relationship_index,
                });
            }
        }
        relation_index.sort_unstable_by_key(|entry| {
            (
                entry.source_object_index,
                hierarchy_kind_rank(entry.kind),
                entry.relationship_index,
            )
        });

        let mut component_issue_objects = Vec::new();
        component_issue_objects
            .try_reserve_exact(self.issues.len())
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "workspace GameObject component issue index",
                requested: self.issues.len(),
            })?;
        for issue in &self.issues {
            if issue.field_path.as_deref().is_some_and(|field_path| {
                field_path == "m_Component" || field_path.starts_with("m_Component[")
            }) {
                component_issue_objects.push(issue.object_index);
            }
        }
        component_issue_objects.sort_unstable();
        component_issue_objects.dedup();

        let mut discovery = WorkspaceSceneRootDiscovery {
            roots: Vec::new(),
            issues: Vec::new(),
        };
        for (game_object_index, game_object) in self.objects.iter().enumerate() {
            if game_object.class_id != Some(GAME_OBJECT_CLASS_ID) {
                continue;
            }
            let mut component_list_incomplete = component_issue_objects
                .binary_search(&game_object_index)
                .is_ok();
            if component_list_incomplete {
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: None,
                        transform_index: None,
                        kind: WorkspaceSceneRootDiscoveryIssueKind::ComponentListIncomplete,
                    },
                )?;
            }

            let mut selected_transform: Option<(usize, usize)> = None;
            let mut transform_match_count = 0usize;
            let component_range = scene_relationship_range(
                &relation_index,
                game_object_index,
                SceneRelationshipKind::GameObjectComponent,
            );
            for entry in &relation_index[component_range] {
                let relationship = self.relationships.get(entry.relationship_index).ok_or(
                    SceneRootDiscoveryError::InvalidRelationshipIndex {
                        relationship_index: entry.relationship_index,
                        relationship_count: self.relationships.len(),
                    },
                )?;
                let target_identity = match &relationship.target {
                    SceneRelationshipTarget::Null => continue,
                    SceneRelationshipTarget::LocalObject { object_index } => {
                        Some((source_file_index, *object_index))
                    }
                    SceneRelationshipTarget::WorkspaceObject {
                        file_index,
                        object_index,
                    } => Some((*file_index, *object_index)),
                    target => {
                        component_list_incomplete = true;
                        push_workspace_scene_root_issue(
                            &mut discovery,
                            limits.max_issues,
                            WorkspaceSceneRootDiscoveryIssue {
                                source_file_index,
                                game_object_index,
                                transform_file_index: selected_transform.map(|identity| identity.0),
                                transform_index: selected_transform.map(|identity| identity.1),
                                kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedComponent {
                                    target: target.clone(),
                                },
                            },
                        )?;
                        None
                    }
                };
                let Some((target_file_index, target_object_index)) = target_identity else {
                    continue;
                };
                match graph.object_class_id(target_file_index, target_object_index) {
                    Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {
                        transform_match_count = transform_match_count.checked_add(1).ok_or(
                            SceneRootDiscoveryError::CountOverflow {
                                resource: "workspace Transform components",
                            },
                        )?;
                        if selected_transform.is_none() {
                            selected_transform = Some((target_file_index, target_object_index));
                        }
                    }
                    Some(_) => {}
                    None => {
                        component_list_incomplete = true;
                        push_workspace_scene_root_issue(
                            &mut discovery,
                            limits.max_issues,
                            WorkspaceSceneRootDiscoveryIssue {
                                source_file_index,
                                game_object_index,
                                transform_file_index: selected_transform.map(|identity| identity.0),
                                transform_index: selected_transform.map(|identity| identity.1),
                                kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedComponent {
                                    target: relationship.target.clone(),
                                },
                            },
                        )?;
                    }
                }
            }
            if transform_match_count > 1 {
                if let Some((transform_file_index, selected_transform_index)) = selected_transform {
                    push_workspace_scene_root_issue(
                        &mut discovery,
                        limits.max_issues,
                        WorkspaceSceneRootDiscoveryIssue {
                            source_file_index,
                            game_object_index,
                            transform_file_index: Some(transform_file_index),
                            transform_index: Some(selected_transform_index),
                            kind:
                                WorkspaceSceneRootDiscoveryIssueKind::MultipleTransformComponents {
                                    selected_transform_file_index: transform_file_index,
                                    selected_transform_index,
                                    match_count: transform_match_count,
                                },
                        },
                    )?;
                }
            }
            if component_list_incomplete {
                continue;
            }

            let Some((transform_file_index, transform_index)) = selected_transform else {
                push_workspace_scene_root_candidate(
                    &mut discovery,
                    limits.max_roots,
                    WorkspaceSceneRootCandidate {
                        source_file_index,
                        game_object_index,
                        transform_file_index: None,
                        transform_index: None,
                        reason: SceneRootReason::NoTransformComponent,
                    },
                )?;
                continue;
            };
            let Some(transform_identity) =
                graph.object_identity(transform_file_index, transform_index)
            else {
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedComponent {
                            target: SceneRelationshipTarget::WorkspaceObject {
                                file_index: transform_file_index,
                                object_index: transform_index,
                            },
                        },
                    },
                )?;
                continue;
            };
            let mut father_references = graph
                .references_from_object(transform_file_index, transform_index)
                .filter(|reference| {
                    reference.source_path_id == transform_identity.path_id
                        && reference_field_path_matches(reference, "m_Father")
                });
            let Some(father_reference) = father_references.next() else {
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::MissingTransformParent,
                    },
                )?;
                continue;
            };
            if father_references.next().is_some() {
                let match_count = graph
                    .references_from_object(transform_file_index, transform_index)
                    .filter(|reference| {
                        reference.source_path_id == transform_identity.path_id
                            && reference_field_path_matches(reference, "m_Father")
                    })
                    .count();
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::DuplicateTransformParent {
                            match_count,
                        },
                    },
                )?;
                continue;
            }
            match &father_reference.resolution {
                MultiFilePPtrResolution::Null => push_workspace_scene_root_candidate(
                    &mut discovery,
                    limits.max_roots,
                    WorkspaceSceneRootCandidate {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        reason: SceneRootReason::NullFather,
                    },
                )?,
                MultiFilePPtrResolution::Object {
                    target_file_index,
                    target_object_index,
                } if matches!(
                    graph.object_class_id(*target_file_index, *target_object_index),
                    Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                ) => {}
                resolution => push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                            target: scene_target_from_multi_file_resolution(
                                resolution,
                                transform_file_index,
                            ),
                        },
                    },
                )?,
            }
        }
        Ok(discovery)
    }

    /// Audit reciprocal parent/child pointers whose targets resolve inside this scene graph.
    ///
    /// Cross-file targets are not checked here because this graph contains scene values for only
    /// one file. The source editor conversion requires each non-root Transform to appear in its
    /// father's child array when it calculates root order; this method reports that and related
    /// contradictions without panicking or repairing the serialized values.
    pub fn audit_local_transform_links(
        &self,
    ) -> Result<Vec<SceneHierarchyIssue>, SceneGraphAuditError> {
        let relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::TransformParent | SceneRelationshipKind::TransformChild
                )
            })
            .count();
        let mut index = Vec::new();
        index.try_reserve_exact(relationship_count).map_err(|_| {
            SceneGraphAuditError::AllocationFailed {
                resource: "local Transform relationship index",
                requested: relationship_count,
            }
        })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            if !matches!(
                relationship.kind,
                SceneRelationshipKind::TransformParent | SceneRelationshipKind::TransformChild
            ) {
                continue;
            }
            index.push(IndexedHierarchyRelationship {
                source_object_index: relationship.source_object_index,
                kind: relationship.kind,
                target_local_object_index: match relationship.target {
                    SceneRelationshipTarget::LocalObject { object_index } => Some(object_index),
                    _ => None,
                },
                relationship_index,
            });
        }
        index.sort_unstable_by_key(|entry| {
            (
                entry.source_object_index,
                hierarchy_kind_rank(entry.kind),
                entry.target_local_object_index.unwrap_or(usize::MAX),
                entry.relationship_index,
            )
        });

        let mut issues = Vec::new();
        issues.try_reserve_exact(relationship_count).map_err(|_| {
            SceneGraphAuditError::AllocationFailed {
                resource: "local Transform consistency issues",
                requested: relationship_count,
            }
        })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            let counterpart_kind = match relationship.kind {
                SceneRelationshipKind::TransformParent => SceneRelationshipKind::TransformChild,
                SceneRelationshipKind::TransformChild => SceneRelationshipKind::TransformParent,
                _ => continue,
            };
            let SceneRelationshipTarget::LocalObject {
                object_index: target_index,
            } = &relationship.target
            else {
                continue;
            };
            let target_index = *target_index;
            let Some(target_object) = self.objects.get(target_index) else {
                continue;
            };
            match target_object.class_id {
                Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {}
                Some(class_id) => {
                    issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetIsNotTransform { class_id },
                    });
                    continue;
                }
                None => {
                    issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetClassUnknown,
                    });
                    continue;
                }
            }

            let counterpart_source_index = target_index;
            let expected_target_index = relationship.source_object_index;
            let range =
                hierarchy_relationship_range(&index, counterpart_source_index, counterpart_kind);
            let entries = &index[range];
            match relationship.kind {
                SceneRelationshipKind::TransformParent => {
                    let match_count = entries
                        .iter()
                        .filter(|entry| {
                            entry.target_local_object_index == Some(expected_target_index)
                        })
                        .count();
                    let kind = match match_count {
                        0 => Some(SceneHierarchyIssueKind::MissingReciprocal { counterpart_kind }),
                        1 => None,
                        match_count => Some(SceneHierarchyIssueKind::DuplicateReciprocal {
                            counterpart_kind,
                            match_count,
                        }),
                    };
                    if let Some(kind) = kind {
                        issues.push(SceneHierarchyIssue {
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            target_object_index: target_index,
                            field_path: relationship.field_path.clone(),
                            kind,
                        });
                    }
                }
                SceneRelationshipKind::TransformChild => match entries {
                    [] => issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::MissingReciprocal { counterpart_kind },
                    }),
                    [entry] if entry.target_local_object_index == Some(expected_target_index) => {}
                    [entry] => {
                        let counterpart = self.relationships.get(entry.relationship_index).ok_or(
                            SceneGraphAuditError::InvalidRelationshipIndex {
                                relationship_index: entry.relationship_index,
                                relationship_count: self.relationships.len(),
                            },
                        )?;
                        issues.push(SceneHierarchyIssue {
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            target_object_index: target_index,
                            field_path: relationship.field_path.clone(),
                            kind: SceneHierarchyIssueKind::MismatchedReciprocalTarget {
                                counterpart_kind,
                                actual_target: counterpart.target.clone(),
                            },
                        });
                    }
                    entries => issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::DuplicateReciprocal {
                            counterpart_kind,
                            match_count: entries.len(),
                        },
                    }),
                },
                _ => {}
            }
        }
        Ok(issues)
    }

    /// Audit reciprocal Transform parent/child pointers across every loaded SerializedFile.
    ///
    /// A missing reciprocal edge is reported only when the target object's reference scan is
    /// complete. If its TypeTree payload or one of its fields could not be scanned, the result is
    /// marked incomplete instead of claiming that the edge is absent.
    pub fn audit_workspace_transform_links(
        &mut self,
        source_file_index: usize,
        source_file_name: &str,
        graph: &MultiFileReferenceGraph<'_>,
        limits: SceneHierarchyAuditLimits,
    ) -> Result<WorkspaceSceneHierarchyAudit, SceneGraphAuditError> {
        self.resolve_with_multi_file_graph(source_file_index, source_file_name, graph)
            .map_err(|source| SceneGraphAuditError::ReferenceGraphMismatch {
                message: source.to_string(),
            })?;

        let relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::TransformParent | SceneRelationshipKind::TransformChild
                )
            })
            .count();
        if relationship_count > limits.max_relationships {
            return Err(SceneGraphAuditError::RelationshipLimitExceeded {
                count: relationship_count,
                limit: limits.max_relationships,
            });
        }

        let mut audit = WorkspaceSceneHierarchyAudit {
            relationships_checked: 0,
            issues: Vec::new(),
        };
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            let counterpart_kind = match relationship.kind {
                SceneRelationshipKind::TransformParent => SceneRelationshipKind::TransformChild,
                SceneRelationshipKind::TransformChild => SceneRelationshipKind::TransformParent,
                _ => continue,
            };
            audit.relationships_checked = audit.relationships_checked.checked_add(1).ok_or(
                SceneGraphAuditError::CountOverflow {
                    resource: "workspace Transform relationships",
                },
            )?;

            if graph.object_scan_is_complete(source_file_index, relationship.source_object_index)
                != Some(true)
            {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: None,
                        target_object_index: None,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::SourceScanIncomplete,
                    },
                )?;
                continue;
            }

            if relationship.kind == SceneRelationshipKind::TransformParent
                && relationship.target == SceneRelationshipTarget::Null
            {
                continue;
            }

            let (target_file_index, target_object_index) = match &relationship.target {
                SceneRelationshipTarget::LocalObject { object_index } => {
                    (source_file_index, *object_index)
                }
                SceneRelationshipTarget::WorkspaceObject {
                    file_index,
                    object_index,
                } => (*file_index, *object_index),
                target => {
                    push_workspace_hierarchy_issue(
                        &mut audit,
                        limits.max_issues,
                        WorkspaceSceneHierarchyIssue {
                            source_file_index,
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            source_path_id: relationship.source_path_id,
                            target_file_index: None,
                            target_object_index: None,
                            field_path: relationship.field_path.clone(),
                            kind: SceneHierarchyIssueKind::UnresolvedTarget {
                                target: target.clone(),
                            },
                        },
                    )?;
                    continue;
                }
            };

            let Some(target_class_id) =
                graph.object_class_id(target_file_index, target_object_index)
            else {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetClassUnknown,
                    },
                )?;
                continue;
            };
            if !matches!(
                target_class_id,
                TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID
            ) {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetIsNotTransform {
                            class_id: target_class_id,
                        },
                    },
                )?;
                continue;
            }

            if graph.object_scan_is_complete(target_file_index, target_object_index) != Some(true) {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::CounterpartScanIncomplete,
                    },
                )?;
                continue;
            }

            let Some(target_identity) =
                graph.object_identity(target_file_index, target_object_index)
            else {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetClassUnknown,
                    },
                )?;
                continue;
            };
            let mut counterpart_edges = graph
                .references_from_object(target_file_index, target_object_index)
                .filter(|reference| {
                    if reference.source_path_id != target_identity.path_id {
                        return false;
                    }
                    match counterpart_kind {
                        SceneRelationshipKind::TransformParent => {
                            reference_field_path_matches(reference, "m_Father")
                        }
                        SceneRelationshipKind::TransformChild => {
                            reference_field_path_matches_array_element(reference, "m_Children")
                        }
                        _ => false,
                    }
                });
            let Some(counterpart_edge) = counterpart_edges.next() else {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::MissingReciprocal { counterpart_kind },
                    },
                )?;
                continue;
            };

            let Some(_second_edge) = counterpart_edges.next() else {
                if !matches!(
                    counterpart_edge.resolution,
                    MultiFilePPtrResolution::Object {
                        target_file_index: actual_file_index,
                        target_object_index: actual_object_index,
                    } if actual_file_index == source_file_index
                        && actual_object_index == relationship.source_object_index
                ) {
                    push_workspace_hierarchy_issue(
                        &mut audit,
                        limits.max_issues,
                        WorkspaceSceneHierarchyIssue {
                            source_file_index,
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            source_path_id: relationship.source_path_id,
                            target_file_index: Some(target_file_index),
                            target_object_index: Some(target_object_index),
                            field_path: relationship.field_path.clone(),
                            kind: SceneHierarchyIssueKind::MismatchedReciprocalTarget {
                                counterpart_kind,
                                actual_target: scene_target_from_multi_file_resolution(
                                    &counterpart_edge.resolution,
                                    target_file_index,
                                ),
                            },
                        },
                    )?;
                }
                continue;
            };
            let match_count = counterpart_edges.count().checked_add(2).ok_or(
                SceneGraphAuditError::CountOverflow {
                    resource: "workspace reciprocal edge matches",
                },
            )?;
            push_workspace_hierarchy_issue(
                &mut audit,
                limits.max_issues,
                WorkspaceSceneHierarchyIssue {
                    source_file_index,
                    relationship_index,
                    source_object_index: relationship.source_object_index,
                    source_path_id: relationship.source_path_id,
                    target_file_index: Some(target_file_index),
                    target_object_index: Some(target_object_index),
                    field_path: relationship.field_path.clone(),
                    kind: SceneHierarchyIssueKind::DuplicateReciprocal {
                        counterpart_kind,
                        match_count,
                    },
                },
            )?;
        }
        Ok(audit)
    }

    /// Resolve every scene edge through a cross-file graph built from the same ordered workspace.
    ///
    /// The source file name and every object-table PathID are checked before any target is
    /// changed. The update is transactional: a missing or ambiguous field-path match leaves the
    /// original per-file targets intact.
    pub fn resolve_with_multi_file_graph(
        &mut self,
        source_file_index: usize,
        source_file_name: &str,
        graph: &MultiFileReferenceGraph<'_>,
    ) -> Result<(), SceneGraphResolveError> {
        let Some(graph_file_name) = graph.normalized_file_names().get(source_file_index) else {
            return Err(SceneGraphResolveError::SourceFileIndexOutOfBounds {
                file_index: source_file_index,
                file_count: graph.file_count(),
            });
        };
        let normalized_source_name =
            crate::io::special_names::fix_file_identifier(source_file_name);
        if normalized_source_name != *graph_file_name {
            return Err(SceneGraphResolveError::SourceFileNameMismatch {
                file_index: source_file_index,
                expected: graph_file_name.clone(),
                actual: normalized_source_name,
            });
        }
        for object in &self.objects {
            let identity = graph
                .object_identity(source_file_index, object.object_index)
                .ok_or(SceneGraphResolveError::ObjectTableMismatch {
                    file_index: source_file_index,
                    object_index: object.object_index,
                })?;
            if identity.path_id != object.path_id {
                return Err(SceneGraphResolveError::ObjectIdentityMismatch {
                    file_index: source_file_index,
                    object_index: object.object_index,
                    scene_path_id: object.path_id,
                    graph_path_id: identity.path_id,
                });
            }
        }
        let reference_object_count = graph.file_object_count(source_file_index).ok_or(
            SceneGraphResolveError::SourceFileIndexOutOfBounds {
                file_index: source_file_index,
                file_count: graph.file_count(),
            },
        )?;
        if reference_object_count != self.objects.len() {
            return Err(SceneGraphResolveError::ObjectCountMismatch {
                file_index: source_file_index,
                scene_graph_object_count: self.objects.len(),
                reference_object_count,
            });
        }

        let mut resolved_targets = Vec::new();
        resolved_targets
            .try_reserve_exact(self.relationships.len())
            .map_err(|_| SceneGraphResolveError::AllocationFailed {
                requested: self.relationships.len(),
            })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            let mut matches = graph
                .references_from_object(source_file_index, relationship.source_object_index)
                .filter(|reference| {
                    reference.source_path_id == relationship.source_path_id
                        && reference_field_path_matches(reference, &relationship.field_path)
                });
            let Some(reference) = matches.next() else {
                return Err(SceneGraphResolveError::ReferenceFieldNotFound {
                    relationship_index,
                    object_index: relationship.source_object_index,
                    field_path: relationship.field_path.clone(),
                });
            };
            if matches.next().is_some() {
                return Err(SceneGraphResolveError::ReferenceFieldAmbiguous {
                    relationship_index,
                    object_index: relationship.source_object_index,
                    field_path: relationship.field_path.clone(),
                });
            }
            if reference.file_id != relationship.file_id
                || reference.path_id != relationship.path_id
            {
                return Err(SceneGraphResolveError::ReferenceValueMismatch {
                    relationship_index,
                    object_index: relationship.source_object_index,
                    field_path: relationship.field_path.clone(),
                    scene_file_id: relationship.file_id,
                    scene_path_id: relationship.path_id,
                    graph_file_id: reference.file_id,
                    graph_path_id: reference.path_id,
                });
            }
            resolved_targets.push(scene_target_from_multi_file_resolution(
                &reference.resolution,
                source_file_index,
            ));
        }
        for (relationship, target) in self.relationships.iter_mut().zip(resolved_targets) {
            relationship.target = target;
        }
        Ok(())
    }
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

impl<'a> SceneGraphBuilder<'a> {
    fn new(
        metadata: &'a SerializedFileMetadata,
        limits: SceneGraphLimits,
    ) -> Result<Self, SceneGraphBuildError> {
        let object_count = metadata.objects.len();
        if object_count > limits.max_objects {
            return Err(SceneGraphBuildError::ObjectCountLimitExceeded {
                object_count,
                limit: limits.max_objects,
            });
        }
        let mut objects = Vec::new();
        objects.try_reserve_exact(object_count).map_err(|_| {
            SceneGraphBuildError::AllocationFailed {
                resource: "scene object records",
                requested: object_count,
            }
        })?;
        let mut path_id_index = Vec::new();
        path_id_index.try_reserve_exact(object_count).map_err(|_| {
            SceneGraphBuildError::AllocationFailed {
                resource: "scene PathID index",
                requested: object_count,
            }
        })?;
        for (object_index, object) in metadata.objects.iter().enumerate() {
            objects.push(SceneObjectRecord {
                object_index,
                path_id: object.path_id,
                class_id: object_scene_class_id(object),
                name_bytes: None,
                is_active: None,
                transform: None,
            });
            path_id_index.push((object.path_id, object_index));
        }
        path_id_index.sort_unstable();
        Ok(Self {
            metadata,
            limits,
            objects,
            path_id_index,
            relationships: Vec::new(),
            issues: Vec::new(),
            total_decoded_bytes: 0,
            total_name_bytes: 0,
            total_field_path_bytes: 0,
        })
    }

    fn add_payload(&mut self, payload: &ParsedObjectPayload) -> Result<(), SceneGraphBuildError> {
        self.total_decoded_bytes = self.charge(
            self.total_decoded_bytes,
            payload.allocated_bytes,
            self.limits.max_total_decoded_bytes,
            "decoded scene payload bytes",
        )?;
        let object = self.metadata.objects.get(payload.object_index).ok_or(
            SceneGraphBuildError::InvalidObjectIndex {
                object_index: payload.object_index,
                object_count: self.metadata.objects.len(),
            },
        )?;
        self.add_root(
            payload.object_index,
            object.path_id,
            object_scene_class_id(object),
            &payload.root,
        )
    }

    fn add_root(
        &mut self,
        object_index: usize,
        path_id: i64,
        class_id: Option<i32>,
        root: &SerializedValue,
    ) -> Result<(), SceneGraphBuildError> {
        match class_id {
            Some(GAME_OBJECT_CLASS_ID) => self.read_game_object(object_index, path_id, root),
            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => self.read_transform(
                object_index,
                path_id,
                root,
                class_id == Some(RECT_TRANSFORM_CLASS_ID),
            ),
            Some(PREFAB_INSTANCE_CLASS_ID) => {
                self.read_prefab_instance(object_index, path_id, root)
            }
            _ => Ok(()),
        }
    }

    fn read_game_object(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
    ) -> Result<(), SceneGraphBuildError> {
        if let Some(value) = self.unique_field(object_index, path_id, root, "m_Name")? {
            match value {
                SerializedValue::StringBytes(bytes) => {
                    self.total_name_bytes = self.charge(
                        self.total_name_bytes,
                        bytes.len(),
                        self.limits.max_total_name_bytes,
                        "scene object name bytes",
                    )?;
                    self.objects[object_index].name_bytes = Some(bytes.clone());
                }
                other => self.unexpected_type(
                    object_index,
                    path_id,
                    "m_Name",
                    "string",
                    value_type_name(other),
                )?,
            }
        }
        if let Some(value) = self.unique_field(object_index, path_id, root, "m_IsActive")? {
            match value {
                SerializedValue::Bool(is_active) => {
                    self.objects[object_index].is_active = Some(*is_active);
                }
                other => self.unexpected_type(
                    object_index,
                    path_id,
                    "m_IsActive",
                    "bool",
                    value_type_name(other),
                )?,
            }
        }
        let Some(value) = self.unique_field(object_index, path_id, root, "m_Component")? else {
            return Ok(());
        };
        let Some(components) = value.array_elements() else {
            self.unexpected_type(
                object_index,
                path_id,
                "m_Component",
                "array",
                value_type_name(value),
            )?;
            return Ok(());
        };
        for (index, component) in components.iter().enumerate() {
            let field_path = format!("m_Component[{index}]");
            if let Some(pointer) =
                self.component_pointer(object_index, path_id, &field_path, component)?
            {
                self.push_relationship(
                    object_index,
                    path_id,
                    SceneRelationshipKind::GameObjectComponent,
                    pointer.field_path,
                    (pointer.file_id, pointer.path_id),
                )?;
            }
        }
        Ok(())
    }

    fn read_transform(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
        is_rect_transform: bool,
    ) -> Result<(), SceneGraphBuildError> {
        let local_position =
            self.read_vector3_field(object_index, path_id, root, "m_LocalPosition", true)?;
        let local_rotation =
            self.read_quaternion_field(object_index, path_id, root, "m_LocalRotation", true)?;
        let local_scale =
            self.read_vector3_field(object_index, path_id, root, "m_LocalScale", true)?;
        let root_order = self.read_i32_field(object_index, path_id, root, "m_RootOrder")?;
        let local_euler_angles_hint =
            self.read_vector3_field(object_index, path_id, root, "m_LocalEulerAnglesHint", false)?;
        let rect_transform = if is_rect_transform {
            Some(SceneRectTransformValues {
                anchor_min: self.read_vector2_field(
                    object_index,
                    path_id,
                    root,
                    "m_AnchorMin",
                    true,
                )?,
                anchor_max: self.read_vector2_field(
                    object_index,
                    path_id,
                    root,
                    "m_AnchorMax",
                    true,
                )?,
                anchored_position: self.read_vector2_field(
                    object_index,
                    path_id,
                    root,
                    "m_AnchoredPosition",
                    true,
                )?,
                size_delta: self.read_vector2_field(
                    object_index,
                    path_id,
                    root,
                    "m_SizeDelta",
                    true,
                )?,
                pivot: self.read_vector2_field(object_index, path_id, root, "m_Pivot", true)?,
            })
        } else {
            None
        };
        self.objects[object_index].transform = Some(SceneTransformValues {
            local_position,
            local_rotation,
            local_scale,
            root_order,
            local_euler_angles_hint,
            rect_transform,
        });

        for (field_name, kind) in [
            ("m_GameObject", SceneRelationshipKind::ComponentGameObject),
            ("m_Father", SceneRelationshipKind::TransformParent),
        ] {
            let Some(value) = self.unique_field(object_index, path_id, root, field_name)? else {
                continue;
            };
            if let Some(pointer) = self.pointer_value(object_index, path_id, field_name, value)? {
                self.push_relationship(
                    object_index,
                    path_id,
                    kind,
                    field_name.to_owned(),
                    pointer,
                )?;
            }
        }
        let Some(value) = self.unique_field(object_index, path_id, root, "m_Children")? else {
            return Ok(());
        };
        let Some(children) = value.array_elements() else {
            self.unexpected_type(
                object_index,
                path_id,
                "m_Children",
                "array",
                value_type_name(value),
            )?;
            return Ok(());
        };
        for (index, child) in children.iter().enumerate() {
            let field_path = format!("m_Children[{index}]");
            if let Some(pointer) = self.pointer_value(object_index, path_id, &field_path, child)? {
                self.push_relationship(
                    object_index,
                    path_id,
                    SceneRelationshipKind::TransformChild,
                    field_path,
                    pointer,
                )?;
            }
        }
        Ok(())
    }

    fn read_prefab_instance(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
    ) -> Result<(), SceneGraphBuildError> {
        let Some(value) = self.unique_field(object_index, path_id, root, "m_RootGameObject")?
        else {
            return Ok(());
        };
        if let Some(pointer) =
            self.pointer_value(object_index, path_id, "m_RootGameObject", value)?
        {
            self.push_relationship(
                object_index,
                path_id,
                SceneRelationshipKind::PrefabInstanceRootGameObject,
                "m_RootGameObject".to_owned(),
                pointer,
            )?;
        }
        Ok(())
    }

    fn read_vector2_field(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
        field_name: &str,
        required: bool,
    ) -> Result<Option<SceneVector2>, SceneGraphBuildError> {
        let value = if required {
            self.unique_field(object_index, path_id, root, field_name)?
        } else {
            self.optional_field(object_index, path_id, root, field_name)?
        };
        let Some(value) = value else {
            return Ok(None);
        };
        if !matches!(value, SerializedValue::Struct(_)) {
            self.unexpected_type(
                object_index,
                path_id,
                field_name,
                "Vector2",
                value_type_name(value),
            )?;
            return Ok(None);
        }
        let Some(x) = self.read_f32_component(object_index, path_id, value, field_name, "x")?
        else {
            return Ok(None);
        };
        let Some(y) = self.read_f32_component(object_index, path_id, value, field_name, "y")?
        else {
            return Ok(None);
        };
        Ok(Some(SceneVector2 { x, y }))
    }

    fn read_vector3_field(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
        field_name: &str,
        required: bool,
    ) -> Result<Option<SceneVector3>, SceneGraphBuildError> {
        let value = if required {
            self.unique_field(object_index, path_id, root, field_name)?
        } else {
            self.optional_field(object_index, path_id, root, field_name)?
        };
        let Some(value) = value else {
            return Ok(None);
        };
        if !matches!(value, SerializedValue::Struct(_)) {
            self.unexpected_type(
                object_index,
                path_id,
                field_name,
                "Vector3",
                value_type_name(value),
            )?;
            return Ok(None);
        }
        let Some(x) = self.read_f32_component(object_index, path_id, value, field_name, "x")?
        else {
            return Ok(None);
        };
        let Some(y) = self.read_f32_component(object_index, path_id, value, field_name, "y")?
        else {
            return Ok(None);
        };
        let Some(z) = self.read_f32_component(object_index, path_id, value, field_name, "z")?
        else {
            return Ok(None);
        };
        Ok(Some(SceneVector3 { x, y, z }))
    }

    fn read_quaternion_field(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
        field_name: &str,
        required: bool,
    ) -> Result<Option<SceneQuaternion>, SceneGraphBuildError> {
        let value = if required {
            self.unique_field(object_index, path_id, root, field_name)?
        } else {
            self.optional_field(object_index, path_id, root, field_name)?
        };
        let Some(value) = value else {
            return Ok(None);
        };
        if !matches!(value, SerializedValue::Struct(_)) {
            self.unexpected_type(
                object_index,
                path_id,
                field_name,
                "Quaternion",
                value_type_name(value),
            )?;
            return Ok(None);
        }
        let Some(x) = self.read_f32_component(object_index, path_id, value, field_name, "x")?
        else {
            return Ok(None);
        };
        let Some(y) = self.read_f32_component(object_index, path_id, value, field_name, "y")?
        else {
            return Ok(None);
        };
        let Some(z) = self.read_f32_component(object_index, path_id, value, field_name, "z")?
        else {
            return Ok(None);
        };
        let Some(w) = self.read_f32_component(object_index, path_id, value, field_name, "w")?
        else {
            return Ok(None);
        };
        Ok(Some(SceneQuaternion { x, y, z, w }))
    }

    fn read_i32_field(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
        field_name: &str,
    ) -> Result<Option<i32>, SceneGraphBuildError> {
        let Some(value) = self.optional_field(object_index, path_id, root, field_name)? else {
            return Ok(None);
        };
        match value {
            SerializedValue::I32(value) => Ok(Some(*value)),
            other => {
                self.unexpected_type(
                    object_index,
                    path_id,
                    field_name,
                    "i32",
                    value_type_name(other),
                )?;
                Ok(None)
            }
        }
    }

    fn read_f32_component(
        &mut self,
        object_index: usize,
        path_id: i64,
        vector: &SerializedValue,
        vector_path: &str,
        component_name: &str,
    ) -> Result<Option<f32>, SceneGraphBuildError> {
        let component_path = format!("{vector_path}.{component_name}");
        match vector.unique_field(component_name) {
            Ok(Some(SerializedValue::F32(value))) => Ok(Some(*value)),
            Ok(Some(other)) => {
                self.unexpected_type(
                    object_index,
                    path_id,
                    &component_path,
                    "f32",
                    value_type_name(other),
                )?;
                Ok(None)
            }
            Ok(None) => {
                self.push_issue(SceneGraphIssue {
                    object_index,
                    path_id,
                    field_path: Some(component_path),
                    kind: SceneGraphIssueKind::MissingField {
                        field_name: component_name.to_owned(),
                    },
                })?;
                Ok(None)
            }
            Err(error) => {
                self.push_lookup_issue(object_index, path_id, &component_path, error)?;
                Ok(None)
            }
        }
    }

    fn component_pointer(
        &mut self,
        object_index: usize,
        path_id: i64,
        field_path: &str,
        value: &SerializedValue,
    ) -> Result<Option<ScenePointerField>, SceneGraphBuildError> {
        if let Some(pointer) = value.pptr() {
            return Ok(Some(ScenePointerField {
                file_id: pointer.0,
                path_id: pointer.1,
                field_path: field_path.to_owned(),
            }));
        }
        let value = match value.unique_field("component") {
            Ok(Some(value)) => value,
            Ok(None) => {
                self.push_issue(SceneGraphIssue {
                    object_index,
                    path_id,
                    field_path: Some(field_path.to_owned()),
                    kind: SceneGraphIssueKind::MissingField {
                        field_name: "component".to_owned(),
                    },
                })?;
                return Ok(None);
            }
            Err(error) => {
                self.push_lookup_issue(object_index, path_id, field_path, error)?;
                return Ok(None);
            }
        };
        let pointer_field_path = format!("{field_path}.component");
        Ok(self
            .pointer_value(object_index, path_id, &pointer_field_path, value)?
            .map(|pointer| ScenePointerField {
                file_id: pointer.0,
                path_id: pointer.1,
                field_path: pointer_field_path,
            }))
    }

    fn pointer_value(
        &mut self,
        object_index: usize,
        path_id: i64,
        field_path: &str,
        value: &SerializedValue,
    ) -> Result<Option<(i32, i64)>, SceneGraphBuildError> {
        if let Some(pointer) = value.pptr() {
            Ok(Some(pointer))
        } else {
            self.unexpected_type(
                object_index,
                path_id,
                field_path,
                "PPtr",
                value_type_name(value),
            )?;
            Ok(None)
        }
    }

    fn unique_field<'b>(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &'b SerializedValue,
        field_name: &str,
    ) -> Result<Option<&'b SerializedValue>, SceneGraphBuildError> {
        match root.unique_field_in_base_chain(field_name) {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) => {
                self.push_issue(SceneGraphIssue {
                    object_index,
                    path_id,
                    field_path: Some(field_name.to_owned()),
                    kind: SceneGraphIssueKind::MissingField {
                        field_name: field_name.to_owned(),
                    },
                })?;
                Ok(None)
            }
            Err(error) => {
                self.push_lookup_issue(object_index, path_id, field_name, error)?;
                Ok(None)
            }
        }
    }

    fn optional_field<'b>(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &'b SerializedValue,
        field_name: &str,
    ) -> Result<Option<&'b SerializedValue>, SceneGraphBuildError> {
        match root.unique_field_in_base_chain(field_name) {
            Ok(value) => Ok(value),
            Err(error) => {
                self.push_lookup_issue(object_index, path_id, field_name, error)?;
                Ok(None)
            }
        }
    }

    fn push_lookup_issue(
        &mut self,
        object_index: usize,
        path_id: i64,
        field_path: &str,
        error: SerializedValueLookupError,
    ) -> Result<(), SceneGraphBuildError> {
        let kind = match error {
            SerializedValueLookupError::NotStruct => SceneGraphIssueKind::UnexpectedValueType {
                expected: "struct",
                actual: "non-struct",
            },
            SerializedValueLookupError::DuplicateField { name, match_count } => {
                SceneGraphIssueKind::DuplicateField {
                    field_name: name,
                    match_count,
                }
            }
        };
        self.push_issue(SceneGraphIssue {
            object_index,
            path_id,
            field_path: Some(field_path.to_owned()),
            kind,
        })
    }

    fn unexpected_type(
        &mut self,
        object_index: usize,
        path_id: i64,
        field_path: &str,
        expected: &'static str,
        actual: &'static str,
    ) -> Result<(), SceneGraphBuildError> {
        self.push_issue(SceneGraphIssue {
            object_index,
            path_id,
            field_path: Some(field_path.to_owned()),
            kind: SceneGraphIssueKind::UnexpectedValueType { expected, actual },
        })
    }

    fn push_relationship(
        &mut self,
        source_object_index: usize,
        source_path_id: i64,
        kind: SceneRelationshipKind,
        field_path: String,
        (file_id, path_id): (i32, i64),
    ) -> Result<(), SceneGraphBuildError> {
        let requested_count =
            self.relationships
                .len()
                .checked_add(1)
                .ok_or(SceneGraphBuildError::CountOverflow {
                    resource: "scene relationships",
                })?;
        if requested_count > self.limits.max_relationships {
            return Err(SceneGraphBuildError::RelationshipLimitExceeded {
                requested_count,
                limit: self.limits.max_relationships,
            });
        }
        self.total_field_path_bytes = self.charge(
            self.total_field_path_bytes,
            field_path.len(),
            self.limits.max_total_field_path_bytes,
            "scene relationship field paths",
        )?;
        self.relationships
            .try_reserve(1)
            .map_err(|_| SceneGraphBuildError::AllocationFailed {
                resource: "scene relationships",
                requested: requested_count,
            })?;
        let target = self.resolve_pointer(file_id, path_id);
        self.relationships.push(SceneRelationship {
            source_object_index,
            source_path_id,
            kind,
            field_path,
            file_id,
            path_id,
            target,
        });
        Ok(())
    }

    fn resolve_pointer(&self, file_id: i32, path_id: i64) -> SceneRelationshipTarget {
        if path_id == 0 {
            return SceneRelationshipTarget::Null;
        }
        if file_id == 0 {
            let first = self
                .path_id_index
                .partition_point(|(candidate, _)| *candidate < path_id);
            let end = self
                .path_id_index
                .partition_point(|(candidate, _)| *candidate <= path_id);
            return match end - first {
                0 => SceneRelationshipTarget::MissingLocalObject { path_id },
                1 => SceneRelationshipTarget::LocalObject {
                    object_index: self.path_id_index[first].1,
                },
                match_count => SceneRelationshipTarget::AmbiguousLocalObject {
                    path_id,
                    match_count,
                },
            };
        }
        let Some(external_index) = file_id
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
        else {
            return SceneRelationshipTarget::InvalidFileId {
                file_id,
                external_count: self.metadata.externals.len(),
            };
        };
        if external_index >= self.metadata.externals.len() {
            SceneRelationshipTarget::InvalidFileId {
                file_id,
                external_count: self.metadata.externals.len(),
            }
        } else {
            SceneRelationshipTarget::ExternalObject {
                file_id,
                external_index,
                path_id,
            }
        }
    }

    fn push_issue(&mut self, issue: SceneGraphIssue) -> Result<(), SceneGraphBuildError> {
        let requested_count =
            self.issues
                .len()
                .checked_add(1)
                .ok_or(SceneGraphBuildError::CountOverflow {
                    resource: "scene graph issues",
                })?;
        if requested_count > self.limits.max_issues {
            return Err(SceneGraphBuildError::IssueLimitExceeded {
                requested_count,
                limit: self.limits.max_issues,
            });
        }
        self.issues
            .try_reserve(1)
            .map_err(|_| SceneGraphBuildError::AllocationFailed {
                resource: "scene graph issues",
                requested: requested_count,
            })?;
        self.issues.push(issue);
        Ok(())
    }

    fn charge(
        &self,
        current: usize,
        amount: usize,
        limit: usize,
        resource: &'static str,
    ) -> Result<usize, SceneGraphBuildError> {
        let requested = current
            .checked_add(amount)
            .ok_or(SceneGraphBuildError::CountOverflow { resource })?;
        if requested > limit {
            return Err(SceneGraphBuildError::ResourceLimitExceeded {
                resource,
                requested,
                limit,
            });
        }
        Ok(requested)
    }

    fn finish(self) -> Result<SceneGraph, SceneGraphBuildError> {
        Ok(SceneGraph {
            objects: self.objects,
            relationships: self.relationships,
            issues: self.issues,
        })
    }
}

fn object_scene_class_id(object: &ObjectInfo) -> Option<i32> {
    object
        .class_id
        .map(i32::from)
        .or_else(|| object.resolved_type_index.map(|_| object.raw_type_id))
}

fn hierarchy_kind_rank(kind: SceneRelationshipKind) -> u8 {
    match kind {
        SceneRelationshipKind::GameObjectComponent => 0,
        SceneRelationshipKind::ComponentGameObject => 1,
        SceneRelationshipKind::TransformParent => 2,
        SceneRelationshipKind::TransformChild => 3,
        SceneRelationshipKind::PrefabInstanceRootGameObject => 4,
    }
}

fn hierarchy_relationship_range(
    index: &[IndexedHierarchyRelationship],
    source_object_index: usize,
    kind: SceneRelationshipKind,
) -> std::ops::Range<usize> {
    let key = (source_object_index, hierarchy_kind_rank(kind));
    let start = index.partition_point(|entry| {
        (entry.source_object_index, hierarchy_kind_rank(entry.kind)) < key
    });
    let end = index.partition_point(|entry| {
        (entry.source_object_index, hierarchy_kind_rank(entry.kind)) <= key
    });
    start..end
}

fn scene_relationship_range(
    index: &[IndexedSceneRelationship],
    source_object_index: usize,
    kind: SceneRelationshipKind,
) -> std::ops::Range<usize> {
    let key = (source_object_index, hierarchy_kind_rank(kind));
    let start = index.partition_point(|entry| {
        (entry.source_object_index, hierarchy_kind_rank(entry.kind)) < key
    });
    let end = index.partition_point(|entry| {
        (entry.source_object_index, hierarchy_kind_rank(entry.kind)) <= key
    });
    start..end
}

fn push_scene_root_issue(
    issues: &mut Vec<SceneRootDiscoveryIssue>,
    limit: usize,
    issue: SceneRootDiscoveryIssue,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = issues
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "issues" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::IssueCountLimitExceeded { requested, limit });
    }
    issues
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "issues",
            requested,
        })?;
    issues.push(issue);
    Ok(())
}

fn push_scene_root_candidate(
    roots: &mut Vec<SceneRootCandidate>,
    limit: usize,
    root: SceneRootCandidate,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = roots
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "roots" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::RootCountLimitExceeded { requested, limit });
    }
    roots
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "roots",
            requested,
        })?;
    roots.push(root);
    Ok(())
}

fn push_workspace_scene_root_issue(
    discovery: &mut WorkspaceSceneRootDiscovery,
    limit: usize,
    issue: WorkspaceSceneRootDiscoveryIssue,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = discovery
        .issues
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "issues" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::IssueCountLimitExceeded { requested, limit });
    }
    discovery
        .issues
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "workspace root issues",
            requested,
        })?;
    discovery.issues.push(issue);
    Ok(())
}

fn push_workspace_scene_root_candidate(
    discovery: &mut WorkspaceSceneRootDiscovery,
    limit: usize,
    root: WorkspaceSceneRootCandidate,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = discovery
        .roots
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "roots" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::RootCountLimitExceeded { requested, limit });
    }
    discovery
        .roots
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "workspace root candidates",
            requested,
        })?;
    discovery.roots.push(root);
    Ok(())
}

fn push_workspace_hierarchy_issue(
    audit: &mut WorkspaceSceneHierarchyAudit,
    limit: usize,
    issue: WorkspaceSceneHierarchyIssue,
) -> Result<(), SceneGraphAuditError> {
    let requested =
        audit
            .issues
            .len()
            .checked_add(1)
            .ok_or(SceneGraphAuditError::CountOverflow {
                resource: "workspace hierarchy issues",
            })?;
    if requested > limit {
        return Err(SceneGraphAuditError::IssueLimitExceeded {
            count: requested,
            limit,
        });
    }
    audit
        .issues
        .try_reserve(1)
        .map_err(|_| SceneGraphAuditError::AllocationFailed {
            resource: "workspace hierarchy issues",
            requested,
        })?;
    audit.issues.push(issue);
    Ok(())
}

fn is_scene_object_class(class_id: Option<i32>) -> bool {
    matches!(
        class_id,
        Some(
            GAME_OBJECT_CLASS_ID
                | TRANSFORM_CLASS_ID
                | RECT_TRANSFORM_CLASS_ID
                | PREFAB_INSTANCE_CLASS_ID
        )
    )
}

fn value_type_name(value: &SerializedValue) -> &'static str {
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
        SerializedValue::Raw { .. } => "raw",
    }
}

fn reference_field_path_matches(
    reference: &MultiFileObjectReference<'_>,
    relationship_field_path: &str,
) -> bool {
    reference.field_path == relationship_field_path
        || reference
            .field_path
            .strip_suffix(relationship_field_path)
            .and_then(|prefix| prefix.strip_suffix('.'))
            .is_some_and(|prefix| prefix.split('.').all(|segment| segment == "Base"))
}

fn reference_field_path_matches_array_element(
    reference: &MultiFileObjectReference<'_>,
    field_name: &str,
) -> bool {
    let mut path = reference.field_path;
    while let Some(base_path) = path.strip_prefix("Base.") {
        path = base_path;
    }
    let Some(element) = path
        .strip_prefix(field_name)
        .and_then(|value| value.strip_prefix('['))
    else {
        return false;
    };
    let Some((index, suffix)) = element.split_once(']') else {
        return false;
    };
    !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()) && suffix.is_empty()
}

fn scene_target_from_multi_file_resolution(
    resolution: &MultiFilePPtrResolution,
    source_file_index: usize,
) -> SceneRelationshipTarget {
    match resolution {
        MultiFilePPtrResolution::Null => SceneRelationshipTarget::Null,
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } if *target_file_index == source_file_index => SceneRelationshipTarget::LocalObject {
            object_index: *target_object_index,
        },
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => SceneRelationshipTarget::WorkspaceObject {
            file_index: *target_file_index,
            object_index: *target_object_index,
        },
        MultiFilePPtrResolution::MissingLocalObject { path_id } => {
            SceneRelationshipTarget::MissingLocalObject { path_id: *path_id }
        }
        MultiFilePPtrResolution::AmbiguousLocalObject {
            path_id,
            match_count,
        } => SceneRelationshipTarget::AmbiguousLocalObject {
            path_id: *path_id,
            match_count: *match_count,
        },
        MultiFilePPtrResolution::MissingDependency {
            external_index,
            path_id,
        } => SceneRelationshipTarget::MissingDependency {
            external_index: *external_index,
            path_id: *path_id,
        },
        MultiFilePPtrResolution::AmbiguousDependency {
            external_index,
            path_id,
            candidate_count,
        } => SceneRelationshipTarget::AmbiguousDependency {
            external_index: *external_index,
            path_id: *path_id,
            candidate_count: *candidate_count,
        },
        MultiFilePPtrResolution::MissingExternalObject {
            target_file_index,
            path_id,
        } => SceneRelationshipTarget::MissingExternalObject {
            file_index: *target_file_index,
            path_id: *path_id,
        },
        MultiFilePPtrResolution::AmbiguousExternalObject {
            target_file_index,
            path_id,
            match_count,
        } => SceneRelationshipTarget::AmbiguousExternalObject {
            file_index: *target_file_index,
            path_id: *path_id,
            match_count: *match_count,
        },
        MultiFilePPtrResolution::InvalidFileId {
            file_id,
            external_count,
        } => SceneRelationshipTarget::InvalidFileId {
            file_id: *file_id,
            external_count: *external_count,
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneGraphResolveError {
    SourceFileIndexOutOfBounds {
        file_index: usize,
        file_count: usize,
    },
    SourceFileNameMismatch {
        file_index: usize,
        expected: String,
        actual: String,
    },
    ObjectTableMismatch {
        file_index: usize,
        object_index: usize,
    },
    ObjectIdentityMismatch {
        file_index: usize,
        object_index: usize,
        scene_path_id: i64,
        graph_path_id: i64,
    },
    ObjectCountMismatch {
        file_index: usize,
        scene_graph_object_count: usize,
        reference_object_count: usize,
    },
    ReferenceFieldNotFound {
        relationship_index: usize,
        object_index: usize,
        field_path: String,
    },
    ReferenceFieldAmbiguous {
        relationship_index: usize,
        object_index: usize,
        field_path: String,
    },
    ReferenceValueMismatch {
        relationship_index: usize,
        object_index: usize,
        field_path: String,
        scene_file_id: i32,
        scene_path_id: i64,
        graph_file_id: i32,
        graph_path_id: i64,
    },
    AllocationFailed {
        requested: usize,
    },
}

impl Display for SceneGraphResolveError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceFileIndexOutOfBounds {
                file_index,
                file_count,
            } => write!(
                f,
                "scene source file index {file_index} is outside the {file_count}-file reference graph"
            ),
            Self::SourceFileNameMismatch {
                file_index,
                expected,
                actual,
            } => write!(
                f,
                "scene source file {file_index} has normalized name '{actual}', but the reference graph has '{expected}'"
            ),
            Self::ObjectTableMismatch {
                file_index,
                object_index,
            } => write!(
                f,
                "scene object {file_index}:{object_index} is absent from the reference graph"
            ),
            Self::ObjectIdentityMismatch {
                file_index,
                object_index,
                scene_path_id,
                graph_path_id,
            } => write!(
                f,
                "scene object {file_index}:{object_index} has PathID {scene_path_id}, but the reference graph has PathID {graph_path_id}"
            ),
            Self::ObjectCountMismatch {
                file_index,
                scene_graph_object_count,
                reference_object_count,
            } => write!(
                f,
                "scene graph file {file_index} has {scene_graph_object_count} object records, but the reference graph has {reference_object_count}"
            ),
            Self::ReferenceFieldNotFound {
                relationship_index,
                object_index,
                field_path,
            } => write!(
                f,
                "scene relationship {relationship_index} for object {object_index} has no matching decoded reference at '{field_path}'"
            ),
            Self::ReferenceFieldAmbiguous {
                relationship_index,
                object_index,
                field_path,
            } => write!(
                f,
                "scene relationship {relationship_index} for object {object_index} matches multiple decoded references at '{field_path}'"
            ),
            Self::ReferenceValueMismatch {
                relationship_index,
                object_index,
                field_path,
                scene_file_id,
                scene_path_id,
                graph_file_id,
                graph_path_id,
            } => write!(
                f,
                "scene relationship {relationship_index} for object {object_index} at '{field_path}' has PPtr ({scene_file_id}, {scene_path_id}), but the reference graph has ({graph_file_id}, {graph_path_id})"
            ),
            Self::AllocationFailed { requested } => write!(
                f,
                "could not reserve {requested} resolved scene targets"
            ),
        }
    }
}

impl Error for SceneGraphResolveError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneGraphBuildError {
    ObjectCountLimitExceeded {
        object_count: usize,
        limit: usize,
    },
    RelationshipLimitExceeded {
        requested_count: usize,
        limit: usize,
    },
    IssueLimitExceeded {
        requested_count: usize,
        limit: usize,
    },
    ResourceLimitExceeded {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    InvalidObjectIndex {
        object_index: usize,
        object_count: usize,
    },
    DecodedRootCountMismatch {
        expected: usize,
        actual: usize,
    },
    DuplicateDecodedRoot {
        object_index: usize,
    },
    UnexpectedDecodedRoot {
        object_index: usize,
        class_id: Option<i32>,
    },
}

impl Display for SceneGraphBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectCountLimitExceeded { object_count, limit } => write!(
                f,
                "scene graph object count {object_count} exceeds configured limit {limit}"
            ),
            Self::RelationshipLimitExceeded {
                requested_count,
                limit,
            } => write!(
                f,
                "scene graph relationship count {requested_count} exceeds configured limit {limit}"
            ),
            Self::IssueLimitExceeded {
                requested_count,
                limit,
            } => write!(
                f,
                "scene graph issue count {requested_count} exceeds configured limit {limit}"
            ),
            Self::ResourceLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(
                f,
                "scene graph requires {requested} units for {resource}, above configured limit {limit}"
            ),
            Self::CountOverflow { resource } => {
                write!(f, "scene graph count overflows for {resource}")
            }
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(
                f,
                "could not reserve {requested} entries for scene graph {resource}"
            ),
            Self::InvalidObjectIndex {
                object_index,
                object_count,
            } => write!(
                f,
                "decoded scene root refers to object index {object_index}, but the table has {object_count} objects"
            ),
            Self::DecodedRootCountMismatch { expected, actual } => write!(
                f,
                "decoded scene roots contain {actual} entries, but the object table has {expected} scene objects"
            ),
            Self::DuplicateDecodedRoot { object_index } => write!(
                f,
                "decoded scene roots contain object index {object_index} more than once"
            ),
            Self::UnexpectedDecodedRoot {
                object_index,
                class_id,
            } => write!(
                f,
                "decoded scene root at object index {object_index} has non-scene class ID {class_id:?}"
            ),
        }
    }
}

impl Error for SceneGraphBuildError {}

/// The asset kinds `SceneHierarchyObject.Create` sorts on, in source arm order.
///
/// Interface checks (`IGameObject`, `ILevelGameManager`, `ISceneRoots`, …) and the
/// `IsComponentOnGameObject` pointer test need live objects and stay caller-side;
/// the caller reports the winning arm, including which side of the MonoBehaviour
/// gate the asset falls on.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SceneHierarchyAssetKind {
    /// `IGameObject`.
    GameObject,
    /// `IMonoBehaviour` with `IsComponentOnGameObject`.
    AttachedMonoBehaviour,
    /// `IMonoBehaviour` without a GameObject: matched by no arm, ignored.
    DetachedMonoBehaviour,
    /// Any other `IComponent`.
    Component,
    /// `ILevelGameManager`.
    LevelGameManager,
    /// `IPrefabInstance`.
    PrefabInstance,
    /// `ISceneRoots`.
    SceneRoots,
    /// Anything else: matched by no arm, ignored.
    Other,
}

/// Which list `SceneHierarchyObject.Create` files the asset into.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SceneHierarchySlot {
    /// `GameObjects`.
    GameObjects,
    /// `Components`.
    Components,
    /// `Managers`.
    Managers,
    /// `PrefabInstances`.
    PrefabInstances,
    /// `SceneRoots` (a single assignment: repeated roots overwrite, caller keeps last).
    SceneRoots,
    /// Matched by no arm; the asset lands in no list.
    Ignored,
}

/// `SceneHierarchyObject.Create` (`AssetRipper/Source/AssetRipper.Processing/Prefabs/SceneHierarchyObject.cs`
/// lines 82-108): GameObjects, attached MonoBehaviours, and other components file into
/// their lists; managers, prefab instances, and scene roots file into theirs; detached
/// MonoBehaviours and anything else are ignored.
pub const fn classify_scene_hierarchy_asset(kind: SceneHierarchyAssetKind) -> SceneHierarchySlot {
    match kind {
        SceneHierarchyAssetKind::GameObject => SceneHierarchySlot::GameObjects,
        SceneHierarchyAssetKind::AttachedMonoBehaviour | SceneHierarchyAssetKind::Component => {
            SceneHierarchySlot::Components
        }
        SceneHierarchyAssetKind::LevelGameManager => SceneHierarchySlot::Managers,
        SceneHierarchyAssetKind::PrefabInstance => SceneHierarchySlot::PrefabInstances,
        SceneHierarchyAssetKind::SceneRoots => SceneHierarchySlot::SceneRoots,
        SceneHierarchyAssetKind::DetachedMonoBehaviour | SceneHierarchyAssetKind::Other => {
            SceneHierarchySlot::Ignored
        }
    }
}

/// `SceneHierarchyObject.FetchDependencies` edge labels in walk order
/// (`AssetRipper/Source/AssetRipper.Processing/Prefabs/SceneHierarchyObject.cs` lines 43-54
/// with `GameObjectHierarchyObject.FetchDependencies` first): one `{List}[]` label per
/// member of `GameObjects`, `Components`, `PrefabInstances`, and `Managers`, then a single
/// `SceneRoots` label when a root block applies. Counts arrive as caller facts; pointer
/// resolution stays caller-side.
pub fn scene_hierarchy_dependency_fields(
    game_objects: usize,
    components: usize,
    prefab_instances: usize,
    managers: usize,
    has_scene_roots: bool,
) -> Vec<String> {
    let mut fields = Vec::new();
    for _ in 0..game_objects {
        fields.push("GameObjects[]".to_owned());
    }
    for _ in 0..components {
        fields.push("Components[]".to_owned());
    }
    for _ in 0..prefab_instances {
        fields.push("PrefabInstances[]".to_owned());
    }
    for _ in 0..managers {
        fields.push("Managers[]".to_owned());
    }
    if has_scene_roots {
        fields.push("SceneRoots".to_owned());
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::{
        classify_scene_hierarchy_asset, reference_field_path_matches,
        scene_hierarchy_dependency_fields, MultiFileReferenceGraph, SceneGraph,
        SceneGraphIssueKind, SceneGraphLimits, SceneHierarchyAssetKind, SceneHierarchyAuditLimits,
        SceneHierarchyIssueKind, SceneHierarchySlot, SceneQuaternion, SceneRectTransformValues,
        SceneRelationshipKind, SceneRelationshipTarget, SceneRootDiscoveryIssueKind,
        SceneRootDiscoveryLimits, SceneRootReason, SceneTransformValues, SceneVector2,
        SceneVector3,
    };
    use crate::io::binary::Endian;
    use crate::serialized_file::file::SerializedFile;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::metadata::{FileIdentifier, ObjectInfo, SerializedFileMetadata};
    use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
    use crate::serialized_file::object::{SerializedFieldValue as Field, SerializedValue as Value};

    fn metadata(path_ids: &[(i64, i16)]) -> SerializedFileMetadata {
        SerializedFileMetadata {
            prefix: SerializedFileMetadataPrefix {
                metadata_offset: 0,
                metadata_size: 0,
                body_endian: Endian::Little,
                unity_version_text: None,
                target_platform: None,
                has_type_tree: false,
                consumed_bytes: 0,
            },
            types: Vec::new(),
            long_file_id: None,
            objects: path_ids
                .iter()
                .map(|(path_id, class_id)| ObjectInfo {
                    path_id: *path_id,
                    byte_start: 0,
                    byte_size: 0,
                    serialized_type_index: None,
                    raw_type_id: i32::from(*class_id),
                    class_id: Some(*class_id),
                    script_type_index: None,
                    is_destroyed: None,
                    is_stripped: None,
                    resolved_type_index: None,
                })
                .collect(),
            script_types: Vec::new(),
            externals: vec![FileIdentifier {
                asset_path: Some("shared.assets".to_owned()),
                guid: None,
                file_type: None,
                path_name_origin: "shared.assets".to_owned(),
            }],
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        }
    }

    fn field(name: &str, value: Value) -> Field {
        Field {
            name: name.to_owned(),
            value,
        }
    }

    fn pptr(file_id: i32, path_id: i64) -> Value {
        Value::PPtr { file_id, path_id }
    }

    fn vector2(x: f32, y: f32) -> Value {
        Value::Struct(vec![field("x", Value::F32(x)), field("y", Value::F32(y))])
    }

    fn vector3(x: f32, y: f32, z: f32) -> Value {
        Value::Struct(vec![
            field("x", Value::F32(x)),
            field("y", Value::F32(y)),
            field("z", Value::F32(z)),
        ])
    }

    fn quaternion(x: f32, y: f32, z: f32, w: f32) -> Value {
        Value::Struct(vec![
            field("x", Value::F32(x)),
            field("y", Value::F32(y)),
            field("z", Value::F32(z)),
            field("w", Value::F32(w)),
        ])
    }

    fn transform_fields(include_optional_version_fields: bool) -> Vec<Field> {
        transform_fields_with_links(include_optional_version_fields, pptr(0, 0), Vec::new())
    }

    fn transform_fields_with_links(
        include_optional_version_fields: bool,
        father: Value,
        children: Vec<Value>,
    ) -> Vec<Field> {
        let mut fields = vec![
            field("m_GameObject", pptr(0, 10)),
            field("m_LocalRotation", quaternion(0.1, 0.2, 0.3, 0.9)),
            field("m_LocalPosition", vector3(1.25, -2.0, 3.5)),
            field("m_LocalScale", vector3(2.0, 3.0, 4.0)),
            field("m_Children", Value::Array(children)),
            field("m_Father", father),
        ];
        if include_optional_version_fields {
            fields.push(field("m_RootOrder", Value::I32(7)));
            fields.push(field("m_LocalEulerAnglesHint", vector3(10.0, 20.0, 30.0)));
        }
        fields
    }

    fn game_object_fields(name: &str, components: Vec<Value>) -> Vec<Field> {
        vec![
            field("m_Name", Value::StringBytes(name.as_bytes().to_vec())),
            field("m_IsActive", Value::Bool(true)),
            field(
                "m_Component",
                Value::Array(
                    components
                        .into_iter()
                        .map(|component| Value::Struct(vec![field("component", component)]))
                        .collect(),
                ),
            ),
        ]
    }

    #[test]
    fn reads_prefab_instance_root_game_object_pointer() {
        let metadata = metadata(&[(100, 1001), (200, 1)]);
        let roots = vec![
            (
                0,
                Value::Struct(vec![field("m_RootGameObject", pptr(0, 200))]),
            ),
            (1, Value::Struct(game_object_fields("Root", Vec::new()))),
        ];
        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        assert!(graph.is_complete(), "{:?}", graph.issues());
        assert_eq!(graph.relationships().len(), 1);
        let relationship = &graph.relationships()[0];
        assert_eq!(
            relationship.kind,
            SceneRelationshipKind::PrefabInstanceRootGameObject
        );
        assert_eq!(relationship.field_path, "m_RootGameObject");
        assert_eq!(relationship.file_id, 0);
        assert_eq!(relationship.path_id, 200);
        assert_eq!(
            relationship.target,
            SceneRelationshipTarget::LocalObject { object_index: 1 }
        );
    }

    #[test]
    fn distinguishes_null_and_missing_prefab_root_pointer_fields() {
        let metadata = metadata(&[(100, 1001)]);
        let null_graph = SceneGraph::from_decoded_roots(
            &metadata,
            &[(
                0,
                Value::Struct(vec![field("m_RootGameObject", pptr(0, 0))]),
            )],
            SceneGraphLimits::default(),
        )
        .unwrap();
        assert!(null_graph.is_complete());
        assert_eq!(null_graph.relationships().len(), 1);
        assert_eq!(
            null_graph.relationships()[0].target,
            SceneRelationshipTarget::Null
        );

        let missing_graph = SceneGraph::from_decoded_roots(
            &metadata,
            &[(0, Value::Struct(Vec::new()))],
            SceneGraphLimits::default(),
        )
        .unwrap();
        assert!(!missing_graph.is_complete());
        assert!(missing_graph.relationships().is_empty());
        assert!(missing_graph.issues().iter().any(|issue| matches!(
            &issue.kind,
            SceneGraphIssueKind::MissingField { field_name }
                if field_name == "m_RootGameObject"
        )));
    }

    #[test]
    fn resolves_serialized_prefab_root_pointer_across_loaded_files() {
        let prefab_bytes = make_prefab_instance_file(100, 1, 42, Some("shared.assets"));
        let root_bytes = make_game_object_file("Root", 42, None, None);
        let prefab_file = SerializedFile::parse(&prefab_bytes).unwrap();
        let root_file = SerializedFile::parse(&root_bytes).unwrap();
        let prefab_reference_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&prefab_file)
                .unwrap();
        let root_reference_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&root_file)
                .unwrap();
        let files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &prefab_file.metadata,
                graph: &prefab_reference_graph,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &root_file.metadata,
                graph: &root_reference_graph,
            },
        ];
        let references = MultiFileReferenceGraph::build(&files).unwrap();
        let mut scene = SceneGraph::build(&prefab_file, SceneGraphLimits::default()).unwrap();
        assert_eq!(
            scene.relationships().len(),
            1,
            "class={:?}; issues={:?}; decoded={:?}",
            prefab_file.metadata.objects[0].class_id,
            scene.issues(),
            prefab_file.decode_object_payload(0).unwrap().root
        );
        assert_eq!(
            scene.relationships()[0].target,
            SceneRelationshipTarget::ExternalObject {
                file_id: 1,
                external_index: 0,
                path_id: 42,
            }
        );

        scene
            .resolve_with_multi_file_graph(0, "main.assets", &references)
            .unwrap();
        assert_eq!(
            scene.relationships()[0].target,
            SceneRelationshipTarget::WorkspaceObject {
                file_index: 1,
                object_index: 0,
            }
        );
    }

    fn put_u16_le(output: &mut Vec<u8>, value: u16) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_i32_le(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_i64_le(output: &mut Vec<u8>, value: i64) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_f32_le(output: &mut Vec<u8>, value: f32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32_le(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn push_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    #[allow(clippy::too_many_arguments)]
    fn put_tree_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        put_u16_le(output, 1);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, 0);
    }

    fn make_game_object_file(
        name: &str,
        path_id: i32,
        component: Option<(i32, i32)>,
        external_path: Option<&str>,
    ) -> Vec<u8> {
        match component {
            Some(component) => {
                make_game_object_file_with_components(name, path_id, &[component], external_path)
            }
            None => make_game_object_file_with_components(name, path_id, &[], external_path),
        }
    }

    fn make_game_object_file_with_components(
        name: &str,
        path_id: i32,
        components: &[(i32, i32)],
        external_path: Option<&str>,
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let root_type = push_tree_string(&mut strings, "GameObject");
        let empty = push_tree_string(&mut strings, "");
        let name_field = push_tree_string(&mut strings, "m_Name");
        let string_type = push_tree_string(&mut strings, "string");
        let array = push_tree_string(&mut strings, "Array");
        let size = push_tree_string(&mut strings, "size");
        let int_type = push_tree_string(&mut strings, "int");
        let data = push_tree_string(&mut strings, "data");
        let char_type = push_tree_string(&mut strings, "char");
        let active_field = push_tree_string(&mut strings, "m_IsActive");
        let bool_type = push_tree_string(&mut strings, "bool");
        let component_field = push_tree_string(&mut strings, "m_Component");
        let vector_type = push_tree_string(&mut strings, "vector");
        let pptr_type = push_tree_string(&mut strings, "PPtr<Component>");
        let file_id_field = push_tree_string(&mut strings, "m_FileID");
        let path_id_field = push_tree_string(&mut strings, "m_PathID");
        let int32_type = push_tree_string(&mut strings, "SInt32");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 12);
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty, -1, 0),
            (1, string_type, name_field, -1, 1),
            (2, array, array, -1, 2),
            (3, int_type, size, 4, 3),
            (3, char_type, data, 1, 4),
            (1, bool_type, active_field, 1, 5),
            (1, vector_type, component_field, -1, 6),
            (2, array, array, -1, 7),
            (3, int_type, size, 4, 8),
            (3, pptr_type, data, -1, 9),
            (4, int32_type, file_id_field, 4, 10),
            (4, int32_type, path_id_field, 4, 11),
        ] {
            put_tree_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, path_id);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, i32::try_from(name.len()).unwrap());
        payload.extend_from_slice(name.as_bytes());
        let name_padding = (4 - name.len() % 4) % 4;
        payload.resize(payload.len() + name_padding, 0);
        payload.push(1);
        put_i32_le(&mut payload, i32::try_from(components.len()).unwrap());
        for &(file_id, target_path_id) in components {
            put_i32_le(&mut payload, file_id);
            put_i32_le(&mut payload, target_path_id);
        }
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::from(external_path.is_some()));
        if let Some(external_path) = external_path {
            metadata.push(0); // empty asset path
            metadata.extend_from_slice(&[0; 16]); // GUID
            put_i32_le(&mut metadata, 0); // asset type
            metadata.extend_from_slice(external_path.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&FormatVersion::Unknown10.raw().to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_prefab_instance_file(
        prefab_path_id: i32,
        root_file_id: i32,
        root_path_id: i64,
        external_path: Option<&str>,
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let root_type = push_tree_string(&mut strings, "PrefabInstance");
        let empty = push_tree_string(&mut strings, "");
        let root_game_object = push_tree_string(&mut strings, "m_RootGameObject");
        let pptr_game_object = push_tree_string(&mut strings, "PPtr<GameObject>");
        let file_id_field = push_tree_string(&mut strings, "m_FileID");
        let file_id_type = push_tree_string(&mut strings, "SInt32");
        let path_id_field = push_tree_string(&mut strings, "m_PathID");
        let path_id_type = push_tree_string(&mut strings, "SInt64");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 1001);
        put_i32_le(&mut metadata, 4);
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty, -1, 0),
            (1, pptr_game_object, root_game_object, -1, 1),
            (2, file_id_type, file_id_field, 4, 2),
            (2, path_id_type, path_id_field, 8, 3),
        ] {
            put_tree_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, prefab_path_id);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, root_file_id);
        put_i64_le(&mut payload, root_path_id);
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 1001);
        put_u16_le(&mut metadata, 1001);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::from(external_path.is_some()));
        if let Some(external_path) = external_path {
            metadata.push(0);
            metadata.extend_from_slice(&[0; 16]);
            put_i32_le(&mut metadata, 0);
            metadata.extend_from_slice(external_path.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&FormatVersion::Unknown10.raw().to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_transform_file() -> Vec<u8> {
        make_transform_file_with_links(42, 0, 0)
    }

    fn make_transform_file_with_links(
        transform_path_id: i32,
        father_file_id: i32,
        father_path_id: i64,
    ) -> Vec<u8> {
        make_transform_file_with_hierarchy(
            transform_path_id,
            father_file_id,
            father_path_id,
            &[],
            &[],
            false,
        )
    }

    fn make_transform_file_with_hierarchy(
        transform_path_id: i32,
        father_file_id: i32,
        father_path_id: i64,
        children: &[(i32, i64)],
        external_names: &[&str],
        include_unparsed_raw_leaf: bool,
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let root_type = push_tree_string(&mut strings, "Transform");
        let empty = push_tree_string(&mut strings, "");
        let pptr_game_object = push_tree_string(&mut strings, "PPtr<GameObject>");
        let pptr_transform = push_tree_string(&mut strings, "PPtr<Transform>");
        let quaternion_type = push_tree_string(&mut strings, "Quaternion");
        let vector3_type = push_tree_string(&mut strings, "Vector3");
        let float_type = push_tree_string(&mut strings, "float");
        let int_type = push_tree_string(&mut strings, "int");
        let vector_type = push_tree_string(&mut strings, "vector");
        let array_type = push_tree_string(&mut strings, "Array");
        let file_id_field = push_tree_string(&mut strings, "m_FileID");
        let file_id_type = push_tree_string(&mut strings, "SInt32");
        let path_id_field = push_tree_string(&mut strings, "m_PathID");
        let path_id_type = push_tree_string(&mut strings, "SInt64");
        let game_object_field = push_tree_string(&mut strings, "m_GameObject");
        let local_rotation_field = push_tree_string(&mut strings, "m_LocalRotation");
        let local_position_field = push_tree_string(&mut strings, "m_LocalPosition");
        let local_scale_field = push_tree_string(&mut strings, "m_LocalScale");
        let children_field = push_tree_string(&mut strings, "m_Children");
        let father_field = push_tree_string(&mut strings, "m_Father");
        let root_order_field = push_tree_string(&mut strings, "m_RootOrder");
        let euler_hint_field = push_tree_string(&mut strings, "m_LocalEulerAnglesHint");
        let unknown_type = push_tree_string(&mut strings, "UnknownSerializedType");
        let unknown_field = push_tree_string(&mut strings, "m_UnparsedTail");
        let x_field = push_tree_string(&mut strings, "x");
        let y_field = push_tree_string(&mut strings, "y");
        let z_field = push_tree_string(&mut strings, "z");
        let w_field = push_tree_string(&mut strings, "w");
        let size_field = push_tree_string(&mut strings, "size");
        let data_field = push_tree_string(&mut strings, "data");

        let mut nodes = Vec::new();
        let mut add_node = |level, type_offset, name_offset, byte_size| {
            let index = i32::try_from(nodes.len()).unwrap();
            nodes.push((level, type_offset, name_offset, byte_size, index));
        };
        add_node(0, root_type, empty, -1);
        add_node(1, pptr_game_object, game_object_field, -1);
        add_node(2, file_id_type, file_id_field, 4);
        add_node(2, path_id_type, path_id_field, 8);
        add_node(1, quaternion_type, local_rotation_field, 16);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);
        add_node(2, float_type, w_field, 4);
        add_node(1, vector3_type, local_position_field, 12);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);
        add_node(1, vector3_type, local_scale_field, 12);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);
        add_node(1, vector_type, children_field, -1);
        add_node(2, array_type, array_type, -1);
        add_node(3, int_type, size_field, 4);
        add_node(3, pptr_transform, data_field, -1);
        add_node(4, file_id_type, file_id_field, 4);
        add_node(4, path_id_type, path_id_field, 8);
        add_node(1, pptr_transform, father_field, -1);
        add_node(2, file_id_type, file_id_field, 4);
        add_node(2, path_id_type, path_id_field, 8);
        add_node(1, int_type, root_order_field, 4);
        add_node(1, vector3_type, euler_hint_field, 12);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);
        if include_unparsed_raw_leaf {
            add_node(1, unknown_type, unknown_field, 1);
        }

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 4);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in nodes {
            put_tree_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, transform_path_id);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, 0);
        put_i64_le(&mut payload, 0);
        for value in [0.1, 0.2, 0.3, 0.9] {
            put_f32_le(&mut payload, value);
        }
        for value in [1.25, -2.0, 3.5, 2.0, 3.0, 4.0] {
            put_f32_le(&mut payload, value);
        }
        put_i32_le(&mut payload, i32::try_from(children.len()).unwrap());
        for &(file_id, path_id) in children {
            put_i32_le(&mut payload, file_id);
            put_i64_le(&mut payload, path_id);
        }
        put_i32_le(&mut payload, father_file_id); // m_Father file ID
        put_i64_le(&mut payload, father_path_id); // m_Father PathID
        put_i32_le(&mut payload, 7); // m_RootOrder
        for value in [10.0, 20.0, 30.0] {
            put_f32_le(&mut payload, value);
        }
        if include_unparsed_raw_leaf {
            payload.push(0x7f);
        }

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 4);
        put_u16_le(&mut metadata, 4);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
        for external_name in external_names {
            metadata.push(0); // asset path
            metadata.extend_from_slice(&[0; 16]); // GUID
            put_i32_le(&mut metadata, 0); // asset type
            metadata.extend_from_slice(external_name.as_bytes());
            metadata.push(0);
        }
        metadata.push(0); // reference type count

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&FormatVersion::Unknown10.raw().to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    #[test]
    fn reconstructs_game_object_component_and_transform_edges_without_merging_order() {
        let metadata = metadata(&[(10, 1), (20, 4), (30, 4), (40, 23)]);
        let roots = vec![
            (
                0,
                Value::Struct(vec![
                    field(
                        "Base",
                        Value::Struct(vec![
                            field("m_Name", Value::StringBytes(b"Avatar".to_vec())),
                            field("m_IsActive", Value::Bool(true)),
                        ]),
                    ),
                    field(
                        "m_Component",
                        Value::Array(vec![
                            Value::Struct(vec![field("component", pptr(0, 20))]),
                            Value::Struct(vec![field("component", pptr(0, 40))]),
                        ]),
                    ),
                ]),
            ),
            (
                1,
                Value::Struct(vec![field(
                    "Base",
                    Value::Struct(transform_fields_with_links(
                        true,
                        pptr(0, 0),
                        vec![pptr(0, 30)],
                    )),
                )]),
            ),
            (
                2,
                Value::Struct(vec![field(
                    "Base",
                    Value::Struct(transform_fields_with_links(true, pptr(0, 20), Vec::new())),
                )]),
            ),
        ];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();
        assert_eq!(graph.objects().len(), 4);
        assert_eq!(
            graph.objects()[0].name_bytes.as_deref(),
            Some(&b"Avatar"[..])
        );
        assert_eq!(graph.objects()[0].is_active, Some(true));
        assert_eq!(graph.relationships().len(), 7);
        assert_eq!(
            graph
                .relationships()
                .iter()
                .map(|relationship| relationship.kind)
                .collect::<Vec<_>>(),
            [
                SceneRelationshipKind::GameObjectComponent,
                SceneRelationshipKind::GameObjectComponent,
                SceneRelationshipKind::ComponentGameObject,
                SceneRelationshipKind::TransformParent,
                SceneRelationshipKind::TransformChild,
                SceneRelationshipKind::ComponentGameObject,
                SceneRelationshipKind::TransformParent,
            ]
        );
        assert_eq!(
            graph.relationships()[0].target,
            SceneRelationshipTarget::LocalObject { object_index: 1 }
        );
        assert_eq!(
            graph.relationships()[3].target,
            SceneRelationshipTarget::Null
        );
        assert_eq!(
            graph.relationships()[6].target,
            SceneRelationshipTarget::LocalObject { object_index: 1 }
        );
        assert!(graph.issues().is_empty());
        assert!(graph.audit_local_transform_links().unwrap().is_empty());
        assert!(graph.is_complete());
    }

    #[test]
    fn audits_missing_reciprocal_transform_child_links() {
        let metadata = metadata(&[(10, 4), (20, 4)]);
        let mut child_fields = transform_fields(false);
        child_fields
            .iter_mut()
            .find(|field| field.name == "m_Father")
            .unwrap()
            .value = pptr(0, 20);
        let roots = vec![
            (0, Value::Struct(child_fields)),
            (
                1,
                Value::Struct(transform_fields_with_links(false, pptr(0, 0), Vec::new())),
            ),
        ];
        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        let issues = graph.audit_local_transform_links().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0].kind,
            SceneHierarchyIssueKind::MissingReciprocal {
                counterpart_kind: SceneRelationshipKind::TransformChild,
            }
        );
        assert_eq!(issues[0].source_object_index, 0);
        assert_eq!(issues[0].target_object_index, 1);
    }

    #[test]
    fn audits_mismatched_and_duplicate_reciprocal_transform_links() {
        let metadata = metadata(&[(10, 4), (20, 4)]);
        let mismatched_roots = vec![
            (
                0,
                Value::Struct(transform_fields_with_links(
                    false,
                    pptr(0, 0),
                    vec![pptr(0, 20)],
                )),
            ),
            (1, Value::Struct(transform_fields(false))),
        ];
        let mismatched = SceneGraph::from_decoded_roots(
            &metadata,
            &mismatched_roots,
            SceneGraphLimits::default(),
        )
        .unwrap();
        let mismatch_issues = mismatched.audit_local_transform_links().unwrap();
        assert_eq!(mismatch_issues.len(), 1);
        assert_eq!(
            mismatch_issues[0].kind,
            SceneHierarchyIssueKind::MismatchedReciprocalTarget {
                counterpart_kind: SceneRelationshipKind::TransformParent,
                actual_target: SceneRelationshipTarget::Null,
            }
        );

        let duplicate_roots = vec![
            (
                0,
                Value::Struct(transform_fields_with_links(
                    false,
                    pptr(0, 0),
                    vec![pptr(0, 20), pptr(0, 20)],
                )),
            ),
            (
                1,
                Value::Struct(transform_fields_with_links(false, pptr(0, 10), Vec::new())),
            ),
        ];
        let duplicate = SceneGraph::from_decoded_roots(
            &metadata,
            &duplicate_roots,
            SceneGraphLimits::default(),
        )
        .unwrap();
        let duplicate_issues = duplicate.audit_local_transform_links().unwrap();
        assert_eq!(duplicate_issues.len(), 1);
        assert_eq!(
            duplicate_issues[0].kind,
            SceneHierarchyIssueKind::DuplicateReciprocal {
                counterpart_kind: SceneRelationshipKind::TransformChild,
                match_count: 2,
            }
        );
    }

    #[test]
    fn audits_local_parent_pointers_that_target_non_transforms() {
        let metadata = metadata(&[(10, 4), (20, 23)]);
        let mut fields = transform_fields(false);
        fields
            .iter_mut()
            .find(|field| field.name == "m_Father")
            .unwrap()
            .value = pptr(0, 20);
        let roots = vec![(0, Value::Struct(fields))];
        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        let issues = graph.audit_local_transform_links().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0].kind,
            SceneHierarchyIssueKind::TargetIsNotTransform { class_id: 23 }
        );
    }

    #[test]
    fn discovers_transform_roots_and_game_objects_without_transforms() {
        let metadata = metadata(&[(10, 1), (20, 4), (30, 1), (40, 4), (50, 1)]);
        let mut child_transform_fields =
            transform_fields_with_links(false, pptr(0, 20), Vec::new());
        child_transform_fields
            .iter_mut()
            .find(|field| field.name == "m_GameObject")
            .unwrap()
            .value = pptr(0, 30);
        let roots = vec![
            (
                0,
                Value::Struct(game_object_fields("Root", vec![pptr(0, 20)])),
            ),
            (
                1,
                Value::Struct(transform_fields_with_links(
                    false,
                    pptr(0, 0),
                    vec![pptr(0, 40)],
                )),
            ),
            (
                2,
                Value::Struct(game_object_fields("Child", vec![pptr(0, 40)])),
            ),
            (3, Value::Struct(child_transform_fields)),
            (
                4,
                Value::Struct(game_object_fields("NoTransform", Vec::new())),
            ),
        ];
        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        let discovery = graph
            .discover_local_roots(SceneRootDiscoveryLimits::default())
            .unwrap();
        assert_eq!(
            discovery.roots(),
            [
                super::SceneRootCandidate {
                    game_object_index: 0,
                    transform_index: Some(1),
                    reason: SceneRootReason::NullFather,
                },
                super::SceneRootCandidate {
                    game_object_index: 4,
                    transform_index: None,
                    reason: SceneRootReason::NoTransformComponent,
                },
            ]
        );
        assert!(discovery.issues().is_empty());
        assert!(discovery.is_complete());
        assert!(matches!(
            graph.discover_local_roots(SceneRootDiscoveryLimits {
                max_roots: 1,
                ..SceneRootDiscoveryLimits::default()
            }),
            Err(super::SceneRootDiscoveryError::RootCountLimitExceeded {
                requested: 2,
                limit: 1,
            })
        ));
    }

    #[test]
    fn root_discovery_keeps_unresolved_component_and_parent_states_visible() {
        let component_metadata = metadata(&[(10, 1), (20, 4)]);
        let component_roots = vec![
            (
                0,
                Value::Struct(game_object_fields(
                    "Unknown",
                    vec![pptr(0, 99), pptr(0, 20)],
                )),
            ),
            (1, Value::Struct(transform_fields(false))),
        ];
        let component_graph = SceneGraph::from_decoded_roots(
            &component_metadata,
            &component_roots,
            SceneGraphLimits::default(),
        )
        .unwrap();
        let component_discovery = component_graph
            .discover_local_roots(SceneRootDiscoveryLimits::default())
            .unwrap();
        assert!(component_discovery.roots().is_empty());
        assert!(matches!(
            component_discovery.issues()[0].kind,
            SceneRootDiscoveryIssueKind::UnresolvedComponent {
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 99 }
            }
        ));
        assert!(matches!(
            component_graph.discover_local_roots(SceneRootDiscoveryLimits {
                max_issues: 0,
                ..SceneRootDiscoveryLimits::default()
            }),
            Err(super::SceneRootDiscoveryError::IssueCountLimitExceeded {
                requested: 1,
                limit: 0,
            })
        ));

        let parent_metadata = metadata(&[(10, 1), (20, 4)]);
        let mut transform = transform_fields(false);
        transform
            .iter_mut()
            .find(|field| field.name == "m_Father")
            .unwrap()
            .value = pptr(0, 99);
        let parent_roots = vec![
            (
                0,
                Value::Struct(game_object_fields("UnknownParent", vec![pptr(0, 20)])),
            ),
            (1, Value::Struct(transform)),
        ];
        let parent_graph = SceneGraph::from_decoded_roots(
            &parent_metadata,
            &parent_roots,
            SceneGraphLimits::default(),
        )
        .unwrap();
        let parent_discovery = parent_graph
            .discover_local_roots(SceneRootDiscoveryLimits::default())
            .unwrap();
        assert!(parent_discovery.roots().is_empty());
        assert!(parent_discovery.issues().iter().any(|issue| matches!(
            issue.kind,
            SceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 99 }
            }
        )));
    }

    #[test]
    fn decodes_transform_and_rect_transform_values_from_type_tree_fields() {
        let metadata = metadata(&[(10, 4), (20, 224)]);
        let mut rect_fields = vec![field("Base", Value::Struct(transform_fields(true)))];
        rect_fields.extend([
            field("m_AnchorMin", vector2(0.1, 0.2)),
            field("m_AnchorMax", vector2(0.8, 0.9)),
            field("m_AnchoredPosition", vector2(12.0, -6.0)),
            field("m_SizeDelta", vector2(320.0, 180.0)),
            field("m_Pivot", vector2(0.5, 1.0)),
        ]);
        let roots = vec![
            (0, Value::Struct(transform_fields(false))),
            (1, Value::Struct(rect_fields)),
        ];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        assert_eq!(
            graph.objects()[0].transform,
            Some(SceneTransformValues {
                local_position: Some(SceneVector3 {
                    x: 1.25,
                    y: -2.0,
                    z: 3.5,
                }),
                local_rotation: Some(SceneQuaternion {
                    x: 0.1,
                    y: 0.2,
                    z: 0.3,
                    w: 0.9,
                }),
                local_scale: Some(SceneVector3 {
                    x: 2.0,
                    y: 3.0,
                    z: 4.0,
                }),
                root_order: None,
                local_euler_angles_hint: None,
                rect_transform: None,
            })
        );
        assert_eq!(
            graph.objects()[1]
                .transform
                .as_ref()
                .unwrap()
                .rect_transform,
            Some(SceneRectTransformValues {
                anchor_min: Some(SceneVector2 { x: 0.1, y: 0.2 }),
                anchor_max: Some(SceneVector2 { x: 0.8, y: 0.9 }),
                anchored_position: Some(SceneVector2 { x: 12.0, y: -6.0 }),
                size_delta: Some(SceneVector2 { x: 320.0, y: 180.0 }),
                pivot: Some(SceneVector2 { x: 0.5, y: 1.0 }),
            })
        );
        assert_eq!(
            graph.objects()[1].transform.as_ref().unwrap().root_order,
            Some(7)
        );
        assert_eq!(
            graph.objects()[1]
                .transform
                .as_ref()
                .unwrap()
                .local_euler_angles_hint,
            Some(SceneVector3 {
                x: 10.0,
                y: 20.0,
                z: 30.0,
            })
        );
        assert!(graph.issues().is_empty());
    }

    #[test]
    fn reports_malformed_transform_values_without_filling_defaults() {
        let metadata = metadata(&[(10, 4)]);
        let mut fields = transform_fields(false);
        fields
            .iter_mut()
            .find(|field| field.name == "m_LocalPosition")
            .unwrap()
            .value = Value::Bool(false);
        let roots = vec![(0, Value::Struct(fields))];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        assert_eq!(
            graph.objects()[0]
                .transform
                .as_ref()
                .unwrap()
                .local_position,
            None
        );
        assert!(graph.issues().iter().any(|issue| matches!(
            issue.kind,
            SceneGraphIssueKind::UnexpectedValueType {
                expected: "Vector3",
                actual: "bool",
            }
        )));
        assert!(!graph.is_complete());
    }

    #[test]
    fn builds_a_scene_graph_from_a_complete_serialized_file() {
        let bytes = make_game_object_file("Root", 42, Some((0, 42)), None);
        let file = SerializedFile::parse(&bytes).unwrap();
        let mut graph = SceneGraph::build(&file, SceneGraphLimits::default()).unwrap();

        assert_eq!(graph.objects().len(), 1);
        assert_eq!(graph.objects()[0].path_id, 42);
        assert_eq!(graph.objects()[0].class_id, Some(1));
        assert_eq!(graph.objects()[0].name_bytes.as_deref(), Some(&b"Root"[..]));
        assert_eq!(graph.objects()[0].is_active, Some(true));
        assert_eq!(graph.relationships().len(), 1);
        assert_eq!(graph.relationships()[0].field_path, "m_Component[0]");
        assert_eq!(
            graph.relationships()[0].target,
            SceneRelationshipTarget::LocalObject { object_index: 0 }
        );
        assert!(graph.issues().is_empty());
        assert!(graph.is_complete());

        let per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&file).unwrap();
        let named_file = crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "GameObject.assets",
            metadata: &file.metadata,
            graph: &per_file_graph,
        };
        let multi_file_graph = MultiFileReferenceGraph::build(&[named_file]).unwrap();
        let target_before_failed_resolution = graph.relationships()[0].target.clone();
        assert!(matches!(
            graph.resolve_with_multi_file_graph(0, "wrong.assets", &multi_file_graph),
            Err(super::SceneGraphResolveError::SourceFileNameMismatch { .. })
        ));
        assert_eq!(
            graph.relationships()[0].target,
            target_before_failed_resolution
        );
        graph
            .resolve_with_multi_file_graph(0, "GAMEOBJECT.ASSETS", &multi_file_graph)
            .unwrap();
        assert_eq!(
            graph.relationships()[0].target,
            SceneRelationshipTarget::LocalObject { object_index: 0 }
        );
    }

    #[test]
    fn builds_transform_values_from_a_complete_2019_serialized_file() {
        let bytes = make_transform_file();
        let file = SerializedFile::parse(&bytes).unwrap();
        let graph = SceneGraph::build(&file, SceneGraphLimits::default()).unwrap();

        assert_eq!(graph.objects().len(), 1);
        assert_eq!(graph.objects()[0].class_id, Some(4));
        assert_eq!(
            graph.objects()[0].transform,
            Some(SceneTransformValues {
                local_position: Some(SceneVector3 {
                    x: 1.25,
                    y: -2.0,
                    z: 3.5,
                }),
                local_rotation: Some(SceneQuaternion {
                    x: 0.1,
                    y: 0.2,
                    z: 0.3,
                    w: 0.9,
                }),
                local_scale: Some(SceneVector3 {
                    x: 2.0,
                    y: 3.0,
                    z: 4.0,
                }),
                root_order: Some(7),
                local_euler_angles_hint: Some(SceneVector3 {
                    x: 10.0,
                    y: 20.0,
                    z: 30.0,
                }),
                rect_transform: None,
            })
        );
        assert_eq!(graph.relationships().len(), 2);
        assert!(graph.issues().is_empty());
    }

    #[test]
    fn resolves_scene_component_links_across_loaded_serialized_files() {
        let source_bytes =
            make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
        let target_bytes = make_game_object_file("Target", 99, None, None);
        let source_file = SerializedFile::parse(&source_bytes).unwrap();
        let target_file = SerializedFile::parse(&target_bytes).unwrap();
        let source_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&source_file)
                .unwrap();
        let target_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&target_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_file.metadata,
                graph: &source_per_file_graph,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &target_file.metadata,
                graph: &target_per_file_graph,
            },
        ];
        let reference_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut scene = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
        assert!(matches!(
            scene.relationships()[0].target,
            SceneRelationshipTarget::ExternalObject {
                file_id: 1,
                external_index: 0,
                path_id: 99,
            }
        ));

        scene
            .resolve_with_multi_file_graph(0, "main.assets", &reference_graph)
            .unwrap();
        assert_eq!(
            scene.relationships()[0].target,
            SceneRelationshipTarget::WorkspaceObject {
                file_index: 1,
                object_index: 0,
            }
        );
        assert!(scene.is_complete());

        let source_only = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_file.metadata,
                graph: &source_per_file_graph,
            },
        ];
        let missing_reference_graph = MultiFileReferenceGraph::build(&source_only).unwrap();
        let mut missing_scene =
            SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
        missing_scene
            .resolve_with_multi_file_graph(0, "main.assets", &missing_reference_graph)
            .unwrap();
        assert_eq!(
            missing_scene.relationships()[0].target,
            SceneRelationshipTarget::MissingDependency {
                external_index: 0,
                path_id: 99,
            }
        );
        assert!(!missing_scene.is_complete());

        let no_edge_bytes = make_game_object_file("Source", 42, None, Some("shared.assets"));
        let no_edge_file = SerializedFile::parse(&no_edge_bytes).unwrap();
        let no_edge_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&no_edge_file)
                .unwrap();
        let no_edge_named = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &no_edge_file.metadata,
                graph: &no_edge_per_file_graph,
            },
        ];
        let no_edge_graph = MultiFileReferenceGraph::build(&no_edge_named).unwrap();
        let mut unmatched_scene =
            SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
        let original_target = unmatched_scene.relationships()[0].target.clone();
        assert!(matches!(
            unmatched_scene.resolve_with_multi_file_graph(0, "main.assets", &no_edge_graph),
            Err(super::SceneGraphResolveError::ReferenceFieldNotFound { .. })
        ));
        assert_eq!(unmatched_scene.relationships()[0].target, original_target);
    }

    #[test]
    fn refuses_scene_reference_join_when_same_identity_has_a_different_pptr_value() {
        let scene_bytes = make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
        let graph_bytes = make_game_object_file("Source", 42, Some((1, 98)), Some("shared.assets"));
        let scene_file = SerializedFile::parse(&scene_bytes).unwrap();
        let graph_file = SerializedFile::parse(&graph_bytes).unwrap();
        let per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&graph_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &graph_file.metadata,
                graph: &per_file_graph,
            },
        ];
        let reference_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut scene = SceneGraph::build(&scene_file, SceneGraphLimits::default()).unwrap();
        let original_target = scene.relationships()[0].target.clone();

        assert!(matches!(
            scene.resolve_with_multi_file_graph(0, "main.assets", &reference_graph),
            Err(super::SceneGraphResolveError::ReferenceValueMismatch {
                scene_file_id: 1,
                scene_path_id: 99,
                graph_file_id: 1,
                graph_path_id: 98,
                ..
            })
        ));
        assert_eq!(scene.relationships()[0].target, original_target);
    }

    #[test]
    fn discovers_workspace_roots_when_transform_component_is_external() {
        let source_bytes =
            make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
        let target_bytes = make_transform_file_with_links(99, 0, 0);
        let source_file = SerializedFile::parse(&source_bytes).unwrap();
        let target_file = SerializedFile::parse(&target_bytes).unwrap();
        let source_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&source_file)
                .unwrap();
        let target_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&target_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_file.metadata,
                graph: &source_per_file_graph,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &target_file.metadata,
                graph: &target_per_file_graph,
            },
        ];
        let reference_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut scene = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();

        let roots = scene
            .discover_workspace_roots(
                0,
                "main.assets",
                &reference_graph,
                SceneRootDiscoveryLimits::default(),
            )
            .unwrap();
        assert_eq!(
            roots.roots(),
            &[super::WorkspaceSceneRootCandidate {
                source_file_index: 0,
                game_object_index: 0,
                transform_file_index: Some(1),
                transform_index: Some(0),
                reason: SceneRootReason::NullFather,
            }]
        );
        assert!(roots.issues().is_empty());
        assert!(roots.is_complete());
    }

    #[test]
    fn workspace_root_discovery_does_not_hide_missing_external_transform_parent() {
        let source_bytes =
            make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
        let target_bytes = make_transform_file_with_links(99, 0, 777);
        let source_file = SerializedFile::parse(&source_bytes).unwrap();
        let target_file = SerializedFile::parse(&target_bytes).unwrap();
        let source_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&source_file)
                .unwrap();
        let target_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&target_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_file.metadata,
                graph: &source_per_file_graph,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &target_file.metadata,
                graph: &target_per_file_graph,
            },
        ];
        let reference_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut scene = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();

        let roots = scene
            .discover_workspace_roots(
                0,
                "main.assets",
                &reference_graph,
                SceneRootDiscoveryLimits::default(),
            )
            .unwrap();
        assert!(roots.roots().is_empty());
        assert_eq!(
            roots.issues(),
            &[super::WorkspaceSceneRootDiscoveryIssue {
                source_file_index: 0,
                game_object_index: 0,
                transform_file_index: Some(1),
                transform_index: Some(0),
                kind: super::WorkspaceSceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                    target: SceneRelationshipTarget::MissingLocalObject { path_id: 777 },
                },
            }]
        );
        assert!(!roots.is_complete());
    }

    #[test]
    fn scene_definition_owns_compacted_cross_file_hierarchy_data() {
        use crate::serialized_file::reference_graph::{
            MultiFileReferenceGraph, NamedObjectReferenceGraph, ObjectReferenceGraph,
        };

        let source_bytes = make_game_object_file_with_components(
            "SceneObject",
            42,
            &[(1, 99), (0, 0)],
            Some("shared.assets"),
        );
        let target_bytes =
            make_transform_file_with_hierarchy(99, 0, 0, &[], &["main.assets"], false);
        let source_file = SerializedFile::parse(&source_bytes).unwrap();
        let target_file = SerializedFile::parse(&target_bytes).unwrap();
        let source_references = ObjectReferenceGraph::build(&source_file).unwrap();
        let target_references = ObjectReferenceGraph::build(&target_file).unwrap();
        let named_files = [
            NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_file.metadata,
                graph: &source_references,
            },
            NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &target_file.metadata,
                graph: &target_references,
            },
        ];
        let multi_file_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut scene_graph = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
        let roots = scene_graph
            .discover_workspace_roots(
                0,
                "main.assets",
                &multi_file_graph,
                SceneRootDiscoveryLimits::default(),
            )
            .unwrap();
        let audit = scene_graph
            .audit_workspace_transform_links(
                0,
                "main.assets",
                &multi_file_graph,
                super::SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        let plan = crate::processing::scene_collections::plan_scene_definition(
            0,
            "main.assets",
            &crate::processing::scene_collections::SceneCollectionPathStatus::BuildSettingsMissing,
            None,
            None,
            crate::processing::scene_collections::SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        let definitions =
            crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
                vec![plan],
                2,
                crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
            )
            .unwrap();

        let limited = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
            max_scene_objects: 0,
            ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
        };
        let mut bounded =
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(2, limited)
                .unwrap();
        assert!(matches!(
            bounded.add_collection_graph(
                0,
                &scene_graph,
                roots.clone(),
                audit.clone(),
                &multi_file_graph,
            ),
            Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
                resource: "scene objects",
                requested: 1,
                limit: 0,
            })
        ));
        assert!(!bounded.contains_collection_graph(0));

        let graph_limited = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
            max_workspace_collection_graphs: 0,
            ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
        };
        let mut bounded_graphs =
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
                2,
                graph_limited,
            )
            .unwrap();
        assert!(matches!(
            bounded_graphs.add_collection_graph(
                0,
                &scene_graph,
                roots.clone(),
                audit.clone(),
                &multi_file_graph,
            ),
            Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
                resource: "workspace hierarchy collection graphs",
                requested: 1,
                limit: 0,
            })
        ));
        assert!(!bounded_graphs.contains_collection_graph(0));

        let asset_limited = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
            max_scene_asset_records: 1,
            ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
        };
        let mut bounded_assets =
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
                2,
                asset_limited,
            )
            .unwrap();
        assert!(matches!(
            bounded_assets.add_collection_graph(
                0,
                &scene_graph,
                roots.clone(),
                audit.clone(),
                &multi_file_graph,
            ),
            Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
                resource: "scene asset and generated-transform records",
                requested: 2,
                limit: 1,
            })
        ));
        assert!(!bounded_assets.contains_collection_graph(0));

        let non_scene_definitions =
            crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
                Vec::new(),
                2,
                crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
            )
            .unwrap();
        let mut non_scene_builder =
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
                2,
                crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
            )
            .unwrap();
        non_scene_builder
            .add_collection_graph(
                0,
                &scene_graph,
                roots.clone(),
                audit.clone(),
                &multi_file_graph,
            )
            .unwrap();
        let non_scene_hierarchies = non_scene_builder.finish(&non_scene_definitions).unwrap();
        assert!(non_scene_hierarchies.scenes().is_empty());
        assert_eq!(non_scene_hierarchies.non_scene_collections().len(), 1);
        assert_eq!(
            non_scene_hierarchies.non_scene_collections()[0].collection_file_index(),
            0
        );
        assert_eq!(
            non_scene_hierarchies.non_scene_collections()[0].game_object_count(),
            1
        );

        let mut builder = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
            2,
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
        )
        .unwrap();
        builder
            .add_collection_graph(0, &scene_graph, roots, audit, &multi_file_graph)
            .unwrap();
        let hierarchy = builder.finish(&definitions).unwrap();
        assert_eq!(hierarchy.scenes().len(), 1);
        assert_eq!(hierarchy.scenes()[0].plan.collection_file_index, 0);
        let collections = hierarchy.scenes()[0].collections();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].game_object_count(), 1);
        assert_eq!(collections[0].transform_count(), 0);
        assert_eq!(collections[0].scene_assets().len(), 1);
        assert_eq!(
            collections[0].scene_assets()[0],
            crate::processing::scene_hierarchy::WorkspaceSceneAssetIdentity {
                identity: crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
                    file_index: 0,
                    object_index: 0,
                    path_id: 42,
                    class_id: Some(1),
                },
                category:
                    crate::processing::scene_hierarchy::WorkspaceSceneAssetCategory::GameObject,
            }
        );
        assert_eq!(collections[0].component_candidates().len(), 1);
        assert_eq!(
            collections[0].component_candidates()[0],
            crate::processing::scene_hierarchy::WorkspaceSceneComponentCandidate {
                identity: crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
                    file_index: 1,
                    object_index: 0,
                    path_id: 99,
                    class_id: Some(4),
                },
                evidence: crate::processing::scene_hierarchy::WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            }
        );
        assert_eq!(collections[0].relationships().len(), 2);
        assert_eq!(collections[0].game_object_hierarchies().len(), 1);
        assert_eq!(
            collections[0].game_object_hierarchies()[0].game_object(),
            crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
                file_index: 0,
                object_index: 0,
                path_id: 42,
                class_id: Some(1),
            }
        );
        assert_eq!(
            collections[0].game_object_hierarchies()[0].component_relationship_count(),
            2
        );
        let ordered_components: Vec<_> = collections[0]
            .game_object_component_relationships(0)
            .unwrap()
            .collect();
        assert_eq!(ordered_components.len(), 2);
        assert_eq!(ordered_components[0].field_path, "m_Component[0]");
        assert_eq!(ordered_components[0].file_id, 1);
        assert_eq!(ordered_components[0].path_id, 99);
        assert_eq!(
            ordered_components[0].target,
            SceneRelationshipTarget::WorkspaceObject {
                file_index: 1,
                object_index: 0,
            }
        );
        assert_eq!(ordered_components[1].field_path, "m_Component[1]");
        assert_eq!(ordered_components[1].target, SceneRelationshipTarget::Null);
        assert!(matches!(
            collections[0].game_object_component_relationships(1),
            Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::GameObjectHierarchyIndexOutOfBounds {
                hierarchy_index: 1,
                hierarchy_count: 1,
            })
        ));
        assert_eq!(collections[0].roots().roots().len(), 1);
        assert_eq!(
            collections[0].roots().roots()[0].transform_file_index,
            Some(1)
        );
        assert!(collections[0].is_complete());
    }

    #[test]
    fn scene_definition_plans_source_defaults_for_a_missing_game_object_transform() {
        use crate::serialized_file::reference_graph::{
            MultiFileReferenceGraph, NamedObjectReferenceGraph, ObjectReferenceGraph,
        };

        let bytes = make_game_object_file("NoTransform", 42, None, None);
        let file = SerializedFile::parse(&bytes).unwrap();
        let object_references = ObjectReferenceGraph::build(&file).unwrap();
        let named_file = NamedObjectReferenceGraph {
            file_name: "scene.assets",
            metadata: &file.metadata,
            graph: &object_references,
        };
        let references = MultiFileReferenceGraph::build(&[named_file]).unwrap();
        let mut scene_graph = SceneGraph::build(&file, SceneGraphLimits::default()).unwrap();
        let roots = scene_graph
            .discover_workspace_roots(
                0,
                "scene.assets",
                &references,
                SceneRootDiscoveryLimits::default(),
            )
            .unwrap();
        assert_eq!(roots.roots().len(), 1);
        assert_eq!(
            roots.roots()[0].reason,
            SceneRootReason::NoTransformComponent
        );
        let mut bounded_non_scene_builder =
            crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanBuilder::new(
                1,
                crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanLimits {
                    max_missing_transform_plans: 0,
                    ..crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanLimits::default()
                },
            )
            .unwrap();
        assert!(matches!(
            bounded_non_scene_builder.add_file(0, &roots, &references),
            Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
                resource: "workspace missing-Transform plans",
                requested: 1,
                limit: 0,
            })
        ));
        assert!(!bounded_non_scene_builder.contains_file(0));
        let audit = scene_graph
            .audit_workspace_transform_links(
                0,
                "scene.assets",
                &references,
                SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        let plan = crate::processing::scene_collections::plan_scene_definition(
            0,
            "scene.assets",
            &crate::processing::scene_collections::SceneCollectionPathStatus::BuildSettingsMissing,
            None,
            None,
            crate::processing::scene_collections::SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        let definitions =
            crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
                vec![plan],
                1,
                crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
            )
            .unwrap();
        let mut non_scene_plan_builder =
            crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanBuilder::new(
                1,
                crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanLimits::default(
                ),
            )
            .unwrap();
        non_scene_plan_builder
            .add_file(0, &roots, &references)
            .unwrap();
        assert!(non_scene_plan_builder.contains_file(0));
        let no_scene_definitions =
            crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
                Vec::new(),
                1,
                crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
            )
            .unwrap();
        let non_scene_plans = non_scene_plan_builder
            .finish(&no_scene_definitions)
            .unwrap();
        assert_eq!(non_scene_plans.plans().len(), 1);
        assert_eq!(non_scene_plans.plans()[0].game_object.path_id, 42);

        let mut scene_plan_builder =
            crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanBuilder::new(
                1,
                crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanLimits::default(
                ),
            )
            .unwrap();
        scene_plan_builder.add_file(0, &roots, &references).unwrap();
        assert!(scene_plan_builder
            .finish(&definitions)
            .unwrap()
            .plans()
            .is_empty());

        let mut bounded_builder =
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
                1,
                crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
                    max_scene_asset_records: 1,
                    ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
                },
            )
            .unwrap();
        assert!(matches!(
            bounded_builder.add_collection_graph(
                0,
                &scene_graph,
                roots.clone(),
                audit.clone(),
                &references,
            ),
            Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
                resource: "scene asset and generated-transform records",
                requested: 2,
                limit: 1,
            })
        ));
        assert!(!bounded_builder.contains_collection_graph(0));
        let mut builder = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
            1,
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
        )
        .unwrap();
        builder
            .add_collection_graph(0, &scene_graph, roots, audit, &references)
            .unwrap();

        let hierarchy = builder.finish(&definitions).unwrap();
        let plans = hierarchy.scenes()[0].collections()[0].missing_transform_plans();
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].game_object,
            crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
                file_index: 0,
                object_index: 0,
                path_id: 42,
                class_id: Some(1),
            }
        );
        assert_eq!(
            plans[0].default_values,
            crate::processing::scene_hierarchy::WorkspaceGeneratedTransformDefaults {
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
        );
    }

    #[test]
    fn audits_reciprocal_transform_edges_across_serialized_files() {
        let child_bytes =
            make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
        let parent_bytes =
            make_transform_file_with_hierarchy(99, 0, 0, &[(1, 42)], &["child.assets"], false);
        let child_file = SerializedFile::parse(&child_bytes).unwrap();
        let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
        let child_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file)
                .unwrap();
        let parent_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "child.assets",
                metadata: &child_file.metadata,
                graph: &child_references,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "parent.assets",
                metadata: &parent_file.metadata,
                graph: &parent_references,
            },
        ];
        let references = MultiFileReferenceGraph::build(&named_files).unwrap();

        let mut child_scene = SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();
        let child_audit = child_scene
            .audit_workspace_transform_links(
                0,
                "child.assets",
                &references,
                super::SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        assert_eq!(child_audit.relationships_checked(), 1);
        assert!(child_audit.issues().is_empty());
        assert!(child_audit.is_complete());

        let mut parent_scene =
            SceneGraph::build(&parent_file, SceneGraphLimits::default()).unwrap();
        let parent_audit = parent_scene
            .audit_workspace_transform_links(
                1,
                "parent.assets",
                &references,
                super::SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        assert_eq!(parent_audit.relationships_checked(), 2);
        assert!(parent_audit.issues().is_empty());
        assert!(parent_audit.is_complete());
    }

    #[test]
    fn reports_missing_cross_file_transform_reciprocal_without_inventing_a_match() {
        let child_bytes =
            make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
        let parent_bytes = make_transform_file_with_hierarchy(99, 0, 0, &[], &[], false);
        let child_file = SerializedFile::parse(&child_bytes).unwrap();
        let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
        let child_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file)
                .unwrap();
        let parent_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "child.assets",
                metadata: &child_file.metadata,
                graph: &child_references,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "parent.assets",
                metadata: &parent_file.metadata,
                graph: &parent_references,
            },
        ];
        let references = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut child_scene = SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();

        let audit = child_scene
            .audit_workspace_transform_links(
                0,
                "child.assets",
                &references,
                super::SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        assert_eq!(audit.relationships_checked(), 1);
        assert_eq!(
            audit.issues(),
            &[super::WorkspaceSceneHierarchyIssue {
                source_file_index: 0,
                relationship_index: 1,
                source_object_index: 0,
                source_path_id: 42,
                target_file_index: Some(1),
                target_object_index: Some(0),
                field_path: "m_Father".to_owned(),
                kind: super::SceneHierarchyIssueKind::MissingReciprocal {
                    counterpart_kind: SceneRelationshipKind::TransformChild,
                },
            }]
        );
        assert!(!audit.is_complete());
    }

    #[test]
    fn workspace_transform_audit_enforces_relationship_and_issue_limits() {
        let child_bytes =
            make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
        let parent_bytes = make_transform_file_with_hierarchy(99, 0, 0, &[], &[], false);
        let child_file = SerializedFile::parse(&child_bytes).unwrap();
        let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
        let child_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file)
                .unwrap();
        let parent_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "child.assets",
                metadata: &child_file.metadata,
                graph: &child_references,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "parent.assets",
                metadata: &parent_file.metadata,
                graph: &parent_references,
            },
        ];
        let references = MultiFileReferenceGraph::build(&named_files).unwrap();

        let mut relationship_limited_scene =
            SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();
        assert!(matches!(
            relationship_limited_scene.audit_workspace_transform_links(
                0,
                "child.assets",
                &references,
                super::SceneHierarchyAuditLimits {
                    max_relationships: 0,
                    max_issues: 1,
                },
            ),
            Err(super::SceneGraphAuditError::RelationshipLimitExceeded { count: 1, limit: 0 })
        ));

        let mut issue_limited_scene =
            SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();
        assert!(matches!(
            issue_limited_scene.audit_workspace_transform_links(
                0,
                "child.assets",
                &references,
                super::SceneHierarchyAuditLimits {
                    max_relationships: 1,
                    max_issues: 0,
                },
            ),
            Err(super::SceneGraphAuditError::IssueLimitExceeded { count: 1, limit: 0 })
        ));
    }

    #[test]
    fn does_not_call_cross_file_reciprocal_missing_when_target_scan_is_incomplete() {
        let child_bytes =
            make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
        let parent_bytes = make_transform_file_with_hierarchy(99, 0, 0, &[], &[], true);
        let child_file = SerializedFile::parse(&child_bytes).unwrap();
        let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
        let child_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file)
                .unwrap();
        let parent_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file)
                .unwrap();
        assert!(!parent_references.object_scan_is_complete(0));
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "child.assets",
                metadata: &child_file.metadata,
                graph: &child_references,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "parent.assets",
                metadata: &parent_file.metadata,
                graph: &parent_references,
            },
        ];
        let references = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut child_scene = SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();

        let audit = child_scene
            .audit_workspace_transform_links(
                0,
                "child.assets",
                &references,
                super::SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        assert_eq!(audit.relationships_checked(), 1);
        assert_eq!(
            audit.issues(),
            &[super::WorkspaceSceneHierarchyIssue {
                source_file_index: 0,
                relationship_index: 1,
                source_object_index: 0,
                source_path_id: 42,
                target_file_index: Some(1),
                target_object_index: Some(0),
                field_path: "m_Father".to_owned(),
                kind: super::SceneHierarchyIssueKind::CounterpartScanIncomplete,
            }]
        );
        assert!(!audit.is_complete());
    }

    #[test]
    fn does_not_report_transform_audit_complete_when_source_scan_is_incomplete() {
        let child_bytes =
            make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], true);
        let parent_bytes =
            make_transform_file_with_hierarchy(99, 0, 0, &[(1, 42)], &["child.assets"], false);
        let child_file = SerializedFile::parse(&child_bytes).unwrap();
        let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
        let child_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file)
                .unwrap();
        let parent_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file)
                .unwrap();
        assert!(!child_references.object_scan_is_complete(0));
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "child.assets",
                metadata: &child_file.metadata,
                graph: &child_references,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "parent.assets",
                metadata: &parent_file.metadata,
                graph: &parent_references,
            },
        ];
        let references = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut child_scene = SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();

        let audit = child_scene
            .audit_workspace_transform_links(
                0,
                "child.assets",
                &references,
                super::SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        assert_eq!(audit.relationships_checked(), 1);
        assert_eq!(
            audit.issues(),
            &[super::WorkspaceSceneHierarchyIssue {
                source_file_index: 0,
                relationship_index: 1,
                source_object_index: 0,
                source_path_id: 42,
                target_file_index: None,
                target_object_index: None,
                field_path: "m_Father".to_owned(),
                kind: super::SceneHierarchyIssueKind::SourceScanIncomplete,
            }]
        );
        assert!(!audit.is_complete());
    }

    #[test]
    fn matches_inherited_scene_reference_field_paths_without_matching_neighbors() {
        let reference = crate::serialized_file::reference_graph::MultiFileObjectReference {
            source_file_index: 0,
            source_object_index: 4,
            source_path_id: 42,
            field_path: "Base.m_Component[0].component",
            file_id: 0,
            path_id: 0,
            resolution: crate::serialized_file::reference_graph::MultiFilePPtrResolution::Null,
        };
        assert!(reference_field_path_matches(
            &reference,
            "m_Component[0].component"
        ));
        assert!(!reference_field_path_matches(
            &reference,
            "m_Component[1].component"
        ));
        assert!(!reference_field_path_matches(&reference, "component"));

        for (field_path, expected) in [
            ("Base.Base.m_Children[12]", true),
            ("m_Children[0]", true),
            ("m_Children[]", false),
            ("m_Children[not-an-index]", false),
            ("m_Children[0].unexpected", false),
            ("m_Children[0]suffix", false),
        ] {
            let array_reference =
                crate::serialized_file::reference_graph::MultiFileObjectReference {
                    source_file_index: 0,
                    source_object_index: 4,
                    source_path_id: 42,
                    field_path,
                    file_id: 0,
                    path_id: 99,
                    resolution:
                        crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                            target_file_index: 0,
                            target_object_index: 5,
                        },
                };
            assert_eq!(
                super::reference_field_path_matches_array_element(&array_reference, "m_Children"),
                expected,
                "unexpected array-field match for {field_path}"
            );
        }
    }

    #[test]
    fn preserves_all_cross_file_reference_failure_categories() {
        use crate::serialized_file::reference_graph::MultiFilePPtrResolution as Resolution;

        let cases = [
            (
                Resolution::AmbiguousDependency {
                    external_index: 2,
                    path_id: 70,
                    candidate_count: 3,
                },
                SceneRelationshipTarget::AmbiguousDependency {
                    external_index: 2,
                    path_id: 70,
                    candidate_count: 3,
                },
            ),
            (
                Resolution::MissingExternalObject {
                    target_file_index: 4,
                    path_id: 71,
                },
                SceneRelationshipTarget::MissingExternalObject {
                    file_index: 4,
                    path_id: 71,
                },
            ),
            (
                Resolution::AmbiguousExternalObject {
                    target_file_index: 5,
                    path_id: 72,
                    match_count: 2,
                },
                SceneRelationshipTarget::AmbiguousExternalObject {
                    file_index: 5,
                    path_id: 72,
                    match_count: 2,
                },
            ),
            (
                Resolution::InvalidFileId {
                    file_id: -1,
                    external_count: 2,
                },
                SceneRelationshipTarget::InvalidFileId {
                    file_id: -1,
                    external_count: 2,
                },
            ),
        ];
        for (resolution, expected) in cases {
            assert_eq!(
                super::scene_target_from_multi_file_resolution(&resolution, 0),
                expected
            );
        }
    }

    #[test]
    fn uses_resolved_serialized_types_for_modern_object_class_ids() {
        let mut metadata = metadata(&[(42, 1)]);
        metadata.objects[0].class_id = None;
        metadata.objects[0].resolved_type_index = Some(0);
        let roots = vec![(
            0,
            Value::Struct(vec![
                field("m_Name", Value::StringBytes(b"Modern".to_vec())),
                field("m_IsActive", Value::Bool(true)),
                field("m_Component", Value::Array(Vec::new())),
            ]),
        )];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();
        assert_eq!(graph.objects()[0].class_id, Some(1));
        assert!(graph.issues().is_empty());
    }

    #[test]
    fn keeps_external_missing_duplicate_and_malformed_relationships_explicit() {
        let metadata = metadata(&[(10, 1), (20, 4), (20, 23)]);
        let roots = vec![
            (
                0,
                Value::Struct(vec![
                    field("m_Name", Value::StringBytes(vec![0xff])),
                    field("m_IsActive", Value::Bool(false)),
                    field(
                        "m_Component",
                        Value::Array(vec![
                            Value::Struct(vec![field("component", pptr(1, 99))]),
                            Value::Struct(vec![field("component", pptr(0, 20))]),
                            Value::Struct(vec![field("component", pptr(0, 88))]),
                        ]),
                    ),
                ]),
            ),
            (
                1,
                Value::Struct(vec![
                    field(
                        "Base",
                        Value::Struct(vec![
                            field("m_GameObject", pptr(-1, 10)),
                            field("m_LocalPosition", vector3(0.0, 0.0, 0.0)),
                            field("m_LocalRotation", quaternion(0.0, 0.0, 0.0, 1.0)),
                            field("m_LocalScale", vector3(1.0, 1.0, 1.0)),
                        ]),
                    ),
                    field("m_Father", pptr(0, 0)),
                    field("m_Father", pptr(0, 20)),
                    field("m_Children", Value::Bool(true)),
                ]),
            ),
        ];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();
        assert_eq!(graph.objects()[0].name_bytes.as_deref(), Some(&[0xff][..]));
        assert!(matches!(
            graph.relationships()[0].target,
            SceneRelationshipTarget::ExternalObject {
                external_index: 0,
                path_id: 99,
                ..
            }
        ));
        assert_eq!(
            graph.relationships()[1].target,
            SceneRelationshipTarget::AmbiguousLocalObject {
                path_id: 20,
                match_count: 2,
            }
        );
        assert_eq!(
            graph.relationships()[2].target,
            SceneRelationshipTarget::MissingLocalObject { path_id: 88 }
        );
        assert_eq!(
            graph.relationships()[3].target,
            SceneRelationshipTarget::InvalidFileId {
                file_id: -1,
                external_count: 1,
            }
        );
        assert!(graph.issues().iter().any(|issue| matches!(
            issue.kind,
            SceneGraphIssueKind::DuplicateField {
                field_name: ref name,
                match_count: 2,
            } if name == "m_Father"
        )));
        assert!(graph.issues().iter().any(|issue| matches!(
            issue.kind,
            SceneGraphIssueKind::UnexpectedValueType {
                expected: "array",
                actual: "bool",
            }
        )));
        assert!(!graph.is_complete());
    }

    #[test]
    fn enforces_object_and_relationship_limits() {
        let metadata = metadata(&[(10, 1)]);
        let roots = vec![(
            0,
            Value::Struct(vec![
                field("m_Name", Value::StringBytes(Vec::new())),
                field("m_IsActive", Value::Bool(true)),
                field("m_Component", Value::Array(vec![pptr(0, 0)])),
            ]),
        )];
        let limits = SceneGraphLimits {
            max_objects: 0,
            ..SceneGraphLimits::default()
        };
        assert!(SceneGraph::from_decoded_roots(&metadata, &roots, limits).is_err());

        let limits = SceneGraphLimits {
            max_relationships: 0,
            ..SceneGraphLimits::default()
        };
        assert!(SceneGraph::from_decoded_roots(&metadata, &roots, limits).is_err());
    }

    #[test]
    fn rejects_incomplete_or_duplicate_decoded_root_sets() {
        let metadata = metadata(&[(10, 1), (20, 4)]);
        assert!(
            SceneGraph::from_decoded_roots(&metadata, &[], SceneGraphLimits::default()).is_err()
        );

        let root = Value::Struct(Vec::new());
        let roots = vec![(0, root.clone()), (0, root)];
        assert!(matches!(
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()),
            Err(super::SceneGraphBuildError::DuplicateDecodedRoot { object_index: 0 })
        ));
    }

    #[test]
    fn hierarchy_create_sorts_every_arm_into_its_list() {
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::GameObject),
            SceneHierarchySlot::GameObjects
        );
        // Only MonoBehaviours on a GameObject join Components; detached ones are ignored.
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::AttachedMonoBehaviour),
            SceneHierarchySlot::Components
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::Component),
            SceneHierarchySlot::Components
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::DetachedMonoBehaviour),
            SceneHierarchySlot::Ignored
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::LevelGameManager),
            SceneHierarchySlot::Managers
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::PrefabInstance),
            SceneHierarchySlot::PrefabInstances
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::SceneRoots),
            SceneHierarchySlot::SceneRoots
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::Other),
            SceneHierarchySlot::Ignored
        );
    }

    #[test]
    fn hierarchy_dependency_fields_follow_the_fetch_order() {
        assert_eq!(
            scene_hierarchy_dependency_fields(2, 1, 1, 1, true),
            vec![
                "GameObjects[]",
                "GameObjects[]",
                "Components[]",
                "PrefabInstances[]",
                "Managers[]",
                "SceneRoots",
            ]
        );
        // The SceneRoots edge exists only when a root block applies.
        assert_eq!(
            scene_hierarchy_dependency_fields(1, 0, 0, 0, false),
            vec!["GameObjects[]"]
        );
        assert!(scene_hierarchy_dependency_fields(0, 0, 0, 0, false).is_empty());
    }
}
