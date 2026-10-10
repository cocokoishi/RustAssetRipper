impl SchemeReader {
    /// Probe a raw VFS file header without reading the complete file or decoding wrappers.
    ///
    /// This is for source-style directory inventory, which checks file and bundle headers
    /// directly. `prefix` should contain at least the first 32 bytes when available.
    pub fn probe_raw_kind(prefix: &[u8], total_length: u64) -> FileKind {
        detect_file_kind_with_length(prefix, total_length)
    }

    /// Detect the source scheme and decode any outer GZip/Brotli wrappers recursively.
    pub fn read(input: &[u8]) -> Result<DispatchedFile<'_>, SchemeError> {
        Self::read_with_limits(input, SchemeReaderLimits::default())
    }

    /// Open a regular path or its numbered split parts and dispatch the logical file.
    ///
    /// This is the bounded seekable streaming entry point: see [`Self::read_path_streaming`].
    pub fn read_path(
        path: impl AsRef<Path>,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'static>, SchemeError> {
        Self::read_path_streaming(path, limits)
    }

    /// Read a logical path with bounded seekable streaming.
    ///
    /// The source `SchemeReader.LoadFile` opens a `SmartStream` and lets each scheme read what it
    /// needs instead of buffering the whole input first. This entry point keeps that shape:
    ///
    /// 1. It opens the logical file (a regular file or its assembled numbered `.splitN` parts) and
    ///    probes a bounded prefix for scheme detection, so detection never needs the whole input.
    /// 2. When the prefix is a GZip or Brotli wrapper, the layer is decompressed *directly from the
    ///    seekable stream* through the streaming codec entry points, so the compressed input is
    ///    never materialized. The decoded layer is bounded per layer and cumulatively exactly like
    ///    the in-memory dispatcher; any deeper wrapper is handled by that same bounded state
    ///    machine after the outer layer has been decoded.
    /// 3. Otherwise the finally dispatched layer is materialized, because every container and leaf
    ///    parser in this port reads contiguous bytes.
    pub fn read_path_streaming(
        path: impl AsRef<Path>,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'static>, SchemeError> {
        let mut stream = open_logical_file(path)
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        Self::read_stream(&mut stream, limits)
    }

    /// Read and dispatch a seekable stream through the same bounded streaming path as a local
    /// file. Outer GZip/Brotli wrappers are decoded from the stream without first copying their
    /// compressed input; the final container or leaf is materialized under `max_input_bytes`
    /// because the current format parsers consume contiguous byte slices.
    pub fn read_stream<R: Read + Seek>(
        stream: &mut R,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'static>, SchemeError> {
        let stream_length = stream
            .seek(SeekFrom::End(0))
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        if stream_length > u64::try_from(limits.max_input_bytes).unwrap_or(u64::MAX) {
            return Err(SchemeError::new(SchemeErrorKind::InputSizeLimitExceeded {
                requested: stream_length,
                limit: limits.max_input_bytes,
            }));
        }

        let prefix = read_stream_prefix(stream, stream_length)?;
        if let Some(kind) = detect_primary_file_kind_with_length(&prefix, stream_length) {
            let contents = Self::materialize_stream(stream, stream_length)?;
            return Ok(DispatchedFile {
                contents: Cow::Owned(contents),
                kind,
                wrappers: Vec::new(),
                limits,
            });
        }

        let wrapper_kind = if has_gzip_magic(&prefix) {
            Some(WrapperKind::Gzip)
        } else if has_brotli_file_signature(&prefix) {
            Some(WrapperKind::Brotli)
        } else {
            None
        };
        let Some(wrapper_kind) = wrapper_kind else {
            let contents = Self::materialize_stream(stream, stream_length)?;
            let kind = detect_file_kind_with_length(&contents, stream_length);
            return Ok(DispatchedFile {
                contents: Cow::Owned(contents),
                kind,
                wrappers: Vec::new(),
                limits,
            });
        };

        if limits.max_wrapper_depth == 0 {
            return Err(SchemeError::new(SchemeErrorKind::WrapperDepthExceeded {
                limit: 0,
            }));
        }
        stream
            .seek(SeekFrom::Start(0))
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        let output = match wrapper_kind {
            WrapperKind::Gzip => {
                decompress_gzip_stream(&mut *stream, limits.max_decompressed_bytes_per_layer)
            }
            WrapperKind::Brotli => {
                decompress_brotli_stream(&mut *stream, limits.max_decompressed_bytes_per_layer)
            }
        }
        .map_err(|error| {
            SchemeError::new(SchemeErrorKind::WrapperDecode {
                kind: wrapper_kind,
                source: error,
            })
        })?;
        if output.len() > limits.max_total_decompressed_bytes {
            return Err(SchemeError::new(
                SchemeErrorKind::TotalDecompressionLimitExceeded {
                    requested: output.len(),
                    limit: limits.max_total_decompressed_bytes,
                },
            ));
        }
        let input_size = usize::try_from(stream_length).map_err(|_| {
            SchemeError::new(SchemeErrorKind::InputLengthNotRepresentable {
                length: stream_length,
            })
        })?;
        let output_size = output.len();
        let layer = WrapperLayer {
            kind: wrapper_kind,
            input_size,
            output_size,
        };
        Self::read_cow_with_state(Cow::Owned(output), limits, vec![layer], output_size)
    }

    /// Check whether a seekable input is a direct UnityFS container without reading its body.
    fn is_direct_unityfs_stream<R: Read + Seek>(
        stream: &mut R,
        limits: SchemeReaderLimits,
    ) -> Result<bool, SchemeError> {
        Ok(probe_direct_stream_kind(stream, limits)? == Some(FileKind::UnityFs))
    }

    /// Materialize the remaining seekable layer into bounded storage.
    fn materialize_stream<R: Read + Seek>(
        stream: &mut R,
        length: u64,
    ) -> Result<Vec<u8>, SchemeError> {
        let length = usize::try_from(length).map_err(|_| {
            SchemeError::new(SchemeErrorKind::InputLengthNotRepresentable { length })
        })?;
        stream
            .seek(SeekFrom::Start(0))
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        let mut contents = Vec::new();
        contents.try_reserve_exact(length).map_err(|_| {
            SchemeError::new(SchemeErrorKind::InputAllocationFailed { requested: length })
        })?;
        contents.resize(length, 0);
        stream
            .read_exact(&mut contents)
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        Ok(contents)
    }

    /// Read one local regular or split-file input and mount it into the virtual filesystem.
    ///
    /// Containers are expanded through `mount_container_tree`; standalone resources and
    /// SerializedFiles are mounted under their logical input filename. Directory and batch
    /// discovery are handled by a higher-level caller.
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
        let dispatched = Self::read_stream(&mut stream, scheme_limits).map_err(|error| {
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
            let dispatched = if direct_unityfs {
                None
            } else {
                Some(
                    Self::read_stream(&mut stream, scheme_limits).map_err(|error| {
                        BatchMountError::new(
                            report.clone(),
                            Some(input.path.clone()),
                            BatchMountErrorKind::InputMount(Box::new(ContainerMountError::new(
                                ContainerMountReport::default(),
                                ContainerMountErrorKind::Scheme(error),
                            ))),
                        )
                    })?,
                )
            };
            let root_kind = dispatched
                .as_ref()
                .map(DispatchedFile::kind)
                .unwrap_or(FileKind::UnityFs);
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

    /// Check only scheme signatures, matching `SchemeReader.IsReadableFile`; this does not decode
    /// or fully parse the input. UnityArchive is considered readable here because the baseline
    /// registers its detector even though its `Read` implementation is unsupported.
    pub fn is_readable(input: &[u8]) -> bool {
        detect_primary_file_kind(input).is_some()
            || has_brotli_file_signature(input)
            || has_gzip_magic(input)
            || is_serialized_file(input)
    }

    pub fn read_with_limits(
        input: &[u8],
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'_>, SchemeError> {
        Self::read_cow_with_limits(Cow::Borrowed(input), limits)
    }

    fn read_cow_with_limits<'a>(
        contents: Cow<'a, [u8]>,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'a>, SchemeError> {
        Self::read_cow_with_state(contents, limits, Vec::new(), 0)
    }

    /// The in-memory dispatcher, continued from an already-recorded wrapper state.
    ///
    /// [`Self::read_path_streaming`] uses this to hand a streamed wrapper layer to the same
    /// detection/limit logic without re-decoding it or double-counting its bytes.
    fn read_cow_with_state<'a>(
        mut contents: Cow<'a, [u8]>,
        limits: SchemeReaderLimits,
        mut wrappers: Vec<WrapperLayer>,
        mut total_decompressed: usize,
    ) -> Result<DispatchedFile<'a>, SchemeError> {
        if contents.len() > limits.max_input_bytes {
            return Err(SchemeError::new(SchemeErrorKind::InputSizeLimitExceeded {
                requested: u64::try_from(contents.len()).unwrap_or(u64::MAX),
                limit: limits.max_input_bytes,
            }));
        }

        loop {
            let current = contents.as_ref();
            if let Some(kind) = detect_primary_file_kind(current) {
                return Ok(DispatchedFile {
                    contents,
                    kind,
                    wrappers,
                    limits,
                });
            }
            let Some((kind, output)) = decode_wrapper(current, limits, wrappers.len())? else {
                let kind = detect_file_kind(current);
                return Ok(DispatchedFile {
                    contents,
                    kind,
                    wrappers,
                    limits,
                });
            };

            let requested_total =
                total_decompressed
                    .checked_add(output.len())
                    .ok_or_else(|| {
                        SchemeError::new(SchemeErrorKind::TotalDecompressedLengthOverflow)
                    })?;
            if requested_total > limits.max_total_decompressed_bytes {
                return Err(SchemeError::new(
                    SchemeErrorKind::TotalDecompressionLimitExceeded {
                        requested: requested_total,
                        limit: limits.max_total_decompressed_bytes,
                    },
                ));
            }
            wrappers.try_reserve(1).map_err(|_| {
                SchemeError::new(SchemeErrorKind::AllocationFailed {
                    requested: wrappers.len().saturating_add(1),
                })
            })?;
            wrappers.push(WrapperLayer {
                kind,
                input_size: current.len(),
                output_size: output.len(),
            });
            total_decompressed = requested_total;
            contents = Cow::Owned(output);
        }
    }
}

