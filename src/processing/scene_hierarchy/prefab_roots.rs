use super::reference_mapping::{resolve_relationship_identity, RelationshipIdentityResolution};
use super::*;

pub(super) fn resolve_transform_game_object(
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

impl WorkspaceSceneHierarchies {
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
}
