//! `TerrainHeatmap` ported from
//! `AssetRipper.Export.Modules.Textures/TerrainHeatmap.cs`, plus the height-grid kernel
//! from `AssetRipper.Export.Modules.Models/GlbTerrainBuilder.GetHeights`.
//!
//! Converts a `TerrainData` (class 156) heightmap into an RGBA32 heatmap: each `short` height is
//! normalized by `short.MaxValue`, mapped through the five-stop colour ramp, written as opaque
//! pixels, and flipped vertically. The bitmap size is `max(width, resolution)` by
//! `max(height, resolution)`. The same grid also feeds the mesh exporter as row-major
//! normalized heights; vertex positions, UV scales, and triangle winding need the mesh
//! builder and stay caller-side, as do splat-alpha texture resolution and the
//! `Terrain` (class 218) shadow decisions.
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

    /// `GlbTerrainBuilder.GetHeights`: the `max(width, resolution)` by
    /// `max(height, resolution)` grid of `height / short.MaxValue` in row-major
    /// `[x + y * width]` order, feeding the terrain mesh exporter.
    ///
    /// A short height list is the same `HeightmapTooShort` error as [`TerrainHeightmap::bitmap`];
    /// the source would throw out of range. Longer lists are read up to the grid capacity,
    /// matching the source loop bounds.
    pub fn mesh_heights(&self) -> Result<Vec<f32>, TerrainHeatmapError> {
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
        Ok(self.heights[..expected]
            .iter()
            .map(|height| f32::from(*height) / f32::from(i16::MAX))
            .collect())
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
        SerializedValue::ManagedReferenceId { .. } => "managed reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
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

    #[test]
    fn mesh_heights_normalize_the_grid_in_row_major_order() {
        let view =
            TerrainHeightmap::from_value(&terrain_with(2, 2, 2, &[0, 16384, -16384, i16::MAX]))
                .unwrap();
        let grid = view.mesh_heights().unwrap();
        assert_eq!(grid.len(), 4);
        assert_eq!(grid[0], 0.0);
        assert_eq!(grid[1], 16384.0 / 32767.0);
        assert_eq!(grid[2], -16384.0 / 32767.0);
        assert_eq!(grid[3], 1.0);
        // Resolution padding grows the grid row-major, like the bitmap dimensions.
        let padded = TerrainHeightmap::from_value(&terrain_with(1, 1, 2, &[1, 2, 3, 4])).unwrap();
        assert_eq!(padded.bitmap_width(), 2);
        assert_eq!(padded.bitmap_height(), 2);
        assert_eq!(padded.mesh_heights().unwrap().len(), 4);
        // A short list reports the same shortfall as the bitmap path.
        let short = TerrainHeightmap::from_value(&terrain(&[0], 2)).unwrap();
        assert!(matches!(
            short.mesh_heights(),
            Err(TerrainHeatmapError::HeightmapTooShort {
                actual: 1,
                required: 4
            })
        ));
    }

    fn heightmap_root(
        width: SerializedValue,
        height: SerializedValue,
        resolution: SerializedValue,
        heights: SerializedValue,
    ) -> SerializedValue {
        SerializedValue::Struct(vec![field(
            "m_Heightmap",
            SerializedValue::Struct(vec![
                field("m_Width", width),
                field("m_Height", height),
                field("m_Resolution", resolution),
                field("m_Heights", heights),
            ]),
        )])
    }

    #[test]
    fn field_errors_follow_the_source_guards() {
        // Negative dimensions fail.
        let root = terrain_with(-3, 1, 1, &[0]);
        assert!(matches!(
            TerrainHeightmap::from_value(&root),
            Err(TerrainHeatmapError::NegativeValue { field: "m_Width" })
        ));
        // Wrong kinds name the decoded kind.
        let bad_width = heightmap_root(
            SerializedValue::F32(1.0),
            SerializedValue::I32(1),
            SerializedValue::I32(1),
            SerializedValue::Array(vec![]),
        );
        assert!(matches!(
            TerrainHeightmap::from_value(&bad_width),
            Err(TerrainHeatmapError::WrongType { .. })
        ));
        let bad_list = heightmap_root(
            SerializedValue::I32(1),
            SerializedValue::I32(1),
            SerializedValue::I32(1),
            SerializedValue::I32(0),
        );
        assert!(matches!(
            TerrainHeightmap::from_value(&bad_list),
            Err(TerrainHeatmapError::WrongType { .. })
        ));
        let bad_element = heightmap_root(
            SerializedValue::I32(1),
            SerializedValue::I32(1),
            SerializedValue::I32(1),
            SerializedValue::Array(vec![SerializedValue::F32(0.0)]),
        );
        assert!(matches!(
            TerrainHeightmap::from_value(&bad_element),
            Err(TerrainHeatmapError::WrongType { .. })
        ));
        // Unsigned dimensions read like signed ones.
        let unsigned = heightmap_root(
            SerializedValue::U32(2),
            SerializedValue::U32(1),
            SerializedValue::U32(2),
            SerializedValue::Array(vec![
                SerializedValue::I16(0),
                SerializedValue::I16(0),
                SerializedValue::I16(0),
                SerializedValue::I16(0),
            ]),
        );
        let view = TerrainHeightmap::from_value(&unsigned).unwrap();
        assert_eq!((view.width, view.height, view.resolution), (2, 1, 2));
        // Duplicate names surface the lookup failure.
        let doubled = SerializedValue::Struct(vec![
            field("m_Heightmap", SerializedValue::I32(0)),
            field("m_Heightmap", SerializedValue::I32(0)),
        ]);
        assert!(matches!(
            TerrainHeightmap::from_value(&doubled),
            Err(TerrainHeatmapError::FieldLookup {
                field: "m_Heightmap",
                ..
            })
        ));
        // Overflowing dimensions fail without allocating the grid.
        let huge = TerrainHeightmap {
            width: usize::MAX,
            height: 2,
            resolution: 1,
            heights: vec![0],
        };
        assert!(matches!(
            huge.bitmap(),
            Err(TerrainHeatmapError::DimensionOverflow)
        ));
        assert!(matches!(
            huge.mesh_heights(),
            Err(TerrainHeatmapError::DimensionOverflow)
        ));
    }

    #[test]
    fn heatmap_error_messages_pin_the_reporting_strings() {
        assert_eq!(
            TerrainHeatmapError::MissingField {
                field: "m_Heightmap"
            }
            .to_string(),
            "TerrainData is missing required field 'm_Heightmap'"
        );
        assert_eq!(
            TerrainHeatmapError::WrongType {
                field: "m_Width",
                actual: "float"
            }
            .to_string(),
            "TerrainData field 'm_Width' has kind float, expected a number"
        );
        assert_eq!(
            TerrainHeatmapError::NegativeValue { field: "m_Height" }.to_string(),
            "TerrainData field 'm_Height' is negative"
        );
        assert_eq!(
            TerrainHeatmapError::DimensionOverflow.to_string(),
            "TerrainData heightmap dimensions overflow usize"
        );
        assert_eq!(
            TerrainHeatmapError::HeightmapTooShort {
                actual: 1,
                required: 4
            }
            .to_string(),
            "TerrainData heightmap has 1 heights; needs 4"
        );
        let doubled = SerializedValue::Struct(vec![
            field("m_Heightmap", SerializedValue::I32(0)),
            field("m_Heightmap", SerializedValue::I32(0)),
        ]);
        let error = TerrainHeightmap::from_value(&doubled).unwrap_err();
        assert_eq!(
            error.to_string(),
            "could not read TerrainData field 'm_Heightmap': \
             field 'm_Heightmap' occurs 2 times in the decoded struct"
        );
    }
}
