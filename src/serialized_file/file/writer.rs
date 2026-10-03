use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::Endian;

use super::super::format_version::FormatVersion;
use super::super::metadata::{
    ObjectInfo, ObjectRecordLayout, ObjectRecordTableLayout, SerializedType,
};
use super::super::object::{
    ObjectEncodeError, ObjectEncodeLimits, ParsedObjectPayload, SerializedValue,
};
use super::SerializedFile;

const OBJECT_ALIGNMENT: usize = 8;
const DATA_ALIGNMENT: usize = 16;
const MAX_HEADER_SIZE: usize = 48;

#[derive(Clone, Copy)]
struct MetadataInsertionContext {
    version: FormatVersion,
    endian: Endian,
    metadata_file_offset: usize,
    output_limit: usize,
}

/// Bounds for rebuilding one complete SerializedFile after object payload edits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SerializedFileRewriteLimits {
    pub max_output_file_bytes: usize,
    pub max_objects: usize,
    pub max_replacements: usize,
    pub max_insertions: usize,
    pub object_payload_limits: ObjectEncodeLimits,
}

impl Default for SerializedFileRewriteLimits {
    fn default() -> Self {
        Self {
            max_output_file_bytes: 2 * 1024 * 1024 * 1024,
            max_objects: 5_000_000,
            max_replacements: 1_000_000,
            max_insertions: 1_000_000,
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

/// Error produced while rebuilding a SerializedFile around edited object payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedFileRewriteError {
    object_index: Option<usize>,
    kind: SerializedFileRewriteErrorKind,
}

impl SerializedFileRewriteError {
    fn new(object_index: Option<usize>, kind: SerializedFileRewriteErrorKind) -> Self {
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

impl SerializedFile<'_> {
    /// Rebuild the complete file with zero or more replaced object payloads.
    ///
    /// Original metadata bytes and non-object metadata records are retained. Object records,
    /// data-section offsets, versioned headers, and alignment padding are updated for the new
    /// payload lengths. With an empty replacement list, the original bytes are copied unchanged.
    pub fn rewrite_object_payloads(
        &self,
        replacements: &[(usize, ParsedObjectPayload)],
        limits: SerializedFileRewriteLimits,
    ) -> Result<Vec<u8>, SerializedFileRewriteError> {
        self.rewrite_with_insertions(replacements, &[], limits)
    }

    /// Rebuild the file with object payload replacements and new records using existing types.
    ///
    /// Inserted objects are appended after the original object table and their payloads are
    /// placed after all original payloads. Metadata after the object table is preserved exactly.
    pub fn rewrite_with_insertions(
        &self,
        replacements: &[(usize, ParsedObjectPayload)],
        insertions: &[SerializedObjectInsertion],
        limits: SerializedFileRewriteLimits,
    ) -> Result<Vec<u8>, SerializedFileRewriteError> {
        let total_object_count = self
            .metadata
            .objects
            .len()
            .checked_add(insertions.len())
            .ok_or_else(|| {
                rewrite_error(
                    None,
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "rewritten object count",
                    },
                )
            })?;
        if total_object_count > limits.max_objects {
            return Err(SerializedFileRewriteError::new(
                None,
                SerializedFileRewriteErrorKind::ObjectLimitExceeded {
                    requested: total_object_count,
                    limit: limits.max_objects,
                },
            ));
        }
        if replacements.len() > limits.max_replacements {
            return Err(SerializedFileRewriteError::new(
                None,
                SerializedFileRewriteErrorKind::ReplacementLimitExceeded {
                    requested: replacements.len(),
                    limit: limits.max_replacements,
                },
            ));
        }
        if insertions.len() > limits.max_insertions {
            return Err(SerializedFileRewriteError::new(
                None,
                SerializedFileRewriteErrorKind::InsertionLimitExceeded {
                    requested: insertions.len(),
                    limit: limits.max_insertions,
                },
            ));
        }
        if total_object_count > i32::MAX as usize {
            return Err(SerializedFileRewriteError::new(
                None,
                SerializedFileRewriteErrorKind::ObjectLimitExceeded {
                    requested: total_object_count,
                    limit: i32::MAX as usize,
                },
            ));
        }
        if replacements.is_empty() && insertions.is_empty() {
            return copy_input(self.input(), limits.max_output_file_bytes);
        }

        if self.metadata.objects.len() != self.object_record_layout.records.len() {
            return Err(SerializedFileRewriteError::new(
                None,
                SerializedFileRewriteErrorKind::ObjectRecordCountMismatch {
                    objects: self.metadata.objects.len(),
                    layouts: self.object_record_layout.records.len(),
                },
            ));
        }
        for (object, layout) in self
            .metadata
            .objects
            .iter()
            .zip(&self.object_record_layout.records)
        {
            if object.path_id != layout.path_id {
                return Err(SerializedFileRewriteError::new(
                    None,
                    SerializedFileRewriteErrorKind::ObjectRecordIdentityMismatch {
                        record_path_id: layout.path_id,
                        metadata_path_id: object.path_id,
                    },
                ));
            }
        }

        let version = self.header.format_version;
        if !insertions.is_empty() {
            let mut path_ids = Vec::new();
            path_ids
                .try_reserve_exact(total_object_count)
                .map_err(|_| allocation_error("object PathID index", total_object_count))?;
            path_ids.extend(self.metadata.objects.iter().map(|object| object.path_id));
            for (insertion_index, insertion) in insertions.iter().enumerate() {
                validate_insertion_type(
                    insertion,
                    &self.metadata.types,
                    version,
                    self.metadata.objects.len() + insertion_index,
                )?;
                path_ids.push(insertion.object.path_id);
            }
            path_ids.sort_unstable();
            if let Some(duplicate) = path_ids.windows(2).find(|pair| pair[0] == pair[1]) {
                return Err(rewrite_error(
                    None,
                    SerializedFileRewriteErrorKind::DuplicateObjectPathId {
                        path_id: duplicate[0],
                    },
                ));
            }
        }

        let mut ordered_replacements = Vec::new();
        ordered_replacements
            .try_reserve_exact(replacements.len())
            .map_err(|_| allocation_error("replacement index", replacements.len()))?;
        for (replacement_index, (object_index, payload)) in replacements.iter().enumerate() {
            if *object_index >= self.metadata.objects.len() {
                return Err(SerializedFileRewriteError::new(
                    Some(*object_index),
                    SerializedFileRewriteErrorKind::ReplacementIndexOutOfRange {
                        object_count: self.metadata.objects.len(),
                    },
                ));
            }
            if payload.object_index != *object_index {
                return Err(SerializedFileRewriteError::new(
                    Some(*object_index),
                    SerializedFileRewriteErrorKind::ReplacementObjectIndexMismatch {
                        replacement_index,
                    },
                ));
            }
            ordered_replacements.push((*object_index, payload));
        }
        ordered_replacements.sort_unstable_by_key(|(object_index, _)| *object_index);
        if ordered_replacements
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0)
        {
            return Err(SerializedFileRewriteError::new(
                None,
                SerializedFileRewriteErrorKind::DuplicateReplacement,
            ));
        }

        let expected_header_size = serialized_header_size(version);
        if self.header.header_size != expected_header_size {
            return Err(SerializedFileRewriteError::new(
                None,
                SerializedFileRewriteErrorKind::HeaderLayoutMismatch {
                    expected: expected_header_size,
                    actual: self.header.header_size,
                },
            ));
        }
        let metadata_start = self.metadata.prefix.metadata_offset;
        let metadata_size = usize::try_from(self.header.metadata_size).map_err(|_| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::LengthOverflow {
                    resource: "metadata size",
                },
            )
        })?;
        let metadata_end = metadata_start.checked_add(metadata_size).ok_or_else(|| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::LengthOverflow {
                    resource: "metadata end offset",
                },
            )
        })?;
        if metadata_end > self.input().len()
            || (version.metadata_at_end() && metadata_end != self.input().len())
            || (!version.metadata_at_end() && metadata_start != self.header.header_size)
        {
            return Err(rewrite_error(
                None,
                SerializedFileRewriteErrorKind::InvalidMetadataRange {
                    start: metadata_start,
                    end: metadata_end,
                    input_size: self.input().len(),
                },
            ));
        }
        let metadata_bytes = self
            .input()
            .get(metadata_start..metadata_end)
            .ok_or_else(|| {
                rewrite_error(
                    None,
                    SerializedFileRewriteErrorKind::InvalidMetadataRange {
                        start: metadata_start,
                        end: metadata_end,
                        input_size: self.input().len(),
                    },
                )
            })?;
        validate_object_record_table_layout(
            &self.object_record_layout,
            self.metadata.objects.len(),
            metadata_bytes,
            version,
            self.metadata.prefix.body_endian,
        )?;
        let (rewritten_metadata_owned, object_record_layouts) = if insertions.is_empty() {
            let mut layouts = Vec::new();
            layouts
                .try_reserve_exact(self.object_record_layout.records.len())
                .map_err(|_| {
                    allocation_error(
                        "rewritten object record layouts",
                        self.object_record_layout.records.len(),
                    )
                })?;
            layouts.extend_from_slice(&self.object_record_layout.records);
            (None, layouts)
        } else {
            let (metadata, layouts) = build_metadata_with_insertions(
                metadata_bytes,
                &self.object_record_layout,
                self.metadata.objects.len(),
                insertions,
                MetadataInsertionContext {
                    version,
                    endian: self.metadata.prefix.body_endian,
                    metadata_file_offset: metadata_start,
                    output_limit: limits.max_output_file_bytes,
                },
            )?;
            (Some(metadata), layouts)
        };
        let rewritten_metadata = rewritten_metadata_owned
            .as_deref()
            .unwrap_or(metadata_bytes);
        let input_data_start = self.header.data_offset;
        let input_data_end = if version.metadata_at_end() {
            u64::try_from(metadata_start).map_err(|_| {
                rewrite_error(
                    None,
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "input metadata start",
                    },
                )
            })?
        } else {
            u64::try_from(self.input().len()).map_err(|_| {
                rewrite_error(
                    None,
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "input file size",
                    },
                )
            })?
        };

        let mut output = Vec::new();
        let initial_capacity = self
            .input()
            .len()
            .min(limits.max_output_file_bytes)
            .min(1024 * 1024);
        reserve_output(
            &mut output,
            initial_capacity,
            limits.max_output_file_bytes,
            None,
        )?;
        const EMPTY_HEADER: [u8; MAX_HEADER_SIZE] = [0; MAX_HEADER_SIZE];
        append_output(
            &mut output,
            &EMPTY_HEADER[..expected_header_size],
            limits.max_output_file_bytes,
            None,
        )?;
        let metadata_output_offset = if version.metadata_at_end() {
            None
        } else {
            append_output(
                &mut output,
                rewritten_metadata,
                limits.max_output_file_bytes,
                None,
            )?;
            Some(expected_header_size)
        };

        align_output(
            &mut output,
            DATA_ALIGNMENT,
            limits.max_output_file_bytes,
            None,
        )?;
        let output_data_start = output.len();
        let mut object_records = Vec::new();
        object_records
            .try_reserve_exact(total_object_count)
            .map_err(|_| allocation_error("rewritten object record index", total_object_count))?;

        let mut replacement_cursor = 0;
        for (object_index, object) in self.metadata.objects.iter().enumerate() {
            let replacement = ordered_replacements
                .get(replacement_cursor)
                .filter(|(replacement_index, _)| *replacement_index == object_index)
                .map(|(_, payload)| {
                    replacement_cursor += 1;
                    *payload
                });
            let encoded_replacement;
            let payload_bytes = if let Some(payload) = replacement {
                encoded_replacement = self
                    .metadata
                    .encode_object_payload(payload, limits.object_payload_limits)
                    .map_err(|error| {
                        rewrite_error(
                            Some(object_index),
                            SerializedFileRewriteErrorKind::ObjectEncode(error),
                        )
                    })?;
                encoded_replacement.as_slice()
            } else {
                let range = object.data_range(&self.header).ok_or_else(|| {
                    rewrite_error(
                        Some(object_index),
                        SerializedFileRewriteErrorKind::LengthOverflow {
                            resource: "source object range",
                        },
                    )
                })?;
                if range.start < input_data_start || range.end > input_data_end {
                    return Err(rewrite_error(
                        Some(object_index),
                        SerializedFileRewriteErrorKind::InvalidObjectRange {
                            start: range.start,
                            end: range.end,
                            data_start: input_data_start,
                            data_end: input_data_end,
                        },
                    ));
                }
                let start = usize::try_from(range.start).map_err(|_| {
                    rewrite_error(
                        Some(object_index),
                        SerializedFileRewriteErrorKind::LengthOverflow {
                            resource: "source object start",
                        },
                    )
                })?;
                let end = usize::try_from(range.end).map_err(|_| {
                    rewrite_error(
                        Some(object_index),
                        SerializedFileRewriteErrorKind::LengthOverflow {
                            resource: "source object end",
                        },
                    )
                })?;
                self.input().get(start..end).ok_or_else(|| {
                    rewrite_error(
                        Some(object_index),
                        SerializedFileRewriteErrorKind::InvalidObjectRange {
                            start: range.start,
                            end: range.end,
                            data_start: input_data_start,
                            data_end: input_data_end,
                        },
                    )
                })?
            };

            let object_offset = output.len().checked_sub(output_data_start).ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "rewritten object offset",
                    },
                )
            })?;
            let object_offset = u64::try_from(object_offset).map_err(|_| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "rewritten object offset",
                    },
                )
            })?;
            let object_size = i32::try_from(payload_bytes.len()).map_err(|_| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::ObjectSizeUnrepresentable {
                        size: payload_bytes.len(),
                    },
                )
            })?;
            let offset_width = if version.has_large_file_support() {
                8
            } else {
                4
            };
            if (offset_width == 4 && u32::try_from(object_offset).is_err())
                || (offset_width == 8 && i64::try_from(object_offset).is_err())
            {
                return Err(rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::ObjectOffsetUnrepresentable {
                        offset: object_offset,
                        width: offset_width,
                    },
                ));
            }
            object_records.push((object_offset, object_size));
            append_output(
                &mut output,
                payload_bytes,
                limits.max_output_file_bytes,
                Some(object_index),
            )?;
            align_output(
                &mut output,
                OBJECT_ALIGNMENT,
                limits.max_output_file_bytes,
                Some(object_index),
            )?;
        }

        for (insertion_index, insertion) in insertions.iter().enumerate() {
            let object_index = self.metadata.objects.len() + insertion_index;
            let type_index = insertion.object.resolved_type_index.ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::InsertionTypeUnresolved,
                )
            })?;
            let payload_bytes = self
                .metadata
                .encode_object_payload_for_type(
                    object_index,
                    type_index,
                    &insertion.root,
                    &insertion.trailing_bytes,
                    limits.object_payload_limits,
                )
                .map_err(|error| {
                    rewrite_error(
                        Some(object_index),
                        SerializedFileRewriteErrorKind::ObjectEncode(error),
                    )
                })?;
            let object_offset = output.len().checked_sub(output_data_start).ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "inserted object offset",
                    },
                )
            })?;
            let object_offset = u64::try_from(object_offset).map_err(|_| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "inserted object offset",
                    },
                )
            })?;
            let object_size = i32::try_from(payload_bytes.len()).map_err(|_| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::ObjectSizeUnrepresentable {
                        size: payload_bytes.len(),
                    },
                )
            })?;
            let offset_width = if version.has_large_file_support() {
                8
            } else {
                4
            };
            if (offset_width == 4 && u32::try_from(object_offset).is_err())
                || (offset_width == 8 && i64::try_from(object_offset).is_err())
            {
                return Err(rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::ObjectOffsetUnrepresentable {
                        offset: object_offset,
                        width: offset_width,
                    },
                ));
            }
            object_records.push((object_offset, object_size));
            append_output(
                &mut output,
                &payload_bytes,
                limits.max_output_file_bytes,
                Some(object_index),
            )?;
            align_output(
                &mut output,
                OBJECT_ALIGNMENT,
                limits.max_output_file_bytes,
                Some(object_index),
            )?;
        }

        let metadata_output_offset = if let Some(offset) = metadata_output_offset {
            offset
        } else {
            let offset = output.len();
            append_output(
                &mut output,
                rewritten_metadata,
                limits.max_output_file_bytes,
                None,
            )?;
            offset
        };
        patch_object_records(
            &mut output,
            metadata_output_offset,
            rewritten_metadata.len(),
            &object_record_layouts,
            &object_records,
            version,
            self.metadata.prefix.body_endian,
        )?;

        let file_size = output.len();
        patch_header(
            &mut output,
            self.header.format_version,
            self.metadata.prefix.body_endian,
            rewritten_metadata.len(),
            file_size,
            output_data_start,
            expected_header_size,
        )?;
        Ok(output)
    }
}

