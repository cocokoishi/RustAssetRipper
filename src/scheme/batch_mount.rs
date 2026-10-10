use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use super::container_mount::{
    checked_container_entry_path, directory_input_destination, directory_relative_destination,
    mount_container_entry_at_path, mount_container_stream_at_path, mount_unityfs_stream_tree,
};
use super::*;
use crate::io::discovery::discover_directory_inputs;
use crate::io::filesystem::VirtualFileSystem;
use crate::io::stream::{logical_file_name, logical_file_path, open_logical_file, SmartStream};

fn detach_mount_stream(
    mut source: SmartStream,
    path: &str,
    entry_index: usize,
    limits: ContainerMountLimits,
    report: &ContainerMountReport,
) -> Result<SmartStream, ContainerMountError> {
    let length = source.length().map_err(|source| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FileSystem {
                entry_index,
                path: path.to_owned(),
                source,
            },
        )
    })?;
    let requested_total = report.mounted_bytes.checked_add(length).ok_or_else(|| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::TotalOutputSizeOverflow { entry_index },
        )
    })?;
    if requested_total > limits.max_total_output_bytes {
        return Err(ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::TotalOutputLimitExceeded {
                entry_index,
                requested_total,
                limit: limits.max_total_output_bytes,
            },
        ));
    }
    source.seek(SeekFrom::Start(0)).map_err(|source| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FileSystem {
                entry_index,
                path: path.to_owned(),
                source,
            },
        )
    })?;
    let mut detached = SmartStream::create_temp().map_err(|source| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FileSystem {
                entry_index,
                path: path.to_owned(),
                source,
            },
        )
    })?;
    let copied = std::io::copy(&mut source.take(length), &mut detached).map_err(|source| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FileSystem {
                entry_index,
                path: path.to_owned(),
                source,
            },
        )
    })?;
    if copied != length {
        return Err(ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FileSystem {
                entry_index,
                path: path.to_owned(),
                source: std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("input stream ended at {copied} of {length} bytes"),
                ),
            },
        ));
    }
    detached.flush().map_err(|source| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FileSystem {
                entry_index,
                path: path.to_owned(),
                source,
            },
        )
    })?;
    detached.seek(SeekFrom::Start(0)).map_err(|source| {
        ContainerMountError::new(
            report.clone(),
            ContainerMountErrorKind::FileSystem {
                entry_index,
                path: path.to_owned(),
                source,
            },
        )
    })?;
    Ok(detached)
}

struct BatchInputRequest {
    path: std::path::PathBuf,
    source_bytes: u64,
    standalone_destination: Option<String>,
    container_destination: Option<String>,
    explicit_path: bool,
}

fn merge_batch_mount_report(
    batch: &mut BatchMountReport,
    input: &ContainerMountReport,
    input_path: &Path,
    limits: BatchMountLimits,
) -> Result<(), BatchMountErrorKind> {
    let mounted_entries = batch
        .mounted_entries
        .checked_add(input.mounted_entries)
        .ok_or(BatchMountErrorKind::ReportOverflow)?;
    let mounted_bytes = batch
        .mounted_bytes
        .checked_add(input.mounted_bytes)
        .ok_or(BatchMountErrorKind::ReportOverflow)?;
    let failed_entry_count = batch
        .failed_entries
        .len()
        .checked_add(input.failed_entries.len())
        .ok_or(BatchMountErrorKind::ReportOverflow)?;
    if failed_entry_count > limits.max_failed_entries {
        return Err(BatchMountErrorKind::FailedEntryCountLimitExceeded {
            count: failed_entry_count,
            limit: limits.max_failed_entries,
        });
    }
    let input_path_record_bytes = u64::try_from(input_path.as_os_str().len())
        .ok()
        .and_then(|path_bytes| {
            u64::try_from(input.failed_entries.len())
                .ok()
                .and_then(|count| path_bytes.checked_mul(count))
        })
        .ok_or(BatchMountErrorKind::ReportOverflow)?;
    let failed_entry_metadata_bytes = batch
        .failed_entry_metadata_bytes
        .checked_add(input.failed_entry_metadata_bytes)
        .and_then(|bytes| bytes.checked_add(input_path_record_bytes))
        .ok_or(BatchMountErrorKind::ReportOverflow)?;
    if failed_entry_metadata_bytes > limits.max_total_failed_entry_metadata_bytes {
        return Err(BatchMountErrorKind::FailedEntryMetadataLimitExceeded {
            requested: failed_entry_metadata_bytes,
            limit: limits.max_total_failed_entry_metadata_bytes,
        });
    }
    batch
        .failed_entries
        .try_reserve(input.failed_entries.len())
        .map_err(|_| BatchMountErrorKind::FailedEntryAllocationFailed {
            requested: failed_entry_count,
        })?;
    batch.mounted_entries = mounted_entries;
    batch.mounted_bytes = mounted_bytes;
    batch.failed_entry_metadata_bytes = failed_entry_metadata_bytes;
    for entry in &input.failed_entries {
        batch.failed_entries.push(BatchFailedContainerEntry {
            input_path: input_path.to_path_buf(),
            entry: entry.clone(),
        });
    }
    Ok(())
}

