//! Preparation of streamed Mesh and image-texture fields for project YAML export.
//!
//! AssetRipper's YAML exporter temporarily copies streamed bytes into the asset's inline data
//! field and clears its StreamingInfo before serialization. This module applies that operation to
//! an export-owned decoded-value copy, leaving the source workspace payload unchanged.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::resource::{
    extract_streamed_resource_info, StreamedResourceInfo, StreamedResourceInfoError,
};
use crate::serialized_file::object::{
    SerializedValue, SerializedValueLookupError, SerializedValueMutationError,
};

const STREAMED_CONTENT_LIMIT: usize = 512 * 1024 * 1024;

/// A baseline YAML asset class that embeds streamed data before writing its document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamedYamlAssetKind {
    Mesh,
    ImageTexture,
}

/// Bounds for copying stream bytes into an export-owned decoded payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamedYamlAssetLimits {
    pub max_inline_bytes: usize,
    pub max_source_value_copy_bytes: usize,
}

impl Default for StreamedYamlAssetLimits {
    fn default() -> Self {
        Self {
            max_inline_bytes: STREAMED_CONTENT_LIMIT,
            max_source_value_copy_bytes: STREAMED_CONTENT_LIMIT,
        }
    }
}

/// What the YAML staging pass did with an asset's streamed data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamedYamlAssetPreparation {
    NoStreamField,
    StreamUnset,
    ImageTextureWithoutData,
    ExistingInlineData {
        resource: StreamedResourceInfo,
    },
    Materialized {
        resource: StreamedResourceInfo,
        byte_count: usize,
    },
}

/// An invalid shape or unavailable byte range while preparing a streamed YAML asset.
#[derive(Debug)]
pub enum StreamedYamlAssetError {
    FieldLookup {
        field: &'static str,
        source: SerializedValueLookupError,
    },
    FieldMutation {
        field: &'static str,
        source: SerializedValueMutationError,
    },
    AmbiguousFieldAliases {
        role: &'static str,
        first: &'static str,
        second: &'static str,
    },
    StreamInfo {
        field: &'static str,
        source: StreamedResourceInfoError,
    },
    InlineFieldMissing {
        asset_kind: StreamedYamlAssetKind,
    },
    InlineFieldWrongType {
        field: &'static str,
        actual: &'static str,
    },
    StreamContentRequired {
        resource: StreamedResourceInfo,
    },
    StreamContentSizeMismatch {
        expected: u32,
        actual: usize,
    },
    InlineByteLimitExceeded {
        size: usize,
        limit: usize,
    },
    SourceValueCopyLimitExceeded {
        size: usize,
        limit: usize,
    },
    AllocationFailed {
        requested: usize,
    },
    StreamFieldWrongType {
        field: &'static str,
        actual: &'static str,
    },
    StreamFieldWrongOffsetType {
        field: &'static str,
        actual: &'static str,
    },
}