fn validate_insertion_type(
    insertion: &SerializedObjectInsertion,
    types: &[SerializedType],
    version: FormatVersion,
    object_index: usize,
) -> Result<(), SerializedFileRewriteError> {
    let object = &insertion.object;
    if version.raw() < 14 && i32::try_from(object.path_id).is_err() {
        return Err(rewrite_error(
            Some(object_index),
            SerializedFileRewriteErrorKind::InsertionPathIdUnrepresentable {
                path_id: object.path_id,
                width: 4,
            },
        ));
    }
    let type_index = object.resolved_type_index.ok_or_else(|| {
        rewrite_error(
            Some(object_index),
            SerializedFileRewriteErrorKind::InsertionTypeUnresolved,
        )
    })?;
    let serialized_type = types.get(type_index).ok_or_else(|| {
        rewrite_error(
            Some(object_index),
            SerializedFileRewriteErrorKind::InsertionTypeIndexOutOfRange {
                type_index,
                type_count: types.len(),
            },
        )
    })?;
    let mismatch = |field| {
        rewrite_error(
            Some(object_index),
            SerializedFileRewriteErrorKind::InsertionTypeMismatch { field },
        )
    };
    if object.raw_type_id != serialized_type.raw_type_id {
        return Err(mismatch("raw type ID"));
    }
    if version.raw() >= 17 {
        let serialized_type_index =
            i32::try_from(type_index).map_err(|_| mismatch("type index"))?;
        if object.serialized_type_index != Some(serialized_type_index) {
            return Err(mismatch("serialized type index"));
        }
    } else if object.serialized_type_index.is_some() {
        return Err(mismatch("serialized type index"));
    }
    if (version.raw() < 16) != object.class_id.is_some() {
        return Err(mismatch("class ID field"));
    }
    if (11..17).contains(&version.raw()) && object.script_type_index.is_none() {
        return Err(mismatch("script type index field"));
    }
    if (version.raw() < 11) != object.is_destroyed.is_some() {
        return Err(mismatch("destroyed field"));
    }
    if (15..17).contains(&version.raw())
        && object.is_stripped != Some(serialized_type.is_stripped_type)
    {
        return Err(mismatch("stripped flag"));
    }
    Ok(())
}

