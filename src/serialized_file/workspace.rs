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

/// Options for resolving a SerializedFile workspace and its external dependencies.
pub struct SerializedFileWorkspaceLoadOptions<'a> {
    /// Ordered Unity `DataPaths` used for external-file lookup.
    pub data_paths: &'a [String],
    /// Bounds applied while loading files and walking dependencies.
    pub limits: SerializedFileWorkspaceLimits,
    /// Bounds applied while indexing dependency candidates.
    pub index_limits: DependencyIndexLimits,
    /// Bounds applied while mounting dependency containers.
    pub container_limits: ContainerMountLimits,
    /// Initial containers already expanded into the VFS, with their source entry order.
    pub preexpanded_containers: &'a [PreExpandedContainer],
    /// Generated-class and TPK layout provider for object materialization.
    pub layouts: &'a dyn crate::processing::asset_factory::ClassLayoutSource,
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
pub struct PreExpandedContainer {
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

#[path = "workspace/container_index.rs"]
mod container_index;
use container_index::*;
#[path = "workspace/loader.rs"]
mod loader;

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
