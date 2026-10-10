use std::collections::HashMap;

use crate::serialized_file::collection_workspace::WorkspaceAssetCollections;
use crate::serialized_file::object::SerializedValue;
use crate::serialized_file::object_workspace::{ObjectWorkspace, WorkspaceObject};
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::bundle_names::{
    workspace_object_class_id, AssetBundleContainerTarget, WorkspaceAssetBundleNamePlan,
};
use super::path_model::{
    export_path_metadata_bytes, export_path_record_bytes, original_path_detail_slices,
    try_copy_export_path_string, BundledAssetsExportMode, WorkspaceAssetBundlePathError,
    WorkspaceAssetExportPath, WorkspaceAssetExportPathLimits, WorkspaceAssetExportPathReport,
    WorkspaceAssetExportPathTable, WorkspaceOriginalPathPlans,
};
use super::resource_manager::WorkspaceResourceManagerPathPlan;
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BundleOriginalPathEffect {
    pub(super) original_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct OriginalPathDetails {
    pub(super) directory: Option<String>,
    pub(super) name: Option<String>,
    pub(super) extension: Option<String>,
}

pub(super) struct WorkspaceAssetBundlePathApplyState<'a> {
    records: &'a mut Vec<WorkspaceAssetExportPath>,
    record_index: &'a mut HashMap<(usize, usize), usize>,
    metadata_bytes: &'a mut usize,
    operation_count: &'a mut usize,
    report: &'a mut WorkspaceAssetExportPathReport,
    limits: WorkspaceAssetExportPathLimits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkspaceOriginalPathEventKind {
    AssetBundle(usize),
    ResourceManager(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WorkspaceOriginalPathEvent {
    pub(super) collection_index: usize,
    pub(super) source_object_index: usize,
    pub(super) container_entry_index: usize,
    pub(super) kind: WorkspaceOriginalPathEventKind,
}

pub(super) fn sort_workspace_original_path_events(events: &mut [WorkspaceOriginalPathEvent]) {
    events.sort_unstable_by_key(|event| {
        (
            event.collection_index,
            event.source_object_index,
            event.container_entry_index,
        )
    });
}

/// Apply ResourceManager and AssetBundle path updates in `GameBundle.FetchAssets()` order.
///
/// The per-object Container order is retained, then GroupByBundleName collection directories
/// are filled after both processors as the baseline does.
pub fn apply_workspace_original_paths(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    collections: &WorkspaceAssetCollections,
    plans: WorkspaceOriginalPathPlans<'_>,
    mode: BundledAssetsExportMode,
    mut export_paths: WorkspaceAssetExportPathTable,
    limits: WorkspaceAssetExportPathLimits,
) -> Result<
    (
        WorkspaceAssetExportPathTable,
        WorkspaceAssetExportPathReport,
    ),
    WorkspaceAssetBundlePathError,
> {
    let bundle_plan = plans.asset_bundles;
    let resource_manager_plan = plans.resource_managers;
    if !bundle_plan.is_complete() {
        return Err(WorkspaceAssetBundlePathError::IncompleteNamePlan {
            issue_count: bundle_plan.issues.len(),
        });
    }
    if !resource_manager_plan.is_complete() {
        return Err(
            WorkspaceAssetBundlePathError::IncompleteResourceManagerPlan {
                issue_count: resource_manager_plan.issues.len(),
            },
        );
    }
    if bundle_plan.bundles.len() > limits.max_bundles {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "AssetBundle path source records",
            count: bundle_plan.bundles.len(),
            limit: limits.max_bundles,
        });
    }
    if bundle_plan.assignments.len() > limits.max_assignments {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "AssetBundle path assignments",
            count: bundle_plan.assignments.len(),
            limit: limits.max_assignments,
        });
    }
    if resource_manager_plan.assignments.len() > limits.max_assignments {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "ResourceManager path assignments",
            count: resource_manager_plan.assignments.len(),
            limit: limits.max_assignments,
        });
    }
    let event_count = bundle_plan
        .assignments
        .len()
        .checked_add(resource_manager_plan.assignments.len())
        .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
            resource: "OriginalPath processor events",
        })?;
    if event_count > limits.max_total_operations {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "OriginalPath processor events",
            count: event_count,
            limit: limits.max_total_operations,
        });
    }
    let mut events = Vec::new();
    events.try_reserve_exact(event_count).map_err(|_| {
        WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "OriginalPath processor events",
            requested: event_count,
        }
    })?;
    for (assignment_index, assignment) in bundle_plan.assignments.iter().enumerate() {
        let bundle = bundle_plan
            .bundles
            .get(assignment.bundle_record_index)
            .ok_or(WorkspaceAssetBundlePathError::InvalidNamePlan {
                resource: "AssetBundle record index",
                index: assignment.bundle_record_index,
            })?;
        let collection_index = collections
            .collection_index_for_file(bundle.source_file_index)
            .ok_or(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                resource: "AssetBundle source collection",
                index: bundle.source_file_index,
            })?;
        let container_entry_index = assignment
            .reference_record_index
            .checked_sub(bundle.container_references.start)
            .filter(|index| *index < bundle.container_references.len())
            .ok_or(WorkspaceAssetBundlePathError::InvalidNamePlan {
                resource: "AssetBundle Container assignment order",
                index: assignment.reference_record_index,
            })?;
        events.push(WorkspaceOriginalPathEvent {
            collection_index,
            source_object_index: bundle.source_object_index,
            container_entry_index,
            kind: WorkspaceOriginalPathEventKind::AssetBundle(assignment_index),
        });
    }
    for (assignment_index, assignment) in resource_manager_plan.assignments.iter().enumerate() {
        let actual_collection_index = collections
            .collection_index_for_file(assignment.source_file_index)
            .ok_or(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                resource: "ResourceManager source collection",
                index: assignment.source_file_index,
            })?;
        if actual_collection_index != assignment.source_collection_index {
            return Err(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                resource: "ResourceManager source collection order",
                index: assignment.source_file_index,
            });
        }
        events.push(WorkspaceOriginalPathEvent {
            collection_index: assignment.source_collection_index,
            source_object_index: assignment.source_object_index,
            container_entry_index: assignment.container_entry_index,
            kind: WorkspaceOriginalPathEventKind::ResourceManager(assignment_index),
        });
    }
    sort_workspace_original_path_events(&mut events);
    if export_paths.records.len() > limits.max_records {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "export path metadata records",
            count: export_paths.records.len(),
            limit: limits.max_records,
        });
    }
    let mut metadata_bytes = export_path_metadata_bytes(&export_paths.records)?;
    if metadata_bytes > limits.max_total_metadata_bytes {
        return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
            byte_count: metadata_bytes,
            limit: limits.max_total_metadata_bytes,
        });
    }

    let mut record_index = HashMap::<(usize, usize), usize>::new();
    record_index
        .try_reserve(export_paths.records.len())
        .map_err(|_| WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "export path metadata identity index",
            requested: export_paths.records.len(),
        })?;
    for (index, record) in export_paths.records.iter().enumerate() {
        let object = objects.get(record.file_index, record.object_index).ok_or(
            WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                resource: "export metadata object",
                index: record.object_index,
                count: objects.objects().len(),
            },
        )?;
        if object.path_id != record.path_id {
            return Err(WorkspaceAssetBundlePathError::ObjectIdentityMismatch {
                file_index: record.file_index,
                object_index: record.object_index,
            });
        }
        if record_index
            .insert((record.file_index, record.object_index), index)
            .is_some()
        {
            return Err(WorkspaceAssetBundlePathError::DuplicateMetadataIdentity {
                file_index: record.file_index,
                object_index: record.object_index,
            });
        }
    }

    let mut report = WorkspaceAssetExportPathReport::default();
    let mut operation_count = 0usize;
    for event in &events {
        operation_count = checked_path_operation_count(operation_count, limits)?;
        if let WorkspaceOriginalPathEventKind::AssetBundle(assignment_index) = event.kind {
            let assignment = bundle_plan.assignments.get(assignment_index).ok_or(
                WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "AssetBundle assignment index",
                    index: assignment_index,
                },
            )?;
            let bundle = bundle_plan
                .bundles
                .get(assignment.bundle_record_index)
                .ok_or(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "AssetBundle record index",
                    index: assignment.bundle_record_index,
                })?;
            let reference = bundle_plan
                .container_references
                .get(assignment.reference_record_index)
                .ok_or(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "Container reference index",
                    index: assignment.reference_record_index,
                })?;
            if !bundle
                .container_references
                .contains(&assignment.reference_record_index)
                || reference.bundle_record_index != assignment.bundle_record_index
                || !matches!(
                    reference.mapping.target,
                    AssetBundleContainerTarget::LocalObject { object_index }
                        if object_index == assignment.target_object_index
                )
                || bundle.source_file_index != assignment.target_file_index
            {
                return Err(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "AssetBundle assignment identity",
                    index: assignment.reference_record_index,
                });
            }
            let target = objects
                .get(assignment.target_file_index, assignment.target_object_index)
                .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                    resource: "AssetBundle assignment target object",
                    index: assignment.target_object_index,
                    count: objects.objects().len(),
                })?;
            if target.path_id != assignment.target_path_id
                || target.path_id != reference.mapping.reference.path_id
            {
                return Err(WorkspaceAssetBundlePathError::ObjectIdentityMismatch {
                    file_index: assignment.target_file_index,
                    object_index: assignment.target_object_index,
                });
            }

            let path_effect = bundle_container_original_path(
                &reference.mapping.reference.path,
                &bundle.asset_name,
                mode,
            )?;
            let bundle_name = try_copy_export_path_string(&bundle.asset_name)?;
            let named_asset_name = if path_effect.is_some() {
                match workspace_named_asset_name(target) {
                    Ok(name) => name,
                    Err(WorkspaceAssetBundlePathError::ObjectPayloadUnavailable { .. }) => {
                        report.named_asset_name_unavailable = report
                            .named_asset_name_unavailable
                            .checked_add(1)
                            .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                                resource: "unavailable INamed asset names",
                            })?;
                        None
                    }
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            let is_shader = workspace_object_class_id(target) == Some(48);
            let index = ensure_export_path_record(
                &mut export_paths.records,
                &mut record_index,
                assignment.target_file_index,
                assignment.target_object_index,
                assignment.target_path_id,
                limits.max_records,
            )?;
            let old_record_bytes = export_path_record_bytes(&export_paths.records[index])?;
            let record = &mut export_paths.records[index];
            record.asset_bundle_name = Some(bundle_name);
            if let Some(effect) = path_effect {
                if effect.original_path.is_some() {
                    report.assigned_original_paths = report
                        .assigned_original_paths
                        .checked_add(1)
                        .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                            resource: "assigned OriginalPaths",
                        })?;
                }
                apply_original_path_postprocessing(
                    record,
                    effect.original_path,
                    named_asset_name.as_deref(),
                    is_shader,
                    &mut report,
                )?;
            }
            let new_record_bytes = export_path_record_bytes(record)?;
            metadata_bytes = metadata_bytes
                .checked_sub(old_record_bytes)
                .and_then(|bytes| bytes.checked_add(new_record_bytes))
                .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
            if metadata_bytes > limits.max_total_metadata_bytes {
                return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
                    byte_count: metadata_bytes,
                    limit: limits.max_total_metadata_bytes,
                });
            }
            report.assigned_bundle_names = report.assigned_bundle_names.checked_add(1).ok_or(
                WorkspaceAssetBundlePathError::CountOverflow {
                    resource: "assigned AssetBundleNames",
                },
            )?;
        } else {
            let WorkspaceOriginalPathEventKind::ResourceManager(assignment_index) = event.kind
            else {
                continue;
            };
            let assignment = resource_manager_plan
                .assignments
                .get(assignment_index)
                .ok_or(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "ResourceManager assignment index",
                    index: assignment_index,
                })?;
            if assignment.source_collection_index != event.collection_index
                || assignment.source_object_index != event.source_object_index
                || assignment.container_entry_index != event.container_entry_index
            {
                return Err(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "ResourceManager assignment order",
                    index: assignment_index,
                });
            }
            let source = objects
                .get(assignment.source_file_index, assignment.source_object_index)
                .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                    resource: "ResourceManager assignment source",
                    index: assignment.source_object_index,
                    count: objects.objects().len(),
                })?;
            if source.path_id != assignment.source_path_id
                || workspace_object_class_id(source) != Some(147)
                || assignment.target_path_id == 0
            {
                return Err(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "ResourceManager source identity",
                    index: assignment_index,
                });
            }
            let target = objects
                .get(assignment.target_file_index, assignment.target_object_index)
                .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                    resource: "ResourceManager assignment target",
                    index: assignment.target_object_index,
                    count: objects.objects().len(),
                })?;
            if target.path_id != assignment.target_path_id {
                return Err(WorkspaceAssetBundlePathError::ObjectIdentityMismatch {
                    file_index: assignment.target_file_index,
                    object_index: assignment.target_object_index,
                });
            }
            let resource_path = join_project_path(&["Assets/Resources", &assignment.path])?;
            let current_path = if let Some(index) = record_index
                .get(&(assignment.target_file_index, assignment.target_object_index))
                .copied()
            {
                let current_record = export_paths
                    .records
                    .get(index)
                    .ok_or(WorkspaceAssetBundlePathError::InvalidMetadataIndex { index })?;
                if let Some(current_path) = current_record.original_path.as_deref() {
                    Some(try_copy_export_path_string(current_path)?)
                } else {
                    calculated_original_path(current_record)?
                }
            } else {
                None
            };
            if !resource_manager_path_should_replace(current_path.as_deref(), &resource_path) {
                continue;
            }
            let named_asset_name = match workspace_named_asset_name(target) {
                Ok(name) => name,
                Err(WorkspaceAssetBundlePathError::ObjectPayloadUnavailable { .. }) => {
                    report.named_asset_name_unavailable = report
                        .named_asset_name_unavailable
                        .checked_add(1)
                        .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                            resource: "unavailable INamed asset names",
                        })?;
                    None
                }
                Err(error) => return Err(error),
            };
            let index = ensure_export_path_record(
                &mut export_paths.records,
                &mut record_index,
                assignment.target_file_index,
                assignment.target_object_index,
                assignment.target_path_id,
                limits.max_records,
            )?;
            let old_record_bytes = export_path_record_bytes(&export_paths.records[index])?;
            let record = &mut export_paths.records[index];
            apply_original_path_postprocessing(
                record,
                Some(resource_path),
                named_asset_name.as_deref(),
                workspace_object_class_id(target) == Some(48),
                &mut report,
            )?;
            let new_record_bytes = export_path_record_bytes(record)?;
            metadata_bytes = metadata_bytes
                .checked_sub(old_record_bytes)
                .and_then(|bytes| bytes.checked_add(new_record_bytes))
                .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
            if metadata_bytes > limits.max_total_metadata_bytes {
                return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
                    byte_count: metadata_bytes,
                    limit: limits.max_total_metadata_bytes,
                });
            }
            report.assigned_resource_manager_original_paths = report
                .assigned_resource_manager_original_paths
                .checked_add(1)
                .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                    resource: "assigned ResourceManager OriginalPaths",
                })?;
        }
    }

    if mode == BundledAssetsExportMode::GroupByBundleName {
        let mut state = WorkspaceAssetBundlePathApplyState {
            records: &mut export_paths.records,
            record_index: &mut record_index,
            metadata_bytes: &mut metadata_bytes,
            operation_count: &mut operation_count,
            report: &mut report,
            limits,
        };
        apply_group_by_bundle_name_directories(
            workspace,
            objects,
            collections,
            bundle_plan,
            &mut state,
        )?;
    }

    export_paths
        .records
        .sort_unstable_by_key(|record| (record.file_index, record.object_index));
    report.metadata_record_count = export_paths.records.len();
    report.metadata_string_bytes = metadata_bytes;
    Ok((export_paths, report))
}

