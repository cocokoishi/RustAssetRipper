use super::*;
pub(super) fn build_game_object_component_relationship_index(
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

pub(super) fn build_prefab_root_relationship_index(
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

pub(super) fn build_transform_hierarchy_index(
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
