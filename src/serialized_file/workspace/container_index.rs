use super::*;

#[derive(Clone, Copy)]
pub(super) struct ContainerFileOrder<'a> {
    pub(super) scope: Option<&'a str>,
    pub(super) entry_index: usize,
    pub(super) collection_index: Option<usize>,
}

pub(super) fn build_mounted_file_order_index<'a>(
    mounts: impl IntoIterator<Item = (&'a [String], &'a [MountedSerializedFile])>,
    record_limit: usize,
    report: SerializedFileWorkspaceReport,
) -> Result<HashMap<&'a str, ContainerFileOrder<'a>>, Box<SerializedFileWorkspaceError>> {
    let mut order_index = HashMap::new();
    let mut record_count = 0usize;
    for (container_scopes, serialized_files) in mounts {
        for serialized_file in serialized_files {
            record_count = record_count.checked_add(1).ok_or_else(|| {
                workspace_error(
                    report,
                    Some(&serialized_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::MountedFileOrderCountOverflow,
                )
            })?;
            if record_count > record_limit {
                return Err(workspace_error(
                    report,
                    Some(&serialized_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::MountedFileOrderCountLimitExceeded {
                        count: record_count,
                        limit: record_limit,
                    },
                ));
            }
            let scope = match serialized_file.container_scope_index {
                Some(scope_index) => Some(
                    container_scopes
                        .get(scope_index)
                        .ok_or_else(|| {
                            workspace_error(
                                report,
                                Some(&serialized_file.virtual_path),
                                SerializedFileWorkspaceErrorKind::MountedFileScopeIndexOutOfBounds {
                                    index: scope_index,
                                    count: container_scopes.len(),
                                },
                            )
                        })?
                        .as_str(),
                ),
                None => None,
            };
            if serialized_file.container_bundle_index.is_some()
                != serialized_file.collection_index.is_some()
                || (scope.is_some() && serialized_file.container_bundle_index.is_none())
            {
                return Err(workspace_error(
                    report,
                    Some(&serialized_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::InvalidMountedFileCollectionOrder,
                ));
            }
            order_index.try_reserve(1).map_err(|_| {
                workspace_error(
                    report,
                    Some(&serialized_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "mounted SerializedFile order index",
                        requested: record_count,
                    },
                )
            })?;
            if order_index
                .insert(
                    serialized_file.virtual_path.as_str(),
                    ContainerFileOrder {
                        scope,
                        entry_index: serialized_file.entry_index,
                        collection_index: serialized_file.collection_index,
                    },
                )
                .is_some()
            {
                return Err(workspace_error(
                    report,
                    Some(&serialized_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::DuplicateMountedFileOrderPath,
                ));
            }
        }
    }
    Ok(order_index)
}

pub(super) struct MountedContainerBundleInput<'a> {
    pub(super) container_scopes: &'a [String],
    pub(super) mounted_bundles: &'a [MountedContainerBundle],
    pub(super) serialized_files: &'a [MountedSerializedFile],
    pub(super) resource_files: &'a [MountedResourceFile],
    pub(super) failed_entries: &'a [FailedContainerEntry],
    pub(super) source_container_path: &'a str,
    pub(super) loaded_paths: &'a HashMap<String, usize>,
    pub(super) limits: SerializedFileWorkspaceLimits,
    pub(super) bundle_count_limit: usize,
}

pub(super) fn append_mounted_container_bundles(
    workspace: &mut SerializedFileWorkspace,
    input: MountedContainerBundleInput<'_>,
    total_node_path_bytes: &mut usize,
    total_resource_file_path_bytes: &mut usize,
) -> Result<(), Box<SerializedFileWorkspaceError>> {
    let MountedContainerBundleInput {
        container_scopes,
        mounted_bundles,
        serialized_files,
        resource_files,
        failed_entries,
        source_container_path,
        loaded_paths,
        limits,
        bundle_count_limit,
    } = input;
    if mounted_bundles.is_empty() {
        if serialized_files
            .iter()
            .any(|file| file.container_bundle_index.is_some())
            || !resource_files.is_empty()
            || !failed_entries.is_empty()
        {
            return Err(workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                    bundle_index: 0,
                },
            ));
        }
        return Ok(());
    }

    let base_index = workspace.container_bundles.len();
    let requested_bundle_count =
        base_index
            .checked_add(mounted_bundles.len())
            .ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    None,
                    SerializedFileWorkspaceErrorKind::ContainerBundleCountOverflow,
                )
            })?;
    if requested_bundle_count > bundle_count_limit {
        return Err(workspace_error(
            workspace.report,
            None,
            SerializedFileWorkspaceErrorKind::ContainerBundleCountLimitExceeded {
                count: requested_bundle_count,
                limit: bundle_count_limit,
            },
        ));
    }
    workspace
        .container_bundles
        .try_reserve(mounted_bundles.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "workspace serialized bundle nodes",
                    requested: requested_bundle_count,
                },
            )
        })?;
    let requested_resource_file_count = workspace
        .resource_files
        .len()
        .checked_add(resource_files.len())
        .ok_or_else(|| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::ResourceFileCountOverflow,
            )
        })?;
    workspace
        .resource_files
        .try_reserve(resource_files.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "workspace mounted ResourceFiles",
                    requested: requested_resource_file_count,
                },
            )
        })?;

    let mut bundle_by_scope = Vec::new();
    bundle_by_scope
        .try_reserve_exact(container_scopes.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "mounted container scope-to-bundle map",
                    requested: container_scopes.len(),
                },
            )
        })?;
    bundle_by_scope.resize(container_scopes.len(), None::<usize>);
    for (bundle_index, bundle) in mounted_bundles.iter().enumerate() {
        if bundle.parent_bundle_index.is_some() != bundle.parent_entry_index.is_some()
            || bundle
                .parent_bundle_index
                .is_some_and(|parent| parent >= bundle_index)
        {
            return Err(workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy { bundle_index },
            ));
        }
        if let Some(scope_index) = bundle.scope_index {
            let Some(scope_bundle_index) = bundle_by_scope.get_mut(scope_index) else {
                return Err(workspace_error(
                    workspace.report,
                    None,
                    SerializedFileWorkspaceErrorKind::MountedFileScopeIndexOutOfBounds {
                        index: scope_index,
                        count: container_scopes.len(),
                    },
                ));
            };
            if scope_bundle_index.replace(bundle_index).is_some() {
                return Err(workspace_error(
                    workspace.report,
                    container_scopes.get(scope_index).map(String::as_str),
                    SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                        bundle_index,
                    },
                ));
            }
        }
    }

    let mut collection_files_by_bundle = Vec::new();
    collection_files_by_bundle
        .try_reserve_exact(mounted_bundles.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "direct collection mappings by bundle",
                    requested: mounted_bundles.len(),
                },
            )
        })?;
    collection_files_by_bundle.resize_with(mounted_bundles.len(), Vec::new);
    for mounted_file in serialized_files {
        let (Some(bundle_index), Some(collection_index)) = (
            mounted_file.container_bundle_index,
            mounted_file.collection_index,
        ) else {
            if mounted_file.container_bundle_index.is_some()
                || mounted_file.container_scope_index.is_some()
                || mounted_file.collection_index.is_some()
            {
                return Err(workspace_error(
                    workspace.report,
                    Some(&mounted_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::InvalidMountedFileCollectionOrder,
                ));
            }
            continue;
        };
        let Some(bundle) = mounted_bundles.get(bundle_index) else {
            return Err(workspace_error(
                workspace.report,
                Some(&mounted_file.virtual_path),
                SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy { bundle_index },
            ));
        };
        if bundle.scope_index != mounted_file.container_scope_index {
            return Err(workspace_error(
                workspace.report,
                Some(&mounted_file.virtual_path),
                SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy { bundle_index },
            ));
        }
        let Some(workspace_file_index) = loaded_paths.get(&mounted_file.virtual_path).copied()
        else {
            return Err(workspace_error(
                workspace.report,
                Some(&mounted_file.virtual_path),
                SerializedFileWorkspaceErrorKind::MountedContainerCollectionFileMissing {
                    virtual_path: mounted_file.virtual_path.clone(),
                },
            ));
        };
        let pairs = &mut collection_files_by_bundle[bundle_index];
        let requested = pairs.len().checked_add(1).ok_or_else(|| {
            workspace_error(
                workspace.report,
                Some(&mounted_file.virtual_path),
                SerializedFileWorkspaceErrorKind::MountedFileOrderCountOverflow,
            )
        })?;
        pairs.try_reserve(1).map_err(|_| {
            workspace_error(
                workspace.report,
                Some(&mounted_file.virtual_path),
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "bundle direct collection file mappings",
                    requested,
                },
            )
        })?;
        pairs.push((collection_index, workspace_file_index));
    }

    let mut resource_files_by_bundle = Vec::new();
    resource_files_by_bundle
        .try_reserve_exact(mounted_bundles.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "direct ResourceFile mappings by bundle",
                    requested: mounted_bundles.len(),
                },
            )
        })?;
    resource_files_by_bundle.resize_with(mounted_bundles.len(), Vec::new);

    let mut ordered_resource_files = Vec::new();
    ordered_resource_files
        .try_reserve_exact(resource_files.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "mounted ResourceFile order records",
                    requested: resource_files.len(),
                },
            )
        })?;
    for resource_file in resource_files {
        let bundle_index = resource_file.container_bundle_index;
        let Some(bundle) = mounted_bundles.get(bundle_index) else {
            return Err(workspace_error(
                workspace.report,
                Some(&resource_file.virtual_path),
                SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy { bundle_index },
            ));
        };
        if bundle.scope_index != resource_file.container_scope_index {
            return Err(workspace_error(
                workspace.report,
                Some(&resource_file.virtual_path),
                SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy { bundle_index },
            ));
        }
        ordered_resource_files.push((bundle_index, resource_file.resource_index, resource_file));
    }
    ordered_resource_files
        .sort_unstable_by_key(|(bundle_index, resource_index, _)| (*bundle_index, *resource_index));

    let mut seen_resource_paths = HashSet::<&str>::new();
    let seen_resource_path_count = workspace
        .resource_files
        .len()
        .checked_add(resource_files.len())
        .ok_or_else(|| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::ResourceFileCountOverflow,
            )
        })?;
    seen_resource_paths
        .try_reserve(seen_resource_path_count)
        .map_err(|_| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "mounted ResourceFile path validation",
                    requested: seen_resource_path_count,
                },
            )
        })?;
    for resource_file in &workspace.resource_files {
        seen_resource_paths.insert(resource_file.virtual_path.as_str());
    }
    let mut resource_file_count_by_bundle = Vec::new();
    resource_file_count_by_bundle
        .try_reserve_exact(mounted_bundles.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "mounted ResourceFile order counters",
                    requested: mounted_bundles.len(),
                },
            )
        })?;
    resource_file_count_by_bundle.resize(mounted_bundles.len(), 0usize);
    let mut new_resource_files = Vec::new();
    new_resource_files
        .try_reserve_exact(resource_files.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "workspace ResourceFile records",
                    requested: resource_files.len(),
                },
            )
        })?;
    let mut next_resource_path_bytes = *total_resource_file_path_bytes;
    let mut next_resource_name_bytes = workspace.report.bundle_resource_file_name_bytes;
    for (bundle_index, actual_index, resource_file) in ordered_resource_files {
        let expected_index = resource_file_count_by_bundle[bundle_index];
        if actual_index != expected_index {
            return Err(workspace_error(
                workspace.report,
                Some(&resource_file.virtual_path),
                SerializedFileWorkspaceErrorKind::InvalidContainerResourceSequence {
                    expected: expected_index,
                    actual: actual_index,
                },
            ));
        }
        resource_file_count_by_bundle[bundle_index] =
            expected_index.checked_add(1).ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    Some(&resource_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::ResourceFileCountOverflow,
                )
            })?;
        if !seen_resource_paths.insert(&resource_file.virtual_path) {
            return Err(workspace_error(
                workspace.report,
                Some(&resource_file.virtual_path),
                SerializedFileWorkspaceErrorKind::DuplicateMountedResourceFilePath,
            ));
        }
        next_resource_path_bytes = next_resource_path_bytes
            .checked_add(resource_file.virtual_path.len())
            .ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    Some(&resource_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::BundleResourcePathByteCountOverflow,
                )
            })?;
        if next_resource_path_bytes > limits.max_total_bundle_resource_file_path_bytes {
            return Err(workspace_error(
                workspace.report,
                Some(&resource_file.virtual_path),
                SerializedFileWorkspaceErrorKind::TotalBundleResourceFilePathBytesLimitExceeded {
                    byte_count: next_resource_path_bytes,
                    limit: limits.max_total_bundle_resource_file_path_bytes,
                },
            ));
        }
        next_resource_name_bytes = next_resource_name_bytes
            .checked_add(resource_file.name.len())
            .ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    Some(&resource_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::BundleResourceNameByteCountOverflow,
                )
            })?;
        if next_resource_name_bytes > limits.max_total_bundle_resource_file_name_bytes {
            return Err(workspace_error(
                workspace.report,
                Some(&resource_file.virtual_path),
                SerializedFileWorkspaceErrorKind::TotalBundleResourceFileNameBytesLimitExceeded {
                    byte_count: next_resource_name_bytes,
                    limit: limits.max_total_bundle_resource_file_name_bytes,
                },
            ));
        }
        let mut owned_path = String::new();
        owned_path
            .try_reserve_exact(resource_file.virtual_path.len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    Some(&resource_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "workspace ResourceFile path",
                        requested: resource_file.virtual_path.len(),
                    },
                )
            })?;
        owned_path.push_str(&resource_file.virtual_path);
        let mut owned_name = String::new();
        owned_name
            .try_reserve_exact(resource_file.name.len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    Some(&resource_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "workspace ResourceFile source name",
                        requested: resource_file.name.len(),
                    },
                )
            })?;
        owned_name.push_str(&resource_file.name);
        let owned_name_fixed = fix_file_identifier(&owned_name);
        let global_bundle_index = base_index.checked_add(bundle_index).ok_or_else(|| {
            workspace_error(
                workspace.report,
                Some(&resource_file.virtual_path),
                SerializedFileWorkspaceErrorKind::ContainerBundleCountOverflow,
            )
        })?;
        let resource_workspace_index = workspace
            .resource_files
            .len()
            .checked_add(new_resource_files.len())
            .ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    Some(&resource_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::ResourceFileCountOverflow,
                )
            })?;
        new_resource_files.push(WorkspaceResourceFile {
            virtual_path: owned_path,
            name: owned_name,
            name_fixed: owned_name_fixed,
            container_bundle_index: global_bundle_index,
            container_entry_index: resource_file.entry_index,
            container_resource_index: resource_file.resource_index,
        });
        let direct_resource_indices = &mut resource_files_by_bundle[bundle_index];
        let requested_direct_resource_count = direct_resource_indices
            .len()
            .checked_add(1)
            .ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    Some(&resource_file.virtual_path),
                    SerializedFileWorkspaceErrorKind::ResourceFileCountOverflow,
                )
            })?;
        direct_resource_indices.try_reserve(1).map_err(|_| {
            workspace_error(
                workspace.report,
                Some(&resource_file.virtual_path),
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "bundle direct ResourceFile indexes",
                    requested: requested_direct_resource_count,
                },
            )
        })?;
        direct_resource_indices.push(resource_workspace_index);
    }
    drop(seen_resource_paths);

    let mut new_bundles = Vec::new();
    new_bundles
        .try_reserve_exact(mounted_bundles.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "workspace serialized bundle nodes",
                    requested: requested_bundle_count,
                },
            )
        })?;
    let mut next_node_path_bytes = *total_node_path_bytes;
    let mut next_bundle_name_bytes = workspace.report.bundle_name_bytes;
    let requested_failed_count = workspace
        .failed_container_entries
        .len()
        .checked_add(failed_entries.len())
        .ok_or_else(|| {
            workspace_error(
                workspace.report,
                Some(source_container_path),
                SerializedFileWorkspaceErrorKind::FailedContainerEntryCountLimitExceeded {
                    count: usize::MAX,
                    limit: limits.max_total_failed_container_entries,
                },
            )
        })?;
    if requested_failed_count > limits.max_total_failed_container_entries {
        return Err(workspace_error(
            workspace.report,
            Some(source_container_path),
            SerializedFileWorkspaceErrorKind::FailedContainerEntryCountLimitExceeded {
                count: requested_failed_count,
                limit: limits.max_total_failed_container_entries,
            },
        ));
    }
    let mut next_failed_metadata_bytes = workspace.failed_container_metadata_bytes;
    let mut new_failed_entries = Vec::new();
    new_failed_entries
        .try_reserve_exact(failed_entries.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                Some(source_container_path),
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "workspace failed container entry records",
                    requested: requested_failed_count,
                },
            )
        })?;
    for failed in failed_entries {
        let Some(local_bundle_index) = failed.container_bundle_index else {
            return Err(workspace_error(
                workspace.report,
                Some(&failed.virtual_path),
                SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                    bundle_index: 0,
                },
            ));
        };
        if local_bundle_index >= mounted_bundles.len() {
            return Err(workspace_error(
                workspace.report,
                Some(&failed.virtual_path),
                SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                    bundle_index: local_bundle_index,
                },
            ));
        }
        let record_bytes = source_container_path
            .len()
            .checked_add(failed.virtual_path.len())
            .and_then(|count| count.checked_add(failed.source_name.len()))
            .ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    Some(&failed.virtual_path),
                    SerializedFileWorkspaceErrorKind::TotalFailedContainerMetadataLimitExceeded {
                        byte_count: usize::MAX,
                        limit: limits.max_total_failed_container_metadata_bytes,
                    },
                )
            })?;
        next_failed_metadata_bytes = next_failed_metadata_bytes
            .checked_add(record_bytes)
            .ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    Some(&failed.virtual_path),
                    SerializedFileWorkspaceErrorKind::TotalFailedContainerMetadataLimitExceeded {
                        byte_count: usize::MAX,
                        limit: limits.max_total_failed_container_metadata_bytes,
                    },
                )
            })?;
        if next_failed_metadata_bytes > limits.max_total_failed_container_metadata_bytes {
            return Err(workspace_error(
                workspace.report,
                Some(&failed.virtual_path),
                SerializedFileWorkspaceErrorKind::TotalFailedContainerMetadataLimitExceeded {
                    byte_count: next_failed_metadata_bytes,
                    limit: limits.max_total_failed_container_metadata_bytes,
                },
            ));
        }
        let mut owned_source_path = String::new();
        owned_source_path
            .try_reserve_exact(source_container_path.len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    Some(&failed.virtual_path),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "failed container source paths",
                        requested: source_container_path.len(),
                    },
                )
            })?;
        owned_source_path.push_str(source_container_path);
        let mut owned_virtual_path = String::new();
        owned_virtual_path
            .try_reserve_exact(failed.virtual_path.len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    Some(&failed.virtual_path),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "failed container virtual paths",
                        requested: failed.virtual_path.len(),
                    },
                )
            })?;
        owned_virtual_path.push_str(&failed.virtual_path);
        let mut owned_source_name = String::new();
        owned_source_name
            .try_reserve_exact(failed.source_name.len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    Some(&failed.virtual_path),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "failed container source names",
                        requested: failed.source_name.len(),
                    },
                )
            })?;
        owned_source_name.push_str(&failed.source_name);
        let global_bundle_index = base_index.checked_add(local_bundle_index).ok_or_else(|| {
            workspace_error(
                workspace.report,
                Some(&failed.virtual_path),
                SerializedFileWorkspaceErrorKind::ContainerBundleCountOverflow,
            )
        })?;
        new_failed_entries.push(WorkspaceFailedContainerEntry {
            source_container_path: owned_source_path,
            entry: FailedContainerEntry {
                virtual_path: owned_virtual_path,
                source_name: owned_source_name,
                container_bundle_index: Some(global_bundle_index),
                entry_index: failed.entry_index,
                error: failed.error.clone(),
            },
        });
    }
    for (bundle_index, bundle) in mounted_bundles.iter().enumerate() {
        let scope_path = bundle
            .scope_index
            .map(|scope_index| {
                container_scopes.get(scope_index).ok_or_else(|| {
                    workspace_error(
                        workspace.report,
                        None,
                        SerializedFileWorkspaceErrorKind::MountedFileScopeIndexOutOfBounds {
                            index: scope_index,
                            count: container_scopes.len(),
                        },
                    )
                })
            })
            .transpose()?;
        let owned_scope_path = if let Some(scope_path) = scope_path {
            next_node_path_bytes = next_node_path_bytes
                .checked_add(scope_path.len())
                .ok_or_else(|| {
                    workspace_error(
                        workspace.report,
                        Some(scope_path),
                        SerializedFileWorkspaceErrorKind::BundleNodePathByteCountOverflow,
                    )
                })?;
            if next_node_path_bytes > limits.max_total_bundle_node_path_bytes {
                return Err(workspace_error(
                    workspace.report,
                    Some(scope_path),
                    SerializedFileWorkspaceErrorKind::TotalBundleNodePathBytesLimitExceeded {
                        byte_count: next_node_path_bytes,
                        limit: limits.max_total_bundle_node_path_bytes,
                    },
                ));
            }
            let mut owned_scope_path = String::new();
            owned_scope_path
                .try_reserve_exact(scope_path.len())
                .map_err(|_| {
                    workspace_error(
                        workspace.report,
                        Some(scope_path),
                        SerializedFileWorkspaceErrorKind::AllocationFailed {
                            resource: "workspace serialized bundle scope path",
                            requested: scope_path.len(),
                        },
                    )
                })?;
            owned_scope_path.push_str(scope_path);
            Some(owned_scope_path)
        } else {
            None
        };

        let pairs = &mut collection_files_by_bundle[bundle_index];
        pairs.sort_unstable_by_key(|(collection_index, _)| *collection_index);
        for (expected_index, (actual_index, file_index)) in pairs.iter().enumerate() {
            if *actual_index != expected_index {
                let path = workspace
                    .files
                    .get(*file_index)
                    .map(|file| file.virtual_path.as_str());
                return Err(workspace_error(
                    workspace.report,
                    path,
                    SerializedFileWorkspaceErrorKind::InvalidContainerCollectionSequence {
                        expected: expected_index,
                        actual: *actual_index,
                    },
                ));
            }
        }
        let mut direct_collection_file_indices = Vec::new();
        direct_collection_file_indices
            .try_reserve_exact(pairs.len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    owned_scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "bundle direct collection file indexes",
                        requested: pairs.len(),
                    },
                )
            })?;
        direct_collection_file_indices.extend(pairs.iter().map(|(_, file_index)| *file_index));
        let mut direct_resource_file_indices = Vec::new();
        direct_resource_file_indices
            .try_reserve_exact(resource_files_by_bundle[bundle_index].len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    owned_scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "bundle direct ResourceFile indexes",
                        requested: resource_files_by_bundle[bundle_index].len(),
                    },
                )
            })?;
        direct_resource_file_indices.extend_from_slice(&resource_files_by_bundle[bundle_index]);
        let parent_bundle_index = match bundle.parent_bundle_index {
            Some(parent) => Some(base_index.checked_add(parent).ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    owned_scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::ContainerBundleCountOverflow,
                )
            })?),
            None => None,
        };
        let owned_name_fixed = if let Some(name_fixed) = bundle.name_fixed.as_deref() {
            next_bundle_name_bytes = next_bundle_name_bytes
                .checked_add(name_fixed.len())
                .ok_or_else(|| {
                    workspace_error(
                        workspace.report,
                        owned_scope_path.as_deref(),
                        SerializedFileWorkspaceErrorKind::BundleNameByteCountOverflow,
                    )
                })?;
            if next_bundle_name_bytes > limits.max_total_bundle_name_bytes {
                return Err(workspace_error(
                    workspace.report,
                    owned_scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::TotalBundleNameBytesLimitExceeded {
                        byte_count: next_bundle_name_bytes,
                        limit: limits.max_total_bundle_name_bytes,
                    },
                ));
            }
            let mut owned_name = String::new();
            owned_name
                .try_reserve_exact(name_fixed.len())
                .map_err(|_| {
                    workspace_error(
                        workspace.report,
                        owned_scope_path.as_deref(),
                        SerializedFileWorkspaceErrorKind::AllocationFailed {
                            resource: "workspace fixed Bundle names",
                            requested: name_fixed.len(),
                        },
                    )
                })?;
            owned_name.push_str(name_fixed);
            Some(owned_name)
        } else {
            None
        };
        new_bundles.push(WorkspaceSerializedBundle {
            scope_path: owned_scope_path,
            name_fixed: owned_name_fixed,
            parent_bundle_index,
            parent_entry_index: bundle.parent_entry_index,
            direct_collection_file_indices,
            direct_resource_file_indices,
        });
    }
    for (local_bundle_index, bundle) in new_bundles.iter().enumerate() {
        let global_bundle_index = base_index.checked_add(local_bundle_index).ok_or_else(|| {
            workspace_error(
                workspace.report,
                bundle.scope_path.as_deref(),
                SerializedFileWorkspaceErrorKind::ContainerBundleCountOverflow,
            )
        })?;
        for &file_index in &bundle.direct_collection_file_indices {
            let Some(file) = workspace.files.get_mut(file_index) else {
                return Err(workspace_error(
                    workspace.report,
                    bundle.scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                        bundle_index: local_bundle_index,
                    },
                ));
            };
            if file
                .container_bundle_index
                .replace(global_bundle_index)
                .is_some()
            {
                return Err(workspace_error(
                    workspace.report,
                    Some(&file.virtual_path),
                    SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                        bundle_index: local_bundle_index,
                    },
                ));
            }
        }
    }
    workspace
        .failed_container_entries
        .try_reserve(new_failed_entries.len())
        .map_err(|_| {
            workspace_error(
                workspace.report,
                Some(source_container_path),
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "workspace failed container entry records",
                    requested: requested_failed_count,
                },
            )
        })?;
    workspace.container_bundles.extend(new_bundles);
    workspace.resource_files.extend(new_resource_files);
    workspace
        .failed_container_entries
        .extend(new_failed_entries);
    workspace.failed_container_metadata_bytes = next_failed_metadata_bytes;
    *total_node_path_bytes = next_node_path_bytes;
    *total_resource_file_path_bytes = next_resource_path_bytes;
    workspace.report.bundle_resource_file_name_bytes = next_resource_name_bytes;
    workspace.report.bundle_name_bytes = next_bundle_name_bytes;
    Ok(())
}