fn apply_group_by_bundle_name_directories(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    collections: &WorkspaceAssetCollections,
    bundle_plan: &WorkspaceAssetBundleNamePlan,
    state: &mut WorkspaceAssetBundlePathApplyState<'_>,
) -> Result<(), WorkspaceAssetBundlePathError> {
    let mut collection_index_by_file = Vec::new();
    collection_index_by_file
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "collection identity index",
            requested: workspace.files().len(),
        })?;
    collection_index_by_file.resize(workspace.files().len(), None::<usize>);
    for (index, collection) in collections.collections().iter().enumerate() {
        let slot = collection_index_by_file
            .get_mut(collection.source_file_index)
            .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                resource: "source collection file",
                index: collection.source_file_index,
                count: workspace.files().len(),
            })?;
        if slot.replace(index).is_some() {
            return Err(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                resource: "duplicate collection file",
                index: collection.source_file_index,
            });
        }
    }

    let mut bundle_for_collection = Vec::new();
    bundle_for_collection
        .try_reserve_exact(collections.collections().len())
        .map_err(|_| WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "bundle-name collection map",
            requested: collections.collections().len(),
        })?;
    bundle_for_collection.resize(collections.collections().len(), None::<usize>);
    for (bundle_record_index, bundle_record) in bundle_plan.bundles.iter().enumerate() {
        let source_file = workspace
            .files()
            .get(bundle_record.source_file_index)
            .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                resource: "AssetBundle source file",
                index: bundle_record.source_file_index,
                count: workspace.files().len(),
            })?;
        let Some(owner_bundle_index) = source_file.container_bundle_index else {
            continue;
        };
        let owner_bundle = workspace
            .container_bundles()
            .get(owner_bundle_index)
            .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                resource: "owner Bundle",
                index: owner_bundle_index,
                count: workspace.container_bundles().len(),
            })?;
        for &file_index in &owner_bundle.direct_collection_file_indices {
            let collection_index = collection_index_by_file
                .get(file_index)
                .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                    resource: "Bundle collection file",
                    index: file_index,
                    count: collection_index_by_file.len(),
                })?
                .ok_or(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                    resource: "Bundle collection missing from asset collections",
                    index: file_index,
                })?;
            let collection = collections.collections().get(collection_index).ok_or(
                WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                    resource: "asset collection index",
                    index: collection_index,
                    count: collections.collections().len(),
                },
            )?;
            if collection.container_bundle_index != Some(owner_bundle_index) {
                return Err(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                    resource: "Bundle collection owner mismatch",
                    index: file_index,
                });
            }
            bundle_for_collection[collection_index] = Some(bundle_record_index);
        }
    }

    for (collection_index, bundle_record_index) in bundle_for_collection.into_iter().enumerate() {
        let Some(bundle_record_index) = bundle_record_index else {
            continue;
        };
        let bundle = bundle_plan.bundles.get(bundle_record_index).ok_or(
            WorkspaceAssetBundlePathError::InvalidNamePlan {
                resource: "collection Bundle record index",
                index: bundle_record_index,
            },
        )?;
        let collection = collections.collections().get(collection_index).ok_or(
            WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                resource: "asset collection index",
                index: collection_index,
                count: collections.collections().len(),
            },
        )?;
        let source_objects = objects
            .objects()
            .get(collection.object_range.clone())
            .ok_or(WorkspaceAssetBundlePathError::ObjectRangeOutOfBounds {
                start: collection.object_range.start,
                end: collection.object_range.end,
                object_count: objects.objects().len(),
            })?;
        for object in source_objects {
            if object.file_index != collection.source_file_index {
                return Err(WorkspaceAssetBundlePathError::ObjectIdentityMismatch {
                    file_index: object.file_index,
                    object_index: object.object_index,
                });
            }
            let existing_index = state
                .record_index
                .get(&(object.file_index, object.object_index))
                .copied();
            if let Some(index) = existing_index {
                let existing = state
                    .records
                    .get(index)
                    .ok_or(WorkspaceAssetBundlePathError::InvalidMetadataIndex { index })?;
                if existing.original_directory.is_some() {
                    continue;
                }
            }
            let payload = object.payload.as_ref().map_err(|_| {
                WorkspaceAssetBundlePathError::ObjectPayloadUnavailable {
                    file_index: object.file_index,
                    object_index: object.object_index,
                }
            })?;
            if payload.type_name.is_empty() {
                return Err(WorkspaceAssetBundlePathError::ClassNameUnavailable {
                    file_index: object.file_index,
                    object_index: object.object_index,
                });
            }
            *state.operation_count =
                checked_path_operation_count(*state.operation_count, state.limits)?;
            let directory = join_project_path(&[
                "Assets/AssetBundles",
                &bundle.asset_name,
                &payload.type_name,
            ])?;
            let record_index_value = ensure_export_path_record(
                state.records,
                state.record_index,
                object.file_index,
                object.object_index,
                object.path_id,
                state.limits.max_records,
            )?;
            let old_record_bytes = export_path_record_bytes(&state.records[record_index_value])?;
            state.records[record_index_value].original_directory = Some(directory);
            state.records[record_index_value].original_path =
                calculated_original_path(&state.records[record_index_value])?;
            let new_record_bytes = export_path_record_bytes(&state.records[record_index_value])?;
            *state.metadata_bytes = (*state.metadata_bytes)
                .checked_sub(old_record_bytes)
                .and_then(|bytes| bytes.checked_add(new_record_bytes))
                .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
            if *state.metadata_bytes > state.limits.max_total_metadata_bytes {
                return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
                    byte_count: *state.metadata_bytes,
                    limit: state.limits.max_total_metadata_bytes,
                });
            }
            state.report.assigned_collection_directories = state
                .report
                .assigned_collection_directories
                .checked_add(1)
                .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                    resource: "collection OriginalDirectories",
                })?;
        }
    }
    Ok(())
}