fn build_metadata_with_insertions(
    source: &[u8],
    table: &ObjectRecordTableLayout,
    object_count: usize,
    insertions: &[SerializedObjectInsertion],
    context: MetadataInsertionContext,
) -> Result<(Vec<u8>, Vec<ObjectRecordLayout>), SerializedFileRewriteError> {
    let MetadataInsertionContext {
        version,
        endian,
        metadata_file_offset,
        output_limit,
    } = context;
    let inserted_count = insertions.len();
    let total_count = object_count.checked_add(inserted_count).ok_or_else(|| {
        rewrite_error(
            None,
            SerializedFileRewriteErrorKind::LengthOverflow {
                resource: "object record count",
            },
        )
    })?;
    let serialized_count = i32::try_from(total_count).map_err(|_| {
        rewrite_error(
            None,
            SerializedFileRewriteErrorKind::ObjectLimitExceeded {
                requested: total_count,
                limit: i32::MAX as usize,
            },
        )
    })?;
    let mut inserted_records = Vec::new();
    let mut inserted_layouts = Vec::new();
    inserted_layouts
        .try_reserve_exact(inserted_count)
        .map_err(|_| allocation_error("inserted object record layouts", inserted_count))?;
    for (insertion_index, insertion) in insertions.iter().enumerate() {
        let object_index = object_count + insertion_index;
        if version.raw() >= 14 {
            let absolute_position = metadata_file_offset
                .checked_add(table.records_end_offset)
                .and_then(|value| value.checked_add(inserted_records.len()))
                .ok_or_else(|| {
                    rewrite_error(
                        Some(object_index),
                        SerializedFileRewriteErrorKind::LengthOverflow {
                            resource: "inserted object record alignment",
                        },
                    )
                })?;
            let padding = (4 - (absolute_position & 3)) & 3;
            append_metadata_bytes(
                &mut inserted_records,
                &[0; 3][..padding],
                output_limit,
                Some(object_index),
            )?;
        }

        let path_id = &insertion.object.path_id;
        if version.raw() >= 14 {
            append_metadata_bytes(
                &mut inserted_records,
                &encode_i64(*path_id, endian),
                output_limit,
                Some(object_index),
            )?;
        } else {
            let path_id = i32::try_from(*path_id).map_err(|_| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::InsertionPathIdUnrepresentable {
                        path_id: *path_id,
                        width: 4,
                    },
                )
            })?;
            append_metadata_bytes(
                &mut inserted_records,
                &encode_i32(path_id, endian),
                output_limit,
                Some(object_index),
            )?;
        }

        let data_offset_field_offset = table
            .records_end_offset
            .checked_add(inserted_records.len())
            .ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "inserted data offset field location",
                    },
                )
            })?;
        let offset_width = if version.has_large_file_support() {
            8
        } else {
            4
        };
        append_metadata_bytes(
            &mut inserted_records,
            &[0; 8][..offset_width],
            output_limit,
            Some(object_index),
        )?;
        let data_size_field_offset = table
            .records_end_offset
            .checked_add(inserted_records.len())
            .ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "inserted data size field location",
                    },
                )
            })?;
        append_metadata_bytes(
            &mut inserted_records,
            &encode_i32(0, endian),
            output_limit,
            Some(object_index),
        )?;

        if version.raw() >= 17 {
            let type_index = insertion.object.resolved_type_index.ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::InsertionTypeUnresolved,
                )
            })?;
            let type_index = i32::try_from(type_index).map_err(|_| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::InsertionTypeIndexOutOfRange {
                        type_index,
                        type_count: i32::MAX as usize,
                    },
                )
            })?;
            append_metadata_bytes(
                &mut inserted_records,
                &encode_i32(type_index, endian),
                output_limit,
                Some(object_index),
            )?;
        } else {
            append_metadata_bytes(
                &mut inserted_records,
                &encode_i32(insertion.object.raw_type_id, endian),
                output_limit,
                Some(object_index),
            )?;
        }
        if version.raw() < 16 {
            let class_id = insertion.object.class_id.ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::InsertionTypeMismatch {
                        field: "class ID field",
                    },
                )
            })?;
            append_metadata_bytes(
                &mut inserted_records,
                &encode_i16(class_id, endian),
                output_limit,
                Some(object_index),
            )?;
        }
        if (11..17).contains(&version.raw()) {
            let script_type_index = insertion.object.script_type_index.ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::InsertionTypeMismatch {
                        field: "script type index field",
                    },
                )
            })?;
            append_metadata_bytes(
                &mut inserted_records,
                &encode_i16(script_type_index, endian),
                output_limit,
                Some(object_index),
            )?;
        }
        if version.raw() < 11 {
            let destroyed = insertion.object.is_destroyed.ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::InsertionTypeMismatch {
                        field: "destroyed field",
                    },
                )
            })?;
            append_metadata_bytes(
                &mut inserted_records,
                &encode_u16(destroyed, endian),
                output_limit,
                Some(object_index),
            )?;
        }
        if (15..17).contains(&version.raw()) {
            let stripped = insertion.object.is_stripped.ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::InsertionTypeMismatch {
                        field: "stripped flag field",
                    },
                )
            })?;
            append_metadata_bytes(
                &mut inserted_records,
                &[u8::from(stripped)],
                output_limit,
                Some(object_index),
            )?;
        }
        inserted_layouts.push(ObjectRecordLayout {
            path_id: insertion.object.path_id,
            data_offset_field_offset,
            data_size_field_offset,
        });
    }

    let metadata_size = source
        .len()
        .checked_add(inserted_records.len())
        .ok_or_else(|| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::LengthOverflow {
                    resource: "rewritten metadata",
                },
            )
        })?;
    if metadata_size > output_limit {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::OutputLimitExceeded {
                requested: metadata_size,
                limit: output_limit,
            },
        ));
    }
    let mut metadata = Vec::new();
    metadata
        .try_reserve_exact(metadata_size)
        .map_err(|_| allocation_error("rewritten metadata", metadata_size))?;
    let records_end = table.records_end_offset;
    metadata.extend_from_slice(&source[..records_end]);
    metadata.extend_from_slice(&inserted_records);
    metadata.extend_from_slice(&source[records_end..]);
    let count_end = table
        .count_field_offset
        .checked_add(4)
        .ok_or_else(|| invalid_object_table_layout(table, source.len()))?;
    let rewritten_metadata_size = metadata.len();
    let count_field = metadata
        .get_mut(table.count_field_offset..count_end)
        .ok_or_else(|| invalid_object_table_layout(table, rewritten_metadata_size))?;
    count_field.copy_from_slice(&encode_i32(serialized_count, endian));

    let layout_count = table
        .records
        .len()
        .checked_add(inserted_layouts.len())
        .ok_or_else(|| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::LengthOverflow {
                    resource: "rewritten object record layouts",
                },
            )
        })?;
    let mut layouts = Vec::new();
    layouts
        .try_reserve_exact(layout_count)
        .map_err(|_| allocation_error("rewritten object record layouts", layout_count))?;
    layouts.extend_from_slice(&table.records);
    layouts.extend(inserted_layouts);
    Ok((metadata, layouts))
}

