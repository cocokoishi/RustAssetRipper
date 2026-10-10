/// Replays the source-created collection and asset order after an identity-level PrefabProcessor
/// plan. PathIDs start at one independently in each ProcessedAssetCollection, as in the baseline.
pub fn plan_workspace_processed_prefab_assets(
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

    let mut graphs_by_file = HashMap::new();
    let graph_count = workspace
        .scenes()
        .iter()
        .flat_map(|scene| scene.collections())
        .count()
        .checked_add(workspace.non_scene_collections().len())
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "scene hierarchy collection graphs",
        })?;
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

    let collection_file_indices = processing.source_collection_file_indices();
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
                // Collections containing only non-hierarchy assets do not participate in
                // AddMissingTransforms, SceneHierarchyObject, or generated Prefab allocation.
                // Their absence from the typed hierarchy graph therefore creates no generated
                // PathID order to replay. Any scene, GameObject, PrefabInstance, or other
                // processor event keeps the original fail-closed behavior below.
                continue;
            }
            return Ok(incomplete_processed_asset_plan(&format!(
                "source collection file {file_index} has no recovered hierarchy graph; generated PathID order is indeterminate"
            )));
        };
        let has_hierarchy_inputs = graph.game_object_count() != 0
            || graph.transform_count() != 0
            || !graph.scene_assets().is_empty()
            || !graph.component_candidates().is_empty()
            || !graph.missing_transform_plans().is_empty();
        if !graph.is_complete() && has_hierarchy_inputs {
            return Ok(incomplete_processed_asset_plan(&format!(
                "collection file {file_index} has an incomplete scene hierarchy graph ({} GameObjects, {} Transforms, {} scene assets, {} component candidates, {} missing-Transform plans)",
                graph.game_object_count(),
                graph.transform_count(),
                graph.scene_assets().len(),
                graph.component_candidates().len(),
                graph.missing_transform_plans().len()
            )));
        }
        // The baseline's AddMissingTransforms pass creates no objects for a collection with no
        // GameObject, Transform, scene-asset, or component candidates. An opaque raw object in
        // such a collection cannot affect generated Prefab PathID order; keep the source scan
        // incomplete, but allow the empty processed-asset collections to be planned. If any
        // recognized hierarchy input exists, the fail-closed branch above still applies.
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
    let prefab_hierarchies = add_processed_collection(
        &mut plan,
        WorkspaceProcessedCollectionKind::PrefabHierarchies,
        "Prefab Hierarchies",
        limits,
    )?;
    let generated_prefabs = add_processed_collection(
        &mut plan,
        WorkspaceProcessedCollectionKind::GeneratedPrefabs,
        "Generated Prefabs",
        limits,
    )?;
    let missing_prefab_transforms = add_processed_collection(
        &mut plan,
        WorkspaceProcessedCollectionKind::MissingPrefabTransforms,
        "Missing Prefab Transforms",
        limits,
    )?;

    let mut scene_collection_by_index = Vec::new();
    scene_collection_by_index
        .try_reserve_exact(workspace.scenes().len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "per-scene generated collection index",
            requested: workspace.scenes().len(),
        })?;
    scene_collection_by_index.resize(workspace.scenes().len(), None);
    let mut source_dependency_append_keys = HashSet::new();
    let requested_source_dependency_keys = missing_transforms
        .len()
        .checked_add(workspace.scenes().len())
        .and_then(|count| count.checked_add(animator_controller_inventory.modeled_append_count()))
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "source processed-collection dependency append index",
        })?;
    source_dependency_append_keys
        .try_reserve(requested_source_dependency_keys)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "source processed-collection dependency append index",
            requested: requested_source_dependency_keys,
        })?;

    // AnimatorControllerProcessor runs before LightingDataProcessor and PrefabProcessor.
    plan_animator_controller_source_dependency_appends(
        &mut plan,
        &mut source_dependency_append_keys,
        animator_controller_inventory,
        limits,
    )?;

    // LightingDataProcessor runs before PrefabProcessor. It creates the first generated asset
    // collection referenced by a source collection and writes that PPtr to LightmapSettings.
    for scene in workspace.scenes() {
        let Some(owner) = scene.lighting_data_dependency_owner() else {
            continue;
        };
        plan_source_dependency_append(
            &mut plan,
            &mut source_dependency_append_keys,
            SourceDependencyAppendRequest {
                owner,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::GeneratedLightingDataAssets,
                pointer_field:
                    WorkspaceSourceDependencyPointerField::LightmapSettingsLightingDataAsset,
                target_asset: None,
                target_animator_state_machine: None,
            },
            limits,
        )?;
    }

    // AddMissingTransforms runs before SceneHierarchyObject creation and follows FetchAssets order.
    for asset in workspace.scene_asset_identities_in_collection_order(collection_file_indices) {
        if asset.category != WorkspaceSceneAssetCategory::GameObject {
            continue;
        }
        let key = (asset.identity.file_index, asset.identity.object_index);
        let Some((game_object, default_values)) = missing_transforms.remove(&key) else {
            continue;
        };
        if game_object != asset.identity {
            return Err(processed_asset_operation_error(format!(
                "missing-Transform identity for {}:{} disagrees with source collection order",
                key.0, key.1
            )));
        }
        let collection_index = if let Some(&scene_index) = scene_index_by_file.get(&key.0) {
            let scene = workspace.scenes().get(scene_index).ok_or_else(|| {
                processed_asset_operation_error(format!(
                    "collection file {} maps to missing SceneDefinition {scene_index}",
                    key.0
                ))
            })?;
            ensure_scene_processed_collection(
                &mut plan,
                &mut scene_collection_by_index,
                scene_index,
                &scene.plan.name,
                limits,
            )?
        } else {
            missing_prefab_transforms
        };
        let transform_asset = add_processed_asset(
            &mut plan,
            collection_index,
            4,
            WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
                game_object,
                default_values,
            },
            limits,
        )?;
        let game_object_file_id = add_processed_source_file_dependency(
            &mut plan,
            collection_index,
            game_object.file_index,
            limits,
        )?;
        add_generated_constructor_pointer(
            &mut plan,
            WorkspaceGeneratedConstructorPointer {
                source_asset: transform_asset,
                field: WorkspaceGeneratedPointerField::TransformGameObject,
                file_id: game_object_file_id,
                path_id: game_object.path_id,
                target: game_object,
            },
            limits,
        )?;
        let graph = graphs_by_file.get(&key.0).copied().ok_or_else(|| {
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
        let requested_overlay_slots = plan
            .component_overlay_slot_count
            .checked_add(appended_index)
            .and_then(|count| count.checked_add(1))
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "GameObject component overlay slots",
            })?;
        if requested_overlay_slots > limits.max_component_overlay_slots {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "GameObject component overlay slots",
                requested: requested_overlay_slots,
                limit: limits.max_component_overlay_slots,
            });
        }
        let requested_additions = plan.component_additions.len().checked_add(1).ok_or(
            WorkspacePrefabProcessingError::CountOverflow {
                resource: "generated GameObject component additions",
            },
        )?;
        if requested_additions > limits.max_component_additions {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "generated GameObject component additions",
                requested: requested_additions,
                limit: limits.max_component_additions,
            });
        }
        plan.component_additions.try_reserve(1).map_err(|_| {
            WorkspacePrefabProcessingError::AllocationFailed {
                resource: "generated GameObject component additions",
                requested: requested_additions,
            }
        })?;
        plan.component_list_overlays.try_reserve(1).map_err(|_| {
            WorkspacePrefabProcessingError::AllocationFailed {
                resource: "GameObject component list overlays",
                requested: requested_additions,
            }
        })?;
        let addition = WorkspaceGeneratedComponentAddition {
            game_object,
            component_class_id: 4,
            appended_index,
            component_asset: transform_asset,
        };
        plan_source_dependency_append(
            &mut plan,
            &mut source_dependency_append_keys,
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
            limits,
        )?;
        plan.component_additions.push(addition);
        plan.component_list_overlays
            .push(WorkspaceGameObjectComponentListOverlay {
                game_object,
                source_slots,
                generated_component: addition,
            });
        plan.component_overlay_slot_count = requested_overlay_slots;
    }
    if let Some((file_index, object_index)) = missing_transforms.keys().copied().min() {
        return Err(processed_asset_operation_error(format!(
            "missing-Transform GameObject {file_index}:{object_index} is absent from source FetchAssets order"
        )));
    }

    // SceneHierarchyObject.Create allocates one SceneAsset per SceneDefinition after missing transforms.
    for (scene_index, scene) in workspace.scenes().iter().enumerate() {
        let collection_index = ensure_scene_processed_collection(
            &mut plan,
            &mut scene_collection_by_index,
            scene_index,
            &scene.plan.name,
            limits,
        )?;
        add_processed_asset(
            &mut plan,
            collection_index,
            1032,
            WorkspaceProcessedPrefabAssetKind::SceneHierarchy { scene_index },
            limits,
        )?;
    }

    // Existing PrefabHierarchy objects and generated prefabs are interleaved by source events.
    for event in processing.events() {
        match event {
            WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyCreated {
                prefab_instance,
                root_game_object,
                ..
            } => {
                add_processed_asset(
                    &mut plan,
                    prefab_hierarchies,
                    1001,
                    WorkspaceProcessedPrefabAssetKind::PrefabHierarchy {
                        root_game_object: *root_game_object,
                        prefab: WorkspaceProcessedPrefabReference::Existing(*prefab_instance),
                    },
                    limits,
                )?;
            }
            WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyCreated {
                root_game_object,
                ..
            } => {
                let prefab_instance = add_processed_asset(
                    &mut plan,
                    generated_prefabs,
                    1001,
                    WorkspaceProcessedPrefabAssetKind::GeneratedPrefabInstance {
                        root_game_object: *root_game_object,
                    },
                    limits,
                )?;
                let root_file_id = add_processed_source_file_dependency(
                    &mut plan,
                    generated_prefabs,
                    root_game_object.file_index,
                    limits,
                )?;
                add_generated_constructor_pointer(
                    &mut plan,
                    WorkspaceGeneratedConstructorPointer {
                        source_asset: prefab_instance,
                        field: WorkspaceGeneratedPointerField::PrefabInstanceRootGameObject,
                        file_id: root_file_id,
                        path_id: root_game_object.path_id,
                        target: *root_game_object,
                    },
                    limits,
                )?;
                add_processed_asset(
                    &mut plan,
                    prefab_hierarchies,
                    1001,
                    WorkspaceProcessedPrefabAssetKind::PrefabHierarchy {
                        root_game_object: *root_game_object,
                        prefab: WorkspaceProcessedPrefabReference::Generated(prefab_instance),
                    },
                    limits,
                )?;
            }
            _ => {}
        }
    }

    assign_source_dependency_file_ids(
        &mut plan,
        source_external_identifier_counts,
        animator_controller_inventory,
        limits,
    )?;

    Ok(plan)
}

