//! Block and packed pixel decoders ported from the `AssetRipper.TextureDecoder` dependency used by
//! `AssetRipper.Export.Modules.Textures.TextureConverter`.
//!
//! The baseline's `TextureConverter` dispatches `TextureFormat` values to this package. The Rust
//! port covers the DXT family and the packed byte formats first; each routine writes RGBA8 into a
//! caller-provided buffer and returns the number of input bytes consumed, matching the reference.
//!
//! Two expansion quirks are preserved deliberately and confirmed by the isolated probe in
//! `Temp/opencode/texdecprobe`:
//!
//! - The DXT color endpoint expands a 5/6-bit channel with bit replication
//!   (`(value << n) | (value >> (5 - n))`), so `0xF800` becomes `255`.
//! - The standalone `RGB565` converter expands with a bare shift (`value << n`), so `0xF800`
//!   becomes `248`. The two paths disagree on purpose.
//! - In the three-color DXT mode (`color0 <= color1`) the reference emits opaque black
//!   `(0, 0, 0, 255)` for index 3 rather than transparent black.
//!
//! Differential fixture: `texture_decode_fixture.txt` (probe `Temp/opencode/texdecprobe`).

/// A texture decode failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextureDecodeError {
    /// The input ended before a whole block or pixel.
    InputTooShort {
        /// Required input bytes.
        required: usize,
        /// Provided input bytes.
        actual: usize,
    },
    /// The output buffer cannot hold the decoded RGBA image.
    OutputTooSmall {
        /// Required output bytes.
        required: usize,
        /// Provided output bytes.
        actual: usize,
    },
    /// The ASTC block footprint is outside the supported `4..=12` range.
    InvalidBlockDimension {
        /// Requested block width.
        block_width: usize,
        /// Requested block height.
        block_height: usize,
    },
}

impl core::fmt::Display for TextureDecodeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InputTooShort { required, actual } => {
                write!(
                    formatter,
                    "texture input has {actual} bytes; needs {required}"
                )
            }
            Self::OutputTooSmall { required, actual } => {
                write!(
                    formatter,
                    "texture output has {actual} bytes; needs {required}"
                )
            }
            Self::InvalidBlockDimension {
                block_width,
                block_height,
            } => {
                write!(
                    formatter,
                    "ASTC block dimensions {block_width}x{block_height} are outside the supported 4..=12 range"
                )
            }
        }
    }
}

impl std::error::Error for TextureDecodeError {}

pub(crate) fn require_input(input: &[u8], required: usize) -> Result<(), TextureDecodeError> {
    if input.len() < required {
        return Err(TextureDecodeError::InputTooShort {
            required,
            actual: input.len(),
        });
    }
    Ok(())
}

pub(crate) fn require_output(
    output: &[u8],
    width: usize,
    height: usize,
) -> Result<usize, TextureDecodeError> {
    let required = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(TextureDecodeError::OutputTooSmall {
            required: usize::MAX,
            actual: output.len(),
        })?;
    if output.len() < required {
        return Err(TextureDecodeError::OutputTooSmall {
            required,
            actual: output.len(),
        });
    }
    Ok(required)
}

/// Expands a `RGB565` word the way the DXT endpoint decoder does: bit replication.
pub fn dxt_rgb565_to_rgba(value: u16) -> [u8; 4] {
    let r5 = ((value >> 11) & 0x1F) as u8;
    let g6 = ((value >> 5) & 0x3F) as u8;
    let b5 = (value & 0x1F) as u8;
    [
        (r5 << 3) | (r5 >> 2),
        (g6 << 2) | (g6 >> 4),
        (b5 << 3) | (b5 >> 2),
        0xFF,
    ]
}

/// Expands a `RGB565` word the way the standalone converter does: a bare shift.
pub fn rgb565_to_rgba(value: u16) -> [u8; 4] {
    let r5 = ((value >> 11) & 0x1F) as u8;
    let g6 = ((value >> 5) & 0x3F) as u8;
    let b5 = (value & 0x1F) as u8;
    [r5 << 3, g6 << 2, b5 << 3, 0xFF]
}

fn mix(a: [u8; 4], b: [u8; 4], weight_a: u32, weight_b: u32) -> [u8; 4] {
    let mut out = [0u8; 4];
    let total = weight_a + weight_b;
    for (index, value) in out.iter_mut().enumerate() {
        *value = ((u32::from(a[index]) * weight_a + u32::from(b[index]) * weight_b) / total) as u8;
    }
    out
}

