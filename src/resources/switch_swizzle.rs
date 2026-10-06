//! Switch texture unswizzling ported from
//! `AssetRipper.SourceGenerated.Extensions/SwitchSwizzle.cs`.
//!
//! `GetImageData` routes Switch textures with a platform blob of at least 12 bytes
//! through `Unswizzle`, which rearranges GOB-swizzled blocks into linear order,
//! crops padding rows, and converts the corrected RGBA layouts back to RGB. Every
//! input arrives as a caller fact; buffer overruns that the source answers with
//! `Debug.Assert` (stripped in release builds) or span-copy throws are explicit
//! errors here.
//!
//! Texture-format discriminants follow the injected `UnityEngine.TextureFormat`
//! values (`Pass039_InjectEnumValues`, plus the negative ASTC literals the
//! baseline still names): `ARGBFloat = 6`, `BGR24 = 8`, `DXT3 = 11`,
//! `ASTC_RGB_* = -48..-53`, `ASTC_RGBA_* = -54..-59`, `ASTC_HDR_* = 66..71`.

use std::error::Error;
use std::fmt::{self, Display};

/// Texels per GOB row in `SwitchSwizzle`.
pub const GOB_X_TEXEL_COUNT: u32 = 4;
/// Texels per GOB column in `SwitchSwizzle`.
pub const GOB_Y_TEXEL_COUNT: u32 = 8;
/// Bytes per texel slot in the swizzled stream.
pub const TEXEL_BYTE_SIZE: usize = 16;
/// Texel slots per GOB (`GobXTexelCount * GobYTexelCount`).
pub const BLOCKS_IN_GOB: usize = 32;

/// `GobXPoses`: the GOB-local X slot of each of the 32 stream positions.
pub const GOB_X_POSES: [u32; BLOCKS_IN_GOB] = [
    0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 3, 3, 2, 2, 3, 3, 2, 2, 3, 3, 2, 2, 3, 3,
];

/// `GobYPoses`: the GOB-local Y slot of each of the 32 stream positions.
pub const GOB_Y_POSES: [u32; BLOCKS_IN_GOB] = [
    0, 1, 0, 1, 2, 3, 2, 3, 4, 5, 4, 5, 6, 7, 6, 7, 0, 1, 0, 1, 2, 3, 2, 3, 4, 5, 4, 5, 6, 7, 6, 7,
];

/// The platform-blob size `GetImageData` requires before calling `Unswizzle`.
pub const PLATFORM_BLOB_MIN_LENGTH: usize = 12;
/// The platform-blob offset holding the texel-height shift.
pub const TEXEL_HEIGHT_BLOB_OFFSET: usize = 8;

/// Why Switch bytes cannot be unswizzled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwitchSwizzleError {
    /// The platform blob is shorter than the 12 bytes the gate requires.
    PlatformBlobTooShort { byte_count: usize },
    /// The blob's texel-height shift yields no positive height. Real blobs carry
    /// small shifts; anything else has no defined source outcome.
    InvalidTexelHeight { shift: i32 },
    /// A format the block tables do not know, which the source answers with
    /// `NotSupportedException` from `GetBlockSize`. Unreachable when the block
    /// dimensions are known, since both tables cover the same formats.
    UnsupportedBlockFormat { format: i32 },
    /// An intermediate product overflows.
    ArithmeticOverflow { what: &'static str },
    /// A stream or destination range leaves the available bytes.
    RangeOutOfBounds {
        /// Which copy step failed.
        step: &'static str,
        /// The first byte the step needs.
        offset: u64,
        /// The bytes the step needs.
        length: u64,
        /// The bytes available.
        available: usize,
    },
}

