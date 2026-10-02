//! Bounded loading of named SerializedFiles from the mounted VFS.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use crate::io::dependency::{
    DependencyDispatchError, DependencyFilePath, DependencyIndex, DependencyIndexBuildError,
    DependencyIndexLimits, DependencyLookup, DependencyLookupError,
};
use crate::io::filesystem::{SearchOption, VirtualFileSystem};
use crate::io::special_names::fix_file_identifier;
use crate::scheme::{
    ContainerMountError, ContainerMountLimits, DispatchedFile, FileKind, ParsedFile, SchemeError,
    SchemeReader, SchemeReaderLimits,
};

use super::header::SerializedFileHeader;
use super::metadata::SerializedFileMetadata;
use super::reference_graph::{
    MultiFileReferenceGraph, MultiFileReferenceGraphBuildError, MultiFileReferenceGraphLimits,
    NamedObjectReferenceGraph, ObjectReferenceGraph, ReferenceGraphBuildError,
    ReferenceGraphLimits,
};

/// Resource bounds for loading and scanning a named set of VFS SerializedFiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SerializedFileWorkspaceLimits {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_input_bytes: u64,
    pub max_file_name_bytes: usize,
    pub max_total_file_name_bytes: usize,
    pub max_dependency_requests: usize,
    pub max_total_dependency_name_bytes: usize,
    pub max_total_dependency_candidate_path_bytes: usize,
    pub max_total_dependency_container_input_bytes: u64,
    pub max_total_dependency_container_entries: usize,
    pub max_total_dependency_container_output_bytes: u64,
    pub max_total_bundle_scope_bytes: usize,
    pub max_total_objects: usize,
    pub max_total_reference_edges: usize,
    pub scheme: SchemeReaderLimits,
    pub reference_graph: ReferenceGraphLimits,
}

impl Default for SerializedFileWorkspaceLimits {
    fn default() -> Self {
        Self {
            max_files: 1_000_000,
            max_file_bytes: 2 * 1024 * 1024 * 1024,
            max_total_input_bytes: 16 * 1024 * 1024 * 1024,
            max_file_name_bytes: 1024 * 1024,
            max_total_file_name_bytes: 64 * 1024 * 1024,
            max_dependency_requests: 1_000_000,
            max_total_dependency_name_bytes: 64 * 1024 * 1024,
            max_total_dependency_candidate_path_bytes: 64 * 1024 * 1024,
            max_total_dependency_container_input_bytes: 16 * 1024 * 1024 * 1024,
            max_total_dependency_container_entries: 1_000_000,
            max_total_dependency_container_output_bytes: 2 * 1024 * 1024 * 1024,
            max_total_bundle_scope_bytes: 64 * 1024 * 1024,
            max_total_objects: 20_000_000,
            max_total_reference_edges: 5_000_000,
            scheme: SchemeReaderLimits::default(),
            reference_graph: ReferenceGraphLimits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SerializedFileWorkspaceReport {
    pub input_files: usize,
    pub completed_files: usize,
    pub total_input_bytes: u64,
    pub completed_input_bytes: u64,
    pub total_file_name_bytes: usize,
    pub total_objects: usize,
    pub total_reference_edges: usize,
}

/// One fully parsed SerializedFile with its header, owned metadata, and per-file PPtr graph.
///
/// Raw input bytes remain in the source VFS and are identified by `virtual_path`; this workspace
/// retains indexes rather than duplicating potentially large object payload buffers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedSerializedFile {
    pub file_name: String,
    pub virtual_path: String,
    /// Deepest source container virtual root containing this SerializedFile, if any.
    pub container_scope: Option<String>,
    pub header: SerializedFileHeader,
    pub metadata: SerializedFileMetadata,
    pub reference_graph: ObjectReferenceGraph,
}

/// A loaded set of SerializedFiles whose cross-file references can be joined on demand.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SerializedFileWorkspace {
    files: Vec<LoadedSerializedFile>,
    report: SerializedFileWorkspaceReport,
}

/// Result of walking external-file tables reachable from the initial workspace files.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SerializedFileDependencyReport {
    pub requests: Vec<SerializedFileDependencyRequest>,
    pub loaded_serialized_files: usize,
}

/// A container mounted before the external dependency walk began.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreExpandedContainer {
    pub virtual_path: String,
    pub serialized_files_loaded: usize,
    pub container_scopes: Vec<String>,
}

pub(crate) struct SerializedFileDependencyLoadOptions<'a> {
    pub data_paths: &'a [String],
    pub limits: SerializedFileWorkspaceLimits,
    pub index_limits: DependencyIndexLimits,
    pub container_limits: ContainerMountLimits,
    pub preexpanded_containers: &'a [PreExpandedContainer],
}