fn incomplete_processed_asset_plan(reason: &str) -> WorkspaceProcessedPrefabAssetPlan {
    WorkspaceProcessedPrefabAssetPlan {
        complete: false,
        incomplete_reason: Some(reason.to_owned()),
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
    }
}

fn add_processed_collection(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    kind: WorkspaceProcessedCollectionKind,
    name: &str,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<usize, WorkspacePrefabProcessingError> {
    let requested = plan.collections.len().checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed asset collections",
        },
    )?;
    if requested > limits.max_collections {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed asset collections",
            requested,
            limit: limits.max_collections,
        });
    }
    let requested_name_bytes = plan
        .total_collection_name_bytes
        .checked_add(name.len())
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed asset collection names",
        })?;
    if requested_name_bytes > limits.max_total_collection_name_bytes {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed asset collection name bytes",
            requested: requested_name_bytes,
            limit: limits.max_total_collection_name_bytes,
        });
    }
    plan.collections.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed asset collections",
            requested,
        }
    })?;
    if limits.max_dependencies_per_collection == 0 {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed collection dependency entries",
            requested: 1,
            limit: limits.max_dependencies_per_collection,
        });
    }
    let requested_dependency_entries = plan.dependency_entry_count.checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed collection dependency entries",
        },
    )?;
    if requested_dependency_entries > limits.max_dependency_entries {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed collection dependency entries",
            requested: requested_dependency_entries,
            limit: limits.max_dependency_entries,
        });
    }
    let mut owned_name = String::new();
    owned_name.try_reserve_exact(name.len()).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed asset collection name",
            requested: name.len(),
        }
    })?;
    owned_name.push_str(name);
    let collection_index = plan.collections.len();
    let mut dependencies = Vec::new();
    dependencies.try_reserve_exact(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed collection dependency entries",
            requested: 1,
        }
    })?;
    dependencies.push(WorkspaceProcessedCollectionDependency {
        file_id: 0,
        target: WorkspaceProcessedDependencyTarget::ProcessedCollection { collection_index },
    });
    plan.collections
        .push(WorkspaceProcessedAssetCollectionPlan {
            collection_index,
            dependency_indices_complete: kind
                != WorkspaceProcessedCollectionKind::PrefabHierarchies,
            kind,
            name: owned_name,
            assets: Vec::new(),
            dependencies,
            next_path_id: 0,
            source_dependency_index: HashMap::new(),
        });
    plan.total_collection_name_bytes = requested_name_bytes;
    plan.dependency_entry_count = requested_dependency_entries;
    Ok(collection_index)
}

