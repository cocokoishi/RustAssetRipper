use std::borrow::Cow;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use crate::compression::wrappers::{
    decompress_brotli, decompress_brotli_stream, decompress_gzip, decompress_gzip_stream,
    has_brotli_file_signature, has_gzip_magic, WrapperCodecError,
};
use crate::container::raw_web::{RawWebBundle, RawWebError, RawWebKind, RawWebLimits};
use crate::container::unityfs::{UnityFsBundle, UnityFsError, UnityFsLimits, UnityFsStreamBundle};
use crate::container::web_file::{WebFile, WebFileError, WebFileLimits};
use crate::io::discovery::{discover_directory_inputs, DirectoryDiscoveryError};
use crate::io::filesystem::VirtualFileSystem;
use crate::io::special_names::fix_file_identifier;
use crate::io::stream::{logical_file_name, logical_file_path, open_logical_file, SmartStream};
use crate::serialized_file::file::{SerializedFile, SerializedFileError};
use crate::serialized_file::format_version::FormatVersion;

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

struct BatchInputRequest {
    path: std::path::PathBuf,
    source_bytes: u64,
    standalone_destination: Option<String>,
    container_destination: Option<String>,
    explicit_path: bool,
}

#[derive(Debug)]
pub struct BatchMountError {
    pub partial_report: Box<BatchMountReport>,
    pub input_path: Option<std::path::PathBuf>,
    pub kind: BatchMountErrorKind,
}

#[derive(Debug)]
pub enum BatchMountErrorKind {
    DirectoryDiscovery(Box<DirectoryDiscoveryError>),
    InputCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    ContainerEntryCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    DuplicateInputPath,
    ExplicitInputIsDirectory,
    ExplicitInputIsSymbolicLink,
    ExplicitInputNotRegular,
    TotalInputLimitExceeded {
        requested_total: u64,
        limit: u64,
    },
    InputFileLimitExceeded {
        size: u64,
        limit: u64,
    },
    InputSizeChanged {
        discovered_bytes: u64,
        opened_bytes: u64,
    },
    InputListAllocationFailed {
        requested: usize,
    },
    ContainerTreeAllocationFailed {
        requested: usize,
    },
    TotalOutputLimitExceeded {
        requested_total: u64,
        limit: u64,
    },
    FailedEntryCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    FailedEntryMetadataLimitExceeded {
        requested: u64,
        limit: u64,
    },
    FailedEntryAllocationFailed {
        requested: usize,
    },
    InvalidInputPathEncoding,
    ReportOverflow,
    InputMount(Box<ContainerMountError>),
}

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

#[derive(Debug)]
pub struct ContainerMountError {
    pub partial_report: Box<ContainerMountReport>,
    pub kind: ContainerMountErrorKind,
}

#[derive(Debug)]
pub enum ContainerMountErrorKind {
    Scheme(SchemeError),
    NotContainer,
    InputPathHasNoFileName,
    EntryCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    EntryCountOverflow,
    NestingDepthLimitExceeded {
        depth: usize,
        limit: usize,
    },
    ActiveNestedContainerLimitExceeded {
        entry_index: usize,
        requested_total: u64,
        limit: u64,
    },
    ContainerScopePathSizeOverflow,
    ContainerScopePathLimitExceeded {
        requested: u64,
        limit: u64,
    },
    ContainerScopeCountOverflow,
    ContainerScopeAllocationFailed {
        requested: usize,
    },
    ContainerBundleNameSizeOverflow,
    ContainerBundleNameLimitExceeded {
        requested: u64,
        limit: u64,
    },
    ContainerBundleCountOverflow,
    ContainerBundleAllocationFailed {
        requested: usize,
    },
    ContainerBundleCollectionIndexAllocationFailed {
        requested: usize,
    },
    ContainerBundleResourceIndexAllocationFailed {
        requested: usize,
    },
    ContainerBundleParentIndexOutOfBounds {
        index: usize,
        count: usize,
    },
    ContainerBundleParentMetadataMismatch,
    SerializedFilePathSizeOverflow,
    SerializedFilePathLimitExceeded {
        requested: u64,
        limit: u64,
    },
    SerializedFilePathAllocationFailed {
        requested: usize,
    },
    SerializedFileEntryCountOverflow,
    SerializedFileEntryAllocationFailed {
        requested: usize,
    },
    ContainerScopeIndexOutOfBounds {
        index: usize,
        count: usize,
    },
    ContainerCollectionIndexOverflow,
    ResourceFilePathSizeOverflow,
    ResourceFilePathLimitExceeded {
        requested: u64,
        limit: u64,
    },
    ResourceFilePathAllocationFailed {
        requested: usize,
    },
    ResourceFileNameSizeOverflow,
    ResourceFileNameLimitExceeded {
        requested: u64,
        limit: u64,
    },
    ResourceFileNameAllocationFailed {
        requested: usize,
    },
    ResourceFileEntryCountOverflow,
    ResourceFileEntryAllocationFailed {
        requested: usize,
    },
    ContainerResourceIndexOverflow,
    FailedEntryCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    FailedEntryMetadataLimitExceeded {
        requested: u64,
        limit: u64,
    },
    FailedEntryAllocationFailed {
        requested: usize,
    },
    TotalOutputLimitExceeded {
        entry_index: usize,
        requested_total: u64,
        limit: u64,
    },
    TotalOutputSizeOverflow {
        entry_index: usize,
    },
    InvalidEntryPath {
        entry_index: usize,
        path: String,
    },
    EntryPathTooLong {
        entry_index: usize,
        path: String,
        limit: usize,
    },
    EntryPathAllocationFailed {
        entry_index: usize,
        requested: usize,
    },
    EntryAllocationFailed {
        entry_index: usize,
        requested: usize,
    },
    FileSystem {
        entry_index: usize,
        path: String,
        source: io::Error,
    },
}

