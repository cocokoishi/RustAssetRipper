use std::collections::{HashMap, HashSet};

use crate::processing::scene_hierarchy::{
    WorkspaceGeneratedTransformDefaults, WorkspaceSceneAssetCategory,
    WorkspaceSceneCollectionHierarchy, WorkspaceSceneHierarchies, WorkspaceSceneObjectIdentity,
};

use super::dependencies::{
    assign_source_dependency_file_ids, plan_animator_controller_source_dependency_appends,
    plan_source_dependency_append, SourceDependencyAppendRequest,
};
use super::*;

enum HierarchyIndexPreparation<'a> {
    Ready(SourceHierarchyIndex<'a>),
    Incomplete(String),
}

/// Source collection graphs needed to replay generated prefab assets in FetchAssets order.
struct SourceHierarchyIndex<'a> {
    graphs_by_file: HashMap<usize, &'a WorkspaceSceneCollectionHierarchy>,
    scene_index_by_file: HashMap<usize, usize>,
    missing_transforms: HashMap<
        (usize, usize),
        (
            WorkspaceSceneObjectIdentity,
            WorkspaceGeneratedTransformDefaults,
        ),
    >,
}

impl<'a> SourceHierarchyIndex<'a> {
    fn prepare(
        workspace: &'a WorkspaceSceneHierarchies,
        processing: &WorkspacePrefabProcessingPlan,
        collection_file_indices: &[usize],
        limits: WorkspaceProcessedPrefabAssetPlanLimits,
    ) -> Result<HierarchyIndexPreparation<'a>, WorkspacePrefabProcessingError> {
        let graph_count = workspace
            .scenes()
            .iter()
            .flat_map(|scene| scene.collections())
            .count()
            .checked_add(workspace.non_scene_collections().len())
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "scene hierarchy collection graphs",
            })?;
        let mut graphs_by_file = HashMap::new();
        graphs_by_file.try_reserve(graph_count).map_err(|_| {
            WorkspacePrefabProcessingError::AllocationFailed {
                resource: "Prefab allocation source collection index",
                requested: graph_count,
            }
        })?;
        let mut scene_index_by_file = HashMap::new();
        scene_index_by_file.try_reserve(graph_count).map_err(|_| {
            WorkspacePrefabProcessingError::AllocationFailed {
                resource: "Prefab allocation scene ownership index",
                requested: graph_count,
            }
        })?;
        for (scene_index, scene) in workspace.scenes().iter().enumerate() {
            for graph in scene.collections() {
                let file_index = graph.collection_file_index();
                if graphs_by_file.insert(file_index, graph).is_some()
                    || scene_index_by_file
                        .insert(file_index, scene_index)
                        .is_some()
                {
                    return Err(processed_asset_operation_error(format!(
                        "collection file {file_index} belongs to more than one hierarchy graph"
                    )));
                }
            }
        }
        for graph in workspace.non_scene_collections() {
            let file_index = graph.collection_file_index();
            if graphs_by_file.insert(file_index, graph).is_some() {
                return Err(processed_asset_operation_error(format!(
                    "collection file {file_index} belongs to more than one hierarchy graph"
                )));
            }
        }

        let no_prefab_hierarchy_work = workspace.scenes().is_empty()
            && processing.processed_game_objects().is_empty()
            && processing.events().is_empty();
        let mut seen_collection_files = HashSet::new();
        seen_collection_files
            .try_reserve(collection_file_indices.len())
            .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
                resource: "Prefab allocation collection membership set",
                requested: collection_file_indices.len(),
            })?;
        let mut missing_transforms = HashMap::new();
        for &file_index in collection_file_indices {
            if !seen_collection_files.insert(file_index) {
                return Err(processed_asset_operation_error(format!(
                    "source collection order contains duplicate file index {file_index}"
                )));
            }
            let Some(graph) = graphs_by_file.get(&file_index).copied() else {
                if no_prefab_hierarchy_work {
                    // Non-hierarchy collections do not participate in generated prefab PathID
                    // allocation when the source processor produced no hierarchy work.
                    continue;
                }
                return Ok(HierarchyIndexPreparation::Incomplete(format!(
                    "source collection file {file_index} has no recovered hierarchy graph; generated PathID order is indeterminate"
                )));
            };
            let has_hierarchy_inputs = graph.game_object_count() != 0
                || graph.transform_count() != 0
                || !graph.scene_assets().is_empty()
                || !graph.component_candidates().is_empty()
                || !graph.missing_transform_plans().is_empty();
            if !graph.is_complete() && has_hierarchy_inputs {
                return Ok(HierarchyIndexPreparation::Incomplete(format!(
                    "collection file {file_index} has an incomplete scene hierarchy graph ({} GameObjects, {} Transforms, {} scene assets, {} component candidates, {} missing-Transform plans)",
                    graph.game_object_count(),
                    graph.transform_count(),
                    graph.scene_assets().len(),
                    graph.component_candidates().len(),
                    graph.missing_transform_plans().len()
                )));
            }
            // An opaque raw object cannot affect generated prefab PathID order when a collection
            // contains no recognized hierarchy input. Recognized input above remains fail-closed.
            let requested = missing_transforms
                .len()
                .checked_add(graph.missing_transform_plans().len())
                .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                    resource: "missing Transform allocation candidates",
                })?;
            if requested > limits.max_missing_transform_candidates {
                return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                    resource: "missing Transform allocation candidates",
                    requested,
                    limit: limits.max_missing_transform_candidates,
                });
            }
            missing_transforms
                .try_reserve(graph.missing_transform_plans().len())
                .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
                    resource: "missing Transform allocation candidates",
                    requested,
                })?;
            for transform in graph.missing_transform_plans() {
                let key = (
                    transform.game_object.file_index,
                    transform.game_object.object_index,
                );
                if missing_transforms
                    .insert(key, (transform.game_object, transform.default_values))
                    .is_some()
                {
                    return Err(processed_asset_operation_error(format!(
                        "GameObject {}:{} has duplicate missing-Transform plans",
                        key.0, key.1
                    )));
                }
            }
        }

        Ok(HierarchyIndexPreparation::Ready(Self {
            graphs_by_file,
            scene_index_by_file,
            missing_transforms,
        }))
    }
}

