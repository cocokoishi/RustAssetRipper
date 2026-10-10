use std::borrow::Cow;
use std::io::{Seek, SeekFrom};
use std::path::Path;

use super::reader::probe_direct_stream_kind;
use super::*;
use crate::container::unityfs::{UnityFsError, UnityFsStreamBundle};
use crate::io::filesystem::VirtualFileSystem;
use crate::io::special_names::fix_file_identifier;
use crate::io::stream::SmartStream;
struct ContainerTreeMountState<'a> {
    filesystem: &'a mut VirtualFileSystem,
    limits: ContainerMountLimits,
    visited_entries: usize,
    active_nested_bytes: u64,
    container_scope_path_bytes: u64,
    container_bundle_name_bytes: u64,
    bundle_serialized_file_counts: Vec<usize>,
    bundle_resource_file_counts: Vec<usize>,
    resource_file_path_bytes: u64,
    resource_file_name_bytes: u64,
    report: ContainerMountReport,
}

fn record_container_scope(
    virtual_path: Option<&str>,
    state: &mut ContainerTreeMountState<'_>,
) -> Result<Option<usize>, ContainerMountError> {
    let Some(virtual_path) = virtual_path else {
        return Ok(None);
    };
    let path_bytes = u64::try_from(virtual_path.len()).map_err(|_| {
        ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ContainerScopePathSizeOverflow,
        )
    })?;
    let requested_bytes = state
        .container_scope_path_bytes
        .checked_add(path_bytes)
        .ok_or_else(|| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerScopePathSizeOverflow,
            )
        })?;
    if requested_bytes > state.limits.max_total_container_scope_path_bytes {
        return Err(ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ContainerScopePathLimitExceeded {
                requested: requested_bytes,
                limit: state.limits.max_total_container_scope_path_bytes,
            },
        ));
    }
    let requested_scopes = state
        .report
        .container_scopes
        .len()
        .checked_add(1)
        .ok_or_else(|| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerScopeCountOverflow,
            )
        })?;
    state.report.container_scopes.try_reserve(1).map_err(|_| {
        ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ContainerScopeAllocationFailed {
                requested: requested_scopes,
            },
        )
    })?;
    let mut owned_path = String::new();
    owned_path
        .try_reserve_exact(virtual_path.len())
        .map_err(|_| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerScopeAllocationFailed {
                    requested: virtual_path.len(),
                },
            )
        })?;
    owned_path.push_str(virtual_path);
    let scope_index = state.report.container_scopes.len();
    state.report.container_scopes.push(owned_path);
    state.container_scope_path_bytes = requested_bytes;
    state.report.container_scope_path_bytes = requested_bytes;
    Ok(Some(scope_index))
}

fn record_container_bundle(
    scope_index: Option<usize>,
    parent_bundle_index: Option<usize>,
    parent_entry_index: Option<usize>,
    source_name: Option<&str>,
    state: &mut ContainerTreeMountState<'_>,
) -> Result<usize, ContainerMountError> {
    if parent_bundle_index.is_some() != parent_entry_index.is_some() {
        return Err(ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ContainerBundleParentMetadataMismatch,
        ));
    }
    if let Some(parent_index) = parent_bundle_index {
        if parent_index >= state.report.container_bundles.len() {
            return Err(ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerBundleParentIndexOutOfBounds {
                    index: parent_index,
                    count: state.report.container_bundles.len(),
                },
            ));
        }
    }
    if let Some(scope_index) = scope_index {
        if scope_index >= state.report.container_scopes.len() {
            return Err(ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerScopeIndexOutOfBounds {
                    index: scope_index,
                    count: state.report.container_scopes.len(),
                },
            ));
        }
    }
    let name_fixed = if let Some(source_name) = source_name {
        let source_name_bytes = u64::try_from(source_name.len()).map_err(|_| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerBundleNameSizeOverflow,
            )
        })?;
        if source_name_bytes > state.limits.max_total_container_bundle_name_bytes {
            return Err(ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerBundleNameLimitExceeded {
                    requested: source_name_bytes,
                    limit: state.limits.max_total_container_bundle_name_bytes,
                },
            ));
        }
        let fixed_name = fix_file_identifier(source_name);
        let fixed_name_bytes = u64::try_from(fixed_name.len()).map_err(|_| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerBundleNameSizeOverflow,
            )
        })?;
        let requested_name_bytes = state
            .container_bundle_name_bytes
            .checked_add(fixed_name_bytes)
            .ok_or_else(|| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::ContainerBundleNameSizeOverflow,
                )
            })?;
        if requested_name_bytes > state.limits.max_total_container_bundle_name_bytes {
            return Err(ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerBundleNameLimitExceeded {
                    requested: requested_name_bytes,
                    limit: state.limits.max_total_container_bundle_name_bytes,
                },
            ));
        }
        Some((fixed_name, requested_name_bytes))
    } else {
        None
    };
    let requested_count = state
        .report
        .container_bundles
        .len()
        .checked_add(1)
        .ok_or_else(|| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerBundleCountOverflow,
            )
        })?;
    state.report.container_bundles.try_reserve(1).map_err(|_| {
        ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ContainerBundleAllocationFailed {
                requested: requested_count,
            },
        )
    })?;
    let requested_collection_counters = state
        .bundle_serialized_file_counts
        .len()
        .checked_add(1)
        .ok_or_else(|| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerBundleCountOverflow,
            )
        })?;
    state
        .bundle_serialized_file_counts
        .try_reserve(1)
        .map_err(|_| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerBundleCollectionIndexAllocationFailed {
                    requested: requested_collection_counters,
                },
            )
        })?;
    let requested_resource_counters = state
        .bundle_resource_file_counts
        .len()
        .checked_add(1)
        .ok_or_else(|| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerBundleCountOverflow,
            )
        })?;
    state
        .bundle_resource_file_counts
        .try_reserve(1)
        .map_err(|_| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ContainerBundleResourceIndexAllocationFailed {
                    requested: requested_resource_counters,
                },
            )
        })?;
    let bundle_index = state.report.container_bundles.len();
    let (name_fixed, next_bundle_name_bytes) = match name_fixed {
        Some((name, byte_count)) => (Some(name), byte_count),
        None => (None, state.container_bundle_name_bytes),
    };
    state.report.container_bundles.push(MountedContainerBundle {
        scope_index,
        parent_bundle_index,
        parent_entry_index,
        name_fixed,
    });
    state.container_bundle_name_bytes = next_bundle_name_bytes;
    state.report.container_bundle_name_bytes = next_bundle_name_bytes;
    state.bundle_serialized_file_counts.push(0);
    state.bundle_resource_file_counts.push(0);
    Ok(bundle_index)
}