/// Input bytes after any recognized GZip/Brotli wrappers have been removed.
///
/// The original input is borrowed when no wrapper exists. For wrapped input only the latest
/// decoded layer is retained, so nested wrappers do not keep every intermediate expansion alive.
#[derive(Debug)]
pub struct DispatchedFile<'a> {
    contents: Cow<'a, [u8]>,
    kind: FileKind,
    wrappers: Vec<WrapperLayer>,
    limits: SchemeReaderLimits,
}

impl<'a> DispatchedFile<'a> {
    pub fn kind(&self) -> FileKind {
        self.kind
    }

    pub fn wrappers(&self) -> &[WrapperLayer] {
        &self.wrappers
    }

    pub fn bytes(&self) -> &[u8] {
        self.contents.as_ref()
    }

    /// Match the source `SchemeReader.IsReadableFile` behavior, which does not count unknown
    /// resource files as scheme-readable.
    pub fn is_readable(&self) -> bool {
        self.kind != FileKind::Resource || !self.wrappers.is_empty()
    }

    /// Dispatch one directory entry from any supported container through the same registry.
    /// The returned file borrows from this container when the entry is stored without decoding.
    pub fn dispatch_entry(&self, index: usize) -> Result<DispatchedFile<'_>, SchemeError> {
        match self.kind {
            FileKind::UnityFs => {
                let ParsedFile::UnityFs(bundle) = self.parse()? else {
                    return Err(SchemeError::new(SchemeErrorKind::NotContainer));
                };
                let entry = bundle
                    .read_entry(index)
                    .map_err(|error| SchemeError::new(SchemeErrorKind::UnityFs(error)))?;
                SchemeReader::read_cow_with_limits(entry, self.limits)
            }
            FileKind::UnityRaw | FileKind::UnityWebBundle => {
                let ParsedFile::RawWeb(bundle) = self.parse()? else {
                    return Err(SchemeError::new(SchemeErrorKind::NotContainer));
                };
                let entry = bundle
                    .into_entry(index)
                    .map_err(|error| SchemeError::new(SchemeErrorKind::RawWeb(error)))?;
                SchemeReader::read_cow_with_limits(entry, self.limits)
            }
            FileKind::UnityWebData => self.parse_web_data_entry(index),
            _ => Err(SchemeError::new(SchemeErrorKind::NotContainer)),
        }
    }

    /// Dispatch one resource entry in a Unity WebData file through the same scheme registry.
    pub fn parse_web_data_entry(&self, index: usize) -> Result<DispatchedFile<'_>, SchemeError> {
        if self.kind != FileKind::UnityWebData {
            return Err(SchemeError::new(SchemeErrorKind::NotWebDataContainer));
        }
        let ParsedFile::WebData(web_file) = self.parse()? else {
            return Err(SchemeError::new(SchemeErrorKind::NotWebDataContainer));
        };
        let bytes = web_file.entry_data(index).ok_or_else(|| {
            SchemeError::new(SchemeErrorKind::WebDataEntryIndexOutOfBounds {
                index,
                entry_count: web_file.entries.len(),
            })
        })?;
        SchemeReader::read_cow_with_limits(Cow::Borrowed(bytes), self.limits)
    }

    /// Parse the detected leaf with its format parser.
    pub fn parse(&self) -> Result<ParsedFile<'_>, SchemeError> {
        let bytes = self.contents.as_ref();
        match self.kind {
            FileKind::UnityFs => UnityFsBundle::parse_with_limits(bytes, self.limits.unity_fs)
                .map(ParsedFile::UnityFs)
                .map_err(|error| SchemeError::new(SchemeErrorKind::UnityFs(error))),
            FileKind::UnityRaw | FileKind::UnityWebBundle => {
                let bundle = RawWebBundle::parse_with_limits(bytes, self.limits.raw_web)
                    .map_err(|error| SchemeError::new(SchemeErrorKind::RawWeb(error)))?;
                let expected_kind = if self.kind == FileKind::UnityRaw {
                    RawWebKind::UnityRaw
                } else {
                    RawWebKind::UnityWeb
                };
                if bundle.header.kind != expected_kind {
                    return Err(SchemeError::new(SchemeErrorKind::DetectedKindChanged {
                        detected: self.kind,
                        parsed: bundle.header.kind,
                    }));
                }
                Ok(ParsedFile::RawWeb(bundle))
            }
            FileKind::UnityArchive => {
                Err(SchemeError::new(SchemeErrorKind::ArchiveReadUnsupported))
            }
            FileKind::UnityWebData => WebFile::parse_with_limits(bytes, self.limits.web_file)
                .map(ParsedFile::WebData)
                .map_err(|error| SchemeError::new(SchemeErrorKind::WebFile(error))),
            FileKind::SerializedFile => SerializedFile::parse(bytes)
                .map(ParsedFile::SerializedFile)
                .map_err(|error| SchemeError::new(SchemeErrorKind::SerializedFile(error))),
            FileKind::Resource => Ok(ParsedFile::Resource(bytes)),
        }
    }

    /// Mount each direct bundle/WebData entry under `destination_root` in its parsed path form.
    ///
    /// Entry bytes are placed in owned SmartStreams. UnityFS block-read failures are retained in
    /// `failed_entries` and later siblings continue, matching the source FileContainer behavior.
    /// Bounds and failures that prevent safe traversal return an error with the mounted partial
    /// report; failed-entry count and metadata have explicit limits.
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