fn append_metadata_bytes(
    output: &mut Vec<u8>,
    bytes: &[u8],
    limit: usize,
    object_index: Option<usize>,
) -> Result<(), SerializedFileRewriteError> {
    let requested = output.len().checked_add(bytes.len()).ok_or_else(|| {
        rewrite_error(
            object_index,
            SerializedFileRewriteErrorKind::LengthOverflow {
                resource: "inserted object record bytes",
            },
        )
    })?;
    if requested > limit {
        return Err(rewrite_error(
            object_index,
            SerializedFileRewriteErrorKind::OutputLimitExceeded { requested, limit },
        ));
    }
    output
        .try_reserve(bytes.len())
        .map_err(|_| allocation_error("inserted object record bytes", requested))?;
    output.extend_from_slice(bytes);
    Ok(())
}

fn validate_object_record_table_layout(
    layout: &ObjectRecordTableLayout,
    object_count: usize,
    metadata_bytes: &[u8],
    version: FormatVersion,
    endian: Endian,
) -> Result<(), SerializedFileRewriteError> {
    let table = layout;
    let metadata_size = metadata_bytes.len();
    let Some(count_field_end) = table.count_field_offset.checked_add(4) else {
        return Err(invalid_object_table_layout(table, metadata_size));
    };
    if count_field_end > metadata_size
        || table.records_start_offset < count_field_end
        || table.records_end_offset < table.records_start_offset
        || table.records_end_offset > metadata_size
    {
        return Err(invalid_object_table_layout(table, metadata_size));
    }
    let count_bytes: [u8; 4] = metadata_bytes[table.count_field_offset..count_field_end]
        .try_into()
        .map_err(|_| invalid_object_table_layout(table, metadata_size))?;
    let serialized_count = match endian {
        Endian::Little => i32::from_le_bytes(count_bytes),
        Endian::Big => i32::from_be_bytes(count_bytes),
    };
    if usize::try_from(serialized_count).ok() != Some(object_count)
        || layout.records.len() != object_count
    {
        return Err(SerializedFileRewriteError::new(
            None,
            SerializedFileRewriteErrorKind::ObjectRecordCountMismatch {
                objects: object_count,
                layouts: layout.records.len(),
            },
        ));
    }

    let offset_width = if version.has_large_file_support() {
        8
    } else {
        4
    };
    for (object_index, record) in layout.records.iter().enumerate() {
        for (field_offset, field_width) in [
            (record.data_offset_field_offset, offset_width),
            (record.data_size_field_offset, 4),
        ] {
            let field_end = field_offset.checked_add(field_width).ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "object record field range",
                    },
                )
            })?;
            if field_offset < table.records_start_offset || field_end > table.records_end_offset {
                return Err(rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::ObjectRecordFieldOutsideTable {
                        field_offset,
                        field_width,
                        records_start_offset: table.records_start_offset,
                        records_end_offset: table.records_end_offset,
                    },
                ));
            }
        }
    }
    Ok(())
}