fn add_processed_source_file_dependency(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    collection_index: usize,
    source_file_index: usize,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<i32, WorkspacePrefabProcessingError> {
    let collection = plan.collections.get_mut(collection_index).ok_or_else(|| {
        processed_asset_operation_error(format!(
            "processed asset collection index {collection_index} is unavailable"
        ))
    })?;
    if let Some(&file_id) = collection.source_dependency_index.get(&source_file_index) {
        return Ok(file_id);
    }
    let requested_collection_entries = collection.dependencies.len().checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed collection dependency entries",
        },
    )?;
    if requested_collection_entries > limits.max_dependencies_per_collection {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed collection dependency entries",
            requested: requested_collection_entries,
            limit: limits.max_dependencies_per_collection,
        });
    }
    let requested_total_entries = plan.dependency_entry_count.checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed collection dependency entries",
        },
    )?;
    if requested_total_entries > limits.max_dependency_entries {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed collection dependency entries",
            requested: requested_total_entries,
            limit: limits.max_dependency_entries,
        });
    }
    let file_id = i32::try_from(collection.dependencies.len()).map_err(|_| {
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed collection PPtr FileID",
        }
    })?;
    collection.dependencies.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed collection dependency entries",
            requested: requested_collection_entries,
        }
    })?;
    collection
        .source_dependency_index
        .try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed collection dependency lookup",
            requested: requested_collection_entries - 1,
        })?;
    collection
        .dependencies
        .push(WorkspaceProcessedCollectionDependency {
            file_id,
            target: WorkspaceProcessedDependencyTarget::SerializedFileCollection {
                file_index: source_file_index,
            },
        });
    collection
        .source_dependency_index
        .insert(source_file_index, file_id);
    plan.dependency_entry_count = requested_total_entries;
    Ok(file_id)
}

fn add_generated_constructor_pointer(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    pointer: WorkspaceGeneratedConstructorPointer,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    if pointer.path_id == 0 || pointer.target.path_id != pointer.path_id {
        return Err(processed_asset_operation_error(format!(
            "generated {:?} pointer for collection {} PathID {} has a null or mismatched target",
            pointer.field, pointer.source_asset.collection_index, pointer.source_asset.path_id
        )));
    }
    let requested = plan.constructor_pointers.len().checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "generated constructor-time PPtrs",
        },
    )?;
    if requested > limits.max_constructor_pointers {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "generated constructor-time PPtrs",
            requested,
            limit: limits.max_constructor_pointers,
        });
    }
    plan.constructor_pointers.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "generated constructor-time PPtrs",
            requested,
        }
    })?;
    plan.constructor_pointers.push(pointer);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceDependencyAppendRequest {
    owner: WorkspaceSceneObjectIdentity,
    target_collection: WorkspaceSourceDependencyAppendTarget,
    pointer_field: WorkspaceSourceDependencyPointerField,
    target_asset: Option<WorkspaceProcessedAssetIdentity>,
    target_animator_state_machine: Option<WorkspaceAnimatorControllerStateMachineIdentity>,
}

fn plan_source_dependency_append(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    seen: &mut HashSet<(usize, WorkspaceSourceDependencyAppendTarget)>,
    request: SourceDependencyAppendRequest,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    let key = (request.owner.file_index, request.target_collection);
    if seen.contains(&key) {
        return Ok(());
    }
    let requested = plan.source_dependency_appends.len().checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "source processed-collection dependency appends",
        },
    )?;
    if requested > limits.max_source_dependency_appends {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "source processed-collection dependency appends",
            requested,
            limit: limits.max_source_dependency_appends,
        });
    }
    plan.source_dependency_appends.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "source processed-collection dependency appends",
            requested,
        }
    })?;
    seen.try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "source processed-collection dependency append index",
            requested,
        })?;
    seen.insert(key);
    plan.source_dependency_appends
        .push(WorkspaceSourceCollectionDependencyAppend {
            source_file_index: request.owner.file_index,
            target_collection: request.target_collection,
            pointer_field: request.pointer_field,
            first_source_owner: request.owner,
            target_asset: request.target_asset,
            target_animator_state_machine: request.target_animator_state_machine,
            file_id: None,
        });
    Ok(())
}

fn plan_animator_controller_source_dependency_appends(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    seen: &mut HashSet<(usize, WorkspaceSourceDependencyAppendTarget)>,
    inventory: &AnimatorControllerSourceDependencyInventory,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    for target in &inventory.append_targets {
        if inventory
            .deferred_file_indices
            .contains(&target.source_controller.file_index)
        {
            continue;
        }
        plan_source_dependency_append(
            plan,
            seen,
            SourceDependencyAppendRequest {
                owner: target.source_controller,
                target_collection:
                    WorkspaceSourceDependencyAppendTarget::GeneratedAnimatorControllerDependencies,
                pointer_field:
                    WorkspaceSourceDependencyPointerField::AnimatorControllerLayerStateMachine,
                target_asset: None,
                target_animator_state_machine: Some(*target),
            },
            limits,
        )?;
    }
    Ok(())
}

/// Ordered source-collection dependency effects from AnimatorControllerProcessor.
///
/// A source collection gets one dependency entry for the shared generated AnimatorController
/// collection when at least one decoded controller has a non-empty layer array. Files with any
/// controller whose relevant TypeTree data is unavailable or inconsistent remain deferred.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AnimatorControllerSourceDependencyInventory {
    append_file_indices: HashSet<usize>,
    deferred_file_indices: HashSet<usize>,
    append_targets: Vec<WorkspaceAnimatorControllerStateMachineIdentity>,
}

