use super::bytes::{
    align_output, allocation_error, append_output, patch_header, patch_object_records,
    reserve_output, rewrite_error, DATA_ALIGNMENT, MAX_HEADER_SIZE,
};
use super::model::{
    SerializedFileRewriteError, SerializedFileRewriteErrorKind, SerializedFileRewriteLimits,
};
use super::plan::SerializedFileRewritePlan;
use crate::serialized_file::file::SerializedFile;

const OBJECT_ALIGNMENT: usize = 8;

pub(super) fn execute(
    file: &SerializedFile<'_>,
    plan: &SerializedFileRewritePlan<'_, '_>,
    limits: SerializedFileRewriteLimits,
) -> Result<Vec<u8>, SerializedFileRewriteError> {
    let version = file.header.format_version;
    let expected_header_size = plan.metadata.expected_header_size;
    let rewritten_metadata = plan.metadata.metadata_bytes();
    let object_record_layouts = &plan.metadata.object_record_layouts;
    let input_data_start = plan.metadata.input_data_start;
    let input_data_end = plan.metadata.input_data_end;
    let total_object_count = plan.request.total_object_count;
    let ordered_replacements = &plan.request.ordered_replacements;
    let insertions = plan.request.insertions;
    let mut output = Vec::new();
    let initial_capacity = file
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
    for (object_index, object) in file.metadata.objects.iter().enumerate() {
        let replacement = ordered_replacements
            .get(replacement_cursor)
            .filter(|(replacement_index, _)| *replacement_index == object_index)
            .map(|(_, payload)| {
                replacement_cursor += 1;
                *payload
            });
        let encoded_replacement;
        let payload_bytes = if let Some(payload) = replacement {
            encoded_replacement = file
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
            let range = object.data_range(&file.header).ok_or_else(|| {
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
            file.input().get(start..end).ok_or_else(|| {
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
        let object_index = file.metadata.objects.len() + insertion_index;
        let type_index = insertion.object.resolved_type_index.ok_or_else(|| {
            rewrite_error(
                Some(object_index),
                SerializedFileRewriteErrorKind::InsertionTypeUnresolved,
            )
        })?;
        let payload_bytes = file
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
        object_record_layouts,
        &object_records,
        version,
        file.metadata.prefix.body_endian,
    )?;

    let file_size = output.len();
    patch_header(
        &mut output,
        file.header.format_version,
        file.metadata.prefix.body_endian,
        rewritten_metadata.len(),
        file_size,
        output_data_start,
        expected_header_size,
    )?;
    Ok(output)
}