impl Display for SwitchSwizzleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformBlobTooShort { byte_count } => write!(
                formatter,
                "switch platform blob holds {byte_count} bytes, fewer than the required {PLATFORM_BLOB_MIN_LENGTH}"
            ),
            Self::InvalidTexelHeight { shift } => write!(
                formatter,
                "switch platform blob shift {shift} yields no positive texel height"
            ),
            Self::UnsupportedBlockFormat { format } => write!(
                formatter,
                "switch unswizzle does not support texture format {format}"
            ),
            Self::ArithmeticOverflow { what } => {
                write!(formatter, "switch unswizzle overflowed computing {what}")
            }
            Self::RangeOutOfBounds {
                step,
                offset,
                length,
                available,
            } => write!(
                formatter,
                "switch unswizzle step {step} needs bytes {offset}..{} but only {available} are available",
                offset + length,
            ),
        }
    }
}

impl Error for SwitchSwizzleError {}

/// `CeilDivide` over non-negative inputs: the smallest integer greater than or
/// equal to `a / b`.
fn ceil_divide(a: u64, b: u64) -> Result<u64, SwitchSwizzleError> {
    if b == 0 {
        return Err(SwitchSwizzleError::ArithmeticOverflow {
            what: "ceil divisor",
        });
    }
    a.checked_add(b - 1)
        .and_then(|sum| sum.checked_div(b))
        .ok_or(SwitchSwizzleError::ArithmeticOverflow {
            what: "ceiling division",
        })
}

/// `GetBlockDimensions`: the block footprint of formats `Unswizzle` understands.
/// `None` backs out and returns the data unchanged, like the source.
pub const fn block_dimensions(format: i32) -> Option<(u32, u32)> {
    match format {
        1 /* Alpha8 */ => Some((1, 1)),
        2 /* ARGB4444 */ => Some((1, 1)),
        13 /* RGBA4444 */ => Some((1, 1)),
        5 /* ARGB32 */ => Some((1, 1)),
        14 /* BGRA32_14 */ | 37 /* BGRA32_37 */ => Some((1, 1)),
        7 /* RGB565 */ => Some((1, 1)),
        10 /* DXT1 */ => Some((4, 4)),
        12 /* DXT5 */ => Some((4, 4)),
        26 /* BC4 */ => Some((4, 4)),
        27 /* BC5 */ => Some((4, 4)),
        24 /* BC6H */ => Some((4, 4)),
        25 /* BC7 */ => Some((4, 4)),
        -48 /* ASTC_RGB_4x4 */ | -54 /* ASTC_RGBA_4x4 */ | 66 /* ASTC_HDR_4x4 */ => Some((4, 4)),
        -49 /* ASTC_RGB_5x5 */ | -55 /* ASTC_RGBA_5x5 */ | 67 /* ASTC_HDR_5x5 */ => Some((5, 5)),
        -50 /* ASTC_RGB_6x6 */ | -56 /* ASTC_RGBA_6x6 */ | 68 /* ASTC_HDR_6x6 */ => Some((6, 6)),
        -51 /* ASTC_RGB_8x8 */ | -57 /* ASTC_RGBA_8x8 */ | 69 /* ASTC_HDR_8x8 */ => Some((8, 8)),
        -52 /* ASTC_RGB_10x10 */ | -58 /* ASTC_RGBA_10x10 */ | 70 /* ASTC_HDR_10x10 */ => {
            Some((10, 10))
        }
        -53 /* ASTC_RGB_12x12 */ | -59 /* ASTC_RGBA_12x12 */ | 71 /* ASTC_HDR_12x12 */ => {
            Some((12, 12))
        }
        63 /* R8 */ | 75 /* R8_SIGNED */ => Some((1, 1)),
        62 /* RG16 */ | 76 /* RG16_SIGNED */ => Some((1, 1)),
        4 /* RGBA32 */ | 78 /* RGBA32_SIGNED */ => Some((1, 1)),
        9 /* R16 */ | 79 /* R16_SIGNED */ | 15 /* RHalf */ => Some((1, 1)),
        72 /* RG32 */ | 80 /* RG32_SIGNED */ | 16 /* RGHalf */ => Some((1, 1)),
        74 /* RGBA64 */ | 82 /* RGBA64_SIGNED */ | 17 /* RGBAHalf */ => Some((1, 1)),
        18 /* RFloat */ => Some((1, 1)),
        19 /* RGFloat */ => Some((1, 1)),
        20 /* RGBAFloat */ => Some((1, 1)),
        6 /* ARGBFloat */ => Some((1, 1)),
        _ => None,
    }
}

