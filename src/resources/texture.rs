//! `Texture2D` (class 28) field view and bitmap export.
//!
//! Ports the field reads that drive `TextureConverter.TryConvertToBitmap(ITexture2D)`:
//! `Texture2DExtensions.CompleteImageSize`/`ActualImageSize`/`Mips`, the `GetImageData`
//! platform pipeline (inline-vs-streamed selection, the Xbox360 adjacent-pair
//! swap, Switch GOB unswizzling), the `Height * Depth` layer layout, and the
//! `NormalmapDXT5nm` normal-map unpack.
//!
//! Open, and deliberately not guessed here: the class-189 `ImageTexture` `GraphicsFormat`
//! route; and the generated `Has_*` version gates that decide which `m_*` field alias exists.
//! A view built from a payload missing a required field returns an explicit error rather than a
//! zero-filled texture.
//!
//! Platform facts (`Collection.Platform`, `Collection.EndianType`) arrive as caller
//! arguments: the CLI reads them from the SerializedFile metadata prefix. An unknown
//! platform (discriminant 0, which no `BuildTarget` member uses) applies neither transform.

use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Display};

use crate::resource::{
    extract_streamed_resource_info, StreamedResourceInfo, StreamedResourceInfoError,
};
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};

use super::image_export::{ImageExportFormat, ImageWriteError};
use super::switch_swizzle::{SwitchSwizzleError, SwitchUnswizzleInput, PLATFORM_BLOB_MIN_LENGTH};
use super::texture_array::{
    cubemap_array_complete_image_size, cubemap_array_depth, resolve_array_image_data,
    texture2d_array_complete_image_size, CubemapArrayView, Texture2DArrayView, Texture3DView,
    TextureArrayError,
};
use super::texture_converter::{try_convert_to_bitmap, TextureConvertError, TextureImage};
use super::texture_format::{
    is_swap_bytes, swap_adjacent_bytes, SwapBytesError, SWITCH_BUILD_TARGET,
};

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
    /// The Xbox360 adjacent-pair swap rejected the byte count.
    Swap(SwapBytesError),
    /// Switch GOB unswizzling rejected the inputs.
    Unswizzle(SwitchSwizzleError),
    /// An array/3D texture rule rejected the inputs.
    Array(TextureArrayError),
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

impl From<SwapBytesError> for TextureExportError {
    fn from(error: SwapBytesError) -> Self {
        Self::Swap(error)
    }
}

impl From<SwitchSwizzleError> for TextureExportError {
    fn from(error: SwitchSwizzleError) -> Self {
        Self::Unswizzle(error)
    }
}