/// The bounded prefix read from a seekable logical input for scheme detection.
///
/// 64 bytes cover every detection input: the 0x20-byte bundle-header stream minimum, the WebData
/// signature, and the Brotli marker's bit-level header plus its 34-byte signature.
const STREAM_PROBE_BYTES: usize = 64;

/// Probe a bounded prefix of a seekable logical file without materializing the whole input.
fn read_stream_prefix<R: Read + Seek>(stream: &mut R, length: u64) -> Result<Vec<u8>, SchemeError> {
    let prefix_length = usize::try_from(length)
        .unwrap_or(usize::MAX)
        .min(STREAM_PROBE_BYTES);
    stream
        .seek(SeekFrom::Start(0))
        .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
    let mut prefix = vec![0u8; prefix_length];
    stream
        .read_exact(&mut prefix)
        .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
    Ok(prefix)
}

/// Determine the primary file kind from a seekable stream without reading its body.
/// A wrapper returns `None` so callers that need its decoded leaf can use the full dispatcher.
fn probe_direct_stream_kind<R: Read + Seek>(
    stream: &mut R,
    limits: SchemeReaderLimits,
) -> Result<Option<FileKind>, SchemeError> {
    let stream_length = stream
        .seek(SeekFrom::End(0))
        .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
    if stream_length > u64::try_from(limits.max_input_bytes).unwrap_or(u64::MAX) {
        return Err(SchemeError::new(SchemeErrorKind::InputSizeLimitExceeded {
            requested: stream_length,
            limit: limits.max_input_bytes,
        }));
    }
    let prefix = read_stream_prefix(stream, stream_length)?;
    if let Some(kind) = detect_primary_file_kind_with_length(&prefix, stream_length) {
        return Ok(Some(kind));
    }
    if is_serialized_file_with_length(&prefix, stream_length) {
        return Ok(Some(FileKind::SerializedFile));
    }
    if has_gzip_magic(&prefix) || has_brotli_file_signature(&prefix) {
        return Ok(None);
    }
    Ok(Some(FileKind::Resource))
}

