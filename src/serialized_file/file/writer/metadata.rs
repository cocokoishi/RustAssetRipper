use super::bytes::{
    allocation_error, encode_i16, encode_i32, encode_i64, encode_u16, encode_u32, rewrite_error,
    serialized_header_size,
};
use super::model::{
    MetadataInsertionContext, MetadataInsertions, SerializedExternalFileInsertion,
    SerializedFileRewriteError, SerializedFileRewriteErrorKind, SerializedFileRewriteLimits,
    SerializedObjectInsertion,
};
use crate::io::binary::Endian;
use crate::serialized_file::format_version::FormatVersion;
use crate::serialized_file::metadata::{
    ExternalFileTableLayout, ObjectRecordLayout, ObjectRecordTableLayout, SerializedFileMetadata,
};
pub(super) struct MetadataRewritePlan<'input> {
    pub(super) source_metadata: &'input [u8],
    pub(super) rewritten_metadata: Option<Vec<u8>>,
    pub(super) object_record_layouts: Vec<ObjectRecordLayout>,
    pub(super) expected_header_size: usize,
    pub(super) input_data_start: u64,
    pub(super) input_data_end: u64,
}

impl<'input> MetadataRewritePlan<'input> {
    pub(super) fn prepare(
        file: &crate::serialized_file::file::SerializedFile<'input>,
        insertions: &[SerializedObjectInsertion],
        external_file_insertions: &[SerializedExternalFileInsertion],
        limits: SerializedFileRewriteLimits,
    ) -> Result<Self, SerializedFileRewriteError> {
        let version = file.header.format_version;
        let expected_header_size = serialized_header_size(version);
        if file.header.header_size != expected_header_size {
            return Err(SerializedFileRewriteError::new(
                None,
                SerializedFileRewriteErrorKind::HeaderLayoutMismatch {
                    expected: expected_header_size,
                    actual: file.header.header_size,
                },
            ));
        }
        let metadata_start = file.metadata.prefix.metadata_offset;
        let metadata_size = usize::try_from(file.header.metadata_size).map_err(|_| {
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
        if metadata_end > file.input().len()
            || (version.metadata_at_end() && metadata_end != file.input().len())
            || (!version.metadata_at_end() && metadata_start != file.header.header_size)
        {
            return Err(rewrite_error(
                None,
                SerializedFileRewriteErrorKind::InvalidMetadataRange {
                    start: metadata_start,
                    end: metadata_end,
                    input_size: file.input().len(),
                },
            ));
        }
        let metadata_bytes = file
            .input()
            .get(metadata_start..metadata_end)
            .ok_or_else(|| {
                rewrite_error(
                    None,
                    SerializedFileRewriteErrorKind::InvalidMetadataRange {
                        start: metadata_start,
                        end: metadata_end,
                        input_size: file.input().len(),
                    },
                )
            })?;
        validate_object_record_table_layout(
            &file.object_record_layout,
            file.metadata.objects.len(),
            metadata_bytes,
            version,
            file.metadata.prefix.body_endian,
        )?;
        validate_external_file_table_layout(
            &file.external_file_table_layout,
            file.metadata.externals.len(),
            metadata_bytes,
            file.metadata.prefix.body_endian,
        )?;
        let metadata_insertions = MetadataInsertions {
            objects: insertions,
            external_files: external_file_insertions,
        };
        let (rewritten_metadata_owned, object_record_layouts) =
            if insertions.is_empty() && external_file_insertions.is_empty() {
                let mut layouts = Vec::new();
                layouts
                    .try_reserve_exact(file.object_record_layout.records.len())
                    .map_err(|_| {
                        allocation_error(
                            "rewritten object record layouts",
                            file.object_record_layout.records.len(),
                        )
                    })?;
                layouts.extend_from_slice(&file.object_record_layout.records);
                (None, layouts)
            } else {
                let (metadata, layouts) = build_metadata_with_insertions(
                    metadata_bytes,
                    &file.object_record_layout,
                    &file.external_file_table_layout,
                    &file.metadata,
                    metadata_insertions,
                    MetadataInsertionContext {
                        version,
                        endian: file.metadata.prefix.body_endian,
                        metadata_file_offset: metadata_start,
                        output_limit: limits.max_output_file_bytes,
                    },
                )?;
                (Some(metadata), layouts)
            };
        let input_data_start = file.header.data_offset;
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
            u64::try_from(file.input().len()).map_err(|_| {
                rewrite_error(
                    None,
                    SerializedFileRewriteErrorKind::LengthOverflow {
                        resource: "input file size",
                    },
                )
            })?
        };
        Ok(Self {
            source_metadata: metadata_bytes,
            rewritten_metadata: rewritten_metadata_owned,
            object_record_layouts,
            expected_header_size,
            input_data_start,
            input_data_end,
        })
    }

    pub(super) fn metadata_bytes(&self) -> &[u8] {
        self.rewritten_metadata
            .as_deref()
            .unwrap_or(self.source_metadata)
    }
}

pub(super) fn build_metadata_with_insertions(
    source: &[u8],
    object_table: &ObjectRecordTableLayout,
    external_file_table: &ExternalFileTableLayout,
    metadata: &SerializedFileMetadata,
    insertions: MetadataInsertions<'_>,
    context: MetadataInsertionContext,
) -> Result<(Vec<u8>, Vec<ObjectRecordLayout>), SerializedFileRewriteError> {
    let MetadataInsertionContext {
        version,
        endian,
        metadata_file_offset,
        output_limit,
    } = context;
    let object_insertions = insertions.objects;
    let external_insertions = insertions.external_files;
    let inserted_count = object_insertions.len();
    let total_count = metadata
        .objects
        .len()
        .checked_add(inserted_count)
        .ok_or_else(|| {
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
    for (insertion_index, insertion) in object_insertions.iter().enumerate() {
        let object_index = metadata.objects.len() + insertion_index;
        if version.raw() >= 14 {
            let absolute_position = metadata_file_offset
                .checked_add(object_table.records_end_offset)
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

        let data_offset_field_offset = object_table
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
        let data_size_field_offset = object_table
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

    let total_external_file_count = metadata
        .externals
        .len()
        .checked_add(external_insertions.len())
        .ok_or_else(|| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::LengthOverflow {
                    resource: "external-file record count",
                },
            )
        })?;
    let serialized_external_file_count =
        i32::try_from(total_external_file_count).map_err(|_| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::ExternalFileCountLimitExceeded {
                    requested: total_external_file_count,
                    limit: i32::MAX as usize,
                },
            )
        })?;
    let mut inserted_external_records = Vec::new();
    for insertion in external_insertions {
        let identifier = &insertion.file_identifier;
        if version.raw() >= 6 {
            let asset_path = identifier.asset_path.as_deref().ok_or_else(|| {
                rewrite_error(
                    None,
                    SerializedFileRewriteErrorKind::ExternalFileFieldMismatch {
                        field: "asset path presence",
                    },
                )
            })?;
            append_metadata_bytes(
                &mut inserted_external_records,
                asset_path.as_bytes(),
                output_limit,
                None,
            )?;
            append_metadata_bytes(&mut inserted_external_records, &[0], output_limit, None)?;
        }
        if version.raw() >= 5 {
            let guid = identifier.guid.as_ref().ok_or_else(|| {
                rewrite_error(
                    None,
                    SerializedFileRewriteErrorKind::ExternalFileFieldMismatch {
                        field: "GUID presence",
                    },
                )
            })?;
            for word in guid {
                append_metadata_bytes(
                    &mut inserted_external_records,
                    &encode_u32(*word, endian),
                    output_limit,
                    None,
                )?;
            }
            let file_type = identifier.file_type.ok_or_else(|| {
                rewrite_error(
                    None,
                    SerializedFileRewriteErrorKind::ExternalFileFieldMismatch {
                        field: "asset type presence",
                    },
                )
            })?;
            append_metadata_bytes(
                &mut inserted_external_records,
                &encode_i32(file_type, endian),
                output_limit,
                None,
            )?;
        }
        append_metadata_bytes(
            &mut inserted_external_records,
            identifier.path_name_origin.as_bytes(),
            output_limit,
            None,
        )?;
        append_metadata_bytes(&mut inserted_external_records, &[0], output_limit, None)?;
    }

    if object_table.records_end_offset > external_file_table.count_field_offset {
        return Err(invalid_external_file_table_layout(
            external_file_table,
            source.len(),
        ));
    }
    let metadata_size = source
        .len()
        .checked_add(inserted_records.len())
        .and_then(|size| size.checked_add(inserted_external_records.len()))
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
    let object_records_end = object_table.records_end_offset;
    let external_records_end = external_file_table.records_end_offset;
    metadata.extend_from_slice(&source[..object_records_end]);
    metadata.extend_from_slice(&inserted_records);
    metadata.extend_from_slice(&source[object_records_end..external_records_end]);
    metadata.extend_from_slice(&inserted_external_records);
    metadata.extend_from_slice(&source[external_records_end..]);
    let count_end = object_table
        .count_field_offset
        .checked_add(4)
        .ok_or_else(|| invalid_object_table_layout(object_table, source.len()))?;
    let rewritten_metadata_size = metadata.len();
    let count_field = metadata
        .get_mut(object_table.count_field_offset..count_end)
        .ok_or_else(|| invalid_object_table_layout(object_table, rewritten_metadata_size))?;
    count_field.copy_from_slice(&encode_i32(serialized_count, endian));

    let shifted_external_count_field = external_file_table
        .count_field_offset
        .checked_add(inserted_records.len())
        .ok_or_else(|| invalid_external_file_table_layout(external_file_table, source.len()))?;
    let external_count_end = shifted_external_count_field.checked_add(4).ok_or_else(|| {
        invalid_external_file_table_layout(external_file_table, rewritten_metadata_size)
    })?;
    let external_count_field = metadata
        .get_mut(shifted_external_count_field..external_count_end)
        .ok_or_else(|| {
            invalid_external_file_table_layout(external_file_table, rewritten_metadata_size)
        })?;
    external_count_field.copy_from_slice(&encode_i32(serialized_external_file_count, endian));

    let layout_count = object_table
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
    layouts.extend_from_slice(&object_table.records);
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

pub(super) fn validate_object_record_table_layout(
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

pub(super) fn validate_external_file_table_layout(
    layout: &ExternalFileTableLayout,
    external_count: usize,
    metadata_bytes: &[u8],
    endian: Endian,
) -> Result<(), SerializedFileRewriteError> {
    let metadata_size = metadata_bytes.len();
    let Some(count_field_end) = layout.count_field_offset.checked_add(4) else {
        return Err(invalid_external_file_table_layout(layout, metadata_size));
    };
    if count_field_end > metadata_size
        || layout.records_start_offset < count_field_end
        || layout.records_end_offset < layout.records_start_offset
        || layout.records_end_offset > metadata_size
    {
        return Err(invalid_external_file_table_layout(layout, metadata_size));
    }
    let count_bytes: [u8; 4] = metadata_bytes[layout.count_field_offset..count_field_end]
        .try_into()
        .map_err(|_| invalid_external_file_table_layout(layout, metadata_size))?;
    let serialized_count = match endian {
        Endian::Little => i32::from_le_bytes(count_bytes),
        Endian::Big => i32::from_be_bytes(count_bytes),
    };
    if usize::try_from(serialized_count).ok() != Some(external_count) {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::ExternalFileCountMismatch {
                metadata: external_count,
                serialized: serialized_count,
            },
        ));
    }
    Ok(())
}

pub(super) fn invalid_external_file_table_layout(
    layout: &ExternalFileTableLayout,
    metadata_size: usize,
) -> SerializedFileRewriteError {
    rewrite_error(
        None,
        SerializedFileRewriteErrorKind::InvalidExternalFileTableLayout {
            count_field_offset: layout.count_field_offset,
            records_start_offset: layout.records_start_offset,
            records_end_offset: layout.records_end_offset,
            metadata_size,
        },
    )
}

pub(super) fn validate_external_file_identifier(
    insertion: &SerializedExternalFileInsertion,
    version: FormatVersion,
) -> Result<(), SerializedFileRewriteError> {
    let identifier = &insertion.file_identifier;
    if (version.raw() >= 6) != identifier.asset_path.is_some() {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::ExternalFileFieldMismatch {
                field: "asset path presence",
            },
        ));
    }
    if (version.raw() >= 5) != identifier.guid.is_some()
        || (version.raw() >= 5) != identifier.file_type.is_some()
    {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::ExternalFileFieldMismatch {
                field: "GUID or asset type presence",
            },
        ));
    }
    if identifier
        .asset_path
        .as_deref()
        .is_some_and(|value| value.as_bytes().contains(&0))
    {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::ExternalFileStringContainsNul {
                field: "asset path",
            },
        ));
    }
    if identifier.path_name_origin.as_bytes().contains(&0) {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::ExternalFileStringContainsNul {
                field: "path name origin",
            },
        ));
    }
    Ok(())
}

pub(super) fn invalid_object_table_layout(
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