fn ensure_export_path_record(
    records: &mut Vec<WorkspaceAssetExportPath>,
    record_index: &mut HashMap<(usize, usize), usize>,
    file_index: usize,
    object_index: usize,
    path_id: i64,
    maximum_records: usize,
) -> Result<usize, WorkspaceAssetBundlePathError> {
    if let Some(index) = record_index.get(&(file_index, object_index)) {
        let record = records
            .get(*index)
            .ok_or(WorkspaceAssetBundlePathError::InvalidMetadataIndex { index: *index })?;
        if record.path_id != path_id {
            return Err(WorkspaceAssetBundlePathError::ObjectIdentityMismatch {
                file_index,
                object_index,
            });
        }
        return Ok(*index);
    }
    let requested =
        records
            .len()
            .checked_add(1)
            .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                resource: "export path metadata records",
            })?;
    if requested > maximum_records {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "export path metadata records",
            count: requested,
            limit: maximum_records,
        });
    }
    records
        .try_reserve(1)
        .map_err(|_| WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "export path metadata records",
            requested,
        })?;
    record_index
        .try_reserve(1)
        .map_err(|_| WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "export path metadata identity index",
            requested,
        })?;
    let index = records.len();
    records.push(WorkspaceAssetExportPath {
        file_index,
        object_index,
        path_id,
        ..WorkspaceAssetExportPath::default()
    });
    record_index.insert((file_index, object_index), index);
    Ok(index)
}

