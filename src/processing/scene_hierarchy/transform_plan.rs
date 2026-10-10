use super::asset_classification::scene_object_identity;
use super::resource_limits::checked_charge;
use super::*;
use crate::serialized_file::reference_graph::MultiFileReferenceGraph;
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
pub(super) fn build_missing_transform_plans(
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