fn invalid_object_table_layout(
    table: &ObjectRecordTableLayout,
    metadata_size: usize,
) -> SerializedFileRewriteError {
    SerializedFileRewriteError::new(
        None,
        SerializedFileRewriteErrorKind::InvalidObjectRecordTableLayout {
            count_field_offset: table.count_field_offset,
            records_start_offset: table.records_start_offset,
            records_end_offset: table.records_end_offset,
            metadata_size,
        },
    )
}

fn patch_object_records(
    output: &mut [u8],
    metadata_output_offset: usize,
    metadata_size: usize,
    layouts: &[ObjectRecordLayout],
    records: &[(u64, i32)],
    version: FormatVersion,
    endian: Endian,
) -> Result<(), SerializedFileRewriteError> {
    let offset_width = if version.has_large_file_support() {
        8
    } else {
        4
    };
    if layouts.len() != records.len() {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::ObjectRecordCountMismatch {
                objects: records.len(),
                layouts: layouts.len(),
            },
        ));
    }
    for (object_index, (layout, (object_offset, object_size))) in
        layouts.iter().zip(records).enumerate()
    {
        let relative_offset = layout.data_offset_field_offset;
        let relative_size = layout.data_size_field_offset;
        let absolute_offset = metadata_output_offset
            .checked_add(relative_offset)
            .ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "object offset field location",
                    },
                )
            })?;
        let absolute_size = metadata_output_offset
            .checked_add(relative_size)
            .ok_or_else(|| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "object size field location",
                    },
                )
            })?;
        check_metadata_field(relative_offset, offset_width, metadata_size, object_index)?;
        check_metadata_field(relative_size, 4, metadata_size, object_index)?;
        if offset_width == 4 {
            let value = u32::try_from(*object_offset).map_err(|_| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::ObjectOffsetUnrepresentable {
                        offset: *object_offset,
                        width: 4,
                    },
                )
            })?;
            patch_bytes(
                output,
                absolute_offset,
                &encode_u32(value, endian),
                object_index,
            )?;
        } else {
            let value = i64::try_from(*object_offset).map_err(|_| {
                rewrite_error(
                    Some(object_index),
                    SerializedFileRewriteErrorKind::ObjectOffsetUnrepresentable {
                        offset: *object_offset,
                        width: 8,
                    },
                )
            })?;
            patch_bytes(
                output,
                absolute_offset,
                &encode_i64(value, endian),
                object_index,
            )?;
        }
        patch_bytes(
            output,
            absolute_size,
            &encode_i32(*object_size, endian),
            object_index,
        )?;
    }
    Ok(())
}