impl From<TextureArrayError> for TextureExportError {
    fn from(error: TextureArrayError) -> Self {
        Self::Array(error)
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
            Self::Swap(error) => write!(formatter, "{error}"),
            Self::Unswizzle(error) => write!(formatter, "{error}"),
            Self::Array(error) => write!(formatter, "{error}"),
            Self::Convert(error) => write!(formatter, "{error}"),
            Self::Image(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for TextureExportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::View(error) => Some(error),
            Self::Swap(error) => Some(error),
            Self::Unswizzle(error) => Some(error),
            Self::Array(error) => Some(error),
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
    /// `m_MipCount`; `> 1` means the texture has mipmaps even without the flag below.
    pub mip_count: i32,
    /// `m_MipMap`; OR-ed with `m_MipCount > 1` by `Texture2DExtensions.Mips`.
    /// Payloads whose class predates the field behave as `false`, like the
    /// version-gated blob below.
    pub mip_map: bool,
    /// `m_LightmapFormat`.
    pub lightmap_format: u8,
    /// Inline `image data` bytes.
    pub image_data: Vec<u8>,
    /// `m_StreamData`, if present.
    pub stream_data: Option<StreamedResourceInfo>,
    /// `m_PlatformBlob` (the Switch texel-height word lives at offset 8).
    /// The field is version-gated in the source (present on 2020.2+, absent on
    /// 2020.1 and older); a payload without it behaves like an empty blob.
    pub platform_blob: Vec<u8>,
}

impl Texture2DView {
    /// Reads the export-relevant fields from a decoded `Texture2D` payload.
    pub fn from_value(root: &SerializedValue) -> Result<Self, TextureViewError> {
        let width = required_i32(root, &["m_Width"])?;
        let height = required_i32(root, &["m_Height"])?;
        let texture_format = optional_i32(root, &["m_TextureFormat"])?.unwrap_or(0);
        let complete_image_size =
            optional_number(root, &["m_CompleteImageSize", "m_DataSize"])?.unwrap_or(0);
        let image_count = optional_i32(root, &["m_ImageCount", "m_Depth"])?.unwrap_or(1);
        let mip_count = optional_i32(root, &["m_MipCount"])?.unwrap_or(0);
        let mip_map = optional_bool(root, &["m_MipMap"])?.unwrap_or(false);
        let lightmap_format = optional_u8(root, &["m_LightmapFormat"])?.unwrap_or(0);
        let image_data =
            optional_bytes(root, &["image data", "m_ImageData_C28"])?.unwrap_or_default();
        let stream_data = optional_stream(root, &["m_StreamData", "m_StreamData_C28"])?;
        let platform_blob = optional_bytes(root, &["m_PlatformBlob"])?.unwrap_or_default();

        Ok(Self {
            width: nonnegative(width, "m_Width")?,
            height: nonnegative(height, "m_Height")?,
            texture_format,
            complete_image_size,
            image_count: nonnegative(image_count, "m_ImageCount")?,
            mip_count,
            mip_map,
            lightmap_format,
            image_data,
            stream_data,
            platform_blob,
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

    /// `Texture2DExtensions.Mips`: `MipMap_C28 || MipCount_C28 > 1`.
    ///
    /// This is the flag `ImporterFactory.TextureImporterData` copies into
    /// `EnableMipMap` for `ITexture2D` assets.
    pub fn has_mips(&self) -> bool {
        self.mip_map || self.mip_count > 1
    }

    /// The image bytes `GetImageData` would return, given optional streamed content.
    pub fn image_bytes<'a>(&'a self, stream_content: Option<&'a [u8]>) -> Option<&'a [u8]> {
        if !self.image_data.is_empty() {
            Some(self.image_data.as_slice())
        } else {
            stream_content
        }
    }

    /// `Texture2DExtensions.GetImageData` over caller facts: the collection
    /// platform discriminant, the collection endianness for the blob shift, and
    /// the caller-resolved streamed content.
    ///
    /// Inline bytes win when non-empty (copied before an Xbox360 swap so the
    /// view is never mutated, exactly like the source copy); otherwise the
    /// streamed content is swapped in place, like the freshly fetched source
    /// buffer. The Switch unswizzle runs exactly when the blob holds at least
    /// 12 bytes and the platform discriminant is `Switch` (38); `Switch2` does
    /// not qualify. Untransformed bytes borrow the view; transformed bytes are
    /// owned. An odd swap count is an explicit error where the source loop
    /// throws `IndexOutOfRangeException`.
    pub fn resolved_image_data<'a>(
        &'a self,
        platform: i32,
        platform_blob_little_endian: bool,
        stream_content: Option<&'a [u8]>,
    ) -> Result<Cow<'a, [u8]>, TextureExportError> {
        let swap = is_swap_bytes(platform, self.texture_format);
        let unswizzle =
            platform == SWITCH_BUILD_TARGET && self.platform_blob.len() >= PLATFORM_BLOB_MIN_LENGTH;
        let mut data: Cow<'a, [u8]> = match self.image_bytes(stream_content) {
            // The swap step below always copies before swapping, so borrowing
            // here never mutates the view; the source's explicit copy and this
            // borrow-then-copy are observably identical.
            Some(bytes) => Cow::Borrowed(bytes),
            // No inline bytes and no stream field: `GetImageData` returns the
            // empty array (transforms below are no-ops over it, except a gated
            // unswizzle, which fails exactly where the source copy throws).
            None if self.stream_data.is_none() => Cow::Borrowed(&[]),
            // A declared stream the caller could not resolve stays an error.
            None => return Err(TextureExportError::ImageDataUnavailable),
        };
        if swap {
            data = Cow::Owned(swap_adjacent_bytes(&data)?);
        }
        if unswizzle {
            let input = SwitchUnswizzleInput {
                image_count: self.image_count as u32,
                format: self.texture_format,
                width: self.width as u32,
                height: self.height as u32,
                platform_blob: self.platform_blob.as_slice(),
                platform_blob_little_endian,
                data: &data,
            };
            data = Cow::Owned(super::switch_swizzle::unswizzle(&input)?);
        }
        Ok(data)
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

/// The class ID of `Cubemap` (class 89): it implements `ITexture2D` with the same
/// `_C28` fields, but `TryConvertToBitmap` skips the vertical flip for it
/// (`texture is not ICubemap`).
pub const CUBEMAP_CLASS_ID: i32 = 89;

/// The class ID of `ImageTexture` (class 189), the format-less texture base whose
/// concrete subtypes carry the `GraphicsFormat`.
pub const IMAGE_TEXTURE_CLASS_ID: i32 = 189;

/// The fields `ImageTextureExtensions` reads from a class-189 `ImageTexture`.
///
/// Unlike class 28, the 189 base carries no format field, no mip/size fields, and
/// no platform blob: format selection lives on the concrete subtypes, and neither
/// the Xbox360 byte swap nor the Switch unswizzle applies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageTextureView {
    /// `m_Width`.
    pub width: usize,
    /// `m_Height`.
    pub height: usize,
    /// Inline image bytes (`image data`, `m_ImageData`, `m_ImageData_C189`).
    pub image_data: Vec<u8>,
    /// `m_StreamData` (`m_StreamData_C189`), if present.
    pub stream_data: Option<StreamedResourceInfo>,
}

impl ImageTextureView {
    /// Reads the export-relevant fields from a decoded `ImageTexture` payload.
    pub fn from_value(root: &SerializedValue) -> Result<Self, TextureViewError> {
        let width = required_i32(root, &["m_Width"])?;
        let height = required_i32(root, &["m_Height"])?;
        let image_data = optional_bytes(root, &["image data", "m_ImageData", "m_ImageData_C189"])?
            .unwrap_or_default();
        let stream_data = optional_stream(root, &["m_StreamData", "m_StreamData_C189"])?;

        Ok(Self {
            width: nonnegative(width, "m_Width")?,
            height: nonnegative(height, "m_Height")?,
            image_data,
            stream_data,
        })
    }

    /// `ImageTextureExtensions.GetImageData` over caller-resolved streamed content:
    /// inline bytes win when non-empty, otherwise the streamed content (already
    /// gated on presence by the caller, like `IsSet`), otherwise nothing.
    ///
    /// `CheckAssetIntegrity`'s collection-backed stream check stays caller-side.
    pub fn image_bytes<'a>(&'a self, stream_content: Option<&'a [u8]>) -> Option<&'a [u8]> {
        if !self.image_data.is_empty() {
            Some(self.image_data.as_slice())
        } else {
            stream_content
        }
    }
}

/// Exports one `Texture2D` to an encoded image.
///
/// `stream_content` must be the exact byte range for `m_StreamData` when the texture has no inline
/// data; it is ignored otherwise, matching `GetImageData`'s precedence. `platform` is the
/// collection platform discriminant (unknown platforms pass 0, which transforms nothing) and
/// `platform_blob_little_endian` is the collection endianness reading the Switch blob shift.
/// `flip_y` mirrors the `texture is not ICubemap` gate: cubemaps skip the vertical flip.
pub fn export_texture2d_image(
    view: &Texture2DView,
    stream_content: Option<&[u8]>,
    platform: i32,
    platform_blob_little_endian: bool,
    flip_y: bool,
    format: ImageExportFormat,
) -> Result<Vec<u8>, TextureExportError> {
    let data = view.resolved_image_data(platform, platform_blob_little_endian, stream_content)?;
    let image = view.to_texture_image(&data);
    let unpack_normal = view.lightmap_format == TEXTURE_USAGE_MODE_NORMALMAP_DXT5NM;
    let bitmap = try_convert_to_bitmap(&image, flip_y, unpack_normal)?;
    let mut output = Vec::new();
    bitmap.save(&mut output, format)?;
    Ok(output)
}

/// Exports one `Texture2D` directly from its decoded payload.
pub fn export_texture2d_value(
    root: &SerializedValue,
    stream_content: Option<&[u8]>,
    platform: i32,
    platform_blob_little_endian: bool,
    flip_y: bool,
    format: ImageExportFormat,
) -> Result<Vec<u8>, TextureExportError> {
    let view = Texture2DView::from_value(root)?;
    export_texture2d_image(
        &view,
        stream_content,
        platform,
        platform_blob_little_endian,
        flip_y,
        format,
    )
}

/// Exports one `Texture3D` to an encoded image.
///
/// `stream_content` follows the same inline-wins contract as `GetImageData`.
/// The converter runs the `GraphicsFormat` route with the derived format, the
/// derived complete size, and the vertical flip; the DXT5nm unpack follows the
/// derived lightmap value.
pub fn export_texture3d_image(
    view: &Texture3DView,
    stream_content: Option<&[u8]>,
    format: ImageExportFormat,
) -> Result<Vec<u8>, TextureExportError> {
    let data =
        resolve_array_image_data(&view.image_data, view.stream_data.is_some(), stream_content)?;
    let image = TextureImage {
        texture_format: 0,
        graphics_format: view.format_value(),
        width: view.width,
        height: view.height,
        depth: view.depth,
        image_size: usize::try_from(view.complete_image_size_value()).unwrap_or(0),
        data: &data,
    };
    let unpack_normal = view.lightmap_value() == i32::from(TEXTURE_USAGE_MODE_NORMALMAP_DXT5NM);
    let bitmap = try_convert_to_bitmap(&image, true, unpack_normal)?;
    let mut output = Vec::new();
    bitmap.save(&mut output, format)?;
    Ok(output)
}

/// Exports one `Texture2DArray` to an encoded image.
///
/// The converter runs the `GraphicsFormat` route with the array depth and the
/// derived complete size, then the vertical flip. No normal unpack applies.
pub fn export_texture2d_array_image(
    view: &Texture2DArrayView,
    stream_content: Option<&[u8]>,
    format: ImageExportFormat,
) -> Result<Vec<u8>, TextureExportError> {
    let data =
        resolve_array_image_data(&view.image_data, view.stream_data.is_some(), stream_content)?;
    let image = TextureImage {
        texture_format: 0,
        graphics_format: view.format,
        width: view.width,
        height: view.height,
        depth: view.depth,
        image_size: usize::try_from(texture2d_array_complete_image_size(
            view.data_size,
            view.depth,
        ))
        .unwrap_or(0),
        data: &data,
    };
    let bitmap = try_convert_to_bitmap(&image, true, false)?;
    let mut output = Vec::new();
    bitmap.save(&mut output, format)?;
    Ok(output)
}

/// Exports one `CubemapArray` to an encoded image.
///
/// Height equals width and depth is six faces per cubemap; the converter runs
/// the `GraphicsFormat` route and then the vertical flip (kept despite the
/// source `Maybe not needed?` comment).
pub fn export_cubemap_array_image(
    view: &CubemapArrayView,
    stream_content: Option<&[u8]>,
    format: ImageExportFormat,
) -> Result<Vec<u8>, TextureExportError> {
    let data =
        resolve_array_image_data(&view.image_data, view.stream_data.is_some(), stream_content)?;
    let depth = cubemap_array_depth(view.cubemap_count)?;
    let image = TextureImage {
        texture_format: 0,
        graphics_format: view.format,
        width: view.width,
        height: view.width,
        depth: usize::try_from(depth).unwrap_or(0),
        image_size: usize::try_from(cubemap_array_complete_image_size(view.data_size, depth))
            .unwrap_or(0),
        data: &data,
    };
    let bitmap = try_convert_to_bitmap(&image, true, false)?;
    let mut output = Vec::new();
    bitmap.save(&mut output, format)?;
    Ok(output)
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

fn optional_bool(
    root: &SerializedValue,
    names: &[&'static str],
) -> Result<Option<bool>, TextureViewError> {
    match lookup(root, names)? {
        None => Ok(None),
        Some((_, SerializedValue::Bool(value))) => Ok(Some(*value)),
        Some((_, SerializedValue::U8(value))) => Ok(Some(*value != 0)),
        Some((_, SerializedValue::I32(value))) => Ok(Some(*value != 0)),
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

        // With layers that do not divide the data, the whole length is kept.
        let mut ragged = root.clone();
        let SerializedValue::Struct(fields) = &mut ragged else {
            unreachable!()
        };
        let image_count = fields
            .iter_mut()
            .find(|f| f.name == "m_ImageCount")
            .unwrap();
        image_count.value = SerializedValue::I32(3);
        let view = Texture2DView::from_value(&ragged).unwrap();
        assert_eq!(view.actual_image_size(), 8);
    }

    #[test]
    fn mip_flag_and_count_drive_has_mips() {
        // `Texture2DExtensions.Mips`: `MipMap_C28 || MipCount_C28 > 1`. The
        // shared fixture carries neither, so it reports no mipmaps.
        let plain = Texture2DView::from_value(&rgba32_texture()).unwrap();
        assert!(!plain.mip_map);
        assert!(!plain.has_mips());

        // (mip_map field, mip_count field, expected).
        let rows = [
            (SerializedValue::Bool(true), 1, true),
            (SerializedValue::Bool(true), 0, true),
            (SerializedValue::Bool(false), 2, true),
            (SerializedValue::Bool(false), 1, false),
            (SerializedValue::U8(1), 1, true),
            (SerializedValue::I32(0), 2, true),
        ];
        for (flag, count, expected) in rows {
            let mut root = rgba32_texture();
            let SerializedValue::Struct(fields) = &mut root else {
                unreachable!()
            };
            fields.push(field("m_MipMap", flag));
            fields.push(field("m_MipCount", SerializedValue::I32(count)));
            assert_eq!(
                Texture2DView::from_value(&root).unwrap().has_mips(),
                expected,
                "mip count {count}"
            );
        }
    }

    #[test]
    fn mip_flag_with_a_wrong_kind_is_rejected() {
        let mut root = rgba32_texture();
        let SerializedValue::Struct(fields) = &mut root else {
            unreachable!()
        };
        fields.push(field(
            "m_MipMap",
            SerializedValue::StringBytes(b"yes".to_vec()),
        ));
        assert!(matches!(
            Texture2DView::from_value(&root),
            Err(TextureViewError::WrongType {
                field: "m_MipMap",
                ..
            })
        ));
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
        let output = export_texture2d_value(
            &rgba32_texture(),
            None,
            0,
            true,
            true,
            ImageExportFormat::Png,
        )
        .unwrap();
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
            export_texture2d_value(&root, None, 0, true, true, ImageExportFormat::Png),
            Err(TextureExportError::ImageDataUnavailable)
        ));
        // Providing the exact streamed content succeeds.
        let content = [1u8, 2, 3, 4, 5, 6, 7, 8];
        assert!(export_texture2d_value(
            &root,
            Some(&content),
            0,
            true,
            true,
            ImageExportFormat::Png
        )
        .is_ok());
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
            mip_map: false,
            lightmap_format: TEXTURE_USAGE_MODE_NORMALMAP_DXT5NM,
            image_data: vec![128, 128, 0, 255],
            stream_data: None,
            platform_blob: Vec::new(),
        };
        let output =
            export_texture2d_image(&view, None, 0, true, true, ImageExportFormat::Png).unwrap();
        assert!(output.len() > 8);
    }

    /// The `texture is not ICubemap` gate: a 2x1 texture flips its rows, while
    /// the same bytes as a cubemap keep file order (asserted on BMP tails,
    /// which need no decoder: the 122-byte header precedes bottom-up BGRA).
    #[test]
    fn cubemaps_skip_the_vertical_flip() {
        // Two 1-wide rows: top [1,2,3,4], bottom [5,6,7,8].
        let view = Texture2DView {
            width: 1,
            height: 2,
            texture_format: 4,
            complete_image_size: 8,
            image_count: 1,
            mip_count: 0,
            mip_map: false,
            lightmap_format: 0,
            image_data: vec![1, 2, 3, 4, 5, 6, 7, 8],
            stream_data: None,
            platform_blob: Vec::new(),
        };
        let flipped =
            export_texture2d_image(&view, None, 0, true, true, ImageExportFormat::Bmp).unwrap();
        let unflipped =
            export_texture2d_image(&view, None, 0, true, false, ImageExportFormat::Bmp).unwrap();
        assert_ne!(flipped, unflipped);
        // BMP stores rows bottom-up as BGRA: the flipped bitmap ends with the
        // top row, the cubemap bitmap with the bottom row.
        assert_eq!(&flipped[122..], &[3, 2, 1, 4, 7, 6, 5, 8]);
        assert_eq!(&unflipped[122..], &[7, 6, 5, 8, 3, 2, 1, 4]);
    }

    /// The `GraphicsFormat`-routed exports produce images from 1x1(x1) RGBA
    /// pixels (discriminant 4 decodes RGBA32).
    #[test]
    fn array_exports_produce_images() {
        let pixels = vec![10, 20, 30, 255];
        let view3d = crate::resources::texture_array::Texture3DView {
            width: 1,
            height: 1,
            depth: 1,
            format_i32: None,
            format_u32: Some(4),
            image_data: pixels.clone(),
            stream_data: None,
            lightmap_format: None,
            usage_mode: 0,
            mip_count: None,
            mip_map: false,
            complete_image_size_u32: Some(4),
            complete_image_size_i32: None,
        };
        let png = export_texture3d_image(&view3d, None, ImageExportFormat::Png).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

        let view_array = crate::resources::texture_array::Texture2DArrayView {
            width: 1,
            height: 1,
            depth: 1,
            format: 4,
            data_size: 4,
            image_data: pixels.clone(),
            stream_data: None,
        };
        let png = export_texture2d_array_image(&view_array, None, ImageExportFormat::Png).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

        let view_cube = crate::resources::texture_array::CubemapArrayView {
            width: 1,
            cubemap_count: 1,
            format: 4,
            data_size: 24,
            // Six faces of one RGBA pixel each.
            image_data: vec![
                10, 20, 30, 255, 11, 21, 31, 255, 12, 22, 32, 255, //
                13, 23, 33, 255, 14, 24, 34, 255, 15, 25, 35, 255,
            ],
            stream_data: None,
        };
        let png = export_cubemap_array_image(&view_cube, None, ImageExportFormat::Png).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    /// The generated cubemap surface carries every `_C28` field the view reads,
    /// on both the oldest (`Cubemap_5`) and newest (`Cubemap_6000_1_0_a9`)
    /// audited classes, and implements `ICubemap` over `ITexture2D` (probe
    /// `Temp/opencode/cubeprobe`).
    #[test]
    fn cubemap_generated_surface_covers_the_view_fields() {
        assert_eq!(CUBEMAP_CLASS_ID, 89);
        let fixture = include_str!("cubemap_generated_api_fixture.txt");
        let mut classes: Vec<&str> = Vec::new();
        let mut sections: Vec<(String, Vec<String>)> = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix("class=") {
                classes.push(name);
            } else if let Some(header) = line.strip_prefix("### ") {
                let header = header.strip_suffix(" properties:").unwrap_or(header);
                sections.push((header.to_owned(), Vec::new()));
            } else if let Some(property) = line.strip_prefix('P') {
                if let Some((_, props)) = sections.last_mut() {
                    props.push(property.trim().to_owned());
                }
            }
        }
        assert!(classes.len() >= 27, "all versioned cubemap classes listed");
        for wanted in [
            "AssetRipper.SourceGenerated.Classes.ClassID_89.Cubemap_5",
            "AssetRipper.SourceGenerated.Classes.ClassID_89.Cubemap_6000_1_0_a9",
        ] {
            let (_, props) = sections
                .iter()
                .find(|(header, _)| header.ends_with(wanted))
                .unwrap_or_else(|| panic!("{wanted} audited"));
            for field in [
                "Format_C28",
                "Width_C28",
                "Height_C28",
                "ImageCount_C28",
                "ImageData_C28",
                "StreamData_C28",
                "PlatformBlob_C28",
                "MipCount_C28",
                "MipMap_C28",
                "LightmapFormat_C28",
                "CompleteImageSize_C28_Int32",
            ] {
                assert!(props.iter().any(|p| p.ends_with(field)), "{wanted} {field}");
            }
        }
        let ifaces = fixture
            .lines()
            .find_map(|line| line.strip_prefix("ifaces="))
            .unwrap_or("");
        for marker in ["ICubemap", "ITexture2D"] {
            assert!(ifaces.split(',').any(|name| name == marker), "{marker}");
        }
    }

    #[test]
    fn ambiguous_aliases_are_rejected() {
        let mut root = rgba32_texture();
        let SerializedValue::Struct(fields) = &mut root else {
            unreachable!()
        };
        fields.push(field("m_DataSize", SerializedValue::I32(8)));
        assert!(matches!(
            Texture2DView::from_value(&root),
            Err(TextureViewError::WrongType {
                field: "m_DataSize",
                ..
            })
        ));
    }

    fn image_texture(fields: Vec<SerializedFieldValue>) -> SerializedValue {
        let mut root = vec![
            field("m_Width", SerializedValue::I32(2)),
            field("m_Height", SerializedValue::I32(1)),
        ];
        root.extend(fields);
        SerializedValue::Struct(root)
    }

    fn stream_info() -> SerializedValue {
        SerializedValue::Struct(vec![
            field("path", SerializedValue::StringBytes(b"tex.resS".to_vec())),
            field("offset", SerializedValue::U32(0)),
            field("size", SerializedValue::U32(4)),
        ])
    }

    #[test]
    fn image_texture_view_reads_base_fields() {
        assert_eq!(IMAGE_TEXTURE_CLASS_ID, 189);
        let root = image_texture(vec![
            field("m_ImageData_C189", SerializedValue::Bytes(vec![9, 9])),
            field("m_StreamData_C189", stream_info()),
        ]);
        let view = ImageTextureView::from_value(&root).unwrap();
        assert_eq!((view.width, view.height), (2, 1));
        assert_eq!(view.image_data, vec![9, 9]);
        assert!(view.stream_data.is_some());
        // The legacy `image data` alias reads too.
        let legacy = image_texture(vec![field("image data", SerializedValue::Bytes(vec![7]))]);
        assert_eq!(
            ImageTextureView::from_value(&legacy).unwrap().image_data,
            vec![7]
        );
        // Width is required.
        let missing = SerializedValue::Struct(vec![field("m_Height", SerializedValue::I32(1))]);
        assert!(ImageTextureView::from_value(&missing).is_err());
    }

    #[test]
    fn image_texture_bytes_prefer_inline_data() {
        // Inline bytes win even when streamed content is available.
        let root = image_texture(vec![
            field("m_ImageData", SerializedValue::Bytes(vec![1, 2])),
            field("m_StreamData", stream_info()),
        ]);
        let view = ImageTextureView::from_value(&root).unwrap();
        assert_eq!(view.image_bytes(Some(&[3, 4])), Some([1, 2].as_slice()));
        // Empty inline bytes fall through to the streamed content.
        let root = image_texture(vec![field("m_StreamData", stream_info())]);
        let view = ImageTextureView::from_value(&root).unwrap();
        assert_eq!(view.image_bytes(Some(&[3, 4])), Some([3, 4].as_slice()));
        assert_eq!(view.image_bytes(None), None);
    }

    fn platform_view(
        format: i32,
        width: usize,
        height: usize,
        data: Vec<u8>,
        blob: Vec<u8>,
    ) -> Texture2DView {
        Texture2DView {
            width,
            height,
            texture_format: format,
            complete_image_size: 0,
            image_count: 1,
            mip_count: 0,
            mip_map: false,
            lightmap_format: 0,
            image_data: data,
            stream_data: None,
            platform_blob: blob,
        }
    }

    /// The live-runtime `GetImageData` differential committed in
    /// `texture_platform_fixture.txt` (probe `Temp/opencode/texplatprobe`):
    /// every row must reproduce byte-for-byte, including the odd-count throw.
    #[test]
    fn platform_pipeline_matches_the_live_runtime() {
        let fixture = include_str!("texture_platform_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            let (kind, tag) = (parts[0], parts[1]);
            let platform: i32 = parts[2].parse().unwrap();
            let format: i32 = parts[3].parse().unwrap();
            let width: usize = parts[4].parse().unwrap();
            let height: usize = parts[5].parse().unwrap();
            let input = parse_hex_bytes(parts[6]);
            let blob = parse_hex_bytes(parts[7]);
            let view = platform_view(format, width, height, input, blob);
            if kind == "ERR" {
                assert_eq!(parts[8], "IndexOutOfRangeException", "{tag}");
                assert!(
                    matches!(
                        view.resolved_image_data(platform, true, None),
                        Err(TextureExportError::Swap(
                            crate::resources::texture_format::SwapBytesError::OddByteCount { .. }
                        ))
                    ),
                    "{tag}"
                );
            } else {
                let expected = parse_hex_bytes(parts[8]);
                assert_eq!(
                    view.resolved_image_data(platform, true, None)
                        .unwrap()
                        .into_owned(),
                    expected,
                    "{tag}"
                );
            }
            rows += 1;
        }
        assert_eq!(rows, 12, "all fixture rows are asserted");
    }

    #[test]
    fn swap_never_mutates_the_view() {
        let view = platform_view(
            10, /* DXT1 */
            4,
            4,
            vec![1, 2, 3, 4, 5, 6, 7, 8],
            vec![],
        );
        let resolved = view.resolved_image_data(11, true, None).unwrap();
        assert_eq!(resolved.as_ref(), &[2, 1, 4, 3, 6, 5, 8, 7]);
        assert_eq!(view.image_data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn passthrough_borrows_while_transforms_own() {
        // No transform: the bytes borrow the view, like the source alias.
        let view = platform_view(4, 2, 1, vec![1, 2, 3, 4], vec![]);
        assert!(matches!(
            view.resolved_image_data(5, true, None).unwrap(),
            std::borrow::Cow::Borrowed(_)
        ));
        // A swap owns its buffer, like the source copy.
        let view = platform_view(10, 4, 4, vec![1, 2, 3, 4], vec![]);
        assert!(matches!(
            view.resolved_image_data(11, true, None).unwrap(),
            std::borrow::Cow::Owned(_)
        ));
    }

    #[test]
    fn streamed_bytes_take_the_swap_path() {
        // Empty inline bytes resolve the streamed content, which the Xbox360
        // swap then transforms in place (the source swaps its fresh buffer).
        let mut view = platform_view(10 /* DXT1 */, 4, 4, vec![], vec![]);
        view.stream_data = Some(crate::resource::StreamedResourceInfo {
            path: "tex.resS".to_owned(),
            offset: 0,
            size: 4,
        });
        let stream = [1u8, 2, 3, 4];
        let resolved = view.resolved_image_data(11, true, Some(&stream)).unwrap();
        assert_eq!(resolved.as_ref(), &[2, 1, 4, 3]);
        assert_eq!(stream, [1, 2, 3, 4]);
    }

    #[test]
    fn blob_field_reads_the_serialized_spelling() {
        // `m_PlatformBlob` (single UTF-16 literal in the shipped generated DLL;
        // version-gated in the source: present on 2020.2+, absent before).
        let root = SerializedValue::Struct(vec![
            field("m_Width", SerializedValue::I32(2)),
            field("m_Height", SerializedValue::I32(1)),
            field("m_PlatformBlob", SerializedValue::Bytes(vec![0; 12])),
        ]);
        assert_eq!(
            Texture2DView::from_value(&root).unwrap().platform_blob,
            vec![0; 12]
        );
        // Payloads without the field behave like an empty blob.
        let root = SerializedValue::Struct(vec![
            field("m_Width", SerializedValue::I32(2)),
            field("m_Height", SerializedValue::I32(1)),
        ]);
        assert!(Texture2DView::from_value(&root)
            .unwrap()
            .platform_blob
            .is_empty());
    }

    fn parse_hex_bytes(text: &str) -> Vec<u8> {
        assert!(text.len().is_multiple_of(2), "even hex length");
        let mut bytes = Vec::with_capacity(text.len() / 2);
        for pair in text.as_bytes().chunks_exact(2) {
            bytes.push(hex_nibble(pair[0]) << 4 | hex_nibble(pair[1]));
        }
        bytes
    }

    fn hex_nibble(digit: u8) -> u8 {
        match digit {
            b'0'..=b'9' => digit - b'0',
            b'A'..=b'F' => digit - b'A' + 10,
            b'a'..=b'f' => digit - b'a' + 10,
            _ => panic!("non-hex digit {digit}"),
        }
    }
}
