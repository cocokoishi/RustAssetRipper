//! Bounded access to byte ranges stored in Unity ResourceFiles.
//!
//! Covers `ResourceFile` (`IsDefaultResourceFile` plus the streamed-range read surface behind
//! `Write`/`ToByteArray`), the export-time `StreamingInfo` value operations (`ClearValues`/
//! `SetValues`, used by `YamlStreamedAssetExportCollection` around inline exports),
//! `Bundle.ResolveResource` (bundle-hierarchy order, then the external provider), and
//! `StreamedResourceExtensions` (`CheckIntegrity`/`GetContent` gates). Bundle construction,
//! stream ownership, the export save/clear/restore orchestration itself, and the parent-chain
//! walk stay caller-side; the hierarchy lookup itself lives in the serialized-file workspace.

use std::fmt;
use std::fmt::Write as FmtWrite;
use std::io;

use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::{
    PlatformResourceLookup, PlatformResourceLookupError, PlatformResourceProvider,
    PlatformResourceSource, PlatformStructureLimits,
};
use crate::io::stream::SmartStream;
use crate::scheme::{DispatchedFile, FileKind, MountedExplicitResourceFile};
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};
use crate::serialized_file::workspace::{
    SerializedFileWorkspace, WorkspaceResolvedResourceFile, WorkspaceResourceLookupError,
    WorkspaceRootResourceFile,
};

const UNITY_STREAM_OFFSET_MAX: u64 = i64::MAX as u64;

/// `ResourceFile.ResourceFileExtension`.
pub const RESOURCE_FILE_EXTENSION: &str = ".resource";

/// `ResourceFile.StreamingFileExtension`.
pub const STREAMING_FILE_EXTENSION: &str = ".ress";

/// `ResourceFile.IsDefaultResourceFile`: the name's extension, lowercased like the source,
/// is `.resource` or `.ress`.
pub fn is_default_resource_file(file_name: &str) -> bool {
    let extension = file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or("");
    extension.eq_ignore_ascii_case("resource") || extension.eq_ignore_ascii_case("ress")
}

/// Limits for streamed ResourceFile reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamedResourceLimits {
    /// Maximum range copied when the source stream cannot provide a zero-copy file range.
    pub max_range_copy_bytes: usize,
    /// Maximum byte count returned by [`read_streamed_resource_bytes`].
    pub max_materialized_bytes: usize,
}

/// Work and diagnostic-path bounds for finding nested StreamingInfo values in one decoded object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamedResourceScanLimits {
    pub max_value_nodes: usize,
    pub max_candidates: usize,
    pub max_resource_path_bytes: usize,
    pub max_total_resource_path_bytes: usize,
    pub max_single_field_path_bytes: usize,
    pub max_total_field_path_bytes: usize,
}

impl Default for StreamedResourceScanLimits {
    fn default() -> Self {
        Self {
            max_value_nodes: 10_000_000,
            max_candidates: 1_000_000,
            max_resource_path_bytes: 1024 * 1024,
            max_total_resource_path_bytes: 64 * 1024 * 1024,
            max_single_field_path_bytes: 1024 * 1024,
            max_total_field_path_bytes: 64 * 1024 * 1024,
        }
    }
}

/// A ResourceFile selected from a mounted bundle or an external game DataPath.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedResourceFile<'a> {
    Bundled {
        resource_index: usize,
        virtual_path: &'a str,
    },
    ExplicitRoot {
        resource_index: usize,
        virtual_path: &'a str,
    },
    External {
        virtual_path: String,
        source: PlatformResourceSource,
        data_path_index: usize,
        unavailable_data_paths: Vec<String>,
    },
    Missing {
        unavailable_data_paths: Vec<String>,
    },
}

/// Explicit root inputs must already have been dispatched as Resource files by `SchemeReader`.
pub struct ExplicitRootResourceInput<'a, 'contents> {
    pub file_name: &'a str,
    pub virtual_path: &'a str,
    pub dispatched_file: &'a DispatchedFile<'contents>,
}

/// Bounds for registering explicit root ResourceFiles separately from scanned DataPath files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootResourceCatalogLimits {
    pub max_files: usize,
    pub max_file_name_bytes: usize,
    pub max_virtual_path_bytes: usize,
    pub max_total_file_name_bytes: usize,
    pub max_total_virtual_path_bytes: usize,
}

impl Default for RootResourceCatalogLimits {
    fn default() -> Self {
        Self {
            max_files: 1_000_000,
            max_file_name_bytes: 1024 * 1024,
            max_virtual_path_bytes: 1024 * 1024,
            max_total_file_name_bytes: 64 * 1024 * 1024,
            max_total_virtual_path_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Root GameBundle ResourceFiles from explicitly selected paths, stored in reverse
/// input order. `GameBundle.InitializeFromPaths` pops its file stack from the end, so
/// the last input is added first and wins duplicate names through
/// `Bundle.ResolveResource`'s first match; this catalog keeps that file-stack pop
/// order (verified against the real runtime: `FromPaths([A, B])` resolves B's file).
/// Diagnostic input indexes still number the original input order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RootResourceFileCatalog {
    files: Vec<WorkspaceRootResourceFile>,
}

impl RootResourceFileCatalog {
    /// Builds an ordered catalog from already-dispatched root input files. Platform inventory
    /// candidates must not be passed here; those are searched through `PlatformResourceProvider`.
    pub fn from_explicit_resource_inputs(
        filesystem: &VirtualFileSystem,
        inputs: &[ExplicitRootResourceInput<'_, '_>],
        limits: RootResourceCatalogLimits,
    ) -> Result<Self, RootResourceCatalogError> {
        let mut builder = RootResourceCatalogBuilder::new(filesystem, inputs.len(), limits)?;
        for (input_index, input) in inputs.iter().enumerate() {
            builder.push(
                input_index,
                input.dispatched_file.kind(),
                input.file_name,
                input.virtual_path,
            )?;
        }
        Ok(builder.finish())
    }

    /// Builds from path-list mount metadata. `SchemeReader::mount_paths` records this metadata
    /// only for explicit inputs that it dispatched as `FileKind::Resource`.
    pub fn from_mounted_explicit_resource_files(
        filesystem: &VirtualFileSystem,
        inputs: &[MountedExplicitResourceFile],
        limits: RootResourceCatalogLimits,
    ) -> Result<Self, RootResourceCatalogError> {
        let mut builder = RootResourceCatalogBuilder::new(filesystem, inputs.len(), limits)?;
        for (input_index, input) in inputs.iter().enumerate() {
            builder.push(
                input_index,
                input.kind,
                &input.file_name,
                &input.virtual_path,
            )?;
        }
        Ok(builder.finish())
    }

    pub fn files(&self) -> &[WorkspaceRootResourceFile] {
        &self.files
    }
}

/// Errors while registering explicitly selected root ResourceFiles.
#[derive(Debug)]
pub enum RootResourceCatalogError {
    FileCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    NotResourceFile {
        input_index: usize,
        kind: FileKind,
    },
    EmptyNameOrPath {
        input_index: usize,
    },
    FileNameTooLong {
        input_index: usize,
        byte_count: usize,
        limit: usize,
    },
    VirtualPathTooLong {
        input_index: usize,
        byte_count: usize,
        limit: usize,
    },
    AggregateSizeOverflow {
        resource: &'static str,
    },
    AggregateSizeLimitExceeded {
        resource: &'static str,
        byte_count: usize,
        limit: usize,
    },
    AllocationFailed {
        requested: usize,
    },
    FileNotFound {
        input_index: usize,
        virtual_path: String,
    },
    FileSystem {
        input_index: usize,
        virtual_path: String,
        source: io::Error,
    },
}

impl fmt::Display for RootResourceCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileCountLimitExceeded { count, limit } => write!(
                formatter,
                "explicit root ResourceFile count {count} exceeds limit {limit}"
            ),
            Self::NotResourceFile { input_index, kind } => write!(
                formatter,
                "explicit root input {input_index} dispatched as {kind:?}, not Resource"
            ),
            Self::EmptyNameOrPath { input_index } => write!(
                formatter,
                "explicit root ResourceFile input {input_index} has an empty name or path"
            ),
            Self::FileNameTooLong {
                input_index,
                byte_count,
                limit,
            } => write!(
                formatter,
                "explicit root ResourceFile name at input {input_index} has {byte_count} bytes, exceeding {limit}"
            ),
            Self::VirtualPathTooLong {
                input_index,
                byte_count,
                limit,
            } => write!(
                formatter,
                "explicit root ResourceFile path at input {input_index} has {byte_count} bytes, exceeding {limit}"
            ),
            Self::AggregateSizeOverflow { resource } => {
                write!(formatter, "aggregate byte count for {resource} overflows")
            }
            Self::AggregateSizeLimitExceeded {
                resource,
                byte_count,
                limit,
            } => write!(
                formatter,
                "{resource} use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::AllocationFailed { requested } => write!(
                formatter,
                "could not allocate metadata for {requested} explicit root ResourceFiles"
            ),
            Self::FileNotFound {
                input_index,
                virtual_path,
            } => write!(
                formatter,
                "explicit root ResourceFile input {input_index} is missing at '{virtual_path}'"
            ),
            Self::FileSystem {
                input_index,
                virtual_path,
                source,
            } => write!(
                formatter,
                "could not inspect explicit root ResourceFile input {input_index} at '{virtual_path}': {source}"
            ),
        }
    }
}