fn batch_input_io_error(
    report: BatchMountReport,
    path: &Path,
    source: std::io::Error,
) -> BatchMountError {
    BatchMountError::new(
        report,
        Some(path.to_path_buf()),
        BatchMountErrorKind::InputMount(Box::new(ContainerMountError::new(
            ContainerMountReport::default(),
            ContainerMountErrorKind::Scheme(SchemeError::new(SchemeErrorKind::StreamInput(source))),
        ))),
    )
}

impl SchemeReader {
    pub fn mount_path(
        path: impl AsRef<Path>,
        filesystem: &mut VirtualFileSystem,
        destination_root: Option<&str>,
        scheme_limits: SchemeReaderLimits,
        mount_limits: ContainerMountLimits,
    ) -> Result<ContainerMountReport, ContainerMountError> {
        let input_path = path.as_ref();
        let mut stream = SmartStream::open_read_multi(input_path).map_err(|error| {
            ContainerMountError::new(
                ContainerMountReport::default(),
                ContainerMountErrorKind::Scheme(SchemeError::new(SchemeErrorKind::StreamInput(
                    error,
                ))),
            )
        })?;
        let direct_unityfs =
            Self::is_direct_unityfs_stream(&mut stream, scheme_limits).map_err(|error| {
                ContainerMountError::new(
                    ContainerMountReport::default(),
                    ContainerMountErrorKind::Scheme(error),
                )
            })?;
        if direct_unityfs {
            let source_name = logical_file_name(input_path).ok_or_else(|| {
                ContainerMountError::new(
                    ContainerMountReport::default(),
                    ContainerMountErrorKind::InputPathHasNoFileName,
                )
            })?;
            return mount_unityfs_stream_tree(
                &stream,
                Some(&source_name),
                filesystem,
                destination_root,
                scheme_limits,
                mount_limits,
            );
        }
        let stream_dispatched =
            Self::dispatch_stream_file_backed(stream, scheme_limits).map_err(|error| {
                ContainerMountError::new(
                    ContainerMountReport::default(),
                    ContainerMountErrorKind::Scheme(error),
                )
            })?;

        if stream_dispatched.kind == FileKind::UnityFs {
            let source_name = logical_file_name(input_path).ok_or_else(|| {
                ContainerMountError::new(
                    ContainerMountReport::default(),
                    ContainerMountErrorKind::InputPathHasNoFileName,
                )
            })?;
            return mount_unityfs_stream_tree(
                &stream_dispatched.stream,
                Some(&source_name),
                filesystem,
                destination_root,
                scheme_limits,
                mount_limits,
            );
        }

        if matches!(
            stream_dispatched.kind,
            FileKind::SerializedFile | FileKind::Resource
        ) {
            let file_name = logical_file_name(input_path).ok_or_else(|| {
                ContainerMountError::new(
                    ContainerMountReport::default(),
                    ContainerMountErrorKind::InputPathHasNoFileName,
                )
            })?;
            let mut report = ContainerMountReport::default();
            let virtual_path =
                checked_container_entry_path(filesystem, destination_root, &file_name, 0, &report)?;
            let stream = if stream_dispatched.has_wrappers() {
                stream_dispatched.stream
            } else {
                detach_mount_stream(
                    stream_dispatched.stream,
                    &virtual_path,
                    0,
                    mount_limits,
                    &report,
                )?
            };
            mount_container_stream_at_path(
                filesystem,
                &virtual_path,
                0,
                stream,
                mount_limits,
                &mut report,
            )?;
            return Ok(report);
        }

        let dispatched = stream_dispatched
            .into_dispatched(scheme_limits)
            .map_err(|error| {
                ContainerMountError::new(
                    ContainerMountReport::default(),
                    ContainerMountErrorKind::Scheme(error),
                )
            })?;

        Self::mount_dispatched_path(
            input_path,
            dispatched,
            filesystem,
            destination_root,
            mount_limits,
        )
    }

