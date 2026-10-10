use super::asset_classification::{
    classify_scene_collection_objects, ClassifiedSceneCollectionObjects,
};
use super::collection_snapshot::{
    copy_scene_graph_issue, copy_scene_object, copy_scene_relationship,
};
use super::collection_validation::validate_collection_graph;
use super::relationship_indexes::{
    build_game_object_component_relationship_index, build_prefab_root_relationship_index,
    build_transform_hierarchy_index,
};
use super::transform_plan::build_missing_transform_plans;
use super::*;
use crate::serialized_file::reference_graph::MultiFileReferenceGraph;

/// Inputs needed to validate and classify one source collection before it is retained.
pub(super) struct CollectionGraphInput<'a, 'b> {
    pub(super) file_index: usize,
    pub(super) scene_graph: &'a SceneGraph,
    pub(super) roots: WorkspaceSceneRootDiscovery,
    pub(super) hierarchy_audit: WorkspaceSceneHierarchyAudit,
    pub(super) has_lighting_data_asset: Option<bool>,
    pub(super) references: &'a MultiFileReferenceGraph<'b>,
}

/// Validated, classified source data ready for budget accounting and snapshot construction.
pub(super) struct PreparedCollectionGraph<'a> {
    file_index: usize,
    scene_graph: &'a SceneGraph,
    roots: WorkspaceSceneRootDiscovery,
    hierarchy_audit: WorkspaceSceneHierarchyAudit,
    source_reference_scans_complete: bool,
    scene_assets: Vec<WorkspaceSceneAssetIdentity>,
    lighting_data_asset_field_object_indices: Vec<usize>,
    component_candidates: Vec<WorkspaceSceneComponentCandidate>,
    missing_transform_plans: Vec<WorkspaceMissingTransformPlan>,
    usage: CollectionGraphUsage,
}

/// Resource charges measured from one source collection before storage limits are applied.
pub(super) struct CollectionGraphUsage {
    pub(super) scene_objects: usize,
    pub(super) issue_count: usize,
    pub(super) name_bytes: usize,
    pub(super) diagnostic_bytes: usize,
}

impl<'a> PreparedCollectionGraph<'a> {
    pub(super) fn prepare(
        input: CollectionGraphInput<'a, '_>,
    ) -> Result<Self, WorkspaceSceneHierarchyError> {
        let CollectionGraphInput {
            file_index,
            scene_graph,
            roots,
            hierarchy_audit,
            has_lighting_data_asset,
            references,
        } = input;

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
        let ClassifiedSceneCollectionObjects {
            scene_assets,
            lighting_data_asset_field_object_indices,
            component_candidates,
        } = classify_scene_collection_objects(
            file_index,
            scene_graph,
            has_lighting_data_asset,
            references,
        )?;
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
                        SceneGraphIssueKind::ObjectDecodeFailed { message } => message.len(),
                        SceneGraphIssueKind::MissingField { field_name } => field_name.len(),
                        SceneGraphIssueKind::DuplicateField { field_name, .. } => field_name.len(),
                        SceneGraphIssueKind::UnexpectedValueType { .. } => 0,
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

        Ok(Self {
            file_index,
            scene_graph,
            roots,
            hierarchy_audit,
            source_reference_scans_complete,
            scene_assets,
            lighting_data_asset_field_object_indices,
            component_candidates,
            missing_transform_plans,
            usage: CollectionGraphUsage {
                scene_objects,
                issue_count,
                name_bytes,
                diagnostic_bytes,
            },
        })
    }

    pub(super) fn usage(&self) -> &CollectionGraphUsage {
        &self.usage
    }

    pub(super) fn scene_asset_record_count(&self) -> Result<usize, WorkspaceSceneHierarchyError> {
        self.scene_assets
            .len()
            .checked_add(self.component_candidates.len())
            .and_then(|count| count.checked_add(self.missing_transform_plans.len()))
            .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
                resource: "scene asset and generated-transform records",
            })
    }

    pub(super) fn build_snapshot(
        self,
    ) -> Result<WorkspaceSceneCollectionHierarchy, WorkspaceSceneHierarchyError> {
        let scene_object_count = self.usage.scene_objects;
        let Self {
            file_index,
            scene_graph,
            roots,
            hierarchy_audit,
            source_reference_scans_complete,
            scene_assets,
            lighting_data_asset_field_object_indices,
            component_candidates,
            missing_transform_plans,
            ..
        } = self;

        let mut objects = Vec::new();
        objects.try_reserve_exact(scene_object_count).map_err(|_| {
            WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "SceneDefinition scene objects",
                requested: scene_object_count,
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

        Ok(WorkspaceSceneCollectionHierarchy {
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
        })
    }
}
