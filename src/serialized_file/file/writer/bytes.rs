use super::model::{SerializedFileRewriteError, SerializedFileRewriteErrorKind};
use crate::io::binary::Endian;
use crate::serialized_file::format_version::FormatVersion;
use crate::serialized_file::metadata::ObjectRecordLayout;
pub(super) const DATA_ALIGNMENT: usize = 16;
pub(super) const MAX_HEADER_SIZE: usize = 48;

pub(super) fn patch_object_records(
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

pub(super) fn check_metadata_field(
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

pub(super) fn patch_header(
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

pub(super) fn serialized_header_size(version: FormatVersion) -> usize {
    16 + usize::from(version.has_endianness()) * 4
        + usize::from(version.has_large_file_support()) * 28
}

pub(super) fn copy_input(
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

pub(super) fn append_output(
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

pub(super) fn reserve_output(
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

pub(super) fn align_output(
    output: &mut Vec<u8>,
    alignment: usize,
    limit: usize,
    object_index: Option<usize>,
) -> Result<(), SerializedFileRewriteError> {
    let padding = (alignment - (output.len() & (alignment - 1))) & (alignment - 1);
    const ZEROES: [u8; DATA_ALIGNMENT - 1] = [0; DATA_ALIGNMENT - 1];
    append_output(output, &ZEROES[..padding], limit, object_index)
}

pub(super) fn patch_bytes(
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

pub(super) fn encode_u32(value: u32, endian: Endian) -> [u8; 4] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

pub(super) fn encode_i32(value: i32, endian: Endian) -> [u8; 4] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

pub(super) fn encode_i16(value: i16, endian: Endian) -> [u8; 2] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

pub(super) fn encode_u16(value: u16, endian: Endian) -> [u8; 2] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

pub(super) fn encode_i64(value: i64, endian: Endian) -> [u8; 8] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

pub(super) fn rewrite_error(
    object_index: Option<usize>,
    kind: SerializedFileRewriteErrorKind,
) -> SerializedFileRewriteError {
    SerializedFileRewriteError::new(object_index, kind)
}

pub(super) fn allocation_error(
    resource: &'static str,
    requested: usize,
) -> SerializedFileRewriteError {
    rewrite_error(
        None,
        SerializedFileRewriteErrorKind::AllocationFailed {
            resource,
            requested,
        },
    )
}