impl AnimatorControllerSourceDependencyInventory {
    pub fn modeled_append_count(&self) -> usize {
        self.append_file_indices.len()
    }

    pub fn deferred_file_count(&self) -> usize {
        self.deferred_file_indices.len()
    }
}

pub fn animator_controller_source_dependency_inventory(
    source_objects: &ObjectWorkspace,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<AnimatorControllerSourceDependencyInventory, WorkspacePrefabProcessingError> {
    const ANIMATOR_CONTROLLER_CLASS_ID: i32 = 91;
    let mut inventory = AnimatorControllerSourceDependencyInventory::default();
    for object in source_objects.objects() {
        if object.class_id.map(i32::from).unwrap_or(object.raw_type_id)
            == ANIMATOR_CONTROLLER_CLASS_ID
        {
            if inventory.deferred_file_indices.contains(&object.file_index) {
                continue;
            }
            let first_state_machine_index =
                object.payload.as_ref().ok().and_then(|payload| {
                    animator_controller_requires_source_dependency(&payload.root)
                });
            match first_state_machine_index {
                Some(Some(state_machine_index)) => {
                    let target = WorkspaceAnimatorControllerStateMachineIdentity {
                        source_controller: WorkspaceSceneObjectIdentity {
                            file_index: object.file_index,
                            object_index: object.object_index,
                            path_id: object.path_id,
                            class_id: Some(ANIMATOR_CONTROLLER_CLASS_ID),
                        },
                        state_machine_index,
                        source_layer_index: 0,
                    };
                    insert_animator_controller_source_append(&mut inventory, target, limits)?;
                }
                Some(None) => {}
                None => {
                    inventory.append_file_indices.remove(&object.file_index);
                    insert_animator_controller_source_file(
                        &mut inventory.deferred_file_indices,
                        &inventory.append_file_indices,
                        object.file_index,
                        limits,
                    )?;
                }
            }
        }
    }
    let deferred_files = &inventory.deferred_file_indices;
    inventory
        .append_targets
        .retain(|target| !deferred_files.contains(&target.source_controller.file_index));
    Ok(inventory)
}

fn insert_animator_controller_source_append(
    inventory: &mut AnimatorControllerSourceDependencyInventory,
    target: WorkspaceAnimatorControllerStateMachineIdentity,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    let file_index = target.source_controller.file_index;
    if inventory.append_file_indices.contains(&file_index) {
        return Ok(());
    }
    let requested = inventory
        .append_file_indices
        .len()
        .checked_add(inventory.deferred_file_indices.len())
        .and_then(|count| count.checked_add(1))
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "AnimatorController source dependency file indexes",
        })?;
    if requested > limits.max_collections {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "AnimatorController source dependency file indexes",
            requested,
            limit: limits.max_collections,
        });
    }
    inventory.append_file_indices.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "AnimatorController source dependency inventory",
            requested,
        }
    })?;
    inventory.append_targets.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "AnimatorController source StateMachine targets",
            requested,
        }
    })?;
    inventory.append_file_indices.insert(file_index);
    inventory.append_targets.push(target);
    Ok(())
}

fn insert_animator_controller_source_file(
    destination: &mut HashSet<usize>,
    other_effects: &HashSet<usize>,
    file_index: usize,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    if destination.contains(&file_index) {
        return Ok(());
    }
    let requested = destination
        .len()
        .checked_add(other_effects.len())
        .and_then(|count| count.checked_add(1))
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "AnimatorController source dependency file indexes",
        })?;
    if requested > limits.max_collections {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "AnimatorController source dependency file indexes",
            requested,
            limit: limits.max_collections,
        });
    }
    destination
        .try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "AnimatorController source dependency inventory",
            requested,
        })?;
    destination.insert(file_index);
    Ok(())
}

fn animator_controller_requires_source_dependency(root: &SerializedValue) -> Option<Option<usize>> {
    let controller_constant = root.unique_field_in_base_chain("m_Controller").ok()??;
    let layer_array =
        unique_field_from_aliases(controller_constant, &["m_LayerArray", "m_HumanLayerArray"])?;
    let layer_values = layer_array.array_elements()?;
    let state_machine_array = controller_constant
        .unique_field("m_StateMachineArray")
        .ok()??;
    let state_machine_values = state_machine_array.array_elements()?;

    let mut first_state_machine_index = None;
    for (layer_index, layer_entry) in layer_values.iter().enumerate() {
        let layer = unwrap_animator_layer_constant(layer_entry)?;
        let state_machine_index = layer
            .unique_field_in_base_chain("m_StateMachineIndex")
            .ok()??;
        let state_machine_index = serialized_nonnegative_index(state_machine_index)?;
        if state_machine_index >= state_machine_values.len() {
            return None;
        }
        if layer_index == 0 {
            first_state_machine_index = Some(state_machine_index);
        }
    }

    Some(first_state_machine_index)
}

fn unique_field_from_aliases<'a>(
    value: &'a SerializedValue,
    aliases: &[&str],
) -> Option<&'a SerializedValue> {
    let mut found = None;
    for alias in aliases {
        if let Some(candidate) = value.unique_field(alias).ok()? {
            if found.is_some() {
                return None;
            }
            found = Some(candidate);
        }
    }
    found
}

fn unwrap_animator_layer_constant(value: &SerializedValue) -> Option<&SerializedValue> {
    let capitalized_data = value.unique_field("Data").ok()?;
    let lowercase_data = value.unique_field("data").ok()?;
    let wrapped = match (capitalized_data, lowercase_data) {
        (Some(data), None) | (None, Some(data)) => Some(data),
        (None, None) => None,
        (Some(_), Some(_)) => return None,
    };
    if let Some(layer) = wrapped {
        if value
            .unique_field_in_base_chain("m_StateMachineIndex")
            .ok()?
            .is_some()
        {
            return None;
        }
        Some(layer)
    } else {
        Some(value)
    }
}

fn serialized_nonnegative_index(value: &SerializedValue) -> Option<usize> {
    match value {
        SerializedValue::U8(index) => Some(usize::from(*index)),
        SerializedValue::U16(index) => Some(usize::from(*index)),
        SerializedValue::U32(index) => usize::try_from(*index).ok(),
        SerializedValue::U64(index) => usize::try_from(*index).ok(),
        SerializedValue::I8(index) => usize::try_from(*index).ok(),
        SerializedValue::I16(index) => usize::try_from(*index).ok(),
        SerializedValue::I32(index) => usize::try_from(*index).ok(),
        SerializedValue::I64(index) => usize::try_from(*index).ok(),
        _ => None,
    }
}

