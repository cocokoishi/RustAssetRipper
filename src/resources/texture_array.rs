//! `Texture3D` (class 117), `Texture2DArray` (class 187), and `CubemapArray`
//! (class 188) field views and size/selection rules.
//!
//! Ports the field reads behind `TextureConverter.TryConvertToBitmap` for the three
//! `GraphicsFormat`-routed overloads, plus the `*Extensions` helpers:
//! `Texture2DArrayExtensions`/`Texture3DExtensions`/`CubemapArrayExtensions`
//! `GetImageData` (inline-wins over streamed, no swap or unswizzle on these classes),
//! the three `GetCompleteImageSize` variants, `CubemapArray.GetHeight`/`GetDepth`
//! (height is the width; depth is six faces per cubemap, reproducing the source
//! `CubemapCount * 6` including its `Not sure about this` comment), and the
//! `Texture3D` format/lightmap/mip gates (`Format_Int32`-vs-`Format_UInt32`,
//! `Has_LightmapFormat`-vs-`UsageMode`, `MipCount`-vs-`MipMap`, `Int32`-vs-`UInt32`
//! complete size, `GraphicsFormat`-to-`TextureFormat` with an `RGBA32` fallback).
//!
//! Open: the bitmap export wiring, the CLI classes, and per-version field gates
//! (probed `Has_*` outcomes: `Format_Int32`, `MipCount`, and `CompleteImageSize_UInt32`
//! hold on 5.6/2019.4/2022.3; `LightmapFormat` does not, so the usage mode backs it).

use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Display};

use crate::resource::{
    extract_streamed_resource_info, StreamedResourceInfo, StreamedResourceInfoError,
};
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};

use super::graphics_format::to_texture_format;

/// The class ID of `Texture3D`.
pub const TEXTURE3D_CLASS_ID: i32 = 117;
/// The class ID of `Texture2DArray`.
pub const TEXTURE2DARRAY_CLASS_ID: i32 = 187;
/// The class ID of `CubemapArray`.
pub const CUBEMAPARRAY_CLASS_ID: i32 = 188;

/// `TextureFormat.RGBA32`, the `GetTextureFormat` fallback.
pub const RGBA32_TEXTURE_FORMAT: i32 = 4;

/// A failure while reading an array/3D texture field view or deriving its rules.
#[derive(Debug)]
pub enum TextureArrayError {
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
    /// A declared stream the caller could not resolve.
    StreamUnavailable,
    /// An integer product does not fit its target.
    ArithmeticOverflow {
        /// The computation that overflowed.
        what: &'static str,
    },
    /// The mip fallback saw no positive side to take a logarithm of. The source
    /// `Convert.ToInt32` throws on the resulting infinity; this is explicit here.
    NonPositiveMipBase {
        /// The decoded width.
        width: usize,
        /// The decoded height.
        height: usize,
    },
}

impl Display for TextureArrayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field, source } => {
                write!(
                    formatter,
                    "could not read texture field '{field}': {source}"
                )
            }
            Self::MissingField { field } => {
                write!(formatter, "texture is missing required field '{field}'")
            }
            Self::WrongType { field, actual } => {
                write!(
                    formatter,
                    "texture field '{field}' has kind {actual}, expected a number"
                )
            }
            Self::StreamInfo { field, source } => {
                write!(
                    formatter,
                    "could not decode StreamingInfo field '{field}': {source}"
                )
            }
            Self::StreamUnavailable => {
                formatter.write_str("texture has neither inline image data nor a resolvable stream")
            }
            Self::ArithmeticOverflow { what } => {
                write!(formatter, "texture arithmetic overflowed computing {what}")
            }
            Self::NonPositiveMipBase { width, height } => write!(
                formatter,
                "cannot derive a mip count from non-positive dimensions {width}x{height}"
            ),
        }
    }
}