    fn mount_dispatched_path(
        input_path: &Path,
        dispatched: DispatchedFile<'static>,
        filesystem: &mut VirtualFileSystem,
        destination_root: Option<&str>,
        mount_limits: ContainerMountLimits,
    ) -> Result<ContainerMountReport, ContainerMountError> {
        if is_container_kind(dispatched.kind) {
            let source_name = logical_file_name(input_path).ok_or_else(|| {
                ContainerMountError::new(
                    ContainerMountReport::default(),
                    ContainerMountErrorKind::InputPathHasNoFileName,
                )
            })?;
            dispatched.mount_container_tree_with_source_name(
                filesystem,
                destination_root,
                Some(&source_name),
                mount_limits,
            )
        } else {
            dispatched.parse().map_err(|error| {
                ContainerMountError::new(
                    ContainerMountReport::default(),
                    ContainerMountErrorKind::Scheme(error),
                )
            })?;
            let file_name = logical_file_name(input_path).ok_or_else(|| {
                ContainerMountError::new(
                    ContainerMountReport::default(),
                    ContainerMountErrorKind::InputPathHasNoFileName,
                )
            })?;
            let mut report = ContainerMountReport::default();
            let virtual_path =
                checked_container_entry_path(filesystem, destination_root, &file_name, 0, &report)?;
            mount_container_entry_at_path(
                filesystem,
                &virtual_path,
                0,
                dispatched.contents,
                mount_limits,
                &mut report,
            )?;
            Ok(report)
        }
    }