impl std::error::Error for RootResourceCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FileSystem { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn clone_catalog_string(value: &str, requested: usize) -> Result<String, RootResourceCatalogError> {
    let mut result = String::new();
    result
        .try_reserve_exact(requested)
        .map_err(|_| RootResourceCatalogError::AllocationFailed { requested })?;
    result.push_str(value);
    Ok(result)
}

struct RootResourceCatalogBuilder<'a> {
    filesystem: &'a VirtualFileSystem,
    limits: RootResourceCatalogLimits,
    files: Vec<WorkspaceRootResourceFile>,
    total_name_bytes: usize,
    total_path_bytes: usize,
}

impl<'a> RootResourceCatalogBuilder<'a> {
    fn new(
        filesystem: &'a VirtualFileSystem,
        input_count: usize,
        limits: RootResourceCatalogLimits,
    ) -> Result<Self, RootResourceCatalogError> {
        if input_count > limits.max_files {
            return Err(RootResourceCatalogError::FileCountLimitExceeded {
                count: input_count,
                limit: limits.max_files,
            });
        }
        let mut files = Vec::new();
        files.try_reserve_exact(input_count).map_err(|_| {
            RootResourceCatalogError::AllocationFailed {
                requested: input_count,
            }
        })?;
        Ok(Self {
            filesystem,
            limits,
            files,
            total_name_bytes: 0,
            total_path_bytes: 0,
        })
    }

    fn push(
        &mut self,
        input_index: usize,
        kind: FileKind,
        file_name: &str,
        virtual_path: &str,
    ) -> Result<(), RootResourceCatalogError> {
        if kind != FileKind::Resource {
            return Err(RootResourceCatalogError::NotResourceFile { input_index, kind });
        }
        if file_name.is_empty() || virtual_path.is_empty() {
            return Err(RootResourceCatalogError::EmptyNameOrPath { input_index });
        }
        if file_name.len() > self.limits.max_file_name_bytes {
            return Err(RootResourceCatalogError::FileNameTooLong {
                input_index,
                byte_count: file_name.len(),
                limit: self.limits.max_file_name_bytes,
            });
        }
        if virtual_path.len() > self.limits.max_virtual_path_bytes {
            return Err(RootResourceCatalogError::VirtualPathTooLong {
                input_index,
                byte_count: virtual_path.len(),
                limit: self.limits.max_virtual_path_bytes,
            });
        }
        self.total_name_bytes = self.total_name_bytes.checked_add(file_name.len()).ok_or(
            RootResourceCatalogError::AggregateSizeOverflow {
                resource: "explicit root ResourceFile names",
            },
        )?;
        if self.total_name_bytes > self.limits.max_total_file_name_bytes {
            return Err(RootResourceCatalogError::AggregateSizeLimitExceeded {
                resource: "explicit root ResourceFile names",
                byte_count: self.total_name_bytes,
                limit: self.limits.max_total_file_name_bytes,
            });
        }
        self.total_path_bytes = self
            .total_path_bytes
            .checked_add(virtual_path.len())
            .ok_or(RootResourceCatalogError::AggregateSizeOverflow {
                resource: "explicit root ResourceFile paths",
            })?;
        if self.total_path_bytes > self.limits.max_total_virtual_path_bytes {
            return Err(RootResourceCatalogError::AggregateSizeLimitExceeded {
                resource: "explicit root ResourceFile paths",
                byte_count: self.total_path_bytes,
                limit: self.limits.max_total_virtual_path_bytes,
            });
        }
        let exists = self
            .filesystem
            .logical_file_exists(virtual_path)
            .map_err(|source| RootResourceCatalogError::FileSystem {
                input_index,
                virtual_path: virtual_path.to_owned(),
                source,
            })?;
        if !exists {
            return Err(RootResourceCatalogError::FileNotFound {
                input_index,
                virtual_path: virtual_path.to_owned(),
            });
        }
        self.files.push(WorkspaceRootResourceFile {
            name: clone_catalog_string(file_name, file_name.len())?,
            virtual_path: clone_catalog_string(virtual_path, virtual_path.len())?,
        });
        Ok(())
    }

    fn finish(mut self) -> RootResourceFileCatalog {
        // Last input first, like the source file-stack pop order.
        self.files.reverse();
        RootResourceFileCatalog { files: self.files }
    }
}

/// Errors from the ordered bundle-scope and external-provider ResourceFile lookup.
#[derive(Debug)]
pub enum ResourceFileResolveError {
    Bundle(WorkspaceResourceLookupError),
    ExplicitRootResourceIndexOutOfBounds { index: usize, count: usize },
    ExternalProvider(PlatformResourceLookupError),
}

impl fmt::Display for ResourceFileResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundle(source) => {
                write!(formatter, "bundle ResourceFile lookup failed: {source}")
            }
            Self::ExplicitRootResourceIndexOutOfBounds { index, count } => write!(
                formatter,
                "explicit root ResourceFile index {index} is outside {count} inputs"
            ),
            Self::ExternalProvider(source) => {
                write!(formatter, "external ResourceFile lookup failed: {source}")
            }
        }
    }
}

impl std::error::Error for ResourceFileResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bundle(source) => Some(source),
            Self::ExplicitRootResourceIndexOutOfBounds { .. } => None,
            Self::ExternalProvider(source) => Some(source),
        }
    }
}

/// Whether a decoded StreamingInfo is unset, unresolved, or points at a valid VFS range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamedResourceResolution<'a> {
    Unset,
    Missing {
        unavailable_data_paths: Vec<String>,
    },
    Resolved {
        resource: ResolvedResourceFile<'a>,
        range_is_valid: bool,
    },
}

/// A resource name lookup or range-integrity error for decoded StreamingInfo.
#[derive(Debug)]
pub enum StreamedResourceResolutionError {
    ResourceLookup(ResourceFileResolveError),
    RangeIntegrity(StreamedResourceError),
}

impl fmt::Display for StreamedResourceResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResourceLookup(source) => {
                write!(
                    formatter,
                    "StreamingInfo ResourceFile lookup failed: {source}"
                )
            }
            Self::RangeIntegrity(source) => {
                write!(
                    formatter,
                    "StreamingInfo range integrity check failed: {source}"
                )
            }
        }
    }
}

impl std::error::Error for StreamedResourceResolutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ResourceLookup(source) => Some(source),
            Self::RangeIntegrity(source) => Some(source),
        }
    }
}

/// A bounded byte range selected from a ResourceFile for a decoded StreamingInfo.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamedResourceContentResolution<'a> {
    Unset,
    Missing {
        unavailable_data_paths: Vec<String>,
    },
    Resolved {
        resource: ResolvedResourceFile<'a>,
        bytes: Vec<u8>,
    },
}

/// A lookup or bounded read failure while materializing a decoded StreamingInfo.
#[derive(Debug)]
pub enum StreamedResourceContentError {
    ResourceLookup(ResourceFileResolveError),
    Read(StreamedResourceError),
    UnexpectedUnsetAfterLookup,
}

impl fmt::Display for StreamedResourceContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResourceLookup(source) => {
                write!(
                    formatter,
                    "StreamingInfo ResourceFile lookup failed: {source}"
                )
            }
            Self::Read(source) => {
                write!(formatter, "StreamingInfo byte-range read failed: {source}")
            }
            Self::UnexpectedUnsetAfterLookup => formatter
                .write_str("a set StreamingInfo became unset after its ResourceFile was resolved"),
        }
    }
}

impl std::error::Error for StreamedResourceContentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ResourceLookup(source) => Some(source),
            Self::Read(source) => Some(source),
            Self::UnexpectedUnsetAfterLookup => None,
        }
    }
}

/// The path, byte offset, and byte count decoded from a Unity resource stream field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamedResourceInfo {
    pub path: String,
    pub offset: u64,
    pub size: u32,
}

impl StreamedResourceInfo {
    /// Matches the source `StreamingInfo.IsSet` rule, which checks whether Path is nonempty.
    pub fn is_set(&self) -> bool {
        !self.path.is_empty()
    }

    /// `StreamingInfoExtensions.ClearValues`: empty the path and zero the offset and size.
    ///
    /// The export collections (`YamlStreamedAssetExportCollection`) clear the values before
    /// writing the asset so the streamed payload is exported inline, then restore them with
    /// [`Self::set_values`].
    pub fn clear_values(&mut self) {
        self.path.clear();
        self.offset = 0;
        self.size = 0;
    }

    /// `StreamingInfoExtensions.SetValues`: replace the path, offset, and size wholesale.
    ///
    /// This mirrors the source blind assignment (`uint` size included); range validity stays
    /// with the read-time range checks, not with this setter.
    /// Field reads need no helper: they are plain field accesses like `GetValues` out-params.
    pub fn set_values(&mut self, path: String, offset: u64, size: u32) {
        self.path = path;
        self.offset = offset;
        self.size = size;
    }
}

/// Why a decoded value could not be interpreted using the `StreamingInfo` TypeTree shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamedResourceInfoError {
    FieldLookup {
        field: &'static str,
        source: SerializedValueLookupError,
    },
    MissingField {
        field: &'static str,
    },
    WrongFieldType {
        field: &'static str,
        expected: &'static str,
        actual: &'static str,
    },
    InvalidUtf8Path {
        valid_up_to: usize,
        error_len: Option<usize>,
    },
    AllocationFailed {
        requested: usize,
    },
}