fn decode_wrapper(
    input: &[u8],
    limits: SchemeReaderLimits,
    depth: usize,
) -> Result<Option<(WrapperKind, Vec<u8>)>, SchemeError> {
    let kind = if has_gzip_magic(input) {
        Some(WrapperKind::Gzip)
    } else if has_brotli_file_signature(input) {
        Some(WrapperKind::Brotli)
    } else {
        None
    };
    let Some(kind) = kind else {
        return Ok(None);
    };
    if depth >= limits.max_wrapper_depth {
        return Err(SchemeError::new(SchemeErrorKind::WrapperDepthExceeded {
            limit: limits.max_wrapper_depth,
        }));
    }

    let output = match kind {
        WrapperKind::Gzip => decompress_gzip(input, limits.max_decompressed_bytes_per_layer),
        WrapperKind::Brotli => decompress_brotli(input, limits.max_decompressed_bytes_per_layer),
    }
    .map_err(|error| {
        SchemeError::new(SchemeErrorKind::WrapperDecode {
            kind,
            source: error,
        })
    })?;
    Ok(Some((kind, output)))
}

/// Match the effective enumeration order of the source `Stack<IScheme>`.
///
/// The source collection initializer pushes SerializedFile first and FileStreamBundle last;
/// `Stack<T>` enumerates from its top, so the actual precedence starts with UnityFS.
fn detect_file_kind(input: &[u8]) -> FileKind {
    detect_file_kind_with_length(input, input.len() as u64)
}