fn write_color_block(
    block: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
    bx: usize,
    by: usize,
) {
    let color0 = u16::from_le_bytes([block[0], block[1]]);
    let color1 = u16::from_le_bytes([block[2], block[3]]);
    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    let rgb0 = dxt_rgb565_to_rgba(color0);
    let rgb1 = dxt_rgb565_to_rgba(color1);
    let colors = if color0 > color1 {
        [rgb0, rgb1, mix(rgb0, rgb1, 2, 1), mix(rgb0, rgb1, 1, 2)]
    } else {
        [rgb0, rgb1, mix(rgb0, rgb1, 1, 1), [0, 0, 0, 0xFF]]
    };
    for py in 0..4 {
        for px in 0..4 {
            let x = bx * 4 + px;
            let y = by * 4 + py;
            if x >= width || y >= height {
                continue;
            }
            let index = ((indices >> (2 * (py * 4 + px))) & 3) as usize;
            let offset = (y * width + x) * 4;
            output[offset..offset + 4].copy_from_slice(&colors[index]);
        }
    }
}

fn block_grid(width: usize, height: usize) -> (usize, usize) {
    (width.div_ceil(4), height.div_ceil(4))
}

/// `DxtDecoder.DecompressDXT1`.
pub fn decode_dxt1(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    require_output(output, width, height)?;
    let (blocks_x, blocks_y) = block_grid(width, height);
    let required = blocks_x.saturating_mul(blocks_y).saturating_mul(8);
    require_input(input, required)?;
    let mut offset = 0;
    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            write_color_block(&input[offset..offset + 8], output, width, height, bx, by);
            offset += 8;
        }
    }
    Ok(offset)
}

/// `DxtDecoder.DecompressDXT3`.
pub fn decode_dxt3(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    require_output(output, width, height)?;
    let (blocks_x, blocks_y) = block_grid(width, height);
    let required = blocks_x.saturating_mul(blocks_y).saturating_mul(16);
    require_input(input, required)?;
    let mut offset = 0;
    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let block = &input[offset..offset + 16];
            write_color_block(&block[8..16], output, width, height, bx, by);
            for py in 0..4 {
                for px in 0..4 {
                    let x = bx * 4 + px;
                    let y = by * 4 + py;
                    if x >= width || y >= height {
                        continue;
                    }
                    let index = py * 4 + px;
                    let nibble = (block[index / 2] >> (4 * (index % 2))) & 0x0F;
                    let pixel = (y * width + x) * 4;
                    output[pixel + 3] = nibble * 17;
                }
            }
            offset += 16;
        }
    }
    Ok(offset)
}

pub(crate) fn dxt5_alpha_table(alpha0: u8, alpha1: u8) -> [u8; 8] {
    let a0 = u32::from(alpha0);
    let a1 = u32::from(alpha1);
    if alpha0 > alpha1 {
        [
            alpha0,
            alpha1,
            ((6 * a0 + a1) / 7) as u8,
            ((5 * a0 + 2 * a1) / 7) as u8,
            ((4 * a0 + 3 * a1) / 7) as u8,
            ((3 * a0 + 4 * a1) / 7) as u8,
            ((2 * a0 + 5 * a1) / 7) as u8,
            ((a0 + 6 * a1) / 7) as u8,
        ]
    } else {
        [
            alpha0,
            alpha1,
            ((4 * a0 + a1) / 5) as u8,
            ((3 * a0 + 2 * a1) / 5) as u8,
            ((2 * a0 + 3 * a1) / 5) as u8,
            ((a0 + 4 * a1) / 5) as u8,
            0,
            255,
        ]
    }
}

/// `DxtDecoder.DecompressDXT5`.
pub fn decode_dxt5(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    require_output(output, width, height)?;
    let (blocks_x, blocks_y) = block_grid(width, height);
    let required = blocks_x.saturating_mul(blocks_y).saturating_mul(16);
    require_input(input, required)?;
    let mut offset = 0;
    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let block = &input[offset..offset + 16];
            write_color_block(&block[8..16], output, width, height, bx, by);
            let alphas = dxt5_alpha_table(block[0], block[1]);
            let mut bits = 0u64;
            for (index, byte) in block[2..8].iter().enumerate() {
                bits |= u64::from(*byte) << (8 * index);
            }
            for py in 0..4 {
                for px in 0..4 {
                    let x = bx * 4 + px;
                    let y = by * 4 + py;
                    if x >= width || y >= height {
                        continue;
                    }
                    let index = py * 4 + px;
                    let alpha_index = ((bits >> (3 * index)) & 7) as usize;
                    let pixel = (y * width + x) * 4;
                    output[pixel + 3] = alphas[alpha_index];
                }
            }
            offset += 16;
        }
    }
    Ok(offset)
}

