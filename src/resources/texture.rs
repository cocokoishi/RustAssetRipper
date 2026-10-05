//! `Texture2D` (class 28) field view and bitmap export.
//!
//! Ports the field reads that drive `TextureConverter.TryConvertToBitmap(ITexture2D)`:
//! `Texture2DExtensions.CompleteImageSize`/`ActualImageSize`, the inline-vs-streamed image-data
//! selection of `GetImageData`, the `Height * Depth` layer layout, and the `NormalmapDXT5nm`
//! normal-map unpack.
//!
//! Open, and deliberately not guessed here: the Xbox360 `IsSwapBytes` byte swap and the Switch
//! `SwitchSwizzle.Unswizzle` platform transforms; the class-189 `ImageTexture` `GraphicsFormat`
//! route; and the generated `Has_*` version gates that decide which `m_*` field alias exists.
//! A view built from a payload missing a required field returns an explicit error rather than a
//! zero-filled texture.

use std::error::Error;
use std::fmt::{self, Display};

use crate::resource::{
    extract_streamed_resource_info, StreamedResourceInfo, StreamedResourceInfoError,
};
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};

use super::image_export::{ImageExportFormat, ImageWriteError};
use super::texture_converter::{try_convert_to_bitmap, TextureConvertError, TextureImage};

/// `TextureUsageMode.NormalmapDXT5nm`; the only lightmap format that triggers `UnpackNormal`.
pub const TEXTURE_USAGE_MODE_NORMALMAP_DXT5NM: u8 = 5;

/// A failure while reading a `Texture2D` field view.
#[derive(Debug)]
pub enum TextureViewError {
    /// A field lookup found duplicate names along the base chain.
    FieldLookup {
        /// The field being read.
        field: &'static str,
        /// The underlying lookup failure.
        source: SerializedValueLookupError,
    },
    /// A required field is absent.
    MissingField {
        /// The required field name.
        field: &'static str,
    },
    /// A field is present but has the wrong value kind.
    WrongType {
        /// The field being read.
        field: &'static str,
        /// The value kind actually decoded.
        actual: &'static str,
    },
    /// The StreamingInfo field could not be decoded.
    StreamInfo {
        /// The field being read.
        field: &'static str,
        /// The underlying decode failure.
        source: StreamedResourceInfoError,
    },
}

impl Display for TextureViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field, source } => {
                write!(
                    formatter,
                    "could not read Texture2D field '{field}': {source}"
                )
            }
            Self::MissingField { field } => {
                write!(formatter, "Texture2D is missing required field '{field}'")
            }
            Self::WrongType { field, actual } => {
                write!(
                    formatter,
                    "Texture2D field '{field}' has kind {actual}, expected a number"
                )
            }
            Self::StreamInfo { field, source } => {
                write!(
                    formatter,
                    "could not decode StreamingInfo field '{field}': {source}"
                )
            }
        }
    }
}

impl Error for TextureViewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FieldLookup { source, .. } => Some(source),
            Self::StreamInfo { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// A failure while exporting a `Texture2D` to an image.
#[derive(Debug)]
pub enum TextureExportError {
    /// The field view could not be built.
    View(TextureViewError),
    /// The image data is unavailable: neither inline bytes nor a resolvable stream.
    ImageDataUnavailable,
    /// The decoder or converter rejected the image.
    Convert(TextureConvertError),
    /// The image writer rejected the output.
    Image(ImageWriteError),
}

impl From<TextureViewError> for TextureExportError {
    fn from(error: TextureViewError) -> Self {
        Self::View(error)
    }
}

impl From<TextureConvertError> for TextureExportError {
    fn from(error: TextureConvertError) -> Self {
        Self::Convert(error)
    }
}

impl From<ImageWriteError> for TextureExportError {
    fn from(error: ImageWriteError) -> Self {
        Self::Image(error)
    }
}

impl Display for TextureExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::View(error) => write!(formatter, "{error}"),
            Self::ImageDataUnavailable => formatter
                .write_str("Texture2D has neither inline image data nor a resolvable stream"),
            Self::Convert(error) => write!(formatter, "{error}"),
            Self::Image(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for TextureExportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::View(error) => Some(error),
            Self::Convert(error) => Some(error),
            Self::Image(error) => Some(error),
            Self::ImageDataUnavailable => None,
        }
    }
}