pub(super) fn mount_unityfs_stream_tree(
    source: &SmartStream,
    source_name: Option<&str>,
    filesystem: &mut VirtualFileSystem,
    destination_root: Option<&str>,
    scheme_limits: SchemeReaderLimits,
    limits: ContainerMountLimits,
) -> Result<ContainerMountReport, ContainerMountError> {
    let mut state = ContainerTreeMountState {
        filesystem,
        limits,
        visited_entries: 0,
        active_nested_bytes: 0,
        container_scope_path_bytes: 0,
        container_bundle_name_bytes: 0,
        bundle_serialized_file_counts: Vec::new(),
        bundle_resource_file_counts: Vec::new(),
        resource_file_path_bytes: 0,
        resource_file_name_bytes: 0,
        report: ContainerMountReport::default(),
    };
    mount_unityfs_stream_tree_at(
        source,
        UnityFsStreamContext {
            destination_root,
            depth: 0,
            parent_bundle_index: None,
            parent_entry_index: None,
            source_name,
        },
        scheme_limits,
        &mut state,
    )?;
    Ok(state.report)
}

fn mount_unityfs_stream_tree_at(
    source: &SmartStream,
    context: UnityFsStreamContext<'_>,
    scheme_limits: SchemeReaderLimits,
    state: &mut ContainerTreeMountState<'_>,
) -> Result<(), ContainerMountError> {
    let UnityFsStreamContext {
        destination_root,
        depth,
        parent_bundle_index,
        parent_entry_index,
        source_name,
    } = context;
    let container_scope_index = record_container_scope(destination_root, state)?;
    let container_bundle_index = record_container_bundle(
        container_scope_index,
        parent_bundle_index,
        parent_entry_index,
        source_name,
        state,
    )?;
    if depth > state.limits.max_depth {
        return Err(ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::NestingDepthLimitExceeded {
                depth,
                limit: state.limits.max_depth,
            },
        ));
    }
    let bundle = UnityFsStreamBundle::parse(
        source,
        scheme_limits.unity_fs,
        scheme_limits.max_input_bytes,
    )
    .map_err(|error| {
        ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::Scheme(SchemeError::new(SchemeErrorKind::UnityFs(error))),
        )
    })?;
    preflight_tree_entry_count(bundle.nodes.len(), state)?;
    for (index, node) in bundle.nodes.iter().enumerate() {
        let path = checked_container_entry_path(
            state.filesystem,
            destination_root,
            node.path.as_str(),
            index,
            &state.report,
        )?;
        let (mut entry_stream, source_backed) = match bundle.read_entry_with_source_backing(index) {
            Ok(stream) => stream,
            Err(error) => {
                record_failed_container_entry(
                    &mut state.report,
                    node.path.as_str(),
                    path,
                    Some(container_bundle_index),
                    index,
                    error,
                    state.limits,
                )?;
                continue;
            }
        };
        let mut child_kind =
            probe_direct_stream_kind(&mut entry_stream, scheme_limits).map_err(|error| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::Scheme(error),
                )
            })?;
        if child_kind.is_none() {
            // The reference container reader unwraps one compression layer, then keeps its
            // decoded bytes as a ResourceFile without dispatching a nested container or asset.
            entry_stream = SchemeReader::unwrap_one_wrapper_stream(
                entry_stream.create_reference(),
                scheme_limits,
            )
            .map_err(|error| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::Scheme(error),
                )
            })?
            .ok_or_else(|| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::Scheme(SchemeError::new(
                        SchemeErrorKind::NotContainer,
                    )),
                )
            })?;
            child_kind = Some(FileKind::Resource);
        }
        if child_kind == Some(FileKind::UnityFs) {
            let child_depth = depth.checked_add(1).ok_or_else(|| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::NestingDepthLimitExceeded {
                        depth: usize::MAX,
                        limit: state.limits.max_depth,
                    },
                )
            })?;
            let child_size = entry_stream.length().map_err(|source| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::FileSystem {
                        entry_index: index,
                        path: path.clone(),
                        source,
                    },
                )
            })?;
            let child_owned_bytes = if source_backed { 0 } else { child_size };
            let requested_active_bytes = state
                .active_nested_bytes
                .checked_add(child_owned_bytes)
                .ok_or_else(|| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::ActiveNestedContainerLimitExceeded {
                        entry_index: index,
                        requested_total: u64::MAX,
                        limit: state.limits.max_active_nested_bytes,
                    },
                )
            })?;
            if requested_active_bytes > state.limits.max_active_nested_bytes {
                return Err(ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::ActiveNestedContainerLimitExceeded {
                        entry_index: index,
                        requested_total: requested_active_bytes,
                        limit: state.limits.max_active_nested_bytes,
                    },
                ));
            }
            state.active_nested_bytes = requested_active_bytes;
            let result = mount_unityfs_stream_tree_at(
                &entry_stream,
                UnityFsStreamContext {
                    destination_root: Some(&path),
                    depth: child_depth,
                    parent_bundle_index: Some(container_bundle_index),
                    parent_entry_index: Some(index),
                    source_name: Some(node.path.as_str()),
                },
                scheme_limits,
                state,
            );
            state.active_nested_bytes -= child_owned_bytes;
            result?;
            continue;
        }

        if matches!(
            child_kind,
            Some(FileKind::SerializedFile | FileKind::Resource)
        ) {
            let serialized_file = if child_kind == Some(FileKind::SerializedFile) {
                Some(prepare_mounted_serialized_file(
                    &path,
                    index,
                    container_scope_index,
                    container_bundle_index,
                    state,
                )?)
            } else {
                None
            };
            let resource_file = if child_kind == Some(FileKind::Resource) {
                Some(prepare_mounted_resource_file(
                    &path,
                    node.path.as_str(),
                    index,
                    container_scope_index,
                    container_bundle_index,
                    state,
                )?)
            } else {
                None
            };
            mount_container_stream_at_path(
                state.filesystem,
                &path,
                index,
                entry_stream,
                state.limits,
                &mut state.report,
            )?;
            if let Some(serialized_file) = serialized_file {
                state.report.serialized_file_path_bytes = serialized_file.total_path_bytes;
                state.report.serialized_files.push(serialized_file.record);
                state.bundle_serialized_file_counts[container_bundle_index] =
                    serialized_file.next_bundle_collection_count;
            }
            if let Some(resource_file) = resource_file {
                state.resource_file_path_bytes = resource_file.total_path_bytes;
                state.report.resource_file_path_bytes = resource_file.total_path_bytes;
                state.resource_file_name_bytes = resource_file.total_name_bytes;
                state.report.resource_file_name_bytes = resource_file.total_name_bytes;
                state.report.resource_files.push(resource_file.record);
                state.bundle_resource_file_counts[container_bundle_index] =
                    resource_file.next_bundle_resource_count;
            }
            continue;
        }

        let entry_bytes = entry_stream
            .to_vec(scheme_limits.max_input_bytes)
            .map_err(|error| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::Scheme(SchemeError::new(
                        SchemeErrorKind::StreamInput(error),
                    )),
                )
            })?;
        let child = SchemeReader::read_cow_with_limits(Cow::Owned(entry_bytes), scheme_limits)
            .map_err(|error| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::Scheme(error),
                )
            })?;
        mount_tree_child(
            child,
            ContainerTreeEntry {
                virtual_path: path,
                source_name: node.path.as_str(),
                entry_index: index,
            },
            container_scope_index,
            container_bundle_index,
            depth,
            state,
        )?;
    }
    Ok(())
}