pub(super) fn attach_container_file_order(
    file: &mut LoadedSerializedFile,
    order_index: &HashMap<&str, ContainerFileOrder<'_>>,
    report: SerializedFileWorkspaceReport,
) -> Result<(), Box<SerializedFileWorkspaceError>> {
    let Some(order) = order_index.get(file.virtual_path.as_str()) else {
        return Ok(());
    };
    if order.scope != file.container_scope.as_deref() {
        return Err(workspace_error(
            report,
            Some(&file.virtual_path),
            SerializedFileWorkspaceErrorKind::MountedFileScopeMismatch,
        ));
    }
    file.container_entry_index = Some(order.entry_index);
    file.container_collection_index = order.collection_index;
    Ok(())
}

pub(super) fn build_container_scope_index<'a>(
    scope_paths: impl IntoIterator<Item = &'a String>,
    scope_count: usize,
    scope_count_limit: usize,
    report: SerializedFileWorkspaceReport,
    input_path: Option<&str>,
) -> Result<HashSet<&'a str>, Box<SerializedFileWorkspaceError>> {
    if scope_count > scope_count_limit {
        return Err(workspace_error(
            report,
            input_path,
            SerializedFileWorkspaceErrorKind::ContainerScopeCountLimitExceeded {
                count: scope_count,
                limit: scope_count_limit,
            },
        ));
    }
    let mut scope_index = HashSet::new();
    scope_index.try_reserve(scope_count).map_err(|_| {
        workspace_error(
            report,
            input_path,
            SerializedFileWorkspaceErrorKind::AllocationFailed {
                resource: "container scope identity index",
                requested: scope_count,
            },
        )
    })?;
    for scope_path in scope_paths {
        if !scope_index.insert(scope_path.as_str()) {
            return Err(workspace_error(
                report,
                Some(scope_path),
                SerializedFileWorkspaceErrorKind::DuplicateContainerScopePath,
            ));
        }
    }
    Ok(scope_index)
}