impl ContainerMountError {
    fn new(partial_report: ContainerMountReport, kind: ContainerMountErrorKind) -> Self {
        Self {
            partial_report: Box::new(partial_report),
            kind,
        }
    }
}

impl BatchMountError {
    fn new(
        partial_report: BatchMountReport,
        input_path: Option<std::path::PathBuf>,
        kind: BatchMountErrorKind,
    ) -> Self {
        Self {
            partial_report: Box::new(partial_report),
            input_path,
            kind,
        }
    }
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

impl Display for BatchMountErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirectoryDiscovery(error) => Display::fmt(error, formatter),
            Self::InputCountLimitExceeded { count, limit } => write!(
                formatter,
                "input list contains {count} paths, exceeding batch limit {limit}"
            ),
            Self::ContainerEntryCountLimitExceeded { count, limit } => write!(
                formatter,
                "directory contains {count} logical files, exceeding mount entry limit {limit}"
            ),
            Self::DuplicateInputPath => formatter.write_str(
                "input list contains the same logical path and destination more than once",
            ),
            Self::ExplicitInputIsDirectory => formatter
                .write_str("path-list inputs must be files; use mount_directory for directories"),
            Self::ExplicitInputIsSymbolicLink => {
                formatter.write_str("symbolic-link inputs are not allowed")
            }
            Self::ExplicitInputNotRegular => {
                formatter.write_str("path-list input is not a regular file")
            }
            Self::TotalInputLimitExceeded {
                requested_total,
                limit,
            } => write!(
                formatter,
                "directory inputs total {requested_total} bytes, exceeding batch limit {limit}"
            ),
            Self::InputFileLimitExceeded { size, limit } => write!(
                formatter,
                "input has {size} bytes, exceeding per-file limit {limit}"
            ),
            Self::InputSizeChanged {
                discovered_bytes,
                opened_bytes,
            } => write!(
                formatter,
                "input size changed from {discovered_bytes} bytes during discovery to {opened_bytes} bytes when opened"
            ),
            Self::InputListAllocationFailed { requested } => write!(
                formatter,
                "could not allocate storage for {requested} path-list inputs"
            ),
            Self::ContainerTreeAllocationFailed { requested } => write!(
                formatter,
                "could not allocate storage for {requested} expanded container trees"
            ),
            Self::TotalOutputLimitExceeded {
                requested_total,
                limit,
            } => write!(
                formatter,
                "mounted output would total {requested_total} bytes, exceeding batch limit {limit}"
            ),
            Self::FailedEntryCountLimitExceeded { count, limit } => write!(
                formatter,
                "batch has {count} failed container entries, exceeding diagnostic limit {limit}"
            ),
            Self::FailedEntryMetadataLimitExceeded { requested, limit } => write!(
                formatter,
                "failed-entry names and paths use {requested} bytes, exceeding batch diagnostic metadata limit {limit}"
            ),
            Self::FailedEntryAllocationFailed { requested } => write!(
                formatter,
                "could not reserve space for {requested} batch failed-entry records"
            ),
            Self::InvalidInputPathEncoding => {
                formatter.write_str("input path cannot be represented as a virtual UTF-8 path")
            }
            Self::ReportOverflow => formatter.write_str("batch mount report counters overflow"),
            Self::InputMount(error) => Display::fmt(error, formatter),
        }
    }
}