struct UnityFsStreamContext<'a> {
    destination_root: Option<&'a str>,
    depth: usize,
    parent_bundle_index: Option<usize>,
    parent_entry_index: Option<usize>,
    source_name: Option<&'a str>,
}

fn mount_container_stream_at_path(
    filesystem: &mut VirtualFileSystem,
    path: &str,
    entry_index: usize,
    mut stream: SmartStream,
    limits: ContainerMountLimits,
    report: &mut ContainerMountReport,
) -> Result<(), ContainerMountError> {
    let entry_size = stream.length().map_err(|source| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FileSystem {
                entry_index,
                path: path.to_owned(),
                source,
            },
        )
    })?;
    let new_total = report
        .mounted_bytes
        .checked_add(entry_size)
        .ok_or_else(|| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::TotalOutputSizeOverflow { entry_index },
            )
        })?;
    if new_total > limits.max_total_output_bytes {
        return Err(ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::TotalOutputLimitExceeded {
                entry_index,
                requested_total: new_total,
                limit: limits.max_total_output_bytes,
            },
        ));
    }
    stream.seek(SeekFrom::Start(0)).map_err(|source| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FileSystem {
                entry_index,
                path: path.to_owned(),
                source,
            },
        )
    })?;
    filesystem
        .insert_file_stream_with_parents(path, stream)
        .map_err(|source| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::FileSystem {
                    entry_index,
                    path: path.to_owned(),
                    source,
                },
            )
        })?;
    report.mounted_entries += 1;
    report.mounted_bytes = new_total;
    Ok(())
}

struct ContainerTreeEntry<'a> {
    virtual_path: String,
    source_name: &'a str,
    entry_index: usize,
}