pub(super) fn deepest_container_scope_for_path<'a>(
    virtual_path: &str,
    scope_index: &HashSet<&'a str>,
) -> Option<&'a str> {
    let mut search_end = virtual_path.len();
    loop {
        let separator = virtual_path[..search_end].rfind('/')?;
        let candidate_end = if separator == 0 { 1 } else { separator };
        let candidate = &virtual_path[..candidate_end];
        if let Some(scope) = scope_index.get(candidate) {
            return Some(*scope);
        }
        if separator == 0 {
            return None;
        }
        search_end = separator;
    }
}

pub(super) fn attach_container_scope(
    file: &mut LoadedSerializedFile,
    scope_index: &HashSet<&str>,
    total_scope_bytes: &mut usize,
    limits: SerializedFileWorkspaceLimits,
    report: SerializedFileWorkspaceReport,
) -> Result<(), Box<SerializedFileWorkspaceError>> {
    let Some(scope_path) = deepest_container_scope_for_path(&file.virtual_path, scope_index) else {
        return Ok(());
    };
    let requested_bytes = total_scope_bytes
        .checked_add(scope_path.len())
        .ok_or_else(|| {
            workspace_error(
                report,
                Some(&file.virtual_path),
                SerializedFileWorkspaceErrorKind::BundleScopeByteCountOverflow,
            )
        })?;
    if requested_bytes > limits.max_total_bundle_scope_bytes {
        return Err(workspace_error(
            report,
            Some(&file.virtual_path),
            SerializedFileWorkspaceErrorKind::TotalBundleScopeBytesLimitExceeded {
                byte_count: requested_bytes,
                limit: limits.max_total_bundle_scope_bytes,
            },
        ));
    }
    let mut owned_scope = String::new();
    owned_scope
        .try_reserve_exact(scope_path.len())
        .map_err(|_| {
            workspace_error(
                report,
                Some(&file.virtual_path),
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "loaded SerializedFile container scope",
                    requested: scope_path.len(),
                },
            )
        })?;
    owned_scope.push_str(scope_path);
    file.container_scope = Some(owned_scope);
    *total_scope_bytes = requested_bytes;
    Ok(())
}