fn check_metadata_field(
    field_offset: usize,
    field_width: usize,
    metadata_size: usize,
    object_index: usize,
) -> Result<(), SerializedFileRewriteError> {
    let field_end = field_offset.checked_add(field_width).ok_or_else(|| {
        rewrite_error(
            Some(object_index),
            SerializedFileRewriteErrorKind::LengthOverflow {
                resource: "object metadata field range",
            },
        )
    })?;
    if field_end > metadata_size {
        return Err(rewrite_error(
            Some(object_index),
            SerializedFileRewriteErrorKind::InvalidMetadataFieldRange {
                field_offset,
                field_width,
                metadata_size,
            },
        ));
    }
    Ok(())
}

fn patch_header(
    output: &mut [u8],
    version: FormatVersion,
    endian: Endian,
    metadata_size: usize,
    file_size: usize,
    data_offset: usize,
    header_size: usize,
) -> Result<(), SerializedFileRewriteError> {
    let expected_header_size = serialized_header_size(version);
    if header_size != expected_header_size || output.len() < header_size {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::HeaderLayoutMismatch {
                expected: expected_header_size,
                actual: header_size,
            },
        ));
    }
    let mut header = [0_u8; MAX_HEADER_SIZE];
    header[8..12].copy_from_slice(&version.raw().to_be_bytes());
    if version.has_endianness() {
        header[16] = u8::from(endian == Endian::Big);
    }
    if version.has_large_file_support() {
        let metadata_size = u32::try_from(metadata_size).map_err(|_| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::MetadataSizeUnrepresentable {
                    size: metadata_size,
                    width: 4,
                },
            )
        })?;
        let file_size = i64::try_from(file_size).map_err(|_| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::FileSizeUnrepresentable {
                    size: file_size,
                    width: 8,
                },
            )
        })?;
        let data_offset = i64::try_from(data_offset).map_err(|_| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::DataOffsetUnrepresentable {
                    offset: data_offset,
                    width: 8,
                },
            )
        })?;
        header[20..24].copy_from_slice(&metadata_size.to_be_bytes());
        header[24..32].copy_from_slice(&file_size.to_be_bytes());
        header[32..40].copy_from_slice(&data_offset.to_be_bytes());
    } else {
        let metadata_size = i32::try_from(metadata_size).map_err(|_| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::MetadataSizeUnrepresentable {
                    size: metadata_size,
                    width: 4,
                },
            )
        })?;
        let file_size = u32::try_from(file_size).map_err(|_| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::FileSizeUnrepresentable {
                    size: file_size,
                    width: 4,
                },
            )
        })?;
        let data_offset = u32::try_from(data_offset).map_err(|_| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::DataOffsetUnrepresentable {
                    offset: data_offset,
                    width: 4,
                },
            )
        })?;
        header[0..4].copy_from_slice(&metadata_size.to_be_bytes());
        header[4..8].copy_from_slice(&file_size.to_be_bytes());
        header[12..16].copy_from_slice(&data_offset.to_be_bytes());
    }
    let output_size = output.len();
    let target = output.get_mut(..header_size).ok_or_else(|| {
        rewrite_error(
            None,
            SerializedFileRewriteErrorKind::HeaderLayoutMismatch {
                expected: header_size,
                actual: output_size,
            },
        )
    })?;
    target.copy_from_slice(&header[..header_size]);
    Ok(())
}

