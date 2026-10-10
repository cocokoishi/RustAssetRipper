use super::*;
use crate::serialized_file::reference_graph::MultiFileReferenceGraph;
pub(super) fn validate_collection_graph(
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