fn mount_tree_child<'a, 'b>(
    child: DispatchedFile<'a>,
    entry: ContainerTreeEntry<'b>,
    container_scope_index: Option<usize>,
    container_bundle_index: usize,
    parent_depth: usize,
    state: &mut ContainerTreeMountState<'_>,
) -> Result<(), ContainerMountError> {
    let ContainerTreeEntry {
        virtual_path: entry_path,
        source_name: resource_name,
        entry_index,
    } = entry;
    if is_container_kind(child.kind) {
        let depth = parent_depth.checked_add(1).ok_or_else(|| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::NestingDepthLimitExceeded {
                    depth: usize::MAX,
                    limit: state.limits.max_depth,
                },
            )
        })?;
        let child_owned_bytes = match &child.contents {
            Cow::Owned(bytes) => u64::try_from(bytes.len()).map_err(|_| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::TotalOutputSizeOverflow { entry_index },
                )
            })?,
            Cow::Borrowed(_) => 0,
        };
        let requested_active_bytes = state
            .active_nested_bytes
            .checked_add(child_owned_bytes)
            .ok_or_else(|| {
                ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::ActiveNestedContainerLimitExceeded {
                        entry_index,
                        requested_total: u64::MAX,
                        limit: state.limits.max_active_nested_bytes,
                    },
                )
            })?;
        if requested_active_bytes > state.limits.max_active_nested_bytes {
            return Err(ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ActiveNestedContainerLimitExceeded {
                    entry_index,
                    requested_total: requested_active_bytes,
                    limit: state.limits.max_active_nested_bytes,
                },
            ));
        }
        state.active_nested_bytes = requested_active_bytes;
        let result = child.mount_container_tree_at(
            Some(&entry_path),
            depth,
            Some(container_bundle_index),
            Some(entry_index),
            Some(resource_name),
            state,
        );
        state.active_nested_bytes -= child_owned_bytes;
        result
    } else {
        let serialized_file = if child.kind == FileKind::SerializedFile {
            Some(prepare_mounted_serialized_file(
                &entry_path,
                entry_index,
                container_scope_index,
                container_bundle_index,
                state,
            )?)
        } else {
            None
        };
        let resource_file = if child.kind == FileKind::Resource {
            Some(prepare_mounted_resource_file(
                &entry_path,
                resource_name,
                entry_index,
                container_scope_index,
                container_bundle_index,
                state,
            )?)
        } else {
            None
        };
        mount_container_entry_at_path(
            state.filesystem,
            &entry_path,
            entry_index,
            child.contents,
            state.limits,
            &mut state.report,
        )?;
        if let Some(serialized_file) = serialized_file {
            state.report.serialized_file_path_bytes = serialized_file.total_path_bytes;
            state.report.serialized_files.push(serialized_file.record);
            state.bundle_serialized_file_counts[container_bundle_index] =
                serialized_file.next_bundle_collection_count;
        }
        if let Some(resource_file) = resource_file {
            state.resource_file_path_bytes = resource_file.total_path_bytes;
            state.report.resource_file_path_bytes = resource_file.total_path_bytes;
            state.resource_file_name_bytes = resource_file.total_name_bytes;
            state.report.resource_file_name_bytes = resource_file.total_name_bytes;
            state.report.resource_files.push(resource_file.record);
            state.bundle_resource_file_counts[container_bundle_index] =
                resource_file.next_bundle_resource_count;
        }
        Ok(())
    }
}

struct PendingMountedSerializedFile {
    record: MountedSerializedFile,
    total_path_bytes: u64,
    next_bundle_collection_count: usize,
}

fn prepare_mounted_serialized_file(
    virtual_path: &str,
    entry_index: usize,
    container_scope_index: Option<usize>,
    container_bundle_index: usize,
    state: &mut ContainerTreeMountState<'_>,
) -> Result<PendingMountedSerializedFile, ContainerMountError> {
    let limits = state.limits;
    let report = &mut state.report;
    let path_bytes = u64::try_from(virtual_path.len()).map_err(|_| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::SerializedFilePathSizeOverflow,
        )
    })?;
    let total_path_bytes = report
        .serialized_file_path_bytes
        .checked_add(path_bytes)
        .ok_or_else(|| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::SerializedFilePathSizeOverflow,
            )
        })?;
    if total_path_bytes > limits.max_total_serialized_file_path_bytes {
        return Err(ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::SerializedFilePathLimitExceeded {
                requested: total_path_bytes,
                limit: limits.max_total_serialized_file_path_bytes,
            },
        ));
    }
    let requested_count = report
        .serialized_files
        .len()
        .checked_add(1)
        .ok_or_else(|| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::SerializedFileEntryCountOverflow,
            )
        })?;
    report.serialized_files.try_reserve(1).map_err(|_| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::SerializedFileEntryAllocationFailed {
                requested: requested_count,
            },
        )
    })?;
    let mut owned_path = String::new();
    owned_path
        .try_reserve_exact(virtual_path.len())
        .map_err(|_| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::SerializedFilePathAllocationFailed {
                    requested: virtual_path.len(),
                },
            )
        })?;
    owned_path.push_str(virtual_path);
    let Some(collection_index) = state
        .bundle_serialized_file_counts
        .get(container_bundle_index)
        .copied()
    else {
        return Err(ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ContainerBundleParentIndexOutOfBounds {
                index: container_bundle_index,
                count: state.bundle_serialized_file_counts.len(),
            },
        ));
    };
    let next_bundle_collection_count = collection_index.checked_add(1).ok_or_else(|| {
        ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ContainerCollectionIndexOverflow,
        )
    })?;
    Ok(PendingMountedSerializedFile {
        record: MountedSerializedFile {
            virtual_path: owned_path,
            container_scope_index,
            container_bundle_index: Some(container_bundle_index),
            entry_index,
            collection_index: Some(collection_index),
        },
        total_path_bytes,
        next_bundle_collection_count,
    })
}

struct PendingMountedResourceFile {
    record: MountedResourceFile,
    total_path_bytes: u64,
    total_name_bytes: u64,
    next_bundle_resource_count: usize,
}