impl SerializedFileDependencyReport {
    /// True only when every external request resolved to one loaded SerializedFile identity.
    pub fn is_complete(&self) -> bool {
        self.requests.iter().all(|request| {
            matches!(
                &request.outcome,
                SerializedFileDependencyOutcome::AlreadyLoaded { .. }
                    | SerializedFileDependencyOutcome::LoadedSerializedFile { .. }
                    | SerializedFileDependencyOutcome::ContainerExpanded {
                        matching_serialized_files: 1,
                        ..
                    }
                    | SerializedFileDependencyOutcome::ContainerAlreadyExpanded {
                        matching_serialized_files: 1,
                        ..
                    }
            )
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedFileDependencyRequest {
    pub source_file_index: usize,
    pub external_index: usize,
    pub dependency_name: String,
    pub outcome: SerializedFileDependencyOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedFileDependencyOutcome {
    AlreadyLoaded {
        file_index: usize,
    },
    LoadedSerializedFile {
        file_index: usize,
        virtual_path: String,
    },
    Missing,
    Ambiguous {
        candidate_count: usize,
        candidate_paths: Vec<String>,
    },
    AmbiguousLoadedIdentity {
        candidate_count: usize,
    },
    ResourceCandidate {
        virtual_path: String,
    },
    ContainerExpanded {
        virtual_path: String,
        kind: FileKind,
        mounted_entries: usize,
        mounted_bytes: u64,
        serialized_files_loaded: usize,
        matching_serialized_files: usize,
    },
    ContainerAlreadyExpanded {
        virtual_path: String,
        serialized_files_loaded: usize,
        matching_serialized_files: usize,
    },
}

impl SerializedFileWorkspace {
    /// Parse caller-identified SerializedFiles from the VFS and build their per-file reference
    /// graphs. All file sizes are preflighted before any file contents are read.
    pub fn load_from_vfs(
        filesystem: &VirtualFileSystem,
        inputs: &[DependencyFilePath],
        limits: SerializedFileWorkspaceLimits,
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
                let graph =
                    ObjectReferenceGraph::build_with_limits(&parsed, limits.reference_graph)
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
                header,
                metadata,
                reference_graph,
            });
        }

        Ok(Self { files, report })
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
        Self::load_from_vfs_with_preexpanded_containers(
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
        )
    }

    pub(crate) fn load_from_vfs_with_preexpanded_containers(
        filesystem: &mut VirtualFileSystem,
        inputs: &[DependencyFilePath],
        registered_files: &[DependencyFilePath],
        options: SerializedFileDependencyLoadOptions<'_>,
    ) -> Result<(Self, SerializedFileDependencyReport), Box<SerializedFileWorkspaceError>> {
        let SerializedFileDependencyLoadOptions {
            data_paths,
            limits,
            index_limits,
            container_limits,
            preexpanded_containers,
        } = options;
        let mut workspace = Self::load_from_vfs(filesystem, inputs, limits)?;
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
        let mut total_bundle_scope_bytes = 0usize;
        for file in &mut workspace.files {
            attach_container_scope(
                file,
                &initial_scope_index,
                &mut total_bundle_scope_bytes,
                limits,
                workspace.report,
            )?;
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
                                            .mount_container_tree(
                                                filesystem,
                                                Some(&mount_root),
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
                                        for file in &mut workspace.files[first_loaded_file_index..]
                                        {
                                            attach_container_scope(
                                                file,
                                                &scope_index,
                                                &mut total_bundle_scope_bytes,
                                                limits,
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

    pub fn report(&self) -> SerializedFileWorkspaceReport {
        self.report
    }

    /// Join every loaded file's per-file PPtr scan by normalized SerializedFile name.
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
        MultiFileReferenceGraph::build_with_limits(&named_files, limits)
    }
}

fn dependency_registry(
    filesystem: &VirtualFileSystem,
    inputs: &[DependencyFilePath],
    registered_files: &[DependencyFilePath],
    limits: DependencyIndexLimits,
) -> Result<Vec<DependencyFilePath>, SerializedFileWorkspaceErrorKind> {
    let mounted_paths = filesystem
        .enumerate_files(None, "*", SearchOption::AllDirectories)
        .map_err(SerializedFileWorkspaceErrorKind::FileSystem)?;
    let capacity = mounted_paths
        .len()
        .checked_add(inputs.len())
        .and_then(|count| count.checked_add(registered_files.len()))
        .ok_or(SerializedFileWorkspaceErrorKind::DependencyRegistryCountOverflow)?;
    if capacity > limits.max_registered_files {
        return Err(
            SerializedFileWorkspaceErrorKind::DependencyRegistryCountLimitExceeded {
                count: capacity,
                limit: limits.max_registered_files,
            },
        );
    }
    let mut registry = Vec::new();
    registry.try_reserve_exact(capacity).map_err(|_| {
        SerializedFileWorkspaceErrorKind::AllocationFailed {
            resource: "mounted dependency registry",
            requested: capacity,
        }
    })?;
    for virtual_path in mounted_paths {
        registry.push(DependencyFilePath {
            file_name: file_name_from_virtual_path(&virtual_path),
            virtual_path,
        });
    }
    registry.extend(inputs.iter().chain(registered_files).cloned());
    registry.sort_unstable_by(|left, right| {
        left.file_name
            .cmp(&right.file_name)
            .then(left.virtual_path.cmp(&right.virtual_path))
    });
    registry.dedup();
    Ok(registry)
}

fn file_name_from_virtual_path(path: &str) -> String {
    path.rsplit('/').next().unwrap_or_default().to_owned()
}

fn build_container_scope_index<'a>(
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

fn deepest_container_scope_for_path<'a>(
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

fn attach_container_scope(
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

fn remaining_workspace_limits(
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
        ..limits
    }
}

fn append_workspace(
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
    workspace.report = SerializedFileWorkspaceReport {
        input_files,
        completed_files,
        total_input_bytes,
        completed_input_bytes,
        total_file_name_bytes,
        total_objects,
        total_reference_edges,
    };
    workspace.files.append(&mut additional.files);
    Ok(())
}

fn load_dependency_serialized_file(
    filesystem: &VirtualFileSystem,
    input: &DependencyFilePath,
    limits: SerializedFileWorkspaceLimits,
    workspace: &mut SerializedFileWorkspace,
    loaded_names: &mut HashMap<String, (usize, usize)>,
    loaded_paths: &mut HashMap<String, usize>,
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

    let additional = SerializedFileWorkspace::load_from_vfs(
        filesystem,
        std::slice::from_ref(input),
        remaining_workspace_limits(limits, workspace.report, workspace.files.len()),
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

fn dispatch_vfs_file(
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

fn new_dependency_expansion_root(
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

fn container_kind(kind: FileKind) -> bool {
    matches!(
        kind,
        FileKind::UnityFs
            | FileKind::UnityRaw
            | FileKind::UnityWebBundle
            | FileKind::UnityArchive
            | FileKind::UnityWebData
    )
}

fn relative_path_under<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    path.strip_prefix(root)?.strip_prefix('/')
}

#[derive(Debug)]
pub struct SerializedFileWorkspaceError {
    pub report: SerializedFileWorkspaceReport,
    pub input_path: Option<String>,
    pub kind: SerializedFileWorkspaceErrorKind,
}

#[derive(Debug)]
pub enum SerializedFileWorkspaceErrorKind {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    FileCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    FileNameTooLong {
        input_index: usize,
        byte_count: usize,
        limit: usize,
    },
    FileNameByteCountOverflow,
    FileCountOverflow,
    TotalFileNameBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    DependencyRegistryCountOverflow,
    DependencyRegistryCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    DependencyRequestLimitExceeded {
        count: usize,
        limit: usize,
    },
    DependencyRequestCountOverflow,
    DependencyNameByteCountOverflow,
    TotalDependencyNameBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    DependencyCandidatePathByteCountOverflow,
    TotalDependencyCandidatePathBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    DependencyIndexUnavailable,
    DependencyContainerInputByteCountOverflow,
    TotalDependencyContainerInputBytesLimitExceeded {
        byte_count: u64,
        limit: u64,
    },
    DependencyContainerEntryCountOverflow,
    DependencyContainerCountOverflow,
    TotalDependencyContainerEntriesLimitExceeded {
        count: usize,
        limit: usize,
    },
    DependencyContainerOutputByteCountOverflow,
    ContainerScopeCountOverflow,
    ContainerScopeCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    DuplicateContainerScopePath,
    BundleScopeByteCountOverflow,
    TotalBundleScopeBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    PreExpandedContainerCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    DuplicatePreExpandedContainerPath,
    TotalDependencyContainerOutputBytesLimitExceeded {
        byte_count: u64,
        limit: u64,
    },
    UnexpectedContainerLeaf {
        virtual_path: String,
        kind: FileKind,
    },
    DuplicateVirtualPath,
    FileSizeLimitExceeded {
        file_bytes: u64,
        limit: u64,
    },
    TotalInputLimitExceeded {
        requested_total: u64,
        limit: u64,
    },
    InputByteCountOverflow,
    NotSerializedFile {
        detected_kind: FileKind,
    },
    TotalObjectLimitExceeded {
        requested_total: usize,
        limit: usize,
    },
    ObjectCountOverflow,
    TotalReferenceLimitExceeded {
        requested_total: usize,
        limit: usize,
    },
    ReferenceCountOverflow,
    FileSystem(io::Error),
    DependencyIndexBuild(Box<DependencyIndexBuildError>),
    DependencyLookup(Box<DependencyLookupError>),
    DependencyDispatch(Box<DependencyDispatchError>),
    ContainerMount(Box<ContainerMountError>),
    Scheme(SchemeError),
    ReferenceGraph(ReferenceGraphBuildError),
}

impl Display for SerializedFileWorkspaceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.input_path {
            write!(f, "SerializedFile workspace input '{path}' failed")?;
        } else {
            f.write_str("SerializedFile workspace input failed")?;
        }
        write!(
            f,
            " after {} completed files and {} bytes: {}",
            self.report.completed_files, self.report.completed_input_bytes, self.kind
        )
    }
}

impl Display for SerializedFileWorkspaceErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::FileCountLimitExceeded { count, limit } => {
                write!(f, "input file count {count} exceeds limit {limit}")
            }
            Self::FileNameTooLong {
                input_index,
                byte_count,
                limit,
            } => write!(
                f,
                "file name at input {input_index} has {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::FileNameByteCountOverflow => f.write_str("file name byte count overflows"),
            Self::FileCountOverflow => f.write_str("aggregate file count overflows"),
            Self::TotalFileNameBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "total normalized file-name bytes {byte_count} exceed limit {limit}"
            ),
            Self::DependencyRegistryCountOverflow => {
                f.write_str("dependency registry file count overflows")
            }
            Self::DependencyRegistryCountLimitExceeded { count, limit } => write!(
                f,
                "dependency registry has {count} files, exceeding limit {limit}"
            ),
            Self::DependencyRequestLimitExceeded { count, limit } => write!(
                f,
                "external dependency request count {count} exceeds limit {limit}"
            ),
            Self::DependencyRequestCountOverflow => {
                f.write_str("external dependency request count overflows")
            }
            Self::DependencyNameByteCountOverflow => {
                f.write_str("external dependency name byte count overflows")
            }
            Self::TotalDependencyNameBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "external dependency names use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::DependencyCandidatePathByteCountOverflow => {
                f.write_str("reported dependency candidate path bytes overflow")
            }
            Self::TotalDependencyCandidatePathBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "reported dependency candidate paths use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::DependencyIndexUnavailable => {
                f.write_str("dependency index is unavailable during the dependency walk")
            }
            Self::DependencyContainerInputByteCountOverflow => {
                f.write_str("dependency container input byte count overflows")
            }
            Self::TotalDependencyContainerInputBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "dependency container input uses {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::DependencyContainerEntryCountOverflow => {
                f.write_str("dependency container entry count overflows")
            }
            Self::DependencyContainerCountOverflow => {
                f.write_str("expanded dependency container count overflows")
            }
            Self::TotalDependencyContainerEntriesLimitExceeded { count, limit } => write!(
                f,
                "dependency container entry count {count} exceeds limit {limit}"
            ),
            Self::DependencyContainerOutputByteCountOverflow => {
                f.write_str("dependency container output byte count overflows")
            }
            Self::ContainerScopeCountOverflow => {
                f.write_str("container scope count overflows")
            }
            Self::ContainerScopeCountLimitExceeded { count, limit } => write!(
                f,
                "container scope count {count} exceeds workspace limit {limit}"
            ),
            Self::DuplicateContainerScopePath => {
                f.write_str("container scope path occurs more than once")
            }
            Self::BundleScopeByteCountOverflow => {
                f.write_str("loaded bundle-scope byte count overflows")
            }
            Self::TotalBundleScopeBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "loaded bundle scopes use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::PreExpandedContainerCountLimitExceeded { count, limit } => write!(
                f,
                "pre-expanded container count {count} exceeds limit {limit}"
            ),
            Self::DuplicatePreExpandedContainerPath => {
                f.write_str("pre-expanded container path occurs more than once")
            }
            Self::TotalDependencyContainerOutputBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "dependency container output uses {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::UnexpectedContainerLeaf { virtual_path, kind } => write!(
                f,
                "mounted container leaf '{virtual_path}' is still a {kind:?} container"
            ),
            Self::DuplicateVirtualPath => f.write_str("virtual path occurs more than once"),
            Self::FileSizeLimitExceeded { file_bytes, limit } => {
                write!(f, "file size {file_bytes} bytes exceeds limit {limit}")
            }
            Self::TotalInputLimitExceeded {
                requested_total,
                limit,
            } => write!(
                f,
                "aggregate input size {requested_total} bytes exceeds limit {limit}"
            ),
            Self::InputByteCountOverflow => f.write_str("aggregate input byte count overflows"),
            Self::NotSerializedFile { detected_kind } => {
                write!(f, "expected a SerializedFile, detected {detected_kind:?}")
            }
            Self::TotalObjectLimitExceeded {
                requested_total,
                limit,
            } => write!(f, "object count {requested_total} exceeds limit {limit}"),
            Self::ObjectCountOverflow => f.write_str("aggregate object count overflows"),
            Self::TotalReferenceLimitExceeded {
                requested_total,
                limit,
            } => write!(f, "reference count {requested_total} exceeds limit {limit}"),
            Self::ReferenceCountOverflow => f.write_str("aggregate reference count overflows"),
            Self::FileSystem(error) => write!(f, "VFS error: {error}"),
            Self::DependencyIndexBuild(error) => {
                write!(f, "could not build dependency index: {error}")
            }
            Self::DependencyLookup(error) => write!(f, "dependency lookup failed: {error}"),
            Self::DependencyDispatch(error) => {
                write!(f, "could not dispatch dependency: {error}")
            }
            Self::ContainerMount(error) => write!(f, "could not expand dependency container: {error}"),
            Self::Scheme(error) => write!(f, "file scheme error: {error}"),
            Self::ReferenceGraph(error) => write!(f, "per-file reference graph error: {error}"),
        }
    }
}