impl Error for TextureArrayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FieldLookup { source, .. } => Some(source),
            Self::StreamInfo { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The fields the array/3D converters read from a class-117 `Texture3D`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Texture3DView {
    /// `m_Width`.
    pub width: usize,
    /// `m_Height`.
    pub height: usize,
    /// `m_Depth`.
    pub depth: usize,
    /// `m_Format` when stored as `SInt32`.
    pub format_i32: Option<i32>,
    /// `m_Format` when stored as `UInt32`.
    pub format_u32: Option<u32>,
    /// Inline `m_ImageData` bytes.
    pub image_data: Vec<u8>,
    /// `m_StreamData`, if present.
    pub stream_data: Option<StreamedResourceInfo>,
    /// `m_LightmapFormat`, if present.
    pub lightmap_format: Option<i32>,
    /// `m_UsageMode`; backs the lightmap format when that field is absent.
    pub usage_mode: i32,
    /// `m_MipCount`, if present.
    pub mip_count: Option<i32>,
    /// `m_MipMap`; backs the mip count when that field is absent.
    pub mip_map: bool,
    /// `m_CompleteImageSize` when stored as `UInt32`.
    pub complete_image_size_u32: Option<u32>,
    /// `m_CompleteImageSize` when stored as `SInt32`.
    pub complete_image_size_i32: Option<i32>,
}

/// The fields the converter reads from a class-187 `Texture2DArray`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Texture2DArrayView {
    /// `m_Width`.
    pub width: usize,
    /// `m_Height`.
    pub height: usize,
    /// `m_Depth`.
    pub depth: usize,
    /// `m_Format`, a `GraphicsFormat` discriminant.
    pub format: i32,
    /// `m_DataSize`.
    pub data_size: u32,
    /// Inline `m_ImageData` bytes.
    pub image_data: Vec<u8>,
    /// `m_StreamData`, if present.
    pub stream_data: Option<StreamedResourceInfo>,
}

/// The fields the converter reads from a class-188 `CubemapArray`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CubemapArrayView {
    /// `m_Width`; the height equals the width.
    pub width: usize,
    /// `m_CubemapCount`.
    pub cubemap_count: usize,
    /// `m_Format`, a `GraphicsFormat` discriminant.
    pub format: i32,
    /// `m_DataSize`.
    pub data_size: u32,
    /// Inline `m_ImageData` bytes.
    pub image_data: Vec<u8>,
    /// `m_StreamData`, if present.
    pub stream_data: Option<StreamedResourceInfo>,
}

impl Texture3DView {
    /// Reads the export-relevant fields from a decoded `Texture3D` payload.
    pub fn from_value(root: &SerializedValue) -> Result<Self, TextureArrayError> {
        let (format_i32, format_u32) = optional_i32_or_u32(root, "m_Format")?;
        let (complete_u32, complete_i32) = optional_u32_or_i32(root, "m_CompleteImageSize")?;
        Ok(Self {
            width: nonnegative(required_i32(root, "m_Width")?, "m_Width")?,
            height: nonnegative(required_i32(root, "m_Height")?, "m_Height")?,
            depth: nonnegative(required_i32(root, "m_Depth")?, "m_Depth")?,
            format_i32,
            format_u32,
            image_data: optional_bytes(root, "m_ImageData")?.unwrap_or_default(),
            stream_data: optional_stream(root, "m_StreamData")?,
            lightmap_format: optional_i32(root, "m_LightmapFormat")?,
            usage_mode: optional_i32(root, "m_UsageMode")?.unwrap_or(0),
            mip_count: optional_i32(root, "m_MipCount")?,
            mip_map: optional_bool(root, "m_MipMap")?.unwrap_or(false),
            complete_image_size_u32: complete_u32,
            complete_image_size_i32: complete_i32,
        })
    }

    /// `Texture3DExtensions.Format`: the `SInt32` spelling wins when present,
    /// else the `UInt32` spelling casts unchecked.
    pub fn format_value(&self) -> i32 {
        match (self.format_i32, self.format_u32) {
            (Some(value), _) => value,
            (None, Some(value)) => value as i32,
            (None, None) => 0,
        }
    }

    /// `Texture3DExtensions.GetTextureFormat`: the `GraphicsFormat` conversion
    /// with an `RGBA32` fallback for unconvertible values.
    pub fn texture_format_or_default(&self) -> i32 {
        to_texture_format(self.format_value()).unwrap_or(RGBA32_TEXTURE_FORMAT)
    }

    /// `Texture3DExtensions.GetLightmapFormat`: the lightmap field wins when
    /// present, else the usage mode backs it.
    pub fn lightmap_value(&self) -> i32 {
        self.lightmap_format.unwrap_or(self.usage_mode)
    }