/// `GetBlockSize`: the bytes per block. Every format with known dimensions has a
/// known size; anything else is an explicit error like the source throw.
pub const fn block_size(format: i32) -> Result<u32, SwitchSwizzleError> {
    match format {
        1 /* Alpha8 */ => Ok(1),
        2 /* ARGB4444 */ => Ok(2),
        13 /* RGBA4444 */ => Ok(2),
        5 /* ARGB32 */ => Ok(4),
        14 /* BGRA32_14 */ | 37 /* BGRA32_37 */ => Ok(4),
        7 /* RGB565 */ => Ok(2),
        10 /* DXT1 */ => Ok(8),
        12 /* DXT5 */ => Ok(16),
        26 /* BC4 */ => Ok(8),
        27 /* BC5 */ => Ok(16),
        24 /* BC6H */ => Ok(16),
        25 /* BC7 */ => Ok(16),
        -48 /* ASTC_RGB_4x4 */ | -54 /* ASTC_RGBA_4x4 */ | 66 /* ASTC_HDR_4x4 */ => Ok(16),
        -49 /* ASTC_RGB_5x5 */ | -55 /* ASTC_RGBA_5x5 */ | 67 /* ASTC_HDR_5x5 */ => Ok(16),
        -50 /* ASTC_RGB_6x6 */ | -56 /* ASTC_RGBA_6x6 */ | 68 /* ASTC_HDR_6x6 */ => Ok(16),
        -51 /* ASTC_RGB_8x8 */ | -57 /* ASTC_RGBA_8x8 */ | 69 /* ASTC_HDR_8x8 */ => Ok(16),
        -52 /* ASTC_RGB_10x10 */ | -58 /* ASTC_RGBA_10x10 */ | 70 /* ASTC_HDR_10x10 */ => Ok(16),
        -53 /* ASTC_RGB_12x12 */ | -59 /* ASTC_RGBA_12x12 */ | 71 /* ASTC_HDR_12x12 */ => Ok(16),
        63 /* R8 */ | 75 /* R8_SIGNED */ => Ok(1),
        62 /* RG16 */ | 76 /* RG16_SIGNED */ => Ok(2),
        4 /* RGBA32 */ | 78 /* RGBA32_SIGNED */ => Ok(4),
        9 /* R16 */ | 79 /* R16_SIGNED */ | 15 /* RHalf */ => Ok(2),
        72 /* RG32 */ | 80 /* RG32_SIGNED */ | 16 /* RGHalf */ => Ok(4),
        74 /* RGBA64 */ | 82 /* RGBA64_SIGNED */ | 17 /* RGBAHalf */ => Ok(8),
        18 /* RFloat */ => Ok(4),
        19 /* RGFloat */ => Ok(8),
        20 /* RGBAFloat */ => Ok(16),
        6 /* ARGBFloat */ => Ok(16),
        _ => Err(SwitchSwizzleError::UnsupportedBlockFormat { format }),
    }
}

/// `GetCorrectedSwitchTextureFormat`: RGB layouts the blob stores widened to RGBA.
pub const fn corrected_switch_format(format: i32) -> i32 {
    match format {
        3 /* RGB24 */ => 4 /* RGBA32 */,
        73 /* RGB48 */ => 74 /* RGBA64 */,
        77 /* RGB24_SIGNED */ => 78 /* RGBA32_SIGNED */,
        81 /* RGB48_SIGNED */ => 82 /* RGBA64_SIGNED */,
        8 /* BGR24 */ => 14 /* BGRA32_14 */,
        _ => format,
    }
}