fn checked_path_operation_count(
    count: usize,
    limits: WorkspaceAssetExportPathLimits,
) -> Result<usize, WorkspaceAssetBundlePathError> {
    let requested = count
        .checked_add(1)
        .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
            resource: "AssetBundle path operations",
        })?;
    if requested > limits.max_total_operations {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "AssetBundle path operations",
            count: requested,
            limit: limits.max_total_operations,
        });
    }
    Ok(requested)
}

pub(super) fn original_path_details(
    path: &str,
) -> Result<OriginalPathDetails, WorkspaceAssetBundlePathError> {
    let details = original_path_detail_slices(path);
    Ok(OriginalPathDetails {
        directory: details
            .directory
            .map(try_copy_export_path_string)
            .transpose()?,
        name: details.name.map(try_copy_export_path_string).transpose()?,
        extension: details
            .extension
            .map(try_copy_export_path_string)
            .transpose()?,
    })
}

pub(super) fn calculated_original_path(
    record: &WorkspaceAssetExportPath,
) -> Result<Option<String>, WorkspaceAssetBundlePathError> {
    if record.original_directory.is_none()
        && record.original_name.is_none()
        && record.original_extension.is_none()
    {
        return Ok(None);
    }
    let extension_length = record
        .original_extension
        .as_ref()
        .map_or(Some(0usize), |extension| extension.len().checked_add(1))
        .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
    let name_length = record
        .original_name
        .as_ref()
        .map_or(0, String::len)
        .checked_add(extension_length)
        .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
    let mut name = String::new();
    name.try_reserve_exact(name_length).map_err(|_| {
        WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "calculated OriginalPath filename",
            requested: name_length,
        }
    })?;
    if let Some(original_name) = record.original_name.as_deref() {
        name.push_str(original_name);
    }
    if let Some(extension) = record.original_extension.as_deref() {
        name.push('.');
        name.push_str(extension);
    }
    if let Some(directory) = record.original_directory.as_deref() {
        join_project_path(&[directory, &name]).map(Some)
    } else if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

