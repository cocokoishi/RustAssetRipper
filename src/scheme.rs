//! Source-compatible detection and recursive decoding of top-level file schemes.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use crate::compression::wrappers::{
    decompress_brotli, decompress_gzip, has_brotli_file_signature, has_gzip_magic,
    WrapperCodecError,
};
use crate::container::raw_web::{RawWebBundle, RawWebError, RawWebKind, RawWebLimits};
use crate::container::unityfs::{UnityFsBundle, UnityFsError, UnityFsLimits};
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
}

impl Default for BatchMountLimits {
    fn default() -> Self {
        Self {
            max_inputs: 100_000,
            max_directory_entries: 100_000,
            max_directory_depth: 256,
            max_total_input_bytes: 16 * 1024 * 1024 * 1024,
            max_total_output_bytes: 2 * 1024 * 1024 * 1024,
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
    /// Explicit path-list inputs dispatched as root ResourceFiles, in caller input order.
    pub explicit_root_resource_files: Box<Vec<MountedExplicitResourceFile>>,
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
    pub partial_report: BatchMountReport,
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
    TotalOutputLimitExceeded {
        requested_total: u64,
        limit: u64,
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
    /// Entry bytes are placed in owned SmartStreams. A failure reports the number and byte count
    /// already mounted; earlier entries are not silently rolled back or reported as a full mount.
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
                    let bytes = bundle.read_entry(index).map_err(|error| {
                        ContainerMountError::new(
                            report.clone(),
                            ContainerMountErrorKind::Scheme(SchemeError::new(
                                SchemeErrorKind::UnityFs(error),
                            )),
                        )
                    })?;
                    mount_container_entry(
                        filesystem,
                        destination_root,
                        node.path.as_str(),
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
    /// mounted bytes are bounded across the complete traversal.
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
                    let bytes = bundle.read_entry(index).map_err(|error| {
                        ContainerMountError::new(
                            state.report.clone(),
                            ContainerMountErrorKind::Scheme(SchemeError::new(
                                SchemeErrorKind::UnityFs(error),
                            )),
                        )
                    })?;
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
            partial_report,
            input_path,
            kind,
        }
    }
}

fn merge_batch_mount_report(
    batch: &mut BatchMountReport,
    input: &ContainerMountReport,
) -> Result<(), ()> {
    batch.mounted_entries = batch
        .mounted_entries
        .checked_add(input.mounted_entries)
        .ok_or(())?;
    batch.mounted_bytes = batch
        .mounted_bytes
        .checked_add(input.mounted_bytes)
        .ok_or(())?;
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
            Self::TotalOutputLimitExceeded {
                requested_total,
                limit,
            } => write!(
                formatter,
                "mounted output would total {requested_total} bytes, exceeding batch limit {limit}"
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

fn directory_input_destination(
    destination_root: Option<&str>,
    relative_path: &Path,
) -> Result<Option<String>, ()> {
    let relative_parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    directory_relative_destination(destination_root, relative_parent)
}

fn directory_relative_destination(
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
#[derive(Debug)]
pub enum ParsedFile<'a> {
    UnityFs(UnityFsBundle<'a>),
    RawWeb(RawWebBundle<'a>),
    WebData(WebFile<'a>),
    SerializedFile(SerializedFile<'a>),
    Resource(&'a [u8]),
}

pub struct SchemeReader;

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
    pub fn read_path(
        path: impl AsRef<Path>,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'static>, SchemeError> {
        let mut stream = open_logical_file(path)
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        Self::read_stream(&mut stream, limits)
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
        let dispatched = Self::read_path(input_path, scheme_limits).map_err(|error| {
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
            let dispatched = Self::read_path(&input.path, scheme_limits).map_err(|error| {
                BatchMountError::new(
                    report.clone(),
                    Some(input.path.clone()),
                    BatchMountErrorKind::InputMount(Box::new(ContainerMountError::new(
                        ContainerMountReport::default(),
                        ContainerMountErrorKind::Scheme(error),
                    ))),
                )
            })?;
            let is_container = is_container_kind(dispatched.kind);
            let destination = if is_container {
                input.container_destination.as_deref()
            } else {
                input.standalone_destination.as_deref()
            };
            let explicit_root_resource_file =
                if input.explicit_path && dispatched.kind() == FileKind::Resource {
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
                        kind: dispatched.kind(),
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
            let mut per_input_mount_limits = mount_limits;
            per_input_mount_limits.max_total_output_bytes = per_input_mount_limits
                .max_total_output_bytes
                .min(remaining_output);
            let output_before_input = report.mounted_bytes;

            let mounted = Self::mount_dispatched_path(
                &input.path,
                dispatched,
                filesystem,
                destination,
                per_input_mount_limits,
            );
            match mounted {
                Ok(input_report) => {
                    merge_batch_mount_report(&mut report, &input_report).map_err(|_| {
                        BatchMountError::new(
                            report.clone(),
                            Some(input.path.clone()),
                            BatchMountErrorKind::ReportOverflow,
                        )
                    })?;
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
                    merge_batch_mount_report(&mut report, &input_error.partial_report).map_err(
                        |_| {
                            BatchMountError::new(
                                report.clone(),
                                Some(input.path.clone()),
                                BatchMountErrorKind::ReportOverflow,
                            )
                        },
                    )?;
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

    /// Read a complete seekable logical file, such as a file assembled from split parts, into
    /// bounded storage and dispatch it from its beginning.
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
        let length = usize::try_from(stream_length).map_err(|_| {
            SchemeError::new(SchemeErrorKind::InputLengthNotRepresentable {
                length: stream_length,
            })
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
        Self::read_cow_with_limits(Cow::Owned(contents), limits)
    }

    pub fn read_with_limits(
        input: &[u8],
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'_>, SchemeError> {
        Self::read_cow_with_limits(Cow::Borrowed(input), limits)
    }

    fn read_cow_with_limits<'a>(
        mut contents: Cow<'a, [u8]>,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'a>, SchemeError> {
        if contents.len() > limits.max_input_bytes {
            return Err(SchemeError::new(SchemeErrorKind::InputSizeLimitExceeded {
                requested: u64::try_from(contents.len()).unwrap_or(u64::MAX),
                limit: limits.max_input_bytes,
            }));
        }
        let mut wrappers = Vec::new();
        let mut total_decompressed = 0usize;

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
    if let Some(kind) = detect_primary_file_kind(input) {
        return kind;
    }
    if is_serialized_file_with_length(input, total_length) {
        FileKind::SerializedFile
    } else {
        FileKind::Resource
    }
}

fn detect_primary_file_kind(input: &[u8]) -> Option<FileKind> {
    if has_bundle_signature(input, b"UnityFS") {
        Some(FileKind::UnityFs)
    } else if has_bundle_signature(input, b"UnityRaw") {
        Some(FileKind::UnityRaw)
    } else if has_bundle_signature(input, b"UnityWeb") {
        Some(FileKind::UnityWebBundle)
    } else if has_bundle_signature(input, b"UnityArchive") {
        Some(FileKind::UnityArchive)
    } else if WebFile::is_web_file(input) {
        Some(FileKind::UnityWebData)
    } else {
        None
    }
}

fn has_bundle_signature(input: &[u8], magic: &[u8]) -> bool {
    const SOURCE_MINIMUM_STREAM_LENGTH: usize = 0x20;
    input.len() >= SOURCE_MINIMUM_STREAM_LENGTH
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

#[cfg(test)]
mod tests {
    use super::{
        BatchMountErrorKind, BatchMountLimits, ContainerMountErrorKind, ContainerMountLimits,
        FileKind, MountedContainerBundle, MountedExplicitResourceFile, MountedResourceFile,
        MountedSerializedFile, PathMountInput, SchemeErrorKind, SchemeReader, SchemeReaderLimits,
        WrapperKind,
    };
    use crate::compression::wrappers::{
        compress_gzip, decompress_brotli, has_brotli_file_signature,
    };
    use crate::io::filesystem::VirtualFileSystem;
    use std::io::{Cursor, Seek, SeekFrom};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_temp_file_path(extension: &str) -> PathBuf {
        static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "assetripper-rust-mount-{}-{id}.{extension}",
            std::process::id()
        ))
    }

    fn webdata_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
        assert!(entries.iter().all(|(name, _)| name.len() < 128));
        let header_size = 20
            + entries
                .iter()
                .map(|(name, _)| 9 + name.len())
                .sum::<usize>();
        let mut input = b"UnityWebData1.0\0".to_vec();
        input.extend_from_slice(&(header_size as i32).to_le_bytes());

        let mut data_offset = header_size;
        for (name, payload) in entries {
            input.extend_from_slice(&(data_offset as i32).to_le_bytes());
            input.extend_from_slice(&(payload.len() as i32).to_le_bytes());
            input.push(name.len() as u8);
            input.extend_from_slice(name.as_bytes());
            data_offset += payload.len();
        }
        for (_, payload) in entries {
            input.extend_from_slice(payload);
        }
        input
    }

    fn serialized_file_header_fixture() -> Vec<u8> {
        // Scheme detection only needs a valid legacy header envelope; this deliberately minimal
        // payload is not passed to the SerializedFile parser.
        let mut bytes = vec![0; 29];
        bytes[0..4].copy_from_slice(&13_i32.to_be_bytes());
        bytes[4..8].copy_from_slice(&29_u32.to_be_bytes());
        bytes[8..12].copy_from_slice(&6_i32.to_be_bytes());
        bytes
    }

    fn write_bits(bytes: &mut Vec<u8>, bit_position: &mut usize, value: u32, count: usize) {
        for bit in 0..count {
            let byte_index = *bit_position / 8;
            if bytes.len() <= byte_index {
                bytes.resize(byte_index + 1, 0);
            }
            if value & (1 << bit) != 0 {
                bytes[byte_index] |= 1 << (*bit_position % 8);
            }
            *bit_position += 1;
        }
    }

    fn unity_marked_brotli_fixture(output: &[u8], window_bits: u8) -> Vec<u8> {
        let marker = b"UnityWeb Compressed Content (brotli)";
        let mut bytes = Vec::new();
        let mut bit_position = 0;

        // RFC 7932 stream header, followed by an empty metadata meta-block.
        let (window_code, window_code_bits) = match window_bits {
            10 => (33, 7),
            11 => (49, 7),
            12 => (65, 7),
            13 => (81, 7),
            14 => (97, 7),
            15 => (113, 7),
            16 => (0, 1),
            17 => (1, 7),
            18 => (3, 4),
            19 => (5, 4),
            20 => (7, 4),
            21 => (9, 4),
            22 => (11, 4),
            23 => (13, 4),
            24 => (15, 4),
            _ => panic!("invalid fixture WBITS"),
        };
        write_bits(&mut bytes, &mut bit_position, window_code, window_code_bits);
        write_bits(&mut bytes, &mut bit_position, 0, 1); // ISLAST
        write_bits(&mut bytes, &mut bit_position, 3, 2); // MNIBBLES=0
        write_bits(&mut bytes, &mut bit_position, 0, 1); // reserved
        write_bits(&mut bytes, &mut bit_position, 1, 2); // MSKIPBYTES=1
        write_bits(&mut bytes, &mut bit_position, marker.len() as u32 - 1, 8);
        while bit_position & 7 != 0 {
            write_bits(&mut bytes, &mut bit_position, 0, 1);
        }
        bytes.extend_from_slice(marker);
        bit_position += marker.len() * 8;

        // Non-final uncompressed meta-block containing the requested payload.
        let length_minus_one = output.len().checked_sub(1).unwrap() as u32;
        write_bits(&mut bytes, &mut bit_position, 0, 1); // ISLAST=0
        write_bits(&mut bytes, &mut bit_position, 0, 2); // MNIBBLES=4
        write_bits(&mut bytes, &mut bit_position, length_minus_one, 16);
        write_bits(&mut bytes, &mut bit_position, 1, 1); // ISUNCOMPRESSED
        while bit_position & 7 != 0 {
            write_bits(&mut bytes, &mut bit_position, 0, 1);
        }
        bytes.extend_from_slice(output);
        bit_position += output.len() * 8;

        // Final empty meta-block.
        write_bits(&mut bytes, &mut bit_position, 1, 1); // ISLAST
        write_bits(&mut bytes, &mut bit_position, 1, 1); // ISLASTEMPTY
        while bit_position & 7 != 0 {
            write_bits(&mut bytes, &mut bit_position, 0, 1);
        }
        bytes
    }

    #[test]
    fn dispatches_nested_gzip_wrappers_to_the_inner_resource() {
        let once = compress_gzip(b"inner payload", 1024).unwrap();
        let twice = compress_gzip(&once, 1024).unwrap();
        let file = SchemeReader::read(&twice).unwrap();

        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"inner payload");
        assert_eq!(
            file.wrappers()
                .iter()
                .map(|layer| layer.kind)
                .collect::<Vec<_>>(),
            [WrapperKind::Gzip, WrapperKind::Gzip]
        );
        assert!(matches!(
            file.parse().unwrap(),
            super::ParsedFile::Resource(_)
        ));
    }

    #[test]
    fn dispatches_a_complete_seekable_stream_from_its_start() {
        let input = compress_gzip(b"stream payload", 1024).unwrap();
        let mut stream = Cursor::new(input);
        stream.seek(SeekFrom::End(0)).unwrap();

        let file = SchemeReader::read_stream(&mut stream, SchemeReaderLimits::default()).unwrap();
        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"stream payload");
        assert_eq!(file.wrappers()[0].kind, WrapperKind::Gzip);
    }

    #[test]
    fn enforces_the_input_limit_before_allocating_stream_contents() {
        let mut stream = Cursor::new(b"too many input bytes".to_vec());
        let error = SchemeReader::read_stream(
            &mut stream,
            SchemeReaderLimits {
                max_input_bytes: 4,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind(),
            SchemeErrorKind::InputSizeLimitExceeded { limit: 4, .. }
        ));
    }

    #[test]
    fn recursively_dispatches_webdata_entries() {
        let payload = compress_gzip(b"embedded asset", 1024).unwrap();
        let name = b"main.asset";
        let header_size = 16 + 4 + 4 + 4 + 1 + name.len();
        let mut input = b"UnityWebData1.0\0".to_vec();
        input.extend_from_slice(&(header_size as i32).to_le_bytes());
        input.extend_from_slice(&(header_size as i32).to_le_bytes());
        input.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        input.push(name.len() as u8);
        input.extend_from_slice(name);
        input.extend_from_slice(&payload);

        let web_file = SchemeReader::read(&input).unwrap();
        assert_eq!(web_file.kind(), FileKind::UnityWebData);
        let entry = web_file.dispatch_entry(0).unwrap();
        assert_eq!(entry.bytes(), b"embedded asset");
        assert_eq!(entry.wrappers()[0].kind, WrapperKind::Gzip);
        assert!(matches!(
            entry.parse().unwrap(),
            super::ParsedFile::Resource(_)
        ));
    }

    #[test]
    fn mounts_webdata_entries_with_parent_directories() {
        let input = webdata_fixture(&[
            ("nested/asset.bin", b"embedded asset"),
            ("other.bin", b"second"),
        ]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = file
            .mount_container_entries(
                &mut filesystem,
                Some("/bundles/sample"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.mounted_entries, 2);
        assert_eq!(report.mounted_bytes, 20);
        assert_eq!(filesystem.count(), 6);
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/sample/nested/asset.bin", 64)
                .unwrap(),
            b"embedded asset"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/sample/other.bin", 64)
                .unwrap(),
            b"second"
        );
    }

    #[test]
    fn container_mount_limits_fail_before_changing_the_virtual_filesystem() {
        let input = webdata_fixture(&[("first.bin", b"first"), ("second.bin", b"second")]);
        let file = SchemeReader::read(&input).unwrap();

        let mut count_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_entries(
                &mut count_limited,
                Some("/bundle"),
                ContainerMountLimits {
                    max_entries: 1,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::EntryCountLimitExceeded { count: 2, limit: 1 }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert_eq!(count_limited.count(), 1);

        let mut byte_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_entries(
                &mut byte_limited,
                Some("/bundle"),
                ContainerMountLimits {
                    max_total_output_bytes: 5,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::TotalOutputLimitExceeded { .. }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert_eq!(byte_limited.count(), 1);
    }

    #[test]
    fn container_mount_reports_partial_progress_on_path_collision() {
        let input = webdata_fixture(&[("new.bin", b"new"), ("existing.bin", b"incoming")]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/mount")).unwrap();
        filesystem
            .write_all_bytes("/mount/existing.bin", b"original")
            .unwrap();

        let error = file
            .mount_container_entries(
                &mut filesystem,
                Some("/mount"),
                ContainerMountLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::FileSystem { entry_index: 1, .. }
        ));
        assert_eq!(error.partial_report.mounted_entries, 1);
        assert_eq!(error.partial_report.mounted_bytes, 3);
        assert_eq!(
            filesystem.read_all_bytes("/mount/new.bin", 32).unwrap(),
            b"new"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/mount/existing.bin", 32)
                .unwrap(),
            b"original"
        );
    }

    #[test]
    fn container_mount_rejects_parent_directory_paths() {
        let input = webdata_fixture(&[("../escape.bin", b"data")]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_entries(
                &mut filesystem,
                Some("/bundle"),
                ContainerMountLimits::default(),
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::InvalidEntryPath { entry_index: 0, .. }
        ));
        assert_eq!(filesystem.count(), 1);
    }

    #[test]
    fn recursively_mounts_nested_and_wrapper_containers_under_their_entry_paths() {
        let nested = webdata_fixture(&[("leaf.bin", b"nested leaf")]);
        let wrapped_nested = compress_gzip(&nested, 4096).unwrap();
        let input = webdata_fixture(&[
            ("plain-container", &nested),
            ("wrapped-container.gz", &wrapped_nested),
        ]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.mounted_entries, 2);
        assert_eq!(report.mounted_bytes, 22);
        assert_eq!(
            report.container_scopes,
            [
                "/tree",
                "/tree/plain-container",
                "/tree/wrapped-container.gz"
            ]
        );
        assert_eq!(
            report.container_bundles,
            [
                MountedContainerBundle {
                    scope_index: Some(0),
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    name_fixed: None,
                },
                MountedContainerBundle {
                    scope_index: Some(1),
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(0),
                    name_fixed: Some("plain-container".to_owned()),
                },
                MountedContainerBundle {
                    scope_index: Some(2),
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(1),
                    name_fixed: Some("wrapped-container.gz".to_owned()),
                },
            ]
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/tree/plain-container/leaf.bin", 64)
                .unwrap(),
            b"nested leaf"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/tree/wrapped-container.gz/leaf.bin", 64)
                .unwrap(),
            b"nested leaf"
        );
    }

    #[test]
    fn recursively_mounted_serialized_file_records_preserve_each_container_entry_order() {
        let direct_first = serialized_file_header_fixture();
        let nested_leaf = serialized_file_header_fixture();
        let direct_second = serialized_file_header_fixture();
        let nested = webdata_fixture(&[("inside.asset", &nested_leaf)]);
        let input = webdata_fixture(&[
            ("z-first.asset", &direct_first),
            ("nested.bundle", &nested),
            ("a-second.asset", &direct_second),
        ]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.container_scopes, ["/tree", "/tree/nested.bundle"]);
        assert_eq!(
            report.container_bundles,
            [
                MountedContainerBundle {
                    scope_index: Some(0),
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    name_fixed: None,
                },
                MountedContainerBundle {
                    scope_index: Some(1),
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(1),
                    name_fixed: Some("nested.bundle".to_owned()),
                },
            ]
        );
        assert_eq!(
            report.serialized_files,
            [
                MountedSerializedFile {
                    virtual_path: "/tree/z-first.asset".to_owned(),
                    container_scope_index: Some(0),
                    container_bundle_index: Some(0),
                    entry_index: 0,
                    collection_index: Some(0),
                },
                MountedSerializedFile {
                    virtual_path: "/tree/nested.bundle/inside.asset".to_owned(),
                    container_scope_index: Some(1),
                    container_bundle_index: Some(1),
                    entry_index: 0,
                    collection_index: Some(0),
                },
                MountedSerializedFile {
                    virtual_path: "/tree/a-second.asset".to_owned(),
                    container_scope_index: Some(0),
                    container_bundle_index: Some(0),
                    entry_index: 2,
                    collection_index: Some(1),
                },
            ]
        );
        let root_collection_order: Vec<_> = report
            .serialized_files
            .iter()
            .filter(|file| file.container_scope_index == Some(0))
            .map(|file| file.virtual_path.as_str())
            .collect();
        assert_eq!(
            root_collection_order,
            ["/tree/z-first.asset", "/tree/a-second.asset"]
        );
    }

    #[test]
    fn recursive_container_mount_retains_fixed_source_bundle_names() {
        let nested = webdata_fixture(&[("leaf.asset", b"leaf")]);
        let input = webdata_fixture(&[("LIBRARY/Nested.Bundle", &nested)]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = file
            .mount_container_tree_with_source_name(
                &mut filesystem,
                Some("/mounted/root.bundle"),
                Some("Library/Root.Bundle"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(
            report
                .container_bundles
                .iter()
                .map(|bundle| bundle.name_fixed.as_deref())
                .collect::<Vec<_>>(),
            [Some("root.bundle"), Some("nested.bundle")]
        );
        assert_eq!(report.container_bundle_name_bytes, 24);
    }

    #[test]
    fn source_bundle_names_obey_their_aggregate_mount_bound() {
        let input = webdata_fixture(&[("leaf.asset", b"leaf")]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_tree_with_source_name(
                &mut filesystem,
                Some("/mounted"),
                Some("root.bundle"),
                ContainerMountLimits {
                    max_total_container_bundle_name_bytes: 4,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::ContainerBundleNameLimitExceeded {
                requested: 11,
                limit: 4
            }
        ));
        assert_eq!(error.partial_report.container_bundle_name_bytes, 0);
        assert_eq!(filesystem.count(), 1);
    }

    #[test]
    fn recursively_mounted_resource_records_preserve_each_container_entry_order() {
        let nested = webdata_fixture(&[("nested.resS", b"nested resource")]);
        let input = webdata_fixture(&[
            ("z-first.resource", b"first resource"),
            ("nested.bundle", &nested),
            ("a-second.ress", b"second resource"),
            ("archive:/textures/shared.ress", b"archive resource"),
        ]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(
            report.resource_files,
            [
                MountedResourceFile {
                    virtual_path: "/tree/z-first.resource".to_owned(),
                    name: "z-first.resource".to_owned(),
                    container_scope_index: Some(0),
                    container_bundle_index: 0,
                    entry_index: 0,
                    resource_index: 0,
                },
                MountedResourceFile {
                    virtual_path: "/tree/nested.bundle/nested.resS".to_owned(),
                    name: "nested.resS".to_owned(),
                    container_scope_index: Some(1),
                    container_bundle_index: 1,
                    entry_index: 0,
                    resource_index: 0,
                },
                MountedResourceFile {
                    virtual_path: "/tree/a-second.ress".to_owned(),
                    name: "a-second.ress".to_owned(),
                    container_scope_index: Some(0),
                    container_bundle_index: 0,
                    entry_index: 2,
                    resource_index: 1,
                },
                MountedResourceFile {
                    virtual_path: "/tree/archive:/textures/shared.ress".to_owned(),
                    name: "archive:/textures/shared.ress".to_owned(),
                    container_scope_index: Some(0),
                    container_bundle_index: 0,
                    entry_index: 3,
                    resource_index: 2,
                },
            ]
        );
        assert_eq!(
            report.resource_file_path_bytes,
            u64::try_from(
                "/tree/z-first.resource".len()
                    + "/tree/nested.bundle/nested.resS".len()
                    + "/tree/a-second.ress".len()
                    + "/tree/archive:/textures/shared.ress".len(),
            )
            .unwrap()
        );
        assert_eq!(
            report.resource_file_name_bytes,
            u64::try_from(
                "z-first.resource".len()
                    + "nested.resS".len()
                    + "a-second.ress".len()
                    + "archive:/textures/shared.ress".len(),
            )
            .unwrap()
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/tree/nested.bundle/nested.resS", 64)
                .unwrap(),
            b"nested resource"
        );
    }

    #[test]
    fn resource_file_order_metadata_obeys_aggregate_path_bound() {
        let input = webdata_fixture(&[("long.bin", b"resource")]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits {
                    max_total_resource_file_path_bytes: 4,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::ResourceFilePathLimitExceeded {
                requested: 14,
                limit: 4
            }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert!(error.partial_report.resource_files.is_empty());
        assert_eq!(error.partial_report.resource_file_path_bytes, 0);
    }

    #[test]
    fn original_resource_file_names_obey_their_aggregate_bound() {
        let input = webdata_fixture(&[("long.bin", b"resource")]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits {
                    max_total_resource_file_name_bytes: 4,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::ResourceFileNameLimitExceeded {
                requested: 8,
                limit: 4
            }
        ));
        assert!(error.partial_report.resource_files.is_empty());
        assert_eq!(error.partial_report.resource_file_name_bytes, 0);
    }

    #[test]
    fn serialized_file_order_metadata_obeys_aggregate_path_bound() {
        let serialized_file = serialized_file_header_fixture();
        let input = webdata_fixture(&[("leaf.asset", &serialized_file)]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits {
                    max_total_serialized_file_path_bytes: 4,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::SerializedFilePathLimitExceeded {
                requested: 16,
                limit: 4
            }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert!(error.partial_report.serialized_files.is_empty());
    }

    #[test]
    fn mount_path_imports_a_root_resource_under_its_logical_filename() {
        let path = unique_temp_file_path("bin");
        std::fs::write(&path, b"standalone resource").unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_path(
            &path,
            &mut filesystem,
            Some("/inputs"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(report.mounted_bytes, 19);
        assert_eq!(
            filesystem
                .read_all_bytes(
                    &format!("/inputs/{}", path.file_name().unwrap().to_string_lossy()),
                    64,
                )
                .unwrap(),
            b"standalone resource"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn mount_path_assembles_split_inputs_and_uses_the_logical_base_name() {
        let base_path = unique_temp_file_path("bin");
        let first_part = PathBuf::from(format!("{}.split0", base_path.display()));
        let second_part = PathBuf::from(format!("{}.split1", base_path.display()));
        std::fs::write(&first_part, b"split ").unwrap();
        std::fs::write(&second_part, b"resource").unwrap();
        let supplied_part = PathBuf::from(format!("{}.split1", base_path.display()));
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_path(
            &supplied_part,
            &mut filesystem,
            Some("/inputs"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(
            filesystem
                .read_all_bytes(
                    &format!(
                        "/inputs/{}",
                        base_path.file_name().unwrap().to_string_lossy()
                    ),
                    64,
                )
                .unwrap(),
            b"split resource"
        );
        std::fs::remove_file(first_part).unwrap();
        std::fs::remove_file(second_part).unwrap();
    }

    #[test]
    fn mount_path_connects_root_webdata_to_recursive_virtual_mounting() {
        let path = unique_temp_file_path("webdata");
        let input = webdata_fixture(&[("assets/leaf.bin", b"root WebData leaf")]);
        std::fs::write(&path, input).unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_path(
            &path,
            &mut filesystem,
            Some("/bundles/root"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/root/assets/leaf.bin", 64)
                .unwrap(),
            b"root WebData leaf"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn mount_directory_preserves_paths_and_assembles_split_groups() {
        let root = unique_temp_file_path("directory");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir_all(root.join("parts")).unwrap();
        std::fs::write(root.join("a.bin"), b"alpha").unwrap();
        std::fs::write(root.join("nested/z.bin"), b"nested").unwrap();
        std::fs::write(root.join("parts/joined.split0"), b"split ").unwrap();
        std::fs::write(root.join("parts/joined.split1"), b"part").unwrap();
        let container = webdata_fixture(&[("inner/asset.dat", b"bundle leaf")]);
        std::fs::write(root.join("bundle.dat"), container).unwrap();
        let second_container = webdata_fixture(&[("inner/asset.dat", b"other bundle")]);
        std::fs::write(root.join("second.bundle"), second_container).unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_directory(
            &root,
            &mut filesystem,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.discovered_inputs, 5);
        assert_eq!(report.completed_inputs, 5);
        assert_eq!(report.mounted_entries, 5);
        assert_eq!(report.mounted_bytes, 5 + 6 + 10 + 11 + 12);
        assert_eq!(
            filesystem.read_all_bytes("/batch/a.bin", 32).unwrap(),
            b"alpha"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/batch/nested/z.bin", 32)
                .unwrap(),
            b"nested"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/batch/parts/joined", 32)
                .unwrap(),
            b"split part"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/batch/bundle.dat/inner/asset.dat", 32)
                .unwrap(),
            b"bundle leaf"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/batch/second.bundle/inner/asset.dat", 32)
                .unwrap(),
            b"other bundle"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mount_directory_preserving_containers_keeps_split_container_identity() {
        let root = unique_temp_file_path("preserve-container");
        std::fs::create_dir_all(&root).unwrap();
        let container = webdata_fixture(&[("inner/asset.dat", b"bundle leaf")]);
        let split = container.len() / 2;
        std::fs::write(root.join("data.unity3d.split0"), &container[..split]).unwrap();
        std::fs::write(root.join("data.unity3d.split1"), &container[split..]).unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_directory_preserving_containers(
            &root,
            &mut filesystem,
            Some("/platform/Game_Data"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.discovered_inputs, 1);
        assert_eq!(report.completed_inputs, 1);
        assert_eq!(report.mounted_entries, 1);
        assert_eq!(
            report.mounted_bytes,
            u64::try_from(container.len()).unwrap()
        );
        assert!(filesystem
            .file_exists(Some("/platform/Game_Data/data.unity3d"))
            .unwrap());
        assert!(!filesystem
            .directory_exists(Some("/platform/Game_Data/data.unity3d"))
            .unwrap());
        let preserved = filesystem
            .read_all_bytes("/platform/Game_Data/data.unity3d", container.len())
            .unwrap();
        assert_eq!(preserved, container);
        assert_eq!(
            SchemeReader::read(&preserved).unwrap().kind(),
            FileKind::UnityWebData
        );

        std::fs::remove_file(root.join("data.unity3d.split0")).unwrap();
        std::fs::remove_file(root.join("data.unity3d.split1")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn mount_directory_enforces_aggregate_input_limit_before_mutation() {
        let root = unique_temp_file_path("input-limit");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("first.bin"), b"first").unwrap();
        std::fs::write(root.join("second.bin"), b"second").unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let error = SchemeReader::mount_directory(
            &root,
            &mut filesystem,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_total_input_bytes: 10,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();

        assert!(matches!(
            error.kind,
            BatchMountErrorKind::TotalInputLimitExceeded {
                requested_total: 11,
                limit: 10
            }
        ));
        assert_eq!(error.partial_report.completed_inputs, 0);
        assert_eq!(filesystem.count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mount_directory_applies_output_limit_across_inputs_and_reports_partial_progress() {
        let root = unique_temp_file_path("output-limit");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.bin"), b"1234").unwrap();
        std::fs::write(root.join("b.bin"), b"56").unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let error = SchemeReader::mount_directory(
            &root,
            &mut filesystem,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_total_output_bytes: 4,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();

        assert!(matches!(
            error.kind,
            BatchMountErrorKind::TotalOutputLimitExceeded {
                requested_total: 6,
                limit: 4
            }
        ));
        assert_eq!(error.partial_report.completed_inputs, 1);
        assert_eq!(error.partial_report.mounted_entries, 1);
        assert_eq!(error.partial_report.mounted_bytes, 4);
        assert_eq!(
            filesystem.read_all_bytes("/batch/a.bin", 8).unwrap(),
            b"1234"
        );
        assert!(!filesystem.file_exists(Some("/batch/b.bin")).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mount_directory_rejects_regular_and_split_inputs_with_same_logical_path() {
        let root = unique_temp_file_path("duplicate-input");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("same.bin"), b"regular").unwrap();
        std::fs::write(root.join("same.bin.split0"), b"split").unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let error = SchemeReader::mount_directory(
            &root,
            &mut filesystem,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap_err();

        assert!(matches!(
            error.kind,
            BatchMountErrorKind::DirectoryDiscovery(_)
        ));
        assert_eq!(filesystem.count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mount_directory_enforces_input_count_and_depth_limits() {
        let root = unique_temp_file_path("directory-bounds");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("a.bin"), b"root").unwrap();
        std::fs::write(root.join("nested/b.bin"), b"nested").unwrap();

        let mut input_limited = VirtualFileSystem::new();
        let error = SchemeReader::mount_directory(
            &root,
            &mut input_limited,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_inputs: 1,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            BatchMountErrorKind::DirectoryDiscovery(_)
        ));
        assert_eq!(input_limited.count(), 1);

        let mut directory_entry_limited = VirtualFileSystem::new();
        let error = SchemeReader::mount_directory(
            &root,
            &mut directory_entry_limited,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_directory_entries: 1,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            BatchMountErrorKind::DirectoryDiscovery(_)
        ));
        assert_eq!(directory_entry_limited.count(), 1);

        let mut depth_limited = VirtualFileSystem::new();
        let error = SchemeReader::mount_directory(
            &root,
            &mut depth_limited,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_directory_depth: 0,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            BatchMountErrorKind::DirectoryDiscovery(_)
        ));
        assert_eq!(depth_limited.count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mount_paths_imports_explicit_roots_and_honors_each_destination() {
        let resource_path = unique_temp_file_path("explicit-resource");
        let container_path = unique_temp_file_path("explicit-container");
        std::fs::write(&resource_path, b"first").unwrap();
        std::fs::write(&container_path, webdata_fixture(&[("leaf.bin", b"leaf")])).unwrap();
        let inputs = [
            PathMountInput {
                path: resource_path.clone(),
                destination_root: Some("/explicit".to_owned()),
            },
            PathMountInput {
                path: container_path.clone(),
                destination_root: Some("/explicit/container".to_owned()),
            },
        ];
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_paths(
            &inputs,
            &mut filesystem,
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.discovered_inputs, 2);
        assert_eq!(report.completed_inputs, 2);
        assert_eq!(report.mounted_entries, 2);
        assert_eq!(report.mounted_bytes, 9);
        let resource_virtual_path = format!(
            "/explicit/{}",
            resource_path.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(
            report.explicit_root_resource_files,
            Box::new(vec![MountedExplicitResourceFile {
                file_name: resource_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                virtual_path: resource_virtual_path.clone(),
                kind: FileKind::Resource,
            }])
        );
        assert_eq!(
            filesystem
                .read_all_bytes(&resource_virtual_path, 16)
                .unwrap(),
            b"first"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/explicit/container/leaf.bin", 16)
                .unwrap(),
            b"leaf"
        );
        std::fs::remove_file(resource_path).unwrap();
        std::fs::remove_file(container_path).unwrap();
    }

    #[test]
    fn mount_paths_preflights_input_size_and_rejects_duplicate_split_groups() {
        let resource_path = unique_temp_file_path("explicit-limit");
        std::fs::write(&resource_path, b"resource").unwrap();
        let inputs = [PathMountInput {
            path: resource_path.clone(),
            destination_root: Some("/explicit".to_owned()),
        }];
        let mut filesystem = VirtualFileSystem::new();

        let error = SchemeReader::mount_paths(
            &inputs,
            &mut filesystem,
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_total_input_bytes: 4,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            BatchMountErrorKind::TotalInputLimitExceeded {
                requested_total: 8,
                limit: 4
            }
        ));
        assert_eq!(filesystem.count(), 1);

        let split_base = unique_temp_file_path("explicit-split");
        let split0 = PathBuf::from(format!("{}.split0", split_base.display()));
        let split1 = PathBuf::from(format!("{}.split1", split_base.display()));
        std::fs::write(&split0, b"split ").unwrap();
        std::fs::write(&split1, b"group").unwrap();
        let split_inputs = [
            PathMountInput {
                path: split0.clone(),
                destination_root: Some("/explicit".to_owned()),
            },
            PathMountInput {
                path: split1.clone(),
                destination_root: Some("/explicit".to_owned()),
            },
        ];
        let error = SchemeReader::mount_paths(
            &split_inputs,
            &mut filesystem,
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            BatchMountErrorKind::DuplicateInputPath
        ));
        assert_eq!(filesystem.count(), 1);

        std::fs::remove_file(resource_path).unwrap();
        std::fs::remove_file(split0).unwrap();
        std::fs::remove_file(split1).unwrap();
    }

    #[test]
    fn recursive_mount_applies_global_count_depth_and_output_limits() {
        let nested = webdata_fixture(&[("first.bin", b"first"), ("second.bin", b"second")]);
        let input = webdata_fixture(&[("nested", &nested)]);
        let file = SchemeReader::read(&input).unwrap();

        let mut count_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut count_limited,
                Some("/tree"),
                ContainerMountLimits {
                    max_entries: 2,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::EntryCountLimitExceeded { count: 3, limit: 2 }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert_eq!(count_limited.count(), 1);

        let mut scope_path_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut scope_path_limited,
                Some("/tree"),
                ContainerMountLimits {
                    max_total_container_scope_path_bytes: 4,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::ContainerScopePathLimitExceeded {
                requested: 5,
                limit: 4
            }
        ));
        assert!(error.partial_report.container_scopes.is_empty());
        assert_eq!(scope_path_limited.count(), 1);

        let mut depth_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut depth_limited,
                Some("/tree"),
                ContainerMountLimits {
                    max_depth: 0,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::NestingDepthLimitExceeded { depth: 1, limit: 0 }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert_eq!(depth_limited.count(), 1);

        let wrapped_nested = compress_gzip(&nested, 4096).unwrap();
        let wrapped_input = webdata_fixture(&[("nested.gz", &wrapped_nested)]);
        let wrapped_file = SchemeReader::read(&wrapped_input).unwrap();
        let mut active_byte_limited = VirtualFileSystem::new();
        let error = wrapped_file
            .mount_container_tree(
                &mut active_byte_limited,
                Some("/tree"),
                ContainerMountLimits {
                    max_active_nested_bytes: 0,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::ActiveNestedContainerLimitExceeded {
                entry_index: 0,
                limit: 0,
                ..
            }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert_eq!(active_byte_limited.count(), 1);

        let mut output_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut output_limited,
                Some("/tree"),
                ContainerMountLimits {
                    max_total_output_bytes: 5,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::TotalOutputLimitExceeded { .. }
        ));
        assert_eq!(error.partial_report.mounted_entries, 1);
        assert_eq!(error.partial_report.mounted_bytes, 5);
        assert_eq!(
            output_limited
                .read_all_bytes("/tree/nested/first.bin", 32)
                .unwrap(),
            b"first"
        );
        assert!(!output_limited
            .file_exists(Some("/tree/nested/second.bin"))
            .unwrap());
    }

    #[test]
    fn dispatches_a_spec_encoded_unity_brotli_metadata_wrapper() {
        for window_bits in 10..=24 {
            let input = unity_marked_brotli_fixture(b"webdata payload", window_bits);
            assert!(has_brotli_file_signature(&input), "WBITS={window_bits}");
            assert_eq!(
                decompress_brotli(&input, 1024).unwrap(),
                b"webdata payload",
                "WBITS={window_bits}"
            );
        }

        let input = unity_marked_brotli_fixture(b"webdata payload", 18);
        let file = SchemeReader::read(&input).unwrap();
        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"webdata payload");
        assert_eq!(file.wrappers()[0].kind, WrapperKind::Brotli);
    }

    #[test]
    fn readability_checks_signatures_without_decoding() {
        assert!(SchemeReader::is_readable(b"\x1f\x8b"));
        assert!(!SchemeReader::is_readable(b"arbitrary resource"));
        let mut archive = vec![0; 32];
        archive[..12].copy_from_slice(b"UnityArchive");
        assert!(SchemeReader::is_readable(&archive));
    }

    #[test]
    fn enforces_recursive_and_cumulative_wrapper_limits() {
        let once = compress_gzip(b"data", 1024).unwrap();
        let twice = compress_gzip(&once, 1024).unwrap();

        let depth_error = SchemeReader::read_with_limits(
            &twice,
            SchemeReaderLimits {
                max_wrapper_depth: 1,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            depth_error.kind(),
            SchemeErrorKind::WrapperDepthExceeded { limit: 1 }
        ));

        let total_error = SchemeReader::read_with_limits(
            &once,
            SchemeReaderLimits {
                max_total_decompressed_bytes: 2,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            total_error.kind(),
            SchemeErrorKind::TotalDecompressionLimitExceeded { limit: 2, .. }
        ));
    }

    #[test]
    fn follows_the_source_bundle_and_webdata_detection_order() {
        for (magic, expected) in [
            (b"UnityFS".as_slice(), FileKind::UnityFs),
            (b"UnityRaw".as_slice(), FileKind::UnityRaw),
            (b"UnityWeb".as_slice(), FileKind::UnityWebBundle),
            (b"UnityArchive".as_slice(), FileKind::UnityArchive),
        ] {
            let mut bytes = vec![0; 32];
            bytes[..magic.len()].copy_from_slice(magic);
            assert_eq!(SchemeReader::read(&bytes).unwrap().kind(), expected);
        }

        let web_data = b"UnityWebData1.0\0\x14\0\0\0";
        assert_eq!(
            SchemeReader::read(web_data).unwrap().kind(),
            FileKind::UnityWebData
        );
    }

    #[test]
    fn container_kinds_follow_the_source_bundle_and_leaf_partition() {
        for kind in [
            FileKind::UnityFs,
            FileKind::UnityRaw,
            FileKind::UnityWebBundle,
            FileKind::UnityWebData,
            FileKind::UnityArchive,
        ] {
            assert!(
                super::is_container_kind(kind),
                "{kind:?} expands into mounted entries"
            );
        }
        for kind in [FileKind::SerializedFile, FileKind::Resource] {
            assert!(
                !super::is_container_kind(kind),
                "{kind:?} mounts as a single leaf"
            );
        }
    }

    #[test]
    fn archive_inputs_take_the_container_mount_branch() {
        let mut bytes = vec![0; 32];
        bytes[..12].copy_from_slice(b"UnityArchive");
        let file = SchemeReader::read(&bytes).unwrap();
        assert_eq!(file.kind(), FileKind::UnityArchive);
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits::default(),
            )
            .unwrap_err();
        // The container branch parses the bundle: only the unsupported-reader error
        // surfaces, never a leaf mount or a not-a-container rejection.
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::Scheme(error)
            if matches!(error.kind(), SchemeErrorKind::ArchiveReadUnsupported)
        ));
        assert_eq!(filesystem.count(), 1);
    }

    #[test]
    fn reports_the_baseline_archive_reader_as_explicitly_unsupported() {
        let mut bytes = vec![0; 32];
        bytes[..12].copy_from_slice(b"UnityArchive");
        let file = SchemeReader::read(&bytes).unwrap();
        assert_eq!(file.kind(), FileKind::UnityArchive);
        assert!(matches!(
            file.parse().unwrap_err().kind(),
            SchemeErrorKind::ArchiveReadUnsupported
        ));
    }

    #[test]
    fn recognizes_a_source_generation_22_serialized_file_header() {
        let mut bytes = [0; 61];
        bytes[8..12].copy_from_slice(&22i32.to_be_bytes());
        bytes[20..24].copy_from_slice(&13i32.to_be_bytes());
        bytes[24..32].copy_from_slice(&61u64.to_be_bytes());
        assert_eq!(
            SchemeReader::read(&bytes).unwrap().kind(),
            FileKind::SerializedFile
        );
    }

    #[test]
    fn raw_header_probe_classifies_without_reading_the_file_body() {
        let mut bytes = [0; 61];
        bytes[8..12].copy_from_slice(&22i32.to_be_bytes());
        bytes[20..24].copy_from_slice(&13i32.to_be_bytes());
        bytes[24..32].copy_from_slice(&61u64.to_be_bytes());

        assert_eq!(
            SchemeReader::probe_raw_kind(&bytes[..32], bytes.len() as u64),
            FileKind::SerializedFile
        );
        assert_eq!(
            SchemeReader::probe_raw_kind(&bytes[..32], 62),
            FileKind::Resource
        );

        let mut bundle_header = vec![0; 32];
        bundle_header[..7].copy_from_slice(b"UnityFS");
        assert_eq!(
            SchemeReader::probe_raw_kind(&bundle_header, 64),
            FileKind::UnityFs
        );
    }

    #[test]
    fn detection_edges_follow_the_source_minimums_and_readability() {
        // Empty input is an unknown resource, not a scheme.
        let empty = SchemeReader::read(b"").unwrap();
        assert_eq!(empty.kind(), FileKind::Resource);
        assert!(!empty.is_readable());
        assert!(!SchemeReader::is_readable(b""));
        // A bundle magic below the 0x20-byte source minimum does not dispatch.
        let short = b"UnityFS\0".to_vec();
        let short_file = SchemeReader::read(&short).unwrap();
        assert_eq!(short_file.kind(), FileKind::Resource);
        assert!(!SchemeReader::is_readable(&short));
        assert_eq!(
            SchemeReader::probe_raw_kind(b"short", 100),
            FileKind::Resource
        );
        // A wrapped resource counts as scheme-readable; the bare form does not.
        let wrapped = compress_gzip(b"payload", 1024).unwrap();
        let wrapped_file = SchemeReader::read(&wrapped).unwrap();
        assert_eq!(wrapped_file.kind(), FileKind::Resource);
        assert!(wrapped_file.is_readable());
        assert!(SchemeReader::is_readable(&wrapped));
    }
}