    /// `Texture3DExtensions.GetCompleteImageSize`: the `UInt32` spelling wins
    /// when present, else the `SInt32` spelling.
    pub fn complete_image_size_value(&self) -> i32 {
        match (self.complete_image_size_u32, self.complete_image_size_i32) {
            (Some(value), _) => value as i32,
            (None, Some(value)) => value,
            (None, None) => 0,
        }
    }

    /// `Texture3DExtensions.GetMipCount`: the count field wins when present,
    /// else a mipmapped texture derives `round(log2(maxSide))` and a flat one
    /// reports a single mip.
    pub fn mip_count_value(&self) -> Result<i32, TextureArrayError> {
        if let Some(count) = self.mip_count {
            return Ok(count);
        }
        if !self.mip_map {
            return Ok(1);
        }
        let longest = self.width.max(self.height);
        if longest == 0 {
            return Err(TextureArrayError::NonPositiveMipBase {
                width: self.width,
                height: self.height,
            });
        }
        // `Convert.ToInt32` rounds to nearest (banker's); no integer logarithm
        // lands exactly on a half, so ties-even matches on every real input.
        Ok((longest as f64).log2().round_ties_even() as i32)
    }
}

impl Texture2DArrayView {
    /// Reads the export-relevant fields from a decoded `Texture2DArray` payload.
    pub fn from_value(root: &SerializedValue) -> Result<Self, TextureArrayError> {
        Ok(Self {
            width: nonnegative(required_i32(root, "m_Width")?, "m_Width")?,
            height: nonnegative(required_i32(root, "m_Height")?, "m_Height")?,
            depth: nonnegative(required_i32(root, "m_Depth")?, "m_Depth")?,
            format: optional_i32(root, "m_Format")?.unwrap_or(0),
            data_size: optional_u32(root, "m_DataSize")?.unwrap_or(0),
            image_data: optional_bytes(root, "m_ImageData")?.unwrap_or_default(),
            stream_data: optional_stream(root, "m_StreamData")?,
        })
    }
}

impl CubemapArrayView {
    /// Reads the export-relevant fields from a decoded `CubemapArray` payload.
    /// No height is read: `GetHeight` returns the width on every version.
    pub fn from_value(root: &SerializedValue) -> Result<Self, TextureArrayError> {
        Ok(Self {
            width: nonnegative(required_i32(root, "m_Width")?, "m_Width")?,
            cubemap_count: nonnegative(required_i32(root, "m_CubemapCount")?, "m_CubemapCount")?,
            format: optional_i32(root, "m_Format")?.unwrap_or(0),
            data_size: optional_u32(root, "m_DataSize")?.unwrap_or(0),
            image_data: optional_bytes(root, "m_ImageData")?.unwrap_or_default(),
            stream_data: optional_stream(root, "m_StreamData")?,
        })
    }
}

/// `Texture2DArrayExtensions.GetCompleteImageSize`: the data size split across
/// slices, with depths of one or zero keeping the whole size.
pub fn texture2d_array_complete_image_size(data_size: u32, depth: usize) -> i32 {
    let divisor = match i32::try_from(depth) {
        Ok(depth) if depth > 1 => depth,
        _ => 1,
    };
    (data_size as i32).wrapping_div(divisor)
}

/// `CubemapArrayExtensions.GetHeight`: the height equals the width.
pub fn cubemap_array_height(width: usize) -> usize {
    width
}

/// `CubemapArrayExtensions.GetDepth`: six faces per cubemap, reproducing the
/// source arithmetic exactly (including its `Not sure about this` comment).
pub fn cubemap_array_depth(cubemap_count: usize) -> Result<i32, TextureArrayError> {
    let count =
        i32::try_from(cubemap_count).map_err(|_| TextureArrayError::ArithmeticOverflow {
            what: "cubemap face count",
        })?;
    count
        .checked_mul(6)
        .ok_or(TextureArrayError::ArithmeticOverflow {
            what: "cubemap face count",
        })
}

/// `CubemapArrayExtensions.GetCompleteImageSize`: the data size split across
/// faces, or zero when there are no faces.
pub fn cubemap_array_complete_image_size(data_size: u32, depth: i32) -> i32 {
    if depth > 0 {
        (data_size as i32).wrapping_div(depth)
    } else {
        0
    }
}