fn prepare_mounted_resource_file(
    virtual_path: &str,
    resource_name: &str,
    entry_index: usize,
    container_scope_index: Option<usize>,
    container_bundle_index: usize,
    state: &mut ContainerTreeMountState<'_>,
) -> Result<PendingMountedResourceFile, ContainerMountError> {
    let limits = state.limits;
    let path_bytes = u64::try_from(virtual_path.len()).map_err(|_| {
        ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ResourceFilePathSizeOverflow,
        )
    })?;
    let total_path_bytes = state
        .resource_file_path_bytes
        .checked_add(path_bytes)
        .ok_or_else(|| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ResourceFilePathSizeOverflow,
            )
        })?;
    if total_path_bytes > limits.max_total_resource_file_path_bytes {
        return Err(ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ResourceFilePathLimitExceeded {
                requested: total_path_bytes,
                limit: limits.max_total_resource_file_path_bytes,
            },
        ));
    }
    let requested_count = state
        .report
        .resource_files
        .len()
        .checked_add(1)
        .ok_or_else(|| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ResourceFileEntryCountOverflow,
            )
        })?;
    state.report.resource_files.try_reserve(1).map_err(|_| {
        ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ResourceFileEntryAllocationFailed {
                requested: requested_count,
            },
        )
    })?;
    let mut owned_path = String::new();
    owned_path
        .try_reserve_exact(virtual_path.len())
        .map_err(|_| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ResourceFilePathAllocationFailed {
                    requested: virtual_path.len(),
                },
            )
        })?;
    owned_path.push_str(virtual_path);
    let name_bytes = u64::try_from(resource_name.len()).map_err(|_| {
        ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ResourceFileNameSizeOverflow,
        )
    })?;
    let total_name_bytes = state
        .resource_file_name_bytes
        .checked_add(name_bytes)
        .ok_or_else(|| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ResourceFileNameSizeOverflow,
            )
        })?;
    if total_name_bytes > limits.max_total_resource_file_name_bytes {
        return Err(ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ResourceFileNameLimitExceeded {
                requested: total_name_bytes,
                limit: limits.max_total_resource_file_name_bytes,
            },
        ));
    }
    let mut owned_name = String::new();
    owned_name
        .try_reserve_exact(resource_name.len())
        .map_err(|_| {
            ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::ResourceFileNameAllocationFailed {
                    requested: resource_name.len(),
                },
            )
        })?;
    owned_name.push_str(resource_name);
    let Some(resource_index) = state
        .bundle_resource_file_counts
        .get(container_bundle_index)
        .copied()
    else {
        return Err(ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ContainerBundleParentIndexOutOfBounds {
                index: container_bundle_index,
                count: state.bundle_resource_file_counts.len(),
            },
        ));
    };
    let next_bundle_resource_count = resource_index.checked_add(1).ok_or_else(|| {
        ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::ContainerResourceIndexOverflow,
        )
    })?;
    Ok(PendingMountedResourceFile {
        record: MountedResourceFile {
            virtual_path: owned_path,
            name: owned_name,
            container_scope_index,
            container_bundle_index,
            entry_index,
            resource_index,
        },
        total_path_bytes,
        total_name_bytes,
        next_bundle_resource_count,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContainerEntryPathError {
    Invalid,
    TooLong,
    AllocationFailed { requested: usize },
}

pub(super) fn directory_input_destination(
    destination_root: Option<&str>,
    relative_path: &Path,
) -> Result<Option<String>, ()> {
    let relative_parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    directory_relative_destination(destination_root, relative_parent)
}

pub(super) fn directory_relative_destination(
    destination_root: Option<&str>,
    relative_path: &Path,
) -> Result<Option<String>, ()> {
    let mut components = Vec::new();
    for component in relative_path.components() {
        match component {
            std::path::Component::Normal(name) => components.push(name.to_str().ok_or(())?),
            _ => return Err(()),
        }
    }
    let relative_parent = components.join("/");
    if relative_parent.is_empty() {
        return Ok(destination_root.map(str::to_owned));
    }

    let Some(root) = destination_root.filter(|root| !root.is_empty()) else {
        return Ok(Some(relative_parent));
    };
    if root
        .chars()
        .all(|character| matches!(character, '/' | '\\'))
    {
        return Ok(Some(format!("/{relative_parent}")));
    }
    Ok(Some(format!(
        "{}/{relative_parent}",
        root.trim_end_matches(['/', '\\'])
    )))
}

fn container_entry_path(
    destination_root: Option<&str>,
    entry_name: &str,
    maximum_path_bytes: usize,
) -> Result<String, ContainerEntryPathError> {
    let root = destination_root
        .unwrap_or("")
        .trim_matches(|character| character == '/' || character == '\\');
    let entry = entry_name.trim_matches(|character| character == '/' || character == '\\');
    if entry.is_empty()
        || entry
            .split(['/', '\\'])
            .any(|component| matches!(component, "." | ".."))
        || root
            .split(['/', '\\'])
            .any(|component| matches!(component, "." | ".."))
    {
        return Err(ContainerEntryPathError::Invalid);
    }
    let separators = if root.is_empty() { 1 } else { 2 };
    let path_size = root
        .len()
        .checked_add(entry.len())
        .and_then(|size| size.checked_add(separators))
        .ok_or(ContainerEntryPathError::TooLong)?;
    if path_size > maximum_path_bytes {
        return Err(ContainerEntryPathError::TooLong);
    }
    let mut path = String::new();
    path.try_reserve_exact(path_size)
        .map_err(|_| ContainerEntryPathError::AllocationFailed {
            requested: path_size,
        })?;
    path.push('/');
    if !root.is_empty() {
        path.push_str(root);
        path.push('/');
    }
    path.push_str(entry);
    Ok(path)
}

/// Parsed file contents. Views borrow from the owning [`DispatchedFile`].
impl<'a> DispatchedFile<'a> {
    pub fn mount_container_entries(
        &self,
        filesystem: &mut VirtualFileSystem,
        destination_root: Option<&str>,
        limits: ContainerMountLimits,
    ) -> Result<ContainerMountReport, ContainerMountError> {
        let mut report = ContainerMountReport::default();
        let parsed = self.parse().map_err(|error| {
            ContainerMountError::new(report.clone(), ContainerMountErrorKind::Scheme(error))
        })?;

        match parsed {
            ParsedFile::UnityFs(bundle) => {
                preflight_entry_count(bundle.nodes.len(), limits, &report)?;
                preflight_output_sizes(bundle.nodes.iter().map(|node| node.size), limits, &report)?;
                for (index, node) in bundle.nodes.iter().enumerate() {
                    let path = checked_container_entry_path(
                        filesystem,
                        destination_root,
                        node.path.as_str(),
                        index,
                        &report,
                    )?;
                    let bytes = match bundle.read_entry(index) {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            record_failed_container_entry(
                                &mut report,
                                node.path.as_str(),
                                path,
                                None,
                                index,
                                error,
                                limits,
                            )?;
                            continue;
                        }
                    };
                    mount_container_entry_at_path(
                        filesystem,
                        &path,
                        index,
                        bytes,
                        limits,
                        &mut report,
                    )?;
                }
            }
            ParsedFile::RawWeb(bundle) => {
                preflight_entry_count(bundle.entries.len(), limits, &report)?;
                preflight_output_sizes(
                    bundle.entries.iter().map(|entry| u64::from(entry.size)),
                    limits,
                    &report,
                )?;
                for (index, entry) in bundle.entries.iter().enumerate() {
                    let bytes = bundle.read_entry(index).map_err(|error| {
                        ContainerMountError::new(
                            report.clone(),
                            ContainerMountErrorKind::Scheme(SchemeError::new(
                                SchemeErrorKind::RawWeb(error),
                            )),
                        )
                    })?;
                    mount_container_entry(
                        filesystem,
                        destination_root,
                        entry.path.as_str(),
                        index,
                        Cow::Borrowed(bytes),
                        limits,
                        &mut report,
                    )?;
                }
            }
            ParsedFile::WebData(web_file) => {
                preflight_entry_count(web_file.entries.len(), limits, &report)?;
                preflight_output_sizes(
                    web_file.entries.iter().map(|entry| u64::from(entry.size)),
                    limits,
                    &report,
                )?;
                for (index, entry) in web_file.entries.iter().enumerate() {
                    let bytes = web_file.entry_data(index).ok_or_else(|| {
                        ContainerMountError::new(
                            report.clone(),
                            ContainerMountErrorKind::Scheme(SchemeError::new(
                                SchemeErrorKind::WebDataEntryIndexOutOfBounds {
                                    index,
                                    entry_count: web_file.entries.len(),
                                },
                            )),
                        )
                    })?;
                    mount_container_entry(
                        filesystem,
                        destination_root,
                        entry.name.as_str(),
                        index,
                        Cow::Borrowed(bytes),
                        limits,
                        &mut report,
                    )?;
                }
            }
            ParsedFile::SerializedFile(_) | ParsedFile::Resource(_) => {
                return Err(ContainerMountError::new(
                    report,
                    ContainerMountErrorKind::NotContainer,
                ));
            }
        }

        Ok(report)
    }

    /// Recursively mount leaf entries from nested containers and wrapper files.
    ///
    /// Recognized nested containers become virtual directories named after their parent entry;
    /// their contained leaves are mounted below that path. Container and wrapper payloads are
    /// parsed in memory and are not inserted as leaf files. Entry count, nesting depth, and total
    /// mounted bytes are bounded across the complete traversal. UnityFS block-read failures are
    /// retained as source-style failed-entry records while later siblings continue.
    pub fn mount_container_tree(
        &self,
        filesystem: &mut VirtualFileSystem,
        destination_root: Option<&str>,
        limits: ContainerMountLimits,
    ) -> Result<ContainerMountReport, ContainerMountError> {
        self.mount_container_tree_with_source_name(filesystem, destination_root, None, limits)
    }

    /// Mount a container while retaining the source FileBase name used to create its Bundle.
    pub fn mount_container_tree_with_source_name(
        &self,
        filesystem: &mut VirtualFileSystem,
        destination_root: Option<&str>,
        source_name: Option<&str>,
        limits: ContainerMountLimits,
    ) -> Result<ContainerMountReport, ContainerMountError> {
        let mut state = ContainerTreeMountState {
            filesystem,
            limits,
            visited_entries: 0,
            active_nested_bytes: 0,
            container_scope_path_bytes: 0,
            container_bundle_name_bytes: 0,
            bundle_serialized_file_counts: Vec::new(),
            bundle_resource_file_counts: Vec::new(),
            resource_file_path_bytes: 0,
            resource_file_name_bytes: 0,
            report: ContainerMountReport::default(),
        };
        self.mount_container_tree_at(destination_root, 0, None, None, source_name, &mut state)?;
        Ok(state.report)
    }

    fn mount_container_tree_at(
        &self,
        destination_root: Option<&str>,
        depth: usize,
        parent_bundle_index: Option<usize>,
        parent_entry_index: Option<usize>,
        source_name: Option<&str>,
        state: &mut ContainerTreeMountState<'_>,
    ) -> Result<(), ContainerMountError> {
        let container_scope_index = record_container_scope(destination_root, state)?;
        let container_bundle_index = record_container_bundle(
            container_scope_index,
            parent_bundle_index,
            parent_entry_index,
            source_name,
            state,
        )?;
        if depth > state.limits.max_depth {
            return Err(ContainerMountError::new(
                state.report.clone(),
                ContainerMountErrorKind::NestingDepthLimitExceeded {
                    depth,
                    limit: state.limits.max_depth,
                },
            ));
        }

        let parsed = self.parse().map_err(|error| {
            ContainerMountError::new(state.report.clone(), ContainerMountErrorKind::Scheme(error))
        })?;
        match parsed {
            ParsedFile::UnityFs(bundle) => {
                preflight_tree_entry_count(bundle.nodes.len(), state)?;
                for (index, node) in bundle.nodes.iter().enumerate() {
                    let path = checked_container_entry_path(
                        state.filesystem,
                        destination_root,
                        node.path.as_str(),
                        index,
                        &state.report,
                    )?;
                    let bytes = match bundle.read_entry(index) {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            record_failed_container_entry(
                                &mut state.report,
                                node.path.as_str(),
                                path,
                                Some(container_bundle_index),
                                index,
                                error,
                                state.limits,
                            )?;
                            continue;
                        }
                    };
                    let child = SchemeReader::read_cow_with_limits(bytes, self.limits).map_err(
                        |error| {
                            ContainerMountError::new(
                                state.report.clone(),
                                ContainerMountErrorKind::Scheme(error),
                            )
                        },
                    )?;
                    mount_tree_child(
                        child,
                        ContainerTreeEntry {
                            virtual_path: path,
                            source_name: node.path.as_str(),
                            entry_index: index,
                        },
                        container_scope_index,
                        container_bundle_index,
                        depth,
                        state,
                    )?;
                }
            }
            ParsedFile::RawWeb(bundle) => {
                preflight_tree_entry_count(bundle.entries.len(), state)?;
                for (index, entry) in bundle.entries.iter().enumerate() {
                    let path = checked_container_entry_path(
                        state.filesystem,
                        destination_root,
                        entry.path.as_str(),
                        index,
                        &state.report,
                    )?;
                    let bytes = bundle.read_entry(index).map_err(|error| {
                        ContainerMountError::new(
                            state.report.clone(),
                            ContainerMountErrorKind::Scheme(SchemeError::new(
                                SchemeErrorKind::RawWeb(error),
                            )),
                        )
                    })?;
                    let child =
                        SchemeReader::read_cow_with_limits(Cow::Borrowed(bytes), self.limits)
                            .map_err(|error| {
                                ContainerMountError::new(
                                    state.report.clone(),
                                    ContainerMountErrorKind::Scheme(error),
                                )
                            })?;
                    mount_tree_child(
                        child,
                        ContainerTreeEntry {
                            virtual_path: path,
                            source_name: entry.path.as_str(),
                            entry_index: index,
                        },
                        container_scope_index,
                        container_bundle_index,
                        depth,
                        state,
                    )?;
                }
            }
            ParsedFile::WebData(web_file) => {
                preflight_tree_entry_count(web_file.entries.len(), state)?;
                for (index, entry) in web_file.entries.iter().enumerate() {
                    let path = checked_container_entry_path(
                        state.filesystem,
                        destination_root,
                        entry.name.as_str(),
                        index,
                        &state.report,
                    )?;
                    let bytes = web_file.entry_data(index).ok_or_else(|| {
                        ContainerMountError::new(
                            state.report.clone(),
                            ContainerMountErrorKind::Scheme(SchemeError::new(
                                SchemeErrorKind::WebDataEntryIndexOutOfBounds {
                                    index,
                                    entry_count: web_file.entries.len(),
                                },
                            )),
                        )
                    })?;
                    let child =
                        SchemeReader::read_cow_with_limits(Cow::Borrowed(bytes), self.limits)
                            .map_err(|error| {
                                ContainerMountError::new(
                                    state.report.clone(),
                                    ContainerMountErrorKind::Scheme(error),
                                )
                            })?;
                    mount_tree_child(
                        child,
                        ContainerTreeEntry {
                            virtual_path: path,
                            source_name: entry.name.as_str(),
                            entry_index: index,
                        },
                        container_scope_index,
                        container_bundle_index,
                        depth,
                        state,
                    )?;
                }
            }
            ParsedFile::SerializedFile(_) | ParsedFile::Resource(_) => {
                return Err(ContainerMountError::new(
                    state.report.clone(),
                    ContainerMountErrorKind::NotContainer,
                ));
            }
        }

        Ok(())
    }
}