fn assign_source_dependency_file_ids(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    source_external_identifier_counts: &[usize],
    animator_controller_inventory: &AnimatorControllerSourceDependencyInventory,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    plan.modeled_animator_controller_dependency_append_count =
        animator_controller_inventory.modeled_append_count();

    let mut appended_count_by_file = HashMap::<usize, usize>::new();
    appended_count_by_file
        .try_reserve(plan.source_dependency_appends.len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "source dependency append order index",
            requested: plan.source_dependency_appends.len(),
        })?;

    let mut deferred_source_files = HashSet::new();
    for append in &mut plan.source_dependency_appends {
        let Some(&external_identifier_count) =
            source_external_identifier_counts.get(append.source_file_index)
        else {
            return Err(WorkspacePrefabProcessingError::WorkspaceOperationFailed {
                operation: "source dependency FileID assignment",
                message: format!(
                    "external identifier count is missing for source file {}",
                    append.source_file_index
                ),
            });
        };
        if animator_controller_inventory
            .deferred_file_indices
            .contains(&append.source_file_index)
        {
            if !deferred_source_files.contains(&append.source_file_index) {
                let requested = deferred_source_files.len().checked_add(1).ok_or(
                    WorkspacePrefabProcessingError::CountOverflow {
                        resource: "deferred source dependency files",
                    },
                )?;
                deferred_source_files.try_reserve(1).map_err(|_| {
                    WorkspacePrefabProcessingError::AllocationFailed {
                        resource: "deferred source dependency file index",
                        requested,
                    }
                })?;
                deferred_source_files.insert(append.source_file_index);
            }
            continue;
        }
        let prior_append_count = appended_count_by_file
            .get(&append.source_file_index)
            .copied()
            .unwrap_or(0);
        let initial_dependency_count = external_identifier_count.checked_add(1).ok_or(
            WorkspacePrefabProcessingError::CountOverflow {
                resource: "source collection dependency entries",
            },
        )?;
        let dependency_file_id = initial_dependency_count
            .checked_add(prior_append_count)
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "source collection dependency FileID",
            })?;
        let resulting_dependency_count = dependency_file_id.checked_add(1).ok_or(
            WorkspacePrefabProcessingError::CountOverflow {
                resource: "source collection dependency entries",
            },
        )?;
        if resulting_dependency_count > limits.max_source_dependency_entries_per_collection {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "source collection dependency entries",
                requested: resulting_dependency_count,
                limit: limits.max_source_dependency_entries_per_collection,
            });
        }
        append.file_id = Some(i32::try_from(dependency_file_id).map_err(|_| {
            WorkspacePrefabProcessingError::CountOverflow {
                resource: "source collection dependency FileID",
            }
        })?);
        appended_count_by_file.insert(
            append.source_file_index,
            prior_append_count.checked_add(1).ok_or(
                WorkspacePrefabProcessingError::CountOverflow {
                    resource: "source collection dependency append order",
                },
            )?,
        );
    }
    plan.deferred_source_dependency_file_count = deferred_source_files.len();
    if !deferred_source_files.is_empty() {
        plan.complete = false;
        plan.incomplete_reason = Some(format!(
            "AnimatorController source dependency effects remain unknown in {} collection files that also require source dependency FileIDs; those FileIDs are deferred",
            deferred_source_files.len()
        ));
    }
    Ok(())
}

fn ensure_scene_processed_collection(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    scene_collection_by_index: &mut [Option<usize>],
    scene_index: usize,
    scene_name: &str,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<usize, WorkspacePrefabProcessingError> {
    let scene_count = scene_collection_by_index.len();
    let slot = scene_collection_by_index
        .get_mut(scene_index)
        .ok_or_else(|| {
            processed_asset_operation_error(format!(
                "SceneDefinition index {scene_index} is outside {} scenes",
                scene_count
            ))
        })?;
    if let Some(collection_index) = *slot {
        return Ok(collection_index);
    }
    const SUFFIX: &str = " (Generated Assets)";
    let name_length = scene_name.len().checked_add(SUFFIX.len()).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "scene generated collection name",
        },
    )?;
    let mut name = String::new();
    name.try_reserve_exact(name_length).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "scene generated collection name",
            requested: name_length,
        }
    })?;
    name.push_str(scene_name);
    name.push_str(SUFFIX);
    let collection_index = add_processed_collection(
        plan,
        WorkspaceProcessedCollectionKind::SceneGeneratedAssets { scene_index },
        &name,
        limits,
    )?;
    *slot = Some(collection_index);
    Ok(collection_index)
}

/// Candidate asset-bundle name that the source processor tries to assign to a scene hierarchy.
///
/// Mirrors the `PrefabProcessor.Process` scene pass
/// (`AssetRipper/Source/AssetRipper.Processing/Prefabs/PrefabProcessor.cs`,
/// lines 28-48): the first `SerializedBundle` among the scene collections
/// supplies the candidate; its first `IAssetBundle` asset's `GetAssetBundleName`
/// wins when present, otherwise the bundle name is used. With no `SerializedBundle`
/// the source does not attempt an assignment. `GeneratedObjectBase.AssetBundleName`
/// has a no-op setter, so the effective generated hierarchy property remains null.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyBundleName {
    /// No `SerializedBundle` among the scene collections: no assignment candidate.
    Unset,
    /// A `SerializedBundle` without an `IAssetBundle` asset: `bundle.Name`.
    BundleName(String),
    /// The first `IAssetBundle` asset's `GetAssetBundleName()`.
    AssetName(String),
}