fn detect_file_kind_with_length(input: &[u8], total_length: u64) -> FileKind {
    if let Some(kind) = detect_primary_file_kind_with_length(input, total_length) {
        return kind;
    }
    if is_serialized_file_with_length(input, total_length) {
        FileKind::SerializedFile
    } else {
        FileKind::Resource
    }
}

fn detect_primary_file_kind(input: &[u8]) -> Option<FileKind> {
    detect_primary_file_kind_with_length(input, input.len() as u64)
}

fn detect_primary_file_kind_with_length(input: &[u8], total_length: u64) -> Option<FileKind> {
    if has_bundle_signature_with_length(input, b"UnityFS", total_length) {
        Some(FileKind::UnityFs)
    } else if has_bundle_signature_with_length(input, b"UnityRaw", total_length) {
        Some(FileKind::UnityRaw)
    } else if has_bundle_signature_with_length(input, b"UnityWeb", total_length) {
        Some(FileKind::UnityWebBundle)
    } else if has_bundle_signature_with_length(input, b"UnityArchive", total_length) {
        Some(FileKind::UnityArchive)
    } else if WebFile::is_web_file(input) {
        Some(FileKind::UnityWebData)
    } else {
        None
    }
}

/// The source `BundleHeader.IsBundleHeader` minimum applies to the whole stream length,
/// not the buffered prefix: a short prefix of a long stream still detects when it holds
/// the magic and its terminator. Full-buffer callers pass the buffer length, which keeps
/// their behavior unchanged.
fn has_bundle_signature_with_length(input: &[u8], magic: &[u8], total_length: u64) -> bool {
    const SOURCE_MINIMUM_STREAM_LENGTH: u64 = 0x20;
    total_length >= SOURCE_MINIMUM_STREAM_LENGTH
        && input.get(..magic.len()) == Some(magic)
        && input.get(magic.len()) == Some(&0)
}

/// Source-compatible subset of `SerializedFileHeader.IsSerializedFileHeader`.
fn is_serialized_file(input: &[u8]) -> bool {
    is_serialized_file_with_length(input, input.len() as u64)
}

fn is_serialized_file_with_length(input: &[u8], total_length: u64) -> bool {
    if input.len() < 16 {
        return false;
    }
    let legacy_metadata_size = i32::from_be_bytes(input[0..4].try_into().expect("fixed slice"));
    let legacy_file_size = u32::from_be_bytes(input[4..8].try_into().expect("fixed slice"));
    let raw_version = i32::from_be_bytes(input[8..12].try_into().expect("fixed slice"));
    let Ok(version) = FormatVersion::try_from(raw_version) else {
        return false;
    };

    let (metadata_size, file_size) = if version.has_large_file_support() {
        let Some(metadata) = input.get(20..24) else {
            return false;
        };
        let Some(size) = input.get(24..32) else {
            return false;
        };
        (
            i32::from_be_bytes(metadata.try_into().expect("fixed slice")),
            u64::from_be_bytes(size.try_into().expect("fixed slice")),
        )
    } else {
        (legacy_metadata_size, u64::from(legacy_file_size))
    };

    metadata_size >= 13 && file_size >= 16 + 13 && total_length == file_size
}