fn preflight_entry_count(
    count: usize,
    limits: ContainerMountLimits,
    report: &ContainerMountReport,
) -> Result<(), ContainerMountError> {
    if count > limits.max_entries {
        return Err(ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::EntryCountLimitExceeded {
                count,
                limit: limits.max_entries,
            },
        ));
    }
    Ok(())
}

fn preflight_output_sizes(
    sizes: impl IntoIterator<Item = u64>,
    limits: ContainerMountLimits,
    report: &ContainerMountReport,
) -> Result<(), ContainerMountError> {
    let mut total = 0u64;
    for (entry_index, size) in sizes.into_iter().enumerate() {
        total = total.checked_add(size).ok_or_else(|| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::TotalOutputSizeOverflow { entry_index },
            )
        })?;
        if total > limits.max_total_output_bytes {
            return Err(ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::TotalOutputLimitExceeded {
                    entry_index,
                    requested_total: total,
                    limit: limits.max_total_output_bytes,
                },
            ));
        }
    }
    Ok(())
}

fn record_failed_container_entry(
    report: &mut ContainerMountReport,
    source_name: &str,
    virtual_path: String,
    container_bundle_index: Option<usize>,
    entry_index: usize,
    error: UnityFsError,
    limits: ContainerMountLimits,
) -> Result<(), ContainerMountError> {
    let requested_count = report.failed_entries.len().checked_add(1).ok_or_else(|| {
        ContainerMountError::new(report.clone(), ContainerMountErrorKind::EntryCountOverflow)
    })?;
    if requested_count > limits.max_failed_entries {
        return Err(ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FailedEntryCountLimitExceeded {
                count: requested_count,
                limit: limits.max_failed_entries,
            },
        ));
    }
    let record_bytes = u64::try_from(source_name.len())
        .ok()
        .and_then(|source_bytes| {
            u64::try_from(virtual_path.len())
                .ok()
                .and_then(|path_bytes| source_bytes.checked_add(path_bytes))
        })
        .ok_or_else(|| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::FailedEntryMetadataLimitExceeded {
                    requested: u64::MAX,
                    limit: limits.max_total_failed_entry_metadata_bytes,
                },
            )
        })?;
    let requested_metadata_bytes = report
        .failed_entry_metadata_bytes
        .checked_add(record_bytes)
        .ok_or_else(|| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::FailedEntryMetadataLimitExceeded {
                    requested: u64::MAX,
                    limit: limits.max_total_failed_entry_metadata_bytes,
                },
            )
        })?;
    if requested_metadata_bytes > limits.max_total_failed_entry_metadata_bytes {
        return Err(ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FailedEntryMetadataLimitExceeded {
                requested: requested_metadata_bytes,
                limit: limits.max_total_failed_entry_metadata_bytes,
            },
        ));
    }

    let mut owned_source_name = String::new();
    owned_source_name
        .try_reserve_exact(source_name.len())
        .map_err(|_| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::FailedEntryAllocationFailed {
                    requested: source_name.len(),
                },
            )
        })?;
    owned_source_name.push_str(source_name);
    report.failed_entries.try_reserve(1).map_err(|_| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FailedEntryAllocationFailed {
                requested: requested_count,
            },
        )
    })?;
    report.failed_entries.push(FailedContainerEntry {
        virtual_path,
        source_name: owned_source_name,
        container_bundle_index,
        entry_index,
        error,
    });
    report.failed_entry_metadata_bytes = requested_metadata_bytes;
    Ok(())
}