impl fmt::Display for StreamedResourceInfoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field, source } => {
                write!(formatter, "could not read StreamingInfo field '{field}': {source}")
            }
            Self::MissingField { field } => {
                write!(formatter, "StreamingInfo is missing required field '{field}'")
            }
            Self::WrongFieldType {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "StreamingInfo field '{field}' has type {actual}; expected {expected}"
            ),
            Self::InvalidUtf8Path {
                valid_up_to,
                error_len,
            } => write!(
                formatter,
                "StreamingInfo path contains invalid UTF-8 at byte {valid_up_to} (invalid length {error_len:?})"
            ),
            Self::AllocationFailed { requested } => write!(
                formatter,
                "could not allocate {requested} bytes for StreamingInfo path"
            ),
        }
    }
}

impl std::error::Error for StreamedResourceInfoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FieldLookup { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// A decoded StreamingInfo together with its nested field location inside the object value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocatedStreamedResourceInfo {
    pub field_path: String,
    pub info: StreamedResourceInfo,
}

/// Scan results with the amount of decoded value structure visited.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StreamedResourceScanResult {
    pub value_nodes_scanned: usize,
    pub values: Vec<LocatedStreamedResourceInfo>,
}

/// A malformed or over-budget nested StreamingInfo scan.
#[derive(Debug)]
pub enum StreamedResourceScanError {
    ValueNodeCountOverflow,
    ValueNodeLimitExceeded {
        count: usize,
        limit: usize,
    },
    CandidateCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    FieldPathByteCountOverflow,
    ResourcePathByteCountOverflow,
    ResourcePathTooLong {
        byte_count: usize,
        limit: usize,
    },
    TotalResourcePathBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    FieldPathTooLong {
        byte_count: usize,
        limit: usize,
    },
    TotalFieldPathBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    InvalidInfo {
        field_path: String,
        source: StreamedResourceInfoError,
    },
}

impl fmt::Display for StreamedResourceScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValueNodeCountOverflow => {
                formatter.write_str("StreamingInfo value-node count overflows")
            }
            Self::ValueNodeLimitExceeded { count, limit } => write!(
                formatter,
                "StreamingInfo scan visits {count} value nodes, exceeding limit {limit}"
            ),
            Self::CandidateCountLimitExceeded { count, limit } => write!(
                formatter,
                "StreamingInfo scan found {count} candidates, exceeding limit {limit}"
            ),
            Self::FieldPathByteCountOverflow => {
                formatter.write_str("StreamingInfo field-path byte count overflows")
            }
            Self::ResourcePathByteCountOverflow => {
                formatter.write_str("StreamingInfo resource-path byte count overflows")
            }
            Self::ResourcePathTooLong { byte_count, limit } => write!(
                formatter,
                "StreamingInfo resource path has {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::TotalResourcePathBytesLimitExceeded { byte_count, limit } => write!(
                formatter,
                "aggregate StreamingInfo resource paths use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::FieldPathTooLong { byte_count, limit } => write!(
                formatter,
                "StreamingInfo field path has {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::TotalFieldPathBytesLimitExceeded { byte_count, limit } => write!(
                formatter,
                "aggregate StreamingInfo field paths use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::AllocationFailed { resource, requested } => write!(
                formatter,
                "could not allocate {requested} entries for {resource}"
            ),
            Self::InvalidInfo { field_path, source } => {
                write!(formatter, "malformed StreamingInfo at '{field_path}': {source}")
            }
        }
    }
}