/// `GetTexelHeightByPlatformBlob`: `1` shifted by the blob's int32 at offset 8.
///
/// Only shifts `0..=30` are accepted. Real blobs carry small shifts; a negative
/// shift or a shift past the int width has no defined source outcome (the C#
/// shift masks the count and can yield non-positive heights that silently skip
/// the GOB loops), so both are explicit errors here.
pub fn texel_height_from_platform_blob(
    platform_blob: &[u8],
    little_endian: bool,
) -> Result<u32, SwitchSwizzleError> {
    if platform_blob.len() < PLATFORM_BLOB_MIN_LENGTH {
        return Err(SwitchSwizzleError::PlatformBlobTooShort {
            byte_count: platform_blob.len(),
        });
    }
    let word = &platform_blob[TEXEL_HEIGHT_BLOB_OFFSET..TEXEL_HEIGHT_BLOB_OFFSET + 4];
    let bytes = [word[0], word[1], word[2], word[3]];
    let shift = if little_endian {
        i32::from_le_bytes(bytes)
    } else {
        i32::from_be_bytes(bytes)
    };
    if !(0..=30).contains(&shift) {
        return Err(SwitchSwizzleError::InvalidTexelHeight { shift });
    }
    Ok(1u32 << shift)
}

/// Inputs to `Unswizzle`, all as caller facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchUnswizzleInput<'a> {
    /// `ITexture2D.ImageCount_C28`.
    pub image_count: u32,
    /// `ITexture2D.Format_C28E` as a discriminant.
    pub format: i32,
    /// `ITexture2D.Width_C28`.
    pub width: u32,
    /// `ITexture2D.Height_C28`.
    pub height: u32,
    /// `ITexture2D.PlatformBlob_C28`.
    pub platform_blob: &'a [u8],
    /// The collection endianness reading the blob shift.
    pub platform_blob_little_endian: bool,
    /// The swizzled image bytes.
    pub data: &'a [u8],
}

