use std::sync::Arc;

use crate::container::raw_web::RawWebLimits;
use crate::container::unityfs::{UnityFsError, UnityFsLimits};
use crate::container::web_file::WebFileLimits;

/// The file types recognized by the local AssetRipper `SchemeReader`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    UnityFs,
    UnityRaw,
    UnityWebBundle,
    UnityArchive,
    UnityWebData,
    SerializedFile,
    Resource,
}

/// Compression layers applied outside the final recognized file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WrapperKind {
    Gzip,
    Brotli,
}

/// Whether a dispatched kind expands into mounted entries.
///
/// Bundle files mount their entries (recursively for nested containers), while
/// SerializedFiles and bare resources mount as single leaves. UnityArchive counts as
/// a container for dispatch even though its reader is unsupported, matching the
/// source detection precedence that registers its detector.
pub const fn is_container_kind(kind: FileKind) -> bool {
    matches!(
        kind,
        FileKind::UnityFs
            | FileKind::UnityRaw
            | FileKind::UnityWebBundle
            | FileKind::UnityWebData
            | FileKind::UnityArchive
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WrapperLayer {
    pub kind: WrapperKind,
    pub input_size: usize,
    pub output_size: usize,
}

/// Limits for wrapper recursion and the source parsers called by the dispatcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemeReaderLimits {
    pub max_input_bytes: usize,
    pub max_wrapper_depth: usize,
    pub max_decompressed_bytes_per_layer: usize,
    pub max_total_decompressed_bytes: usize,
    pub unity_fs: UnityFsLimits,
    pub raw_web: RawWebLimits,
    pub web_file: WebFileLimits,
}

impl Default for SchemeReaderLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 2 * 1024 * 1024 * 1024,
            max_wrapper_depth: 16,
            max_decompressed_bytes_per_layer: 512 * 1024 * 1024,
            max_total_decompressed_bytes: 2 * 1024 * 1024 * 1024,
            unity_fs: UnityFsLimits::default(),
            raw_web: RawWebLimits::default(),
            web_file: WebFileLimits::default(),
        }
    }
}

/// Bounds for mounting parsed container entries into a virtual filesystem.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContainerMountLimits {
    /// Maximum number of visited entries. Recursive mounts count entries across all levels.
    pub max_entries: usize,
    /// Maximum aggregate bytes inserted into the virtual filesystem.
    pub max_total_output_bytes: u64,
    /// Maximum nested-container depth. The root container has depth zero.
    pub max_depth: usize,
    /// Maximum aggregate owned bytes retained for nested containers during traversal.
    pub max_active_nested_bytes: u64,
    /// Maximum aggregate path bytes retained for the nested container-scope report.
    pub max_total_container_scope_path_bytes: u64,
    /// Maximum aggregate fixed Bundle.Name bytes retained for nested container metadata.
    pub max_total_container_bundle_name_bytes: u64,
    /// Maximum aggregate SerializedFile path bytes retained for source-order metadata.
    pub max_total_serialized_file_path_bytes: u64,
    /// Maximum aggregate ResourceFile path bytes retained for bundle-resource metadata.
    pub max_total_resource_file_path_bytes: u64,
    /// Maximum aggregate original ResourceFile name bytes retained for bundle-resource lookup.
    pub max_total_resource_file_name_bytes: u64,
    /// Maximum number of source-style failed-entry records retained by one container tree.
    pub max_failed_entries: usize,
    /// Maximum aggregate input-path, source-name, and virtual-path bytes retained for failures.
    pub max_total_failed_entry_metadata_bytes: u64,
}