pub(super) fn remaining_workspace_limits(
    limits: SerializedFileWorkspaceLimits,
    report: SerializedFileWorkspaceReport,
    loaded_file_count: usize,
) -> SerializedFileWorkspaceLimits {
    SerializedFileWorkspaceLimits {
        max_files: limits.max_files.saturating_sub(loaded_file_count),
        max_total_input_bytes: limits
            .max_total_input_bytes
            .saturating_sub(report.total_input_bytes),
        max_total_file_name_bytes: limits
            .max_total_file_name_bytes
            .saturating_sub(report.total_file_name_bytes),
        max_total_objects: limits
            .max_total_objects
            .saturating_sub(report.total_objects),
        max_total_reference_edges: limits
            .max_total_reference_edges
            .saturating_sub(report.total_reference_edges),
        max_total_bundle_name_bytes: limits
            .max_total_bundle_name_bytes
            .saturating_sub(report.bundle_name_bytes),
        max_total_bundle_resource_file_name_bytes: limits
            .max_total_bundle_resource_file_name_bytes
            .saturating_sub(report.bundle_resource_file_name_bytes),
        ..limits
    }
}

pub(super) fn append_workspace(
    workspace: &mut SerializedFileWorkspace,
    mut additional: SerializedFileWorkspace,
) -> Result<(), SerializedFileWorkspaceErrorKind> {
    let requested_file_count = workspace
        .files
        .len()
        .checked_add(additional.files.len())
        .ok_or(SerializedFileWorkspaceErrorKind::FileCountOverflow)?;
    workspace
        .files
        .try_reserve_exact(additional.files.len())
        .map_err(|_| SerializedFileWorkspaceErrorKind::AllocationFailed {
            resource: "dependency SerializedFiles",
            requested: requested_file_count,
        })?;
    let input_files = workspace
        .report
        .input_files
        .checked_add(additional.report.input_files)
        .ok_or(SerializedFileWorkspaceErrorKind::FileCountOverflow)?;
    let completed_files = workspace
        .report
        .completed_files
        .checked_add(additional.report.completed_files)
        .ok_or(SerializedFileWorkspaceErrorKind::FileCountOverflow)?;
    let total_input_bytes = workspace
        .report
        .total_input_bytes
        .checked_add(additional.report.total_input_bytes)
        .ok_or(SerializedFileWorkspaceErrorKind::InputByteCountOverflow)?;
    let completed_input_bytes = workspace
        .report
        .completed_input_bytes
        .checked_add(additional.report.completed_input_bytes)
        .ok_or(SerializedFileWorkspaceErrorKind::InputByteCountOverflow)?;
    let total_file_name_bytes = workspace
        .report
        .total_file_name_bytes
        .checked_add(additional.report.total_file_name_bytes)
        .ok_or(SerializedFileWorkspaceErrorKind::FileNameByteCountOverflow)?;
    let total_objects = workspace
        .report
        .total_objects
        .checked_add(additional.report.total_objects)
        .ok_or(SerializedFileWorkspaceErrorKind::ObjectCountOverflow)?;
    let total_reference_edges = workspace
        .report
        .total_reference_edges
        .checked_add(additional.report.total_reference_edges)
        .ok_or(SerializedFileWorkspaceErrorKind::ReferenceCountOverflow)?;
    let bundle_resource_file_name_bytes = workspace
        .report
        .bundle_resource_file_name_bytes
        .checked_add(additional.report.bundle_resource_file_name_bytes)
        .ok_or(SerializedFileWorkspaceErrorKind::BundleResourceNameByteCountOverflow)?;
    let bundle_name_bytes = workspace
        .report
        .bundle_name_bytes
        .checked_add(additional.report.bundle_name_bytes)
        .ok_or(SerializedFileWorkspaceErrorKind::BundleNameByteCountOverflow)?;
    workspace.report = SerializedFileWorkspaceReport {
        input_files,
        completed_files,
        total_input_bytes,
        completed_input_bytes,
        total_file_name_bytes,
        total_objects,
        total_reference_edges,
        bundle_resource_file_name_bytes,
        bundle_name_bytes,
    };
    workspace.files.append(&mut additional.files);
    Ok(())
}

