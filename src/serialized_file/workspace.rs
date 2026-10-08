//! Bounded loading of named SerializedFiles from the mounted VFS.
//!
//! Deliberately open: files that fail to parse abort the load with context instead of
//! becoming baseline `FailedFile` records, so workspace resolution only ever sees files
//! that loaded successfully.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use super::header::SerializedFileHeader;
use super::metadata::SerializedFileMetadata;
use super::reference_graph::{
    MultiFileReferenceGraph, MultiFileReferenceGraphBuildError, MultiFileReferenceGraphLimits,
    NamedObjectReferenceGraph, ObjectReferenceGraph, ReferenceGraphBuildError,
    ReferenceGraphLimits,
};
use crate::io::dependency::{
    DependencyDispatchError, DependencyFilePath, DependencyIndex, DependencyIndexBuildError,
    DependencyIndexLimits, DependencyLookup, DependencyLookupError,
};
use crate::io::filesystem::{SearchOption, VirtualFileSystem};
use crate::io::special_names::{
    fix_file_identifier, BUILTIN_EXTRA_NAME_1, BUILTIN_EXTRA_NAME_2, DEFAULT_RESOURCE_NAME_1,
    DEFAULT_RESOURCE_NAME_2,
};
use crate::io::stream::logical_file_name;
use crate::scheme::{
    ContainerMountError, ContainerMountLimits, DispatchedFile, FailedContainerEntry, FileKind,
    MountedContainerBundle, MountedResourceFile, MountedSerializedFile, ParsedFile, SchemeError,
    SchemeReader, SchemeReaderLimits,
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
    pub max_total_bundle_node_path_bytes: usize,
    pub max_total_bundle_name_bytes: usize,
    pub max_total_bundle_resource_file_path_bytes: usize,
    pub max_total_bundle_resource_file_name_bytes: usize,
    pub max_total_failed_container_entries: usize,
    pub max_total_failed_container_metadata_bytes: usize,
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
            max_total_bundle_node_path_bytes: 64 * 1024 * 1024,
            max_total_bundle_name_bytes: 64 * 1024 * 1024,
            max_total_bundle_resource_file_path_bytes: 64 * 1024 * 1024,
            max_total_bundle_resource_file_name_bytes: 64 * 1024 * 1024,
            max_total_failed_container_entries: 1_000_000,
            max_total_failed_container_metadata_bytes: 64 * 1024 * 1024,
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
    pub bundle_resource_file_name_bytes: usize,
    pub bundle_name_bytes: usize,
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
    /// Source entry index inside `container_scope`, if this file came from a container.
    pub container_entry_index: Option<usize>,
    /// Zero-based collection order among direct SerializedFiles in `container_scope`.
    pub container_collection_index: Option<usize>,
    /// Global index of the source container node whose direct collection list includes this file.
    pub container_bundle_index: Option<usize>,
    pub header: SerializedFileHeader,
    pub metadata: SerializedFileMetadata,
    pub reference_graph: ObjectReferenceGraph,
}

/// A loaded set of SerializedFiles whose cross-file references can be joined on demand.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SerializedFileWorkspace {
    files: Vec<LoadedSerializedFile>,
    container_bundles: Vec<WorkspaceSerializedBundle>,
    resource_files: Vec<WorkspaceResourceFile>,
    failed_container_entries: Vec<WorkspaceFailedContainerEntry>,
    failed_container_metadata_bytes: usize,
    report: SerializedFileWorkspaceReport,
}

/// A source-style FailedFile retained while valid sibling entries remain available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceFailedContainerEntry {
    pub source_container_path: String,
    pub entry: FailedContainerEntry,
}

/// A source container node attached to its parent, direct workspace collections, and resources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSerializedBundle {
    pub scope_path: Option<String>,
    /// Source FileContainer.Name after SpecialFileNames.FixFileIdentifier.
    pub name_fixed: Option<String>,
    pub parent_bundle_index: Option<usize>,
    pub parent_entry_index: Option<usize>,
    pub direct_collection_file_indices: Vec<usize>,
    pub direct_resource_file_indices: Vec<usize>,
}

/// Metadata for a mounted ResourceFile leaf; its bytes remain in the source VFS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceResourceFile {
    pub virtual_path: String,
    /// Original source container entry name, before the workspace mount root was added.
    pub name: String,
    /// `name` after `SpecialFileNames.FixFileIdentifier`, cached at registration the
    /// way `FileBase` fixes `NameFixed` when `Name` is assigned, so lookups never
    /// renormalize. The cached copy is derived from the already-budgeted `name` and
    /// carries no separate aggregate byte accounting.
    pub name_fixed: String,
    pub container_bundle_index: usize,
    pub container_entry_index: usize,
    pub container_resource_index: usize,
}

/// A ResourceFile passed explicitly as a root GameBundle input rather than mounted from a Bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRootResourceFile {
    pub virtual_path: String,
    pub name: String,
}

/// Identifies whether a ResourceFile match came from a mounted Bundle or an explicit root input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceResolvedResourceFile {
    MountedBundle { resource_index: usize },
    ExplicitRoot { resource_index: usize },
}

/// A malformed workspace reference encountered while resolving a bundled ResourceFile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceResourceLookupError {
    BundleIndexOutOfBounds { index: usize, count: usize },
    ParentBundleIndexOutOfBounds { index: usize, count: usize },
    ResourceFileIndexOutOfBounds { index: usize, count: usize },
    RootResourceFileIndexOutOfBounds { index: usize, count: usize },
    BundleParentCycle,
}

impl Display for WorkspaceResourceLookupError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::BundleIndexOutOfBounds { index, count } => write!(
                formatter,
                "resource lookup bundle index {index} is outside {count} bundles"
            ),
            Self::ParentBundleIndexOutOfBounds { index, count } => write!(
                formatter,
                "resource lookup parent bundle index {index} is outside {count} bundles"
            ),
            Self::ResourceFileIndexOutOfBounds { index, count } => write!(
                formatter,
                "resource lookup ResourceFile index {index} is outside {count} resources"
            ),
            Self::RootResourceFileIndexOutOfBounds { index, count } => write!(
                formatter,
                "resource lookup root ResourceFile index {index} is outside {count} inputs"
            ),
            Self::BundleParentCycle => formatter
                .write_str("resource lookup encountered a cycle in the container bundle hierarchy"),
        }
    }
}

impl Error for WorkspaceResourceLookupError {}

struct SourceBundleCollectionLookup<'a> {
    workspace: &'a SerializedFileWorkspace,
    fixed_file_names: Vec<String>,
    root_collection_file_indices: Vec<usize>,
    /// Root Bundle children retain workspace insertion order, as Bundle.ResolveCollection scans.
    root_bundle_indices: Vec<usize>,
    /// Nested Bundle children are ordered by their source entry positions.
    child_bundle_entries: Vec<Vec<(usize, usize)>>,
}

/// `Bundle.ResolveCollection(string)`: the ordered lookup names for one request — the raw
/// name, then the fixed name (skipped when the request is already normalized, since the
/// source walk would just repeat itself), then the engine-resource spelling alias. The
/// caller runs each candidate through the full bundle-hierarchy walk before trying the next.
fn collection_lookup_candidates<'a>(
    requested_name: &'a str,
    fixed_name: &'a str,
) -> [Option<&'a str>; 3] {
    [
        Some(requested_name),
        (fixed_name != requested_name).then_some(fixed_name),
        engine_resource_collection_alias(fixed_name),
    ]
}

/// `Bundle.ResolveCollection`'s engine-resource fallback: each fixed engine name also tries
/// its space/underscore twin (`unity default resources` ↔ `unity_default_resources`,
/// `unity builtin extra` ↔ `unity_builtin_extra`). Other names, including the editor
/// resources name, have no alias.
fn engine_resource_collection_alias(fixed_name: &str) -> Option<&str> {
    match fixed_name {
        DEFAULT_RESOURCE_NAME_1 => Some(DEFAULT_RESOURCE_NAME_2),
        DEFAULT_RESOURCE_NAME_2 => Some(DEFAULT_RESOURCE_NAME_1),
        BUILTIN_EXTRA_NAME_1 => Some(BUILTIN_EXTRA_NAME_2),
        BUILTIN_EXTRA_NAME_2 => Some(BUILTIN_EXTRA_NAME_1),
        _ => None,
    }
}

/// `Bundle.ResolveResource` hierarchy-walk step bound over caller facts: the walk
/// ascends one level per step and finishes at the root scope, so `bundle_count + 1`
/// steps always suffice; anything beyond that is a parent cycle (the source would
/// loop forever here), reported as `BundleParentCycle` instead of hanging.
fn bundle_walk_step_limit(bundle_count: usize) -> usize {
    bundle_count.saturating_add(1)
}

impl<'a> SourceBundleCollectionLookup<'a> {
    fn build(
        workspace: &'a SerializedFileWorkspace,
        limits: MultiFileReferenceGraphLimits,
    ) -> Result<Self, MultiFileReferenceGraphBuildError> {
        if workspace.files.len() > limits.max_files {
            return Err(MultiFileReferenceGraphBuildError::FileCountLimitExceeded {
                file_count: workspace.files.len(),
                limit: limits.max_files,
            });
        }
        let source_order = workspace
            .source_collection_file_indices()
            .map_err(|error| {
                MultiFileReferenceGraphBuildError::SourceCollectionTopologyInvalid {
                    message: error.to_string(),
                }
            })?;

        let mut fixed_file_names = Vec::new();
        fixed_file_names
            .try_reserve_exact(workspace.files.len())
            .map_err(|_| MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "source collection fixed names",
                requested: workspace.files.len(),
            })?;
        let mut total_file_name_bytes = 0usize;
        for (file_index, file) in workspace.files.iter().enumerate() {
            if file.file_name.len() > limits.max_file_name_bytes {
                return Err(MultiFileReferenceGraphBuildError::FileNameLimitExceeded {
                    file_index,
                    byte_count: file.file_name.len(),
                    limit: limits.max_file_name_bytes,
                });
            }
            let fixed_name = fix_file_identifier(&file.file_name);
            if fixed_name.len() > limits.max_file_name_bytes {
                return Err(MultiFileReferenceGraphBuildError::FileNameLimitExceeded {
                    file_index,
                    byte_count: fixed_name.len(),
                    limit: limits.max_file_name_bytes,
                });
            }
            total_file_name_bytes = total_file_name_bytes
                .checked_add(fixed_name.len())
                .ok_or(MultiFileReferenceGraphBuildError::TotalFileNameBytesOverflow)?;
            if total_file_name_bytes > limits.max_total_file_name_bytes {
                return Err(
                    MultiFileReferenceGraphBuildError::TotalFileNameBytesLimitExceeded {
                        byte_count: total_file_name_bytes,
                        limit: limits.max_total_file_name_bytes,
                    },
                );
            }
            fixed_file_names.push(fixed_name);
        }