impl std::error::Error for StreamedResourceScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidInfo { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Extracts Unity's `path`, `offset`, and `size` fields from a decoded StreamingInfo TypeTree.
///
/// The serialized integer variant carries the baseline's `Has_Offset_UInt64` distinction:
/// 32-bit `offset` leaves become `U32`, while 64-bit leaves become `U64`.
pub fn extract_streamed_resource_info(
    value: &SerializedValue,
) -> Result<StreamedResourceInfo, StreamedResourceInfoError> {
    let path_value = required_streaming_info_field(value, "path")?;
    let path_bytes = match path_value {
        SerializedValue::StringBytes(bytes) => bytes,
        other => {
            return Err(wrong_streaming_info_type(
                "path",
                "UTF-8 string",
                serialized_value_kind(other),
            ));
        }
    };
    let utf8_path = std::str::from_utf8(path_bytes).map_err(|error| {
        StreamedResourceInfoError::InvalidUtf8Path {
            valid_up_to: error.valid_up_to(),
            error_len: error.error_len(),
        }
    })?;
    let mut path = String::new();
    path.try_reserve_exact(utf8_path.len()).map_err(|_| {
        StreamedResourceInfoError::AllocationFailed {
            requested: utf8_path.len(),
        }
    })?;
    path.push_str(utf8_path);

    let offset = match required_streaming_info_field(value, "offset")? {
        SerializedValue::U32(offset) => u64::from(*offset),
        SerializedValue::U64(offset) => *offset,
        other => {
            return Err(wrong_streaming_info_type(
                "offset",
                "UInt32 or UInt64",
                serialized_value_kind(other),
            ));
        }
    };
    let size = match required_streaming_info_field(value, "size")? {
        SerializedValue::U32(size) => *size,
        other => {
            return Err(wrong_streaming_info_type(
                "size",
                "UInt32",
                serialized_value_kind(other),
            ));
        }
    };
    Ok(StreamedResourceInfo { path, offset, size })
}

/// Finds decoded `StreamingInfo` structs nested anywhere in an object's TypeTree value.
///
/// A struct is treated as a candidate when it contains at least two of `path`, `offset`, and
/// `size`, or when it is nested under a known StreamingInfo owner field. Candidate shape errors
/// are returned with the field path instead of being skipped.
pub fn scan_streamed_resource_infos(
    root: &SerializedValue,
    limits: StreamedResourceScanLimits,
) -> Result<StreamedResourceScanResult, StreamedResourceScanError> {
    let mut events = Vec::new();
    events
        .try_reserve(1)
        .map_err(|_| scan_allocation_error("StreamingInfo traversal events", 1))?;
    events.push(StreamInfoScanEvent::Visit {
        value: root,
        segment: None,
        expected_info: false,
    });
    let mut field_path = Vec::new();
    let mut located = Vec::new();
    let mut value_node_count = 0usize;
    let mut total_field_path_bytes = 0usize;
    let mut total_resource_path_bytes = 0usize;

    while let Some(event) = events.pop() {
        match event {
            StreamInfoScanEvent::LeavePath(previous_length) => field_path.truncate(previous_length),
            StreamInfoScanEvent::Visit {
                value,
                segment,
                expected_info,
            } => {
                let previous_path_length = field_path.len();
                if let Some(segment) = segment {
                    field_path.try_reserve(1).map_err(|_| {
                        scan_allocation_error("StreamingInfo field path segments", 1)
                    })?;
                    field_path.push(segment);
                }
                value_node_count = value_node_count
                    .checked_add(1)
                    .ok_or(StreamedResourceScanError::ValueNodeCountOverflow)?;
                if value_node_count > limits.max_value_nodes {
                    return Err(StreamedResourceScanError::ValueNodeLimitExceeded {
                        count: value_node_count,
                        limit: limits.max_value_nodes,
                    });
                }
                events
                    .try_reserve(1)
                    .map_err(|_| scan_allocation_error("StreamingInfo traversal events", 1))?;
                events.push(StreamInfoScanEvent::LeavePath(previous_path_length));

                match value {
                    SerializedValue::Struct(fields) => {
                        let relevant_field_count = streaming_info_field_count(fields);
                        let is_candidate = relevant_field_count >= 2
                            || (expected_info && relevant_field_count > 0);
                        if is_candidate {
                            let requested_candidates = located.len().checked_add(1).ok_or(
                                StreamedResourceScanError::CandidateCountLimitExceeded {
                                    count: usize::MAX,
                                    limit: limits.max_candidates,
                                },
                            )?;
                            if requested_candidates > limits.max_candidates {
                                return Err(
                                    StreamedResourceScanError::CandidateCountLimitExceeded {
                                        count: requested_candidates,
                                        limit: limits.max_candidates,
                                    },
                                );
                            }
                            let diagnostic_path = render_streamed_resource_field_path(
                                &field_path,
                                limits.max_single_field_path_bytes,
                            )?;
                            total_field_path_bytes = total_field_path_bytes
                                .checked_add(diagnostic_path.len())
                                .ok_or(StreamedResourceScanError::FieldPathByteCountOverflow)?;
                            if total_field_path_bytes > limits.max_total_field_path_bytes {
                                return Err(
                                    StreamedResourceScanError::TotalFieldPathBytesLimitExceeded {
                                        byte_count: total_field_path_bytes,
                                        limit: limits.max_total_field_path_bytes,
                                    },
                                );
                            }
                            let info = extract_streamed_resource_info(value).map_err(|source| {
                                StreamedResourceScanError::InvalidInfo {
                                    field_path: diagnostic_path.clone(),
                                    source,
                                }
                            })?;
                            if info.path.len() > limits.max_resource_path_bytes {
                                return Err(StreamedResourceScanError::ResourcePathTooLong {
                                    byte_count: info.path.len(),
                                    limit: limits.max_resource_path_bytes,
                                });
                            }
                            total_resource_path_bytes = total_resource_path_bytes
                                .checked_add(info.path.len())
                                .ok_or(StreamedResourceScanError::ResourcePathByteCountOverflow)?;
                            if total_resource_path_bytes > limits.max_total_resource_path_bytes {
                                return Err(
                                    StreamedResourceScanError::TotalResourcePathBytesLimitExceeded {
                                        byte_count: total_resource_path_bytes,
                                        limit: limits.max_total_resource_path_bytes,
                                    },
                                );
                            }
                            located.try_reserve(1).map_err(|_| {
                                scan_allocation_error("located StreamingInfo values", 1)
                            })?;
                            located.push(LocatedStreamedResourceInfo {
                                field_path: diagnostic_path,
                                info,
                            });
                        } else {
                            events.try_reserve(fields.len()).map_err(|_| {
                                scan_allocation_error(
                                    "StreamingInfo traversal events",
                                    fields.len(),
                                )
                            })?;
                            for field in fields.iter().rev() {
                                events.push(StreamInfoScanEvent::Visit {
                                    value: &field.value,
                                    segment: Some(StreamInfoPathSegment::Field(&field.name)),
                                    expected_info: is_streaming_info_owner_field(&field.name),
                                });
                            }
                        }
                    }
                    SerializedValue::Array(values) => {
                        events.try_reserve(values.len()).map_err(|_| {
                            scan_allocation_error("StreamingInfo traversal events", values.len())
                        })?;
                        for (index, value) in values.iter().enumerate().rev() {
                            events.push(StreamInfoScanEvent::Visit {
                                value,
                                segment: Some(StreamInfoPathSegment::Index(index)),
                                expected_info,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(StreamedResourceScanResult {
        value_nodes_scanned: value_node_count,
        values: located,
    })
}

/// Convenience wrapper that returns only the located StreamingInfo values.
pub fn find_streamed_resource_infos(
    root: &SerializedValue,
    limits: StreamedResourceScanLimits,
) -> Result<Vec<LocatedStreamedResourceInfo>, StreamedResourceScanError> {
    scan_streamed_resource_infos(root, limits).map(|result| result.values)
}

#[derive(Clone, Copy)]
enum StreamInfoPathSegment<'a> {
    Field(&'a str),
    Index(usize),
}

enum StreamInfoScanEvent<'a> {
    Visit {
        value: &'a SerializedValue,
        segment: Option<StreamInfoPathSegment<'a>>,
        expected_info: bool,
    },
    LeavePath(usize),
}

fn streaming_info_field_count(
    fields: &[crate::serialized_file::object::SerializedFieldValue],
) -> usize {
    let mut path = false;
    let mut offset = false;
    let mut size = false;
    for field in fields {
        match field.name.as_str() {
            "path" => path = true,
            "offset" => offset = true,
            "size" => size = true,
            _ => {}
        }
    }
    usize::from(path) + usize::from(offset) + usize::from(size)
}

fn is_streaming_info_owner_field(field_name: &str) -> bool {
    matches!(
        field_name,
        "m_StreamData"
            | "m_StreamData_C28"
            | "m_StreamData_C189"
            | "m_StreamingInfo"
            | "StreamingInfo"
    )
}

fn render_streamed_resource_field_path(
    segments: &[StreamInfoPathSegment<'_>],
    limit: usize,
) -> Result<String, StreamedResourceScanError> {
    let mut byte_count = 0usize;
    for (index, segment) in segments.iter().enumerate() {
        let segment_bytes = match segment {
            StreamInfoPathSegment::Field(name) => name
                .len()
                .checked_add(usize::from(index > 0))
                .ok_or(StreamedResourceScanError::FieldPathByteCountOverflow)?,
            StreamInfoPathSegment::Index(value) => {
                let digits = if *value == 0 {
                    1
                } else {
                    usize::try_from(value.ilog10() + 1).unwrap_or(usize::MAX)
                };
                digits
                    .checked_add(2)
                    .ok_or(StreamedResourceScanError::FieldPathByteCountOverflow)?
            }
        };
        byte_count = byte_count
            .checked_add(segment_bytes)
            .ok_or(StreamedResourceScanError::FieldPathByteCountOverflow)?;
    }
    if segments.is_empty() {
        byte_count = "<root>".len();
    }
    if byte_count > limit {
        return Err(StreamedResourceScanError::FieldPathTooLong { byte_count, limit });
    }
    let mut path = String::new();
    path.try_reserve_exact(byte_count)
        .map_err(|_| scan_allocation_error("StreamingInfo diagnostic path", byte_count))?;
    if segments.is_empty() {
        path.push_str("<root>");
    }
    for segment in segments {
        match segment {
            StreamInfoPathSegment::Field(name) => {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(name);
            }
            StreamInfoPathSegment::Index(index) => {
                write!(&mut path, "[{index}]").expect("writing to String cannot fail");
            }
        }
    }
    Ok(path)
}

fn scan_allocation_error(resource: &'static str, requested: usize) -> StreamedResourceScanError {
    StreamedResourceScanError::AllocationFailed {
        resource,
        requested,
    }
}

fn required_streaming_info_field<'a>(
    value: &'a SerializedValue,
    field: &'static str,
) -> Result<&'a SerializedValue, StreamedResourceInfoError> {
    value
        .unique_field(field)
        .map_err(|source| StreamedResourceInfoError::FieldLookup { field, source })?
        .ok_or(StreamedResourceInfoError::MissingField { field })
}

fn wrong_streaming_info_type(
    field: &'static str,
    expected: &'static str,
    actual: &'static str,
) -> StreamedResourceInfoError {
    StreamedResourceInfoError::WrongFieldType {
        field,
        expected,
        actual,
    }
}

fn serialized_value_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "Bool",
        SerializedValue::I8(_) => "Int8",
        SerializedValue::U8(_) => "UInt8",
        SerializedValue::I16(_) => "Int16",
        SerializedValue::U16(_) => "UInt16",
        SerializedValue::Char(_) => "Char",
        SerializedValue::I32(_) => "Int32",
        SerializedValue::U32(_) => "UInt32",
        SerializedValue::I64(_) => "Int64",
        SerializedValue::U64(_) => "UInt64",
        SerializedValue::F32(_) => "Float32",
        SerializedValue::F64(_) => "Float64",
        SerializedValue::StringBytes(_) => "String",
        SerializedValue::Bytes(_) => "ByteArray",
        SerializedValue::Array(_) => "Array",
        SerializedValue::Struct(_) => "Struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::Raw { .. } => "Raw",
    }
}

/// Resolves a ResourceFile using the baseline order: bundle hierarchy first, then the
/// GameBundle's platform/mixed external provider.
///
/// One deliberate difference is documented, not changed: `GameBundle` caches a
/// provider hit back into its resources (`AddResource`), so a second lookup skips the
/// provider, while this immutable model re-queries every time (verified against the
/// real runtime: same instance returned, one provider call). Outcomes match for
/// deterministic providers; only provider side-effect counts differ.
pub fn resolve_resource_file<'a>(
    filesystem: &VirtualFileSystem,
    workspace: &'a SerializedFileWorkspace,
    source_bundle_index: Option<usize>,
    original_name: &str,
    provider: &PlatformResourceProvider<'_>,
    limits: PlatformStructureLimits,
) -> Result<ResolvedResourceFile<'a>, ResourceFileResolveError> {
    if original_name.is_empty() {
        return Ok(ResolvedResourceFile::Missing {
            unavailable_data_paths: Vec::new(),
        });
    }
    let Some(resource_index) = workspace
        .resolve_bundled_resource_file(source_bundle_index, original_name)
        .map_err(ResourceFileResolveError::Bundle)?
    else {
        return resolve_external_resource_file(filesystem, original_name, provider, limits);
    };
    let resource = workspace
        .resource_files()
        .get(resource_index)
        .ok_or_else(|| {
            ResourceFileResolveError::Bundle(
                WorkspaceResourceLookupError::ResourceFileIndexOutOfBounds {
                    index: resource_index,
                    count: workspace.resource_files().len(),
                },
            )
        })?;
    Ok(ResolvedResourceFile::Bundled {
        resource_index,
        virtual_path: &resource.virtual_path,
    })
}

/// Resolves an explicit GameBundle root ResourceFile between mounted Bundle resources and the
/// external Platform/Mixed provider. Provider hits are not cached back (see
/// `resolve_resource_file`); explicit root duplicates resolve last-input-first.
pub fn resolve_resource_file_with_root_catalog<'a>(
    filesystem: &VirtualFileSystem,
    workspace: &'a SerializedFileWorkspace,
    source_bundle_index: Option<usize>,
    original_name: &str,
    root_catalog: &'a RootResourceFileCatalog,
    provider: &PlatformResourceProvider<'_>,
    limits: PlatformStructureLimits,
) -> Result<ResolvedResourceFile<'a>, ResourceFileResolveError> {
    if original_name.is_empty() {
        return Ok(ResolvedResourceFile::Missing {
            unavailable_data_paths: Vec::new(),
        });
    }
    let bundled_resource = workspace
        .resolve_bundled_or_root_resource_file(
            source_bundle_index,
            original_name,
            root_catalog.files(),
        )
        .map_err(ResourceFileResolveError::Bundle)?;
    match bundled_resource {
        Some(WorkspaceResolvedResourceFile::MountedBundle { resource_index }) => {
            let resource = workspace
                .resource_files()
                .get(resource_index)
                .ok_or_else(|| {
                    ResourceFileResolveError::Bundle(
                        WorkspaceResourceLookupError::ResourceFileIndexOutOfBounds {
                            index: resource_index,
                            count: workspace.resource_files().len(),
                        },
                    )
                })?;
            Ok(ResolvedResourceFile::Bundled {
                resource_index,
                virtual_path: &resource.virtual_path,
            })
        }
        Some(WorkspaceResolvedResourceFile::ExplicitRoot { resource_index }) => {
            let resource = root_catalog.files().get(resource_index).ok_or(
                ResourceFileResolveError::ExplicitRootResourceIndexOutOfBounds {
                    index: resource_index,
                    count: root_catalog.files().len(),
                },
            )?;
            Ok(ResolvedResourceFile::ExplicitRoot {
                resource_index,
                virtual_path: &resource.virtual_path,
            })
        }
        None => resolve_external_resource_file(filesystem, original_name, provider, limits),
    }
}

fn resolve_external_resource_file(
    filesystem: &VirtualFileSystem,
    original_name: &str,
    provider: &PlatformResourceProvider<'_>,
    limits: PlatformStructureLimits,
) -> Result<ResolvedResourceFile<'static>, ResourceFileResolveError> {
    match provider
        .find_resource(filesystem, original_name, limits)
        .map_err(ResourceFileResolveError::ExternalProvider)?
    {
        PlatformResourceLookup::Found {
            virtual_path,
            source,
            data_path_index,
            unavailable_data_paths,
        } => Ok(ResolvedResourceFile::External {
            virtual_path,
            source,
            data_path_index,
            unavailable_data_paths,
        }),
        PlatformResourceLookup::Missing {
            unavailable_data_paths,
        } => Ok(ResolvedResourceFile::Missing {
            unavailable_data_paths,
        }),
    }
}

