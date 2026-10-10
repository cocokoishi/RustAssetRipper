use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::Endian;
use crate::serialized_file::format_version::FormatVersion;
use crate::serialized_file::metadata::{FileIdentifier, ObjectInfo};
use crate::serialized_file::object::{ObjectEncodeError, ObjectEncodeLimits, SerializedValue};
#[derive(Clone, Copy)]
pub(super) struct MetadataInsertionContext {
    pub(super) version: FormatVersion,
    pub(super) endian: Endian,
    pub(super) metadata_file_offset: usize,
    pub(super) output_limit: usize,
}

#[derive(Clone, Copy)]
pub(super) struct MetadataInsertions<'a> {
    pub(super) objects: &'a [SerializedObjectInsertion],
    pub(super) external_files: &'a [SerializedExternalFileInsertion],
}

/// Bounds for rebuilding one complete SerializedFile after object payload edits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SerializedFileRewriteLimits {
    pub max_output_file_bytes: usize,
    pub max_objects: usize,
    pub max_replacements: usize,
    pub max_insertions: usize,
    pub max_external_file_insertions: usize,
    pub object_payload_limits: ObjectEncodeLimits,
}

impl Default for SerializedFileRewriteLimits {
    fn default() -> Self {
        Self {
            max_output_file_bytes: 2 * 1024 * 1024 * 1024,
            max_objects: 5_000_000,
            max_replacements: 1_000_000,
            max_insertions: 1_000_000,
            max_external_file_insertions: 1_000_000,
            object_payload_limits: ObjectEncodeLimits::default(),
        }
    }
}

/// A new serialized object that uses an existing type-table entry.
///
/// `ObjectInfo.byte_start` and `byte_size` are ignored and computed by the writer. The remaining
/// serialized identity fields must match the active file-generation layout.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedObjectInsertion {
    pub object: ObjectInfo,
    pub root: SerializedValue,
    pub trailing_bytes: Vec<u8>,
}

/// One new external SerializedFile dependency record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedExternalFileInsertion {
    pub file_identifier: FileIdentifier,
}

/// Error produced while rebuilding a SerializedFile around edited object payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedFileRewriteError {
    object_index: Option<usize>,
    kind: SerializedFileRewriteErrorKind,
}

impl SerializedFileRewriteError {
    pub(super) fn new(object_index: Option<usize>, kind: SerializedFileRewriteErrorKind) -> Self {
        Self { object_index, kind }
    }

    pub fn object_index(&self) -> Option<usize> {
        self.object_index
    }

    pub fn kind(&self) -> &SerializedFileRewriteErrorKind {
        &self.kind
    }
}

impl Display for SerializedFileRewriteError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(object_index) = self.object_index {
            write!(
                f,
                "could not rewrite SerializedFile object {object_index}: "
            )?;
        } else {
            f.write_str("could not rewrite SerializedFile: ")?;
        }
        Display::fmt(&self.kind, f)
    }
}