    /// Preflight and mount an explicit list of local file or split-file paths.
    ///
    /// The list order is preserved. Each item supplies its own virtual destination root so
    /// callers can keep inputs from different source directories separate.
    pub fn mount_paths(
        inputs: &[PathMountInput],
        filesystem: &mut VirtualFileSystem,
        scheme_limits: SchemeReaderLimits,
        mount_limits: ContainerMountLimits,
        batch_limits: BatchMountLimits,
    ) -> Result<BatchMountReport, BatchMountError> {
        let mut report = BatchMountReport {
            discovered_inputs: inputs.len(),
            ..BatchMountReport::default()
        };
        if inputs.len() > batch_limits.max_inputs {
            return Err(BatchMountError::new(
                report,
                None,
                BatchMountErrorKind::InputCountLimitExceeded {
                    count: inputs.len(),
                    limit: batch_limits.max_inputs,
                },
            ));
        }

        let mut requests = Vec::new();
        requests.try_reserve_exact(inputs.len()).map_err(|_| {
            BatchMountError::new(
                report.clone(),
                None,
                BatchMountErrorKind::InputListAllocationFailed {
                    requested: inputs.len(),
                },
            )
        })?;
        let mut seen = BTreeSet::new();
        for input in inputs {
            let destination_key = input.destination_root.as_deref().unwrap_or("").to_owned();
            let logical_path = logical_file_path(&input.path);
            if !seen.insert((logical_path, destination_key)) {
                return Err(BatchMountError::new(
                    report,
                    Some(input.path.clone()),
                    BatchMountErrorKind::DuplicateInputPath,
                ));
            }

            match std::fs::symlink_metadata(&input.path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(BatchMountError::new(
                        report,
                        Some(input.path.clone()),
                        BatchMountErrorKind::ExplicitInputIsSymbolicLink,
                    ));
                }
                Ok(metadata) if metadata.is_dir() => {
                    return Err(BatchMountError::new(
                        report,
                        Some(input.path.clone()),
                        BatchMountErrorKind::ExplicitInputIsDirectory,
                    ));
                }
                Ok(metadata) if !metadata.is_file() => {
                    return Err(BatchMountError::new(
                        report,
                        Some(input.path.clone()),
                        BatchMountErrorKind::ExplicitInputNotRegular,
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(batch_input_io_error(report, &input.path, error));
                }
            }

            let mut stream = open_logical_file(&input.path)
                .map_err(|error| batch_input_io_error(report.clone(), &input.path, error))?;
            let source_bytes = stream
                .seek(SeekFrom::End(0))
                .map_err(|error| batch_input_io_error(report.clone(), &input.path, error))?;
            report.discovered_source_bytes = report
                .discovered_source_bytes
                .checked_add(source_bytes)
                .ok_or_else(|| {
                    BatchMountError::new(
                        report.clone(),
                        Some(input.path.clone()),
                        BatchMountErrorKind::ReportOverflow,
                    )
                })?;
            requests.push(BatchInputRequest {
                path: input.path.clone(),
                source_bytes,
                standalone_destination: input.destination_root.clone(),
                container_destination: input.destination_root.clone(),
                explicit_path: true,
            });
        }

        report
            .explicit_root_resource_files
            .try_reserve_exact(inputs.len())
            .map_err(|_| {
                BatchMountError::new(
                    report.clone(),
                    None,
                    BatchMountErrorKind::InputListAllocationFailed {
                        requested: inputs.len(),
                    },
                )
            })?;

        Self::mount_batch_requests(
            requests,
            filesystem,
            scheme_limits,
            mount_limits,
            batch_limits,
            report,
        )
    }

    /// Discover and mount every regular input beneath a local directory.
    ///
    /// Traversal is deterministic, symbolic links and special files are rejected, numbered
    /// `.splitN` parts are treated as one logical input, and input/output byte limits apply to
    /// the complete batch. Standalone files retain relative paths; root containers use each
    /// logical input path as a namespace in the virtual tree.
    pub fn mount_directory(
        input_directory: impl AsRef<Path>,
        filesystem: &mut VirtualFileSystem,
        destination_root: Option<&str>,
        scheme_limits: SchemeReaderLimits,
        mount_limits: ContainerMountLimits,
        batch_limits: BatchMountLimits,
    ) -> Result<BatchMountReport, BatchMountError> {
        let input_directory = input_directory.as_ref();
        let (inputs, discovery_report) = discover_directory_inputs(
            input_directory,
            batch_limits.max_directory_entries,
            batch_limits.max_directory_depth,
            batch_limits.max_inputs,
        )
        .map_err(|error| {
            let partial_report = BatchMountReport {
                directory_entries_scanned: error.report.directory_entries_scanned,
                discovered_inputs: error.report.discovered_inputs,
                discovered_source_bytes: error.report.source_bytes,
                ..BatchMountReport::default()
            };
            BatchMountError::new(
                partial_report,
                Some(error.path.clone()),
                BatchMountErrorKind::DirectoryDiscovery(Box::new(error)),
            )
        })?;
        let report = BatchMountReport {
            directory_entries_scanned: discovery_report.directory_entries_scanned,
            discovered_inputs: discovery_report.discovered_inputs,
            discovered_source_bytes: discovery_report.source_bytes,
            ..BatchMountReport::default()
        };

        let mut requests = Vec::new();
        requests.try_reserve_exact(inputs.len()).map_err(|_| {
            BatchMountError::new(
                report.clone(),
                None,
                BatchMountErrorKind::InputListAllocationFailed {
                    requested: inputs.len(),
                },
            )
        })?;
        for input in inputs {
            let standalone_destination =
                directory_input_destination(destination_root, &input.relative_path).map_err(
                    |_| {
                        BatchMountError::new(
                            report.clone(),
                            Some(input.path.clone()),
                            BatchMountErrorKind::InvalidInputPathEncoding,
                        )
                    },
                )?;
            let container_destination =
                directory_relative_destination(destination_root, &input.relative_path).map_err(
                    |_| {
                        BatchMountError::new(
                            report.clone(),
                            Some(input.path.clone()),
                            BatchMountErrorKind::InvalidInputPathEncoding,
                        )
                    },
                )?;
            requests.push(BatchInputRequest {
                path: input.path,
                source_bytes: input.source_bytes,
                standalone_destination,
                container_destination,
                explicit_path: false,
            });
        }

        Self::mount_batch_requests(
            requests,
            filesystem,
            scheme_limits,
            mount_limits,
            batch_limits,
            report,
        )
    }

    /// Mounts regular inputs under their relative paths without expanding recognized containers.
    ///
    /// The VFS retains file-backed readers, including logical `.splitN` streams. This mode is for
    /// callers such as platform inventory that must inspect the original container identity
    /// before choosing source-specific expansion rules.
    pub fn mount_directory_preserving_containers(
        input_directory: impl AsRef<Path>,
        filesystem: &mut VirtualFileSystem,
        destination_root: Option<&str>,
        scheme_limits: SchemeReaderLimits,
        mount_limits: ContainerMountLimits,
        batch_limits: BatchMountLimits,
    ) -> Result<BatchMountReport, BatchMountError> {
        let input_directory = input_directory.as_ref();
        let (inputs, discovery_report) = discover_directory_inputs(
            input_directory,
            batch_limits.max_directory_entries,
            batch_limits.max_directory_depth,
            batch_limits.max_inputs,
        )
        .map_err(|error| {
            let partial_report = BatchMountReport {
                directory_entries_scanned: error.report.directory_entries_scanned,
                discovered_inputs: error.report.discovered_inputs,
                discovered_source_bytes: error.report.source_bytes,
                ..BatchMountReport::default()
            };
            BatchMountError::new(
                partial_report,
                Some(error.path.clone()),
                BatchMountErrorKind::DirectoryDiscovery(Box::new(error)),
            )
        })?;
        let mut report = BatchMountReport {
            directory_entries_scanned: discovery_report.directory_entries_scanned,
            discovered_inputs: discovery_report.discovered_inputs,
            discovered_source_bytes: discovery_report.source_bytes,
            ..BatchMountReport::default()
        };
        if report.discovered_source_bytes > batch_limits.max_total_input_bytes {
            let requested_total = report.discovered_source_bytes;
            return Err(BatchMountError::new(
                report,
                None,
                BatchMountErrorKind::TotalInputLimitExceeded {
                    requested_total,
                    limit: batch_limits.max_total_input_bytes,
                },
            ));
        }
        let per_input_limit = u64::try_from(scheme_limits.max_input_bytes).unwrap_or(u64::MAX);
        if let Some(input) = inputs
            .iter()
            .find(|input| input.source_bytes > per_input_limit)
        {
            return Err(BatchMountError::new(
                report,
                Some(input.path.clone()),
                BatchMountErrorKind::InputFileLimitExceeded {
                    size: input.source_bytes,
                    limit: per_input_limit,
                },
            ));
        }
        if inputs.len() > mount_limits.max_entries {
            return Err(BatchMountError::new(
                report,
                None,
                BatchMountErrorKind::ContainerEntryCountLimitExceeded {
                    count: inputs.len(),
                    limit: mount_limits.max_entries,
                },
            ));
        }

        let mut destinations = Vec::new();
        destinations.try_reserve_exact(inputs.len()).map_err(|_| {
            BatchMountError::new(
                report.clone(),
                None,
                BatchMountErrorKind::InputListAllocationFailed {
                    requested: inputs.len(),
                },
            )
        })?;
        for input in inputs {
            let virtual_path =
                directory_relative_destination(destination_root, &input.relative_path)
                    .map_err(|_| {
                        BatchMountError::new(
                            report.clone(),
                            Some(input.path.clone()),
                            BatchMountErrorKind::InvalidInputPathEncoding,
                        )
                    })?
                    .ok_or_else(|| {
                        BatchMountError::new(
                            report.clone(),
                            Some(input.path.clone()),
                            BatchMountErrorKind::InvalidInputPathEncoding,
                        )
                    })?;
            destinations.push((input, virtual_path));
        }

        for (input, virtual_path) in destinations {
            let reader = open_logical_file(&input.path)
                .map_err(|error| batch_input_io_error(report.clone(), &input.path, error))?;
            let stream = SmartStream::from_reader(reader)
                .map_err(|error| batch_input_io_error(report.clone(), &input.path, error))?;
            let opened_bytes = stream
                .length()
                .map_err(|error| batch_input_io_error(report.clone(), &input.path, error))?;
            if opened_bytes != input.source_bytes {
                return Err(BatchMountError::new(
                    report,
                    Some(input.path),
                    BatchMountErrorKind::InputSizeChanged {
                        discovered_bytes: input.source_bytes,
                        opened_bytes,
                    },
                ));
            }
            let next_mounted_bytes =
                report
                    .mounted_bytes
                    .checked_add(opened_bytes)
                    .ok_or_else(|| {
                        BatchMountError::new(
                            report.clone(),
                            Some(input.path.clone()),
                            BatchMountErrorKind::ReportOverflow,
                        )
                    })?;
            filesystem
                .insert_file_stream_with_parents(&virtual_path, stream)
                .map_err(|source| {
                    let error = ContainerMountError::new(
                        ContainerMountReport {
                            mounted_entries: report.mounted_entries,
                            mounted_bytes: report.mounted_bytes,
                            ..ContainerMountReport::default()
                        },
                        ContainerMountErrorKind::FileSystem {
                            entry_index: report.mounted_entries,
                            path: virtual_path.clone(),
                            source,
                        },
                    );
                    BatchMountError::new(
                        report.clone(),
                        Some(input.path.clone()),
                        BatchMountErrorKind::InputMount(Box::new(error)),
                    )
                })?;
            report.mounted_entries = report.mounted_entries.checked_add(1).ok_or_else(|| {
                BatchMountError::new(
                    report.clone(),
                    Some(input.path.clone()),
                    BatchMountErrorKind::ReportOverflow,
                )
            })?;
            report.mounted_bytes = next_mounted_bytes;
            report.completed_inputs = report.completed_inputs.checked_add(1).ok_or_else(|| {
                BatchMountError::new(
                    report.clone(),
                    Some(input.path),
                    BatchMountErrorKind::ReportOverflow,
                )
            })?;
        }

        Ok(report)
    }

    fn mount_batch_requests(
        requests: Vec<BatchInputRequest>,
        filesystem: &mut VirtualFileSystem,
        scheme_limits: SchemeReaderLimits,
        mount_limits: ContainerMountLimits,
        batch_limits: BatchMountLimits,
        mut report: BatchMountReport,
    ) -> Result<BatchMountReport, BatchMountError> {
        if requests.len() > batch_limits.max_inputs {
            return Err(BatchMountError::new(
                report,
                None,
                BatchMountErrorKind::InputCountLimitExceeded {
                    count: requests.len(),
                    limit: batch_limits.max_inputs,
                },
            ));
        }
        if report.discovered_source_bytes > batch_limits.max_total_input_bytes {
            let requested_total = report.discovered_source_bytes;
            return Err(BatchMountError::new(
                report,
                None,
                BatchMountErrorKind::TotalInputLimitExceeded {
                    requested_total,
                    limit: batch_limits.max_total_input_bytes,
                },
            ));
        }
        let per_input_limit = u64::try_from(scheme_limits.max_input_bytes).unwrap_or(u64::MAX);
        if let Some(input) = requests
            .iter()
            .find(|input| input.source_bytes > per_input_limit)
        {
            return Err(BatchMountError::new(
                report,
                Some(input.path.clone()),
                BatchMountErrorKind::InputFileLimitExceeded {
                    size: input.source_bytes,
                    limit: per_input_limit,
                },
            ));
        }

        for input in requests {
            let mut stream = SmartStream::open_read_multi(&input.path).map_err(|error| {
                BatchMountError::new(
                    report.clone(),
                    Some(input.path.clone()),
                    BatchMountErrorKind::InputMount(Box::new(ContainerMountError::new(
                        ContainerMountReport::default(),
                        ContainerMountErrorKind::Scheme(SchemeError::new(
                            SchemeErrorKind::StreamInput(error),
                        )),
                    ))),
                )
            })?;
            let opened_bytes = stream
                .length()
                .map_err(|source| batch_input_io_error(report.clone(), &input.path, source))?;
            if opened_bytes != input.source_bytes {
                return Err(BatchMountError::new(
                    report,
                    Some(input.path),
                    BatchMountErrorKind::InputSizeChanged {
                        discovered_bytes: input.source_bytes,
                        opened_bytes,
                    },
                ));
            }
            let direct_unityfs = Self::is_direct_unityfs_stream(&mut stream, scheme_limits)
                .map_err(|error| {
                    BatchMountError::new(
                        report.clone(),
                        Some(input.path.clone()),
                        BatchMountErrorKind::InputMount(Box::new(ContainerMountError::new(
                            ContainerMountReport::default(),
                            ContainerMountErrorKind::Scheme(error),
                        ))),
                    )
                })?;
            let mut stream_dispatched = if direct_unityfs {
                None
            } else {
                Some(
                    Self::dispatch_stream_file_backed(stream.create_reference(), scheme_limits)
                        .map_err(|error| {
                            BatchMountError::new(
                                report.clone(),
                                Some(input.path.clone()),
                                BatchMountErrorKind::InputMount(Box::new(
                                    ContainerMountError::new(
                                        ContainerMountReport::default(),
                                        ContainerMountErrorKind::Scheme(error),
                                    ),
                                )),
                            )
                        })?,
                )
            };
            let root_kind = stream_dispatched
                .as_ref()
                .map(|dispatched| dispatched.kind)
                .unwrap_or(FileKind::UnityFs);
            let root_is_file = matches!(root_kind, FileKind::SerializedFile | FileKind::Resource);
            let dispatched = if direct_unityfs || root_kind == FileKind::UnityFs || root_is_file {
                None
            } else {
                let Some(stream_dispatched) = stream_dispatched.take() else {
                    return Err(BatchMountError::new(
                        report,
                        Some(input.path),
                        BatchMountErrorKind::InputMount(Box::new(ContainerMountError::new(
                            ContainerMountReport::default(),
                            ContainerMountErrorKind::NotContainer,
                        ))),
                    ));
                };
                Some(
                    stream_dispatched
                        .into_dispatched(scheme_limits)
                        .map_err(|error| {
                            BatchMountError::new(
                                report.clone(),
                                Some(input.path.clone()),
                                BatchMountErrorKind::InputMount(Box::new(
                                    ContainerMountError::new(
                                        ContainerMountReport::default(),
                                        ContainerMountErrorKind::Scheme(error),
                                    ),
                                )),
                            )
                        })?,
                )
            };
            let is_container = direct_unityfs || is_container_kind(root_kind);
            let destination = if is_container {
                input.container_destination.as_deref()
            } else {
                input.standalone_destination.as_deref()
            };
            let explicit_root_resource_file =
                if input.explicit_path && root_kind == FileKind::Resource {
                    let file_name = logical_file_name(&input.path).ok_or_else(|| {
                        BatchMountError::new(
                            report.clone(),
                            Some(input.path.clone()),
                            BatchMountErrorKind::InputMount(Box::new(ContainerMountError::new(
                                ContainerMountReport::default(),
                                ContainerMountErrorKind::InputPathHasNoFileName,
                            ))),
                        )
                    })?;
                    let virtual_path = checked_container_entry_path(
                        filesystem,
                        destination,
                        &file_name,
                        0,
                        &ContainerMountReport::default(),
                    )
                    .map_err(|error| {
                        BatchMountError::new(
                            report.clone(),
                            Some(input.path.clone()),
                            BatchMountErrorKind::InputMount(Box::new(error)),
                        )
                    })?;
                    Some(MountedExplicitResourceFile {
                        file_name,
                        virtual_path,
                        kind: root_kind,
                    })
                } else {
                    None
                };
            let remaining_output = batch_limits
                .max_total_output_bytes
                .checked_sub(report.mounted_bytes)
                .ok_or_else(|| {
                    BatchMountError::new(
                        report.clone(),
                        Some(input.path.clone()),
                        BatchMountErrorKind::ReportOverflow,
                    )
                })?;
            let remaining_failed_entries = batch_limits
                .max_failed_entries
                .checked_sub(report.failed_entries.len())
                .ok_or_else(|| {
                    BatchMountError::new(
                        report.clone(),
                        Some(input.path.clone()),
                        BatchMountErrorKind::ReportOverflow,
                    )
                })?;
            let remaining_failed_metadata = batch_limits
                .max_total_failed_entry_metadata_bytes
                .checked_sub(report.failed_entry_metadata_bytes)
                .ok_or_else(|| {
                    BatchMountError::new(
                        report.clone(),
                        Some(input.path.clone()),
                        BatchMountErrorKind::ReportOverflow,
                    )
                })?;
            let mut per_input_mount_limits = mount_limits;
            per_input_mount_limits.max_total_output_bytes = per_input_mount_limits
                .max_total_output_bytes
                .min(remaining_output);
            per_input_mount_limits.max_failed_entries = per_input_mount_limits
                .max_failed_entries
                .min(remaining_failed_entries);
            per_input_mount_limits.max_total_failed_entry_metadata_bytes = per_input_mount_limits
                .max_total_failed_entry_metadata_bytes
                .min(remaining_failed_metadata);
            let output_before_input = report.mounted_bytes;

            let mounted = if direct_unityfs {
                let source_name = logical_file_name(&input.path).ok_or_else(|| {
                    BatchMountError::new(
                        report.clone(),
                        Some(input.path.clone()),
                        BatchMountErrorKind::InputMount(Box::new(ContainerMountError::new(
                            ContainerMountReport::default(),
                            ContainerMountErrorKind::InputPathHasNoFileName,
                        ))),
                    )
                })?;
                mount_unityfs_stream_tree(
                    &stream,
                    Some(&source_name),
                    filesystem,
                    destination,
                    scheme_limits,
                    per_input_mount_limits,
                )
            } else if root_kind == FileKind::UnityFs {
                let source_name = logical_file_name(&input.path).ok_or_else(|| {
                    BatchMountError::new(
                        report.clone(),
                        Some(input.path.clone()),
                        BatchMountErrorKind::InputMount(Box::new(ContainerMountError::new(
                            ContainerMountReport::default(),
                            ContainerMountErrorKind::InputPathHasNoFileName,
                        ))),
                    )
                })?;
                match stream_dispatched.as_ref() {
                    Some(dispatched) => mount_unityfs_stream_tree(
                        &dispatched.stream,
                        Some(&source_name),
                        filesystem,
                        destination,
                        scheme_limits,
                        per_input_mount_limits,
                    ),
                    None => Err(ContainerMountError::new(
                        ContainerMountReport::default(),
                        ContainerMountErrorKind::NotContainer,
                    )),
                }
            } else if root_is_file {
                let file_name = logical_file_name(&input.path).ok_or_else(|| {
                    BatchMountError::new(
                        report.clone(),
                        Some(input.path.clone()),
                        BatchMountErrorKind::InputMount(Box::new(ContainerMountError::new(
                            ContainerMountReport::default(),
                            ContainerMountErrorKind::InputPathHasNoFileName,
                        ))),
                    )
                })?;
                let virtual_path = checked_container_entry_path(
                    filesystem,
                    destination,
                    &file_name,
                    0,
                    &ContainerMountReport::default(),
                )
                .map_err(|error| {
                    BatchMountError::new(
                        report.clone(),
                        Some(input.path.clone()),
                        BatchMountErrorKind::InputMount(Box::new(error)),
                    )
                })?;
                match stream_dispatched.take() {
                    Some(dispatched) => {
                        let mut input_report = ContainerMountReport::default();
                        let stream = if dispatched.has_wrappers() {
                            Ok(dispatched.stream)
                        } else {
                            detach_mount_stream(
                                dispatched.stream,
                                &virtual_path,
                                0,
                                per_input_mount_limits,
                                &input_report,
                            )
                        };
                        stream.and_then(|stream| {
                            mount_container_stream_at_path(
                                filesystem,
                                &virtual_path,
                                0,
                                stream,
                                per_input_mount_limits,
                                &mut input_report,
                            )
                            .map(|()| input_report)
                        })
                    }
                    None => Err(ContainerMountError::new(
                        ContainerMountReport::default(),
                        ContainerMountErrorKind::NotContainer,
                    )),
                }
            } else {
                match dispatched {
                    Some(dispatched) => Self::mount_dispatched_path(
                        &input.path,
                        dispatched,
                        filesystem,
                        destination,
                        per_input_mount_limits,
                    ),
                    None => Err(ContainerMountError::new(
                        ContainerMountReport::default(),
                        ContainerMountErrorKind::NotContainer,
                    )),
                }
            };
            match mounted {
                Ok(input_report) => {
                    merge_batch_mount_report(&mut report, &input_report, &input.path, batch_limits)
                        .map_err(|kind| {
                            BatchMountError::new(report.clone(), Some(input.path.clone()), kind)
                        })?;
                    if !input_report.container_bundles.is_empty() {
                        let requested_trees =
                            report.container_trees.len().checked_add(1).ok_or_else(|| {
                                BatchMountError::new(
                                    report.clone(),
                                    Some(input.path.clone()),
                                    BatchMountErrorKind::ReportOverflow,
                                )
                            })?;
                        report.container_trees.try_reserve(1).map_err(|_| {
                            BatchMountError::new(
                                report.clone(),
                                Some(input.path.clone()),
                                BatchMountErrorKind::ContainerTreeAllocationFailed {
                                    requested: requested_trees,
                                },
                            )
                        })?;
                        let virtual_path = input_report
                            .container_scopes
                            .first()
                            .cloned()
                            .or_else(|| input.container_destination.clone())
                            .unwrap_or_else(|| {
                                format!("<batch input {}>", report.completed_inputs)
                            });
                        report.container_trees.push(Arc::new(BatchContainerTree {
                            virtual_path,
                            container_scopes: input_report.container_scopes,
                            container_bundles: input_report.container_bundles,
                            serialized_files: input_report.serialized_files,
                            resource_files: input_report.resource_files,
                            failed_entries: input_report.failed_entries,
                        }));
                    }
                    if let Some(resource_file) = explicit_root_resource_file {
                        report.explicit_root_resource_files.push(resource_file);
                    }
                    report.completed_inputs =
                        report.completed_inputs.checked_add(1).ok_or_else(|| {
                            BatchMountError::new(
                                report.clone(),
                                Some(input.path.clone()),
                                BatchMountErrorKind::ReportOverflow,
                            )
                        })?;
                }
                Err(input_error) => {
                    merge_batch_mount_report(
                        &mut report,
                        &input_error.partial_report,
                        &input.path,
                        batch_limits,
                    )
                    .map_err(|kind| {
                        BatchMountError::new(report.clone(), Some(input.path.clone()), kind)
                    })?;
                    let batch_output_limit = match &input_error.kind {
                        ContainerMountErrorKind::TotalOutputLimitExceeded {
                            requested_total,
                            ..
                        } if remaining_output <= mount_limits.max_total_output_bytes => {
                            Some(*requested_total)
                        }
                        _ => None,
                    };
                    let kind = if let Some(requested_total) = batch_output_limit {
                        let requested_total = output_before_input
                            .checked_add(requested_total)
                            .ok_or_else(|| {
                                BatchMountError::new(
                                    report.clone(),
                                    Some(input.path.clone()),
                                    BatchMountErrorKind::ReportOverflow,
                                )
                            })?;
                        BatchMountErrorKind::TotalOutputLimitExceeded {
                            requested_total,
                            limit: batch_limits.max_total_output_bytes,
                        }
                    } else {
                        BatchMountErrorKind::InputMount(Box::new(input_error))
                    };
                    return Err(BatchMountError::new(report, Some(input.path), kind));
                }
            }
        }

        Ok(report)
    }
}