impl Display for BatchMountError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.input_path {
            write!(formatter, "batch mount stopped at '{}': ", path.display())?;
        } else {
            formatter.write_str("batch mount stopped: ")?;
        }
        write!(
            formatter,
            "{} ({} of {} inputs completed, {} mounted entries / {} bytes)",
            self.kind,
            self.partial_report.completed_inputs,
            self.partial_report.discovered_inputs,
            self.partial_report.mounted_entries,
            self.partial_report.mounted_bytes,
        )
    }
}

impl Error for BatchMountError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            BatchMountErrorKind::DirectoryDiscovery(error) => Some(error),
            BatchMountErrorKind::InputMount(error) => Some(error),
            _ => None,
        }
    }
}

impl Display for ContainerMountErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scheme(error) => Display::fmt(error, formatter),
            Self::NotContainer => formatter.write_str("input is not a supported file container"),
            Self::InputPathHasNoFileName => {
                formatter.write_str("input path has no UTF-8 logical filename")
            }
            Self::EntryCountLimitExceeded { count, limit } => write!(
                formatter,
                "container traversal reached {count} entries, exceeding mount limit {limit}"
            ),
            Self::EntryCountOverflow => {
                formatter.write_str("container traversal entry count overflows usize")
            }
            Self::NestingDepthLimitExceeded { depth, limit } => write!(
                formatter,
                "container nesting depth {depth} exceeds mount limit {limit}"
            ),
            Self::ActiveNestedContainerLimitExceeded {
                entry_index,
                requested_total,
                limit,
            } => write!(
                formatter,
                "nested container entry {entry_index} would retain {requested_total} owned bytes, exceeding active-container limit {limit}"
            ),
            Self::ContainerScopePathSizeOverflow => {
                formatter.write_str("container-scope path byte count overflows u64")
            }
            Self::ContainerScopePathLimitExceeded { requested, limit } => write!(
                formatter,
                "container-scope paths use {requested} bytes, exceeding scope limit {limit}"
            ),
            Self::ContainerScopeCountOverflow => {
                formatter.write_str("container-scope count overflows usize")
            }
            Self::ContainerScopeAllocationFailed { requested } => write!(
                formatter,
                "could not reserve {requested} bytes for container-scope metadata"
            ),
            Self::ContainerBundleNameSizeOverflow => {
                formatter.write_str("fixed Bundle.Name byte count overflows u64")
            }
            Self::ContainerBundleNameLimitExceeded { requested, limit } => write!(
                formatter,
                "fixed Bundle.Name values use {requested} bytes, exceeding name limit {limit}"
            ),
            Self::ContainerBundleCountOverflow => {
                formatter.write_str("container bundle count overflows usize")
            }
            Self::ContainerBundleAllocationFailed { requested } => write!(
                formatter,
                "could not reserve space for {requested} container bundle records"
            ),
            Self::ContainerBundleCollectionIndexAllocationFailed { requested } => write!(
                formatter,
                "could not reserve collection-order counters for {requested} container bundles"
            ),
            Self::ContainerBundleResourceIndexAllocationFailed { requested } => write!(
                formatter,
                "could not reserve resource-order counters for {requested} container bundles"
            ),
            Self::ContainerBundleParentIndexOutOfBounds { index, count } => write!(
                formatter,
                "container bundle parent index {index} is outside {count} recorded bundles"
            ),
            Self::ContainerBundleParentMetadataMismatch => formatter.write_str(
                "container bundle parent index and parent entry index must either both be present or both be absent",
            ),
            Self::SerializedFilePathSizeOverflow => {
                formatter.write_str("SerializedFile path byte count overflows u64")
            }
            Self::SerializedFilePathLimitExceeded { requested, limit } => write!(
                formatter,
                "SerializedFile paths use {requested} bytes, exceeding metadata limit {limit}"
            ),
            Self::SerializedFilePathAllocationFailed { requested } => write!(
                formatter,
                "could not reserve {requested} bytes for SerializedFile path metadata"
            ),
            Self::SerializedFileEntryCountOverflow => {
                formatter.write_str("SerializedFile entry metadata count overflows usize")
            }
            Self::SerializedFileEntryAllocationFailed { requested } => write!(
                formatter,
                "could not reserve space for {requested} SerializedFile entry records"
            ),
            Self::ContainerScopeIndexOutOfBounds { index, count } => write!(
                formatter,
                "container-scope index {index} is outside the {count} recorded scopes"
            ),
            Self::ContainerCollectionIndexOverflow => {
                formatter.write_str("container collection order overflows usize")
            }
            Self::ResourceFilePathSizeOverflow => {
                formatter.write_str("ResourceFile path byte count overflows u64")
            }
            Self::ResourceFilePathLimitExceeded { requested, limit } => write!(
                formatter,
                "ResourceFile paths use {requested} bytes, exceeding metadata limit {limit}"
            ),
            Self::ResourceFilePathAllocationFailed { requested } => write!(
                formatter,
                "could not reserve {requested} bytes for ResourceFile path metadata"
            ),
            Self::ResourceFileNameSizeOverflow => {
                formatter.write_str("ResourceFile name byte count overflows u64")
            }
            Self::ResourceFileNameLimitExceeded { requested, limit } => write!(
                formatter,
                "original ResourceFile names use {requested} bytes, exceeding metadata limit {limit}"
            ),
            Self::ResourceFileNameAllocationFailed { requested } => write!(
                formatter,
                "could not reserve {requested} bytes for original ResourceFile name metadata"
            ),
            Self::ResourceFileEntryCountOverflow => {
                formatter.write_str("ResourceFile entry metadata count overflows usize")
            }
            Self::ResourceFileEntryAllocationFailed { requested } => write!(
                formatter,
                "could not reserve space for {requested} ResourceFile entry records"
            ),
            Self::ContainerResourceIndexOverflow => {
                formatter.write_str("container resource order overflows usize")
            }
            Self::FailedEntryCountLimitExceeded { count, limit } => write!(
                formatter,
                "container has {count} failed entries, exceeding diagnostic limit {limit}"
            ),
            Self::FailedEntryMetadataLimitExceeded { requested, limit } => write!(
                formatter,
                "failed-entry names and paths use {requested} bytes, exceeding diagnostic metadata limit {limit}"
            ),
            Self::FailedEntryAllocationFailed { requested } => write!(
                formatter,
                "could not reserve space for {requested} failed-entry records"
            ),
            Self::TotalOutputLimitExceeded {
                entry_index,
                requested_total,
                limit,
            } => write!(
                formatter,
                "mounting entry {entry_index} would require {requested_total} bytes, exceeding output limit {limit}"
            ),
            Self::TotalOutputSizeOverflow { entry_index } => write!(
                formatter,
                "total mounted output size overflows while counting entry {entry_index}"
            ),
            Self::InvalidEntryPath {
                entry_index,
                path,
            } => write!(
                formatter,
                "entry {entry_index} path '{path}' is empty or contains a parent-directory component"
            ),
            Self::EntryPathTooLong {
                entry_index,
                path,
                limit,
            } => write!(
                formatter,
                "entry {entry_index} path '{path}' exceeds the {limit}-byte path limit"
            ),
            Self::EntryPathAllocationFailed {
                entry_index,
                requested,
            } => write!(
                formatter,
                "could not allocate {requested} bytes for the path of entry {entry_index}"
            ),
            Self::EntryAllocationFailed {
                entry_index,
                requested,
            } => write!(
                formatter,
                "could not allocate {requested} bytes while mounting entry {entry_index}"
            ),
            Self::FileSystem {
                entry_index,
                path,
                source,
            } => write!(
                formatter,
                "could not mount entry {entry_index} at '{path}': {source}"
            ),
        }
    }
}

impl Display for ContainerMountError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "container mount stopped after {} entries ({} bytes): {}",
            self.partial_report.mounted_entries, self.partial_report.mounted_bytes, self.kind
        )
    }
}

impl Error for ContainerMountError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ContainerMountErrorKind::Scheme(error) => Some(error),
            ContainerMountErrorKind::FileSystem { source, .. } => Some(source),
            _ => None,
        }
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

fn mount_container_entry_at_path<'a>(
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

fn checked_container_entry_path(
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