/// Selects the candidate that `PrefabProcessor` attempts to assign to the hierarchy.
///
/// `has_serialized_bundle` records the `FirstOrDefault(b => b is SerializedBundle)`
/// lookup and gates everything, matching the nested `if (bundle is not null)`.
/// `bundle_name` is that bundle's `Name`; `asset_bundle_name` carries the first
/// `IAssetBundle` asset's resolved `GetAssetBundleName()` (`Some` when such an
/// asset exists, `None` otherwise). Collection traversal order, the
/// `FetchAssets` order, and the `GetAssetBundleName` resolution itself (see
/// `asset_bundle.rs`) stay caller-side, as does the debug-only streamed-scene
/// assertion, which is intentionally not modeled. The generated `SceneHierarchyObject` inherits
/// a no-op `AssetBundleName` setter from `GeneratedObjectBase`, so the candidate is discarded and
/// the effective property remains unset.
pub fn scene_hierarchy_asset_bundle_name(
    has_serialized_bundle: bool,
    bundle_name: &str,
    asset_bundle_name: Option<&str>,
) -> WorkspaceSceneHierarchyBundleName {
    if !has_serialized_bundle {
        return WorkspaceSceneHierarchyBundleName::Unset;
    }
    match asset_bundle_name {
        Some(name) => WorkspaceSceneHierarchyBundleName::AssetName(name.to_owned()),
        None => WorkspaceSceneHierarchyBundleName::BundleName(bundle_name.to_owned()),
    }
}

/// One source bundle as seen by the `PrefabProcessor` scene pass.
///
/// `bundle_index` is the workspace container-bundle identity. `bundle_name`
/// is that bundle's `Name` (`FileContainer.Name` after
/// `SpecialFileNames.FixFileIdentifier`). `is_serialized_bundle` records the
/// `b is SerializedBundle` filter from
/// `scene.Collections.Select(c => c.Bundle).FirstOrDefault(b => b is SerializedBundle)`:
/// only serialized container bundles participate; standalone files and generated
/// processed collections never supply a hierarchy bundle name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneBundleSource {
    pub bundle_index: usize,
    pub bundle_name: String,
    pub is_serialized_bundle: bool,
}

/// One `IAssetBundle` asset in `Bundle.FetchAssets()` order.
///
/// `resolved_name` must already be the `AssetBundleExtensions.GetAssetBundleName()`
/// result for that asset (non-empty serialized `AssetBundleName_R` wins, otherwise
/// the owning bundle name), resolved with `asset_bundle::get_asset_bundle_name`.
/// The list for each bundle must follow `Bundle.FetchAssets()`:
/// direct collections in order, then child bundles recursively in source entry
/// order (`AssetRipper/Source/AssetRipper.Assets/Bundles/Bundle.cs`, lines 368-384).
/// `has_streamed_scene_flag` records `Has_IsStreamedSceneAssetBundle()` and
/// `is_streamed_scene_asset_bundle` records `IsStreamedSceneAssetBundle`; together
/// they replay the scene-pass `Debug.Assert`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceBundleAssetNameFact {
    pub resolved_name: String,
    pub has_streamed_scene_flag: bool,
    pub is_streamed_scene_asset_bundle: bool,
}

/// Per-scene hierarchy bundle assignment from the integrated scene pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyBundleAssignment {
    pub scene_index: usize,
    /// Candidate value that `PrefabProcessor` passes to the generated hierarchy setter.
    pub name: WorkspaceSceneHierarchyBundleName,
    /// Effective `SceneHierarchyObject.AssetBundleName` read-back value. The generated hierarchy
    /// inherits a no-op setter from `GeneratedObjectBase`, so the source leaves this unset.
    pub effective_asset_bundle_name: Option<String>,
    /// Present when the selected asset carries the streamed-scene field with a
    /// false value, which trips the source `Debug.Assert` in debug builds while
    /// release builds continue with the resolved name. Rust always continues with
    /// the resolved name and records this explicit warning instead of asserting.
    pub streamed_scene_assert_warning: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyBundlePlanLimits {
    pub max_scenes: usize,
    pub max_collections_per_scene: usize,
    pub max_bundle_name_bytes: usize,
    pub max_assets_per_bundle: usize,
}

impl Default for WorkspaceSceneHierarchyBundlePlanLimits {
    fn default() -> Self {
        Self {
            max_scenes: 1_000_000,
            max_collections_per_scene: 1_000_000,
            max_bundle_name_bytes: 16 * 1024 * 1024,
            max_assets_per_bundle: 1_000_000,
        }
    }
}

