use super::*;

impl SerializedFileWorkspace {
    /// Build a resource-only workspace for cross-module reference-fixture tests.
    #[cfg(test)]
    pub(crate) fn with_resource_facts_for_test(
        container_bundles: Vec<WorkspaceSerializedBundle>,
        resource_files: Vec<WorkspaceResourceFile>,
    ) -> Self {
        Self {
            container_bundles,
            resource_files,
            ..Self::default()
        }
    }

    /// Parse caller-identified SerializedFiles from the VFS and build their per-file reference
    /// graphs. All file sizes are preflighted before any file contents are read.
    pub fn load_from_vfs(
        filesystem: &VirtualFileSystem,
        inputs: &[DependencyFilePath],
        limits: SerializedFileWorkspaceLimits,
    ) -> Result<Self, Box<SerializedFileWorkspaceError>> {
        Self::load_from_vfs_inner(
            filesystem,
            inputs,
            limits,
            crate::processing::asset_factory::reference_class_layouts(),
        )
    }

    pub fn load_from_vfs_with_layout_source(
        filesystem: &VirtualFileSystem,
        inputs: &[DependencyFilePath],
        limits: SerializedFileWorkspaceLimits,
        layouts: &dyn crate::processing::asset_factory::ClassLayoutSource,
    ) -> Result<Self, Box<SerializedFileWorkspaceError>> {
        Self::load_from_vfs_inner(filesystem, inputs, limits, layouts)
    }

