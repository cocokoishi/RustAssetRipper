//! Bounded access to byte ranges stored in Unity ResourceFiles.

use std::fmt;
use std::io;

use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::{
    PlatformResourceLookup, PlatformResourceLookupError, PlatformResourceProvider,
    PlatformResourceSource, PlatformStructureLimits,
};
use crate::io::stream::SmartStream;
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};
use crate::serialized_file::workspace::{SerializedFileWorkspace, WorkspaceResourceLookupError};

const UNITY_STREAM_OFFSET_MAX: u64 = i64::MAX as u64;

/// Limits for streamed ResourceFile reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamedResourceLimits {
    /// Maximum range copied when the source stream cannot provide a zero-copy file range.
    pub max_range_copy_bytes: usize,
    /// Maximum byte count returned by [`read_streamed_resource_bytes`].
    pub max_materialized_bytes: usize,
}

/// A ResourceFile selected from a mounted bundle or an external game DataPath.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedResourceFile<'a> {
    Bundled {
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

/// Errors from the ordered bundle-scope and external-provider ResourceFile lookup.
#[derive(Debug)]
pub enum ResourceFileResolveError {
    Bundle(WorkspaceResourceLookupError),
    ExternalProvider(PlatformResourceLookupError),
}

impl fmt::Display for ResourceFileResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundle(source) => {
                write!(formatter, "bundle ResourceFile lookup failed: {source}")
            }
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
            Self::ExternalProvider(source) => Some(source),
        }
    }
}

/// The path, byte offset, and byte count decoded from a Unity `StreamingInfo` value.
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
    let bundled_resource_index = workspace
        .resolve_bundled_resource_file(source_bundle_index, original_name)
        .map_err(ResourceFileResolveError::Bundle)?;
    if let Some(resource_index) = bundled_resource_index {
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
        return Ok(ResolvedResourceFile::Bundled {
            resource_index,
            virtual_path: &resource.virtual_path,
        });
    }

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

impl Default for StreamedResourceLimits {
    fn default() -> Self {
        Self {
            max_range_copy_bytes: 512 * 1024 * 1024,
            max_materialized_bytes: 512 * 1024 * 1024,
        }
    }
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
        open_streamed_resource_range, read_streamed_resource_bytes,
        read_streamed_resource_info_bytes, InvalidStreamedResourceRange, StreamedResourceError,
        StreamedResourceInfo, StreamedResourceInfoError, StreamedResourceLimits,
    };
    use crate::io::filesystem::VirtualFileSystem;
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

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
}