fn mount_container_entry<'a>(
    filesystem: &mut VirtualFileSystem,
    destination_root: Option<&str>,
    entry_name: &str,
    entry_index: usize,
    bytes: Cow<'a, [u8]>,
    limits: ContainerMountLimits,
    report: &mut ContainerMountReport,
) -> Result<(), ContainerMountError> {
    let path = checked_container_entry_path(
        filesystem,
        destination_root,
        entry_name,
        entry_index,
        report,
    )?;
    mount_container_entry_at_path(filesystem, &path, entry_index, bytes, limits, report)
}

pub(super) fn mount_container_entry_at_path<'a>(
    filesystem: &mut VirtualFileSystem,
    path: &str,
    entry_index: usize,
    bytes: Cow<'a, [u8]>,
    limits: ContainerMountLimits,
    report: &mut ContainerMountReport,
) -> Result<(), ContainerMountError> {
    let entry_size = u64::try_from(bytes.len()).map_err(|_| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::TotalOutputSizeOverflow { entry_index },
        )
    })?;
    let new_total = report
        .mounted_bytes
        .checked_add(entry_size)
        .ok_or_else(|| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::TotalOutputSizeOverflow { entry_index },
            )
        })?;
    if new_total > limits.max_total_output_bytes {
        return Err(ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::TotalOutputLimitExceeded {
                entry_index,
                requested_total: new_total,
                limit: limits.max_total_output_bytes,
            },
        ));
    }

    let owned_bytes = match bytes {
        Cow::Owned(bytes) => bytes,
        Cow::Borrowed(bytes) => {
            let mut owned = Vec::new();
            owned.try_reserve_exact(bytes.len()).map_err(|_| {
                ContainerMountError::new(
                    report.clone(),
                    ContainerMountErrorKind::EntryAllocationFailed {
                        entry_index,
                        requested: bytes.len(),
                    },
                )
            })?;
            owned.extend_from_slice(bytes);
            owned
        }
    };
    filesystem
        .insert_file_stream_with_parents(path, SmartStream::from_memory(owned_bytes))
        .map_err(|source| {
            ContainerMountError::new(
                report.clone(),
                ContainerMountErrorKind::FileSystem {
                    entry_index,
                    path: path.to_owned(),
                    source,
                },
            )
        })?;
    report.mounted_entries += 1;
    report.mounted_bytes = new_total;
    Ok(())
}

