use super::bytes::{allocation_error, rewrite_error};
use super::metadata::validate_external_file_identifier;
use super::model::{
    SerializedExternalFileInsertion, SerializedFileRewriteError, SerializedFileRewriteErrorKind,
    SerializedFileRewriteLimits, SerializedObjectInsertion,
};
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::format_version::FormatVersion;
use crate::serialized_file::metadata::SerializedType;
use crate::serialized_file::object::ParsedObjectPayload;

pub(super) struct ValidatedRewriteRequest<'request> {
    pub(super) total_object_count: usize,
    pub(super) ordered_replacements: Vec<(usize, &'request ParsedObjectPayload)>,
    pub(super) insertions: &'request [SerializedObjectInsertion],
    pub(super) external_file_insertions: &'request [SerializedExternalFileInsertion],
}

pub(super) fn validate_counts(
    file: &SerializedFile<'_>,
    replacements: &[(usize, ParsedObjectPayload)],
    insertions: &[SerializedObjectInsertion],
    external_file_insertions: &[SerializedExternalFileInsertion],
    limits: SerializedFileRewriteLimits,
) -> Result<usize, SerializedFileRewriteError> {
    let total_object_count = file
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
    let total_external_file_count = file
        .metadata
        .externals
        .len()
        .checked_add(external_file_insertions.len())
        .ok_or_else(|| {
            rewrite_error(
                None,
                SerializedFileRewriteErrorKind::LengthOverflow {
                    resource: "rewritten external-file count",
                },
            )
        })?;
    if external_file_insertions.len() > limits.max_external_file_insertions {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::ExternalFileInsertionLimitExceeded {
                requested: external_file_insertions.len(),
                limit: limits.max_external_file_insertions,
            },
        ));
    }
    if total_external_file_count > i32::MAX as usize {
        return Err(rewrite_error(
            None,
            SerializedFileRewriteErrorKind::ExternalFileCountLimitExceeded {
                requested: total_external_file_count,
                limit: i32::MAX as usize,
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
    Ok(total_object_count)
}

pub(super) fn validate_request<'request>(
    file: &SerializedFile<'_>,
    replacements: &'request [(usize, ParsedObjectPayload)],
    insertions: &'request [SerializedObjectInsertion],
    external_file_insertions: &'request [SerializedExternalFileInsertion],
    total_object_count: usize,
) -> Result<ValidatedRewriteRequest<'request>, SerializedFileRewriteError> {
    if file.metadata.objects.len() != file.object_record_layout.records.len() {
        return Err(SerializedFileRewriteError::new(
            None,
            SerializedFileRewriteErrorKind::ObjectRecordCountMismatch {
                objects: file.metadata.objects.len(),
                layouts: file.object_record_layout.records.len(),
            },
        ));
    }
    for (object, layout) in file
        .metadata
        .objects
        .iter()
        .zip(&file.object_record_layout.records)
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

    let version = file.header.format_version;
    if !insertions.is_empty() {
        let mut path_ids = Vec::new();
        path_ids
            .try_reserve_exact(total_object_count)
            .map_err(|_| allocation_error("object PathID index", total_object_count))?;
        path_ids.extend(file.metadata.objects.iter().map(|object| object.path_id));
        for (insertion_index, insertion) in insertions.iter().enumerate() {
            validate_insertion_type(
                insertion,
                &file.metadata.types,
                version,
                file.metadata.objects.len() + insertion_index,
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
    for insertion in external_file_insertions {
        validate_external_file_identifier(insertion, version)?;
    }

    let mut ordered_replacements = Vec::new();
    ordered_replacements
        .try_reserve_exact(replacements.len())
        .map_err(|_| allocation_error("replacement index", replacements.len()))?;
    for (replacement_index, (object_index, payload)) in replacements.iter().enumerate() {
        if *object_index >= file.metadata.objects.len() {
            return Err(SerializedFileRewriteError::new(
                Some(*object_index),
                SerializedFileRewriteErrorKind::ReplacementIndexOutOfRange {
                    object_count: file.metadata.objects.len(),
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
    Ok(ValidatedRewriteRequest {
        total_object_count,
        ordered_replacements,
        insertions,
        external_file_insertions,
    })
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