/// Resolves one decoded StreamingInfo and checks its offset/size against the selected ResourceFile.
///
/// This performs no range allocation or content read. Empty paths preserve the source
/// `StreamingInfo.IsSet` behavior, and a missing/unavailable provider result stays distinct from a
/// resolved file whose byte range is invalid.
pub fn resolve_streamed_resource_info<'a>(
    filesystem: &VirtualFileSystem,
    workspace: &'a SerializedFileWorkspace,
    source_bundle_index: Option<usize>,
    info: &StreamedResourceInfo,
    root_catalog: &'a RootResourceFileCatalog,
    provider: &PlatformResourceProvider<'_>,
    limits: PlatformStructureLimits,
) -> Result<StreamedResourceResolution<'a>, StreamedResourceResolutionError> {
    if !info.is_set() {
        return Ok(StreamedResourceResolution::Unset);
    }
    let resource = resolve_resource_file_with_root_catalog(
        filesystem,
        workspace,
        source_bundle_index,
        &info.path,
        root_catalog,
        provider,
        limits,
    )
    .map_err(StreamedResourceResolutionError::ResourceLookup)?;
    let resource_path = match &resource {
        ResolvedResourceFile::Bundled { virtual_path, .. }
        | ResolvedResourceFile::ExplicitRoot { virtual_path, .. } => *virtual_path,
        ResolvedResourceFile::External { virtual_path, .. } => virtual_path,
        ResolvedResourceFile::Missing {
            unavailable_data_paths,
        } => {
            return Ok(StreamedResourceResolution::Missing {
                unavailable_data_paths: unavailable_data_paths.clone(),
            });
        }
    };
    let range_is_valid = check_streamed_resource_integrity(
        filesystem,
        resource_path,
        info.offset,
        u64::from(info.size),
    )
    .map_err(StreamedResourceResolutionError::RangeIntegrity)?;
    Ok(StreamedResourceResolution::Resolved {
        resource,
        range_is_valid,
    })
}

/// Resolves a decoded StreamingInfo through its owner scopes and reads its bounded byte range.
///
/// Missing resources and unset streams remain distinct results. A resolved resource whose range
/// is invalid, too large to materialize, or unreadable returns a contextual error.
pub fn resolve_and_read_streamed_resource_info<'a>(
    filesystem: &VirtualFileSystem,
    workspace: &'a SerializedFileWorkspace,
    source_bundle_index: Option<usize>,
    info: &StreamedResourceInfo,
    root_catalog: &'a RootResourceFileCatalog,
    provider: &PlatformResourceProvider<'_>,
    limits: StreamedResourceContentLimits,
) -> Result<StreamedResourceContentResolution<'a>, StreamedResourceContentError> {
    if !info.is_set() {
        return Ok(StreamedResourceContentResolution::Unset);
    }
    let resource = resolve_resource_file_with_root_catalog(
        filesystem,
        workspace,
        source_bundle_index,
        &info.path,
        root_catalog,
        provider,
        limits.resource_lookup,
    )
    .map_err(StreamedResourceContentError::ResourceLookup)?;
    let resource_path = match &resource {
        ResolvedResourceFile::Bundled { virtual_path, .. }
        | ResolvedResourceFile::ExplicitRoot { virtual_path, .. } => *virtual_path,
        ResolvedResourceFile::External { virtual_path, .. } => virtual_path,
        ResolvedResourceFile::Missing {
            unavailable_data_paths,
        } => {
            return Ok(StreamedResourceContentResolution::Missing {
                unavailable_data_paths: unavailable_data_paths.clone(),
            });
        }
    };
    let bytes =
        read_streamed_resource_info_bytes(filesystem, resource_path, info, limits.range_read)
            .map_err(StreamedResourceContentError::Read)?
            .ok_or(StreamedResourceContentError::UnexpectedUnsetAfterLookup)?;
    Ok(StreamedResourceContentResolution::Resolved { resource, bytes })
}

impl Default for StreamedResourceLimits {
    fn default() -> Self {
        Self {
            max_range_copy_bytes: 512 * 1024 * 1024,
            max_materialized_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Lookup and materialization bounds for resolving a decoded StreamingInfo to bytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamedResourceContentLimits {
    pub resource_lookup: PlatformStructureLimits,
    pub range_read: StreamedResourceLimits,
}

/// Why a streamed-resource range cannot be used.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidStreamedResourceRange {
    ZeroLength,
    OffsetOrSizeOutsideSignedStreamRange,
    EndOffsetOverflow,
}

/// An explicit failure while resolving or reading a ResourceFile byte range.
#[derive(Debug)]
pub enum StreamedResourceError {
    InvalidRange {
        path: String,
        offset: u64,
        size: u64,
        reason: InvalidStreamedResourceRange,
    },
    ResourceNotFound {
        path: String,
    },
    RangeOutsideResource {
        path: String,
        offset: u64,
        size: u64,
        resource_size: u64,
    },
    RangeCopyLimitExceeded {
        path: String,
        size: u64,
        limit: usize,
    },
    MaterializationLimitExceeded {
        path: String,
        size: u64,
        limit: usize,
    },
    AllocationFailed {
        path: String,
        size: usize,
    },
    Io {
        path: String,
        operation: &'static str,
        kind: io::ErrorKind,
        message: String,
    },
}

impl fmt::Display for StreamedResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRange {
                path,
                offset,
                size,
                reason,
            } => write!(
                formatter,
                "streamed resource '{path}' has invalid range offset={offset}, size={size}: {reason:?}"
            ),
            Self::ResourceNotFound { path } => {
                write!(formatter, "streamed resource file '{path}' was not found")
            }
            Self::RangeOutsideResource {
                path,
                offset,
                size,
                resource_size,
            } => write!(
                formatter,
                "streamed resource range {offset}..{} exceeds '{path}' length {resource_size}",
                offset.saturating_add(*size)
            ),
            Self::RangeCopyLimitExceeded { path, size, limit } => write!(
                formatter,
                "reading streamed range {size} from '{path}' exceeds copy limit {limit}"
            ),
            Self::MaterializationLimitExceeded { path, size, limit } => write!(
                formatter,
                "materializing streamed range {size} from '{path}' exceeds limit {limit}"
            ),
            Self::AllocationFailed { path, size } => write!(
                formatter,
                "could not allocate {size} bytes for streamed resource '{path}'"
            ),
            Self::Io {
                path,
                operation,
                message,
                ..
            } => write!(formatter, "could not {operation} streamed resource '{path}': {message}"),
        }
    }
}

impl std::error::Error for StreamedResourceError {}

/// Opens a bounded, seekable range from a ResourceFile already present in the VFS.
///
/// An empty path means that the object has no external stream and returns `Ok(None)`. Unity's
/// StreamedResource helpers reject zero-sized non-empty references and ranges beyond signed
/// stream offsets; those conditions are returned as explicit errors here.
pub fn open_streamed_resource_range(
    filesystem: &VirtualFileSystem,
    path: &str,
    offset: u64,
    size: u64,
    limits: StreamedResourceLimits,
) -> Result<Option<SmartStream>, StreamedResourceError> {
    if path.is_empty() {
        return Ok(None);
    }
    let end = validate_streamed_range(path, offset, size)?;

    let stream = open_resource_stream(filesystem, path)?;
    let resource_size = stream
        .length()
        .map_err(|error| io_error(path, "measure", error))?;
    if end > resource_size {
        return Err(StreamedResourceError::RangeOutsideResource {
            path: path.to_owned(),
            offset,
            size,
            resource_size,
        });
    }

    stream
        .create_partial_with_limit(offset, size, limits.max_range_copy_bytes)
        .map(Some)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidData
                && size > u64::try_from(limits.max_range_copy_bytes).unwrap_or(u64::MAX)
            {
                StreamedResourceError::RangeCopyLimitExceeded {
                    path: path.to_owned(),
                    size,
                    limit: limits.max_range_copy_bytes,
                }
            } else {
                io_error(path, "open byte range", error)
            }
        })
}