/// `SwitchSwizzle.Unswizzle`: GOB-swizzled blocks to linear order with padding
/// cropped and corrected RGBA layouts converted back to RGB.
///
/// Non-single images and unknown block layouts pass the data through unchanged,
/// like the source. The result keeps the input length: cropped rows leave unused
/// tail bytes and the RGB conversion clears its own tail, because downstream
/// decoding expects at least `CompleteImageSize` bytes.
pub fn unswizzle(input: &SwitchUnswizzleInput<'_>) -> Result<Vec<u8>, SwitchSwizzleError> {
    if input.image_count != 1 {
        return Ok(input.data.to_vec());
    }
    let real_format = corrected_switch_format(input.format);
    let Some((block_width, block_height)) = block_dimensions(real_format) else {
        return Ok(input.data.to_vec());
    };
    let block_byte_size = block_size(real_format)? as u64;
    let blocks_per_texel = TEXEL_BYTE_SIZE as u64 / block_byte_size;
    let texel_width = block_width as u64 * blocks_per_texel;
    let texel_height_units = block_height as u64;
    let texel_height = u64::from(texel_height_from_platform_blob(
        input.platform_blob,
        input.platform_blob_little_endian,
    )?);

    let padded_width = ceil_divide(
        input.width as u64,
        texel_width * u64::from(GOB_X_TEXEL_COUNT),
    )? * texel_width
        * u64::from(GOB_X_TEXEL_COUNT);
    let padded_height = ceil_divide(
        input.height as u64,
        texel_height_units * u64::from(GOB_Y_TEXEL_COUNT) * texel_height,
    )? * texel_height_units
        * u64::from(GOB_Y_TEXEL_COUNT)
        * texel_height;
    let texel_count_x = ceil_divide(padded_width, texel_width)?;
    let texel_count_y = ceil_divide(padded_height, texel_height_units)?;
    let gob_count_x = texel_count_x / u64::from(GOB_X_TEXEL_COUNT);
    let gob_count_y = texel_count_y / u64::from(GOB_Y_TEXEL_COUNT);

    let data_length = input.data.len() as u64;
    let mut reordered = vec![0u8; input.data.len()];
    let mut src_pos: u64 = 0;
    for i in 0..gob_count_y / texel_height {
        for j in 0..gob_count_x {
            for k in 0..texel_height {
                for l in 0..BLOCKS_IN_GOB as u64 {
                    let gob_x = u64::from(GOB_X_POSES[l as usize]);
                    let gob_y = u64::from(GOB_Y_POSES[l as usize]);
                    let dst_x = j * u64::from(GOB_X_TEXEL_COUNT) + gob_x;
                    let dst_y = (i * texel_height + k) * u64::from(GOB_Y_TEXEL_COUNT) + gob_y;
                    let dst = dst_y
                        .checked_mul(texel_count_x)
                        .and_then(|row| row.checked_mul(TEXEL_BYTE_SIZE as u64))
                        .and_then(|base| base.checked_add(dst_x * TEXEL_BYTE_SIZE as u64))
                        .ok_or(SwitchSwizzleError::ArithmeticOverflow {
                            what: "gob address",
                        })?;
                    copy_step(
                        input.data,
                        &mut reordered,
                        src_pos,
                        dst,
                        TEXEL_BYTE_SIZE as u64,
                    )?;
                    src_pos += TEXEL_BYTE_SIZE as u64;
                }
            }
        }
    }

    let mut result = reordered;
    let src_block_count_x = texel_count_x.checked_mul(blocks_per_texel).ok_or(
        SwitchSwizzleError::ArithmeticOverflow {
            what: "source block row",
        },
    )?;
    let dst_block_count_x = ceil_divide(input.width as u64, block_width as u64)?;
    if dst_block_count_x != src_block_count_x {
        let mut cropped = vec![0u8; input.data.len()];
        let dst_block_count_y = ceil_divide(input.height as u64, block_height as u64)?;
        for y in 0..dst_block_count_y {
            let from = y
                .checked_mul(src_block_count_x)
                .and_then(|row| row.checked_mul(block_byte_size))
                .ok_or(SwitchSwizzleError::ArithmeticOverflow {
                    what: "crop source row",
                })?;
            let to = y
                .checked_mul(dst_block_count_x)
                .and_then(|row| row.checked_mul(block_byte_size))
                .ok_or(SwitchSwizzleError::ArithmeticOverflow {
                    what: "crop target row",
                })?;
            let length = dst_block_count_x.checked_mul(block_byte_size).ok_or(
                SwitchSwizzleError::ArithmeticOverflow {
                    what: "crop row length",
                },
            )?;
            copy_step(&result, &mut cropped, from, to, length)?;
        }
        result = cropped;
    }

    if real_format != input.format {
        // Convert RGBA to RGB: every block keeps its first three quarters.
        if !block_byte_size.is_multiple_of(4) {
            return Err(SwitchSwizzleError::ArithmeticOverflow {
                what: "rgba block quarter",
            });
        }
        if block_height != 1 {
            return Err(SwitchSwizzleError::ArithmeticOverflow {
                what: "rgba block height",
            });
        }
        let actual_block_byte_size = block_byte_size / 4 * 3;
        for y in 0..input.height as u64 {
            let src_offset = y
                .checked_mul(input.width as u64)
                .and_then(|row| row.checked_mul(block_byte_size))
                .ok_or(SwitchSwizzleError::ArithmeticOverflow {
                    what: "rgba source row",
                })?;
            let dst_offset = y
                .checked_mul(input.width as u64)
                .and_then(|row| row.checked_mul(actual_block_byte_size))
                .ok_or(SwitchSwizzleError::ArithmeticOverflow {
                    what: "rgba target row",
                })?;
            for x in 0..input.width as u64 {
                let from = src_offset
                    .checked_add(x.checked_mul(block_byte_size).ok_or(
                        SwitchSwizzleError::ArithmeticOverflow {
                            what: "rgba source cell",
                        },
                    )?)
                    .ok_or(SwitchSwizzleError::ArithmeticOverflow {
                        what: "rgba source cell",
                    })?;
                let to = dst_offset
                    .checked_add(x.checked_mul(actual_block_byte_size).ok_or(
                        SwitchSwizzleError::ArithmeticOverflow {
                            what: "rgba target cell",
                        },
                    )?)
                    .ok_or(SwitchSwizzleError::ArithmeticOverflow {
                        what: "rgba target cell",
                    })?;
                // In-place downwards move like the source span copy (`srcIndex` is
                // always at or past `dstIndex`); `copy_within` has memmove semantics.
                copy_within_step(&mut result, from, to, actual_block_byte_size)?;
            }
        }
        let new_size = (input.width as u64)
            .checked_mul(input.height as u64)
            .and_then(|area| area.checked_mul(actual_block_byte_size))
            .ok_or(SwitchSwizzleError::ArithmeticOverflow { what: "rgba tail" })?;
        if new_size > data_length {
            return Err(SwitchSwizzleError::RangeOutOfBounds {
                step: "rgba tail clear",
                offset: new_size,
                length: 0,
                available: input.data.len(),
            });
        }
        result[new_size as usize..].fill(0);
    }

    Ok(result)
}