fn read_bc4_alpha_block(block: &[u8]) -> ([u8; 8], u64) {
    let table = bc4_alpha_table(block[0], block[1]);
    let mut bits = 0u64;
    for (index, byte) in block[2..8].iter().enumerate() {
        bits |= u64::from(*byte) << (8 * index);
    }
    (table, bits)
}

/// The `BcHelpers.SmoothAlphaBlock` table: like the DXT5 alpha table but with `+1` rounding
/// before the division, so the `Bc` decoders disagree with the `Dxt` decoder on some endpoints.
fn bc4_alpha_table(alpha0: u8, alpha1: u8) -> [u8; 8] {
    let a0 = u32::from(alpha0);
    let a1 = u32::from(alpha1);
    if alpha0 > alpha1 {
        [
            alpha0,
            alpha1,
            ((6 * a0 + a1 + 1) / 7) as u8,
            ((5 * a0 + 2 * a1 + 1) / 7) as u8,
            ((4 * a0 + 3 * a1 + 1) / 7) as u8,
            ((3 * a0 + 4 * a1 + 1) / 7) as u8,
            ((2 * a0 + 5 * a1 + 1) / 7) as u8,
            ((a0 + 6 * a1 + 1) / 7) as u8,
        ]
    } else {
        [
            alpha0,
            alpha1,
            ((4 * a0 + a1 + 1) / 5) as u8,
            ((3 * a0 + 2 * a1 + 1) / 5) as u8,
            ((2 * a0 + 3 * a1 + 1) / 5) as u8,
            ((a0 + 4 * a1 + 1) / 5) as u8,
            0,
            255,
        ]
    }
}

/// `Bc4.Decompress`: one interpolated channel in the red slot, with `G = B = 0`, `A = 255`.
pub fn decode_bc4(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    require_output(output, width, height)?;
    let (blocks_x, blocks_y) = block_grid(width, height);
    let required = blocks_x.saturating_mul(blocks_y).saturating_mul(8);
    require_input(input, required)?;
    let mut offset = 0;
    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let (table, bits) = read_bc4_alpha_block(&input[offset..offset + 8]);
            for py in 0..4 {
                for px in 0..4 {
                    let x = bx * 4 + px;
                    let y = by * 4 + py;
                    if x >= width || y >= height {
                        continue;
                    }
                    let index = py * 4 + px;
                    let value = table[((bits >> (3 * index)) & 7) as usize];
                    let pixel = (y * width + x) * 4;
                    output[pixel..pixel + 4].copy_from_slice(&[value, 0, 0, 0xFF]);
                }
            }
            offset += 8;
        }
    }
    Ok(offset)
}

/// `Bc5.Decompress`: two interpolated channels (red then green), with `B = 0`, `A = 255`.
pub fn decode_bc5(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    require_output(output, width, height)?;
    let (blocks_x, blocks_y) = block_grid(width, height);
    let required = blocks_x.saturating_mul(blocks_y).saturating_mul(16);
    require_input(input, required)?;
    let mut offset = 0;
    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let block = &input[offset..offset + 16];
            let (red_table, red_bits) = read_bc4_alpha_block(&block[0..8]);
            let (green_table, green_bits) = read_bc4_alpha_block(&block[8..16]);
            for py in 0..4 {
                for px in 0..4 {
                    let x = bx * 4 + px;
                    let y = by * 4 + py;
                    if x >= width || y >= height {
                        continue;
                    }
                    let index = py * 4 + px;
                    let red = red_table[((red_bits >> (3 * index)) & 7) as usize];
                    let green = green_table[((green_bits >> (3 * index)) & 7) as usize];
                    let pixel = (y * width + x) * 4;
                    output[pixel..pixel + 4].copy_from_slice(&[red, green, 0, 0xFF]);
                }
            }
            offset += 16;
        }
    }
    Ok(offset)
}

fn decode_packed<const N: usize>(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
    mut convert: impl FnMut(&[u8]) -> [u8; 4],
) -> Result<usize, TextureDecodeError> {
    require_output(output, width, height)?;
    let pixels = width.saturating_mul(height);
    let required = pixels.saturating_mul(N);
    require_input(input, required)?;
    for index in 0..pixels {
        let source = &input[index * N..index * N + N];
        let pixel = convert(source);
        output[index * 4..index * 4 + 4].copy_from_slice(&pixel);
    }
    Ok(required)
}

/// `RgbConverter.Convert<ColorRGBA<byte>, byte, ...>` for `TextureFormat.RGBA32`.
pub fn decode_rgba32(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| [p[0], p[1], p[2], p[3]])
}

/// `RgbConverter` for `TextureFormat.RGB24`.
pub fn decode_rgb24(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<3>(input, width, height, output, |p| [p[0], p[1], p[2], 0xFF])
}