/// The fields `TextureConverter` reads from a class-28 `Texture2D`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Texture2DView {
    /// `m_Width`.
    pub width: usize,
    /// `m_Height`.
    pub height: usize,
    /// `m_TextureFormat`; `0` means the graphics-format route.
    pub texture_format: i32,
    /// `m_CompleteImageSize`; `0` means `ActualImageSize` is calculated.
    pub complete_image_size: usize,
    /// `m_ImageCount` (`Depth`).
    pub image_count: usize,
    /// `m_MipCount`; `> 1` means the texture has mipmaps.
    pub mip_count: i32,
    /// `m_LightmapFormat`.
    pub lightmap_format: u8,
    /// Inline `image data` bytes.
    pub image_data: Vec<u8>,
    /// `m_StreamData`, if present.
    pub stream_data: Option<StreamedResourceInfo>,
}

impl Texture2DView {
    /// Reads the export-relevant fields from a decoded `Texture2D` payload.
    pub fn from_value(root: &SerializedValue) -> Result<Self, TextureViewError> {
        let width = required_i32(root, &["m_Width"])?;
        let height = required_i32(root, &["m_Height"])?;
        let texture_format = optional_i32(root, &["m_TextureFormat", "m_Format"])?.unwrap_or(0);
        let complete_image_size =
            optional_number(root, &["m_CompleteImageSize", "m_DataSize"])?.unwrap_or(0);
        let image_count = optional_i32(root, &["m_ImageCount", "m_Depth"])?.unwrap_or(1);
        let mip_count = optional_i32(root, &["m_MipCount"])?.unwrap_or(0);
        let lightmap_format = optional_u8(root, &["m_LightmapFormat"])?.unwrap_or(0);
        let image_data =
            optional_bytes(root, &["image data", "m_ImageData_C28"])?.unwrap_or_default();
        let stream_data = optional_stream(root, &["m_StreamData", "m_StreamData_C28"])?;

        Ok(Self {
            width: nonnegative(width, "m_Width")?,
            height: nonnegative(height, "m_Height")?,
            texture_format,
            complete_image_size,
            image_count: nonnegative(image_count, "m_ImageCount")?,
            mip_count,
            lightmap_format,
            image_data,
            stream_data,
        })
    }

    /// `Texture2DExtensions.ImageDataLength`.
    pub fn image_data_length(&self) -> usize {
        if !self.image_data.is_empty() {
            self.image_data.len()
        } else {
            self.stream_data
                .as_ref()
                .map(|info| info.size as usize)
                .unwrap_or(0)
        }
    }

    /// `Texture2DExtensions.ActualImageSize`.
    pub fn actual_image_size(&self) -> usize {
        if self.complete_image_size > 0 {
            return self.complete_image_size;
        }
        let data_length = self.image_data_length();
        if self.image_count > 1 && data_length.is_multiple_of(self.image_count) {
            data_length / self.image_count
        } else {
            data_length
        }
    }

    /// Whether the source `MipMap`/`MipCount` marks the texture as mipmapped.
    pub fn has_mips(&self) -> bool {
        self.mip_count > 1
    }

    /// The image bytes `GetImageData` would return, given optional streamed content.
    pub fn image_bytes<'a>(&'a self, stream_content: Option<&'a [u8]>) -> Option<&'a [u8]> {
        if !self.image_data.is_empty() {
            Some(self.image_data.as_slice())
        } else {
            stream_content
        }
    }

    /// Builds the converter input for this texture.
    pub fn to_texture_image<'a>(&'a self, data: &'a [u8]) -> TextureImage<'a> {
        TextureImage {
            texture_format: self.texture_format,
            graphics_format: 0,
            width: self.width,
            height: self.height,
            depth: self.image_count.max(1),
            image_size: self.actual_image_size(),
            data,
        }
    }
}

/// Exports one `Texture2D` to an encoded image.
///
/// `stream_content` must be the exact byte range for `m_StreamData` when the texture has no inline
/// data; it is ignored otherwise, matching `GetImageData`'s precedence.
pub fn export_texture2d_image(
    view: &Texture2DView,
    stream_content: Option<&[u8]>,
    format: ImageExportFormat,
) -> Result<Vec<u8>, TextureExportError> {
    let data = view
        .image_bytes(stream_content)
        .ok_or(TextureExportError::ImageDataUnavailable)?;
    let image = view.to_texture_image(data);
    let unpack_normal = view.lightmap_format == TEXTURE_USAGE_MODE_NORMALMAP_DXT5NM;
    let bitmap = try_convert_to_bitmap(&image, true, unpack_normal)?;
    let mut output = Vec::new();
    bitmap.save(&mut output, format)?;
    Ok(output)
}

/// Exports one `Texture2D` directly from its decoded payload.
pub fn export_texture2d_value(
    root: &SerializedValue,
    stream_content: Option<&[u8]>,
    format: ImageExportFormat,
) -> Result<Vec<u8>, TextureExportError> {
    let view = Texture2DView::from_value(root)?;
    export_texture2d_image(&view, stream_content, format)
}