/// One bounded in-buffer move with memmove semantics.
fn copy_within_step(
    buffer: &mut [u8],
    from: u64,
    to: u64,
    length: u64,
) -> Result<(), SwitchSwizzleError> {
    let (from, to, length) = (
        usize::try_from(from).map_err(|_| SwitchSwizzleError::ArithmeticOverflow {
            what: "move offset",
        })?,
        usize::try_from(to).map_err(|_| SwitchSwizzleError::ArithmeticOverflow {
            what: "move offset",
        })?,
        usize::try_from(length).map_err(|_| SwitchSwizzleError::ArithmeticOverflow {
            what: "move length",
        })?,
    );
    if from > buffer.len()
        || length > buffer.len() - from
        || to > buffer.len()
        || length > buffer.len() - to
    {
        return Err(SwitchSwizzleError::RangeOutOfBounds {
            step: "in-place move",
            offset: from as u64,
            length: length as u64,
            available: buffer.len(),
        });
    }
    buffer.copy_within(from..from + length, to);
    Ok(())
}

/// One bounded `$src[src..][..len] -> $dst[dst..]` copy with the source step name.
fn copy_step(
    source: &[u8],
    target: &mut [u8],
    from: u64,
    to: u64,
    length: u64,
) -> Result<(), SwitchSwizzleError> {
    let (from, to, length) = (
        usize::try_from(from).map_err(|_| SwitchSwizzleError::ArithmeticOverflow {
            what: "copy offset",
        })?,
        usize::try_from(to).map_err(|_| SwitchSwizzleError::ArithmeticOverflow {
            what: "copy offset",
        })?,
        usize::try_from(length).map_err(|_| SwitchSwizzleError::ArithmeticOverflow {
            what: "copy length",
        })?,
    );
    if from > source.len() || length > source.len() - from {
        return Err(SwitchSwizzleError::RangeOutOfBounds {
            step: "copy source",
            offset: from as u64,
            length: length as u64,
            available: source.len(),
        });
    }
    if to > target.len() || length > target.len() - to {
        return Err(SwitchSwizzleError::RangeOutOfBounds {
            step: "copy target",
            offset: to as u64,
            length: length as u64,
            available: target.len(),
        });
    }
    target[to..to + length].copy_from_slice(&source[from..from + length]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        block_dimensions, block_size, corrected_switch_format, texel_height_from_platform_blob,
        unswizzle, SwitchSwizzleError, SwitchUnswizzleInput, BLOCKS_IN_GOB, GOB_X_POSES,
        GOB_Y_POSES, TEXEL_BYTE_SIZE,
    };

    fn blob_with_shift(shift: i32) -> Vec<u8> {
        let mut blob = vec![0u8; 12];
        blob[8..12].copy_from_slice(&shift.to_le_bytes());
        blob
    }

    #[test]
    fn gob_tables_cover_every_slot_once() {
        assert_eq!(GOB_X_POSES.len(), BLOCKS_IN_GOB);
        assert_eq!(GOB_Y_POSES.len(), BLOCKS_IN_GOB);
        let mut slots = [[false; 8]; 4];
        for l in 0..BLOCKS_IN_GOB {
            let (x, y) = (GOB_X_POSES[l] as usize, GOB_Y_POSES[l] as usize);
            assert!(x < 4 && y < 8, "slot {l} is ({x},{y})");
            assert!(!slots[x][y], "slot ({x},{y}) repeats at {l}");
            slots[x][y] = true;
        }
    }

    #[test]
    fn block_tables_match_the_source_literals() {
        assert_eq!(block_dimensions(4 /* RGBA32 */), Some((1, 1)));
        assert_eq!(block_dimensions(10 /* DXT1 */), Some((4, 4)));
        assert_eq!(block_dimensions(-48 /* ASTC_RGB_4x4 */), Some((4, 4)));
        assert_eq!(block_dimensions(-53 /* ASTC_RGB_12x12 */), Some((12, 12)));
        assert_eq!(block_dimensions(71 /* ASTC_HDR_12x12 */), Some((12, 12)));
        // DXT3 has no block dimensions: the source backs out.
        assert_eq!(block_dimensions(11 /* DXT3 */), None);
        assert_eq!(block_dimensions(999), None);
        assert_eq!(block_size(12 /* DXT5 */), Ok(16));
        assert_eq!(block_size(63 /* R8 */), Ok(1));
        assert_eq!(
            block_size(999),
            Err(SwitchSwizzleError::UnsupportedBlockFormat { format: 999 })
        );
        assert_eq!(corrected_switch_format(3 /* RGB24 */), 4);
        assert_eq!(corrected_switch_format(8 /* BGR24 */), 14);
        assert_eq!(corrected_switch_format(4 /* RGBA32 */), 4);
    }

    #[test]
    fn texel_height_reads_the_blob_shift() {
        assert_eq!(
            texel_height_from_platform_blob(&blob_with_shift(0), true),
            Ok(1)
        );
        assert_eq!(
            texel_height_from_platform_blob(&blob_with_shift(2), true),
            Ok(4)
        );
        assert_eq!(
            texel_height_from_platform_blob(&blob_with_shift(2), false),
            Err(SwitchSwizzleError::InvalidTexelHeight { shift: 0x0200_0000 })
        );
        assert_eq!(
            texel_height_from_platform_blob(&[0u8; 11], true),
            Err(SwitchSwizzleError::PlatformBlobTooShort { byte_count: 11 })
        );
        assert_eq!(
            texel_height_from_platform_blob(&blob_with_shift(-1), true),
            Err(SwitchSwizzleError::InvalidTexelHeight { shift: -1 })
        );
        assert_eq!(
            texel_height_from_platform_blob(&blob_with_shift(31), true),
            Err(SwitchSwizzleError::InvalidTexelHeight { shift: 31 })
        );
    }

    /// A 16x8 RGBA32 texture with texel height 1 fills exactly one GOB: stream
    /// position `l` lands in texel slot `(GOB_X_POSES[l], GOB_Y_POSES[l])`.
    fn one_gob_input() -> (SwitchUnswizzleInput<'static>, Vec<u8>) {
        // Leaked so the input can borrow it; tests only.
        let data: &'static mut [u8] = Box::leak(Box::new([0u8; 512]));
        for l in 0..32 {
            data[l * TEXEL_BYTE_SIZE..(l + 1) * TEXEL_BYTE_SIZE].fill(l as u8);
        }
        let blob: &'static mut [u8] = Box::leak(Box::new([0u8; 12]));
        let input = SwitchUnswizzleInput {
            image_count: 1,
            format: 4,
            width: 16,
            height: 8,
            platform_blob: blob,
            platform_blob_little_endian: true,
            data,
        };
        let mut expected = vec![0u8; 512];
        for l in 0..32 {
            let slot = (GOB_Y_POSES[l] as usize * 4 + GOB_X_POSES[l] as usize) * TEXEL_BYTE_SIZE;
            expected[slot..slot + TEXEL_BYTE_SIZE].fill(l as u8);
        }
        (input, expected)
    }

    #[test]
    fn one_gob_unswizzle_matches_the_pose_tables() {
        let (input, expected) = one_gob_input();
        assert_eq!(unswizzle(&input).unwrap(), expected);
        // Spot lanes: stream 1 lands in texel (0,1), stream 16 in (2,0).
        let output = unswizzle(&input).unwrap();
        assert_eq!(&output[64..80], &[1u8; 16]);
        assert_eq!(&output[32..48], &[16u8; 16]);
    }

    #[test]
    fn passthrough_cases_return_the_bytes_untouched() {
        let (mut input, _) = one_gob_input();
        // Multi-image textures back out.
        input.image_count = 2;
        assert_eq!(
            unswizzle(&input).unwrap(),
            input.data.to_vec(),
            "image count"
        );
        // Unknown block layouts (like DXT3) back out.
        input.image_count = 1;
        input.format = 11;
        assert_eq!(
            unswizzle(&input).unwrap(),
            input.data.to_vec(),
            "unknown layout"
        );
    }

    #[test]
    fn corrected_rgb24_converts_rgba_to_rgb() {
        let (mut input, _) = one_gob_input();
        input.format = 3;
        // Give stream 0 (texel (0,0), the only texel the crop keeps) distinctive bytes.
        let data: &'static mut [u8] = Box::leak(Box::new([0u8; 512]));
        for (index, byte) in data[..TEXEL_BYTE_SIZE].iter_mut().enumerate() {
            *byte = 100 + index as u8;
        }
        input.data = data;
        let output = unswizzle(&input).unwrap();
        // Row 0 keeps the first three bytes of each 4-byte block.
        assert_eq!(
            &output[..12],
            &[100, 101, 102, 104, 105, 106, 108, 109, 110, 112, 113, 114]
        );
        // The RGB tail is cleared past the 12 content bytes.
        assert!(output[12..].iter().all(|byte| *byte == 0));
        assert_eq!(output.len(), 512);
    }

    #[test]
    fn short_buffers_are_explicit_errors() {
        let (mut input, _) = one_gob_input();
        let short: &'static [u8] = Box::leak(Box::new([0u8; 16]));
        input.data = short;
        assert!(matches!(
            unswizzle(&input),
            Err(SwitchSwizzleError::RangeOutOfBounds { .. })
        ));
    }

    /// Cross-check against the independent C# model committed in
    /// `switch_swizzle_fixture.txt`: every vector must reproduce byte-for-byte.
    #[test]
    fn cross_model_vectors_reproduce_byte_for_byte() {
        let fixture = include_str!("switch_swizzle_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts.len(), 7, "{line:?}");
            assert_eq!(parts[0], "VEC");
            let format: i32 = parts[1].parse().unwrap();
            let width: u32 = parts[2].parse().unwrap();
            let height: u32 = parts[3].parse().unwrap();
            let shift: i32 = parts[4].parse().unwrap();
            let data = parse_hex(parts[5]);
            let expected = parse_hex(parts[6]);
            let mut blob = vec![0u8; 12];
            blob[8..12].copy_from_slice(&shift.to_le_bytes());
            let input = SwitchUnswizzleInput {
                image_count: 1,
                format,
                width,
                height,
                platform_blob: &blob,
                platform_blob_little_endian: true,
                data: &data,
            };
            assert_eq!(unswizzle(&input).unwrap(), expected, "format {format}");
            rows += 1;
        }
        assert_eq!(rows, 8, "all fixture vectors are asserted");
    }

    fn parse_hex(text: &str) -> Vec<u8> {
        assert!(text.len().is_multiple_of(2), "even hex length");
        let mut bytes = Vec::with_capacity(text.len() / 2);
        let digits = text.as_bytes();
        for pair in digits.chunks_exact(2) {
            let high = hex_value(pair[0]);
            let low = hex_value(pair[1]);
            bytes.push(high << 4 | low);
        }
        bytes
    }

    fn hex_value(digit: u8) -> u8 {
        match digit {
            b'0'..=b'9' => digit - b'0',
            b'A'..=b'F' => digit - b'A' + 10,
            b'a'..=b'f' => digit - b'a' + 10,
            _ => panic!("non-hex digit {digit}"),
        }
    }
}
