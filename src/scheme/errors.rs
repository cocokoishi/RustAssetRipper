use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use super::*;
use crate::compression::wrappers::WrapperCodecError;
use crate::container::raw_web::{RawWebError, RawWebKind};
use crate::container::unityfs::UnityFsError;
use crate::container::web_file::WebFileError;
use crate::io::discovery::DirectoryDiscoveryError;
use crate::serialized_file::file::SerializedFileError;

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

impl ContainerMountError {
    pub(super) fn new(partial_report: ContainerMountReport, kind: ContainerMountErrorKind) -> Self {
        Self {
            partial_report: Box::new(partial_report),
            kind,
        }
    }
}

impl BatchMountError {
    pub(super) fn new(
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

#[derive(Debug)]
pub struct SchemeError {
    kind: SchemeErrorKind,
}

impl SchemeError {
    pub(super) fn new(kind: SchemeErrorKind) -> Self {
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
            SchemeErrorKind::StreamOutput(error) => Some(error),
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
    StreamOutput(std::io::Error),
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
            Self::StreamOutput(error) => write!(formatter, "temporary output stream: {error}"),
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
