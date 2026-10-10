use super::*;
use crate::processing::scene_collections::scene_manager_kind;
use crate::serialized_file::metadata::TypeTree;
use crate::serialized_file::reference_graph::MultiFileReferenceGraph;
pub(super) fn is_game_object_pointer_path(field_path: &str) -> bool {
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

pub(super) fn normalize_component_candidates(
    candidates: &mut Vec<WorkspaceSceneComponentCandidate>,
) {
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TypeTreeComponentBaseClassification {
    Component,
    NotComponent,
    Unavailable,
    Ambiguous,
}

pub(super) fn classify_type_tree_component_base(
    type_tree: Option<&TypeTree>,
) -> TypeTreeComponentBaseClassification {
    let Some(type_tree) = type_tree else {
        return TypeTreeComponentBaseClassification::Unavailable;
    };
    let Some(root) = type_tree.nodes.first() else {
        // SerializedTypes can retain an empty node list when the file has no TypeTree. That
        // absence cannot support a Component-vs-asset classification and must not become a
        // fabricated component candidate (especially for explicit raw-fallback objects).
        return TypeTreeComponentBaseClassification::Unavailable;
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

pub(super) struct ClassifiedSceneCollectionObjects {
    pub(super) scene_assets: Vec<WorkspaceSceneAssetIdentity>,
    pub(super) lighting_data_asset_field_object_indices: Vec<usize>,
    pub(super) component_candidates: Vec<WorkspaceSceneComponentCandidate>,
}

pub(super) fn classify_scene_collection_objects(
    file_index: usize,
    scene_graph: &SceneGraph,
    has_lighting_data_asset: Option<bool>,
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
            let class_id = references
                .object_class_id(file_index, object.object_index)
                .or(object.class_id);
            class_id == Some(LIGHTMAP_SETTINGS_CLASS_ID)
                && has_lighting_data_asset.unwrap_or_else(|| {
                    references
                        .references_from_object(file_index, object.object_index)
                        .any(|reference| is_lighting_data_asset_field_path(reference.field_path))
                })
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
            && has_lighting_data_asset.unwrap_or_else(|| {
                references
                    .references_from_object(file_index, object.object_index)
                    .any(|reference| is_lighting_data_asset_field_path(reference.field_path))
            })
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

pub(super) fn is_lighting_data_asset_field_path(field_path: &str) -> bool {
    field_path
        .rsplit('.')
        .next()
        .is_some_and(|field_name| field_name == "m_LightingDataAsset")
}

pub(super) fn scene_asset_category(class_id: Option<i32>) -> Option<WorkspaceSceneAssetCategory> {
    match class_id {
        Some(GAME_OBJECT_CLASS_ID) => Some(WorkspaceSceneAssetCategory::GameObject),
        Some(PREFAB_INSTANCE_CLASS_ID) => Some(WorkspaceSceneAssetCategory::PrefabInstance),
        Some(SCENE_ROOTS_CLASS_ID) => Some(WorkspaceSceneAssetCategory::SceneRoots),
        manager => scene_manager_kind(manager).map(WorkspaceSceneAssetCategory::LevelGameManager),
    }
}

pub(super) fn scene_object_identity(
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