/// The shared `GetImageData` selection of all three classes: inline bytes win
/// when non-empty, otherwise the caller-resolved streamed content, otherwise an
/// empty array when no stream field exists. A declared-but-unresolved stream is
/// an explicit error, mirroring the class-28 export contract.
pub fn resolve_array_image_data<'a>(
    image_data: &'a [u8],
    has_stream_data: bool,
    stream_content: Option<&'a [u8]>,
) -> Result<Cow<'a, [u8]>, TextureArrayError> {
    if !image_data.is_empty() {
        return Ok(Cow::Borrowed(image_data));
    }
    match stream_content {
        Some(content) => Ok(Cow::Borrowed(content)),
        None if !has_stream_data => Ok(Cow::Borrowed(&[])),
        None => Err(TextureArrayError::StreamUnavailable),
    }
}

fn nonnegative(value: i32, field: &'static str) -> Result<usize, TextureArrayError> {
    usize::try_from(value).map_err(|_| TextureArrayError::WrongType {
        field,
        actual: "a negative integer",
    })
}

fn serialized_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "boolean",
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
        SerializedValue::ManagedReferenceId { .. } => "managed reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
        SerializedValue::Raw { .. } => "raw data",
    }
}

fn lookup<'a>(
    root: &'a SerializedValue,
    name: &'static str,
) -> Result<Option<&'a SerializedValue>, TextureArrayError> {
    root.unique_field_in_base_chain(name)
        .map_err(|source| TextureArrayError::FieldLookup {
            field: name,
            source,
        })
}