impl Display for StreamedYamlAssetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field, source } => {
                write!(formatter, "could not locate streamed asset field '{field}': {source}")
            }
            Self::FieldMutation { field, source } => {
                write!(formatter, "could not update streamed asset field '{field}': {source}")
            }
            Self::AmbiguousFieldAliases { role, first, second } => write!(
                formatter,
                "streamed asset has multiple {role} fields: '{first}' and '{second}'"
            ),
            Self::StreamInfo { field, source } => {
                write!(formatter, "could not decode StreamingInfo field '{field}': {source}")
            }
            Self::InlineFieldMissing { asset_kind } => write!(
                formatter,
                "{asset_kind:?} has StreamingInfo but no inline byte-data field"
            ),
            Self::InlineFieldWrongType { field, actual } => write!(
                formatter,
                "streamed asset inline field '{field}' has type {actual}, expected byte data"
            ),
            Self::StreamContentRequired { resource } => write!(
                formatter,
                "streamed YAML asset requires bytes for {:?} at offset {} ({} bytes)",
                resource.path,
                resource.offset,
                resource.size
            ),
            Self::StreamContentSizeMismatch { expected, actual } => write!(
                formatter,
                "streamed YAML content has {actual} bytes, but StreamingInfo declares {expected}"
            ),
            Self::InlineByteLimitExceeded { size, limit } => write!(
                formatter,
                "streamed YAML content has {size} bytes, exceeding inline limit {limit}"
            ),
            Self::SourceValueCopyLimitExceeded { size, limit } => write!(
                formatter,
                "decoded streamed asset needs an estimated {size}-byte temporary value copy, exceeding limit {limit}"
            ),
            Self::AllocationFailed { requested } => write!(
                formatter,
                "could not allocate {requested} bytes for streamed YAML content"
            ),
            Self::StreamFieldWrongType { field, actual } => write!(
                formatter,
                "streaming metadata field '{field}' has type {actual}, expected a struct"
            ),
            Self::StreamFieldWrongOffsetType { field, actual } => write!(
                formatter,
                "streaming metadata field '{field}' has offset type {actual}, expected UInt32 or UInt64"
            ),
        }
    }
}