fn serialized_header_size(version: FormatVersion) -> usize {
    16 + usize::from(version.has_endianness()) * 4
        + usize::from(version.has_large_file_support()) * 28
}

fn copy_input(
    input: &[u8],
    max_output_file_bytes: usize,
) -> Result<Vec<u8>, SerializedFileRewriteError> {
    if input.len() > max_output_file_bytes {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::OutputLimitExceeded {
                requested: input.len(),
                limit: max_output_file_bytes,
            },
        ));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(input.len())
        .map_err(|_| allocation_error("unchanged SerializedFile copy", input.len()))?;
    output.extend_from_slice(input);
    Ok(output)
}

fn append_output(
    output: &mut Vec<u8>,
    bytes: &[u8],
    limit: usize,
    object_index: Option<usize>,
) -> Result<(), SerializedFileRewriteError> {
    let requested = output.len().checked_add(bytes.len()).ok_or_else(|| {
        rewrite_error(
            object_index,
            SerializedFileRewriteErrorKind::LengthOverflow {
                resource: "rewritten SerializedFile",
            },
        )
    })?;
    if requested > limit {
        return Err(rewrite_error(
            object_index,
            SerializedFileRewriteErrorKind::OutputLimitExceeded { requested, limit },
        ));
    }
    reserve_output(output, requested, limit, object_index)?;
    output.extend_from_slice(bytes);
    Ok(())
}

fn reserve_output(
    output: &mut Vec<u8>,
    requested_capacity: usize,
    limit: usize,
    object_index: Option<usize>,
) -> Result<(), SerializedFileRewriteError> {
    if requested_capacity > output.capacity() {
        if requested_capacity > limit {
            return Err(rewrite_error(
                object_index,
                SerializedFileRewriteErrorKind::OutputLimitExceeded {
                    requested: requested_capacity,
                    limit,
                },
            ));
        }
        let geometric_capacity = output.capacity().saturating_mul(2).max(64);
        let target_capacity = geometric_capacity.max(requested_capacity).min(limit);
        let additional = target_capacity.saturating_sub(output.len());
        output
            .try_reserve_exact(additional)
            .map_err(|_| allocation_error("rewritten SerializedFile bytes", target_capacity))?;
    }
    Ok(())
}

fn align_output(
    output: &mut Vec<u8>,
    alignment: usize,
    limit: usize,
    object_index: Option<usize>,
) -> Result<(), SerializedFileRewriteError> {
    let padding = (alignment - (output.len() & (alignment - 1))) & (alignment - 1);
    const ZEROES: [u8; DATA_ALIGNMENT - 1] = [0; DATA_ALIGNMENT - 1];
    append_output(output, &ZEROES[..padding], limit, object_index)
}

fn patch_bytes(
    output: &mut [u8],
    start: usize,
    bytes: &[u8],
    object_index: usize,
) -> Result<(), SerializedFileRewriteError> {
    let end = start.checked_add(bytes.len()).ok_or_else(|| {
        rewrite_error(
            Some(object_index),
            SerializedFileRewriteErrorKind::LengthOverflow {
                resource: "rewritten metadata field range",
            },
        )
    })?;
    let output_size = output.len();
    let target = output.get_mut(start..end).ok_or_else(|| {
        rewrite_error(
            Some(object_index),
            SerializedFileRewriteErrorKind::InvalidMetadataFieldRange {
                field_offset: start,
                field_width: bytes.len(),
                metadata_size: output_size,
            },
        )
    })?;
    target.copy_from_slice(bytes);
    Ok(())
}

fn encode_u32(value: u32, endian: Endian) -> [u8; 4] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