fn nonnegative(value: i32, field: &'static str) -> Result<usize, TextureViewError> {
    usize::try_from(value).map_err(|_| TextureViewError::WrongType {
        field,
        actual: "a negative integer",
    })
}

fn serialized_kind(value: &SerializedValue) -> &'static str {
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

fn lookup<'a>(
    root: &'a SerializedValue,
    names: &[&'static str],
) -> Result<Option<(&'static str, &'a SerializedValue)>, TextureViewError> {
    let mut selected = None;
    for name in names {
        if let Some(value) = root.unique_field_in_base_chain(name).map_err(|source| {
            TextureViewError::FieldLookup {
                field: name,
                source,
            }
        })? {
            if selected.is_some() {
                // More than one alias is present; the source generated class would expose only
                // one, so this payload shape is not one the baseline models.
                return Err(TextureViewError::WrongType {
                    field: name,
                    actual: "an ambiguous alias set",
                });
            }
            selected = Some((*name, value));
        }
    }
    Ok(selected)
}

fn required_i32(root: &SerializedValue, names: &[&'static str]) -> Result<i32, TextureViewError> {
    match lookup(root, names)? {
        Some((_, SerializedValue::I32(value))) => Ok(*value),
        Some((_, SerializedValue::U32(value))) => Ok(*value as i32),
        Some((field, value)) => Err(TextureViewError::WrongType {
            field,
            actual: serialized_kind(value),
        }),
        None => Err(TextureViewError::MissingField { field: names[0] }),
    }
}

fn optional_i32(
    root: &SerializedValue,
    names: &[&'static str],
) -> Result<Option<i32>, TextureViewError> {
    match lookup(root, names)? {
        None => Ok(None),
        Some((_, SerializedValue::I32(value))) => Ok(Some(*value)),
        Some((_, SerializedValue::U32(value))) => Ok(Some(*value as i32)),
        Some((field, value)) => Err(TextureViewError::WrongType {
            field,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_number(
    root: &SerializedValue,
    names: &[&'static str],
) -> Result<Option<usize>, TextureViewError> {
    match lookup(root, names)? {
        None => Ok(None),
        Some((_, SerializedValue::I32(value))) => Ok(Some(nonnegative(*value, names[0])?)),
        Some((_, SerializedValue::U32(value))) => Ok(Some(*value as usize)),
        Some((field, value)) => Err(TextureViewError::WrongType {
            field,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_u8(
    root: &SerializedValue,
    names: &[&'static str],
) -> Result<Option<u8>, TextureViewError> {
    match lookup(root, names)? {
        None => Ok(None),
        Some((_, SerializedValue::U8(value))) => Ok(Some(*value)),
        Some((_, SerializedValue::I32(value))) => Ok(Some(*value as u8)),
        Some((field, value)) => Err(TextureViewError::WrongType {
            field,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_bytes(
    root: &SerializedValue,
    names: &[&'static str],
) -> Result<Option<Vec<u8>>, TextureViewError> {
    match lookup(root, names)? {
        None => Ok(None),
        Some((_, SerializedValue::Bytes(bytes))) => Ok(Some(bytes.clone())),
        Some((_, SerializedValue::Array(values))) => {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(values.len())
                .map_err(|_| TextureViewError::WrongType {
                    field: names[0],
                    actual: "an unallocatable byte array",
                })?;
            for value in values {
                match value {
                    SerializedValue::U8(byte) => bytes.push(*byte),
                    other => {
                        return Err(TextureViewError::WrongType {
                            field: names[0],
                            actual: serialized_kind(other),
                        })
                    }
                }
            }
            Ok(Some(bytes))
        }
        Some((field, value)) => Err(TextureViewError::WrongType {
            field,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_stream(
    root: &SerializedValue,
    names: &[&'static str],
) -> Result<Option<StreamedResourceInfo>, TextureViewError> {
    match lookup(root, names)? {
        None => Ok(None),
        Some((field, value)) => {
            let info = extract_streamed_resource_info(value)
                .map_err(|source| TextureViewError::StreamInfo { field, source })?;
            Ok(Some(info))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialized_file::object::SerializedFieldValue;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn rgba32_texture() -> SerializedValue {
        SerializedValue::Struct(vec![
            field("m_Width", SerializedValue::I32(2)),
            field("m_Height", SerializedValue::I32(1)),
            field("m_TextureFormat", SerializedValue::I32(4)),
            field("m_CompleteImageSize", SerializedValue::I32(8)),
            field("m_ImageCount", SerializedValue::I32(1)),
            field("m_LightmapFormat", SerializedValue::U8(0)),
            field(
                "image data",
                SerializedValue::Bytes(vec![1, 2, 3, 4, 5, 6, 7, 8]),
            ),
        ])
    }

    #[test]
    fn reads_the_export_fields() {
        let view = Texture2DView::from_value(&rgba32_texture()).unwrap();
        assert_eq!(view.width, 2);
        assert_eq!(view.height, 1);
        assert_eq!(view.texture_format, 4);
        assert_eq!(view.complete_image_size, 8);
        assert_eq!(view.image_count, 1);
        assert_eq!(view.actual_image_size(), 8);
        assert_eq!(view.image_data_length(), 8);
    }

    #[test]
    fn actual_image_size_falls_back_to_data_length() {
        let mut root = rgba32_texture();
        let SerializedValue::Struct(fields) = &mut root else {
            unreachable!()
        };
        fields.retain(|f| f.name != "m_CompleteImageSize");
        let view = Texture2DView::from_value(&root).unwrap();
        assert_eq!(view.actual_image_size(), 8);

        // With two layers and divisible data, the size is per-layer.
        let mut layered = root.clone();
        let SerializedValue::Struct(fields) = &mut layered else {
            unreachable!()
        };
        let image_count = fields
            .iter_mut()
            .find(|f| f.name == "m_ImageCount")
            .unwrap();
        image_count.value = SerializedValue::I32(2);
        let view = Texture2DView::from_value(&layered).unwrap();
        assert_eq!(view.actual_image_size(), 4);
    }

    #[test]
    fn missing_required_field_is_reported() {
        let root = SerializedValue::Struct(vec![field("m_Height", SerializedValue::I32(1))]);
        assert!(matches!(
            Texture2DView::from_value(&root),
            Err(TextureViewError::MissingField { field: "m_Width" })
        ));
    }

    #[test]
    fn negative_dimension_is_rejected() {
        let mut root = rgba32_texture();
        let SerializedValue::Struct(fields) = &mut root else {
            unreachable!()
        };
        fields[0] = field("m_Width", SerializedValue::I32(-1));
        assert!(matches!(
            Texture2DView::from_value(&root),
            Err(TextureViewError::WrongType {
                field: "m_Width",
                ..
            })
        ));
    }

    #[test]
    fn exports_an_rgba32_texture_to_png() {
        let output =
            export_texture2d_value(&rgba32_texture(), None, ImageExportFormat::Png).unwrap();
        assert_eq!(
            &output[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
    }

    #[test]
    fn inline_data_takes_precedence_over_stream() {
        let view = Texture2DView::from_value(&rgba32_texture()).unwrap();
        let stream = [9u8, 9, 9, 9, 9, 9, 9, 9];
        assert_eq!(view.image_bytes(Some(&stream)), Some(&view.image_data[..]));
    }

    #[test]
    fn streamed_texture_without_content_is_reported() {
        let mut root = rgba32_texture();
        let SerializedValue::Struct(fields) = &mut root else {
            unreachable!()
        };
        fields.retain(|f| f.name != "image data");
        fields.push(field(
            "m_StreamData",
            SerializedValue::Struct(vec![
                field("path", SerializedValue::StringBytes(b"tex.resS".to_vec())),
                field("offset", SerializedValue::U64(4)),
                field("size", SerializedValue::U32(8)),
            ]),
        ));
        assert!(matches!(
            export_texture2d_value(&root, None, ImageExportFormat::Png),
            Err(TextureExportError::ImageDataUnavailable)
        ));
        // Providing the exact streamed content succeeds.
        let content = [1u8, 2, 3, 4, 5, 6, 7, 8];
        assert!(export_texture2d_value(&root, Some(&content), ImageExportFormat::Png).is_ok());
    }

    #[test]
    fn normalmap_lightmap_format_unpacks_blue() {
        let view = Texture2DView {
            width: 1,
            height: 1,
            texture_format: 4,
            complete_image_size: 4,
            image_count: 1,
            mip_count: 0,
            lightmap_format: TEXTURE_USAGE_MODE_NORMALMAP_DXT5NM,
            image_data: vec![128, 128, 0, 255],
            stream_data: None,
        };
        let output = export_texture2d_image(&view, None, ImageExportFormat::Png).unwrap();
        assert!(output.len() > 8);
    }

    #[test]
    fn ambiguous_aliases_are_rejected() {
        let mut root = rgba32_texture();
        let SerializedValue::Struct(fields) = &mut root else {
            unreachable!()
        };
        fields.push(field("m_Format", SerializedValue::I32(4)));
        assert!(matches!(
            Texture2DView::from_value(&root),
            Err(TextureViewError::WrongType {
                field: "m_Format",
                ..
            })
        ));
    }
}