impl Error for StreamedYamlAssetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FieldLookup { source, .. } => Some(source),
            Self::FieldMutation { source, .. } => Some(source),
            Self::StreamInfo { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Stages one Mesh or image texture for YAML serialization.
///
/// `root` must be an export-owned copy of the decoded object value. If the inline field is empty
/// and StreamingInfo is set, the caller must provide the already-resolved exact byte range.
/// Existing inline bytes take precedence, as in the baseline exporter; streamed metadata is then
/// cleared without requesting resource content.
pub fn prepare_streamed_yaml_asset(
    root: &mut SerializedValue,
    asset_kind: StreamedYamlAssetKind,
    materialized_content: Option<&[u8]>,
    limits: StreamedYamlAssetLimits,
) -> Result<StreamedYamlAssetPreparation, StreamedYamlAssetError> {
    let stream_field = find_stream_field(root, asset_kind)?;
    let Some(stream_field) = stream_field else {
        return Ok(StreamedYamlAssetPreparation::NoStreamField);
    };
    let stream_value = root
        .unique_field_in_base_chain(stream_field)
        .map_err(|source| StreamedYamlAssetError::FieldLookup {
            field: stream_field,
            source,
        })?
        .ok_or(StreamedYamlAssetError::StreamFieldWrongType {
            field: stream_field,
            actual: "missing",
        })?;
    let info = extract_streamed_resource_info(stream_value).map_err(|source| {
        StreamedYamlAssetError::StreamInfo {
            field: stream_field,
            source,
        }
    })?;
    if !info.is_set() {
        let image_texture_without_data = match asset_kind {
            StreamedYamlAssetKind::Mesh => false,
            StreamedYamlAssetKind::ImageTexture => image_texture_inline_data_is_empty(root)?,
        };
        clear_streaming_info(root, stream_field)?;
        if image_texture_without_data {
            return Ok(StreamedYamlAssetPreparation::ImageTextureWithoutData);
        }
        return Ok(StreamedYamlAssetPreparation::StreamUnset);
    }

    let (inline_parent, inline_field) = match asset_kind {
        StreamedYamlAssetKind::Mesh => {
            let parent = unique_field_name(
                root,
                &["m_VertexData", "m_Vertex"],
                "Mesh vertex-data parent",
            )?
            .ok_or(StreamedYamlAssetError::InlineFieldMissing { asset_kind })?;
            let parent_value = root
                .unique_field_in_base_chain(parent)
                .map_err(|source| StreamedYamlAssetError::FieldLookup {
                    field: parent,
                    source,
                })?
                .ok_or(StreamedYamlAssetError::InlineFieldMissing { asset_kind })?;
            let inline =
                unique_field_name(parent_value, &["m_Data", "m_DataSize"], "Mesh byte-data")?
                    .ok_or(StreamedYamlAssetError::InlineFieldMissing { asset_kind })?;
            (Some(parent), inline)
        }
        StreamedYamlAssetKind::ImageTexture => (
            None,
            unique_field_name(
                root,
                &["m_ImageData", "m_ImageData_C28", "m_ImageData_C189"],
                "image-texture byte-data",
            )?
            .ok_or(StreamedYamlAssetError::InlineFieldMissing { asset_kind })?,
        ),
    };
    let inline_value = match inline_parent {
        Some(parent) => {
            let parent_value = root
                .unique_field_in_base_chain(parent)
                .map_err(|source| StreamedYamlAssetError::FieldLookup {
                    field: parent,
                    source,
                })?
                .ok_or(StreamedYamlAssetError::InlineFieldMissing { asset_kind })?;
            parent_value
                .unique_field_in_base_chain(inline_field)
                .map_err(|source| StreamedYamlAssetError::FieldLookup {
                    field: inline_field,
                    source,
                })?
        }
        None => root
            .unique_field_in_base_chain(inline_field)
            .map_err(|source| StreamedYamlAssetError::FieldLookup {
                field: inline_field,
                source,
            })?,
    }
    .ok_or(StreamedYamlAssetError::InlineFieldMissing { asset_kind })?;
    let inline_bytes: &[u8] = match inline_value {
        SerializedValue::Bytes(bytes) => bytes.as_slice(),
        SerializedValue::Array(values)
            if values
                .iter()
                .any(|value| !matches!(value, SerializedValue::U8(_))) =>
        {
            return Err(StreamedYamlAssetError::InlineFieldWrongType {
                field: inline_field,
                actual: "a non-byte array",
            });
        }
        SerializedValue::Array(values) => {
            if !values.is_empty() {
                clear_streaming_info(root, stream_field)?;
                return Ok(StreamedYamlAssetPreparation::ExistingInlineData { resource: info });
            }
            &[]
        }
        value => {
            return Err(StreamedYamlAssetError::InlineFieldWrongType {
                field: inline_field,
                actual: serialized_value_kind(value),
            });
        }
    };

    if !inline_bytes.is_empty() {
        clear_streaming_info(root, stream_field)?;
        return Ok(StreamedYamlAssetPreparation::ExistingInlineData { resource: info });
    }
    let content =
        materialized_content.ok_or_else(|| StreamedYamlAssetError::StreamContentRequired {
            resource: info.clone(),
        })?;
    if content.len() > limits.max_inline_bytes {
        return Err(StreamedYamlAssetError::InlineByteLimitExceeded {
            size: content.len(),
            limit: limits.max_inline_bytes,
        });
    }
    if content.len() != usize::try_from(info.size).unwrap_or(usize::MAX) {
        return Err(StreamedYamlAssetError::StreamContentSizeMismatch {
            expected: info.size,
            actual: content.len(),
        });
    }
    if content.is_empty() && asset_kind == StreamedYamlAssetKind::ImageTexture {
        clear_streaming_info(root, stream_field)?;
        return Ok(StreamedYamlAssetPreparation::ImageTextureWithoutData);
    }

    let mut owned_content = Vec::new();
    owned_content
        .try_reserve_exact(content.len())
        .map_err(|_| StreamedYamlAssetError::AllocationFailed {
            requested: content.len(),
        })?;
    owned_content.extend_from_slice(content);
    let inline_target = match inline_parent {
        Some(parent) => {
            let parent_value = root
                .unique_field_in_base_chain_mut(parent)
                .map_err(|source| StreamedYamlAssetError::FieldMutation {
                    field: parent,
                    source,
                })?
                .ok_or(StreamedYamlAssetError::InlineFieldMissing { asset_kind })?;
            parent_value
                .unique_field_in_base_chain_mut(inline_field)
                .map_err(|source| StreamedYamlAssetError::FieldMutation {
                    field: inline_field,
                    source,
                })?
                .ok_or(StreamedYamlAssetError::InlineFieldMissing { asset_kind })?
        }
        None => root
            .unique_field_in_base_chain_mut(inline_field)
            .map_err(|source| StreamedYamlAssetError::FieldMutation {
                field: inline_field,
                source,
            })?
            .ok_or(StreamedYamlAssetError::InlineFieldMissing { asset_kind })?,
    };
    match inline_target {
        SerializedValue::Bytes(bytes) => *bytes = owned_content,
        SerializedValue::Array(values) if values.is_empty() => {
            return Err(StreamedYamlAssetError::InlineFieldWrongType {
                field: inline_field,
                actual: "an empty byte array whose element shape is unavailable",
            });
        }
        _ => {
            return Err(StreamedYamlAssetError::InlineFieldWrongType {
                field: inline_field,
                actual: "a changed byte-data field",
            });
        }
    }
    clear_streaming_info(root, stream_field)?;
    Ok(StreamedYamlAssetPreparation::Materialized {
        resource: info,
        byte_count: content.len(),
    })
}

fn image_texture_inline_data_is_empty(
    root: &SerializedValue,
) -> Result<bool, StreamedYamlAssetError> {
    let field = unique_field_name(
        root,
        &["m_ImageData", "m_ImageData_C28", "m_ImageData_C189"],
        "image-texture byte-data",
    )?
    .ok_or(StreamedYamlAssetError::InlineFieldMissing {
        asset_kind: StreamedYamlAssetKind::ImageTexture,
    })?;
    let value = root
        .unique_field_in_base_chain(field)
        .map_err(|source| StreamedYamlAssetError::FieldLookup { field, source })?
        .ok_or(StreamedYamlAssetError::InlineFieldMissing {
            asset_kind: StreamedYamlAssetKind::ImageTexture,
        })?;
    match value {
        SerializedValue::Bytes(bytes) => Ok(bytes.is_empty()),
        SerializedValue::Array(values)
            if values
                .iter()
                .all(|value| matches!(value, SerializedValue::U8(_))) =>
        {
            Ok(values.is_empty())
        }
        SerializedValue::Array(_) => Err(StreamedYamlAssetError::InlineFieldWrongType {
            field,
            actual: "a non-byte array",
        }),
        value => Err(StreamedYamlAssetError::InlineFieldWrongType {
            field,
            actual: serialized_value_kind(value),
        }),
    }
}

fn stream_field_names(asset_kind: StreamedYamlAssetKind) -> &'static [&'static str] {
    match asset_kind {
        StreamedYamlAssetKind::Mesh => &["m_StreamData"],
        StreamedYamlAssetKind::ImageTexture => {
            &["m_StreamData_C189", "m_StreamData_C28", "m_StreamData"]
        }
    }
}