pub(super) fn load_dependency_serialized_file(
    filesystem: &VirtualFileSystem,
    input: &DependencyFilePath,
    limits: SerializedFileWorkspaceLimits,
    workspace: &mut SerializedFileWorkspace,
    loaded_names: &mut HashMap<String, (usize, usize)>,
    loaded_paths: &mut HashMap<String, usize>,
    layouts: &dyn crate::processing::asset_factory::ClassLayoutSource,
) -> Result<usize, Box<SerializedFileWorkspaceError>> {
    let requested_file_count = workspace.files.len().checked_add(1).ok_or_else(|| {
        workspace_error(
            workspace.report,
            Some(&input.virtual_path),
            SerializedFileWorkspaceErrorKind::FileCountOverflow,
        )
    })?;
    loaded_names.try_reserve(1).map_err(|_| {
        workspace_error(
            workspace.report,
            Some(&input.virtual_path),
            SerializedFileWorkspaceErrorKind::AllocationFailed {
                resource: "loaded dependency identities",
                requested: requested_file_count,
            },
        )
    })?;
    loaded_paths.try_reserve(1).map_err(|_| {
        workspace_error(
            workspace.report,
            Some(&input.virtual_path),
            SerializedFileWorkspaceErrorKind::AllocationFailed {
                resource: "loaded dependency paths",
                requested: requested_file_count,
            },
        )
    })?;

    let additional = SerializedFileWorkspace::load_from_vfs_inner(
        filesystem,
        std::slice::from_ref(input),
        remaining_workspace_limits(limits, workspace.report, workspace.files.len()),
        layouts,
    )
    .map_err(|mut error| {
        error.report = workspace.report;
        error
    })?;
    let file_index = workspace.files.len();
    append_workspace(workspace, additional)
        .map_err(|kind| workspace_error(workspace.report, Some(&input.virtual_path), kind))?;
    let loaded_file = &workspace.files[file_index];
    let loaded_name = fix_file_identifier(&loaded_file.file_name);
    loaded_names
        .entry(loaded_name)
        .and_modify(|(_, count)| *count += 1)
        .or_insert((file_index, 1));
    loaded_paths.insert(loaded_file.virtual_path.clone(), file_index);
    Ok(file_index)
}