    pub(super) fn load_from_vfs_inner(
        filesystem: &VirtualFileSystem,
        inputs: &[DependencyFilePath],
        limits: SerializedFileWorkspaceLimits,
        layouts: &dyn crate::processing::asset_factory::ClassLayoutSource,
    ) -> Result<Self, Box<SerializedFileWorkspaceError>> {
        let mut report = SerializedFileWorkspaceReport {
            input_files: inputs.len(),
            ..SerializedFileWorkspaceReport::default()
        };
        if inputs.len() > limits.max_files {
            return Err(workspace_error(
                report,
                None,
                SerializedFileWorkspaceErrorKind::FileCountLimitExceeded {
                    count: inputs.len(),
                    limit: limits.max_files,
                },
            ));
        }

        let mut seen_paths = BTreeSet::new();
        let mut total_name_bytes = 0usize;
        for (index, input) in inputs.iter().enumerate() {
            if input.file_name.len() > limits.max_file_name_bytes {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::FileNameTooLong {
                        input_index: index,
                        byte_count: input.file_name.len(),
                        limit: limits.max_file_name_bytes,
                    },
                ));
            }
            if !seen_paths.insert(input.virtual_path.as_str()) {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::DuplicateVirtualPath,
                ));
            }
            let normalized_name = fix_file_identifier(&input.file_name);
            if normalized_name.len() > limits.max_file_name_bytes {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::FileNameTooLong {
                        input_index: index,
                        byte_count: normalized_name.len(),
                        limit: limits.max_file_name_bytes,
                    },
                ));
            }
            total_name_bytes = total_name_bytes
                .checked_add(normalized_name.len())
                .ok_or_else(|| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::FileNameByteCountOverflow,
                    )
                })?;
            report.total_file_name_bytes = total_name_bytes;
            if total_name_bytes > limits.max_total_file_name_bytes {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::TotalFileNameBytesLimitExceeded {
                        byte_count: total_name_bytes,
                        limit: limits.max_total_file_name_bytes,
                    },
                ));
            }

            let stream = filesystem
                .open_file(&input.virtual_path)
                .map_err(|source| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::FileSystem(source),
                    )
                })?;
            let file_bytes = stream.length().map_err(|source| {
                workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::FileSystem(source),
                )
            })?;
            let scheme_file_limit =
                u64::try_from(limits.scheme.max_input_bytes).unwrap_or(u64::MAX);
            let effective_file_limit = limits.max_file_bytes.min(scheme_file_limit);
            if file_bytes > effective_file_limit {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::FileSizeLimitExceeded {
                        file_bytes,
                        limit: effective_file_limit,
                    },
                ));
            }
            report.total_input_bytes = report
                .total_input_bytes
                .checked_add(file_bytes)
                .ok_or_else(|| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::InputByteCountOverflow,
                    )
                })?;
            if report.total_input_bytes > limits.max_total_input_bytes {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::TotalInputLimitExceeded {
                        requested_total: report.total_input_bytes,
                        limit: limits.max_total_input_bytes,
                    },
                ));
            }
        }

        let mut files = Vec::new();
        files.try_reserve_exact(inputs.len()).map_err(|_| {
            workspace_error(
                report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "loaded SerializedFiles",
                    requested: inputs.len(),
                },
            )
        })?;

        for input in inputs {
            let max_file_bytes = usize::try_from(limits.max_file_bytes).unwrap_or(usize::MAX);
            let bytes = filesystem
                .read_all_bytes(&input.virtual_path, max_file_bytes)
                .map_err(|source| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::FileSystem(source),
                    )
                })?;
            let dispatched =
                SchemeReader::read_with_limits(&bytes, limits.scheme).map_err(|source| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::Scheme(source),
                    )
                })?;
            if dispatched.kind() != FileKind::SerializedFile {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::NotSerializedFile {
                        detected_kind: dispatched.kind(),
                    },
                ));
            }
            let (header, metadata, reference_graph) = {
                let ParsedFile::SerializedFile(parsed) = dispatched.parse().map_err(|source| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::Scheme(source),
                    )
                })?
                else {
                    return Err(workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::NotSerializedFile {
                            detected_kind: dispatched.kind(),
                        },
                    ));
                };
                let graph = ObjectReferenceGraph::build_with_source_type_tree_fallback(
                    &parsed,
                    limits.reference_graph,
                    &input.file_name,
                    &input.virtual_path,
                    layouts,
                )
                .map_err(|source| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::ReferenceGraph(source),
                    )
                })?;
                (parsed.header, parsed.metadata, graph)
            };

            let next_total_objects = report
                .total_objects
                .checked_add(reference_graph.object_count())
                .ok_or_else(|| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::ObjectCountOverflow,
                    )
                })?;
            if next_total_objects > limits.max_total_objects {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::TotalObjectLimitExceeded {
                        requested_total: next_total_objects,
                        limit: limits.max_total_objects,
                    },
                ));
            }
            let next_total_edges = report
                .total_reference_edges
                .checked_add(reference_graph.references().len())
                .ok_or_else(|| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::ReferenceCountOverflow,
                    )
                })?;
            if next_total_edges > limits.max_total_reference_edges {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::TotalReferenceLimitExceeded {
                        requested_total: next_total_edges,
                        limit: limits.max_total_reference_edges,
                    },
                ));
            }

            report.completed_input_bytes = report
                .completed_input_bytes
                .checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
                .ok_or_else(|| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::InputByteCountOverflow,
                    )
                })?;
            report.completed_files += 1;
            report.total_objects = next_total_objects;
            report.total_reference_edges = next_total_edges;
            files.push(LoadedSerializedFile {
                file_name: input.file_name.clone(),
                virtual_path: input.virtual_path.clone(),
                container_scope: None,
                container_entry_index: None,
                container_collection_index: None,
                container_bundle_index: None,
                header,
                metadata,
                reference_graph,
            });
        }

        Ok(Self {
            files,
            container_bundles: Vec::new(),
            resource_files: Vec::new(),
            failed_container_entries: Vec::new(),
            failed_container_metadata_bytes: 0,
            report,
        })
    }

    /// Load every uniquely resolved SerializedFile reachable through external tables.
    ///
    /// The dependency registry is built from the currently mounted VFS file paths plus caller
    /// supplied source identities. DataPaths retain their source ordering. Unique dependency
    /// containers are recursively mounted beneath isolated VFS roots and their SerializedFile
    /// leaves join the same external-dependency walk.
    pub fn load_from_vfs_with_dependencies(
        filesystem: &mut VirtualFileSystem,
        inputs: &[DependencyFilePath],
        registered_files: &[DependencyFilePath],
        data_paths: &[String],
        limits: SerializedFileWorkspaceLimits,
        index_limits: DependencyIndexLimits,
        container_limits: ContainerMountLimits,
    ) -> Result<(Self, SerializedFileDependencyReport), Box<SerializedFileWorkspaceError>> {
        Self::load_from_vfs_with_preexpanded_containers_inner(
            filesystem,
            inputs,
            registered_files,
            SerializedFileDependencyLoadOptions {
                data_paths,
                limits,
                index_limits,
                container_limits,
                preexpanded_containers: &[],
            },
            crate::processing::asset_factory::reference_class_layouts(),
        )
    }

    pub fn load_from_vfs_with_dependencies_and_layout_source(
        filesystem: &mut VirtualFileSystem,
        inputs: &[DependencyFilePath],
        registered_files: &[DependencyFilePath],
        options: SerializedFileWorkspaceLoadOptions<'_>,
    ) -> Result<(Self, SerializedFileDependencyReport), Box<SerializedFileWorkspaceError>> {
        Self::load_from_vfs_with_preexpanded_containers_inner(
            filesystem,
            inputs,
            registered_files,
            SerializedFileDependencyLoadOptions {
                data_paths: options.data_paths,
                limits: options.limits,
                index_limits: options.index_limits,
                container_limits: options.container_limits,
                preexpanded_containers: options.preexpanded_containers,
            },
            options.layouts,
        )
    }

    pub(crate) fn load_from_vfs_with_preexpanded_containers_and_layout_source(
        filesystem: &mut VirtualFileSystem,
        inputs: &[DependencyFilePath],
        registered_files: &[DependencyFilePath],
        options: SerializedFileDependencyLoadOptions<'_>,
        layouts: &dyn crate::processing::asset_factory::ClassLayoutSource,
    ) -> Result<(Self, SerializedFileDependencyReport), Box<SerializedFileWorkspaceError>> {
        Self::load_from_vfs_with_preexpanded_containers_inner(
            filesystem,
            inputs,
            registered_files,
            options,
            layouts,
        )
    }

    fn load_from_vfs_with_preexpanded_containers_inner(
        filesystem: &mut VirtualFileSystem,
        inputs: &[DependencyFilePath],
        registered_files: &[DependencyFilePath],
        options: SerializedFileDependencyLoadOptions<'_>,
        layouts: &dyn crate::processing::asset_factory::ClassLayoutSource,
    ) -> Result<(Self, SerializedFileDependencyReport), Box<SerializedFileWorkspaceError>> {
        let SerializedFileDependencyLoadOptions {
            data_paths,
            limits,
            index_limits,
            container_limits,
            preexpanded_containers,
        } = options;
        let mut workspace = Self::load_from_vfs_inner(filesystem, inputs, limits, layouts)?;
        let mut dynamic_registered_files = Vec::new();
        dynamic_registered_files
            .try_reserve_exact(registered_files.len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    None,
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "dependency source identities",
                        requested: registered_files.len(),
                    },
                )
            })?;
        dynamic_registered_files.extend_from_slice(registered_files);
        let mut registry =
            dependency_registry(filesystem, inputs, &dynamic_registered_files, index_limits)
                .map_err(|kind| workspace_error(workspace.report, None, kind))?;
        let mut index = Some(
            DependencyIndex::with_limits(filesystem, &registry, data_paths, index_limits).map_err(
                |error| {
                    workspace_error(
                        workspace.report,
                        None,
                        SerializedFileWorkspaceErrorKind::DependencyIndexBuild(Box::new(error)),
                    )
                },
            )?,
        );
        let mut loaded_names = HashMap::<String, (usize, usize)>::new();
        loaded_names
            .try_reserve(workspace.files.len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    None,
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "loaded dependency identities",
                        requested: workspace.files.len(),
                    },
                )
            })?;
        let mut loaded_paths = HashMap::<String, usize>::new();
        loaded_paths
            .try_reserve(workspace.files.len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    None,
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "loaded dependency paths",
                        requested: workspace.files.len(),
                    },
                )
            })?;
        for (file_index, file) in workspace.files.iter().enumerate() {
            let name = fix_file_identifier(&file.file_name);
            loaded_names
                .entry(name)
                .and_modify(|(_, count)| *count += 1)
                .or_insert((file_index, 1));
            loaded_paths.insert(file.virtual_path.clone(), file_index);
        }
        let mut dependency_report = SerializedFileDependencyReport::default();
        let mut total_dependency_name_bytes = 0usize;
        let mut total_candidate_path_bytes = 0usize;
        let mut total_container_input_bytes = 0u64;
        let mut total_container_entries = 0usize;
        let mut total_container_output_bytes = 0u64;
        if preexpanded_containers.len() > limits.max_files {
            return Err(workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::PreExpandedContainerCountLimitExceeded {
                    count: preexpanded_containers.len(),
                    limit: limits.max_files,
                },
            ));
        }
        let mut expanded_containers = HashMap::<String, usize>::new();
        expanded_containers
            .try_reserve(preexpanded_containers.len())
            .map_err(|_| {
                workspace_error(
                    workspace.report,
                    None,
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "pre-expanded container identities",
                        requested: preexpanded_containers.len(),
                    },
                )
            })?;
        for container in preexpanded_containers {
            if expanded_containers
                .insert(
                    container.virtual_path.clone(),
                    container.serialized_files_loaded,
                )
                .is_some()
            {
                return Err(workspace_error(
                    workspace.report,
                    Some(&container.virtual_path),
                    SerializedFileWorkspaceErrorKind::DuplicatePreExpandedContainerPath,
                ));
            }
        }
        let max_container_scope_count = limits
            .max_files
            .checked_add(limits.max_total_dependency_container_entries)
            .ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    None,
                    SerializedFileWorkspaceErrorKind::ContainerScopeCountOverflow,
                )
            })?;
        let max_container_bundle_count = limits
            .max_files
            .checked_add(limits.max_dependency_requests)
            .and_then(|count| count.checked_add(limits.max_total_dependency_container_entries))
            .ok_or_else(|| {
                workspace_error(
                    workspace.report,
                    None,
                    SerializedFileWorkspaceErrorKind::ContainerBundleCountOverflow,
                )
            })?;
        let mut initial_scope_count = 0usize;
        for container in preexpanded_containers {
            initial_scope_count = initial_scope_count
                .checked_add(container.container_scopes.len())
                .ok_or_else(|| {
                    workspace_error(
                        workspace.report,
                        Some(&container.virtual_path),
                        SerializedFileWorkspaceErrorKind::ContainerScopeCountOverflow,
                    )
                })?;
        }
        let initial_scope_index = build_container_scope_index(
            preexpanded_containers
                .iter()
                .flat_map(|container| container.container_scopes.iter()),
            initial_scope_count,
            max_container_scope_count,
            workspace.report,
            None,
        )?;
        let initial_container_file_order = build_mounted_file_order_index(
            preexpanded_containers.iter().map(|container| {
                (
                    container.container_scopes.as_slice(),
                    container.serialized_files.as_slice(),
                )
            }),
            limits.max_files,
            workspace.report,
        )?;
        let mut total_bundle_node_path_bytes = 0usize;
        let mut total_bundle_resource_file_path_bytes = 0usize;
        for container in preexpanded_containers {
            append_mounted_container_bundles(
                &mut workspace,
                MountedContainerBundleInput {
                    container_scopes: &container.container_scopes,
                    mounted_bundles: &container.container_bundles,
                    serialized_files: &container.serialized_files,
                    resource_files: &container.resource_files,
                    failed_entries: &container.failed_entries,
                    source_container_path: &container.virtual_path,
                    loaded_paths: &loaded_paths,
                    limits,
                    bundle_count_limit: max_container_bundle_count,
                },
                &mut total_bundle_node_path_bytes,
                &mut total_bundle_resource_file_path_bytes,
            )?;
        }
        let mut total_bundle_scope_bytes = 0usize;
        for file in &mut workspace.files {
            attach_container_scope(
                file,
                &initial_scope_index,
                &mut total_bundle_scope_bytes,
                limits,
                workspace.report,
            )?;
            attach_container_file_order(file, &initial_container_file_order, workspace.report)?;
        }
        let mut next_mount_namespace = 0usize;
        let mut source_file_index = 0usize;

        while source_file_index < workspace.files.len() {
            let external_count = workspace.files[source_file_index].metadata.externals.len();
            for external_index in 0..external_count {
                let next_request_count = dependency_report
                    .requests
                    .len()
                    .checked_add(1)
                    .ok_or_else(|| {
                        workspace_error(
                            workspace.report,
                            Some(&workspace.files[source_file_index].virtual_path),
                            SerializedFileWorkspaceErrorKind::DependencyRequestCountOverflow,
                        )
                    })?;
                if next_request_count > limits.max_dependency_requests {
                    return Err(workspace_error(
                        workspace.report,
                        Some(&workspace.files[source_file_index].virtual_path),
                        SerializedFileWorkspaceErrorKind::DependencyRequestLimitExceeded {
                            count: next_request_count,
                            limit: limits.max_dependency_requests,
                        },
                    ));
                }

                let identifier =
                    workspace.files[source_file_index].metadata.externals[external_index].clone();
                let dependency_name = identifier.fixed_path_name();
                total_dependency_name_bytes = total_dependency_name_bytes
                    .checked_add(dependency_name.len())
                    .ok_or_else(|| {
                        workspace_error(
                            workspace.report,
                            Some(&workspace.files[source_file_index].virtual_path),
                            SerializedFileWorkspaceErrorKind::DependencyNameByteCountOverflow,
                        )
                    })?;
                if total_dependency_name_bytes > limits.max_total_dependency_name_bytes {
                    return Err(workspace_error(
                        workspace.report,
                        Some(&workspace.files[source_file_index].virtual_path),
                        SerializedFileWorkspaceErrorKind::TotalDependencyNameBytesLimitExceeded {
                            byte_count: total_dependency_name_bytes,
                            limit: limits.max_total_dependency_name_bytes,
                        },
                    ));
                }

                let outcome = match loaded_names.get(&dependency_name).copied() {
                    Some((file_index, 1)) => {
                        SerializedFileDependencyOutcome::AlreadyLoaded { file_index }
                    }
                    Some((_, candidate_count)) => {
                        SerializedFileDependencyOutcome::AmbiguousLoadedIdentity { candidate_count }
                    }
                    None => match index
                        .as_ref()
                        .ok_or_else(|| {
                            workspace_error(
                                workspace.report,
                                Some(&workspace.files[source_file_index].virtual_path),
                                SerializedFileWorkspaceErrorKind::DependencyIndexUnavailable,
                            )
                        })?
                        .resolve(&identifier)
                        .map_err(|error| {
                            workspace_error(
                                workspace.report,
                                Some(&workspace.files[source_file_index].virtual_path),
                                SerializedFileWorkspaceErrorKind::DependencyLookup(Box::new(error)),
                            )
                        })? {
                        DependencyLookup::Missing { .. } => {
                            SerializedFileDependencyOutcome::Missing
                        }
                        DependencyLookup::Ambiguous { candidates, .. } => {
                            const REPORTED_CANDIDATE_PATHS: usize = 32;
                            let candidate_count = candidates.len();
                            let mut candidate_paths = Vec::new();
                            let report_count = candidate_count.min(REPORTED_CANDIDATE_PATHS);
                            candidate_paths
                                .try_reserve_exact(report_count)
                                .map_err(|_| {
                                    workspace_error(
                                        workspace.report,
                                        Some(&workspace.files[source_file_index].virtual_path),
                                        SerializedFileWorkspaceErrorKind::AllocationFailed {
                                            resource: "ambiguous dependency candidate paths",
                                            requested: report_count,
                                        },
                                    )
                                })?;
                            for candidate in candidates.iter().take(REPORTED_CANDIDATE_PATHS) {
                                total_candidate_path_bytes = total_candidate_path_bytes
                                    .checked_add(candidate.virtual_path.len())
                                    .ok_or_else(|| {
                                        workspace_error(
                                            workspace.report,
                                            Some(
                                                &workspace.files[source_file_index].virtual_path,
                                            ),
                                            SerializedFileWorkspaceErrorKind::DependencyCandidatePathByteCountOverflow,
                                        )
                                    })?;
                                if total_candidate_path_bytes
                                    > limits.max_total_dependency_candidate_path_bytes
                                {
                                    return Err(workspace_error(
                                        workspace.report,
                                        Some(
                                            &workspace.files[source_file_index].virtual_path,
                                        ),
                                        SerializedFileWorkspaceErrorKind::TotalDependencyCandidatePathBytesLimitExceeded {
                                            byte_count: total_candidate_path_bytes,
                                            limit: limits
                                                .max_total_dependency_candidate_path_bytes,
                                        },
                                    ));
                                }
                                candidate_paths.push(candidate.virtual_path.clone());
                            }
                            SerializedFileDependencyOutcome::Ambiguous {
                                candidate_count,
                                candidate_paths,
                            }
                        }
                        DependencyLookup::Found(candidate) => {
                            if let Some(file_index) =
                                loaded_paths.get(&candidate.virtual_path).copied()
                            {
                                SerializedFileDependencyOutcome::AlreadyLoaded { file_index }
                            } else if let Some(serialized_files_loaded) =
                                expanded_containers.get(&candidate.virtual_path).copied()
                            {
                                let matching_serialized_files = loaded_names
                                    .get(&dependency_name)
                                    .map(|(_, count)| *count)
                                    .unwrap_or(0);
                                SerializedFileDependencyOutcome::ContainerAlreadyExpanded {
                                    virtual_path: candidate.virtual_path,
                                    serialized_files_loaded,
                                    matching_serialized_files,
                                }
                            } else {
                                let dispatched = index
                                    .as_ref()
                                    .ok_or_else(|| {
                                        workspace_error(
                                            workspace.report,
                                            Some(&candidate.virtual_path),
                                            SerializedFileWorkspaceErrorKind::DependencyIndexUnavailable,
                                        )
                                    })?
                                    .dispatch_candidate(&candidate, limits.scheme)
                                    .map_err(|error| {
                                        workspace_error(
                                            workspace.report,
                                            Some(&candidate.virtual_path),
                                            SerializedFileWorkspaceErrorKind::DependencyDispatch(
                                                Box::new(error),
                                            ),
                                        )
                                    })?;
                                let detected_kind = dispatched.kind();
                                match detected_kind {
                                    FileKind::SerializedFile => {
                                        drop(dispatched);
                                        let dependency_input = DependencyFilePath {
                                            file_name: file_name_from_virtual_path(
                                                &candidate.virtual_path,
                                            ),
                                            virtual_path: candidate.virtual_path.clone(),
                                        };
                                        let file_index = load_dependency_serialized_file(
                                            filesystem,
                                            &dependency_input,
                                            limits,
                                            &mut workspace,
                                            &mut loaded_names,
                                            &mut loaded_paths,
                                            layouts,
                                        )?;
                                        dependency_report.loaded_serialized_files = dependency_report
                                            .loaded_serialized_files
                                            .checked_add(1)
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::FileCountOverflow,
                                                )
                                            })?;
                                        SerializedFileDependencyOutcome::LoadedSerializedFile {
                                            file_index,
                                            virtual_path: candidate.virtual_path,
                                        }
                                    }
                                    FileKind::Resource => {
                                        drop(dispatched);
                                        SerializedFileDependencyOutcome::ResourceCandidate {
                                            virtual_path: candidate.virtual_path,
                                        }
                                    }
                                    kind if container_kind(kind) => {
                                        let container_input_bytes = u64::try_from(
                                            dispatched.bytes().len(),
                                        )
                                        .map_err(|_| {
                                            workspace_error(
                                                workspace.report,
                                                Some(&candidate.virtual_path),
                                                SerializedFileWorkspaceErrorKind::DependencyContainerInputByteCountOverflow,
                                            )
                                        })?;
                                        total_container_input_bytes = total_container_input_bytes
                                            .checked_add(container_input_bytes)
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::DependencyContainerInputByteCountOverflow,
                                                )
                                            })?;
                                        if total_container_input_bytes
                                            > limits.max_total_dependency_container_input_bytes
                                        {
                                            return Err(workspace_error(
                                                workspace.report,
                                                Some(&candidate.virtual_path),
                                                SerializedFileWorkspaceErrorKind::TotalDependencyContainerInputBytesLimitExceeded {
                                                    byte_count: total_container_input_bytes,
                                                    limit: limits
                                                        .max_total_dependency_container_input_bytes,
                                                },
                                            ));
                                        }
                                        let remaining_entries = limits
                                            .max_total_dependency_container_entries
                                            .checked_sub(total_container_entries)
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::DependencyContainerEntryCountOverflow,
                                                )
                                            })?;
                                        let remaining_output_bytes = limits
                                            .max_total_dependency_container_output_bytes
                                            .checked_sub(total_container_output_bytes)
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::DependencyContainerOutputByteCountOverflow,
                                                )
                                            })?;
                                        let mut bounded_mount_limits = container_limits;
                                        bounded_mount_limits.max_entries =
                                            bounded_mount_limits.max_entries.min(remaining_entries);
                                        bounded_mount_limits.max_total_output_bytes =
                                            bounded_mount_limits
                                                .max_total_output_bytes
                                                .min(remaining_output_bytes);

                                        drop(index.take());
                                        let source_bundle_name =
                                            logical_file_name(&candidate.virtual_path);
                                        let mount_root = new_dependency_expansion_root(
                                            filesystem,
                                            &mut next_mount_namespace,
                                        )
                                        .map_err(|source| {
                                            workspace_error(
                                                workspace.report,
                                                Some(&candidate.virtual_path),
                                                SerializedFileWorkspaceErrorKind::FileSystem(
                                                    source,
                                                ),
                                            )
                                        })?;
                                        let mount_report = dispatched
                                            .mount_container_tree_with_source_name(
                                                filesystem,
                                                Some(&mount_root),
                                                source_bundle_name.as_deref(),
                                                bounded_mount_limits,
                                            )
                                            .map_err(|error| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::ContainerMount(
                                                        Box::new(error),
                                                    ),
                                                )
                                            })?;
                                        let container_scope_count_limit = bounded_mount_limits
                                            .max_entries
                                            .checked_add(1)
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::ContainerScopeCountOverflow,
                                                )
                                            })?;
                                        let scope_index = build_container_scope_index(
                                            mount_report.container_scopes.iter(),
                                            mount_report.container_scopes.len(),
                                            container_scope_count_limit,
                                            workspace.report,
                                            Some(&candidate.virtual_path),
                                        )?;
                                        let container_file_order = build_mounted_file_order_index(
                                            std::iter::once((
                                                mount_report.container_scopes.as_slice(),
                                                mount_report.serialized_files.as_slice(),
                                            )),
                                            bounded_mount_limits.max_entries,
                                            workspace.report,
                                        )?;
                                        let first_loaded_file_index = workspace.files.len();
                                        total_container_entries = total_container_entries
                                            .checked_add(mount_report.mounted_entries)
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::DependencyContainerEntryCountOverflow,
                                                )
                                            })?;
                                        if total_container_entries
                                            > limits.max_total_dependency_container_entries
                                        {
                                            return Err(workspace_error(
                                                workspace.report,
                                                Some(&candidate.virtual_path),
                                                SerializedFileWorkspaceErrorKind::TotalDependencyContainerEntriesLimitExceeded {
                                                    count: total_container_entries,
                                                    limit: limits
                                                        .max_total_dependency_container_entries,
                                                },
                                            ));
                                        }
                                        total_container_output_bytes = total_container_output_bytes
                                            .checked_add(mount_report.mounted_bytes)
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::DependencyContainerOutputByteCountOverflow,
                                                )
                                            })?;
                                        if total_container_output_bytes
                                            > limits.max_total_dependency_container_output_bytes
                                        {
                                            return Err(workspace_error(
                                                workspace.report,
                                                Some(&candidate.virtual_path),
                                                SerializedFileWorkspaceErrorKind::TotalDependencyContainerOutputBytesLimitExceeded {
                                                    byte_count: total_container_output_bytes,
                                                    limit: limits
                                                        .max_total_dependency_container_output_bytes,
                                                },
                                            ));
                                        }

                                        let leaf_paths = filesystem
                                            .enumerate_files(
                                                Some(&mount_root),
                                                "*",
                                                SearchOption::AllDirectories,
                                            )
                                            .map_err(|source| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&mount_root),
                                                    SerializedFileWorkspaceErrorKind::FileSystem(
                                                        source,
                                                    ),
                                                )
                                            })?;
                                        let requested_registry_file_count = dynamic_registered_files
                                            .len()
                                            .checked_add(leaf_paths.len())
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&mount_root),
                                                    SerializedFileWorkspaceErrorKind::DependencyRegistryCountOverflow,
                                                )
                                            })?;
                                        if requested_registry_file_count
                                            > index_limits.max_registered_files
                                        {
                                            return Err(workspace_error(
                                                workspace.report,
                                                Some(&mount_root),
                                                SerializedFileWorkspaceErrorKind::DependencyRegistryCountLimitExceeded {
                                                    count: requested_registry_file_count,
                                                    limit: index_limits.max_registered_files,
                                                },
                                            ));
                                        }
                                        dynamic_registered_files
                                            .try_reserve(leaf_paths.len())
                                            .map_err(|_| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&mount_root),
                                                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                                                        resource: "container leaf identities",
                                                        requested: requested_registry_file_count,
                                                    },
                                                )
                                            })?;
                                        let mut serialized_files_loaded = 0usize;
                                        for leaf_path in leaf_paths {
                                            let relative_name = relative_path_under(
                                                &leaf_path,
                                                &mount_root,
                                            )
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&leaf_path),
                                                    SerializedFileWorkspaceErrorKind::UnexpectedContainerLeaf {
                                                        virtual_path: leaf_path.clone(),
                                                        kind: FileKind::Resource,
                                                    },
                                                )
                                            })?;
                                            dynamic_registered_files.push(DependencyFilePath {
                                                file_name: relative_name.to_owned(),
                                                virtual_path: leaf_path.clone(),
                                            });
                                            let leaf = dispatch_vfs_file(
                                                filesystem,
                                                &leaf_path,
                                                limits.scheme,
                                                workspace.report,
                                            )?;
                                            match leaf.kind() {
                                                FileKind::SerializedFile => {
                                                    drop(leaf);
                                                    if !loaded_paths.contains_key(&leaf_path) {
                                                        let leaf_input = DependencyFilePath {
                                                            file_name: relative_name.to_owned(),
                                                            virtual_path: leaf_path.clone(),
                                                        };
                                                        load_dependency_serialized_file(
                                                            filesystem,
                                                            &leaf_input,
                                                            limits,
                                                            &mut workspace,
                                                            &mut loaded_names,
                                                            &mut loaded_paths,
                                                            layouts,
                                                        )?;
                                                        serialized_files_loaded =
                                                            serialized_files_loaded.checked_add(1).ok_or_else(|| {
                                                                workspace_error(
                                                                    workspace.report,
                                                                    Some(&leaf_path),
                                                                    SerializedFileWorkspaceErrorKind::FileCountOverflow,
                                                                )
                                                            })?;
                                                        dependency_report.loaded_serialized_files =
                                                            dependency_report.loaded_serialized_files.checked_add(1).ok_or_else(|| {
                                                                workspace_error(
                                                                    workspace.report,
                                                                    Some(&leaf_path),
                                                                    SerializedFileWorkspaceErrorKind::FileCountOverflow,
                                                                )
                                                            })?;
                                                    }
                                                }
                                                FileKind::Resource => drop(leaf),
                                                kind => {
                                                    return Err(workspace_error(
                                                        workspace.report,
                                                            Some(&leaf_path),
                                                            SerializedFileWorkspaceErrorKind::UnexpectedContainerLeaf {
                                                                virtual_path: leaf_path.clone(),
                                                            kind,
                                                        },
                                                    ));
                                                }
                                            }
                                        }
                                        append_mounted_container_bundles(
                                            &mut workspace,
                                            MountedContainerBundleInput {
                                                container_scopes: &mount_report.container_scopes,
                                                mounted_bundles: &mount_report.container_bundles,
                                                serialized_files: &mount_report.serialized_files,
                                                resource_files: &mount_report.resource_files,
                                                failed_entries: &mount_report.failed_entries,
                                                source_container_path: &candidate.virtual_path,
                                                loaded_paths: &loaded_paths,
                                                limits,
                                                bundle_count_limit: max_container_bundle_count,
                                            },
                                            &mut total_bundle_node_path_bytes,
                                            &mut total_bundle_resource_file_path_bytes,
                                        )?;
                                        for file in &mut workspace.files[first_loaded_file_index..]
                                        {
                                            attach_container_scope(
                                                file,
                                                &scope_index,
                                                &mut total_bundle_scope_bytes,
                                                limits,
                                                workspace.report,
                                            )?;
                                            attach_container_file_order(
                                                file,
                                                &container_file_order,
                                                workspace.report,
                                            )?;
                                        }
                                        let requested_container_count = expanded_containers
                                            .len()
                                            .checked_add(1)
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::DependencyContainerCountOverflow,
                                                )
                                            })?;
                                        expanded_containers.try_reserve(1).map_err(|_| {
                                            workspace_error(
                                                workspace.report,
                                                Some(&candidate.virtual_path),
                                                SerializedFileWorkspaceErrorKind::AllocationFailed {
                                                    resource: "expanded dependency containers",
                                                    requested: requested_container_count,
                                                },
                                            )
                                        })?;
                                        expanded_containers.insert(
                                            candidate.virtual_path.clone(),
                                            serialized_files_loaded,
                                        );
                                        registry = dependency_registry(
                                            filesystem,
                                            inputs,
                                            &dynamic_registered_files,
                                            index_limits,
                                        )
                                        .map_err(|kind| {
                                            workspace_error(
                                                workspace.report,
                                                Some(&candidate.virtual_path),
                                                kind,
                                            )
                                        })?;
                                        index = Some(
                                            DependencyIndex::with_limits(
                                                filesystem,
                                                &registry,
                                                data_paths,
                                                index_limits,
                                            )
                                            .map_err(|error| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::DependencyIndexBuild(
                                                        Box::new(error),
                                                    ),
                                                )
                                            })?,
                                        );
                                        let matching_serialized_files = loaded_names
                                            .get(&dependency_name)
                                            .map(|(_, count)| *count)
                                            .unwrap_or(0);
                                        SerializedFileDependencyOutcome::ContainerExpanded {
                                            virtual_path: candidate.virtual_path,
                                            kind,
                                            mounted_entries: mount_report.mounted_entries,
                                            mounted_bytes: mount_report.mounted_bytes,
                                            serialized_files_loaded,
                                            matching_serialized_files,
                                        }
                                    }
                                    _ => {
                                        drop(dispatched);
                                        return Err(workspace_error(
                                            workspace.report,
                                            Some(&candidate.virtual_path),
                                            SerializedFileWorkspaceErrorKind::UnexpectedContainerLeaf {
                                                virtual_path: candidate.virtual_path.clone(),
                                                kind: detected_kind,
                                            },
                                        ));
                                    }
                                }
                            }
                        }
                    },
                };

                dependency_report.requests.try_reserve(1).map_err(|_| {
                    workspace_error(
                        workspace.report,
                        Some(&workspace.files[source_file_index].virtual_path),
                        SerializedFileWorkspaceErrorKind::AllocationFailed {
                            resource: "external dependency report entries",
                            requested: next_request_count,
                        },
                    )
                })?;
                dependency_report
                    .requests
                    .push(SerializedFileDependencyRequest {
                        source_file_index,
                        external_index,
                        dependency_name,
                        outcome,
                    });
            }
            source_file_index += 1;
        }

        Ok((workspace, dependency_report))
    }

    pub fn files(&self) -> &[LoadedSerializedFile] {
        &self.files
    }

    /// Replace one per-file scan after a caller has recovered additional object fields.
    /// Returns `false` if the requested file index does not exist.
    pub(crate) fn replace_reference_graph(
        &mut self,
        file_index: usize,
        graph: ObjectReferenceGraph,
    ) -> bool {
        let Some(file) = self.files.get_mut(file_index) else {
            return false;
        };
        file.reference_graph = graph;
        true
    }

    /// Source container hierarchy with each node's direct collection and ResourceFile indexes.
    pub fn container_bundles(&self) -> &[WorkspaceSerializedBundle] {
        &self.container_bundles
    }

    /// ResourceFile path and container metadata; file contents remain in the source VFS.
    pub fn resource_files(&self) -> &[WorkspaceResourceFile] {
        &self.resource_files
    }

    /// Source-style UnityFS entries whose block reads failed while valid siblings were retained.
    pub fn failed_container_entries(&self) -> &[WorkspaceFailedContainerEntry] {
        &self.failed_container_entries
    }

    /// Finds a ResourceFile in the source bundle hierarchy using `Bundle.ResolveResource`
    /// precedence. `source_bundle_index == None` represents the root GameBundle. This searches
    /// mounted bundle resources only; a GameBundle external provider is a separate step.
    pub fn resolve_bundled_resource_file(
        &self,
        source_bundle_index: Option<usize>,
        requested_name: &str,
    ) -> Result<Option<usize>, WorkspaceResourceLookupError> {
        resource_resolution::WorkspaceResourceLookup::new(self)
            .resolve_bundled_resource_file(source_bundle_index, requested_name)
    }

    /// Resolves mounted bundle resources together with direct GameBundle ResourceFile inputs.
    /// Explicit root resources are checked at the GameBundle scope before its child Bundles,
    /// matching `Bundle.ResolveResource`'s current-scope resource-before-child order.
    pub fn resolve_bundled_or_root_resource_file(
        &self,
        source_bundle_index: Option<usize>,
        requested_name: &str,
        root_resources: &[WorkspaceRootResourceFile],
    ) -> Result<Option<WorkspaceResolvedResourceFile>, WorkspaceResourceLookupError> {
        resource_resolution::WorkspaceResourceLookup::new(self)
            .resolve_bundled_or_root_resource_file(
                source_bundle_index,
                requested_name,
                root_resources,
            )
    }

    /// Returns workspace indexes for direct collections in one mounted container, ordered as
    /// `SerializedBundle.Collections` is ordered by the source baseline.
    pub fn ordered_collection_file_indices(
        &self,
        container_scope: &str,
    ) -> Result<Vec<usize>, Box<SerializedFileWorkspaceError>> {
        collection_order::WorkspaceCollectionOrder::new(self)
            .ordered_collection_file_indices(container_scope)
    }

    /// Returns all collections in the order used by source `GameBundle.FetchAssetCollections`.
    /// Root standalone collections are emitted first in reverse file-stack order, followed by
    /// root containers in reverse insertion order; each container emits direct collections then
    /// child containers ordered by their containing source entry. These rules are applied to the
    /// deterministic file and bundle insertion order retained by this workspace.
    pub fn source_collection_file_indices(
        &self,
    ) -> Result<Vec<usize>, Box<SerializedFileWorkspaceError>> {
        collection_order::WorkspaceCollectionOrder::new(self).source_collection_file_indices()
    }

    pub fn report(&self) -> SerializedFileWorkspaceReport {
        self.report
    }

    /// Join per-file PPtr scans using the source Bundle collection lookup order.
    pub fn reference_graph(
        &self,
        limits: MultiFileReferenceGraphLimits,
    ) -> Result<MultiFileReferenceGraph<'_>, MultiFileReferenceGraphBuildError> {
        let mut named_files = Vec::new();
        named_files
            .try_reserve_exact(self.files.len())
            .map_err(|_| MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "named SerializedFile graph entries",
                requested: self.files.len(),
            })?;
        named_files.extend(self.files.iter().map(|file| NamedObjectReferenceGraph {
            file_name: &file.file_name,
            metadata: &file.metadata,
            graph: &file.reference_graph,
        }));
        let collection_lookup = SourceBundleCollectionLookup::build(self, limits)?;
        MultiFileReferenceGraph::build_with_dependency_resolver(
            &named_files,
            limits,
            |source_file_index, identifier| {
                let dependency_name = identifier.get_file_path()?;
                collection_lookup.resolve(source_file_index, &dependency_name)
            },
        )
    }
}