/// Reads a complete streamed-resource range into a bounded byte vector.
pub fn read_streamed_resource_bytes(
    filesystem: &VirtualFileSystem,
    path: &str,
    offset: u64,
    size: u64,
    limits: StreamedResourceLimits,
) -> Result<Option<Vec<u8>>, StreamedResourceError> {
    if path.is_empty() {
        return Ok(None);
    }
    let end = validate_streamed_range(path, offset, size)?;
    if size > u64::try_from(limits.max_materialized_bytes).unwrap_or(u64::MAX) {
        return Err(StreamedResourceError::MaterializationLimitExceeded {
            path: path.to_owned(),
            size,
            limit: limits.max_materialized_bytes,
        });
    }
    let stream = open_resource_stream(filesystem, path)?;
    let resource_size = stream
        .length()
        .map_err(|error| io_error(path, "measure", error))?;
    if end > resource_size {
        return Err(StreamedResourceError::RangeOutsideResource {
            path: path.to_owned(),
            offset,
            size,
            resource_size,
        });
    }
    let byte_count =
        usize::try_from(size).map_err(|_| StreamedResourceError::MaterializationLimitExceeded {
            path: path.to_owned(),
            size,
            limit: limits.max_materialized_bytes,
        })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(byte_count)
        .map_err(|_| StreamedResourceError::AllocationFailed {
            path: path.to_owned(),
            size: byte_count,
        })?;
    bytes.resize(byte_count, 0);
    stream
        .read_exact_at(offset, &mut bytes)
        .map_err(|error| io_error(path, "read byte range", error))?;
    Ok(Some(bytes))
}

/// Reads the bounded range described by a decoded `StreamingInfo` using its resolved VFS path.
pub fn read_streamed_resource_info_bytes(
    filesystem: &VirtualFileSystem,
    resolved_resource_path: &str,
    info: &StreamedResourceInfo,
    limits: StreamedResourceLimits,
) -> Result<Option<Vec<u8>>, StreamedResourceError> {
    if !info.is_set() {
        return Ok(None);
    }
    read_streamed_resource_bytes(
        filesystem,
        resolved_resource_path,
        info.offset,
        u64::from(info.size),
        limits,
    )
}

/// Checks a range without allocating or reading its contents.
///
/// An empty path is considered valid and represents an object without an external stream, as in
/// the baseline `StreamedResourceExtensions.CheckIntegrity` behavior.
pub fn check_streamed_resource_integrity(
    filesystem: &VirtualFileSystem,
    path: &str,
    offset: u64,
    size: u64,
) -> Result<bool, StreamedResourceError> {
    if path.is_empty() {
        return Ok(true);
    }
    let end = match validate_streamed_range(path, offset, size) {
        Ok(end) => end,
        Err(StreamedResourceError::InvalidRange { .. }) => return Ok(false),
        Err(error) => return Err(error),
    };
    let stream = match open_resource_stream(filesystem, path) {
        Ok(stream) => stream,
        Err(StreamedResourceError::ResourceNotFound { .. }) => return Ok(false),
        Err(error) => return Err(error),
    };
    let resource_size = stream
        .length()
        .map_err(|error| io_error(path, "measure", error))?;
    Ok(end <= resource_size)
}

fn validate_streamed_range(
    path: &str,
    offset: u64,
    size: u64,
) -> Result<u64, StreamedResourceError> {
    let invalid = |reason| StreamedResourceError::InvalidRange {
        path: path.to_owned(),
        offset,
        size,
        reason,
    };
    if offset > UNITY_STREAM_OFFSET_MAX || size > UNITY_STREAM_OFFSET_MAX {
        return Err(invalid(
            InvalidStreamedResourceRange::OffsetOrSizeOutsideSignedStreamRange,
        ));
    }
    let end = offset
        .checked_add(size)
        .ok_or_else(|| invalid(InvalidStreamedResourceRange::EndOffsetOverflow))?;
    if end > UNITY_STREAM_OFFSET_MAX {
        return Err(invalid(
            InvalidStreamedResourceRange::OffsetOrSizeOutsideSignedStreamRange,
        ));
    }
    if size == 0 {
        return Err(invalid(InvalidStreamedResourceRange::ZeroLength));
    }
    Ok(end)
}

fn open_resource_stream(
    filesystem: &VirtualFileSystem,
    path: &str,
) -> Result<SmartStream, StreamedResourceError> {
    filesystem.open_logical_file(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            StreamedResourceError::ResourceNotFound {
                path: path.to_owned(),
            }
        } else {
            io_error(path, "open", error)
        }
    })
}