fn find_stream_field(
    root: &SerializedValue,
    asset_kind: StreamedYamlAssetKind,
) -> Result<Option<&'static str>, StreamedYamlAssetError> {
    unique_field_name(root, stream_field_names(asset_kind), "StreamingInfo")
}

/// Checks whether a supported streamed-resource field exists without copying the decoded payload.
pub fn has_streamed_yaml_asset_field(
    root: &SerializedValue,
    asset_kind: StreamedYamlAssetKind,
) -> Result<bool, StreamedYamlAssetError> {
    find_stream_field(root, asset_kind).map(|field| field.is_some())
}

fn unique_field_name(
    value: &SerializedValue,
    names: &[&'static str],
    role: &'static str,
) -> Result<Option<&'static str>, StreamedYamlAssetError> {
    let mut selected = None;
    for name in names {
        if value
            .unique_field_in_base_chain(name)
            .map_err(|source| StreamedYamlAssetError::FieldLookup {
                field: name,
                source,
            })?
            .is_some()
        {
            if let Some(first) = selected {
                return Err(StreamedYamlAssetError::AmbiguousFieldAliases {
                    role,
                    first,
                    second: name,
                });
            }
            selected = Some(*name);
        }
    }
    Ok(selected)
}

fn clear_streaming_info(
    root: &mut SerializedValue,
    stream_field: &'static str,
) -> Result<(), StreamedYamlAssetError> {
    let stream = root
        .unique_field_in_base_chain_mut(stream_field)
        .map_err(|source| StreamedYamlAssetError::FieldMutation {
            field: stream_field,
            source,
        })?
        .ok_or(StreamedYamlAssetError::StreamFieldWrongType {
            field: stream_field,
            actual: "missing",
        })?;
    if !matches!(stream, SerializedValue::Struct(_)) {
        return Err(StreamedYamlAssetError::StreamFieldWrongType {
            field: stream_field,
            actual: serialized_value_kind(stream),
        });
    }
    let path = stream
        .unique_field_in_base_chain_mut("path")
        .map_err(|source| StreamedYamlAssetError::FieldMutation {
            field: "path",
            source,
        })?
        .ok_or(StreamedYamlAssetError::StreamFieldWrongType {
            field: "path",
            actual: "missing",
        })?;
    match path {
        SerializedValue::StringBytes(bytes) => bytes.clear(),
        value => {
            return Err(StreamedYamlAssetError::StreamFieldWrongType {
                field: "path",
                actual: serialized_value_kind(value),
            });
        }
    }
    let offset = stream
        .unique_field_in_base_chain_mut("offset")
        .map_err(|source| StreamedYamlAssetError::FieldMutation {
            field: "offset",
            source,
        })?
        .ok_or(StreamedYamlAssetError::StreamFieldWrongType {
            field: "offset",
            actual: "missing",
        })?;
    match offset {
        SerializedValue::U32(value) => *value = 0,
        SerializedValue::U64(value) => *value = 0,
        value => {
            return Err(StreamedYamlAssetError::StreamFieldWrongOffsetType {
                field: "offset",
                actual: serialized_value_kind(value),
            });
        }
    }
    let size = stream
        .unique_field_in_base_chain_mut("size")
        .map_err(|source| StreamedYamlAssetError::FieldMutation {
            field: "size",
            source,
        })?
        .ok_or(StreamedYamlAssetError::StreamFieldWrongType {
            field: "size",
            actual: "missing",
        })?;
    match size {
        SerializedValue::U32(value) => *value = 0,
        value => {
            return Err(StreamedYamlAssetError::StreamFieldWrongType {
                field: "size",
                actual: serialized_value_kind(value),
            });
        }
    }
    Ok(())
}

