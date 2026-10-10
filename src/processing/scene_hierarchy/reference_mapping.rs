use super::*;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct WorkspaceObjectKey {
    pub(super) file_index: usize,
    pub(super) object_index: usize,
}

impl From<WorkspaceSceneObjectIdentity> for WorkspaceObjectKey {
    fn from(identity: WorkspaceSceneObjectIdentity) -> Self {
        Self {
            file_index: identity.file_index,
            object_index: identity.object_index,
        }
    }
}
pub(super) enum RelationshipIdentityResolution {
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
pub(super) fn workspace_object_identity(
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

pub(super) fn resolve_relationship_identity(
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