        let mut root_collection_file_indices = Vec::new();
        root_collection_file_indices
            .try_reserve_exact(workspace.files.len())
            .map_err(|_| MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "root collection indexes",
                requested: workspace.files.len(),
            })?;
        root_collection_file_indices.extend(source_order.into_iter().filter(|file_index| {
            workspace
                .files
                .get(*file_index)
                .is_some_and(|file| file.container_bundle_index.is_none())
        }));

        let bundle_count = workspace.container_bundles.len();
        let mut root_bundle_indices = Vec::new();
        root_bundle_indices
            .try_reserve_exact(bundle_count)
            .map_err(|_| MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "root collection bundle indexes",
                requested: bundle_count,
            })?;
        let mut child_bundle_entries = Vec::new();
        child_bundle_entries
            .try_reserve_exact(bundle_count)
            .map_err(|_| MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "nested collection bundle adjacency",
                requested: bundle_count,
            })?;
        child_bundle_entries.resize_with(bundle_count, Vec::new);
        for (bundle_index, bundle) in workspace.container_bundles.iter().enumerate() {
            match (bundle.parent_bundle_index, bundle.parent_entry_index) {
                (None, None) => root_bundle_indices.push(bundle_index),
                (Some(parent), Some(entry_index)) if parent < bundle_index => {
                    let Some(children) = child_bundle_entries.get_mut(parent) else {
                        return Err(
                            MultiFileReferenceGraphBuildError::SourceCollectionTopologyInvalid {
                                message: format!(
                                    "bundle {bundle_index} has invalid parent bundle {parent}"
                                ),
                            },
                        );
                    };
                    children.try_reserve(1).map_err(|_| {
                        MultiFileReferenceGraphBuildError::AllocationFailed {
                            resource: "nested collection bundle indexes",
                            requested: children.len().saturating_add(1),
                        }
                    })?;
                    children.push((entry_index, bundle_index));
                }
                _ => {
                    return Err(
                        MultiFileReferenceGraphBuildError::SourceCollectionTopologyInvalid {
                            message: format!(
                                "bundle {bundle_index} has an invalid parent/entry relation"
                            ),
                        },
                    );
                }
            }
        }
        for children in &mut child_bundle_entries {
            children.sort_unstable_by_key(|(entry_index, _)| *entry_index);
            if children.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                return Err(
                    MultiFileReferenceGraphBuildError::SourceCollectionTopologyInvalid {
                        message: "sibling bundles have duplicate source entry indexes".to_owned(),
                    },
                );
            }
        }

        Ok(Self {
            workspace,
            fixed_file_names,
            root_collection_file_indices,
            root_bundle_indices,
            child_bundle_entries,
        })
    }

    fn resolve(&self, source_file_index: usize, requested_name: &str) -> Option<usize> {
        let source_file = self.workspace.files.get(source_file_index)?;
        let fixed_name = fix_file_identifier(requested_name);
        for candidate in collection_lookup_candidates(requested_name, &fixed_name)
            .into_iter()
            .flatten()
        {
            if let Some(file_index) =
                self.resolve_name_in_source_scope(source_file.container_bundle_index, candidate)
            {
                return Some(file_index);
            }
        }
        None
    }

    fn resolve_name_in_source_scope(
        &self,
        source_bundle_index: Option<usize>,
        name: &str,
    ) -> Option<usize> {
        let mut current_bundle_index = source_bundle_index;
        let mut excluded_child_bundle = None;
        loop {
            let direct_collections = match current_bundle_index {
                Some(bundle_index) => {
                    &self
                        .workspace
                        .container_bundles
                        .get(bundle_index)?
                        .direct_collection_file_indices
                }
                None => &self.root_collection_file_indices,
            };
            if let Some(file_index) = self.find_name(direct_collections, name) {
                return Some(file_index);
            }

            let child_bundles = match current_bundle_index {
                Some(bundle_index) => self.child_bundle_entries.get(bundle_index)?,
                None => {
                    for &bundle_index in &self.root_bundle_indices {
                        if Some(bundle_index) == excluded_child_bundle {
                            continue;
                        }
                        let bundle = self.workspace.container_bundles.get(bundle_index)?;
                        if let Some(file_index) =
                            self.find_name(&bundle.direct_collection_file_indices, name)
                        {
                            return Some(file_index);
                        }
                    }
                    return None;
                }
            };
            for &(_, child_bundle_index) in child_bundles {
                if Some(child_bundle_index) == excluded_child_bundle {
                    continue;
                }
                let child_bundle = self.workspace.container_bundles.get(child_bundle_index)?;
                if let Some(file_index) =
                    self.find_name(&child_bundle.direct_collection_file_indices, name)
                {
                    return Some(file_index);
                }
            }

            let bundle_index = current_bundle_index?;
            let bundle = self.workspace.container_bundles.get(bundle_index)?;
            excluded_child_bundle = Some(bundle_index);
            current_bundle_index = bundle.parent_bundle_index;
        }
    }

    fn find_name(&self, file_indices: &[usize], name: &str) -> Option<usize> {
        file_indices.iter().copied().find(|file_index| {
            self.fixed_file_names
                .get(*file_index)
                .is_some_and(|fixed_name| fixed_name == name)
        })
    }
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
    pub container_bundles: Vec<MountedContainerBundle>,
    pub serialized_files: Vec<MountedSerializedFile>,
    pub resource_files: Vec<MountedResourceFile>,
    pub failed_entries: Vec<FailedContainerEntry>,
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
                let graph = ObjectReferenceGraph::build_with_generated_type_tree_fallback(
                    &parsed,
                    limits.reference_graph,
                    &input.file_name,
                    &input.virtual_path,
                    crate::processing::asset_factory::reference_class_layouts(),
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
        match self.resolve_bundled_or_root_resource_file(
            source_bundle_index,
            requested_name,
            &[],
        )? {
            Some(WorkspaceResolvedResourceFile::MountedBundle { resource_index }) => {
                Ok(Some(resource_index))
            }
            Some(WorkspaceResolvedResourceFile::ExplicitRoot { resource_index }) => Err(
                WorkspaceResourceLookupError::RootResourceFileIndexOutOfBounds {
                    index: resource_index,
                    count: 0,
                },
            ),
            None => Ok(None),
        }
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
        if requested_name.is_empty() {
            return Ok(None);
        }
        if let Some(index) = source_bundle_index {
            if index >= self.container_bundles.len() {
                return Err(WorkspaceResourceLookupError::BundleIndexOutOfBounds {
                    index,
                    count: self.container_bundles.len(),
                });
            }
        }

        let fixed_name = fix_file_identifier(requested_name);
        let mut current_bundle_index = source_bundle_index;
        let mut excluded_child_bundle = None;
        for _ in 0..=bundle_walk_step_limit(self.container_bundles.len()) {
            if let Some(bundle_index) = current_bundle_index {
                if let Some(resource_index) =
                    self.find_resource_in_bundle(bundle_index, &fixed_name)?
                {
                    return Ok(Some(WorkspaceResolvedResourceFile::MountedBundle {
                        resource_index,
                    }));
                }
                for (child_index, child) in self.container_bundles.iter().enumerate() {
                    if child.parent_bundle_index != Some(bundle_index)
                        || Some(child_index) == excluded_child_bundle
                    {
                        continue;
                    }
                    if let Some(resource_index) =
                        self.find_resource_in_bundle(child_index, &fixed_name)?
                    {
                        return Ok(Some(WorkspaceResolvedResourceFile::MountedBundle {
                            resource_index,
                        }));
                    }
                }
            } else {
                if let Some((resource_index, _)) = root_resources
                    .iter()
                    .enumerate()
                    .find(|(_, resource)| fix_file_identifier(&resource.name) == fixed_name)
                {
                    return Ok(Some(WorkspaceResolvedResourceFile::ExplicitRoot {
                        resource_index,
                    }));
                }
                for (child_index, child) in self.container_bundles.iter().enumerate() {
                    if child.parent_bundle_index.is_some()
                        || Some(child_index) == excluded_child_bundle
                    {
                        continue;
                    }
                    if let Some(resource_index) =
                        self.find_resource_in_bundle(child_index, &fixed_name)?
                    {
                        return Ok(Some(WorkspaceResolvedResourceFile::MountedBundle {
                            resource_index,
                        }));
                    }
                }
                return Ok(None);
            }

            let Some(bundle_index) = current_bundle_index else {
                return Ok(None);
            };
            let bundle = self.container_bundles.get(bundle_index).ok_or(
                WorkspaceResourceLookupError::BundleIndexOutOfBounds {
                    index: bundle_index,
                    count: self.container_bundles.len(),
                },
            )?;
            if let Some(parent_index) = bundle.parent_bundle_index {
                if parent_index >= self.container_bundles.len() {
                    return Err(WorkspaceResourceLookupError::ParentBundleIndexOutOfBounds {
                        index: parent_index,
                        count: self.container_bundles.len(),
                    });
                }
            }
            excluded_child_bundle = Some(bundle_index);
            current_bundle_index = bundle.parent_bundle_index;
        }
        Err(WorkspaceResourceLookupError::BundleParentCycle)
    }

    fn find_resource_in_bundle(
        &self,
        bundle_index: usize,
        fixed_name: &str,
    ) -> Result<Option<usize>, WorkspaceResourceLookupError> {
        let bundle = self.container_bundles.get(bundle_index).ok_or(
            WorkspaceResourceLookupError::BundleIndexOutOfBounds {
                index: bundle_index,
                count: self.container_bundles.len(),
            },
        )?;
        for &resource_index in &bundle.direct_resource_file_indices {
            let resource = self.resource_files.get(resource_index).ok_or(
                WorkspaceResourceLookupError::ResourceFileIndexOutOfBounds {
                    index: resource_index,
                    count: self.resource_files.len(),
                },
            )?;
            if resource.name_fixed.as_str() == fixed_name {
                return Ok(Some(resource_index));
            }
        }
        Ok(None)
    }

    /// Returns workspace indexes for direct collections in one mounted container, ordered as
    /// `SerializedBundle.Collections` is ordered by the source baseline.
    pub fn ordered_collection_file_indices(
        &self,
        container_scope: &str,
    ) -> Result<Vec<usize>, Box<SerializedFileWorkspaceError>> {
        let matching_count = self
            .files
            .iter()
            .filter(|file| file.container_scope.as_deref() == Some(container_scope))
            .count();
        let mut ordered_files = Vec::new();
        ordered_files
            .try_reserve_exact(matching_count)
            .map_err(|_| {
                workspace_error(
                    self.report,
                    Some(container_scope),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "ordered container collection indexes",
                        requested: matching_count,
                    },
                )
            })?;
        for (file_index, file) in self.files.iter().enumerate() {
            if file.container_scope.as_deref() != Some(container_scope) {
                continue;
            }
            let Some(collection_index) = file.container_collection_index else {
                return Err(workspace_error(
                    self.report,
                    Some(&file.virtual_path),
                    SerializedFileWorkspaceErrorKind::MissingContainerCollectionOrder,
                ));
            };
            ordered_files.push((collection_index, file_index, file.virtual_path.as_str()));
        }
        ordered_files.sort_unstable_by_key(|(collection_index, _, _)| *collection_index);
        for (expected_index, (actual_index, _, path)) in ordered_files.iter().enumerate() {
            if *actual_index != expected_index {
                return Err(workspace_error(
                    self.report,
                    Some(path),
                    SerializedFileWorkspaceErrorKind::InvalidContainerCollectionSequence {
                        expected: expected_index,
                        actual: *actual_index,
                    },
                ));
            }
        }
        let mut file_indices = Vec::new();
        file_indices
            .try_reserve_exact(ordered_files.len())
            .map_err(|_| {
                workspace_error(
                    self.report,
                    Some(container_scope),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "ordered container collection file indexes",
                        requested: ordered_files.len(),
                    },
                )
            })?;
        file_indices.extend(
            ordered_files
                .into_iter()
                .map(|(_, file_index, _)| file_index),
        );
        Ok(file_indices)
    }

    /// Returns all collections in the order used by source `GameBundle.FetchAssetCollections`.
    /// Root standalone collections are emitted first in reverse file-stack order, followed by
    /// root containers in reverse insertion order; each container emits direct collections then
    /// child containers ordered by their containing source entry. These rules are applied to the
    /// deterministic file and bundle insertion order retained by this workspace.
    pub fn source_collection_file_indices(
        &self,
    ) -> Result<Vec<usize>, Box<SerializedFileWorkspaceError>> {
        let bundle_count = self.container_bundles.len();
        let mut children = Vec::<Vec<(usize, usize)>>::new();
        children.try_reserve_exact(bundle_count).map_err(|_| {
            workspace_error(
                self.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "source collection bundle adjacency",
                    requested: bundle_count,
                },
            )
        })?;
        children.resize_with(bundle_count, Vec::new);

        let mut root_bundles = Vec::new();
        root_bundles.try_reserve_exact(bundle_count).map_err(|_| {
            workspace_error(
                self.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "source collection root bundle indexes",
                    requested: bundle_count,
                },
            )
        })?;
        for (bundle_index, bundle) in self.container_bundles.iter().enumerate() {
            match (bundle.parent_bundle_index, bundle.parent_entry_index) {
                (None, None) => root_bundles.push(bundle_index),
                (Some(parent), Some(entry_index)) if parent < bundle_index => {
                    let Some(parent_children) = children.get_mut(parent) else {
                        return Err(workspace_error(
                            self.report,
                            bundle.scope_path.as_deref(),
                            SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                                bundle_index,
                            },
                        ));
                    };
                    parent_children.try_reserve(1).map_err(|_| {
                        workspace_error(
                            self.report,
                            bundle.scope_path.as_deref(),
                            SerializedFileWorkspaceErrorKind::AllocationFailed {
                                resource: "source collection child bundle indexes",
                                requested: parent_children.len().saturating_add(1),
                            },
                        )
                    })?;
                    parent_children.push((entry_index, bundle_index));
                }
                _ => {
                    return Err(workspace_error(
                        self.report,
                        bundle.scope_path.as_deref(),
                        SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                            bundle_index,
                        },
                    ));
                }
            }
        }
        for (parent_index, parent_children) in children.iter_mut().enumerate() {
            parent_children.sort_unstable_by_key(|(entry_index, _)| *entry_index);
            if parent_children
                .windows(2)
                .any(|pair| pair[0].0 == pair[1].0)
            {
                return Err(workspace_error(
                    self.report,
                    self.container_bundles[parent_index].scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                        bundle_index: parent_index,
                    },
                ));
            }
        }

        let mut ordered_files = Vec::new();
        ordered_files
            .try_reserve_exact(self.files.len())
            .map_err(|_| {
                workspace_error(
                    self.report,
                    None,
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "source collection file indexes",
                        requested: self.files.len(),
                    },
                )
            })?;
        let mut seen_files = Vec::new();
        seen_files
            .try_reserve_exact(self.files.len())
            .map_err(|_| {
                workspace_error(
                    self.report,
                    None,
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "source collection file visitation state",
                        requested: self.files.len(),
                    },
                )
            })?;
        seen_files.resize(self.files.len(), false);

        // GameBundle pops its loaded file stack from the end and stores standalone collections
        // separately from root bundles, so all standalone collections are fetched first.
        for (file_index, file) in self.files.iter().enumerate().rev() {
            if file.container_bundle_index.is_some() {
                continue;
            }
            if file.container_scope.is_some() || file.container_collection_index.is_some() {
                return Err(workspace_error(
                    self.report,
                    Some(&file.virtual_path),
                    SerializedFileWorkspaceErrorKind::InvalidWorkspaceCollectionTraversal {
                        file_index,
                    },
                ));
            }
            seen_files[file_index] = true;
            ordered_files.push(file_index);
        }

        // Bundle insertion follows the same reversed file-stack walk. Push roots in insertion
        // order so the stack visits the final inserted root first, as FetchAssetCollections does.
        let mut stack = Vec::new();
        stack.try_reserve_exact(bundle_count).map_err(|_| {
            workspace_error(
                self.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "source collection bundle traversal stack",
                    requested: bundle_count,
                },
            )
        })?;
        stack.extend(root_bundles);
        let mut seen_bundles = Vec::new();
        seen_bundles.try_reserve_exact(bundle_count).map_err(|_| {
            workspace_error(
                self.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "source collection bundle visitation state",
                    requested: bundle_count,
                },
            )
        })?;
        seen_bundles.resize(bundle_count, false);

        while let Some(bundle_index) = stack.pop() {
            let Some(bundle) = self.container_bundles.get(bundle_index) else {
                return Err(workspace_error(
                    self.report,
                    None,
                    SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                        bundle_index,
                    },
                ));
            };
            if seen_bundles[bundle_index] {
                return Err(workspace_error(
                    self.report,
                    bundle.scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                        bundle_index,
                    },
                ));
            }
            seen_bundles[bundle_index] = true;
            for (collection_index, &file_index) in
                bundle.direct_collection_file_indices.iter().enumerate()
            {
                let Some(file) = self.files.get(file_index) else {
                    return Err(workspace_error(
                        self.report,
                        bundle.scope_path.as_deref(),
                        SerializedFileWorkspaceErrorKind::InvalidWorkspaceCollectionTraversal {
                            file_index,
                        },
                    ));
                };
                if file.container_bundle_index != Some(bundle_index)
                    || file.container_collection_index != Some(collection_index)
                    || file.container_scope.as_deref() != bundle.scope_path.as_deref()
                    || seen_files[file_index]
                {
                    return Err(workspace_error(
                        self.report,
                        Some(&file.virtual_path),
                        SerializedFileWorkspaceErrorKind::InvalidWorkspaceCollectionTraversal {
                            file_index,
                        },
                    ));
                }
                seen_files[file_index] = true;
                ordered_files.push(file_index);
            }

            let bundle_children = &children[bundle_index];
            stack.try_reserve(bundle_children.len()).map_err(|_| {
                workspace_error(
                    self.report,
                    bundle.scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "source collection bundle traversal stack",
                        requested: stack.len().saturating_add(bundle_children.len()),
                    },
                )
            })?;
            for &(_, child_index) in bundle_children.iter().rev() {
                stack.push(child_index);
            }
        }

        if seen_bundles.iter().any(|seen| !seen)
            || seen_files.iter().any(|seen| !seen)
            || ordered_files.len() != self.files.len()
        {
            let bundle_index = seen_bundles.iter().position(|seen| !seen).unwrap_or(0);
            return Err(workspace_error(
                self.report,
                self.container_bundles
                    .get(bundle_index)
                    .and_then(|bundle| bundle.scope_path.as_deref()),
                SerializedFileWorkspaceErrorKind::IncompleteWorkspaceCollectionTraversal,
            ));
        }
        Ok(ordered_files)
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