fn serialized_value_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "SInt8",
        SerializedValue::U8(_) => "UInt8",
        SerializedValue::I16(_) => "SInt16",
        SerializedValue::U16(_) => "UInt16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "SInt32",
        SerializedValue::U32(_) => "UInt32",
        SerializedValue::I64(_) => "SInt64",
        SerializedValue::U64(_) => "UInt64",
        SerializedValue::F32(_) => "float",
        SerializedValue::F64(_) => "double",
        SerializedValue::StringBytes(_) => "string bytes",
        SerializedValue::Bytes(_) => "byte data",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::Raw { .. } => "raw data",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        prepare_streamed_yaml_asset, StreamedYamlAssetError, StreamedYamlAssetKind,
        StreamedYamlAssetLimits, StreamedYamlAssetPreparation,
    };
    use crate::resource::extract_streamed_resource_info;
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn stream(path: &str, offset: SerializedValue, size: u32) -> SerializedValue {
        SerializedValue::Struct(vec![
            field(
                "path",
                SerializedValue::StringBytes(path.as_bytes().to_vec()),
            ),
            field("offset", offset),
            field("size", SerializedValue::U32(size)),
        ])
    }

    fn mesh_payload(inline: SerializedValue, offset: SerializedValue) -> SerializedValue {
        SerializedValue::Struct(vec![
            field("m_StreamData", stream("mesh.resS", offset, 3)),
            field(
                "m_VertexData",
                SerializedValue::Struct(vec![field("m_DataSize", inline)]),
            ),
        ])
    }

    fn texture_payload(inline: SerializedValue, offset: SerializedValue) -> SerializedValue {
        SerializedValue::Struct(vec![
            field("m_StreamData_C189", stream("texture.resS", offset, 3)),
            field("m_ImageData_C189", inline),
        ])
    }

    fn streamed_info(
        root: &SerializedValue,
        field_name: &str,
    ) -> crate::resource::StreamedResourceInfo {
        extract_streamed_resource_info(
            root.unique_field_in_base_chain(field_name)
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn mesh_yaml_staging_materializes_bytes_and_clears_uint32_stream_fields() {
        let source = mesh_payload(SerializedValue::Bytes(Vec::new()), SerializedValue::U32(7));
        let mut staged = source.clone();

        let result = prepare_streamed_yaml_asset(
            &mut staged,
            StreamedYamlAssetKind::Mesh,
            Some(b"abc"),
            StreamedYamlAssetLimits::default(),
        )
        .unwrap();

        assert_eq!(
            source,
            mesh_payload(SerializedValue::Bytes(Vec::new()), SerializedValue::U32(7))
        );
        assert_eq!(
            result,
            StreamedYamlAssetPreparation::Materialized {
                resource: crate::resource::StreamedResourceInfo {
                    path: "mesh.resS".to_owned(),
                    offset: 7,
                    size: 3,
                },
                byte_count: 3,
            }
        );
        assert_eq!(
            staged
                .unique_field_in_base_chain("m_VertexData")
                .unwrap()
                .unwrap()
                .unique_field_in_base_chain("m_DataSize")
                .unwrap()
                .unwrap(),
            &SerializedValue::Bytes(b"abc".to_vec())
        );
        assert_eq!(
            streamed_info(&staged, "m_StreamData"),
            crate::resource::StreamedResourceInfo {
                path: String::new(),
                offset: 0,
                size: 0,
            }
        );
    }

    #[test]
    fn texture_yaml_staging_uses_existing_inline_data_without_fetching_stream() {
        let mut root = texture_payload(
            SerializedValue::Bytes(b"xyz".to_vec()),
            SerializedValue::U64(11),
        );

        let result = prepare_streamed_yaml_asset(
            &mut root,
            StreamedYamlAssetKind::ImageTexture,
            None,
            StreamedYamlAssetLimits::default(),
        )
        .unwrap();

        assert_eq!(
            result,
            StreamedYamlAssetPreparation::ExistingInlineData {
                resource: crate::resource::StreamedResourceInfo {
                    path: "texture.resS".to_owned(),
                    offset: 11,
                    size: 3,
                },
            }
        );
        assert_eq!(
            root.unique_field_in_base_chain("m_ImageData_C189")
                .unwrap()
                .unwrap(),
            &SerializedValue::Bytes(b"xyz".to_vec())
        );
        assert_eq!(
            streamed_info(&root, "m_StreamData_C189"),
            crate::resource::StreamedResourceInfo {
                path: String::new(),
                offset: 0,
                size: 0,
            }
        );
    }

    #[test]
    fn unset_stream_metadata_is_cleared_before_yaml_staging() {
        let mut root = texture_payload(
            SerializedValue::Bytes(b"xyz".to_vec()),
            SerializedValue::U64(17),
        );
        let stream = root
            .unique_field_in_base_chain_mut("m_StreamData_C189")
            .unwrap()
            .unwrap();
        let path = stream
            .unique_field_in_base_chain_mut("path")
            .unwrap()
            .unwrap();
        *path = SerializedValue::StringBytes(Vec::new());

        assert_eq!(
            prepare_streamed_yaml_asset(
                &mut root,
                StreamedYamlAssetKind::ImageTexture,
                None,
                StreamedYamlAssetLimits::default(),
            )
            .unwrap(),
            StreamedYamlAssetPreparation::StreamUnset
        );
        assert_eq!(
            streamed_info(&root, "m_StreamData_C189"),
            crate::resource::StreamedResourceInfo {
                path: String::new(),
                offset: 0,
                size: 0,
            }
        );
        assert_eq!(
            root.unique_field_in_base_chain("m_ImageData_C189")
                .unwrap()
                .unwrap(),
            &SerializedValue::Bytes(b"xyz".to_vec())
        );
    }

    #[test]
    fn unset_image_texture_without_inline_bytes_is_marked_for_skip() {
        let mut root =
            texture_payload(SerializedValue::Bytes(Vec::new()), SerializedValue::U64(17));
        let stream = root
            .unique_field_in_base_chain_mut("m_StreamData_C189")
            .unwrap()
            .unwrap();
        let path = stream
            .unique_field_in_base_chain_mut("path")
            .unwrap()
            .unwrap();
        *path = SerializedValue::StringBytes(Vec::new());

        assert_eq!(
            prepare_streamed_yaml_asset(
                &mut root,
                StreamedYamlAssetKind::ImageTexture,
                None,
                StreamedYamlAssetLimits::default(),
            )
            .unwrap(),
            StreamedYamlAssetPreparation::ImageTextureWithoutData
        );
        assert_eq!(
            streamed_info(&root, "m_StreamData_C189"),
            crate::resource::StreamedResourceInfo {
                path: String::new(),
                offset: 0,
                size: 0,
            }
        );
    }

    #[test]
    fn zero_length_image_texture_stream_is_marked_for_skip() {
        let mut root =
            texture_payload(SerializedValue::Bytes(Vec::new()), SerializedValue::U64(17));
        let stream = root
            .unique_field_in_base_chain_mut("m_StreamData_C189")
            .unwrap()
            .unwrap();
        let size = stream
            .unique_field_in_base_chain_mut("size")
            .unwrap()
            .unwrap();
        *size = SerializedValue::U32(0);

        assert_eq!(
            prepare_streamed_yaml_asset(
                &mut root,
                StreamedYamlAssetKind::ImageTexture,
                Some(&[]),
                StreamedYamlAssetLimits::default(),
            )
            .unwrap(),
            StreamedYamlAssetPreparation::ImageTextureWithoutData
        );
    }

    #[test]
    fn streamed_yaml_staging_requires_exact_bounded_content_before_mutating_payload() {
        let source = texture_payload(SerializedValue::Bytes(Vec::new()), SerializedValue::U64(13));
        let mut missing_content = source.clone();
        assert!(matches!(
            prepare_streamed_yaml_asset(
                &mut missing_content,
                StreamedYamlAssetKind::ImageTexture,
                None,
                StreamedYamlAssetLimits::default(),
            ),
            Err(StreamedYamlAssetError::StreamContentRequired { .. })
        ));
        assert_eq!(missing_content, source);

        let mut wrong_size = source.clone();
        assert!(matches!(
            prepare_streamed_yaml_asset(
                &mut wrong_size,
                StreamedYamlAssetKind::ImageTexture,
                Some(b"ab"),
                StreamedYamlAssetLimits::default(),
            ),
            Err(StreamedYamlAssetError::StreamContentSizeMismatch {
                expected: 3,
                actual: 2
            })
        ));
        assert_eq!(wrong_size, source);

        assert!(matches!(
            prepare_streamed_yaml_asset(
                &mut wrong_size,
                StreamedYamlAssetKind::ImageTexture,
                Some(b"abc"),
                StreamedYamlAssetLimits {
                    max_inline_bytes: 2,
                    ..StreamedYamlAssetLimits::default()
                },
            ),
            Err(StreamedYamlAssetError::InlineByteLimitExceeded { size: 3, limit: 2 })
        ));
        assert_eq!(wrong_size, source);
    }
}