impl Default for ContainerMountLimits {
    fn default() -> Self {
        Self {
            max_entries: 1_000_000,
            max_total_output_bytes: 2 * 1024 * 1024 * 1024,
            max_depth: 64,
            max_active_nested_bytes: 1024 * 1024 * 1024,
            max_total_container_scope_path_bytes: 64 * 1024 * 1024,
            max_total_container_bundle_name_bytes: 64 * 1024 * 1024,
            max_total_serialized_file_path_bytes: 64 * 1024 * 1024,
            max_total_resource_file_path_bytes: 64 * 1024 * 1024,
            max_total_resource_file_name_bytes: 64 * 1024 * 1024,
            max_failed_entries: 1_000_000,
            max_total_failed_entry_metadata_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Aggregate limits for scanning and mounting one local input directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchMountLimits {
    /// Maximum number of logical files, with each `.splitN` family counting once.
    pub max_inputs: usize,
    /// Maximum number of filesystem entries visited, including directories and split parts.
    pub max_directory_entries: usize,
    /// Maximum subdirectory depth below the selected input directory.
    pub max_directory_depth: usize,
    /// Maximum aggregate physical input size, counting every split part.
    pub max_total_input_bytes: u64,
    /// Maximum aggregate bytes in recursively mounted leaves; raw-preserving imports use the
    /// aggregate source-byte limit above because their inserted streams retain physical files.
    pub max_total_output_bytes: u64,
    /// Maximum source-style failed-entry records retained across the complete batch.
    pub max_failed_entries: usize,
    /// Maximum aggregate source-name and virtual-path bytes retained for failed entries.
    pub max_total_failed_entry_metadata_bytes: u64,
}

impl Default for BatchMountLimits {
    fn default() -> Self {
        Self {
            max_inputs: 100_000,
            max_directory_entries: 100_000,
            max_directory_depth: 256,
            max_total_input_bytes: 16 * 1024 * 1024 * 1024,
            max_total_output_bytes: 2 * 1024 * 1024 * 1024,
            max_failed_entries: 1_000_000,
            max_total_failed_entry_metadata_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerMountReport {
    /// Number of files inserted. Recursive mounting excludes nested container payloads.
    pub mounted_entries: usize,
    /// Total bytes in files inserted into the virtual filesystem.
    pub mounted_bytes: u64,
    /// Virtual roots of the source containers visited, in traversal order.
    pub container_scopes: Vec<String>,
    /// Aggregate bytes in `container_scopes` paths.
    pub container_scope_path_bytes: u64,
    /// Aggregate UTF-8 bytes in source-compatible fixed Bundle names.
    pub container_bundle_name_bytes: u64,
    /// Source container hierarchy in traversal order. Parent indexes refer to this vector; a
    /// child records the parent entry that contained it, while `scope_index` links to the
    /// corresponding virtual root when one was assigned.
    pub container_bundles: Vec<MountedContainerBundle>,
    /// SerializedFile leaves in source entry traversal order. `container_scope_index` indexes
    /// `container_scopes`; filtering these records by scope preserves that container's direct
    /// SerializedFile order even when traversal visits nested containers between direct leaves.
    pub serialized_files: Vec<MountedSerializedFile>,
    /// Aggregate bytes in `serialized_files` virtual paths.
    pub serialized_file_path_bytes: u64,
    /// ResourceFile leaves in source entry traversal order. Their local bundle index identifies
    /// the owning source container, and `resource_index` preserves its direct resource order.
    pub resource_files: Vec<MountedResourceFile>,
    /// Aggregate bytes in `resource_files` virtual paths.
    pub resource_file_path_bytes: u64,
    /// Aggregate bytes in original source names for `resource_files`.
    pub resource_file_name_bytes: u64,
    /// UnityFS entry reads that failed but did not stop sibling traversal, matching source
    /// `FileContainer.FailedFiles`. Limits that prevent retaining the complete list are fatal.
    pub failed_entries: Vec<FailedContainerEntry>,
    /// Aggregate input-path, source-name, and virtual-path bytes in `failed_entries`.
    pub failed_entry_metadata_bytes: u64,
}

/// One UnityFS directory entry that the reference records as a `FailedFile`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedContainerEntry {
    pub virtual_path: String,
    pub source_name: String,
    /// Local index of the owning container node in this mount report, when recursively mounted.
    pub container_bundle_index: Option<usize>,
    pub entry_index: usize,
    pub error: UnityFsError,
}

/// One source FileContainer in the recursively mounted SerializedBundle tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountedContainerBundle {
    pub scope_index: Option<usize>,
    pub parent_bundle_index: Option<usize>,
    pub parent_entry_index: Option<usize>,
    /// Source FileContainer.Name after SpecialFileNames.FixFileIdentifier.
    pub name_fixed: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountedSerializedFile {
    pub virtual_path: String,
    pub container_scope_index: Option<usize>,
    /// Local index of the owning container node in this mount report.
    pub container_bundle_index: Option<usize>,
    pub entry_index: usize,
    /// Zero-based order among this source container's direct SerializedFiles.
    pub collection_index: Option<usize>,
}

/// One ResourceFile leaf directly owned by a recursively mounted source container.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountedResourceFile {
    pub virtual_path: String,
    /// Original path/name from the source container entry, without the VFS mount root.
    pub name: String,
    pub container_scope_index: Option<usize>,
    /// Local index of the owning container node in this mount report.
    pub container_bundle_index: usize,
    pub entry_index: usize,
    /// Zero-based order among this source container's direct ResourceFiles.
    pub resource_index: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BatchMountReport {
    pub directory_entries_scanned: usize,
    pub discovered_inputs: usize,
    pub discovered_source_bytes: u64,
    pub completed_inputs: usize,
    pub mounted_entries: usize,
    pub mounted_bytes: u64,
    /// Source input and entry details for failed UnityFS entries in successfully traversed trees.
    pub failed_entries: Vec<BatchFailedContainerEntry>,
    /// Aggregate source-name and virtual-path bytes in `failed_entries`.
    pub failed_entry_metadata_bytes: u64,
    /// Source container topology for the recursively expanded top-level inputs.
    pub container_trees: Vec<Arc<BatchContainerTree>>,
    /// Explicit path-list inputs dispatched as root ResourceFiles, in caller input order.
    pub explicit_root_resource_files: Box<Vec<MountedExplicitResourceFile>>,
}

/// One recursively expanded top-level container, retaining entry order for workspace joins.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchContainerTree {
    pub virtual_path: String,
    pub container_scopes: Vec<String>,
    pub container_bundles: Vec<MountedContainerBundle>,
    pub serialized_files: Vec<MountedSerializedFile>,
    pub resource_files: Vec<MountedResourceFile>,
    pub failed_entries: Vec<FailedContainerEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchFailedContainerEntry {
    pub input_path: std::path::PathBuf,
    pub entry: FailedContainerEntry,
}

/// A top-level path-list input classified as a ResourceFile by `SchemeReader` and mounted in VFS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountedExplicitResourceFile {
    pub file_name: String,
    pub virtual_path: String,
    pub kind: FileKind,
}

/// One explicitly selected local path and its virtual destination root.
///
/// A split-file group should be represented by one base path or one `.splitN` part path. The
/// destination is used as the root for container entries and as the parent for standalone files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathMountInput {
    pub path: std::path::PathBuf,
    pub destination_root: Option<String>,
}