pub(super) fn dispatch_vfs_file(
    filesystem: &VirtualFileSystem,
    virtual_path: &str,
    limits: SchemeReaderLimits,
    report: SerializedFileWorkspaceReport,
) -> Result<DispatchedFile<'static>, Box<SerializedFileWorkspaceError>> {
    let mut stream = filesystem.open_file(virtual_path).map_err(|source| {
        workspace_error(
            report,
            Some(virtual_path),
            SerializedFileWorkspaceErrorKind::FileSystem(source),
        )
    })?;
    SchemeReader::read_stream(&mut stream, limits).map_err(|source| {
        workspace_error(
            report,
            Some(virtual_path),
            SerializedFileWorkspaceErrorKind::Scheme(source),
        )
    })
}

pub(super) fn new_dependency_expansion_root(
    filesystem: &mut VirtualFileSystem,
    next_namespace: &mut usize,
) -> io::Result<String> {
    loop {
        let namespace = *next_namespace;
        *next_namespace = namespace.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "dependency expansion namespace count overflows",
            )
        })?;
        let root = format!("/__assetripper_dependency_mount_{namespace}");
        if filesystem.file_exists(Some(&root))? || filesystem.directory_exists(Some(&root))? {
            continue;
        }
        filesystem.create_directory(Some(&root))?;
        return Ok(root);
    }
}

pub(super) fn container_kind(kind: FileKind) -> bool {
    matches!(
        kind,
        FileKind::UnityFs
            | FileKind::UnityRaw
            | FileKind::UnityWebBundle
            | FileKind::UnityArchive
            | FileKind::UnityWebData
    )
}

pub(super) fn relative_path_under<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    path.strip_prefix(root)?.strip_prefix('/')
}
