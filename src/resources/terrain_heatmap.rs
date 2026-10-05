//! `TerrainHeatmap` ported from
//! `AssetRipper.Export.Modules.Textures/TerrainHeatmap.cs`.
//!
//! Converts a `TerrainData` (class 156) heightmap into an RGBA32 heatmap: each `short` height is
//! normalized by `short.MaxValue`, mapped through the five-stop colour ramp, written as opaque
//! pixels, and flipped vertically. The bitmap size is `max(width, resolution)` by
//! `max(height, resolution)`.
//!
//! The `(Color32)ColorFloat` conversion is the reference's `ConvertFloatToByte`: `NaN` and values
//! `<= 0` become `0`, values `>= 255` clamp to `255`, and the rest truncate.

use std::error::Error;
use std::fmt::{self, Display};

use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};

use super::texture_converter::Bitmap;
use super::texture_decode::float_to_byte;

const ZERO: [f32; 4] = [0.0, 0.0, 0.4, 1.0];
const Q1: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const Q2: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const Q3: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const ONE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const Q1_POINT: f32 = 0.15;
const Q2_POINT: f32 = 0.3;
const Q3_POINT: f32 = 0.6;

/// A failure while reading a `TerrainData` heightmap.
#[derive(Debug)]
pub enum TerrainHeatmapError {
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
    /// A dimension or the height count is negative.
    NegativeValue {
        /// The offending field.
        field: &'static str,
    },
    /// The bitmap dimensions would not fit `usize`.
    DimensionOverflow,
    /// The height array is shorter than `width * height`.
    HeightmapTooShort {
        /// Provided height count.
        actual: usize,
        /// Required height count.
        required: usize,
    },
}

impl Display for TerrainHeatmapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field, source } => {
                write!(
                    formatter,
                    "could not read TerrainData field '{field}': {source}"
                )
            }
            Self::MissingField { field } => {
                write!(formatter, "TerrainData is missing required field '{field}'")
            }
            Self::WrongType { field, actual } => write!(
                formatter,
                "TerrainData field '{field}' has kind {actual}, expected a number"
            ),
            Self::NegativeValue { field } => {
                write!(formatter, "TerrainData field '{field}' is negative")
            }
            Self::DimensionOverflow => {
                formatter.write_str("TerrainData heightmap dimensions overflow usize")
            }
            Self::HeightmapTooShort { actual, required } => write!(
                formatter,
                "TerrainData heightmap has {actual} heights; needs {required}"
            ),
        }
    }
}

impl Error for TerrainHeatmapError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FieldLookup { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The `TerrainData` heightmap fields `TerrainHeatmap` reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerrainHeightmap {
    /// `m_Heightmap.m_Width`.
    pub width: usize,
    /// `m_Heightmap.m_Height`.
    pub height: usize,
    /// `m_Heightmap.m_Resolution`.
    pub resolution: usize,
    /// `m_Heightmap.m_Heights`.
    pub heights: Vec<i16>,
}

impl TerrainHeightmap {
    /// Reads the heightmap from a decoded `TerrainData` payload.
    pub fn from_value(root: &SerializedValue) -> Result<Self, TerrainHeatmapError> {
        let heightmap = root
            .unique_field_in_base_chain("m_Heightmap")
            .map_err(|source| TerrainHeatmapError::FieldLookup {
                field: "m_Heightmap",
                source,
            })?
            .ok_or(TerrainHeatmapError::MissingField {
                field: "m_Heightmap",
            })?;
        let width = required_usize(heightmap, "m_Width")?;
        let height = required_usize(heightmap, "m_Height")?;
        let resolution = required_usize(heightmap, "m_Resolution")?;
        let heights = required_i16_array(heightmap, "m_Heights")?;
        Ok(Self {
            width,
            height,
            resolution,
            heights,
        })
    }

    /// `Math.Max(Width, Resolution)`.
    pub fn bitmap_width(&self) -> usize {
        self.width.max(self.resolution)
    }