pub(super) fn checked_container_entry_path(
    filesystem: &VirtualFileSystem,
    destination_root: Option<&str>,
    entry_name: &str,
    entry_index: usize,
    report: &ContainerMountReport,
) -> Result<String, ContainerMountError> {
    container_entry_path(
        destination_root,
        entry_name,
        filesystem.limits().max_path_bytes,
    )
    .map_err(|error| {
        let kind = match error {
            ContainerEntryPathError::Invalid => ContainerMountErrorKind::InvalidEntryPath {
                entry_index,
                path: entry_name.to_owned(),
            },
            ContainerEntryPathError::TooLong => ContainerMountErrorKind::EntryPathTooLong {
                entry_index,
                path: entry_name.to_owned(),
                limit: filesystem.limits().max_path_bytes,
            },
            ContainerEntryPathError::AllocationFailed { requested } => {
                ContainerMountErrorKind::EntryPathAllocationFailed {
                    entry_index,
                    requested,
                }
            }
        };
        ContainerMountError::new(report.clone(), kind)
    })
}

fn preflight_tree_entry_count(
    count: usize,
    state: &mut ContainerTreeMountState<'_>,
) -> Result<(), ContainerMountError> {
    let requested = state.visited_entries.checked_add(count).ok_or_else(|| {
        ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::EntryCountOverflow,
        )
    })?;
    if requested > state.limits.max_entries {
        return Err(ContainerMountError::new(
            state.report.clone(),
            ContainerMountErrorKind::EntryCountLimitExceeded {
                count: requested,
                limit: state.limits.max_entries,
            },
        ));
    }
    state.visited_entries = requested;
    Ok(())
}