#[derive(Clone, Copy)]
struct ContainerFileOrder<'a> {
    scope: Option<&'a str>,
    entry_index: usize,
    collection_index: Option<usize>,
}

fn build_mounted_file_order_index<'a>(
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

struct MountedContainerBundleInput<'a> {
    container_scopes: &'a [String],
    mounted_bundles: &'a [MountedContainerBundle],
    serialized_files: &'a [MountedSerializedFile],
    resource_files: &'a [MountedResourceFile],
    failed_entries: &'a [FailedContainerEntry],
    source_container_path: &'a str,
    loaded_paths: &'a HashMap<String, usize>,
    limits: SerializedFileWorkspaceLimits,
    bundle_count_limit: usize,
}

fn append_mounted_container_bundles(
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

fn attach_container_file_order(
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
        max_total_bundle_name_bytes: limits
            .max_total_bundle_name_bytes
            .saturating_sub(report.bundle_name_bytes),
        max_total_bundle_resource_file_name_bytes: limits
            .max_total_bundle_resource_file_name_bytes
            .saturating_sub(report.bundle_resource_file_name_bytes),
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
    ContainerBundleCountOverflow,
    ContainerBundleCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    FailedContainerEntryCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    TotalFailedContainerMetadataLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    InvalidMountedContainerHierarchy {
        bundle_index: usize,
    },
    InvalidWorkspaceCollectionTraversal {
        file_index: usize,
    },
    IncompleteWorkspaceCollectionTraversal,
    MountedContainerCollectionFileMissing {
        virtual_path: String,
    },
    MountedFileOrderCountOverflow,
    MountedFileOrderCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    MountedFileScopeIndexOutOfBounds {
        index: usize,
        count: usize,
    },
    InvalidMountedFileCollectionOrder,
    DuplicateMountedFileOrderPath,
    MountedFileScopeMismatch,
    MissingContainerCollectionOrder,
    InvalidContainerCollectionSequence {
        expected: usize,
        actual: usize,
    },
    BundleScopeByteCountOverflow,
    TotalBundleScopeBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    BundleNameByteCountOverflow,
    TotalBundleNameBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    BundleNodePathByteCountOverflow,
    TotalBundleNodePathBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    ResourceFileCountOverflow,
    BundleResourcePathByteCountOverflow,
    BundleResourceNameByteCountOverflow,
    TotalBundleResourceFilePathBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    TotalBundleResourceFileNameBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    DuplicateMountedResourceFilePath,
    InvalidContainerResourceSequence {
        expected: usize,
        actual: usize,
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
            Self::ContainerBundleCountOverflow => {
                f.write_str("mounted container bundle count overflows")
            }
            Self::ContainerBundleCountLimitExceeded { count, limit } => write!(
                f,
                "mounted container bundle count {count} exceeds workspace limit {limit}"
            ),
            Self::FailedContainerEntryCountLimitExceeded { count, limit } => write!(
                f,
                "failed container entry count {count} exceeds limit {limit}"
            ),
            Self::TotalFailedContainerMetadataLimitExceeded { byte_count, limit } => write!(
                f,
                "failed container entry metadata uses {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::InvalidMountedContainerHierarchy { bundle_index } => write!(
                f,
                "mounted container hierarchy record {bundle_index} has inconsistent parent or scope metadata"
            ),
            Self::InvalidWorkspaceCollectionTraversal { file_index } => write!(
                f,
                "workspace collection traversal has inconsistent links for file {file_index}"
            ),
            Self::IncompleteWorkspaceCollectionTraversal => {
                f.write_str("workspace collection traversal does not cover every file and bundle")
            }
            Self::MountedContainerCollectionFileMissing { virtual_path } => write!(
                f,
                "mounted container collection {virtual_path:?} is not loaded in the workspace"
            ),
            Self::MountedFileOrderCountOverflow => {
                f.write_str("mounted SerializedFile order record count overflows")
            }
            Self::MountedFileOrderCountLimitExceeded { count, limit } => write!(
                f,
                "mounted SerializedFile order record count {count} exceeds limit {limit}"
            ),
            Self::MountedFileScopeIndexOutOfBounds { index, count } => write!(
                f,
                "mounted SerializedFile scope index {index} is outside {count} reported scopes"
            ),
            Self::InvalidMountedFileCollectionOrder => f.write_str(
                "mounted SerializedFile collection order does not match its container scope",
            ),
            Self::DuplicateMountedFileOrderPath => {
                f.write_str("mounted SerializedFile path occurs more than once in order metadata")
            }
            Self::MountedFileScopeMismatch => {
                f.write_str("mounted SerializedFile scope differs from its order metadata")
            }
            Self::MissingContainerCollectionOrder => {
                f.write_str("loaded container collection is missing its source order index")
            }
            Self::InvalidContainerCollectionSequence { expected, actual } => write!(
                f,
                "container collection order is not contiguous: expected index {expected}, found {actual}"
            ),
            Self::BundleScopeByteCountOverflow => {
                f.write_str("loaded bundle-scope byte count overflows")
            }
            Self::TotalBundleScopeBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "loaded bundle scopes use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::BundleNameByteCountOverflow => {
                f.write_str("loaded fixed Bundle name byte count overflows")
            }
            Self::TotalBundleNameBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "loaded fixed Bundle names use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::BundleNodePathByteCountOverflow => {
                f.write_str("container bundle node path byte count overflows")
            }
            Self::TotalBundleNodePathBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "container bundle node paths use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::ResourceFileCountOverflow => {
                f.write_str("mounted ResourceFile count overflows")
            }
            Self::BundleResourcePathByteCountOverflow => {
                f.write_str("mounted bundle ResourceFile path byte count overflows")
            }
            Self::BundleResourceNameByteCountOverflow => {
                f.write_str("mounted bundle ResourceFile name byte count overflows")
            }
            Self::TotalBundleResourceFilePathBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "mounted bundle ResourceFile paths use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::TotalBundleResourceFileNameBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "mounted bundle ResourceFile names use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::DuplicateMountedResourceFilePath => {
                f.write_str("mounted ResourceFile path occurs more than once in bundle metadata")
            }
            Self::InvalidContainerResourceSequence { expected, actual } => write!(
                f,
                "container ResourceFile order is not contiguous: expected index {expected}, found {actual}"
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
        append_mounted_container_bundles, collection_lookup_candidates,
        engine_resource_collection_alias, fix_file_identifier, SerializedFileDependencyOutcome,
        SerializedFileWorkspace, SerializedFileWorkspaceErrorKind, SerializedFileWorkspaceLimits,
        SerializedFileWorkspaceReport, WorkspaceSerializedBundle,
    };
    use crate::io::dependency::{DependencyFilePath, DependencyIndexLimits};
    use crate::io::filesystem::VirtualFileSystem;
    use crate::io::platform_file_inventory::{
        PlatformWorkspaceLoadErrorKind, PlatformWorkspaceLoadOptions,
    };
    use crate::io::platform_structure::{PlatformKind, PlatformStructure};
    use crate::scheme::{
        ContainerMountErrorKind, ContainerMountLimits, FileKind, MountedContainerBundle,
        MountedResourceFile,
    };
    use crate::serialized_file::collection_workspace::{
        WorkspaceAssetCollections, WorkspaceCollectionErrorKind, WorkspaceCollectionLimits,
    };
    use crate::serialized_file::file::SerializedFile;
    use crate::serialized_file::object::SerializedValue;
    use crate::serialized_file::object_workspace::{
        ObjectWorkspace, ObjectWorkspaceErrorKind, ObjectWorkspaceLimits,
    };
    use crate::serialized_file::reference_graph::{
        MultiFilePPtrResolution, MultiFileReferenceGraphLimits, ObjectReferenceGraph,
    };

    #[test]
    fn collection_lookup_candidates_follow_the_resolve_collection_fallback() {
        use crate::io::special_names::{
            BUILTIN_EXTRA_NAME_1, BUILTIN_EXTRA_NAME_2, DEFAULT_RESOURCE_NAME_1,
            DEFAULT_RESOURCE_NAME_2,
        };
        // Normalized non-engine names try only themselves; the source would repeat the same
        // walk, so the fixed stage is skipped.
        assert_eq!(
            collection_lookup_candidates("shared.assets", "shared.assets"),
            [Some("shared.assets"), None, None]
        );
        // Non-normalized names try the raw request before the fixed form.
        assert_eq!(
            collection_lookup_candidates("LIBRARY/Shared.Assets", "shared.assets"),
            [Some("LIBRARY/Shared.Assets"), Some("shared.assets"), None]
        );
        // Engine names add the space/underscore twin in both directions.
        assert_eq!(
            collection_lookup_candidates(DEFAULT_RESOURCE_NAME_1, DEFAULT_RESOURCE_NAME_1),
            [
                Some(DEFAULT_RESOURCE_NAME_1),
                None,
                Some(DEFAULT_RESOURCE_NAME_2)
            ]
        );
        assert_eq!(
            collection_lookup_candidates(DEFAULT_RESOURCE_NAME_2, DEFAULT_RESOURCE_NAME_2),
            [
                Some(DEFAULT_RESOURCE_NAME_2),
                None,
                Some(DEFAULT_RESOURCE_NAME_1)
            ]
        );
        assert_eq!(
            collection_lookup_candidates("library/unity builtin extra", "unity builtin extra"),
            [
                Some("library/unity builtin extra"),
                Some("unity builtin extra"),
                Some(BUILTIN_EXTRA_NAME_2)
            ]
        );
        assert_eq!(
            collection_lookup_candidates(BUILTIN_EXTRA_NAME_1, BUILTIN_EXTRA_NAME_1),
            [Some(BUILTIN_EXTRA_NAME_1), None, Some(BUILTIN_EXTRA_NAME_2)]
        );
        // The editor resources name and ordinary names have no alias.
        assert_eq!(
            engine_resource_collection_alias("unity editor resources"),
            None
        );
        assert_eq!(engine_resource_collection_alias("shared.assets"), None);
        // The fixed form feeds the alias even when the request carried a folder prefix.
        let fixed = fix_file_identifier("resources/unity_default_resources");
        assert_eq!(fixed, DEFAULT_RESOURCE_NAME_2);
        assert_eq!(
            collection_lookup_candidates("resources/unity_default_resources", &fixed),
            [
                Some("resources/unity_default_resources"),
                Some(DEFAULT_RESOURCE_NAME_2),
                Some(DEFAULT_RESOURCE_NAME_1)
            ]
        );
    }

    #[test]
    fn bundled_resource_resolution_matches_bundle_scope_order_and_name_rules() {
        use super::WorkspaceResourceFile;

        let workspace = SerializedFileWorkspace {
            container_bundles: vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/root".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![0],
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/root/first".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(1),
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![1],
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/root/second".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(2),
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![2, 4],
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/root/first/nested".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(1),
                    parent_entry_index: Some(4),
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![3],
                },
            ],
            resource_files: vec![
                WorkspaceResourceFile {
                    virtual_path: "/root/from-parent.resS".to_owned(),
                    name: "from-parent.resS".to_owned(),
                    name_fixed: "from-parent.ress".to_owned(),
                    container_bundle_index: 0,
                    container_entry_index: 0,
                    container_resource_index: 0,
                },
                WorkspaceResourceFile {
                    virtual_path: "/root/first/archive:/textures/Shared.resS".to_owned(),
                    name: "archive:/textures/Shared.resS".to_owned(),
                    name_fixed: "shared.ress".to_owned(),
                    container_bundle_index: 1,
                    container_entry_index: 0,
                    container_resource_index: 0,
                },
                WorkspaceResourceFile {
                    virtual_path: "/root/second/from-parent.resS".to_owned(),
                    name: "from-parent.resS".to_owned(),
                    name_fixed: "from-parent.ress".to_owned(),
                    container_bundle_index: 2,
                    container_entry_index: 0,
                    container_resource_index: 0,
                },
                WorkspaceResourceFile {
                    virtual_path: "/root/first/nested/archive:/other/Shared.resS".to_owned(),
                    name: "archive:/other/Shared.resS".to_owned(),
                    name_fixed: "shared.ress".to_owned(),
                    container_bundle_index: 3,
                    container_entry_index: 0,
                    container_resource_index: 0,
                },
                WorkspaceResourceFile {
                    virtual_path: "/root/second/sibling.resS".to_owned(),
                    name: "sibling.resS".to_owned(),
                    name_fixed: "sibling.ress".to_owned(),
                    container_bundle_index: 2,
                    container_entry_index: 1,
                    container_resource_index: 1,
                },
            ],
            ..SerializedFileWorkspace::default()
        };

        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(1), "ARCHIVE:/request/SHARED.ress")
                .unwrap(),
            Some(1),
            "the current bundle's direct resource precedes its child bundle"
        );
        let resolved_resource = workspace
            .resolve_bundled_resource_file(Some(1), "ARCHIVE:/request/SHARED.ress")
            .unwrap()
            .unwrap();
        let mut filesystem = VirtualFileSystem::new();
        filesystem
            .create_directory(Some("/root/first/archive:/textures"))
            .unwrap();
        filesystem
            .write_all_bytes(
                &workspace.resource_files[resolved_resource].virtual_path,
                b"012345",
            )
            .unwrap();
        assert_eq!(
            crate::resource::read_streamed_resource_bytes(
                &filesystem,
                &workspace.resource_files[resolved_resource].virtual_path,
                1,
                3,
                crate::resource::StreamedResourceLimits::default(),
            )
            .unwrap(),
            Some(b"123".to_vec())
        );
        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(1), "from-parent.resS")
                .unwrap(),
            Some(0),
            "the parent bundle's direct resource precedes a sibling child"
        );
        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(1), "sibling.resS")
                .unwrap(),
            Some(4),
            "the parent's other direct child is searched after its own resources"
        );
        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(1), "archive:/missing/nope.resS")
                .unwrap(),
            None
        );
    }

    #[test]
    fn resource_hierarchy_walk_reports_parent_cycles() {
        use super::WorkspaceResourceLookupError;

        // One step per level plus the root-scope step always suffices.
        assert_eq!(super::bundle_walk_step_limit(0), 1);
        assert_eq!(super::bundle_walk_step_limit(2), 3);
        // Two bundles parenting each other never reach the root scope: the walk
        // reports the cycle instead of hanging (the source would loop forever).
        let workspace = SerializedFileWorkspace {
            container_bundles: vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/a".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(1),
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: Vec::new(),
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/b".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: Vec::new(),
                },
            ],
            resource_files: Vec::new(),
            ..SerializedFileWorkspace::default()
        };
        assert!(matches!(
            workspace.resolve_bundled_resource_file(Some(0), "missing.ress"),
            Err(WorkspaceResourceLookupError::BundleParentCycle)
        ));
    }

    #[test]
    fn unified_resource_resolution_prefers_bundle_then_platform_then_mixed_provider() {
        use super::WorkspaceResourceFile;
        use crate::io::platform_structure::{PlatformResourceProvider, PlatformResourceSource};
        use crate::resource::{resolve_resource_file, ResolvedResourceFile};

        let workspace = SerializedFileWorkspace {
            container_bundles: vec![WorkspaceSerializedBundle {
                scope_path: Some("/bundle".to_owned()),
                name_fixed: None,
                parent_bundle_index: None,
                parent_entry_index: None,
                direct_collection_file_indices: Vec::new(),
                direct_resource_file_indices: vec![0],
            }],
            resource_files: vec![WorkspaceResourceFile {
                virtual_path: "/bundle/shared.resS".to_owned(),
                name: "shared.resS".to_owned(),
                name_fixed: "shared.ress".to_owned(),
                container_bundle_index: 0,
                container_entry_index: 0,
                container_resource_index: 0,
            }],
            ..SerializedFileWorkspace::default()
        };
        let mut filesystem = VirtualFileSystem::new();
        for (path, bytes) in [
            ("/bundle/shared.resS", b"bundle".as_slice()),
            ("/platform/shared.resS", b"platform".as_slice()),
            ("/mixed/shared.resS", b"mixed".as_slice()),
            ("/platform/external.resS", b"external".as_slice()),
            ("/mixed/external.resS", b"mixed-external".as_slice()),
            ("/mixed/mixed-only.resS", b"mixed-only".as_slice()),
        ] {
            let parent = path.rsplit_once('/').unwrap().0;
            filesystem
                .create_directory(Some(parent))
                .unwrap_or_else(|error| {
                    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
                });
            filesystem.write_all_bytes(path, bytes).unwrap();
        }
        let platform = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/platform".to_owned(),
            consumed_paths: vec!["/platform".to_owned()],
            root_path: "/platform".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: None,
            data_paths: vec!["/platform/missing".to_owned(), "/platform".to_owned()],
        };
        let mixed = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/mixed".to_owned(),
            consumed_paths: vec!["/mixed".to_owned()],
            root_path: "/mixed".to_owned(),
            name: Some("Mixed".to_owned()),
            game_data_path: None,
            data_paths: vec!["/mixed".to_owned()],
        };
        let provider = PlatformResourceProvider::new(Some(&platform), Some(&mixed));

        assert_eq!(
            resolve_resource_file(
                &filesystem,
                &workspace,
                None,
                "shared.resS",
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::Bundled {
                resource_index: 0,
                virtual_path: "/bundle/shared.resS",
            },
            "mounted bundle resources precede both external providers"
        );
        assert_eq!(
            resolve_resource_file(
                &filesystem,
                &workspace,
                None,
                "external.resS",
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::External {
                virtual_path: "/platform/external.resS".to_owned(),
                source: PlatformResourceSource::Platform,
                data_path_index: 1,
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            },
            "platform provider precedes Mixed when both contain the resource"
        );
        assert_eq!(
            resolve_resource_file(
                &filesystem,
                &workspace,
                None,
                "mixed-only.resS",
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::External {
                virtual_path: "/mixed/mixed-only.resS".to_owned(),
                source: PlatformResourceSource::Mixed,
                data_path_index: 0,
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            },
            "Mixed is searched only after platform DataPaths"
        );
        assert_eq!(
            resolve_resource_file(
                &filesystem,
                &workspace,
                None,
                "absent.resS",
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::Missing {
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            }
        );
    }

    #[test]
    fn explicit_root_resource_precedes_root_sibling_bundle_and_external_provider() {
        use super::{WorkspaceResourceFile, WorkspaceRootResourceFile};
        use crate::io::platform_structure::PlatformResourceProvider;
        use crate::resource::{
            resolve_resource_file, resolve_resource_file_with_root_catalog,
            ExplicitRootResourceInput, ResolvedResourceFile, RootResourceCatalogLimits,
            RootResourceFileCatalog,
        };
        use crate::scheme::{SchemeReader, SchemeReaderLimits};

        let workspace = SerializedFileWorkspace {
            container_bundles: vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/source".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: Vec::new(),
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/sibling".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![0],
                },
            ],
            resource_files: vec![WorkspaceResourceFile {
                virtual_path: "/sibling/shared.resS".to_owned(),
                name: "shared.resS".to_owned(),
                name_fixed: "shared.ress".to_owned(),
                container_bundle_index: 1,
                container_entry_index: 0,
                container_resource_index: 0,
            }],
            ..SerializedFileWorkspace::default()
        };
        let mut filesystem = VirtualFileSystem::new();
        for path in [
            "/sibling/shared.resS",
            "/explicit/shared.resS",
            "/platform/shared.resS",
        ] {
            let parent = path.rsplit_once('/').unwrap().0;
            filesystem
                .create_directory(Some(parent))
                .unwrap_or_else(|error| {
                    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
                });
            filesystem.write_all_bytes(path, b"resource bytes").unwrap();
        }
        let dispatched =
            SchemeReader::read_with_limits(b"explicit resource", SchemeReaderLimits::default())
                .unwrap();
        let root_inputs = [ExplicitRootResourceInput {
            file_name: "SHARED.resS",
            virtual_path: "/explicit/shared.resS",
            dispatched_file: &dispatched,
        }];
        let root_catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &root_inputs,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(
            root_catalog.files(),
            [WorkspaceRootResourceFile {
                name: "SHARED.resS".to_owned(),
                virtual_path: "/explicit/shared.resS".to_owned(),
            }]
        );
        let platform = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/platform".to_owned(),
            consumed_paths: vec!["/platform".to_owned()],
            root_path: "/platform".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: None,
            data_paths: vec!["/platform".to_owned()],
        };
        let provider = PlatformResourceProvider::new(Some(&platform), None);

        assert_eq!(
            resolve_resource_file(
                &filesystem,
                &workspace,
                Some(0),
                "shared.resS",
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::Bundled {
                resource_index: 0,
                virtual_path: "/sibling/shared.resS",
            },
            "the legacy bundle-only lookup reaches the sibling after the owner branch"
        );
        assert_eq!(
            resolve_resource_file_with_root_catalog(
                &filesystem,
                &workspace,
                Some(0),
                "shared.resS",
                &root_catalog,
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::ExplicitRoot {
                resource_index: 0,
                virtual_path: "/explicit/shared.resS",
            },
            "GameBundle root resources precede sibling Bundles and the external provider"
        );
        assert_eq!(
            resolve_resource_file_with_root_catalog(
                &filesystem,
                &workspace,
                Some(0),
                "other.resS",
                &root_catalog,
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::Missing {
                unavailable_data_paths: Vec::new(),
            }
        );
    }

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
                .map(|(name, _)| 12usize + name.len())
                .sum::<usize>();
        let mut container = b"UnityWebData1.0\0".to_vec();
        container.extend_from_slice(&i32::try_from(header_size).unwrap().to_le_bytes());
        let mut data_offset = header_size;
        for (name, bytes) in entries {
            assert!(name.len() < 1024 * 1024);
            container.extend_from_slice(&i32::try_from(data_offset).unwrap().to_le_bytes());
            container.extend_from_slice(&i32::try_from(bytes.len()).unwrap().to_le_bytes());
            container.extend_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
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
    fn source_collection_file_order_matches_root_and_nested_bundle_walk() {
        let mut filesystem = VirtualFileSystem::new();
        for index in 0..6 {
            let path = format!("/game/file_{index}.assets");
            insert_file(
                &mut filesystem,
                &path,
                &make_generation_10_file_with(&[], 0, index + 1, index + 1),
            );
        }
        let inputs = (0..6)
            .map(|index| input(&format!("/game/file_{index}.assets")))
            .collect::<Vec<_>>();
        let mut workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();

        for (file_index, bundle_index) in [(1, 0), (2, 1), (3, 2), (4, 3)] {
            workspace.files[file_index].container_bundle_index = Some(bundle_index);
            workspace.files[file_index].container_collection_index = Some(0);
        }
        workspace.container_bundles = vec![
            WorkspaceSerializedBundle {
                scope_path: None,
                name_fixed: None,
                parent_bundle_index: None,
                parent_entry_index: None,
                direct_collection_file_indices: vec![1],
                direct_resource_file_indices: Vec::new(),
            },
            WorkspaceSerializedBundle {
                scope_path: None,
                name_fixed: None,
                parent_bundle_index: Some(0),
                parent_entry_index: Some(9),
                direct_collection_file_indices: vec![2],
                direct_resource_file_indices: Vec::new(),
            },
            WorkspaceSerializedBundle {
                scope_path: None,
                name_fixed: None,
                parent_bundle_index: Some(0),
                parent_entry_index: Some(2),
                direct_collection_file_indices: vec![3],
                direct_resource_file_indices: Vec::new(),
            },
            WorkspaceSerializedBundle {
                scope_path: None,
                name_fixed: None,
                parent_bundle_index: None,
                parent_entry_index: None,
                direct_collection_file_indices: vec![4],
                direct_resource_file_indices: Vec::new(),
            },
        ];

        assert_eq!(
            workspace.source_collection_file_indices().unwrap(),
            [5, 0, 4, 1, 3, 2]
        );
    }

    #[test]
    fn decoded_object_workspace_retains_identity_values_and_raw_fallback_ranges() {
        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/main.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let decoded = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();

        assert!(decoded.report().is_complete());
        assert_eq!(decoded.report().object_count, 1);
        let object = &decoded.objects()[0];
        let source_object = &workspace.files()[0].metadata.objects[0];
        assert_eq!(object.file_index, 0);
        assert_eq!(object.object_index, 0);
        assert_eq!(object.path_id, 42);
        assert_eq!(
            object.serialized_type_index,
            source_object.serialized_type_index
        );
        assert_eq!(object.raw_type_id, source_object.raw_type_id);
        assert_eq!(
            object.resolved_type_index,
            source_object.resolved_type_index
        );
        assert_eq!(object.class_id, Some(1));
        assert_eq!(object.script_type_index, source_object.script_type_index);
        assert_eq!(object.is_destroyed, source_object.is_destroyed);
        assert_eq!(object.is_stripped, source_object.is_stripped);
        let Ok(payload) = &object.payload else {
            panic!("fixture object should decode: {:?}", object.payload);
        };
        assert_eq!(payload.path_id, 42);
        assert_eq!(
            payload.root,
            SerializedValue::Struct(vec![
                crate::serialized_file::object::SerializedFieldValue {
                    name: "m_Value".to_owned(),
                    value: SerializedValue::I32(0x1234_5678),
                },
                crate::serialized_file::object::SerializedFieldValue {
                    name: "m_Target".to_owned(),
                    value: SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 42,
                    },
                },
            ])
        );

        let raw_limited = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits {
                max_total_decoded_allocation_bytes: 0,
                ..ObjectWorkspaceLimits::default()
            },
        )
        .unwrap();
        assert!(!raw_limited.report().is_complete());
        assert_eq!(raw_limited.report().decoded_objects, 0);
        assert_eq!(raw_limited.report().undecoded_objects, 1);
        assert_eq!(raw_limited.report().total_undecoded_raw_bytes, 12);
        assert!(matches!(
            &raw_limited.objects()[0].payload,
            Err(error)
                if matches!(error.kind(),
                    crate::serialized_file::file::SerializedFileErrorKind::Object(
                        object_error
                    )
                    if matches!(
                        object_error.kind(),
                        crate::serialized_file::object::ObjectDecodeErrorKind::DecodeLimitExceeded {
                            limit: 0,
                            ..
                        }
                    )
                )
        ));
        let start = usize::try_from(raw_limited.objects()[0].raw_payload_range.start).unwrap();
        let end = usize::try_from(raw_limited.objects()[0].raw_payload_range.end).unwrap();
        assert_eq!(&bytes[start..end], &bytes[bytes.len() - 12..]);
    }

    #[test]
    fn object_workspace_recovers_source_generated_textasset_without_usable_file_typetree() {
        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|textasset_notree|"))
            .expect("the source-factory fixture includes the no-TypeTree TextAsset");
        assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                u8::from_str_radix(digits, 16).expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let parsed = SerializedFile::parse(&bytes).expect("source fixture parses");
        assert_eq!(
            parsed.metadata.prefix.unity_version_text.as_deref(),
            Some("2019.4.40f1")
        );
        assert_eq!(parsed.metadata.prefix.target_platform, Some(19));
        let serialized_type_index = parsed.metadata.objects[0]
            .resolved_type_index
            .expect("TextAsset resolves its serialized type");
        assert!(
            parsed.metadata.types[serialized_type_index]
                .type_tree
                .as_ref()
                .is_none_or(|tree| tree.nodes.is_empty()),
            "the object's serialized type must have no usable TypeTree"
        );
        assert!(matches!(
            parsed.decode_object_payload(0).unwrap_err().kind(),
            crate::serialized_file::file::SerializedFileErrorKind::Object(object_error)
                if matches!(
                    object_error.kind(),
                    crate::serialized_file::object::ObjectDecodeErrorKind::MissingTypeTree { .. }
                        | crate::serialized_file::object::ObjectDecodeErrorKind::EmptyTypeTree { .. }
                )
        ));

        let filesystem = virtual_file_system("/game/textasset_notree.assets", &bytes);
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/textasset_notree.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("workspace loads the reference fixture");
        assert!(workspace.files()[0].reference_graph.is_complete());
        assert!(workspace.files()[0]
            .reference_graph
            .object_scan_is_complete(0));
        assert!(workspace.files()[0].reference_graph.references().is_empty());
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .expect("the factory fallback materializes the generated class");

        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.report().decoded_objects, 1);
        let payload = &objects.objects()[0]
            .payload
            .as_ref()
            .expect("decoded payload");
        assert_eq!(payload.path_id, 42);
        assert_eq!(payload.type_name, "TextAsset");
        assert_eq!(
            payload
                .root
                .unique_field_in_base_chain("m_Name")
                .expect("unique name field")
                .expect("name field exists"),
            &SerializedValue::StringBytes(b"Hero".to_vec())
        );
        assert_eq!(
            payload
                .root
                .unique_field_in_base_chain("m_Script")
                .expect("unique script field")
                .expect("script field exists"),
            &SerializedValue::StringBytes(b"print(1)".to_vec())
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/game/textasset_notree.assets", usize::MAX)
                .unwrap(),
            bytes,
            "recovery must leave source bytes unchanged"
        );
    }

    #[test]
    fn object_workspace_recovers_source_generated_monoscript_without_usable_file_typetree() {
        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|monoscript_notree|"))
            .expect("the source-factory fixture includes the no-usable-TypeTree MonoScript");
        assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                u8::from_str_radix(digits, 16).expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let parsed = SerializedFile::parse(&bytes).expect("source fixture parses");
        assert_eq!(
            parsed.metadata.prefix.unity_version_text.as_deref(),
            Some("2019.4.40f1")
        );
        assert_eq!(parsed.metadata.prefix.target_platform, Some(19));
        let serialized_type_index = parsed.metadata.objects[0]
            .resolved_type_index
            .expect("MonoScript resolves its serialized type");
        assert!(
            parsed.metadata.types[serialized_type_index]
                .type_tree
                .as_ref()
                .is_none_or(|tree| tree.nodes.is_empty()),
            "the object's serialized type must have no usable TypeTree"
        );

        let filesystem = virtual_file_system("/game/monoscript_notree.assets", &bytes);
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/monoscript_notree.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("workspace loads the reference fixture");
        assert!(workspace.files()[0].reference_graph.is_complete());
        assert!(workspace.files()[0]
            .reference_graph
            .object_scan_is_complete(0));
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .expect("the factory fallback materializes the generated class");

        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.report().decoded_objects, 1);
        let payload = &objects.objects()[0]
            .payload
            .as_ref()
            .expect("decoded payload");
        assert_eq!(payload.path_id, 42);
        assert_eq!(payload.type_name, "MonoScript");
        for (field_name, expected) in [
            ("m_Name", &b"HeroController"[..]),
            ("m_ClassName", &b"HeroController"[..]),
            ("m_Namespace", &b"VRC.SDK"[..]),
            ("m_AssemblyName", &b"Assembly-CSharp"[..]),
        ] {
            assert_eq!(
                payload
                    .root
                    .unique_field_in_base_chain(field_name)
                    .expect("unique field")
                    .expect("field exists"),
                &SerializedValue::StringBytes(expected.to_vec()),
                "{field_name}"
            );
        }
        assert_eq!(
            payload
                .root
                .unique_field_in_base_chain("m_ExecutionOrder")
                .expect("unique execution order")
                .expect("execution order exists"),
            &SerializedValue::I32(200)
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/game/monoscript_notree.assets", usize::MAX)
                .unwrap(),
            bytes,
            "recovery must leave source bytes unchanged"
        );
    }

    #[test]
    fn object_workspace_keeps_unknown_no_tree_objects_unresolved() {
        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|unknown_class|"))
            .expect("the source-factory fixture includes an unknown class");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                u8::from_str_radix(digits, 16).expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let filesystem = virtual_file_system("/game/unknown_class.assets", &bytes);
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/unknown_class.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("workspace retains an unknown serialized class");

        assert!(!workspace.files()[0].reference_graph.is_complete());
        assert!(!workspace.files()[0]
            .reference_graph
            .object_scan_is_complete(0));
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .expect("unknown objects remain raw-retained rather than aborting the workspace");
        assert!(!objects.report().is_complete());
        assert_eq!(objects.report().decoded_objects, 0);
        assert_eq!(objects.report().undecoded_objects, 1);
        assert!(matches!(
            objects.objects()[0].payload.as_ref().unwrap_err().kind(),
            crate::serialized_file::file::SerializedFileErrorKind::Object(object_error)
                if matches!(
                    object_error.kind(),
                    crate::serialized_file::object::ObjectDecodeErrorKind::MissingTypeTree { .. }
                        | crate::serialized_file::object::ObjectDecodeErrorKind::EmptyTypeTree { .. }
                )
        ));
    }

    #[test]
    fn decoded_object_workspace_enforces_object_count_limit() {
        let filesystem = virtual_file_system("/game/main.assets", &make_generation_10_file());
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/main.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let error = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits {
                max_objects: 0,
                ..ObjectWorkspaceLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            ObjectWorkspaceErrorKind::ObjectCountLimitExceeded { count: 1, limit: 0 }
        ));
    }

    #[test]
    fn workspace_collection_models_follow_source_order_and_link_object_ranges() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/A.assets",
            &make_generation_10_file(),
        );
        insert_file(
            &mut filesystem,
            "/game/Z.assets",
            &make_generation_10_file(),
        );
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/A.assets"), input("/game/Z.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let collections = WorkspaceAssetCollections::build(
            &workspace,
            &objects,
            WorkspaceCollectionLimits::default(),
        )
        .unwrap();

        assert_eq!(collections.report().collection_count, 2);
        assert_eq!(collections.report().object_count, 2);
        assert_eq!(collections.collections()[0].source_file_index, 1);
        assert_eq!(collections.collections()[0].fixed_name, "z.assets");
        assert_eq!(collections.collections()[0].object_range, 1..2);
        assert_eq!(collections.collections()[1].source_file_index, 0);
        assert_eq!(collections.collections()[1].fixed_name, "a.assets");
        assert_eq!(collections.collections()[1].object_range, 0..1);
        assert_eq!(collections.collection_index_for_file(1), Some(0));
        assert_eq!(collections.collection_index_for_file(0), Some(1));
        assert_eq!(collections.collection_index_for_file(2), None);

        let limited = WorkspaceAssetCollections::build(
            &workspace,
            &objects,
            WorkspaceCollectionLimits {
                max_collections: 1,
                ..WorkspaceCollectionLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            limited.kind,
            WorkspaceCollectionErrorKind::CollectionCountLimitExceeded { count: 2, limit: 1 }
        ));

        let object_limited = WorkspaceAssetCollections::build(
            &workspace,
            &objects,
            WorkspaceCollectionLimits {
                max_objects: 1,
                ..WorkspaceCollectionLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            object_limited.kind,
            WorkspaceCollectionErrorKind::ObjectCountLimitExceeded { count: 2, limit: 1 }
        ));

        let names_limited = WorkspaceAssetCollections::build(
            &workspace,
            &objects,
            WorkspaceCollectionLimits {
                max_total_collection_name_bytes: 1,
                ..WorkspaceCollectionLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            names_limited.kind,
            WorkspaceCollectionErrorKind::CollectionNameByteLimitExceeded { limit: 1, .. }
        ));
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
        assert_eq!(workspace.files()[0].container_entry_index, None);
        assert_eq!(workspace.files()[0].container_collection_index, None);
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
    fn workspace_reference_graph_uses_root_collection_precedence_over_child_bundles() {
        let make_loaded_file =
            |file_name: &str,
             virtual_path: &str,
             bytes: Vec<u8>,
             container_scope: Option<&str>,
             container_entry_index: Option<usize>,
             container_collection_index: Option<usize>,
             container_bundle_index: Option<usize>| {
                let parsed = SerializedFile::parse(&bytes).unwrap();
                let reference_graph = ObjectReferenceGraph::build(&parsed).unwrap();
                super::LoadedSerializedFile {
                    file_name: file_name.to_owned(),
                    virtual_path: virtual_path.to_owned(),
                    container_scope: container_scope.map(str::to_owned),
                    container_entry_index,
                    container_collection_index,
                    container_bundle_index,
                    header: parsed.header,
                    metadata: parsed.metadata,
                    reference_graph,
                }
            };
        let files = vec![
            make_loaded_file(
                "main.assets",
                "/game/main.assets",
                make_generation_10_file_with(&["shared.assets"], 1, 42, 42),
                None,
                None,
                None,
                None,
            ),
            make_loaded_file(
                "shared.assets",
                "/game/container/shared.assets",
                make_generation_10_file_with(&["shared.assets"], 1, 42, 42),
                Some("/game/container"),
                Some(0),
                Some(0),
                Some(0),
            ),
            make_loaded_file(
                "shared.assets",
                "/game/shared.assets",
                make_generation_10_file(),
                None,
                None,
                None,
                None,
            ),
        ];
        let workspace = SerializedFileWorkspace {
            files,
            container_bundles: vec![WorkspaceSerializedBundle {
                scope_path: Some("/game/container".to_owned()),
                name_fixed: None,
                parent_bundle_index: None,
                parent_entry_index: None,
                direct_collection_file_indices: vec![1],
                direct_resource_file_indices: Vec::new(),
            }],
            resource_files: Vec::new(),
            failed_container_entries: Vec::new(),
            failed_container_metadata_bytes: 0,
            report: SerializedFileWorkspaceReport::default(),
        };

        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(graph.references().len(), 3);
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 2,
                target_object_index: 0,
            }
        );
        assert_eq!(
            graph.references()[1].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn workspace_reference_graph_keeps_first_root_bundle_collection_match() {
        let make_loaded_file =
            |file_name: &str,
             virtual_path: &str,
             bytes: Vec<u8>,
             container_scope: Option<&str>,
             container_bundle_index: Option<usize>| {
                let parsed = SerializedFile::parse(&bytes).unwrap();
                let reference_graph = ObjectReferenceGraph::build(&parsed).unwrap();
                super::LoadedSerializedFile {
                    file_name: file_name.to_owned(),
                    virtual_path: virtual_path.to_owned(),
                    container_scope: container_scope.map(str::to_owned),
                    container_entry_index: container_bundle_index.map(|_| 0),
                    container_collection_index: container_bundle_index.map(|_| 0),
                    container_bundle_index,
                    header: parsed.header,
                    metadata: parsed.metadata,
                    reference_graph,
                }
            };
        let workspace = SerializedFileWorkspace {
            files: vec![
                make_loaded_file(
                    "main.assets",
                    "/game/main.assets",
                    make_generation_10_file_with(&["shared.assets"], 1, 42, 42),
                    None,
                    None,
                ),
                make_loaded_file(
                    "shared.assets",
                    "/game/first/shared.assets",
                    make_generation_10_file(),
                    Some("/game/first"),
                    Some(0),
                ),
                make_loaded_file(
                    "shared.assets",
                    "/game/second/shared.assets",
                    make_generation_10_file(),
                    Some("/game/second"),
                    Some(1),
                ),
            ],
            container_bundles: vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/game/first".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: vec![1],
                    direct_resource_file_indices: Vec::new(),
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/game/second".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: vec![2],
                    direct_resource_file_indices: Vec::new(),
                },
            ],
            resource_files: Vec::new(),
            failed_container_entries: Vec::new(),
            failed_container_metadata_bytes: 0,
            report: SerializedFileWorkspaceReport::default(),
        };

        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
    }

    #[test]
    fn workspace_reference_graph_orders_nested_bundle_matches_by_source_entry() {
        let make_loaded_file = |file_name: &str,
                                virtual_path: &str,
                                bytes: Vec<u8>,
                                container_scope: &str,
                                container_entry_index: usize,
                                container_collection_index: usize,
                                container_bundle_index: usize| {
            let parsed = SerializedFile::parse(&bytes).unwrap();
            let reference_graph = ObjectReferenceGraph::build(&parsed).unwrap();
            super::LoadedSerializedFile {
                file_name: file_name.to_owned(),
                virtual_path: virtual_path.to_owned(),
                container_scope: Some(container_scope.to_owned()),
                container_entry_index: Some(container_entry_index),
                container_collection_index: Some(container_collection_index),
                container_bundle_index: Some(container_bundle_index),
                header: parsed.header,
                metadata: parsed.metadata,
                reference_graph,
            }
        };
        let workspace = SerializedFileWorkspace {
            files: vec![
                make_loaded_file(
                    "main.assets",
                    "/game/root/main.assets",
                    make_generation_10_file_with(&["shared.assets"], 1, 42, 42),
                    "/game/root",
                    0,
                    0,
                    0,
                ),
                make_loaded_file(
                    "shared.assets",
                    "/game/root/entry-five/shared.assets",
                    make_generation_10_file(),
                    "/game/root/entry-five",
                    0,
                    0,
                    1,
                ),
                make_loaded_file(
                    "shared.assets",
                    "/game/root/entry-two/shared.assets",
                    make_generation_10_file(),
                    "/game/root/entry-two",
                    0,
                    0,
                    2,
                ),
            ],
            container_bundles: vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/game/root".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: vec![0],
                    direct_resource_file_indices: Vec::new(),
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/game/root/entry-five".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(5),
                    direct_collection_file_indices: vec![1],
                    direct_resource_file_indices: Vec::new(),
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/game/root/entry-two".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(2),
                    direct_collection_file_indices: vec![2],
                    direct_resource_file_indices: Vec::new(),
                },
            ],
            resource_files: Vec::new(),
            failed_container_entries: Vec::new(),
            failed_container_metadata_bytes: 0,
            report: SerializedFileWorkspaceReport::default(),
        };

        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 2,
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
        let nested_webdata = make_webdata_fixture(&[
            ("inner.assets", &nested_leaf),
            ("inner.resS", b"nested stream data"),
        ]);
        let webdata = make_webdata_fixture(&[
            ("z-first.assets", &serialized_leaf),
            ("shared.assets", &serialized_leaf),
            ("nested.bundle", &nested_webdata),
            ("a-last.assets", &serialized_leaf),
            ("z-resource.resS", b"first root stream"),
            ("a-resource.resource", b"second root stream"),
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

        assert_eq!(loaded.workspace.files().len(), 5);
        assert_eq!(loaded.initial_container_expansions.len(), 1);
        assert_eq!(
            loaded.initial_container_expansions[0].kind,
            FileKind::UnityWebData
        );
        assert_eq!(
            loaded.initial_container_expansions[0].serialized_files_found,
            4
        );
        let first_direct_file = loaded
            .workspace
            .files()
            .iter()
            .find(|file| file.virtual_path.ends_with("/z-first.assets"))
            .unwrap();
        assert_eq!(first_direct_file.container_entry_index, Some(0));
        assert_eq!(first_direct_file.container_collection_index, Some(0));
        assert_eq!(first_direct_file.container_bundle_index, Some(0));
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
        assert_eq!(direct_file.container_entry_index, Some(1));
        assert_eq!(direct_file.container_collection_index, Some(1));
        assert_eq!(direct_file.container_bundle_index, Some(0));
        let last_direct_file = loaded
            .workspace
            .files()
            .iter()
            .find(|file| file.virtual_path.ends_with("/a-last.assets"))
            .unwrap();
        assert_eq!(last_direct_file.container_entry_index, Some(3));
        assert_eq!(last_direct_file.container_collection_index, Some(2));
        assert_eq!(last_direct_file.container_bundle_index, Some(0));
        let direct_collection_indices = loaded
            .workspace
            .ordered_collection_file_indices("/.assetripper/import/initial/0")
            .unwrap();
        let direct_collection_names: Vec<_> = direct_collection_indices
            .iter()
            .map(|index| loaded.workspace.files()[*index].file_name.as_str())
            .collect();
        assert_eq!(
            direct_collection_names,
            ["z-first.assets", "shared.assets", "a-last.assets"]
        );
        let bundle_nodes = loaded.workspace.container_bundles();
        assert_eq!(bundle_nodes.len(), 2);
        assert_eq!(
            bundle_nodes[0].scope_path.as_deref(),
            Some("/.assetripper/import/initial/0")
        );
        assert_eq!(bundle_nodes[0].parent_bundle_index, None);
        let root_bundle_collection_names: Vec<_> = bundle_nodes[0]
            .direct_collection_file_indices
            .iter()
            .map(|index| loaded.workspace.files()[*index].file_name.as_str())
            .collect();
        assert_eq!(
            root_bundle_collection_names,
            ["z-first.assets", "shared.assets", "a-last.assets"]
        );
        let nested_file_index = loaded
            .workspace
            .files()
            .iter()
            .position(|file| file.virtual_path.ends_with("/nested.bundle/inner.assets"))
            .unwrap();
        assert_eq!(
            bundle_nodes[1].scope_path.as_deref(),
            Some("/.assetripper/import/initial/0/nested.bundle")
        );
        assert_eq!(bundle_nodes[1].parent_bundle_index, Some(0));
        assert_eq!(bundle_nodes[1].parent_entry_index, Some(2));
        assert_eq!(
            bundle_nodes[1].direct_collection_file_indices,
            [nested_file_index]
        );
        assert_eq!(bundle_nodes[0].direct_resource_file_indices, [0, 1]);
        assert_eq!(bundle_nodes[1].direct_resource_file_indices, [2]);
        let workspace_resources = loaded.workspace.resource_files();
        assert_eq!(workspace_resources.len(), 3);
        assert_eq!(
            workspace_resources
                .iter()
                .map(|resource| resource.virtual_path.as_str())
                .collect::<Vec<_>>(),
            [
                "/.assetripper/import/initial/0/z-resource.resS",
                "/.assetripper/import/initial/0/a-resource.resource",
                "/.assetripper/import/initial/0/nested.bundle/inner.resS",
            ]
        );
        assert_eq!(
            workspace_resources
                .iter()
                .map(|resource| resource.name.as_str())
                .collect::<Vec<_>>(),
            ["z-resource.resS", "a-resource.resource", "inner.resS"]
        );
        assert_eq!(workspace_resources[0].container_bundle_index, 0);
        assert_eq!(workspace_resources[0].container_entry_index, 4);
        assert_eq!(workspace_resources[0].container_resource_index, 0);
        assert_eq!(workspace_resources[2].container_bundle_index, 1);
        assert_eq!(workspace_resources[2].container_entry_index, 1);
        assert_eq!(workspace_resources[2].container_resource_index, 0);
        let mut invalid_workspace = loaded.workspace.clone();
        let a_last_index = invalid_workspace
            .files
            .iter()
            .position(|file| file.file_name == "a-last.assets")
            .unwrap();
        invalid_workspace.files[a_last_index].container_collection_index = Some(4);
        let error = invalid_workspace
            .ordered_collection_file_indices("/.assetripper/import/initial/0")
            .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::InvalidContainerCollectionSequence {
                expected: 2,
                actual: 4
            }
        ));
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
        assert_eq!(nested_file.container_entry_index, Some(0));
        assert_eq!(nested_file.container_collection_index, Some(0));
        assert_eq!(nested_file.container_bundle_index, Some(1));
        assert!(matches!(
            loaded.dependencies.requests[0].outcome,
            SerializedFileDependencyOutcome::AlreadyLoaded { .. }
        ));
        assert_eq!(
            loaded.dependencies.requests[1].outcome,
            SerializedFileDependencyOutcome::ContainerAlreadyExpanded {
                virtual_path: "/game/Game_Data/data.unity3d".to_owned(),
                serialized_files_loaded: 4,
                matching_serialized_files: 0,
            }
        );
    }

    #[test]
    fn workspace_retains_source_failed_entries_with_their_container_owner() {
        use crate::scheme::{ContainerMountLimits, SchemeReader};

        let row = include_str!("../io/io_differential_fixture.txt")
            .lines()
            .find(|line| line.starts_with("SCHEME|ufs_v6_lzham_block|"))
            .expect("reference fixture contains the failed-entry continuation case");
        let input_hex = row
            .split('|')
            .find_map(|field| field.strip_prefix("in="))
            .expect("fixture row contains input bytes");
        let bytes = input_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let dispatched =
            SchemeReader::read_with_limits(&bytes, crate::scheme::SchemeReaderLimits::default())
                .unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let mount = dispatched
            .mount_container_tree_with_source_name(
                &mut filesystem,
                Some("/workspace-ufs"),
                Some("sample.bundle"),
                ContainerMountLimits::default(),
            )
            .unwrap();
        let mut workspace = SerializedFileWorkspace::default();
        append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &mount.container_scopes,
                mounted_bundles: &mount.container_bundles,
                serialized_files: &mount.serialized_files,
                resource_files: &mount.resource_files,
                failed_entries: &mount.failed_entries,
                source_container_path: "/source/sample.bundle",
                loaded_paths: &std::collections::HashMap::new(),
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 4,
            },
            &mut 0,
            &mut 0,
        )
        .unwrap();

        let failures = workspace.failed_container_entries();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].source_container_path, "/source/sample.bundle");
        assert_eq!(failures[0].entry.source_name, "unsupported.bin");
        assert_eq!(failures[0].entry.container_bundle_index, Some(0));
        assert_eq!(failures[0].entry.entry_index, 1);
        assert_eq!(
            filesystem
                .read_all_bytes("/workspace-ufs/after.bin", 16)
                .unwrap(),
            b"LATER"
        );
    }

    #[test]
    fn workspace_container_bundle_path_and_count_limits_are_explicit() {
        let scopes = ["/root".to_owned()];
        let bundles = [MountedContainerBundle {
            scope_index: Some(0),
            parent_bundle_index: None,
            parent_entry_index: None,
            name_fixed: None,
        }];
        let mounted_files = [];
        let mounted_resources = [MountedResourceFile {
            virtual_path: "/root/stream.resS".to_owned(),
            name: "stream.resS".to_owned(),
            container_scope_index: Some(0),
            container_bundle_index: 0,
            entry_index: 0,
            resource_index: 0,
        }];
        let loaded_paths = std::collections::HashMap::new();
        let mut workspace = SerializedFileWorkspace::default();
        let mut total_path_bytes = 0usize;
        let mut total_resource_path_bytes = 0usize;
        let path_limited = SerializedFileWorkspaceLimits {
            max_total_bundle_node_path_bytes: 4,
            ..SerializedFileWorkspaceLimits::default()
        };
        let error = append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &mounted_files,
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &loaded_paths,
                limits: path_limited,
                bundle_count_limit: 1,
            },
            &mut total_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalBundleNodePathBytesLimitExceeded {
                byte_count: 5,
                limit: 4
            }
        ));
        assert!(workspace.container_bundles().is_empty());

        let resource_path_limited = SerializedFileWorkspaceLimits {
            max_total_bundle_resource_file_path_bytes: 4,
            ..SerializedFileWorkspaceLimits::default()
        };
        let error = append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &mounted_files,
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &loaded_paths,
                limits: resource_path_limited,
                bundle_count_limit: 1,
            },
            &mut total_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalBundleResourceFilePathBytesLimitExceeded {
                byte_count: 17,
                limit: 4
            }
        ));
        assert!(workspace.container_bundles().is_empty());
        assert!(workspace.resource_files().is_empty());

        let gapped_resources = [
            MountedResourceFile {
                resource_index: 0,
                ..mounted_resources[0].clone()
            },
            MountedResourceFile {
                virtual_path: "/root/second.resS".to_owned(),
                entry_index: 1,
                resource_index: 2,
                ..mounted_resources[0].clone()
            },
        ];
        let error = append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &mounted_files,
                resource_files: &gapped_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &loaded_paths,
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 1,
            },
            &mut total_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::InvalidContainerResourceSequence {
                expected: 1,
                actual: 2
            }
        ));
        assert!(workspace.container_bundles().is_empty());
        assert!(workspace.resource_files().is_empty());

        let error = append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &mounted_files,
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &loaded_paths,
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 0,
            },
            &mut total_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::ContainerBundleCountLimitExceeded {
                count: 1,
                limit: 0
            }
        ));
        assert!(workspace.container_bundles().is_empty());
    }

    #[test]
    fn workspace_retains_fixed_bundle_names_and_bounds_their_aggregate_bytes() {
        let scopes = ["/bundle".to_owned()];
        let bundles = [MountedContainerBundle {
            scope_index: Some(0),
            parent_bundle_index: None,
            parent_entry_index: None,
            name_fixed: Some("assets.bundle".to_owned()),
        }];
        let mut workspace = SerializedFileWorkspace::default();
        append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &[],
                resource_files: &[],
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &std::collections::HashMap::new(),
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 1,
            },
            &mut 0,
            &mut 0,
        )
        .unwrap();
        assert_eq!(
            workspace.container_bundles()[0].name_fixed.as_deref(),
            Some("assets.bundle")
        );
        assert_eq!(workspace.report().bundle_name_bytes, 13);

        let mut limited_workspace = SerializedFileWorkspace::default();
        let error = append_mounted_container_bundles(
            &mut limited_workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &[],
                resource_files: &[],
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &std::collections::HashMap::new(),
                limits: SerializedFileWorkspaceLimits {
                    max_total_bundle_name_bytes: 4,
                    ..SerializedFileWorkspaceLimits::default()
                },
                bundle_count_limit: 1,
            },
            &mut 0,
            &mut 0,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalBundleNameBytesLimitExceeded {
                byte_count: 13,
                limit: 4
            }
        ));
        assert!(limited_workspace.container_bundles().is_empty());
        assert_eq!(limited_workspace.report().bundle_name_bytes, 0);
    }

    #[test]
    fn workspace_resource_source_names_obey_their_aggregate_bound() {
        let scopes = ["/root".to_owned()];
        let bundles = [MountedContainerBundle {
            scope_index: Some(0),
            parent_bundle_index: None,
            parent_entry_index: None,
            name_fixed: None,
        }];
        let mounted_resources = [MountedResourceFile {
            virtual_path: "/root/stream.resS".to_owned(),
            name: "stream.resS".to_owned(),
            container_scope_index: Some(0),
            container_bundle_index: 0,
            entry_index: 0,
            resource_index: 0,
        }];
        let mut workspace = SerializedFileWorkspace::default();
        let error = append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &[],
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &std::collections::HashMap::new(),
                limits: SerializedFileWorkspaceLimits {
                    max_total_bundle_resource_file_name_bytes: 4,
                    ..SerializedFileWorkspaceLimits::default()
                },
                bundle_count_limit: 1,
            },
            &mut 0,
            &mut 0,
        )
        .unwrap_err();

        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalBundleResourceFileNameBytesLimitExceeded {
                byte_count: 11,
                limit: 4
            }
        ));
        assert_eq!(error.report.bundle_resource_file_name_bytes, 0);
    }

    #[test]
    fn pathless_container_root_keeps_its_direct_collection_membership() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/first.asset", &make_generation_10_file());
        insert_file(&mut filesystem, "/resource.resS", b"stream data");
        let inputs = [input("/first.asset")];
        let mut workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let scopes = [];
        let bundles = [MountedContainerBundle {
            scope_index: None,
            parent_bundle_index: None,
            parent_entry_index: None,
            name_fixed: None,
        }];
        let mounted_files = [crate::scheme::MountedSerializedFile {
            virtual_path: "/first.asset".to_owned(),
            container_scope_index: None,
            container_bundle_index: Some(0),
            entry_index: 0,
            collection_index: Some(0),
        }];
        let mounted_resources = [MountedResourceFile {
            virtual_path: "/resource.resS".to_owned(),
            name: "resource.resS".to_owned(),
            container_scope_index: None,
            container_bundle_index: 0,
            entry_index: 1,
            resource_index: 0,
        }];
        let mut loaded_paths = std::collections::HashMap::new();
        loaded_paths.insert("/first.asset".to_owned(), 0);
        let mut total_node_path_bytes = 0;
        let mut total_resource_path_bytes = 0;
        append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &mounted_files,
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &loaded_paths,
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 1,
            },
            &mut total_node_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap();
        let order = super::build_mounted_file_order_index(
            std::iter::once((scopes.as_slice(), mounted_files.as_slice())),
            1,
            workspace.report,
        )
        .unwrap();
        super::attach_container_file_order(&mut workspace.files[0], &order, workspace.report)
            .unwrap();

        assert_eq!(workspace.container_bundles().len(), 1);
        assert_eq!(workspace.container_bundles()[0].scope_path, None);
        assert_eq!(
            workspace.container_bundles()[0].direct_collection_file_indices,
            [0]
        );
        assert_eq!(
            workspace.container_bundles()[0].direct_resource_file_indices,
            [0]
        );
        assert_eq!(workspace.files()[0].container_bundle_index, Some(0));
        assert_eq!(workspace.files()[0].container_collection_index, Some(0));
        assert_eq!(workspace.resource_files().len(), 1);
        assert_eq!(workspace.resource_files()[0].virtual_path, "/resource.resS");
        assert_eq!(workspace.resource_files()[0].container_bundle_index, 0);
    }

    /// Mounted resources cache their fixed name at registration (like `FileBase.NameFixed`),
    /// so hierarchy lookups compare the cached form instead of renormalizing per lookup.
    #[test]
    fn mounted_resources_cache_their_fixed_names_for_lookups() {
        let scopes = [];
        let bundles = [MountedContainerBundle {
            scope_index: None,
            parent_bundle_index: None,
            parent_entry_index: None,
            name_fixed: None,
        }];
        let mounted_resources = [MountedResourceFile {
            virtual_path: "/archive:/textures/Shared.resS".to_owned(),
            name: "archive:/textures/Shared.resS".to_owned(),
            container_scope_index: None,
            container_bundle_index: 0,
            entry_index: 0,
            resource_index: 0,
        }];
        let mut workspace = SerializedFileWorkspace::default();
        let mut total_node_path_bytes = 0;
        let mut total_resource_path_bytes = 0;
        super::append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &[],
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &std::collections::HashMap::new(),
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 1,
            },
            &mut total_node_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap();

        assert_eq!(workspace.resource_files().len(), 1);
        assert_eq!(
            workspace.resource_files()[0].name_fixed,
            "shared.ress",
            "the fixed name is cached at registration, not per lookup"
        );
        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(0), "ARCHIVE:/request/SHARED.ress")
                .unwrap(),
            Some(0),
            "lookups match through the cached fixed name"
        );
        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(0), "archive:/missing/nope.resS")
                .unwrap(),
            None
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
    fn dependency_container_resource_leaf_attaches_to_its_workspace_bundle() {
        let mut filesystem = VirtualFileSystem::new();
        let serialized_leaf = make_generation_10_file();
        let container = make_webdata_fixture(&[
            ("shared.assets", &serialized_leaf),
            ("stream.resS", b"dependency stream"),
        ]);
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

        assert!(dependency_report.is_complete());
        assert_eq!(workspace.files().len(), 2);
        assert_eq!(workspace.container_bundles().len(), 1);
        assert_eq!(
            workspace.container_bundles()[0].direct_collection_file_indices,
            [1]
        );
        assert_eq!(
            workspace.container_bundles()[0].direct_resource_file_indices,
            [0]
        );
        assert_eq!(
            workspace.resource_files(),
            [super::WorkspaceResourceFile {
                virtual_path: "/__assetripper_dependency_mount_0/stream.resS".to_owned(),
                name: "stream.resS".to_owned(),
                name_fixed: "stream.ress".to_owned(),
                container_bundle_index: 0,
                container_entry_index: 1,
                container_resource_index: 0,
            }]
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/__assetripper_dependency_mount_0/stream.resS", 64)
                .unwrap(),
            b"dependency stream"
        );
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
        assert_eq!(workspace.files()[1].container_entry_index, Some(0));
        assert_eq!(workspace.files()[1].container_collection_index, Some(0));
        assert_eq!(workspace.files()[1].container_bundle_index, Some(0));
        assert_eq!(
            workspace
                .ordered_collection_file_indices("/__assetripper_dependency_mount_0")
                .unwrap(),
            [1]
        );
        assert_eq!(workspace.container_bundles().len(), 1);
        assert_eq!(
            workspace.container_bundles()[0].scope_path.as_deref(),
            Some("/__assetripper_dependency_mount_0")
        );
        assert_eq!(
            workspace.container_bundles()[0].direct_collection_file_indices,
            [1]
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