/// Plan every scene hierarchy's asset-bundle name in source order.
///
/// `scene_collection_file_indices` holds one entry per scene in
/// `GameBundle.Scenes` order; each inner list is that scene's
/// `SceneDefinition.Collections` order. `file_bundles` is indexed by workspace
/// file index; `Some` carries the owning bundle source and `None` means the file
/// has no `SerializedBundle` (standalone input). `bundle_assets_in_fetch_order`
/// maps a bundle index to its `IAssetBundle` facts in `FetchAssets()` order.
///
/// For each scene the first collection whose bundle is a serialized bundle wins
/// (`FirstOrDefault`), then that bundle's first `IAssetBundle` asset wins
/// (`FetchAssets().OfType<IAssetBundle>().FirstOrDefault()`): its resolved
/// `GetAssetBundleName()` becomes the candidate `AssetName`, while a bundle without such an
/// asset falls back to the candidate `bundle.Name`. A scene without any serialized bundle
/// stays `Unset`. All candidates are discarded by `GeneratedObjectBase.AssetBundleName`'s
/// no-op setter, leaving the effective property unset. The streamed-scene `Debug.Assert` becomes
/// an explicit warning; it never changes the candidate and never fails the plan.
pub fn plan_workspace_scene_hierarchy_bundle_names(
    scene_collection_file_indices: &[Vec<usize>],
    file_bundles: &[Option<WorkspaceSceneBundleSource>],
    bundle_assets_in_fetch_order: &HashMap<usize, Vec<WorkspaceBundleAssetNameFact>>,
    limits: WorkspaceSceneHierarchyBundlePlanLimits,
) -> Result<Vec<WorkspaceSceneHierarchyBundleAssignment>, WorkspacePrefabProcessingError> {
    if scene_collection_file_indices.len() > limits.max_scenes {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "scene hierarchy bundle scenes",
            requested: scene_collection_file_indices.len(),
            limit: limits.max_scenes,
        });
    }
    let mut assignments = Vec::new();
    assignments
        .try_reserve_exact(scene_collection_file_indices.len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "scene hierarchy bundle assignments",
            requested: scene_collection_file_indices.len(),
        })?;
    for (scene_index, collection_file_indices) in scene_collection_file_indices.iter().enumerate() {
        if collection_file_indices.len() > limits.max_collections_per_scene {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "scene hierarchy bundle collections",
                requested: collection_file_indices.len(),
                limit: limits.max_collections_per_scene,
            });
        }
        let mut selected_bundle: Option<&WorkspaceSceneBundleSource> = None;
        for &file_index in collection_file_indices {
            let bundle = file_bundles.get(file_index).ok_or_else(|| {
                WorkspacePrefabProcessingError::WorkspaceOperationFailed {
                    operation: "scene hierarchy bundle selection",
                    message: format!(
                        "scene {scene_index} collection file {file_index} is outside {} files",
                        file_bundles.len()
                    ),
                }
            })?;
            if let Some(source) = bundle {
                if source.bundle_name.len() > limits.max_bundle_name_bytes {
                    return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                        resource: "scene hierarchy bundle name bytes",
                        requested: source.bundle_name.len(),
                        limit: limits.max_bundle_name_bytes,
                    });
                }
                if source.is_serialized_bundle {
                    selected_bundle = Some(source);
                    break;
                }
            }
        }
        let Some(bundle) = selected_bundle else {
            assignments.push(WorkspaceSceneHierarchyBundleAssignment {
                scene_index,
                name: WorkspaceSceneHierarchyBundleName::Unset,
                effective_asset_bundle_name: None,
                streamed_scene_assert_warning: None,
            });
            continue;
        };
        let assets = bundle_assets_in_fetch_order.get(&bundle.bundle_index);
        let Some(assets) = assets else {
            assignments.push(WorkspaceSceneHierarchyBundleAssignment {
                scene_index,
                name: WorkspaceSceneHierarchyBundleName::BundleName(bundle.bundle_name.clone()),
                effective_asset_bundle_name: None,
                streamed_scene_assert_warning: None,
            });
            continue;
        };
        if assets.len() > limits.max_assets_per_bundle {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "scene hierarchy bundle assets",
                requested: assets.len(),
                limit: limits.max_assets_per_bundle,
            });
        }
        let Some(first) = assets.first() else {
            assignments.push(WorkspaceSceneHierarchyBundleAssignment {
                scene_index,
                name: WorkspaceSceneHierarchyBundleName::BundleName(bundle.bundle_name.clone()),
                effective_asset_bundle_name: None,
                streamed_scene_assert_warning: None,
            });
            continue;
        };
        if first.resolved_name.len() > limits.max_bundle_name_bytes {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "scene hierarchy asset bundle name bytes",
                requested: first.resolved_name.len(),
                limit: limits.max_bundle_name_bytes,
            });
        }
        let warning = if first.has_streamed_scene_flag && !first.is_streamed_scene_asset_bundle {
            Some(format!(
                "scene {scene_index} bundle '{}' asset '{:?}' carries IsStreamedSceneAssetBundle=false; source Debug.Assert would fire in debug builds, release continues with the resolved name",
                bundle.bundle_name, first.resolved_name
            ))
        } else {
            None
        };
        assignments.push(WorkspaceSceneHierarchyBundleAssignment {
            scene_index,
            name: WorkspaceSceneHierarchyBundleName::AssetName(first.resolved_name.clone()),
            effective_asset_bundle_name: None,
            streamed_scene_assert_warning: warning,
        });
    }
    Ok(assignments)
}

fn add_processed_asset(
    plan: &mut WorkspaceProcessedPrefabAssetPlan,
    collection_index: usize,
    class_id: i32,
    kind: WorkspaceProcessedPrefabAssetKind,
    limits: WorkspaceProcessedPrefabAssetPlanLimits,
) -> Result<WorkspaceProcessedAssetIdentity, WorkspacePrefabProcessingError> {
    let requested_count =
        plan.asset_count
            .checked_add(1)
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "generated processed assets",
            })?;
    if requested_count > limits.max_assets {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "generated processed assets",
            requested: requested_count,
            limit: limits.max_assets,
        });
    }
    let collection = plan.collections.get_mut(collection_index).ok_or_else(|| {
        processed_asset_operation_error(format!(
            "processed asset collection index {collection_index} is unavailable"
        ))
    })?;
    let path_id = collection.next_path_id.checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed collection PathID",
        },
    )?;
    collection.assets.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed collection assets",
            requested: collection.assets.len().saturating_add(1),
        }
    })?;
    let identity = WorkspaceProcessedAssetIdentity {
        collection_index,
        path_id,
        class_id,
    };
    collection
        .assets
        .push(WorkspaceProcessedPrefabAsset { identity, kind });
    collection.next_path_id = path_id;
    plan.asset_count = requested_count;
    Ok(identity)
}

fn processed_asset_operation_error(message: String) -> WorkspacePrefabProcessingError {
    WorkspacePrefabProcessingError::WorkspaceOperationFailed {
        operation: "processed prefab asset allocation planning",
        message,
    }
}

fn fetch_hierarchy(
    workspace: &WorkspaceSceneHierarchies,
    root_game_object: WorkspaceSceneObjectIdentity,
    plan: &WorkspacePrefabProcessingPlan,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<WorkspaceGameObjectHierarchyWalk, WorkspacePrefabProcessingError> {
    let remaining_entries = remaining_hierarchy_entries(plan, limits)?;
    let mut walk_limits = limits.hierarchy_walk;
    walk_limits.max_entries = walk_limits.max_entries.min(remaining_entries);
    workspace
        .fetch_game_object_hierarchy(root_game_object, walk_limits)
        .map_err(hierarchy_walk_error)
}

fn remaining_hierarchy_entries(
    plan: &WorkspacePrefabProcessingPlan,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<usize, WorkspacePrefabProcessingError> {
    limits
        .max_total_hierarchy_entries
        .checked_sub(plan.hierarchy_entry_count)
        .ok_or(WorkspacePrefabProcessingError::CountOverflow {
            resource: "remaining prefab hierarchy entries",
        })
}

fn plan_hierarchy_entries(
    source_walk: WorkspaceGameObjectHierarchyWalk,
    entry_limit: usize,
) -> Result<WorkspacePrefabHierarchyPlan, WorkspacePrefabProcessingError> {
    let source_entries = source_walk.entries();
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(source_entries.len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "Prefab hierarchy plan entries",
            requested: source_entries.len(),
        })?;

    let mut cursor = 0usize;
    while cursor < source_entries.len() {
        let game_object_entry = source_entries[cursor];
        if game_object_entry.kind != WorkspaceGameObjectHierarchyEntryKind::GameObject {
            return Err(WorkspacePrefabProcessingError::WorkspaceOperationFailed {
                operation: "Prefab hierarchy planning",
                message: format!("entry {cursor} is a component without a preceding GameObject"),
            });
        }
        push_hierarchy_plan_entry(
            &mut entries,
            WorkspacePrefabHierarchyPlanEntry::Source(game_object_entry),
            entry_limit,
        )?;
        cursor += 1;

        let mut has_transform = false;
        while let Some(component_entry) = source_entries.get(cursor).copied() {
            if component_entry.kind != WorkspaceGameObjectHierarchyEntryKind::Component {
                break;
            }
            if matches!(
                component_entry.identity.class_id,
                Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
            ) {
                has_transform = true;
            }
            push_hierarchy_plan_entry(
                &mut entries,
                WorkspacePrefabHierarchyPlanEntry::Source(component_entry),
                entry_limit,
            )?;
            cursor += 1;
        }
        if !has_transform {
            push_hierarchy_plan_entry(
                &mut entries,
                WorkspacePrefabHierarchyPlanEntry::GeneratedTransform {
                    game_object: game_object_entry.identity,
                    default_values: default_workspace_generated_transform_values(),
                },
                entry_limit,
            )?;
        }
    }

    Ok(WorkspacePrefabHierarchyPlan {
        source_walk,
        entries,
    })
}

fn push_hierarchy_plan_entry(
    entries: &mut Vec<WorkspacePrefabHierarchyPlanEntry>,
    entry: WorkspacePrefabHierarchyPlanEntry,
    limit: usize,
) -> Result<(), WorkspacePrefabProcessingError> {
    let requested =
        entries
            .len()
            .checked_add(1)
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "Prefab hierarchy plan entries",
            })?;
    if requested > limit {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "Prefab hierarchy plan entries",
            requested,
            limit,
        });
    }
    entries
        .try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "Prefab hierarchy plan entries",
            requested,
        })?;
    entries.push(entry);
    Ok(())
}

