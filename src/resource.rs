//! Bounded access to byte ranges stored in Unity ResourceFiles.

use std::fmt;
use std::io;

use crate::io::filesystem::VirtualFileSystem;
use crate::io::stream::SmartStream;

const UNITY_STREAM_OFFSET_MAX: u64 = i64::MAX as u64;

/// Limits for streamed ResourceFile reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamedResourceLimits {
    /// Maximum range copied when the source stream cannot provide a zero-copy file range.
    pub max_range_copy_bytes: usize,
    /// Maximum byte count returned by [`read_streamed_resource_bytes`].
    pub max_materialized_bytes: usize,
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
    filesystem.open_file(path).map_err(|error| {
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
        check_streamed_resource_integrity, open_streamed_resource_range,
        read_streamed_resource_bytes, InvalidStreamedResourceRange, StreamedResourceError,
        StreamedResourceLimits,
    };
    use crate::io::filesystem::VirtualFileSystem;

    fn filesystem_with_resource() -> VirtualFileSystem {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/bundle")).unwrap();
        filesystem
            .write_all_bytes("/bundle/shared.resS", b"0123456789")
            .unwrap();
        filesystem
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