impl Error for SerializedFileWorkspaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            SerializedFileWorkspaceErrorKind::FileSystem(error) => Some(error),
            SerializedFileWorkspaceErrorKind::DependencyIndexBuild(error) => Some(error.as_ref()),
            SerializedFileWorkspaceErrorKind::DependencyLookup(error) => Some(error.as_ref()),
            SerializedFileWorkspaceErrorKind::DependencyDispatch(error) => Some(error.as_ref()),
            SerializedFileWorkspaceErrorKind::ContainerMount(error) => Some(error.as_ref()),
            SerializedFileWorkspaceErrorKind::Scheme(error) => Some(error),
            SerializedFileWorkspaceErrorKind::ReferenceGraph(error) => Some(error),
            _ => None,
        }
    }
}

impl Error for SerializedFileWorkspaceErrorKind {}

fn workspace_error(
    report: SerializedFileWorkspaceReport,
    input_path: Option<&str>,
    kind: SerializedFileWorkspaceErrorKind,
) -> Box<SerializedFileWorkspaceError> {
    Box::new(SerializedFileWorkspaceError {
        report,
        input_path: input_path.map(str::to_owned),
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        SerializedFileDependencyOutcome, SerializedFileWorkspace, SerializedFileWorkspaceErrorKind,
        SerializedFileWorkspaceLimits,
    };
    use crate::io::dependency::{DependencyFilePath, DependencyIndexLimits};
    use crate::io::filesystem::VirtualFileSystem;
    use crate::io::platform_file_inventory::{
        PlatformWorkspaceLoadErrorKind, PlatformWorkspaceLoadOptions,
    };
    use crate::io::platform_structure::{PlatformKind, PlatformStructure};
    use crate::scheme::{ContainerMountErrorKind, ContainerMountLimits, FileKind};
    use crate::serialized_file::reference_graph::{
        MultiFilePPtrResolution, MultiFileReferenceGraphLimits,
    };

    fn put_u16_le(output: &mut Vec<u8>, value: u16) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_i32_le(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32_le(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn put_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        put_u16_le(output, 1);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, 0);
    }

    fn make_generation_10_file() -> Vec<u8> {
        make_generation_10_file_with(&[], 0, 42, 42)
    }

    fn make_generation_10_file_with(
        external_names: &[&str],
        pointer_file_id: i32,
        object_path_id: i32,
        pointer_path_id: i32,
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, "Root");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let value_name = push_type_tree_string(&mut string_buffer, "m_Value");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let target_name = push_type_tree_string(&mut string_buffer, "m_Target");
        let pptr_type = push_type_tree_string(&mut string_buffer, "PPtr<GameObject>");
        let file_id_name = push_type_tree_string(&mut string_buffer, "m_FileID");
        let int32_type = push_type_tree_string(&mut string_buffer, "SInt32");
        let path_id_name = push_type_tree_string(&mut string_buffer, "m_PathID");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 5);
        put_i32_le(&mut metadata, string_buffer.len() as i32);
        put_node(&mut metadata, 0, root_type, empty_name, -1, 0);
        put_node(&mut metadata, 1, int_type, value_name, 4, 1);
        put_node(&mut metadata, 1, pptr_type, target_name, 8, 2);
        put_node(&mut metadata, 2, int32_type, file_id_name, 4, 3);
        put_node(&mut metadata, 2, int32_type, path_id_name, 4, 4);
        metadata.extend_from_slice(&string_buffer);

        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, object_path_id);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 12);
        put_i32_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
        for external_name in external_names {
            metadata.push(0); // empty AssetPath
            for word in [0, 0, 0, 0] {
                put_u32_le(&mut metadata, word);
            }
            put_i32_le(&mut metadata, 0); // serialized AssetType
            metadata.extend_from_slice(external_name.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let mut payload = Vec::new();
        put_i32_le(&mut payload, 0x1234_5678);
        put_i32_le(&mut payload, pointer_file_id);
        put_i32_le(&mut payload, pointer_path_id);
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as i32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_webdata_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let header_size = 20usize
            + entries
                .iter()
                .map(|(name, _)| 9usize + name.len())
                .sum::<usize>();
        let mut container = b"UnityWebData1.0\0".to_vec();
        container.extend_from_slice(&i32::try_from(header_size).unwrap().to_le_bytes());
        let mut data_offset = header_size;
        for (name, bytes) in entries {
            assert!(name.len() < 128);
            container.extend_from_slice(&i32::try_from(data_offset).unwrap().to_le_bytes());
            container.extend_from_slice(&i32::try_from(bytes.len()).unwrap().to_le_bytes());
            container.push(u8::try_from(name.len()).unwrap());
            container.extend_from_slice(name.as_bytes());
            data_offset += bytes.len();
        }
        for (_, bytes) in entries {
            container.extend_from_slice(bytes);
        }
        container
    }

    fn virtual_file_system(path: &str, bytes: &[u8]) -> VirtualFileSystem {
        let mut filesystem = VirtualFileSystem::new();
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, bytes).unwrap();
        filesystem
    }

    fn insert_file(filesystem: &mut VirtualFileSystem, path: &str, bytes: &[u8]) {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, bytes).unwrap();
    }

    fn dependency_container_filesystem() -> VirtualFileSystem {
        let mut filesystem = VirtualFileSystem::new();
        let leaf = make_generation_10_file();
        let container = make_webdata_fixture(&[("shared.assets", &leaf)]);
        insert_file(
            &mut filesystem,
            "/game/main.assets",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(&mut filesystem, "/game/shared.assets", &container);
        filesystem
    }

    fn input(path: &str) -> DependencyFilePath {
        DependencyFilePath {
            file_name: path.rsplit('/').next().unwrap().to_owned(),
            virtual_path: path.to_owned(),
        }
    }

    #[test]
    fn loads_named_serialized_file_and_builds_cross_file_graph() {
        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();

        assert_eq!(workspace.files().len(), 1);
        assert_eq!(workspace.files()[0].container_scope, None);
        assert_eq!(workspace.files()[0].header.format_version.raw(), 10);
        assert_eq!(workspace.files()[0].metadata.objects[0].path_id, 42);
        assert!(workspace.files()[0].reference_graph.is_complete());
        assert_eq!(
            workspace.report().completed_input_bytes,
            u64::try_from(bytes.len()).unwrap()
        );
        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn platform_inventory_feeds_primary_files_and_ordered_datapaths_to_dependency_loading() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/Game_Data/globalgamemanagers",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(
            &mut filesystem,
            "/game/Game_Data/shared.assets",
            &make_generation_10_file(),
        );
        let structure = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: Some("/game/Game_Data".to_owned()),
            data_paths: vec!["/game/Game_Data".to_owned()],
        };

        let loaded = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap();
        assert_eq!(loaded.file_inventory.primary_file_inputs.len(), 1);
        assert_eq!(
            loaded.file_inventory.primary_file_inputs[0].file_name,
            "globalgamemanagers"
        );
        assert_eq!(loaded.workspace.files().len(), 2);
        assert_eq!(loaded.dependencies.loaded_serialized_files, 1);
        assert!(matches!(
            loaded.dependencies.requests[0].outcome,
            SerializedFileDependencyOutcome::LoadedSerializedFile { .. }
        ));
    }

    #[test]
    fn platform_inventory_expands_initial_webdata_before_external_dependency_loading() {
        let mut filesystem = VirtualFileSystem::new();
        let serialized_leaf = make_generation_10_file();
        let nested_leaf = make_generation_10_file();
        let nested_webdata = make_webdata_fixture(&[("inner.assets", &nested_leaf)]);
        let webdata = make_webdata_fixture(&[
            ("shared.assets", &serialized_leaf),
            ("nested.bundle", &nested_webdata),
        ]);
        insert_file(
            &mut filesystem,
            "/game/Game_Data/globalgamemanagers",
            &make_generation_10_file_with(&["shared.assets", "data.unity3d"], 1, 10, 42),
        );
        insert_file(&mut filesystem, "/game/Game_Data/data.unity3d", &webdata);
        let structure = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: Some("/game/Game_Data".to_owned()),
            data_paths: vec!["/game/Game_Data".to_owned()],
        };

        let loaded = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap();

        assert_eq!(loaded.workspace.files().len(), 3);
        assert_eq!(loaded.initial_container_expansions.len(), 1);
        assert_eq!(
            loaded.initial_container_expansions[0].kind,
            FileKind::UnityWebData
        );
        assert_eq!(
            loaded.initial_container_expansions[0].serialized_files_found,
            2
        );
        let direct_file = loaded
            .workspace
            .files()
            .iter()
            .find(|file| file.virtual_path.ends_with("/shared.assets"))
            .unwrap();
        assert_eq!(
            direct_file.container_scope.as_deref(),
            Some("/.assetripper/import/initial/0")
        );
        let nested_file = loaded
            .workspace
            .files()
            .iter()
            .find(|file| file.virtual_path.ends_with("/nested.bundle/inner.assets"))
            .unwrap();
        assert_eq!(
            nested_file.container_scope.as_deref(),
            Some("/.assetripper/import/initial/0/nested.bundle")
        );
        assert!(matches!(
            loaded.dependencies.requests[0].outcome,
            SerializedFileDependencyOutcome::AlreadyLoaded { .. }
        ));
        assert_eq!(
            loaded.dependencies.requests[1].outcome,
            SerializedFileDependencyOutcome::ContainerAlreadyExpanded {
                virtual_path: "/game/Game_Data/data.unity3d".to_owned(),
                serialized_files_loaded: 2,
                matching_serialized_files: 0,
            }
        );
    }

    #[test]
    fn platform_inventory_enforces_initial_serialized_file_limit() {
        let mut filesystem = VirtualFileSystem::new();
        let serialized_leaf = make_generation_10_file();
        let webdata = make_webdata_fixture(&[("shared.assets", &serialized_leaf)]);
        insert_file(
            &mut filesystem,
            "/game/Game_Data/globalgamemanagers",
            &make_generation_10_file(),
        );
        insert_file(&mut filesystem, "/game/Game_Data/data.unity3d", &webdata);
        let structure = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: Some("/game/Game_Data".to_owned()),
            data_paths: vec!["/game/Game_Data".to_owned()],
        };

        let error = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions {
                    workspace_limits: SerializedFileWorkspaceLimits {
                        max_files: 1,
                        ..SerializedFileWorkspaceLimits::default()
                    },
                    ..PlatformWorkspaceLoadOptions::default()
                },
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            PlatformWorkspaceLoadErrorKind::InitialSerializedFileCountLimitExceeded {
                count: 2,
                limit: 1
            }
        ));
    }

    #[test]
    fn recursively_loads_external_serialized_files_and_resolves_file_scoped_pptrs() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/Data/main.assets",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(
            &mut filesystem,
            "/game/Data/shared.assets",
            &make_generation_10_file_with(&["third.assets"], 1, 42, 42),
        );
        insert_file(
            &mut filesystem,
            "/game/Data/third.assets",
            &make_generation_10_file_with(&["main.assets"], 0, 42, 42),
        );
        let inputs = [input("/game/Data/main.assets")];
        let data_paths = ["/game/Data".to_owned()];

        let (workspace, dependency_report) =
            SerializedFileWorkspace::load_from_vfs_with_dependencies(
                &mut filesystem,
                &inputs,
                &[],
                &data_paths,
                SerializedFileWorkspaceLimits::default(),
                DependencyIndexLimits::default(),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(workspace.files().len(), 3);
        assert_eq!(dependency_report.loaded_serialized_files, 2);
        assert!(dependency_report.is_complete());
        assert_eq!(
            dependency_report.requests[0].outcome,
            SerializedFileDependencyOutcome::LoadedSerializedFile {
                file_index: 1,
                virtual_path: "/game/Data/shared.assets".to_owned(),
            }
        );
        assert_eq!(
            dependency_report.requests[1].outcome,
            SerializedFileDependencyOutcome::LoadedSerializedFile {
                file_index: 2,
                virtual_path: "/game/Data/third.assets".to_owned(),
            }
        );
        assert_eq!(
            dependency_report.requests[2].outcome,
            SerializedFileDependencyOutcome::AlreadyLoaded { file_index: 0 }
        );

        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(graph.references().len(), 3);
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
        assert_eq!(
            graph.references()[1].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 2,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn keeps_missing_ambiguous_and_resource_dependency_results_explicit() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/Data/main.assets",
            &make_generation_10_file_with(
                &[
                    "texture.resS",
                    "dup.assets",
                    "missing.assets",
                    "archive.webdata",
                ],
                0,
                42,
                42,
            ),
        );
        insert_file(&mut filesystem, "/game/Data/texture.resS", &[1, 2, 3]);
        insert_file(&mut filesystem, "/bundles/a/dup.assets", &[1, 2, 3]);
        insert_file(&mut filesystem, "/bundles/b/dup.assets", &[4, 5, 6]);
        let mut webdata_container = b"UnityWebData1.0\0".to_vec();
        webdata_container.extend_from_slice(&20i32.to_le_bytes());
        insert_file(
            &mut filesystem,
            "/game/Data/archive.webdata",
            &webdata_container,
        );
        let inputs = [input("/game/Data/main.assets")];
        let data_paths = ["/game/Data".to_owned()];

        let (workspace, dependency_report) =
            SerializedFileWorkspace::load_from_vfs_with_dependencies(
                &mut filesystem,
                &inputs,
                &[],
                &data_paths,
                SerializedFileWorkspaceLimits::default(),
                DependencyIndexLimits::default(),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(workspace.files().len(), 1);
        assert!(!dependency_report.is_complete());
        assert!(matches!(
            dependency_report.requests[0].outcome,
            SerializedFileDependencyOutcome::ResourceCandidate { .. }
        ));
        assert!(matches!(
            &dependency_report.requests[1].outcome,
            SerializedFileDependencyOutcome::Ambiguous {
                candidate_count: 2,
                candidate_paths,
            } if candidate_paths == &["/bundles/a/dup.assets", "/bundles/b/dup.assets"]
        ));
        assert_eq!(
            dependency_report.requests[2].outcome,
            SerializedFileDependencyOutcome::Missing
        );
        assert_eq!(
            dependency_report.requests[3].outcome,
            SerializedFileDependencyOutcome::ContainerExpanded {
                virtual_path: "/game/Data/archive.webdata".to_owned(),
                kind: FileKind::UnityWebData,
                mounted_entries: 0,
                mounted_bytes: 0,
                serialized_files_loaded: 0,
                matching_serialized_files: 0,
            }
        );
    }

    #[test]
    fn expands_dependency_containers_and_loads_their_serialized_file_leaves() {
        let mut filesystem = VirtualFileSystem::new();
        let serialized_leaf = make_generation_10_file();
        let container = make_webdata_fixture(&[("shared.assets", &serialized_leaf)]);
        insert_file(
            &mut filesystem,
            "/game/main.assets",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(&mut filesystem, "/game/shared.assets", &container);
        let inputs = [input("/game/main.assets")];
        let data_paths = ["/game".to_owned()];

        let (workspace, dependency_report) =
            SerializedFileWorkspace::load_from_vfs_with_dependencies(
                &mut filesystem,
                &inputs,
                &[],
                &data_paths,
                SerializedFileWorkspaceLimits::default(),
                DependencyIndexLimits::default(),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(workspace.files().len(), 2);
        assert_eq!(workspace.files()[1].file_name, "shared.assets");
        assert_eq!(
            workspace.files()[1].container_scope.as_deref(),
            Some("/__assetripper_dependency_mount_0")
        );
        assert!(dependency_report.is_complete());
        assert_eq!(
            dependency_report.requests[0].outcome,
            SerializedFileDependencyOutcome::ContainerExpanded {
                virtual_path: "/game/shared.assets".to_owned(),
                kind: FileKind::UnityWebData,
                mounted_entries: 1,
                mounted_bytes: u64::try_from(serialized_leaf.len()).unwrap(),
                serialized_files_loaded: 1,
                matching_serialized_files: 1,
            }
        );
        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(graph.references().len(), 2);
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn enforces_aggregate_dependency_container_input_entry_and_output_limits() {
        let inputs = [input("/game/main.assets")];
        let data_paths = ["/game".to_owned()];

        let mut filesystem = dependency_container_filesystem();
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &mut filesystem,
            &inputs,
            &[],
            &data_paths,
            SerializedFileWorkspaceLimits {
                max_total_dependency_container_input_bytes: 1,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalDependencyContainerInputBytesLimitExceeded {
                limit: 1,
                ..
            }
        ));
        assert!(!filesystem
            .directory_exists(Some("/__assetripper_dependency_mount_0"))
            .unwrap());

        let mut filesystem = dependency_container_filesystem();
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &mut filesystem,
            &inputs,
            &[],
            &data_paths,
            SerializedFileWorkspaceLimits {
                max_total_dependency_container_entries: 0,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::ContainerMount(ref mount_error)
                if matches!(
                    mount_error.kind,
                    ContainerMountErrorKind::EntryCountLimitExceeded { limit: 0, .. }
                )
        ));

        let mut filesystem = dependency_container_filesystem();
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &mut filesystem,
            &inputs,
            &[],
            &data_paths,
            SerializedFileWorkspaceLimits {
                max_total_dependency_container_output_bytes: 1,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::ContainerMount(ref mount_error)
                if matches!(
                    mount_error.kind,
                    ContainerMountErrorKind::TotalOutputLimitExceeded { limit: 1, .. }
                )
        ));
    }

    #[test]
    fn bounds_dependency_request_walk() {
        let bytes = make_generation_10_file_with(&["shared.assets"], 1, 10, 42);
        let mut filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &mut filesystem,
            &inputs,
            &[],
            &[],
            SerializedFileWorkspaceLimits {
                max_dependency_requests: 0,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();
        assert_eq!(error.report.completed_files, 1);
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::DependencyRequestLimitExceeded { count: 1, limit: 0 }
        ));
    }

    #[test]
    fn applies_aggregate_object_limits_to_recursively_loaded_dependencies() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/main.assets",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(
            &mut filesystem,
            "/game/shared.assets",
            &make_generation_10_file(),
        );
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &mut filesystem,
            &inputs,
            &[],
            &[],
            SerializedFileWorkspaceLimits {
                max_total_objects: 1,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();

        assert_eq!(error.report.completed_files, 1);
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalObjectLimitExceeded {
                requested_total: 1,
                limit: 0
            }
        ));
    }

    #[test]
    fn preflights_aggregate_input_before_loading_any_file() {
        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits {
                max_total_input_bytes: 1,
                ..SerializedFileWorkspaceLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.report.completed_files, 0);
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalInputLimitExceeded { limit: 1, .. }
        ));
    }

    #[test]
    fn rejects_nonserialized_files_and_object_count_overruns_with_context() {
        let resource_filesystem = virtual_file_system("/game/resource.resS", &[1, 2, 3]);
        let resource_inputs = [input("/game/resource.resS")];
        let error = SerializedFileWorkspace::load_from_vfs(
            &resource_filesystem,
            &resource_inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap_err();
        assert_eq!(error.input_path.as_deref(), Some("/game/resource.resS"));
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::NotSerializedFile {
                detected_kind: FileKind::Resource
            }
        ));

        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits {
                max_total_objects: 0,
                ..SerializedFileWorkspaceLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.report.completed_files, 0);
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalObjectLimitExceeded {
                requested_total: 1,
                limit: 0
            }
        ));
    }
}