impl Error for SerializedFileRewriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            SerializedFileRewriteErrorKind::ObjectEncode(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedFileRewriteErrorKind {
    ObjectLimitExceeded {
        requested: usize,
        limit: usize,
    },
    ReplacementLimitExceeded {
        requested: usize,
        limit: usize,
    },
    InsertionLimitExceeded {
        requested: usize,
        limit: usize,
    },
    ReplacementIndexOutOfRange {
        object_count: usize,
    },
    ReplacementObjectIndexMismatch {
        replacement_index: usize,
    },
    DuplicateReplacement,
    DuplicateObjectPathId {
        path_id: i64,
    },
    InsertionTypeUnresolved,
    InsertionTypeIndexOutOfRange {
        type_index: usize,
        type_count: usize,
    },
    InsertionTypeMismatch {
        field: &'static str,
    },
    InsertionPathIdUnrepresentable {
        path_id: i64,
        width: usize,
    },
    ExternalFileInsertionLimitExceeded {
        requested: usize,
        limit: usize,
    },
    ExternalFileCountLimitExceeded {
        requested: usize,
        limit: usize,
    },
    ExternalFileCountMismatch {
        metadata: usize,
        serialized: i32,
    },
    InvalidExternalFileTableLayout {
        count_field_offset: usize,
        records_start_offset: usize,
        records_end_offset: usize,
        metadata_size: usize,
    },
    ExternalFileFieldMismatch {
        field: &'static str,
    },
    ExternalFileStringContainsNul {
        field: &'static str,
    },
    ObjectRecordCountMismatch {
        objects: usize,
        layouts: usize,
    },
    InvalidObjectRecordTableLayout {
        count_field_offset: usize,
        records_start_offset: usize,
        records_end_offset: usize,
        metadata_size: usize,
    },
    ObjectRecordFieldOutsideTable {
        field_offset: usize,
        field_width: usize,
        records_start_offset: usize,
        records_end_offset: usize,
    },
    ObjectRecordIdentityMismatch {
        record_path_id: i64,
        metadata_path_id: i64,
    },
    InvalidMetadataRange {
        start: usize,
        end: usize,
        input_size: usize,
    },
    InvalidObjectRange {
        start: u64,
        end: u64,
        data_start: u64,
        data_end: u64,
    },
    InvalidMetadataFieldRange {
        field_offset: usize,
        field_width: usize,
        metadata_size: usize,
    },
    HeaderLayoutMismatch {
        expected: usize,
        actual: usize,
    },
    ObjectOffsetUnrepresentable {
        offset: u64,
        width: usize,
    },
    ObjectSizeUnrepresentable {
        size: usize,
    },
    MetadataSizeUnrepresentable {
        size: usize,
        width: usize,
    },
    FileSizeUnrepresentable {
        size: usize,
        width: usize,
    },
    DataOffsetUnrepresentable {
        offset: usize,
        width: usize,
    },
    OutputLimitExceeded {
        requested: usize,
        limit: usize,
    },
    LengthOverflow {
        resource: &'static str,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    ObjectEncode(ObjectEncodeError),
}

impl Display for SerializedFileRewriteErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectLimitExceeded { requested, limit } => {
                write!(f, "object count {requested} exceeds the limit of {limit}")
            }
            Self::ReplacementLimitExceeded { requested, limit } => write!(
                f,
                "replacement count {requested} exceeds the limit of {limit}"
            ),
            Self::InsertionLimitExceeded { requested, limit } => write!(
                f,
                "insertion count {requested} exceeds the limit of {limit}"
            ),
            Self::ReplacementIndexOutOfRange { object_count } => {
                write!(f, "replacement index is outside the {object_count}-object table")
            }
            Self::ReplacementObjectIndexMismatch { replacement_index } => write!(
                f,
                "replacement for table index {replacement_index} carries a different object index"
            ),
            Self::DuplicateReplacement => f.write_str("replacement list contains a duplicate object index"),
            Self::DuplicateObjectPathId { path_id } => {
                write!(f, "object PathID {path_id} is duplicated")
            }
            Self::InsertionTypeUnresolved => f.write_str("inserted object type is unresolved"),
            Self::InsertionTypeIndexOutOfRange { type_index, type_count } => write!(
                f,
                "inserted object type index {type_index} is outside the {type_count}-entry type table"
            ),
            Self::InsertionTypeMismatch { field } => {
                write!(f, "inserted object's {field} does not match the file type table")
            }
            Self::InsertionPathIdUnrepresentable { path_id, width } => write!(
                f,
                "inserted PathID {path_id} does not fit the {width}-byte field"
            ),
            Self::ExternalFileInsertionLimitExceeded { requested, limit } => write!(
                f,
                "external-file insertion count {requested} exceeds the limit of {limit}"
            ),
            Self::ExternalFileCountLimitExceeded { requested, limit } => write!(
                f,
                "external-file count {requested} exceeds the limit of {limit}"
            ),
            Self::ExternalFileCountMismatch { metadata, serialized } => write!(
                f,
                "metadata has {metadata} external files but the serialized count is {serialized}"
            ),
            Self::InvalidExternalFileTableLayout {
                count_field_offset,
                records_start_offset,
                records_end_offset,
                metadata_size,
            } => write!(
                f,
                "external-file count at {count_field_offset} and record range {records_start_offset}..{records_end_offset} are inconsistent with {metadata_size}-byte metadata"
            ),
            Self::ExternalFileFieldMismatch { field } => {
                write!(f, "external-file {field} does not match the format generation")
            }
            Self::ExternalFileStringContainsNul { field } => {
                write!(f, "external-file {field} contains an embedded NUL byte")
            }
            Self::ObjectRecordCountMismatch { objects, layouts } => write!(
                f,
                "metadata has {objects} objects but the parsed input has {layouts} object-record layouts"
            ),
            Self::InvalidObjectRecordTableLayout {
                count_field_offset,
                records_start_offset,
                records_end_offset,
                metadata_size,
            } => write!(
                f,
                "object count at {count_field_offset} and record range {records_start_offset}..{records_end_offset} are inconsistent with {metadata_size}-byte metadata"
            ),
            Self::ObjectRecordFieldOutsideTable {
                field_offset,
                field_width,
                records_start_offset,
                records_end_offset,
            } => write!(
                f,
                "object-record field {field_offset}..{} is outside record range {records_start_offset}..{records_end_offset}",
                field_offset.saturating_add(*field_width)
            ),
            Self::ObjectRecordIdentityMismatch { record_path_id, metadata_path_id } => write!(
                f,
                "parsed record PathID {record_path_id} does not match metadata PathID {metadata_path_id}"
            ),
            Self::InvalidMetadataRange { start, end, input_size } => write!(
                f,
                "metadata range {start}..{end} exceeds the {input_size}-byte source file"
            ),
            Self::InvalidObjectRange { start, end, data_start, data_end } => write!(
                f,
                "source object range {start}..{end} is outside the data section {data_start}..{data_end}"
            ),
            Self::InvalidMetadataFieldRange { field_offset, field_width, metadata_size } => write!(
                f,
                "object-record field {field_offset}..{} is outside {metadata_size}-byte metadata",
                field_offset.saturating_add(*field_width)
            ),
            Self::HeaderLayoutMismatch { expected, actual } => write!(
                f,
                "format generation requires a {expected}-byte header, parsed header has {actual} bytes"
            ),
            Self::ObjectOffsetUnrepresentable { offset, width } => write!(
                f,
                "object data offset {offset} does not fit the {width}-byte field"
            ),
            Self::ObjectSizeUnrepresentable { size } => {
                write!(f, "object payload size {size} does not fit its signed 32-bit field")
            }
            Self::MetadataSizeUnrepresentable { size, width } => write!(
                f,
                "metadata size {size} does not fit the {width}-byte header field"
            ),
            Self::FileSizeUnrepresentable { size, width } => {
                write!(f, "file size {size} does not fit the {width}-byte header field")
            }
            Self::DataOffsetUnrepresentable { offset, width } => write!(
                f,
                "data offset {offset} does not fit the {width}-byte header field"
            ),
            Self::OutputLimitExceeded { requested, limit } => write!(
                f,
                "output would contain {requested} bytes, exceeding the limit of {limit}"
            ),
            Self::LengthOverflow { resource } => write!(f, "{resource} length overflowed"),
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not reserve {requested} bytes for {resource}")
            }
            Self::ObjectEncode(error) => Display::fmt(error, f),
        }
    }
}