/// `RgbConverter` for `TextureFormat.BGRA32`.
pub fn decode_bgra32(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| [p[2], p[1], p[0], p[3]])
}

/// `RgbConverter` for `TextureFormat.ARGB32`.
pub fn decode_argb32(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| [p[1], p[2], p[3], p[0]])
}

/// `RgbConverter` for `TextureFormat.BGR24` (`ColorBGR<byte>` layout `b,g,r`).
pub fn decode_bgr24(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<3>(input, width, height, output, |p| [p[2], p[1], p[0], 0xFF])
}

/// `RgbConverter` for `TextureFormat.ARGBFloat` (`ColorARGB<float>` layout `a,r,g,b`).
pub fn decode_argb_float(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<16>(input, width, height, output, |p| {
        let a = f32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        let r = f32::from_le_bytes([p[4], p[5], p[6], p[7]]);
        let g = f32::from_le_bytes([p[8], p[9], p[10], p[11]]);
        let b = f32::from_le_bytes([p[12], p[13], p[14], p[15]]);
        [
            float_to_byte(r),
            float_to_byte(g),
            float_to_byte(b),
            float_to_byte(a),
        ]
    })
}

/// `RgbConverter` for `TextureFormat.Alpha8`.
pub fn decode_alpha8(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<1>(input, width, height, output, |p| [0, 0, 0, p[0]])
}

/// `RgbConverter` for `TextureFormat.R8`.
pub fn decode_r8(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<1>(input, width, height, output, |p| [p[0], 0, 0, 0xFF])
}

/// `RgbConverter` for `TextureFormat.RG16`.
pub fn decode_rg16(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<2>(input, width, height, output, |p| [p[0], p[1], 0, 0xFF])
}

/// `RgbConverter` for `TextureFormat.RGB565`.
pub fn decode_rgb565(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<2>(input, width, height, output, |p| {
        rgb565_to_rgba(u16::from_le_bytes([p[0], p[1]]))
    })
}

