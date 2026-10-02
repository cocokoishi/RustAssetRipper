//! Bounded loading of named SerializedFiles from the mounted VFS.

use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use crate::io::dependency::{
    DependencyDispatchError, DependencyFilePath, DependencyIndex, DependencyIndexBuildError,
    DependencyIndexLimits, DependencyLookup, DependencyLookupError,
};
use crate::io::filesystem::{SearchOption, VirtualFileSystem};
use crate::io::special_names::fix_file_identifier;
use crate::scheme::{FileKind, ParsedFile, SchemeError, SchemeReader, SchemeReaderLimits};

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

impl SerializedFileDependencyReport {
    /// True only when every external request resolved to one loaded SerializedFile identity.
    pub fn is_complete(&self) -> bool {
        self.requests.iter().all(|request| {
            matches!(
                &request.outcome,
                SerializedFileDependencyOutcome::AlreadyLoaded { .. }
                    | SerializedFileDependencyOutcome::LoadedSerializedFile { .. }
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
    ContainerRequiresExpansion {
        virtual_path: String,
        kind: FileKind,
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
    /// supplied source identities. DataPaths retain their source ordering. Containers are
    /// reported explicitly and are not expanded by this method.
    pub fn load_from_vfs_with_dependencies(
        filesystem: &VirtualFileSystem,
        inputs: &[DependencyFilePath],
        registered_files: &[DependencyFilePath],
        data_paths: &[String],
        limits: SerializedFileWorkspaceLimits,
        index_limits: DependencyIndexLimits,
    ) -> Result<(Self, SerializedFileDependencyReport), Box<SerializedFileWorkspaceError>> {
        let mut workspace = Self::load_from_vfs(filesystem, inputs, limits)?;
        let registry = dependency_registry(filesystem, inputs, registered_files, index_limits)
            .map_err(|kind| workspace_error(workspace.report, None, kind))?;
        let index = DependencyIndex::with_limits(filesystem, &registry, data_paths, index_limits)
            .map_err(|error| {
            workspace_error(
                workspace.report,
                None,
                SerializedFileWorkspaceErrorKind::DependencyIndexBuild(Box::new(error)),
            )
        })?;
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
                    None => match index.resolve(&identifier).map_err(|error| {
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
                            } else {
                                let dispatched = index
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
                                drop(dispatched);
                                match detected_kind {
                                    FileKind::SerializedFile => {
                                        let requested_loaded_file_count = workspace
                                            .files
                                            .len()
                                            .checked_add(1)
                                            .ok_or_else(|| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    SerializedFileWorkspaceErrorKind::FileCountOverflow,
                                                )
                                            })?;
                                        loaded_names.try_reserve(1).map_err(|_| {
                                            workspace_error(
                                                workspace.report,
                                                Some(&candidate.virtual_path),
                                                SerializedFileWorkspaceErrorKind::AllocationFailed {
                                                    resource: "loaded dependency identities",
                                                    requested: requested_loaded_file_count,
                                                },
                                            )
                                        })?;
                                        loaded_paths.try_reserve(1).map_err(|_| {
                                            workspace_error(
                                                workspace.report,
                                                Some(&candidate.virtual_path),
                                                SerializedFileWorkspaceErrorKind::AllocationFailed {
                                                    resource: "loaded dependency paths",
                                                    requested: requested_loaded_file_count,
                                                },
                                            )
                                        })?;
                                        let dependency_input = DependencyFilePath {
                                            file_name: file_name_from_virtual_path(
                                                &candidate.virtual_path,
                                            ),
                                            virtual_path: candidate.virtual_path.clone(),
                                        };
                                        let additional = Self::load_from_vfs(
                                            filesystem,
                                            std::slice::from_ref(&dependency_input),
                                            remaining_workspace_limits(
                                                limits,
                                                workspace.report,
                                                workspace.files.len(),
                                            ),
                                        )
                                        .map_err(|mut error| {
                                            error.report = workspace.report;
                                            error
                                        })?;
                                        let file_index = workspace.files.len();
                                        append_workspace(&mut workspace, additional).map_err(
                                            |kind| {
                                                workspace_error(
                                                    workspace.report,
                                                    Some(&candidate.virtual_path),
                                                    kind,
                                                )
                                            },
                                        )?;
                                        let loaded_file = &workspace.files[file_index];
                                        let loaded_name = fix_file_identifier(&loaded_file.file_name);
                                        loaded_names
                                            .entry(loaded_name)
                                            .and_modify(|(_, count)| *count += 1)
                                            .or_insert((file_index, 1));
                                        loaded_paths
                                            .insert(loaded_file.virtual_path.clone(), file_index);
                                        dependency_report.loaded_serialized_files += 1;
                                        SerializedFileDependencyOutcome::LoadedSerializedFile {
                                            file_index,
                                            virtual_path: candidate.virtual_path,
                                        }
                                    }
                                    FileKind::Resource => {
                                        SerializedFileDependencyOutcome::ResourceCandidate {
                                            virtual_path: candidate.virtual_path,
                                        }
                                    }
                                    kind => {
                                        SerializedFileDependencyOutcome::ContainerRequiresExpansion {
                                            virtual_path: candidate.virtual_path,
                                            kind,
                                        }
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
    use crate::scheme::FileKind;
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
                &filesystem,
                &inputs,
                &[],
                &data_paths,
                SerializedFileWorkspaceLimits::default(),
                DependencyIndexLimits::default(),
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
                &filesystem,
                &inputs,
                &[],
                &data_paths,
                SerializedFileWorkspaceLimits::default(),
                DependencyIndexLimits::default(),
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
            SerializedFileDependencyOutcome::ContainerRequiresExpansion {
                virtual_path: "/game/Data/archive.webdata".to_owned(),
                kind: FileKind::UnityWebData,
            }
        );
    }

    #[test]
    fn bounds_dependency_request_walk() {
        let bytes = make_generation_10_file_with(&["shared.assets"], 1, 10, 42);
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &filesystem,
            &inputs,
            &[],
            &[],
            SerializedFileWorkspaceLimits {
                max_dependency_requests: 0,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
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
            &filesystem,
            &inputs,
            &[],
            &[],
            SerializedFileWorkspaceLimits {
                max_total_objects: 1,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
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