pub(super) fn resource_manager_path_should_replace(
    current_path: Option<&str>,
    candidate_path: &str,
) -> bool {
    current_path.is_none_or(|current_path| {
        current_path.encode_utf16().count() < candidate_path.encode_utf16().count()
    })
}

fn workspace_named_asset_name(
    object: &WorkspaceObject,
) -> Result<Option<String>, WorkspaceAssetBundlePathError> {
    let payload = object.payload.as_ref().map_err(|_| {
        WorkspaceAssetBundlePathError::ObjectPayloadUnavailable {
            file_index: object.file_index,
            object_index: object.object_index,
        }
    })?;
    let value = payload
        .root
        .unique_field_in_base_chain("m_Name")
        .map_err(
            |source| WorkspaceAssetBundlePathError::NamedAssetFieldLookup {
                file_index: object.file_index,
                object_index: object.object_index,
                source,
            },
        )?;
    let Some(SerializedValue::StringBytes(bytes)) = value else {
        return Ok(None);
    };
    let name = str::from_utf8(bytes).map_err(|error| {
        WorkspaceAssetBundlePathError::InvalidNamedAssetUtf8 {
            file_index: object.file_index,
            object_index: object.object_index,
            valid_up_to: error.valid_up_to(),
        }
    })?;
    try_copy_export_path_string(name).map(Some)
}