fn required_i32(root: &SerializedValue, name: &'static str) -> Result<i32, TextureArrayError> {
    match lookup(root, name)? {
        None => Err(TextureArrayError::MissingField { field: name }),
        Some(SerializedValue::I32(value)) => Ok(*value),
        Some(value) => Err(TextureArrayError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_i32(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<i32>, TextureArrayError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::I32(value)) => Ok(Some(*value)),
        Some(SerializedValue::U32(value)) => Ok(Some(*value as i32)),
        Some(value) => Err(TextureArrayError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_u32(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<u32>, TextureArrayError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::U32(value)) => Ok(Some(*value)),
        Some(SerializedValue::I32(value)) => Ok(Some(*value as u32)),
        Some(value) => Err(TextureArrayError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

/// Reads one field name that carries either 32-bit spelling (the `m_Format` and
/// `m_CompleteImageSize` version unions).
fn optional_i32_or_u32(
    root: &SerializedValue,
    name: &'static str,
) -> Result<(Option<i32>, Option<u32>), TextureArrayError> {
    match lookup(root, name)? {
        None => Ok((None, None)),
        Some(SerializedValue::I32(value)) => Ok((Some(*value), None)),
        Some(SerializedValue::U32(value)) => Ok((None, Some(*value))),
        Some(value) => Err(TextureArrayError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

/// Reads one field name that carries either 32-bit spelling, `UInt32` first.
fn optional_u32_or_i32(
    root: &SerializedValue,
    name: &'static str,
) -> Result<(Option<u32>, Option<i32>), TextureArrayError> {
    match lookup(root, name)? {
        None => Ok((None, None)),
        Some(SerializedValue::U32(value)) => Ok((Some(*value), None)),
        Some(SerializedValue::I32(value)) => Ok((None, Some(*value))),
        Some(value) => Err(TextureArrayError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_bool(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<bool>, TextureArrayError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::Bool(value)) => Ok(Some(*value)),
        Some(SerializedValue::U8(value)) => Ok(Some(*value != 0)),
        Some(SerializedValue::I32(value)) => Ok(Some(*value != 0)),
        Some(value) => Err(TextureArrayError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_bytes(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<Vec<u8>>, TextureArrayError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::Bytes(bytes)) => Ok(Some(bytes.clone())),
        Some(SerializedValue::Array(values)) => {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(values.len())
                .map_err(|_| TextureArrayError::WrongType {
                    field: name,
                    actual: "an unallocatable byte array",
                })?;
            for value in values {
                match value {
                    SerializedValue::U8(byte) => bytes.push(*byte),
                    other => {
                        return Err(TextureArrayError::WrongType {
                            field: name,
                            actual: serialized_kind(other),
                        })
                    }
                }
            }
            Ok(Some(bytes))
        }
        Some(value) => Err(TextureArrayError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_stream(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<StreamedResourceInfo>, TextureArrayError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(value) => {
            let info = extract_streamed_resource_info(value).map_err(|source| {
                TextureArrayError::StreamInfo {
                    field: name,
                    source,
                }
            })?;
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

    fn parse_hex(text: &str) -> Vec<u8> {
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

    fn array_view(
        format: i32,
        width: usize,
        height: usize,
        depth: usize,
        data: Vec<u8>,
    ) -> Texture2DArrayView {
        Texture2DArrayView {
            width,
            height,
            depth,
            format,
            data_size: 0,
            image_data: data,
            stream_data: None,
        }
    }

    /// The live-asset differential committed in `texture_array_fixture.txt`
    /// (probe `Temp/opencode/arrayprobe`, 2022.3 assets, SDK 10.0.401).
    #[test]
    fn selection_and_sizes_match_the_live_runtime() {
        let fixture = include_str!("texture_array_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            let (tag, class) = (parts[1], parts[2]);
            let format: i32 = parts[3].parse().unwrap();
            let width: usize = parts[4].parse().unwrap();
            let height: usize = parts[5].parse().unwrap();
            let third: usize = parts[6].parse().unwrap();
            let cubes: usize = parts[7].parse().unwrap();
            let size_field: u32 = parts[8].parse().unwrap();
            let input = parse_hex(parts[9]);
            let expected = parse_hex(parts[10]);
            let extra = &parts[11..];
            match class {
                "187" => {
                    let mut view = array_view(format, width, height, third, input);
                    view.data_size = size_field;
                    assert_eq!(
                        resolve_array_image_data(
                            &view.image_data,
                            view.stream_data.is_some(),
                            None
                        )
                        .unwrap()
                        .into_owned(),
                        expected,
                        "{tag} selection"
                    );
                    let size = extra
                        .iter()
                        .find_map(|cell| cell.strip_prefix("size="))
                        .unwrap()
                        .parse::<i32>()
                        .unwrap();
                    assert_eq!(
                        texture2d_array_complete_image_size(view.data_size, view.depth),
                        size,
                        "{tag} size"
                    );
                }
                "188" => {
                    let view = CubemapArrayView {
                        width,
                        cubemap_count: cubes,
                        format,
                        data_size: size_field,
                        image_data: input,
                        stream_data: None,
                    };
                    assert_eq!(
                        resolve_array_image_data(&view.image_data, false, None)
                            .unwrap()
                            .into_owned(),
                        expected,
                        "{tag} selection"
                    );
                    let height_text = extra
                        .iter()
                        .find_map(|cell| cell.strip_prefix("h="))
                        .map(|value| value.parse::<usize>().unwrap());
                    if let Some(height) = height_text {
                        assert_eq!(cubemap_array_height(view.width), height, "{tag} height");
                    }
                    let depth = extra
                        .iter()
                        .find_map(|cell| cell.strip_prefix("d="))
                        .unwrap()
                        .parse::<i32>()
                        .unwrap();
                    assert_eq!(
                        cubemap_array_depth(view.cubemap_count).unwrap(),
                        depth,
                        "{tag} depth"
                    );
                    let size = extra
                        .iter()
                        .find_map(|cell| cell.strip_prefix("size="))
                        .unwrap()
                        .parse::<i32>()
                        .unwrap();
                    assert_eq!(
                        cubemap_array_complete_image_size(view.data_size, depth),
                        size,
                        "{tag} size"
                    );
                }
                "117" => {
                    let view = Texture3DView {
                        width,
                        height,
                        depth: third,
                        format_i32: None,
                        format_u32: Some(format as u32),
                        image_data: input,
                        stream_data: None,
                        lightmap_format: None,
                        usage_mode: 1,
                        mip_count: Some(5),
                        mip_map: false,
                        complete_image_size_u32: Some(size_field),
                        complete_image_size_i32: None,
                    };
                    assert_eq!(
                        resolve_array_image_data(&view.image_data, false, None)
                            .unwrap()
                            .into_owned(),
                        expected,
                        "{tag} selection"
                    );
                    assert_eq!(view.format_value(), format, "{tag} format");
                    // The probe read `RGBA32` back for `GraphicsFormat` 4.
                    assert_eq!(view.texture_format_or_default(), 4, "{tag} texture format");
                    // The probe read `BakedLightmapDoubleLDR` for usage mode 1:
                    // the lightmap field is gated off, so the mode backs it.
                    assert_eq!(view.lightmap_value(), 1, "{tag} lightmap");
                    assert_eq!(view.mip_count_value().unwrap(), 5, "{tag} mips");
                    assert_eq!(
                        view.complete_image_size_value(),
                        size_field as i32,
                        "{tag} size"
                    );
                }
                other => panic!("unknown class {other}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 6, "all fixture rows are asserted");
    }

    #[test]
    fn format_union_prefers_the_signed_spelling() {
        let root = SerializedValue::Struct(vec![
            field("m_Width", SerializedValue::I32(2)),
            field("m_Height", SerializedValue::I32(2)),
            field("m_Depth", SerializedValue::I32(2)),
            field("m_Format", SerializedValue::U32(4)),
        ]);
        let view = Texture3DView::from_value(&root).unwrap();
        assert_eq!((view.format_i32, view.format_u32), (None, Some(4)));
        assert_eq!(view.format_value(), 4);
        // The probe proved both spellings share one field: an `SInt32` read
        // wins as well.
        let root = SerializedValue::Struct(vec![
            field("m_Width", SerializedValue::I32(2)),
            field("m_Height", SerializedValue::I32(2)),
            field("m_Depth", SerializedValue::I32(2)),
            field("m_Format", SerializedValue::I32(7)),
        ]);
        let view = Texture3DView::from_value(&root).unwrap();
        assert_eq!(view.format_value(), 7);
        // Unconvertible graphics formats fall back to `RGBA32`, like the
        // source `try/catch`.
        assert!(to_texture_format(999).is_err());
        let view = Texture3DView {
            format_i32: Some(999),
            ..Texture3DView::from_value(&root).unwrap()
        };
        assert_eq!(view.texture_format_or_default(), 4);
    }

    #[test]
    fn complete_size_union_prefers_the_unsigned_spelling() {
        let root = SerializedValue::Struct(vec![
            field("m_Width", SerializedValue::I32(1)),
            field("m_Height", SerializedValue::I32(1)),
            field("m_Depth", SerializedValue::I32(1)),
            field("m_CompleteImageSize", SerializedValue::U32(64)),
        ]);
        let view = Texture3DView::from_value(&root).unwrap();
        assert_eq!(view.complete_image_size_value(), 64);
        let root = SerializedValue::Struct(vec![
            field("m_Width", SerializedValue::I32(1)),
            field("m_Height", SerializedValue::I32(1)),
            field("m_Depth", SerializedValue::I32(1)),
            field("m_CompleteImageSize", SerializedValue::I32(32)),
        ]);
        let view = Texture3DView::from_value(&root).unwrap();
        assert_eq!(view.complete_image_size_value(), 32);
    }

    #[test]
    fn mip_fallback_rounds_the_binary_logarithm() {
        fn mips(width: usize, height: usize, mip_map: bool) -> i32 {
            Texture3DView {
                width,
                height,
                depth: 1,
                format_i32: Some(4),
                format_u32: None,
                image_data: Vec::new(),
                stream_data: None,
                lightmap_format: None,
                usage_mode: 0,
                mip_count: None,
                mip_map,
                complete_image_size_u32: None,
                complete_image_size_i32: None,
            }
            .mip_count_value()
            .unwrap()
        }
        // `Convert.ToInt32(Math.Log2(n))` rounds to nearest.
        assert_eq!(mips(2, 2, true), 1);
        assert_eq!(mips(3, 2, true), 2);
        assert_eq!(mips(6, 4, true), 3);
        assert_eq!(mips(8, 8, true), 3);
        assert_eq!(mips(9, 9, true), 3);
        // A flat texture reports one mip without consulting the dimensions.
        assert_eq!(mips(0, 0, false), 1);
        // Zero sides have no logarithm, where the source conversion throws.
        assert!(matches!(
            Texture3DView {
                width: 0,
                height: 0,
                depth: 1,
                format_i32: Some(4),
                format_u32: None,
                image_data: Vec::new(),
                stream_data: None,
                lightmap_format: None,
                usage_mode: 0,
                mip_count: None,
                mip_map: true,
                complete_image_size_u32: None,
                complete_image_size_i32: None,
            }
            .mip_count_value(),
            Err(TextureArrayError::NonPositiveMipBase { .. })
        ));
    }

    #[test]
    fn array_size_rules_cover_the_branches() {
        assert_eq!(texture2d_array_complete_image_size(100, 4), 25);
        assert_eq!(texture2d_array_complete_image_size(100, 1), 100);
        assert_eq!(texture2d_array_complete_image_size(100, 0), 100);
        assert_eq!(cubemap_array_complete_image_size(120, 12), 10);
        assert_eq!(cubemap_array_complete_image_size(120, 0), 0);
        assert_eq!(cubemap_array_depth(2).unwrap(), 12);
        assert_eq!(cubemap_array_depth(0).unwrap(), 0);
        assert!(matches!(
            cubemap_array_depth(usize::try_from(i32::MAX).unwrap() / 6 + 1),
            Err(TextureArrayError::ArithmeticOverflow { .. })
        ));
    }

    #[test]
    fn selection_prefers_inline_and_reports_missing_streams() {
        let inline = [1u8, 2];
        let stream = [3u8, 4];
        assert_eq!(
            resolve_array_image_data(&inline, true, Some(&stream))
                .unwrap()
                .into_owned(),
            vec![1, 2]
        );
        assert_eq!(
            resolve_array_image_data(&[], true, Some(&stream))
                .unwrap()
                .into_owned(),
            vec![3, 4]
        );
        assert_eq!(
            resolve_array_image_data(&[], false, None)
                .unwrap()
                .into_owned(),
            Vec::<u8>::new()
        );
        assert!(matches!(
            resolve_array_image_data(&[], true, None),
            Err(TextureArrayError::StreamUnavailable)
        ));
    }

    #[test]
    fn views_reject_bad_fields() {
        let root = SerializedValue::Struct(vec![field("m_Height", SerializedValue::I32(1))]);
        assert!(matches!(
            Texture2DArrayView::from_value(&root),
            Err(TextureArrayError::MissingField { .. })
        ));
        let root = SerializedValue::Struct(vec![
            field("m_Width", SerializedValue::I32(-1)),
            field("m_Height", SerializedValue::I32(1)),
            field("m_Depth", SerializedValue::I32(1)),
        ]);
        assert!(matches!(
            Texture2DArrayView::from_value(&root),
            Err(TextureArrayError::WrongType { .. })
        ));
        let root = SerializedValue::Struct(vec![
            field("m_Width", SerializedValue::I32(1)),
            field("m_CubemapCount", SerializedValue::I32(1)),
        ]);
        assert!(CubemapArrayView::from_value(&root).is_ok());
    }

    #[test]
    fn union_precedence_and_passthroughs_match_the_source_branches() {
        fn view() -> Texture3DView {
            Texture3DView {
                width: 4,
                height: 4,
                depth: 1,
                format_i32: None,
                format_u32: None,
                image_data: Vec::new(),
                stream_data: None,
                lightmap_format: None,
                usage_mode: 1,
                mip_count: None,
                mip_map: false,
                complete_image_size_u32: None,
                complete_image_size_i32: None,
            }
        }
        // When both spellings are present the documented winner takes the field.
        let both = Texture3DView {
            format_i32: Some(7),
            format_u32: Some(8),
            complete_image_size_u32: Some(64),
            complete_image_size_i32: Some(32),
            ..view()
        };
        assert_eq!(both.format_value(), 7);
        assert_eq!(both.complete_image_size_value(), 64);
        // With neither spelling the format reads zero, which no graphics format
        // converts, so the `RGBA32` fallback applies.
        assert_eq!(view().format_value(), 0);
        assert_eq!(view().texture_format_or_default(), 4);
        // A present lightmap field wins over the usage mode.
        let lit = Texture3DView {
            lightmap_format: Some(5),
            ..view()
        };
        assert_eq!(lit.lightmap_value(), 5);
        // A present mip count passes through without validation.
        let flat = Texture3DView {
            mip_count: Some(0),
            mip_map: true,
            ..view()
        };
        assert_eq!(flat.mip_count_value().unwrap(), 0);
        // An unrepresentable depth keeps the whole data size.
        assert_eq!(texture2d_array_complete_image_size(100, usize::MAX), 100);
    }
}