/// `RgbConverter` for `TextureFormat.ARGB4444`.
pub fn decode_argb4444(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<2>(input, width, height, output, |p| {
        let value = u16::from_le_bytes([p[0], p[1]]);
        [
            (((value >> 8) & 0x0F) as u8) * 17,
            (((value >> 4) & 0x0F) as u8) * 17,
            ((value & 0x0F) as u8) * 17,
            (((value >> 12) & 0x0F) as u8) * 17,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGBA4444`.
pub fn decode_rgba4444(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<2>(input, width, height, output, |p| {
        let value = u16::from_le_bytes([p[0], p[1]]);
        [
            (((value >> 12) & 0x0F) as u8) * 17,
            (((value >> 8) & 0x0F) as u8) * 17,
            (((value >> 4) & 0x0F) as u8) * 17,
            ((value & 0x0F) as u8) * 17,
        ]
    })
}

pub(crate) fn ushort_to_byte(value: u16) -> u8 {
    ((u32::from(value) * 255 + 32895) >> 16) as u8
}

fn short_to_byte(value: i16) -> u8 {
    ushort_to_byte((value as u16) ^ 0x8000)
}

fn sbyte_to_byte(value: i8) -> u8 {
    (value as u8) ^ 0x80
}

pub(crate) fn float_to_byte(value: f32) -> u8 {
    let x = value * 255.0;
    if x > 255.0 {
        255
    } else if x > 0.0 {
        x as u8
    } else {
        0
    }
}

fn double_to_byte(value: f64) -> u8 {
    let x = value * 255.0;
    if x > 255.0 {
        255
    } else if x > 0.0 {
        x as u8
    } else {
        0
    }
}

/// Decodes an IEEE 754 binary16 bit pattern to `f32`.
pub(crate) fn half_bits_to_f32(bits: u16) -> f32 {
    let sign = (bits >> 15) & 1;
    let exponent = (bits >> 10) & 0x1F;
    let mantissa = bits & 0x03FF;
    let value = if exponent == 0 {
        (f32::from(mantissa)) * 2.0f32.powi(-24)
    } else if exponent == 31 {
        if mantissa == 0 {
            f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        (1.0 + f32::from(mantissa) / 1024.0) * 2.0f32.powi(i32::from(exponent) - 15)
    };
    if sign == 1 {
        -value
    } else {
        value
    }
}

/// `RgbConverter` for `TextureFormat.R16`.
pub fn decode_r16(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<2>(input, width, height, output, |p| {
        [ushort_to_byte(u16::from_le_bytes([p[0], p[1]])), 0, 0, 0xFF]
    })
}

/// `RgbConverter` for `TextureFormat.RG32`.
pub fn decode_rg32(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| {
        [
            ushort_to_byte(u16::from_le_bytes([p[0], p[1]])),
            ushort_to_byte(u16::from_le_bytes([p[2], p[3]])),
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGB48`.
pub fn decode_rgb48(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<6>(input, width, height, output, |p| {
        [
            ushort_to_byte(u16::from_le_bytes([p[0], p[1]])),
            ushort_to_byte(u16::from_le_bytes([p[2], p[3]])),
            ushort_to_byte(u16::from_le_bytes([p[4], p[5]])),
            0xFF,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGBA64`.
pub fn decode_rgba64(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<8>(input, width, height, output, |p| {
        [
            ushort_to_byte(u16::from_le_bytes([p[0], p[1]])),
            ushort_to_byte(u16::from_le_bytes([p[2], p[3]])),
            ushort_to_byte(u16::from_le_bytes([p[4], p[5]])),
            ushort_to_byte(u16::from_le_bytes([p[6], p[7]])),
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RHalf`.
pub fn decode_r_half(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<2>(input, width, height, output, |p| {
        [
            float_to_byte(half_bits_to_f32(u16::from_le_bytes([p[0], p[1]]))),
            0,
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGHalf`.
pub fn decode_rg_half(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| {
        [
            float_to_byte(half_bits_to_f32(u16::from_le_bytes([p[0], p[1]]))),
            float_to_byte(half_bits_to_f32(u16::from_le_bytes([p[2], p[3]]))),
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGBAHalf`.
pub fn decode_rgba_half(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<8>(input, width, height, output, |p| {
        [
            float_to_byte(half_bits_to_f32(u16::from_le_bytes([p[0], p[1]]))),
            float_to_byte(half_bits_to_f32(u16::from_le_bytes([p[2], p[3]]))),
            float_to_byte(half_bits_to_f32(u16::from_le_bytes([p[4], p[5]]))),
            float_to_byte(half_bits_to_f32(u16::from_le_bytes([p[6], p[7]]))),
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RFloat`.
pub fn decode_r_float(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| {
        [
            float_to_byte(f32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            0,
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGFloat`.
pub fn decode_rg_float(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<8>(input, width, height, output, |p| {
        [
            float_to_byte(f32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            float_to_byte(f32::from_le_bytes([p[4], p[5], p[6], p[7]])),
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGBAFloat`.
pub fn decode_rgba_float(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<16>(input, width, height, output, |p| {
        [
            float_to_byte(f32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            float_to_byte(f32::from_le_bytes([p[4], p[5], p[6], p[7]])),
            float_to_byte(f32::from_le_bytes([p[8], p[9], p[10], p[11]])),
            float_to_byte(f32::from_le_bytes([p[12], p[13], p[14], p[15]])),
        ]
    })
}

pub(crate) fn uint32_to_byte(value: u32) -> u8 {
    // `NumericConversion.ConvertUInt32<byte>`: interpolate then round half away from zero.
    let interpolated = f64::from(value) / f64::from(u32::MAX);
    let exact = interpolated * 255.0;
    exact.round() as u8
}

pub(crate) fn int32_to_byte(value: i32) -> u8 {
    // `ChangeSign(int)` flips the sign bit before the unsigned conversion.
    uint32_to_byte((value as u32) ^ 0x8000_0000)
}

/// `RgbConverter` for `GraphicsFormat.R32_UInt` (`ColorR<uint>`).
pub fn decode_r32_unsigned(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| {
        [
            uint32_to_byte(u32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            0,
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.R32G32_UInt` (`ColorRG<uint>`).
pub fn decode_r32g32_unsigned(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<8>(input, width, height, output, |p| {
        [
            uint32_to_byte(u32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            uint32_to_byte(u32::from_le_bytes([p[4], p[5], p[6], p[7]])),
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.R32G32B32_UInt` (`ColorRGB<uint>`).
pub fn decode_r32g32b32_unsigned(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<12>(input, width, height, output, |p| {
        [
            uint32_to_byte(u32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            uint32_to_byte(u32::from_le_bytes([p[4], p[5], p[6], p[7]])),
            uint32_to_byte(u32::from_le_bytes([p[8], p[9], p[10], p[11]])),
            0xFF,
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.R32G32B32A32_UInt` (`ColorRGBA<uint>`).
pub fn decode_r32g32b32a32_unsigned(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<16>(input, width, height, output, |p| {
        [
            uint32_to_byte(u32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            uint32_to_byte(u32::from_le_bytes([p[4], p[5], p[6], p[7]])),
            uint32_to_byte(u32::from_le_bytes([p[8], p[9], p[10], p[11]])),
            uint32_to_byte(u32::from_le_bytes([p[12], p[13], p[14], p[15]])),
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.R32_SInt` (`ColorR<int>`).
pub fn decode_r32_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| {
        [
            int32_to_byte(i32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            0,
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.R32G32_SInt` (`ColorRG<int>`).
pub fn decode_r32g32_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<8>(input, width, height, output, |p| {
        [
            int32_to_byte(i32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            int32_to_byte(i32::from_le_bytes([p[4], p[5], p[6], p[7]])),
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.R32G32B32_SInt` (`ColorRGB<int>`).
pub fn decode_r32g32b32_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<12>(input, width, height, output, |p| {
        [
            int32_to_byte(i32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            int32_to_byte(i32::from_le_bytes([p[4], p[5], p[6], p[7]])),
            int32_to_byte(i32::from_le_bytes([p[8], p[9], p[10], p[11]])),
            0xFF,
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.R32G32B32A32_SInt` (`ColorRGBA<int>`).
pub fn decode_r32g32b32a32_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<16>(input, width, height, output, |p| {
        [
            int32_to_byte(i32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            int32_to_byte(i32::from_le_bytes([p[4], p[5], p[6], p[7]])),
            int32_to_byte(i32::from_le_bytes([p[8], p[9], p[10], p[11]])),
            int32_to_byte(i32::from_le_bytes([p[12], p[13], p[14], p[15]])),
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.R16G16B16_SFloat` (`ColorRGB<Half>`).
pub fn decode_rgb_half(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<6>(input, width, height, output, |p| {
        [
            float_to_byte(half_bits_to_f32(u16::from_le_bytes([p[0], p[1]]))),
            float_to_byte(half_bits_to_f32(u16::from_le_bytes([p[2], p[3]]))),
            float_to_byte(half_bits_to_f32(u16::from_le_bytes([p[4], p[5]]))),
            0xFF,
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.R32G32B32_SFloat` (`ColorRGB<float>`).
pub fn decode_rgb_float(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<12>(input, width, height, output, |p| {
        [
            float_to_byte(f32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            float_to_byte(f32::from_le_bytes([p[4], p[5], p[6], p[7]])),
            float_to_byte(f32::from_le_bytes([p[8], p[9], p[10], p[11]])),
            0xFF,
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.B8G8R8_SNorm` / `B8G8R8_SInt` (`ColorBGR<sbyte>`).
pub fn decode_bgr24_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<3>(input, width, height, output, |p| {
        [
            sbyte_to_byte(p[2] as i8),
            sbyte_to_byte(p[1] as i8),
            sbyte_to_byte(p[0] as i8),
            0xFF,
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.B8G8R8A8_SNorm` / `B8G8R8A8_SInt` (`ColorBGRA<sbyte>`).
pub fn decode_bgra32_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| {
        [
            sbyte_to_byte(p[2] as i8),
            sbyte_to_byte(p[1] as i8),
            sbyte_to_byte(p[0] as i8),
            sbyte_to_byte(p[3] as i8),
        ]
    })
}

/// `RgbConverter` for `GraphicsFormat.A16_UNorm` (`ColorA<ushort>`).
pub fn decode_a16(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<2>(input, width, height, output, |p| {
        [0, 0, 0, ushort_to_byte(u16::from_le_bytes([p[0], p[1]]))]
    })
}

/// `RgbConverter` for `TextureFormat.R8_SIGNED`.
pub fn decode_r8_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<1>(input, width, height, output, |p| {
        [sbyte_to_byte(p[0] as i8), 0, 0, 0xFF]
    })
}

/// `RgbConverter` for `TextureFormat.RG16_SIGNED`.
pub fn decode_rg16_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<2>(input, width, height, output, |p| {
        [
            sbyte_to_byte(p[0] as i8),
            sbyte_to_byte(p[1] as i8),
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGB24_SIGNED`.
pub fn decode_rgb24_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<3>(input, width, height, output, |p| {
        [
            sbyte_to_byte(p[0] as i8),
            sbyte_to_byte(p[1] as i8),
            sbyte_to_byte(p[2] as i8),
            0xFF,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGBA32_SIGNED`.
pub fn decode_rgba32_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| {
        [
            sbyte_to_byte(p[0] as i8),
            sbyte_to_byte(p[1] as i8),
            sbyte_to_byte(p[2] as i8),
            sbyte_to_byte(p[3] as i8),
        ]
    })
}

/// `RgbConverter` for `TextureFormat.R16_SIGNED`.
pub fn decode_r16_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<2>(input, width, height, output, |p| {
        [short_to_byte(i16::from_le_bytes([p[0], p[1]])), 0, 0, 0xFF]
    })
}

/// `RgbConverter` for `TextureFormat.RG32_SIGNED`.
pub fn decode_rg32_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| {
        [
            short_to_byte(i16::from_le_bytes([p[0], p[1]])),
            short_to_byte(i16::from_le_bytes([p[2], p[3]])),
            0,
            0xFF,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGB48_SIGNED`.
pub fn decode_rgb48_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<6>(input, width, height, output, |p| {
        [
            short_to_byte(i16::from_le_bytes([p[0], p[1]])),
            short_to_byte(i16::from_le_bytes([p[2], p[3]])),
            short_to_byte(i16::from_le_bytes([p[4], p[5]])),
            0xFF,
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGBA64_SIGNED`.
pub fn decode_rgba64_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<8>(input, width, height, output, |p| {
        [
            short_to_byte(i16::from_le_bytes([p[0], p[1]])),
            short_to_byte(i16::from_le_bytes([p[2], p[3]])),
            short_to_byte(i16::from_le_bytes([p[4], p[5]])),
            short_to_byte(i16::from_le_bytes([p[6], p[7]])),
        ]
    })
}

/// `RgbConverter` for `TextureFormat.RGB9e5Float`.
pub fn decode_rgb9e5_float(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_packed::<4>(input, width, height, output, |p| {
        let bits = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        let exponent = (bits >> 27) as i32 - 24;
        let scale = 2.0f64.powi(exponent);
        let red = f64::from(bits & 0x1FF) * scale;
        let green = f64::from((bits >> 9) & 0x1FF) * scale;
        let blue = f64::from((bits >> 18) & 0x1FF) * scale;
        [
            double_to_byte(red),
            double_to_byte(green),
            double_to_byte(blue),
            0xFF,
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks_exact(2)
            .map(|chunk| {
                let text = std::str::from_utf8(chunk).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }

    fn decode(kind: &str, input: &[u8], width: usize, height: usize) -> (usize, Vec<u8>) {
        let mut output = vec![0u8; width * height * 4];
        let consumed = match kind {
            "DXT1" => decode_dxt1(input, width, height, &mut output),
            "DXT3" => decode_dxt3(input, width, height, &mut output),
            "DXT5" => decode_dxt5(input, width, height, &mut output),
            "BC4" => decode_bc4(input, width, height, &mut output),
            "BC4T" => decode_bc4(input, width, height, &mut output),
            "BC5" => decode_bc5(input, width, height, &mut output),
            "RGBA32" => decode_rgba32(input, width, height, &mut output),
            "RGB24" => decode_rgb24(input, width, height, &mut output),
            "BGRA32" => decode_bgra32(input, width, height, &mut output),
            "ARGB32" => decode_argb32(input, width, height, &mut output),
            "Alpha8" => decode_alpha8(input, width, height, &mut output),
            "RGB565" => decode_rgb565(input, width, height, &mut output),
            "ARGB4444" => decode_argb4444(input, width, height, &mut output),
            "RGBA4444" => decode_rgba4444(input, width, height, &mut output),
            "R8" => decode_r8(input, width, height, &mut output),
            "RG16" => decode_rg16(input, width, height, &mut output),
            other => panic!("unknown decode kind {other}"),
        }
        .unwrap();
        (consumed, output)
    }

    /// Byte-for-byte differential against `AssetRipper.TextureDecoder` (probe
    /// `Temp/opencode/texdecprobe`).
    #[test]
    fn texture_decoders_match_the_reference_runtime() {
        let fixture = include_str!("texture_decode_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            let width: usize = parts[1].parse().unwrap();
            let height: usize = parts[2].parse().unwrap();
            let input = parse_hex(parts[3]);
            let result: Vec<&str> = parts[4].split(':').collect();
            let expected_consumed: usize = result[0].parse().unwrap();
            let expected_output = parse_hex(result[1]);
            let (consumed, output) = decode(parts[0], &input, width, height);
            assert_eq!(consumed, expected_consumed, "consumed {line}");
            assert_eq!(output, expected_output, "output {line}");
            rows += 1;
        }
        assert_eq!(rows, 33);
    }

    fn decode_wide(kind: &str, input: &[u8], width: usize, height: usize) -> (usize, Vec<u8>) {
        let mut output = vec![0u8; width * height * 4];
        let consumed = match kind {
            "R16" => decode_r16(input, width, height, &mut output),
            "RG32" => decode_rg32(input, width, height, &mut output),
            "RGB48" => decode_rgb48(input, width, height, &mut output),
            "RGBA64" => decode_rgba64(input, width, height, &mut output),
            "RHalf" => decode_r_half(input, width, height, &mut output),
            "RGHalf" => decode_rg_half(input, width, height, &mut output),
            "RGBAHalf" => decode_rgba_half(input, width, height, &mut output),
            "RFloat" => decode_r_float(input, width, height, &mut output),
            "RGFloat" => decode_rg_float(input, width, height, &mut output),
            "RGBAFloat" => decode_rgba_float(input, width, height, &mut output),
            "R8_SIGNED" => decode_r8_signed(input, width, height, &mut output),
            "RG16_SIGNED" => decode_rg16_signed(input, width, height, &mut output),
            "RGB24_SIGNED" => decode_rgb24_signed(input, width, height, &mut output),
            "RGBA32_SIGNED" => decode_rgba32_signed(input, width, height, &mut output),
            "R16_SIGNED" => decode_r16_signed(input, width, height, &mut output),
            "RG32_SIGNED" => decode_rg32_signed(input, width, height, &mut output),
            "RGB48_SIGNED" => decode_rgb48_signed(input, width, height, &mut output),
            "RGBA64_SIGNED" => decode_rgba64_signed(input, width, height, &mut output),
            "RGB9e5Float" => decode_rgb9e5_float(input, width, height, &mut output),
            "BGR24" => decode_bgr24(input, width, height, &mut output),
            "ARGBFloat" => decode_argb_float(input, width, height, &mut output),
            other => panic!("unknown wide format {other}"),
        }
        .unwrap();
        (consumed, output)
    }

    /// Byte-for-byte differential against `AssetRipper.TextureDecoder`'s `RgbConverter` (probe
    /// `Temp/opencode/texprobe2`) for the 16-bit, half, float, signed, and RGB9e5 formats.
    #[test]
    fn wide_formats_match_the_reference_runtime() {
        let fixture = include_str!("wide_format_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            let width: usize = parts[1].parse().unwrap();
            let height: usize = parts[2].parse().unwrap();
            let input = parse_hex(parts[3]);
            let result: Vec<&str> = parts[4].split(':').collect();
            let expected_consumed: usize = result[0].parse().unwrap();
            let expected_output = parse_hex(result[1]);
            let (consumed, output) = decode_wide(parts[0], &input, width, height);
            assert_eq!(consumed, expected_consumed, "consumed {line}");
            assert_eq!(output, expected_output, "output {line}");
            rows += 1;
        }
        assert_eq!(rows, 23);
    }

    fn decode_graphics(kind: &str, input: &[u8], width: usize, height: usize) -> (usize, Vec<u8>) {
        let mut output = vec![0u8; width * height * 4];
        let consumed = match kind {
            "R32_UInt" => decode_r32_unsigned(input, width, height, &mut output),
            "R32G32_UInt" => decode_r32g32_unsigned(input, width, height, &mut output),
            "R32G32B32_UInt" => decode_r32g32b32_unsigned(input, width, height, &mut output),
            "R32G32B32A32_UInt" => decode_r32g32b32a32_unsigned(input, width, height, &mut output),
            "R32_SInt" => decode_r32_signed(input, width, height, &mut output),
            "R32G32_SInt" => decode_r32g32_signed(input, width, height, &mut output),
            "R32G32B32_SInt" => decode_r32g32b32_signed(input, width, height, &mut output),
            "R32G32B32A32_SInt" => decode_r32g32b32a32_signed(input, width, height, &mut output),
            "R16G16B16_SFloat" => decode_rgb_half(input, width, height, &mut output),
            "R32G32B32_SFloat" => decode_rgb_float(input, width, height, &mut output),
            "B8G8R8_SNorm" => decode_bgr24_signed(input, width, height, &mut output),
            "B8G8R8A8_SNorm" => decode_bgra32_signed(input, width, height, &mut output),
            "A16_UNorm" => decode_a16(input, width, height, &mut output),
            other => panic!("unknown graphics format {other}"),
        }
        .unwrap();
        (consumed, output)
    }

    /// Byte-for-byte differential against `AssetRipper.TextureDecoder`'s `RgbConverter` (probe
    /// `Temp/opencode/texprobe4`) for the `GraphicsFormat`-only decoders.
    #[test]
    fn graphics_format_decoders_match_the_reference_runtime() {
        let fixture = include_str!("graphics_format_decode_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            let width: usize = parts[1].parse().unwrap();
            let height: usize = parts[2].parse().unwrap();
            let input = parse_hex(parts[3]);
            let result: Vec<&str> = parts[4].split(':').collect();
            let expected_consumed: usize = result[0].parse().unwrap();
            let expected_output = parse_hex(result[1]);
            let (consumed, output) = decode_graphics(parts[0], &input, width, height);
            assert_eq!(consumed, expected_consumed, "consumed {line}");
            assert_eq!(output, expected_output, "output {line}");
            rows += 1;
        }
        assert_eq!(rows, 13);
    }
}
