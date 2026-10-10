use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SourceDependencyAppendRequest {
    pub(super) owner: WorkspaceSceneObjectIdentity,
    pub(super) target_collection: WorkspaceSourceDependencyAppendTarget,
    pub(super) pointer_field: WorkspaceSourceDependencyPointerField,
    pub(super) target_asset: Option<WorkspaceProcessedAssetIdentity>,
    pub(super) target_animator_state_machine:
        Option<WorkspaceAnimatorControllerStateMachineIdentity>,
}

pub(super) fn plan_source_dependency_append(
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

pub(super) fn plan_animator_controller_source_dependency_appends(
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

pub(super) fn assign_source_dependency_file_ids(
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

#[cfg(test)]
#[path = "../tests/dependencies.rs"]
mod tests;
