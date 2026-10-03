//! SceneDefinition-owned workspace views of recovered GameObject and Transform data.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::reference_graph::MultiFileReferenceGraph;

use super::scene::{
    SceneGraph, SceneGraphIssue, SceneObjectRecord, SceneRelationship, SceneRootReason,
    WorkspaceSceneHierarchyAudit, WorkspaceSceneRootDiscovery,
};
use super::scene_collections::{SceneDefinitionPlan, WorkspaceSceneDefinitions};

const GAME_OBJECT_CLASS_ID: i32 = 1;
const TRANSFORM_CLASS_ID: i32 = 4;
const RECT_TRANSFORM_CLASS_ID: i32 = 224;

/// Aggregate limits for SceneDefinition-owned recovered scene graphs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyLimits {
    pub max_workspace_files: usize,
    pub max_scene_definitions: usize,
    pub max_scene_collections: usize,
    pub max_scene_objects: usize,
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
            max_scene_collections: 1_000_000,
            max_scene_objects: 20_000_000,
            max_relationships: 5_000_000,
            max_issues: 500_000,
            max_total_name_bytes: 256 * 1024 * 1024,
            max_total_diagnostic_bytes: 256 * 1024 * 1024,
        }
    }
}

/// A recovered scene-collection graph attached to one loaded SerializedFile collection.
///
/// This retains the scene parser's known GameObject/Transform objects, every recognized scene
/// PPtr, root discovery, and hierarchy audit diagnostics. It does not claim that unrecognized
/// component types have been materialized as generated Unity objects.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSceneCollectionHierarchy {
    collection_file_index: usize,
    scene_objects: Vec<SceneObjectRecord>,
    relationships: Vec<SceneRelationship>,
    graph_issues: Vec<SceneGraphIssue>,
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

    pub fn graph_issues(&self) -> &[SceneGraphIssue] {
        &self.graph_issues
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
}

/// Scene graphs associated with source collection ownership, in SceneDefinition order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceSceneHierarchies {
    scenes: Vec<WorkspaceSceneHierarchyScene>,
}

impl WorkspaceSceneHierarchies {
    pub fn scenes(&self) -> &[WorkspaceSceneHierarchyScene] {
        &self.scenes
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

/// Incrementally compacts scene graphs so the inspect path does not retain full per-file indexes.
pub struct WorkspaceSceneHierarchyBuilder {
    collection_graphs: Vec<Option<WorkspaceSceneCollectionHierarchy>>,
    limits: WorkspaceSceneHierarchyLimits,
    scene_collection_count: usize,
    scene_object_count: usize,
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
    UnassignedCollectionGraph {
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
            Self::UnassignedCollectionGraph { file_index } => write!(
                f,
                "scene graph for workspace file {file_index} is not assigned to a SceneDefinition"
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
            scene_collection_count: 0,
            scene_object_count: 0,
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

        let next_scene_collection_count = checked_charge(
            self.scene_collection_count,
            1,
            self.limits.max_scene_collections,
            "scene collection graphs",
        )?;
        let next_scene_object_count = checked_charge(
            self.scene_object_count,
            scene_objects,
            self.limits.max_scene_objects,
            "scene objects",
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
            graph_issues,
            source_graph_complete: scene_graph.is_complete(),
            source_reference_scans_complete,
            roots,
            hierarchy_audit,
        };
        self.collection_graphs[file_index] = Some(snapshot);
        self.scene_collection_count = next_scene_collection_count;
        self.scene_object_count = next_scene_object_count;
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
        if let Some((file_index, _)) = self
            .collection_graphs
            .iter()
            .enumerate()
            .find(|(_, graph)| graph.is_some())
        {
            return Err(WorkspaceSceneHierarchyError::UnassignedCollectionGraph { file_index });
        }
        Ok(WorkspaceSceneHierarchies { scenes })
    }
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