fn io_error(path: &str, operation: &'static str, error: io::Error) -> StreamedResourceError {
    StreamedResourceError::Io {
        path: path.to_owned(),
        operation,
        kind: error.kind(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom};

    use super::{
        check_streamed_resource_integrity, extract_streamed_resource_info,
        find_streamed_resource_infos, open_streamed_resource_range, read_streamed_resource_bytes,
        read_streamed_resource_info_bytes, resolve_and_read_streamed_resource_info,
        resolve_streamed_resource_info, ExplicitRootResourceInput, InvalidStreamedResourceRange,
        LocatedStreamedResourceInfo, ResolvedResourceFile, RootResourceCatalogError,
        RootResourceCatalogLimits, RootResourceFileCatalog, StreamedResourceContentError,
        StreamedResourceContentLimits, StreamedResourceContentResolution, StreamedResourceError,
        StreamedResourceInfo, StreamedResourceInfoError, StreamedResourceLimits,
        StreamedResourceResolution, StreamedResourceScanError, StreamedResourceScanLimits,
    };
    use crate::io::filesystem::VirtualFileSystem;
    use crate::scheme::{FileKind, SchemeReader, SchemeReaderLimits};
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
    use crate::serialized_file::workspace::WorkspaceRootResourceFile;

    fn filesystem_with_resource() -> VirtualFileSystem {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/bundle")).unwrap();
        filesystem
            .write_all_bytes("/bundle/shared.resS", b"0123456789")
            .unwrap();
        filesystem
    }

    fn streaming_info_value(path: &[u8], offset: SerializedValue) -> SerializedValue {
        SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "path".to_owned(),
                value: SerializedValue::StringBytes(path.to_vec()),
            },
            SerializedFieldValue {
                name: "offset".to_owned(),
                value: offset,
            },
            SerializedFieldValue {
                name: "size".to_owned(),
                value: SerializedValue::U32(12),
            },
        ])
    }

    /// Explicit inputs are stored in reverse (file-stack pop) order so the last input
    /// wins duplicate names, matching `GameBundle.InitializeFromPaths`. Proven by a
    /// direct runtime differential: real `FromPaths([A/dup.ress, B/dup.ress])`
    /// resolves B's file.
    #[test]
    fn explicit_root_resource_catalog_applies_file_stack_pop_order() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/root")).unwrap();
        filesystem
            .write_all_bytes("/root/first.resS", b"first")
            .unwrap();
        filesystem
            .write_all_bytes("/root/second.resS", b"second")
            .unwrap();
        let first_dispatch =
            SchemeReader::read_with_limits(b"first resource", SchemeReaderLimits::default())
                .unwrap();
        let second_dispatch =
            SchemeReader::read_with_limits(b"second resource", SchemeReaderLimits::default())
                .unwrap();
        assert_eq!(first_dispatch.kind(), FileKind::Resource);

        let inputs = [
            ExplicitRootResourceInput {
                file_name: "SHARED.resS",
                virtual_path: "/root/first.resS",
                dispatched_file: &first_dispatch,
            },
            ExplicitRootResourceInput {
                file_name: "shared.ress",
                virtual_path: "/root/second.resS",
                dispatched_file: &second_dispatch,
            },
        ];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &inputs,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(
            catalog.files(),
            [
                WorkspaceRootResourceFile {
                    name: "shared.ress".to_owned(),
                    virtual_path: "/root/second.resS".to_owned(),
                },
                WorkspaceRootResourceFile {
                    name: "SHARED.resS".to_owned(),
                    virtual_path: "/root/first.resS".to_owned(),
                },
            ]
        );
        assert!(matches!(
            RootResourceFileCatalog::from_explicit_resource_inputs(
                &filesystem,
                &inputs,
                RootResourceCatalogLimits {
                    max_files: 1,
                    ..RootResourceCatalogLimits::default()
                },
            ),
            Err(RootResourceCatalogError::FileCountLimitExceeded { count: 2, limit: 1 })
        ));
        assert!(matches!(
            RootResourceFileCatalog::from_explicit_resource_inputs(
                &filesystem,
                &inputs,
                RootResourceCatalogLimits {
                    max_total_file_name_bytes: 21,
                    ..RootResourceCatalogLimits::default()
                },
            ),
            Err(RootResourceCatalogError::AggregateSizeLimitExceeded {
                resource: "explicit root ResourceFile names",
                byte_count: 22,
                limit: 21,
            })
        ));

        let mut bundle_bytes = vec![0; 32];
        bundle_bytes[..7].copy_from_slice(b"UnityFS");
        let bundle_dispatch =
            SchemeReader::read_with_limits(&bundle_bytes, SchemeReaderLimits::default()).unwrap();
        let wrong_kind = [ExplicitRootResourceInput {
            file_name: "wrong.resS",
            virtual_path: "/root/first.resS",
            dispatched_file: &bundle_dispatch,
        }];
        assert!(matches!(
            RootResourceFileCatalog::from_explicit_resource_inputs(
                &filesystem,
                &wrong_kind,
                RootResourceCatalogLimits::default(),
            ),
            Err(RootResourceCatalogError::NotResourceFile {
                input_index: 0,
                kind: FileKind::UnityFs,
            })
        ));
        let missing = [ExplicitRootResourceInput {
            file_name: "missing.resS",
            virtual_path: "/root/missing.resS",
            dispatched_file: &first_dispatch,
        }];
        assert!(matches!(
            RootResourceFileCatalog::from_explicit_resource_inputs(
                &filesystem,
                &missing,
                RootResourceCatalogLimits::default(),
            ),
            Err(RootResourceCatalogError::FileNotFound { input_index: 0, .. })
        ));
    }

    #[test]
    fn explicit_root_duplicates_resolve_last_input_first() {
        use super::resolve_resource_file_with_root_catalog;
        use crate::io::platform_structure::{PlatformResourceProvider, PlatformStructureLimits};
        use crate::serialized_file::workspace::SerializedFileWorkspace;

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/root")).unwrap();
        filesystem
            .write_all_bytes("/root/first.resS", b"first")
            .unwrap();
        filesystem
            .write_all_bytes("/root/second.resS", b"second")
            .unwrap();
        let first_dispatch =
            SchemeReader::read_with_limits(b"first resource", SchemeReaderLimits::default())
                .unwrap();
        let second_dispatch =
            SchemeReader::read_with_limits(b"second resource", SchemeReaderLimits::default())
                .unwrap();
        let inputs = [
            ExplicitRootResourceInput {
                file_name: "dup.resS",
                virtual_path: "/root/first.resS",
                dispatched_file: &first_dispatch,
            },
            ExplicitRootResourceInput {
                file_name: "DUP.ress",
                virtual_path: "/root/second.resS",
                dispatched_file: &second_dispatch,
            },
        ];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &inputs,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        let workspace = SerializedFileWorkspace::default();
        let provider = PlatformResourceProvider::new(None, None);
        assert_eq!(
            resolve_resource_file_with_root_catalog(
                &filesystem,
                &workspace,
                None,
                "dup.resS",
                &catalog,
                &provider,
                PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::ExplicitRoot {
                resource_index: 0,
                virtual_path: "/root/second.resS",
            },
            "last explicit input wins, like real FromPaths([first, second]) resolving second's file"
        );
    }

    #[test]
    fn streaming_info_extracts_both_baseline_offset_widths() {
        assert_eq!(
            extract_streamed_resource_info(&streaming_info_value(
                b"archive:/data/shared.resS",
                SerializedValue::U32(0xFEDC_BA98),
            ))
            .unwrap(),
            StreamedResourceInfo {
                path: "archive:/data/shared.resS".to_owned(),
                offset: 0xFEDC_BA98,
                size: 12,
            }
        );
        assert_eq!(
            extract_streamed_resource_info(&streaming_info_value(
                b"shared.resS",
                SerializedValue::U64(0x1_0000_0000),
            ))
            .unwrap(),
            StreamedResourceInfo {
                path: "shared.resS".to_owned(),
                offset: 0x1_0000_0000,
                size: 12,
            }
        );
        assert!(!extract_streamed_resource_info(&streaming_info_value(
            b"",
            SerializedValue::U32(0),
        ))
        .unwrap()
        .is_set());
    }

    #[test]
    fn nested_streaming_info_scan_preserves_field_paths_and_source_order() {
        let root = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "m_StreamData".to_owned(),
                value: streaming_info_value(b"first.resS", SerializedValue::U32(3)),
            },
            SerializedFieldValue {
                name: "m_Levels".to_owned(),
                value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    SerializedFieldValue {
                        name: "m_StreamData_C28".to_owned(),
                        value: streaming_info_value(b"second.resS", SerializedValue::U64(9)),
                    },
                ])]),
            },
        ]);

        let found =
            find_streamed_resource_infos(&root, StreamedResourceScanLimits::default()).unwrap();
        assert_eq!(
            found,
            [
                LocatedStreamedResourceInfo {
                    field_path: "m_StreamData".to_owned(),
                    info: StreamedResourceInfo {
                        path: "first.resS".to_owned(),
                        offset: 3,
                        size: 12,
                    },
                },
                LocatedStreamedResourceInfo {
                    field_path: "m_Levels[0].m_StreamData_C28".to_owned(),
                    info: StreamedResourceInfo {
                        path: "second.resS".to_owned(),
                        offset: 9,
                        size: 12,
                    },
                },
            ]
        );
    }

    #[test]
    fn nested_streaming_info_scan_reports_malformed_owner_shapes_and_limits() {
        let malformed = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_StreamData".to_owned(),
            value: SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "path".to_owned(),
                    value: SerializedValue::StringBytes(b"broken.resS".to_vec()),
                },
                SerializedFieldValue {
                    name: "offset".to_owned(),
                    value: SerializedValue::U32(0),
                },
            ]),
        }]);
        assert!(matches!(
            find_streamed_resource_infos(&malformed, StreamedResourceScanLimits::default()),
            Err(StreamedResourceScanError::InvalidInfo {
                field_path,
                source: StreamedResourceInfoError::MissingField { field: "size" },
            }) if field_path == "m_StreamData"
        ));
        let nested = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "nested".to_owned(),
            value: SerializedValue::Struct(Vec::new()),
        }]);
        assert!(matches!(
            find_streamed_resource_infos(
                &nested,
                StreamedResourceScanLimits {
                    max_value_nodes: 1,
                    ..StreamedResourceScanLimits::default()
                },
            ),
            Err(StreamedResourceScanError::ValueNodeLimitExceeded { count: 2, limit: 1 })
        ));
    }

    #[test]
    fn decoded_streaming_info_resolves_explicit_root_and_checks_its_range() {
        use crate::io::platform_structure::{PlatformResourceProvider, PlatformStructureLimits};
        use crate::serialized_file::workspace::SerializedFileWorkspace;

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/explicit")).unwrap();
        filesystem
            .write_all_bytes("/explicit/root.resS", b"0123456789ab")
            .unwrap();
        let dispatched =
            SchemeReader::read_with_limits(b"root resource", SchemeReaderLimits::default())
                .unwrap();
        let input = [ExplicitRootResourceInput {
            file_name: "root.resS",
            virtual_path: "/explicit/root.resS",
            dispatched_file: &dispatched,
        }];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &input,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        let info = StreamedResourceInfo {
            path: "root.resS".to_owned(),
            offset: 2,
            size: 4,
        };
        let workspace = SerializedFileWorkspace::default();
        let provider = PlatformResourceProvider::new(None, None);
        let resolution = resolve_streamed_resource_info(
            &filesystem,
            &workspace,
            None,
            &info,
            &catalog,
            &provider,
            PlatformStructureLimits::default(),
        )
        .unwrap();
        assert_eq!(
            resolution,
            StreamedResourceResolution::Resolved {
                resource: ResolvedResourceFile::ExplicitRoot {
                    resource_index: 0,
                    virtual_path: "/explicit/root.resS",
                },
                range_is_valid: true,
            }
        );
        assert_eq!(
            resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                None,
                &info,
                &catalog,
                &provider,
                StreamedResourceContentLimits::default(),
            )
            .unwrap(),
            StreamedResourceContentResolution::Resolved {
                resource: ResolvedResourceFile::ExplicitRoot {
                    resource_index: 0,
                    virtual_path: "/explicit/root.resS",
                },
                bytes: b"2345".to_vec(),
            }
        );
    }

    #[test]
    fn streamed_resource_content_keeps_unset_missing_and_read_errors_distinct() {
        use crate::io::platform_structure::PlatformResourceProvider;
        use crate::serialized_file::workspace::SerializedFileWorkspace;

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/explicit")).unwrap();
        filesystem
            .write_all_bytes("/explicit/root.resS", b"0123456789ab")
            .unwrap();
        let dispatched =
            SchemeReader::read_with_limits(b"root resource", SchemeReaderLimits::default())
                .unwrap();
        let input = [ExplicitRootResourceInput {
            file_name: "root.resS",
            virtual_path: "/explicit/root.resS",
            dispatched_file: &dispatched,
        }];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &input,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        let workspace = SerializedFileWorkspace::default();
        let provider = PlatformResourceProvider::new(None, None);
        let content_limits = StreamedResourceContentLimits::default();

        let unset = StreamedResourceInfo {
            path: String::new(),
            offset: 0,
            size: 0,
        };
        assert_eq!(
            resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                None,
                &unset,
                &catalog,
                &provider,
                content_limits,
            )
            .unwrap(),
            StreamedResourceContentResolution::Unset
        );

        let missing = StreamedResourceInfo {
            path: "not-selected.resS".to_owned(),
            offset: 0,
            size: 4,
        };
        assert_eq!(
            resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                None,
                &missing,
                &catalog,
                &provider,
                content_limits,
            )
            .unwrap(),
            StreamedResourceContentResolution::Missing {
                unavailable_data_paths: Vec::new(),
            }
        );

        let outside = StreamedResourceInfo {
            path: "root.resS".to_owned(),
            offset: 10,
            size: 4,
        };
        assert!(matches!(
            resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                None,
                &outside,
                &catalog,
                &provider,
                content_limits,
            ),
            Err(StreamedResourceContentError::Read(
                StreamedResourceError::RangeOutsideResource {
                    offset: 10,
                    size: 4,
                    resource_size: 12,
                    ..
                }
            ))
        ));

        let over_limit = StreamedResourceInfo {
            path: "root.resS".to_owned(),
            offset: 0,
            size: 4,
        };
        assert!(matches!(
            resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                None,
                &over_limit,
                &catalog,
                &provider,
                StreamedResourceContentLimits {
                    range_read: StreamedResourceLimits {
                        max_materialized_bytes: 3,
                        ..content_limits.range_read
                    },
                    ..content_limits
                },
            ),
            Err(StreamedResourceContentError::Read(
                StreamedResourceError::MaterializationLimitExceeded {
                    size: 4,
                    limit: 3,
                    ..
                }
            ))
        ));
    }

    #[test]
    fn streaming_info_rejects_missing_duplicate_invalid_and_wrongly_typed_fields() {
        assert_eq!(
            extract_streamed_resource_info(&SerializedValue::Struct(vec![])).unwrap_err(),
            StreamedResourceInfoError::MissingField { field: "path" }
        );
        assert!(matches!(
            extract_streamed_resource_info(
                &streaming_info_value(&[0xFF], SerializedValue::U32(0),)
            ),
            Err(StreamedResourceInfoError::InvalidUtf8Path { .. })
        ));
        assert!(matches!(
            extract_streamed_resource_info(&streaming_info_value(
                b"resource.resS",
                SerializedValue::I64(5),
            )),
            Err(StreamedResourceInfoError::WrongFieldType {
                field: "offset",
                expected: "UInt32 or UInt64",
                actual: "Int64",
            })
        ));
        let duplicate_path = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "path".to_owned(),
                value: SerializedValue::StringBytes(b"one".to_vec()),
            },
            SerializedFieldValue {
                name: "path".to_owned(),
                value: SerializedValue::StringBytes(b"two".to_vec()),
            },
        ]);
        assert!(matches!(
            extract_streamed_resource_info(&duplicate_path),
            Err(StreamedResourceInfoError::FieldLookup { field: "path", .. })
        ));
        let mut wrong_size = streaming_info_value(b"resource.resS", SerializedValue::U32(0));
        let SerializedValue::Struct(fields) = &mut wrong_size else {
            unreachable!();
        };
        fields
            .iter_mut()
            .find(|field| field.name == "size")
            .unwrap()
            .value = SerializedValue::U64(12);
        assert!(matches!(
            extract_streamed_resource_info(&wrong_size),
            Err(StreamedResourceInfoError::WrongFieldType {
                field: "size",
                expected: "UInt32",
                actual: "UInt64",
            })
        ));
    }

    #[test]
    fn decoded_streaming_info_reads_its_range_from_the_resolved_resource_path() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/resolved")).unwrap();
        filesystem
            .write_all_bytes("/resolved/shared.resS", b"0123456789abcdefg")
            .unwrap();
        let info = extract_streamed_resource_info(&streaming_info_value(
            b"archive:/bundle/shared.resS",
            SerializedValue::U64(3),
        ))
        .unwrap();

        assert_eq!(
            read_streamed_resource_info_bytes(
                &filesystem,
                "/resolved/shared.resS",
                &info,
                StreamedResourceLimits::default(),
            )
            .unwrap(),
            Some(b"3456789abcde".to_vec())
        );
        let empty_info =
            extract_streamed_resource_info(&streaming_info_value(b"", SerializedValue::U32(0)))
                .unwrap();
        assert_eq!(
            read_streamed_resource_info_bytes(
                &filesystem,
                "/resolved/missing.resS",
                &empty_info,
                StreamedResourceLimits::default(),
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn reads_only_the_requested_streamed_resource_range() {
        let filesystem = filesystem_with_resource();
        let mut source_stream = filesystem.open_file("/bundle/shared.resS").unwrap();
        source_stream.seek(SeekFrom::Start(7)).unwrap();

        let bytes = read_streamed_resource_bytes(
            &filesystem,
            "/bundle/shared.resS",
            3,
            4,
            StreamedResourceLimits::default(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(bytes, b"3456");
        assert_eq!(source_stream.stream_position().unwrap(), 7);
        assert!(
            check_streamed_resource_integrity(&filesystem, "/bundle/shared.resS", 3, 4).unwrap()
        );
    }

    #[test]
    fn reads_streamed_resources_from_contiguous_split_files() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/bundle")).unwrap();
        filesystem
            .write_all_bytes("/bundle/chunk.resS.split0", b"012")
            .unwrap();
        filesystem
            .write_all_bytes("/bundle/chunk.resS.split1", b"345")
            .unwrap();

        assert_eq!(
            read_streamed_resource_bytes(
                &filesystem,
                "/bundle/chunk.resS",
                2,
                3,
                StreamedResourceLimits::default(),
            )
            .unwrap(),
            Some(b"234".to_vec())
        );
        assert!(
            check_streamed_resource_integrity(&filesystem, "/bundle/chunk.resS", 2, 3).unwrap()
        );
    }

    #[test]
    fn opens_an_independently_seekable_resource_range() {
        let filesystem = filesystem_with_resource();
        let mut range = open_streamed_resource_range(
            &filesystem,
            "/bundle/shared.resS",
            2,
            5,
            StreamedResourceLimits::default(),
        )
        .unwrap()
        .unwrap();
        range.seek(SeekFrom::Start(1)).unwrap();
        let mut bytes = [0; 2];
        range.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"34");
    }

    #[test]
    fn empty_resource_path_is_absent_and_integrity_compatible() {
        let filesystem = VirtualFileSystem::new();
        assert!(open_streamed_resource_range(
            &filesystem,
            "",
            0,
            0,
            StreamedResourceLimits::default()
        )
        .unwrap()
        .is_none());
        assert!(read_streamed_resource_bytes(
            &filesystem,
            "",
            0,
            0,
            StreamedResourceLimits::default()
        )
        .unwrap()
        .is_none());
        assert!(check_streamed_resource_integrity(&filesystem, "", u64::MAX, 0).unwrap());
    }

    #[test]
    fn rejects_zero_length_overflow_missing_and_out_of_bounds_ranges() {
        let filesystem = filesystem_with_resource();
        assert!(matches!(
            open_streamed_resource_range(
                &filesystem,
                "/bundle/shared.resS",
                0,
                0,
                StreamedResourceLimits::default()
            ),
            Err(StreamedResourceError::InvalidRange {
                reason: InvalidStreamedResourceRange::ZeroLength,
                ..
            })
        ));
        assert!(matches!(
            open_streamed_resource_range(
                &filesystem,
                "/bundle/shared.resS",
                u64::MAX,
                2,
                StreamedResourceLimits::default()
            ),
            Err(StreamedResourceError::InvalidRange { .. })
        ));
        assert!(matches!(
            open_streamed_resource_range(
                &filesystem,
                "/bundle/missing.resS",
                0,
                1,
                StreamedResourceLimits::default()
            ),
            Err(StreamedResourceError::ResourceNotFound { .. })
        ));
        assert!(matches!(
            open_streamed_resource_range(
                &filesystem,
                "/bundle/shared.resS",
                9,
                2,
                StreamedResourceLimits::default()
            ),
            Err(StreamedResourceError::RangeOutsideResource {
                resource_size: 10,
                ..
            })
        ));
        assert!(
            !check_streamed_resource_integrity(&filesystem, "/bundle/missing.resS", 0, 1).unwrap()
        );
        assert!(
            !check_streamed_resource_integrity(&filesystem, "/bundle/shared.resS", 0, 0).unwrap()
        );
    }

    #[test]
    fn check_integrity_rejects_short_and_huge_ranges_like_the_baseline() {
        // Edge outcomes from a direct runtime differential over the real
        // `StreamingInfo` extensions against a 10-byte resource: a range past the
        // end fails, and an offset outside the signed stream range fails. Where the
        // overflow-checked baseline throws `OverflowException`, this port returns
        // `InvalidRange`; both reject loudly instead of yielding a short buffer.
        let filesystem = filesystem_with_resource();
        assert!(
            !check_streamed_resource_integrity(&filesystem, "/bundle/shared.resS", 8, 4).unwrap(),
            "range past the end fails, like baseline `GetContent` returning null"
        );
        assert!(
            !check_streamed_resource_integrity(
                &filesystem,
                "/bundle/shared.resS",
                (i64::MAX as u64) + 1,
                4,
            )
            .unwrap(),
            "offset outside the signed range fails, like the baseline bounds check"
        );
    }

    #[test]
    fn enforces_memory_copy_and_materialized_output_limits() {
        let filesystem = filesystem_with_resource();
        let limits = StreamedResourceLimits {
            max_range_copy_bytes: 3,
            max_materialized_bytes: 4,
        };
        assert!(matches!(
            open_streamed_resource_range(&filesystem, "/bundle/shared.resS", 0, 4, limits),
            Err(StreamedResourceError::RangeCopyLimitExceeded { limit: 3, .. })
        ));
        assert!(matches!(
            read_streamed_resource_bytes(&filesystem, "/bundle/shared.resS", 0, 5, limits),
            Err(StreamedResourceError::MaterializationLimitExceeded { limit: 4, .. })
        ));
    }

    #[test]
    fn default_resource_files_match_the_source_extensions() {
        use super::is_default_resource_file;
        assert!(is_default_resource_file("shared.ress"));
        assert!(is_default_resource_file("shared.resource"));
        assert!(is_default_resource_file("SHARED.RESS"));
        assert!(is_default_resource_file("archive:/textures/shared.ResS"));
        assert!(!is_default_resource_file("shared.assets"));
        assert!(!is_default_resource_file("shared"));
        assert!(!is_default_resource_file("shared.ress.txt"));
        assert!(!is_default_resource_file(""));
    }

    /// The export-time value operations (`StreamingInfoExtensions`): snapshot the values,
    /// clear them for the inline export, then restore them exactly, as
    /// `YamlStreamedAssetExportCollection` does around `ExportInner`.
    #[test]
    fn streaming_info_values_clear_and_restore_around_exports() {
        use super::StreamedResourceInfo;
        let mut info = StreamedResourceInfo {
            path: String::from("archive:/level0"),
            offset: 128,
            size: 64,
        };
        assert!(info.is_set());
        let snapshot = (info.path.clone(), info.offset, info.size);

        info.clear_values();
        assert!(!info.is_set());
        assert_eq!(info.offset, 0);
        assert_eq!(info.size, 0);

        info.set_values(snapshot.0.clone(), snapshot.1, snapshot.2);
        assert!(info.is_set());
        assert_eq!(info.path, snapshot.0);
        assert_eq!(info.offset, snapshot.1);
        assert_eq!(info.size, snapshot.2);
    }
}