fn encode_i32(value: i32, endian: Endian) -> [u8; 4] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

fn encode_i16(value: i16, endian: Endian) -> [u8; 2] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

fn encode_u16(value: u16, endian: Endian) -> [u8; 2] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

fn encode_i64(value: i64, endian: Endian) -> [u8; 8] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

fn rewrite_error(
    object_index: Option<usize>,
    kind: SerializedFileRewriteErrorKind,
) -> SerializedFileRewriteError {
    SerializedFileRewriteError::new(object_index, kind)
}

fn allocation_error(resource: &'static str, requested: usize) -> SerializedFileRewriteError {
    rewrite_error(
        None,
        SerializedFileRewriteErrorKind::AllocationFailed {
            resource,
            requested,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        build_metadata_with_insertions, patch_header, patch_object_records, serialized_header_size,
        MetadataInsertionContext, SerializedObjectInsertion,
    };
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::header::SerializedFileHeader;
    use crate::serialized_file::metadata::{
        ObjectInfo, ObjectRecordLayout, ObjectRecordTableLayout,
    };
    use crate::serialized_file::object::SerializedValue;

    fn make_insertion(version: FormatVersion, path_id: i64) -> SerializedObjectInsertion {
        SerializedObjectInsertion {
            object: ObjectInfo {
                path_id,
                byte_start: 123,
                byte_size: 456,
                serialized_type_index: (version.raw() >= 17).then_some(0),
                raw_type_id: 1,
                class_id: (version.raw() < 16).then_some(1),
                script_type_index: (11..17).contains(&version.raw()).then_some(0),
                is_destroyed: (version.raw() < 11).then_some(0),
                is_stripped: (15..17).contains(&version.raw()).then_some(false),
                resolved_type_index: Some(0),
            },
            root: SerializedValue::I32(7),
            trailing_bytes: Vec::new(),
        }
    }

    #[test]
    fn writes_extended_generation_22_header_and_64_bit_object_offsets() {
        let version = FormatVersion::LargeFilesSupport;
        let header_size = serialized_header_size(version);
        assert_eq!(header_size, 48);
        let mut header_bytes = [0; 48];
        patch_header(
            &mut header_bytes,
            version,
            Endian::Big,
            123,
            456,
            64,
            header_size,
        )
        .unwrap();
        let header = SerializedFileHeader::parse(&header_bytes).unwrap();
        assert_eq!(header.metadata_size, 123);
        assert_eq!(header.file_size, 456);
        assert_eq!(header.data_offset, 64);
        assert_eq!(header.data_endian, Some(Endian::Big));
        assert_eq!(header.reserved_v22, Some(0));

        let layout = [ObjectRecordLayout {
            path_id: 99,
            data_offset_field_offset: 0,
            data_size_field_offset: 8,
        }];
        let offset = u64::from(u32::MAX) + 1;
        let mut metadata_bytes = [0; 12];
        let metadata_size = metadata_bytes.len();
        patch_object_records(
            &mut metadata_bytes,
            0,
            metadata_size,
            &layout,
            &[(offset, 13)],
            version,
            Endian::Big,
        )
        .unwrap();
        assert_eq!(
            i64::from_be_bytes(metadata_bytes[..8].try_into().unwrap()),
            i64::try_from(offset).unwrap()
        );
        assert_eq!(
            i32::from_be_bytes(metadata_bytes[8..12].try_into().unwrap()),
            13
        );
    }

    #[test]
    fn writes_inserted_record_fields_for_aligned_and_extended_generations() {
        for (version, expected_record_size, offset_width, expected_type_field) in [
            (FormatVersion::Unknown14, 24, 4, 1),
            (FormatVersion::SupportsStrippedObject, 25, 4, 1),
            (FormatVersion::RefactorTypeData, 20, 4, 0),
            (FormatVersion::LargeFilesSupport, 24, 8, 0),
        ] {
            let source = [0, 0, 0, 0, 0xca, 0xfe];
            let table = ObjectRecordTableLayout {
                count_field_offset: 0,
                records_start_offset: 4,
                records_end_offset: 4,
                records: Vec::new(),
            };
            let insertion = make_insertion(version, 0x0102_0304_0506_0708);
            let (metadata, layouts) = build_metadata_with_insertions(
                &source,
                &table,
                0,
                &[insertion],
                MetadataInsertionContext {
                    version,
                    endian: Endian::Big,
                    metadata_file_offset: 20,
                    output_limit: 1024,
                },
            )
            .unwrap();

            assert_eq!(&metadata[..4], &1_i32.to_be_bytes());
            assert_eq!(metadata.len(), source.len() + expected_record_size);
            assert_eq!(&metadata[4 + expected_record_size..], &[0xca, 0xfe]);
            assert_eq!(layouts.len(), 1);
            let layout = layouts[0];
            assert_eq!(layout.path_id, 0x0102_0304_0506_0708);
            assert_eq!(layout.data_offset_field_offset, 12);
            assert_eq!(layout.data_size_field_offset, 12 + offset_width);
            assert_eq!(
                &metadata[layout.data_offset_field_offset
                    ..layout.data_offset_field_offset + offset_width],
                &[0; 8][..offset_width]
            );
            assert_eq!(
                i32::from_be_bytes(
                    metadata[layout.data_size_field_offset..layout.data_size_field_offset + 4]
                        .try_into()
                        .unwrap()
                ),
                0
            );
            let type_field_offset = layout.data_size_field_offset + 4;
            let actual_type_field = i32::from_be_bytes(
                metadata[type_field_offset..type_field_offset + 4]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(actual_type_field, expected_type_field);
        }
    }
}