pub(super) fn restore_original_name(
    record: &mut WorkspaceAssetExportPath,
    asset_name: &str,
    report: &mut WorkspaceAssetExportPathReport,
) -> Result<bool, WorkspaceAssetBundlePathError> {
    let Some(original_name) = record.original_name.as_deref() else {
        return Ok(false);
    };
    if original_name == asset_name {
        return Ok(true);
    }
    if original_name.encode_utf16().count() != asset_name.encode_utf16().count() {
        return Ok(false);
    }
    if original_name.is_ascii() && asset_name.is_ascii() {
        if !original_name.eq_ignore_ascii_case(asset_name) {
            return Ok(false);
        }
        record.original_name = Some(try_copy_export_path_string(asset_name)?);
        report.restored_original_names = report.restored_original_names.checked_add(1).ok_or(
            WorkspaceAssetBundlePathError::CountOverflow {
                resource: "restored OriginalNames",
            },
        )?;
        return Ok(true);
    }
    report.case_restoration_unverified = report.case_restoration_unverified.checked_add(1).ok_or(
        WorkspaceAssetBundlePathError::CountOverflow {
            resource: "unverified non-ASCII OriginalName comparisons",
        },
    )?;
    Ok(false)
}

pub(super) fn initialize_shader_overrides(
    record: &mut WorkspaceAssetExportPath,
    report: &mut WorkspaceAssetExportPathReport,
) -> Result<(), WorkspaceAssetBundlePathError> {
    let mut initialized = false;
    if record.override_directory.is_none() {
        if let Some(value) = record.original_directory.as_deref() {
            record.override_directory = Some(try_copy_export_path_string(value)?);
            initialized = true;
        }
    }
    if record.override_name.is_none() {
        if let Some(value) = record.original_name.as_deref() {
            record.override_name = Some(try_copy_export_path_string(value)?);
            initialized = true;
        }
    }
    if record.override_extension.is_none() {
        if let Some(value) = record.original_extension.as_deref() {
            record.override_extension = Some(try_copy_export_path_string(value)?);
            initialized = true;
        }
    }
    if initialized {
        report.initialized_shader_overrides = report
            .initialized_shader_overrides
            .checked_add(1)
            .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                resource: "initialized Shader override records",
            })?;
    }
    Ok(())
}