#[derive(Debug)]
pub struct SchemeError {
    kind: SchemeErrorKind,
}

impl SchemeError {
    fn new(kind: SchemeErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> &SchemeErrorKind {
        &self.kind
    }
}

impl Display for SchemeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.kind, formatter)
    }
}

impl Error for SchemeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            SchemeErrorKind::StreamInput(error) => Some(error),
            SchemeErrorKind::WrapperDecode { source, .. } => Some(source),
            SchemeErrorKind::UnityFs(error) => Some(error),
            SchemeErrorKind::RawWeb(error) => Some(error),
            SchemeErrorKind::WebFile(error) => Some(error),
            SchemeErrorKind::SerializedFile(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum SchemeErrorKind {
    InputSizeLimitExceeded {
        requested: u64,
        limit: usize,
    },
    InputLengthNotRepresentable {
        length: u64,
    },
    InputAllocationFailed {
        requested: usize,
    },
    StreamInput(std::io::Error),
    WrapperDepthExceeded {
        limit: usize,
    },
    TotalDecompressionLimitExceeded {
        requested: usize,
        limit: usize,
    },
    TotalDecompressedLengthOverflow,
    NotWebDataContainer,
    NotContainer,
    WebDataEntryIndexOutOfBounds {
        index: usize,
        entry_count: usize,
    },
    AllocationFailed {
        requested: usize,
    },
    WrapperDecode {
        kind: WrapperKind,
        source: WrapperCodecError,
    },
    UnityFs(UnityFsError),
    RawWeb(RawWebError),
    WebFile(WebFileError),
    SerializedFile(SerializedFileError),
    DetectedKindChanged {
        detected: FileKind,
        parsed: RawWebKind,
    },
    ArchiveReadUnsupported,
}

impl Display for SchemeErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputSizeLimitExceeded { requested, limit } => write!(
                formatter,
                "input stream has {requested} bytes, exceeding configured limit {limit}"
            ),
            Self::InputLengthNotRepresentable { length } => write!(
                formatter,
                "input stream length {length} cannot be represented on this platform"
            ),
            Self::InputAllocationFailed { requested } => {
                write!(formatter, "could not allocate {requested} input bytes")
            }
            Self::StreamInput(error) => write!(formatter, "input stream: {error}"),
            Self::WrapperDepthExceeded { limit } => {
                write!(formatter, "compressed wrapper nesting exceeds limit {limit}")
            }
            Self::TotalDecompressionLimitExceeded { requested, limit } => write!(
                formatter,
                "nested wrappers decoded {requested} total bytes, exceeding limit {limit}"
            ),
            Self::TotalDecompressedLengthOverflow => {
                write!(formatter, "total decompressed byte count overflows")
            }
            Self::NotWebDataContainer => {
                write!(formatter, "cannot dispatch an entry from a non-WebData file")
            }
            Self::NotContainer => {
                write!(formatter, "cannot dispatch an entry from a non-container file")
            }
            Self::WebDataEntryIndexOutOfBounds { index, entry_count } => write!(
                formatter,
                "WebData entry index {index} is outside the {entry_count} parsed entries"
            ),
            Self::AllocationFailed { requested } => {
                write!(formatter, "could not allocate {requested} wrapper records")
            }
            Self::WrapperDecode { kind, source } => {
                write!(formatter, "{kind:?} wrapper: {source}")
            }
            Self::UnityFs(error) => write!(formatter, "UnityFS parser: {error}"),
            Self::RawWeb(error) => write!(formatter, "UnityRaw/UnityWeb parser: {error}"),
            Self::WebFile(error) => write!(formatter, "Unity WebData parser: {error}"),
            Self::SerializedFile(error) => write!(formatter, "SerializedFile parser: {error}"),
            Self::DetectedKindChanged { detected, parsed } => write!(
                formatter,
                "scheme detector selected {detected:?}, but the parsed raw/web signature was {parsed:?}"
            ),
            Self::ArchiveReadUnsupported => write!(
                formatter,
                "UnityArchive is recognized by the source baseline, whose reader is unsupported"
            ),
        }
    }
}