/// Collection identities allocated by the source processor before per-scene collections.
#[derive(Clone, Copy)]
struct GeneratedCollectionIndices {
    prefab_hierarchies: usize,
    generated_prefabs: usize,
    missing_prefab_transforms: usize,
}

/// Mutable state for the ordered replay. Inputs are explicit, and each method owns one source
/// processor phase so the allocation order and its invariants can be reviewed independently.
struct ProcessedPrefabAssetPlanner<'a> {
    workspace: &'a WorkspaceSceneHierarchies,
    processing: &'a WorkspacePrefabProcessingPlan,
    source_external_identifier_counts: &'a [usize],
    animator_controller_inventory: &'a AnimatorControllerSourceDependencyInventory,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
    hierarchy: SourceHierarchyIndex<'a>,
    plan: WorkspaceProcessedPrefabAssetPlan,
    generated_collections: GeneratedCollectionIndices,
    scene_collection_by_index: Vec<Option<usize>>,
    source_dependency_append_keys: HashSet<(usize, WorkspaceSourceDependencyAppendTarget)>,
}

impl<'a> ProcessedPrefabAssetPlanner<'a> {
    fn new(
        workspace: &'a WorkspaceSceneHierarchies,
        processing: &'a WorkspacePrefabProcessingPlan,
        source_external_identifier_counts: &'a [usize],
        animator_controller_inventory: &'a AnimatorControllerSourceDependencyInventory,
        limits: WorkspaceProcessedPrefabAssetPlanLimits,
        hierarchy: SourceHierarchyIndex<'a>,
    ) -> Result<Self, WorkspacePrefabProcessingError> {
        let mut plan = WorkspaceProcessedPrefabAssetPlan {
            complete: true,
            incomplete_reason: None,
            collections: Vec::new(),
            component_additions: Vec::new(),
            component_list_overlays: Vec::new(),
            component_overlay_slot_count: 0,
            constructor_pointers: Vec::new(),
            dependency_entry_count: 0,
            source_dependency_appends: Vec::new(),
            modeled_animator_controller_dependency_append_count: 0,
            deferred_source_dependency_file_count: 0,
            asset_count: 0,
            total_collection_name_bytes: 0,
        };
        let generated_collections = GeneratedCollectionIndices {
            prefab_hierarchies: add_processed_collection(
                &mut plan,
                WorkspaceProcessedCollectionKind::PrefabHierarchies,
                "Prefab Hierarchies",
                limits,
            )?,
            generated_prefabs: add_processed_collection(
                &mut plan,
                WorkspaceProcessedCollectionKind::GeneratedPrefabs,
                "Generated Prefabs",
                limits,
            )?,
            missing_prefab_transforms: add_processed_collection(
                &mut plan,
                WorkspaceProcessedCollectionKind::MissingPrefabTransforms,
                "Missing Prefab Transforms",
                limits,
            )?,
        };
        let mut scene_collection_by_index = Vec::new();
        scene_collection_by_index
            .try_reserve_exact(workspace.scenes().len())
            .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
                resource: "per-scene generated collection index",
                requested: workspace.scenes().len(),
            })?;
        scene_collection_by_index.resize(workspace.scenes().len(), None);
        let requested_source_dependency_keys = hierarchy
            .missing_transforms
            .len()
            .checked_add(workspace.scenes().len())
            .and_then(|count| {
                count.checked_add(animator_controller_inventory.modeled_append_count())
            })
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "source processed-collection dependency append index",
            })?;
        let mut source_dependency_append_keys = HashSet::new();
        source_dependency_append_keys
            .try_reserve(requested_source_dependency_keys)
            .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
                resource: "source processed-collection dependency append index",
                requested: requested_source_dependency_keys,
            })?;

        Ok(Self {
            workspace,
            processing,
            source_external_identifier_counts,
            animator_controller_inventory,
            limits,
            hierarchy,
            plan,
            generated_collections,
            scene_collection_by_index,
            source_dependency_append_keys,
        })
    }

    fn execute(
        mut self,
    ) -> Result<WorkspaceProcessedPrefabAssetPlan, WorkspacePrefabProcessingError> {
        self.plan_processor_dependencies()?;
        self.append_missing_transform_assets()?;
        self.append_scene_hierarchy_assets()?;
        self.append_prefab_hierarchy_assets()?;
        assign_source_dependency_file_ids(
            &mut self.plan,
            self.source_external_identifier_counts,
            self.animator_controller_inventory,
            self.limits,
        )?;
        Ok(self.plan)
    }

    fn plan_processor_dependencies(&mut self) -> Result<(), WorkspacePrefabProcessingError> {
        // AnimatorControllerProcessor runs before LightingDataProcessor and PrefabProcessor.
        plan_animator_controller_source_dependency_appends(
            &mut self.plan,
            &mut self.source_dependency_append_keys,
            self.animator_controller_inventory,
            self.limits,
        )?;

        // LightingDataProcessor creates the first generated asset collection referenced by a
        // source collection and writes that PPtr to LightmapSettings.
        for scene in self.workspace.scenes() {
            let Some(owner) = scene.lighting_data_dependency_owner() else {
                continue;
            };
            plan_source_dependency_append(
                &mut self.plan,
                &mut self.source_dependency_append_keys,
                SourceDependencyAppendRequest {
                    owner,
                    target_collection:
                        WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
                    pointer_field:
                        WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
                    target_asset: None,
                    target_animator_state_machine: None,
                },
                self.limits,
            )?;
        }
        Ok(())
    }

    fn append_missing_transform_assets(&mut self) -> Result<(), WorkspacePrefabProcessingError> {
        let collection_file_indices = self.processing.source_collection_file_indices();
        for asset in self
            .workspace
            .scene_asset_identities_in_collection_order(collection_file_indices)
        {
            if asset.category != WorkspaceSceneAssetCategory::GameObject {
                continue;
            }
            let key = (asset.identity.file_index, asset.identity.object_index);
            let Some((game_object, default_values)) =
                self.hierarchy.missing_transforms.remove(&key)
            else {
                continue;
            };
            if game_object != asset.identity {
                return Err(processed_asset_operation_error(format!(
                    "missing-Transform identity for {}:{} disagrees with source collection order",
                    key.0, key.1
                )));
            }
            let collection_index =
                if let Some(&scene_index) = self.hierarchy.scene_index_by_file.get(&key.0) {
                    let scene = self.workspace.scenes().get(scene_index).ok_or_else(|| {
                        processed_asset_operation_error(format!(
                            "collection file {} maps to missing SceneDefinition {scene_index}",
                            key.0
                        ))
                    })?;
                    ensure_scene_processed_collection(
                        &mut self.plan,
                        &mut self.scene_collection_by_index,
                        scene_index,
                        &scene.plan.name,
                        self.limits,
                    )?
                } else {
                    self.generated_collections.missing_prefab_transforms
                };
            let transform_asset = add_processed_asset(
                &mut self.plan,
                collection_index,
                4,
                WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
                    game_object,
                    default_values,
                },
                self.limits,
            )?;
            let game_object_file_id = add_processed_source_file_dependency(
                &mut self.plan,
                collection_index,
                game_object.file_index,
                self.limits,
            )?;
            add_generated_constructor_pointer(
                &mut self.plan,
                WorkspaceGeneratedConstructorPointer {
                    source_asset: transform_asset,
                    field: WorkspaceGeneratedPointerField::TransformGameObject,
                    file_id: game_object_file_id,
                    path_id: game_object.path_id,
                    target: game_object,
                },
                self.limits,
            )?;

            let graph = self
                .hierarchy
                .graphs_by_file
                .get(&key.0)
                .copied()
                .ok_or_else(|| {
                    processed_asset_operation_error(format!(
                        "missing-Transform GameObject collection {} disappeared during allocation",
                        key.0
                    ))
                })?;
            let appended_index = graph
                .game_object_component_slot_count(game_object)
                .map_err(|error| processed_asset_operation_error(error.to_string()))?;
            let mut source_slots = Vec::new();
            source_slots
                .try_reserve_exact(appended_index)
                .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
                    resource: "source GameObject component slots for overlay",
                    requested: appended_index,
                })?;
            for (source_array_index, relationship) in graph
                .game_object_component_relationships_for_identity(game_object)
                .map_err(|error| processed_asset_operation_error(error.to_string()))?
                .enumerate()
            {
                source_slots.push(WorkspaceSourceGameObjectComponentSlot {
                    source_array_index,
                    file_id: relationship.file_id,
                    path_id: relationship.path_id,
                    target: relationship.target.clone(),
                });
            }
            if source_slots.len() != appended_index {
                return Err(processed_asset_operation_error(format!(
                    "GameObject {}:{} component slot count changed during overlay planning",
                    game_object.file_index, game_object.object_index
                )));
            }
            let requested_overlay_slots = self
                .plan
                .component_overlay_slot_count
                .checked_add(appended_index)
                .and_then(|count| count.checked_add(1))
                .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                    resource: "GameObject component overlay slots",
                })?;
            if requested_overlay_slots > self.limits.max_component_overlay_slots {
                return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                    resource: "GameObject component overlay slots",
                    requested: requested_overlay_slots,
                    limit: self.limits.max_component_overlay_slots,
                });
            }
            let requested_additions = self.plan.component_additions.len().checked_add(1).ok_or(
                WorkspacePrefabProcessingError::CountOverflow {
                    resource: "generated GameObject component additions",
                },
            )?;
            if requested_additions > self.limits.max_component_additions {
                return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                    resource: "generated GameObject component additions",
                    requested: requested_additions,
                    limit: self.limits.max_component_additions,
                });
            }
            self.plan.component_additions.try_reserve(1).map_err(|_| {
                WorkspacePrefabProcessingError::AllocationFailed {
                    resource: "generated GameObject component additions",
                    requested: requested_additions,
                }
            })?;
            self.plan
                .component_list_overlays
                .try_reserve(1)
                .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
                    resource: "GameObject component list overlays",
                    requested: requested_additions,
                })?;
            let addition = WorkspaceGeneratedComponentAddition {
                game_object,
                component_class_id: 4,
                appended_index,
                component_asset: transform_asset,
            };
            plan_source_dependency_append(
                &mut self.plan,
                &mut self.source_dependency_append_keys,
                SourceDependencyAppendRequest {
                    owner: game_object,
                    target_collection:
                        WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                            collection_index,
                        },
                    pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
                    target_asset: Some(transform_asset),
                    target_animator_state_machine: None,
                },
                self.limits,
            )?;
            self.plan.component_additions.push(addition);
            self.plan
                .component_list_overlays
                .push(WorkspaceGameObjectComponentListOverlay {
                    game_object,
                    source_slots,
                    generated_component: addition,
                });
            self.plan.component_overlay_slot_count = requested_overlay_slots;
        }
        if let Some((file_index, object_index)) =
            self.hierarchy.missing_transforms.keys().copied().min()
        {
            return Err(processed_asset_operation_error(format!(
                "missing-Transform GameObject {file_index}:{object_index} is absent from source FetchAssets order"
            )));
        }
        Ok(())
    }

    fn append_scene_hierarchy_assets(&mut self) -> Result<(), WorkspacePrefabProcessingError> {
        // SceneHierarchyObject.Create allocates one SceneAsset per SceneDefinition after missing transforms.
        for (scene_index, scene) in self.workspace.scenes().iter().enumerate() {
            let collection_index = ensure_scene_processed_collection(
                &mut self.plan,
                &mut self.scene_collection_by_index,
                scene_index,
                &scene.plan.name,
                self.limits,
            )?;
            add_processed_asset(
                &mut self.plan,
                collection_index,
                1032,
                WorkspaceProcessedPrefabAssetKind::SceneHierarchy { scene_index },
                self.limits,
            )?;
        }
        Ok(())
    }

    fn append_prefab_hierarchy_assets(&mut self) -> Result<(), WorkspacePrefabProcessingError> {
        // Existing and generated PrefabHierarchy allocations are interleaved by source events.
        for event in self.processing.events() {
            match event {
                WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyCreated {
                    prefab_instance,
                    root_game_object,
                    ..
                } => {
                    add_processed_asset(
                        &mut self.plan,
                        self.generated_collections.prefab_hierarchies,
                        1001,
                        WorkspaceProcessedPrefabAssetKind::PrefabHierarchy {
                            root_game_object: *root_game_object,
                            prefab: WorkspaceProcessedPrefabReference::Existing(*prefab_instance),
                        },
                        self.limits,
                    )?;
                }
                WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyCreated {
                    root_game_object,
                    ..
                } => {
                    let prefab_instance = add_processed_asset(
                        &mut self.plan,
                        self.generated_collections.generated_prefabs,
                        1001,
                        WorkspaceProcessedPrefabAssetKind::GeneratedPrefabInstance {
                            root_game_object: *root_game_object,
                        },
                        self.limits,
                    )?;
                    let root_file_id = add_processed_source_file_dependency(
                        &mut self.plan,
                        self.generated_collections.generated_prefabs,
                        root_game_object.file_index,
                        self.limits,
                    )?;
                    add_generated_constructor_pointer(
                        &mut self.plan,
                        WorkspaceGeneratedConstructorPointer {
                            source_asset: prefab_instance,
                            field: WorkspaceGeneratedPointerField::PrefabInstanceRootGameObject,
                            file_id: root_file_id,
                            path_id: root_game_object.path_id,
                            target: *root_game_object,
                        },
                        self.limits,
                    )?;
                    add_processed_asset(
                        &mut self.plan,
                        self.generated_collections.prefab_hierarchies,
                        1001,
                        WorkspaceProcessedPrefabAssetKind::PrefabHierarchy {
                            root_game_object: *root_game_object,
                            prefab: WorkspaceProcessedPrefabReference::Generated(prefab_instance),
                        },
                        self.limits,
                    )?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub(super) fn plan_workspace_processed_prefab_assets(
    workspace: &WorkspaceSceneHierarchies,
    processing: &WorkspacePrefabProcessingPlan,
    source_external_identifier_counts: &[usize],
    animator_controller_inventory: &AnimatorControllerSourceDependencyInventory,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<WorkspaceProcessedPrefabAssetPlan, WorkspacePrefabProcessingError> {
    if !processing.is_complete() {
        return Ok(incomplete_processed_asset_plan(
            "PrefabProcessor identity plan is incomplete; generated PathID order is indeterminate",
        ));
    }
    let hierarchy = match SourceHierarchyIndex::prepare(
        workspace,
        processing,
        processing.source_collection_file_indices(),
        limits,
    )? {
        HierarchyIndexPreparation::Ready(hierarchy) => hierarchy,
        HierarchyIndexPreparation::Incomplete(reason) => {
            return Ok(incomplete_processed_asset_plan(&reason));
        }
    };
    ProcessedPrefabAssetPlanner::new(
        workspace,
        processing,
        source_external_identifier_counts,
        animator_controller_inventory,
        limits,
        hierarchy,
    )?
    .execute()
}