fn apply_original_path_postprocessing(
    record: &mut WorkspaceAssetExportPath,
    path_to_set: Option<String>,
    named_asset_name: Option<&str>,
    is_shader: bool,
    report: &mut WorkspaceAssetExportPathReport,
) -> Result<(), WorkspaceAssetBundlePathError> {
    if let Some(path) = path_to_set {
        let details = original_path_details(&path)?;
        record.original_path = Some(path);
        record.original_directory = details.directory;
        record.original_name = details.name;
        record.original_extension = details.extension;
    }
    let mut path_details_changed = false;
    if let Some(asset_name) = named_asset_name {
        path_details_changed = restore_original_name(record, asset_name, report)?;
    }
    if is_shader {
        initialize_shader_overrides(record, report)?;
    }
    if path_details_changed {
        record.original_path = calculated_original_path(record)?;
    }
    Ok(())
}

pub(super) fn bundle_container_original_path(
    container_path: &str,
    bundle_name: &str,
    mode: BundledAssetsExportMode,
) -> Result<Option<BundleOriginalPathEffect>, WorkspaceAssetBundlePathError> {
    let mut asset_path = ensure_path_not_rooted(container_path)?;
    if asset_path.is_empty() {
        return Ok(None);
    }
    match mode {
        BundledAssetsExportMode::GroupByAssetType => Ok(Some(BundleOriginalPathEffect {
            original_path: None,
        })),
        BundledAssetsExportMode::DirectExport => {
            ensure_starts_with_assets(&asset_path).map(|original_path| {
                Some(BundleOriginalPathEffect {
                    original_path: Some(original_path),
                })
            })
        }
        BundledAssetsExportMode::GroupByBundleName => {
            if let Some(relative_path) = strip_prefix_ignore_ascii_case(&asset_path, "Assets/") {
                asset_path = try_copy_export_path_string(relative_path)?;
            }
            let mut bundle_prefix = try_copy_export_path_string(bundle_name)?;
            bundle_prefix.try_reserve(1).map_err(|_| {
                WorkspaceAssetBundlePathError::AllocationFailed {
                    resource: "AssetBundle path prefix",
                    requested: bundle_prefix.len().saturating_add(1),
                }
            })?;
            bundle_prefix.push('/');
            if let Some(relative_path) = strip_prefix_ignore_ascii_case(&asset_path, &bundle_prefix)
            {
                asset_path = try_copy_export_path_string(relative_path)?;
            }
            join_project_path(&["Assets/AssetBundles", bundle_name, &asset_path]).map(
                |original_path| {
                    Some(BundleOriginalPathEffect {
                        original_path: Some(original_path),
                    })
                },
            )
        }
    }
}