fn add_hierarchy_entry_count(
    plan: &mut WorkspacePrefabProcessingPlan,
    count: usize,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    let requested = plan.hierarchy_entry_count.checked_add(count).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "prefab hierarchy entries",
        },
    )?;
    if requested > limits.max_total_hierarchy_entries {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "prefab hierarchy entries",
            requested,
            limit: limits.max_total_hierarchy_entries,
        });
    }
    plan.hierarchy_entry_count = requested;
    Ok(())
}

fn mark_hierarchy_game_objects(
    hierarchy: &WorkspacePrefabHierarchyPlan,
    processed_set: &mut HashSet<(usize, usize)>,
    plan: &mut WorkspacePrefabProcessingPlan,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    for entry in hierarchy.source_walk().entries() {
        if entry.kind == WorkspaceGameObjectHierarchyEntryKind::GameObject {
            mark_processed(entry.identity, processed_set, plan, limits)?;
        }
    }
    Ok(())
}

fn is_processed(
    game_object: WorkspaceSceneObjectIdentity,
    processed_set: &HashSet<(usize, usize)>,
) -> bool {
    processed_set.contains(&(game_object.file_index, game_object.object_index))
}

fn mark_processed(
    game_object: WorkspaceSceneObjectIdentity,
    processed_set: &mut HashSet<(usize, usize)>,
    plan: &mut WorkspacePrefabProcessingPlan,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<bool, WorkspacePrefabProcessingError> {
    if is_processed(game_object, processed_set) {
        return Ok(false);
    }
    let requested = plan.processed_game_objects.len().checked_add(1).ok_or(
        WorkspacePrefabProcessingError::CountOverflow {
            resource: "processed GameObjects",
        },
    )?;
    if requested > limits.max_processed_game_objects {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "processed GameObjects",
            requested,
            limit: limits.max_processed_game_objects,
        });
    }
    plan.processed_game_objects.try_reserve(1).map_err(|_| {
        WorkspacePrefabProcessingError::AllocationFailed {
            resource: "ordered processed GameObjects",
            requested,
        }
    })?;
    processed_set
        .try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "processed GameObject membership set",
            requested,
        })?;
    processed_set.insert((game_object.file_index, game_object.object_index));
    plan.processed_game_objects.push(game_object);
    Ok(true)
}

fn push_event(
    plan: &mut WorkspacePrefabProcessingPlan,
    event: WorkspacePrefabProcessingEvent,
    limits: WorkspacePrefabProcessingLimits,
) -> Result<(), WorkspacePrefabProcessingError> {
    let requested =
        plan.events
            .len()
            .checked_add(1)
            .ok_or(WorkspacePrefabProcessingError::CountOverflow {
                resource: "Prefab processing events",
            })?;
    if requested > limits.max_events {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "Prefab processing events",
            requested,
            limit: limits.max_events,
        });
    }
    plan.events
        .try_reserve(1)
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "Prefab processing events",
            requested,
        })?;
    plan.events.push(event);
    Ok(())
}

fn operation_error(operation: &'static str, error: impl Display) -> WorkspacePrefabProcessingError {
    WorkspacePrefabProcessingError::WorkspaceOperationFailed {
        operation,
        message: error.to_string(),
    }
}

fn root_walk_error(error: WorkspaceGetRootGameObjectError) -> WorkspacePrefabProcessingError {
    match error {
        WorkspaceGetRootGameObjectError::AllocationFailed {
            resource,
            requested,
        } => WorkspacePrefabProcessingError::AllocationFailed {
            resource,
            requested,
        },
        WorkspaceGetRootGameObjectError::CountOverflow { resource } => {
            WorkspacePrefabProcessingError::CountOverflow { resource }
        }
        WorkspaceGetRootGameObjectError::ParentDepthLimitExceeded { requested, limit } => {
            WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "Transform parent depth",
                requested,
                limit,
            }
        }
        WorkspaceGetRootGameObjectError::GraphInvariantFailed { message } => {
            WorkspacePrefabProcessingError::WorkspaceOperationFailed {
                operation: "GameObject GetRoot",
                message,
            }
        }
    }
}

fn hierarchy_walk_error(
    error: WorkspaceGameObjectHierarchyWalkError,
) -> WorkspacePrefabProcessingError {
    match error {
        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
            resource,
            requested,
        } => WorkspacePrefabProcessingError::AllocationFailed {
            resource,
            requested,
        },
        WorkspaceGameObjectHierarchyWalkError::CountOverflow { resource } => {
            WorkspacePrefabProcessingError::CountOverflow { resource }
        }
        WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
            resource,
            requested,
            limit,
        } => WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource,
            requested,
            limit,
        },
        other => operation_error("GameObject FetchHierarchy", other),
    }
}
