//! Source-compatible detection and recursive decoding of top-level file schemes.

use std::borrow::Cow;
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
use crate::io::filesystem::VirtualFileSystem;
use crate::io::stream::{logical_file_name, open_logical_file, SmartStream};
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
}

impl Default for ContainerMountLimits {
    fn default() -> Self {
        Self {
            max_entries: 1_000_000,
            max_total_output_bytes: 2 * 1024 * 1024 * 1024,
            max_depth: 64,
            max_active_nested_bytes: 1024 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerMountReport {
    /// Number of files inserted. Recursive mounting excludes nested container payloads.
    pub mounted_entries: usize,
    /// Total bytes in files inserted into the virtual filesystem.
    pub mounted_bytes: u64,
}

struct ContainerTreeMountState<'a> {
    filesystem: &'a mut VirtualFileSystem,
    limits: ContainerMountLimits,
    visited_entries: usize,
    active_nested_bytes: u64,
    report: ContainerMountReport,
}

#[derive(Debug)]
pub struct ContainerMountError {
    pub partial_report: ContainerMountReport,
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
        let mut state = ContainerTreeMountState {
            filesystem,
            limits,
            visited_entries: 0,
            active_nested_bytes: 0,
            report: ContainerMountReport::default(),
        };
        self.mount_container_tree_at(destination_root, 0, &mut state)?;
        Ok(state.report)
    }

    fn mount_container_tree_at(
        &self,
        destination_root: Option<&str>,
        depth: usize,
        state: &mut ContainerTreeMountState<'_>,
    ) -> Result<(), ContainerMountError> {
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
                    mount_tree_child(child, path, index, depth, state)?;
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
                    mount_tree_child(child, path, index, depth, state)?;
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
                    mount_tree_child(child, path, index, depth, state)?;
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
            partial_report,
            kind,
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

fn mount_tree_child<'a>(
    child: DispatchedFile<'a>,
    entry_path: String,
    entry_index: usize,
    parent_depth: usize,
    state: &mut ContainerTreeMountState<'_>,
) -> Result<(), ContainerMountError> {
    match child.kind {
        FileKind::UnityFs
        | FileKind::UnityRaw
        | FileKind::UnityWebBundle
        | FileKind::UnityWebData
        | FileKind::UnityArchive => {
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
            let result = child.mount_container_tree_at(Some(&entry_path), depth, state);
            state.active_nested_bytes -= child_owned_bytes;
            result
        }
        FileKind::SerializedFile | FileKind::Resource => mount_container_entry_at_path(
            state.filesystem,
            &entry_path,
            entry_index,
            child.contents,
            state.limits,
            &mut state.report,
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContainerEntryPathError {
    Invalid,
    TooLong,
    AllocationFailed { requested: usize },
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

        match dispatched.kind {
            FileKind::UnityFs
            | FileKind::UnityRaw
            | FileKind::UnityWebBundle
            | FileKind::UnityWebData
            | FileKind::UnityArchive => {
                dispatched.mount_container_tree(filesystem, destination_root, mount_limits)
            }
            FileKind::SerializedFile | FileKind::Resource => {
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
                let virtual_path = checked_container_entry_path(
                    filesystem,
                    destination_root,
                    &file_name,
                    0,
                    &report,
                )?;
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
    if let Some(kind) = detect_primary_file_kind(input) {
        return kind;
    }
    if is_serialized_file(input) {
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

    metadata_size >= 13 && file_size >= 16 + 13 && u64::try_from(input.len()) == Ok(file_size)
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
        ContainerMountErrorKind, ContainerMountLimits, FileKind, SchemeErrorKind, SchemeReader,
        SchemeReaderLimits, WrapperKind,
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
        let mut bytes = vec![0; 61];
        bytes[8..12].copy_from_slice(&22i32.to_be_bytes());
        bytes[20..24].copy_from_slice(&13i32.to_be_bytes());
        bytes[24..32].copy_from_slice(&61u64.to_be_bytes());
        assert_eq!(
            SchemeReader::read(&bytes).unwrap().kind(),
            FileKind::SerializedFile
        );
    }
}