fn ensure_path_not_rooted(path: &str) -> Result<String, WorkspaceAssetBundlePathError> {
    if !is_path_rooted_for_host(path) {
        return try_copy_export_path_string(path);
    }
    let mut start = 0usize;
    for segment in path.split('/') {
        if segment.eq_ignore_ascii_case("Assets") {
            return try_copy_export_path_string(&path[start..]);
        }
        start = start
            .checked_add(segment.len())
            .and_then(|value| value.checked_add(1))
            .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                resource: "AssetBundle path segment offset",
            })?;
    }
    Ok(String::new())
}

fn is_path_rooted_for_host(path: &str) -> bool {
    if std::path::Path::new(path).is_absolute() {
        return true;
    }
    if cfg!(windows) {
        let bytes = path.as_bytes();
        path.starts_with('/')
            || path.starts_with('\\')
            || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
    } else {
        false
    }
}

pub(super) fn ensure_starts_with_assets(
    path: &str,
) -> Result<String, WorkspaceAssetBundlePathError> {
    if path.starts_with("Assets/") {
        return try_copy_export_path_string(path);
    }
    if let Some(suffix) = strip_prefix_ignore_ascii_case(path, "Assets/") {
        let mut result = try_copy_export_path_string("Assets/")?;
        result.try_reserve(suffix.len()).map_err(|_| {
            WorkspaceAssetBundlePathError::AllocationFailed {
                resource: "direct AssetBundle path",
                requested: result.len().saturating_add(suffix.len()),
            }
        })?;
        result.push_str(suffix);
        return Ok(result);
    }
    let mut result = try_copy_export_path_string("Assets/")?;
    result.try_reserve(path.len()).map_err(|_| {
        WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "direct AssetBundle path",
            requested: result.len().saturating_add(path.len()),
        }
    })?;
    result.push_str(path);
    Ok(result)
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

fn join_project_path(parts: &[&str]) -> Result<String, WorkspaceAssetBundlePathError> {
    let separator = std::path::MAIN_SEPARATOR;
    let mut length = 0usize;
    let mut has_content = false;
    let mut previous_ended_with_separator = false;
    for part in parts.iter().copied().filter(|part| !part.is_empty()) {
        if has_content && !previous_ended_with_separator {
            length = length
                .checked_add(separator.len_utf8())
                .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
        }
        length = length
            .checked_add(part.len())
            .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
        has_content = true;
        previous_ended_with_separator = part.ends_with('/') || part.ends_with('\\');
    }
    let mut result = String::new();
    result.try_reserve_exact(length).map_err(|_| {
        WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "joined AssetBundle path",
            requested: length,
        }
    })?;
    has_content = false;
    previous_ended_with_separator = false;
    for part in parts.iter().copied().filter(|part| !part.is_empty()) {
        if has_content && !previous_ended_with_separator {
            result.push(separator);
        }
        result.push_str(part);
        has_content = true;
        previous_ended_with_separator = part.ends_with('/') || part.ends_with('\\');
    }
    Ok(result)
}

/// Apply the AssetBundle portion of `OriginalPathProcessor` to export-owned path metadata.
///
/// The input name plan must be complete. Bundle Container assignments run in source order, then
/// GroupByBundleName collection directories are filled with the baseline's `??=` behavior.
pub fn apply_workspace_asset_bundle_paths(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    collections: &WorkspaceAssetCollections,
    bundle_plan: &WorkspaceAssetBundleNamePlan,
    mode: BundledAssetsExportMode,
    export_paths: WorkspaceAssetExportPathTable,
    limits: WorkspaceAssetExportPathLimits,
) -> Result<
    (
        WorkspaceAssetExportPathTable,
        WorkspaceAssetExportPathReport,
    ),
    WorkspaceAssetBundlePathError,
> {
    apply_workspace_original_paths(
        workspace,
        objects,
        collections,
        WorkspaceOriginalPathPlans {
            asset_bundles: bundle_plan,
            resource_managers: &WorkspaceResourceManagerPathPlan::default(),
        },
        mode,
        export_paths,
        limits,
    )
}