    /// `Math.Max(Height, Resolution)`.
    pub fn bitmap_height(&self) -> usize {
        self.height.max(self.resolution)
    }

    /// `TerrainHeatmap.GetRGBA32Data`.
    pub fn rgba32_data(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(self.heights.len() * 4);
        for height in &self.heights {
            let color = convert_to_color(f32::from(*height) / f32::from(i16::MAX));
            result.push(float_to_byte(color[0]));
            result.push(float_to_byte(color[1]));
            result.push(float_to_byte(color[2]));
            result.push(u8::MAX); // the source's "small optimization"
        }
        result
    }

    /// `TerrainHeatmap.GetBitmap`: the RGBA32 data in a `max(dimension, resolution)` bitmap, flipped.
    pub fn bitmap(&self) -> Result<Bitmap, TerrainHeatmapError> {
        let width = self.bitmap_width();
        let height = self.bitmap_height();
        let expected = width
            .checked_mul(height)
            .ok_or(TerrainHeatmapError::DimensionOverflow)?;
        if self.heights.len() < expected {
            return Err(TerrainHeatmapError::HeightmapTooShort {
                actual: self.heights.len(),
                required: expected,
            });
        }
        let mut bitmap = Bitmap {
            width,
            height,
            depth: 1,
            is_rgba8: true,
            has_alpha: true,
            pixels: self.rgba32_data(),
        };
        // The source allocates `width * height` pixels from a possibly longer height list, so
        // trim to the bitmap capacity before flipping.
        bitmap.pixels.truncate(expected * 4);
        bitmap.flip_y();
        Ok(bitmap)
    }
}

fn required_usize(
    root: &SerializedValue,
    field: &'static str,
) -> Result<usize, TerrainHeatmapError> {
    let value = root
        .unique_field_in_base_chain(field)
        .map_err(|source| TerrainHeatmapError::FieldLookup { field, source })?
        .ok_or(TerrainHeatmapError::MissingField { field })?;
    let raw = match value {
        SerializedValue::I32(value) => i64::from(*value),
        SerializedValue::U32(value) => i64::from(*value),
        other => {
            return Err(TerrainHeatmapError::WrongType {
                field,
                actual: serialized_kind(other),
            })
        }
    };
    usize::try_from(raw).map_err(|_| TerrainHeatmapError::NegativeValue { field })
}

fn required_i16_array(
    root: &SerializedValue,
    field: &'static str,
) -> Result<Vec<i16>, TerrainHeatmapError> {
    let value = root
        .unique_field_in_base_chain(field)
        .map_err(|source| TerrainHeatmapError::FieldLookup { field, source })?
        .ok_or(TerrainHeatmapError::MissingField { field })?;
    let SerializedValue::Array(values) = value else {
        return Err(TerrainHeatmapError::WrongType {
            field,
            actual: serialized_kind(value),
        });
    };
    let mut heights = Vec::new();
    heights
        .try_reserve_exact(values.len())
        .map_err(|_| TerrainHeatmapError::DimensionOverflow)?;
    for element in values {
        match element {
            SerializedValue::I16(height) => heights.push(*height),
            other => {
                return Err(TerrainHeatmapError::WrongType {
                    field,
                    actual: serialized_kind(other),
                })
            }
        }
    }
    Ok(heights)
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

fn convert_to_color(value: f32) -> [f32; 4] {
    if value <= 0.0 {
        ZERO
    } else if value < Q1_POINT {
        average(ZERO, 0.0, Q1, Q1_POINT, value)
    } else if value < Q2_POINT {
        average(Q1, Q1_POINT, Q2, Q2_POINT, value)
    } else if value < Q3_POINT {
        average(Q2, Q2_POINT, Q3, Q3_POINT, value)
    } else if value < 1.0 {
        average(Q3, Q3_POINT, ONE, 1.0, value)
    } else {
        ONE
    }
}

fn normalize(min: f32, max: f32, value: f32) -> f32 {
    if value <= min {
        0.0
    } else if value >= max {
        1.0
    } else {
        (value - min) / (max - min)
    }
}

fn average(min_color: [f32; 4], min: f32, max_color: [f32; 4], max: f32, value: f32) -> [f32; 4] {
    let normalized = normalize(min, max, value);
    let mut result = [0.0f32; 4];
    for index in 0..4 {
        result[index] = (1.0 - normalized) * min_color[index] + normalized * max_color[index];
    }
    result
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

    fn terrain_with(width: i32, height: i32, resolution: i32, heights: &[i16]) -> SerializedValue {
        SerializedValue::Struct(vec![field(
            "m_Heightmap",
            SerializedValue::Struct(vec![
                field("m_Width", SerializedValue::I32(width)),
                field("m_Height", SerializedValue::I32(height)),
                field("m_Resolution", SerializedValue::I32(resolution)),
                field(
                    "m_Heights",
                    SerializedValue::Array(
                        heights.iter().map(|h| SerializedValue::I16(*h)).collect(),
                    ),
                ),
            ]),
        )])
    }

    fn terrain(heights: &[i16], resolution: i32) -> SerializedValue {
        terrain_with(heights.len() as i32, 1, resolution, heights)
    }

    #[test]
    fn reads_the_heightmap_fields() {
        let view = TerrainHeightmap::from_value(&terrain(&[0, 100, -100], 1)).unwrap();
        assert_eq!(view.width, 3);
        assert_eq!(view.height, 1);
        assert_eq!(view.resolution, 1);
        assert_eq!(view.heights, vec![0, 100, -100]);
        assert_eq!(view.bitmap_width(), 3);
        assert_eq!(view.bitmap_height(), 1);
    }

    #[test]
    fn zero_height_maps_to_the_zero_colour() {
        let view = TerrainHeightmap::from_value(&terrain(&[0], 1)).unwrap();
        assert_eq!(view.rgba32_data(), vec![0, 0, 102, 255]);
    }

    #[test]
    fn maximum_height_maps_to_white() {
        let view = TerrainHeightmap::from_value(&terrain(&[i16::MAX], 1)).unwrap();
        assert_eq!(view.rgba32_data(), vec![255, 255, 255, 255]);
    }

    #[test]
    fn resolution_grows_the_bitmap_and_flips_rows() {
        // A 2x2 heightmap at resolution 2: row 0 is black, row 1 is white.
        let view =
            TerrainHeightmap::from_value(&terrain_with(2, 2, 2, &[0, 0, i16::MAX, i16::MAX]))
                .unwrap();
        assert_eq!(view.bitmap_width(), 2);
        assert_eq!(view.bitmap_height(), 2);
        let bitmap = view.bitmap().unwrap();
        assert_eq!(bitmap.pixels.len(), 16);
        // `FlipY` moves the source's second row (white) to the top.
        assert_eq!(bitmap.pixels[0..4], [255, 255, 255, 255]);
        assert_eq!(bitmap.pixels[8..12], [0, 0, 102, 255]);
    }

    #[test]
    fn short_heightmap_is_reported() {
        // A 1-pixel heightmap at resolution 2 needs 4 pixels of data.
        let view = TerrainHeightmap::from_value(&terrain(&[0], 2)).unwrap();
        assert!(matches!(
            view.bitmap(),
            Err(TerrainHeatmapError::HeightmapTooShort {
                actual: 1,
                required: 4
            })
        ));
    }

    #[test]
    fn missing_heightmap_is_reported() {
        let root = SerializedValue::Struct(vec![field("m_Width", SerializedValue::I32(1))]);
        assert!(matches!(
            TerrainHeightmap::from_value(&root),
            Err(TerrainHeatmapError::MissingField {
                field: "m_Heightmap"
            })
        ));
    }
}
