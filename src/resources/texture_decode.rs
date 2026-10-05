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
        }
    }
}

impl std::error::Error for TextureDecodeError {}

fn require_input(input: &[u8], required: usize) -> Result<(), TextureDecodeError> {
    if input.len() < required {
        return Err(TextureDecodeError::InputTooShort {
            required,
            actual: input.len(),
        });
    }
    Ok(())
}

fn require_output(output: &[u8], width: usize, height: usize) -> Result<usize, TextureDecodeError> {
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
    let required = blocks_x
        .checked_mul(blocks_y)
        .and_then(|count| count.checked_mul(8))
        .unwrap_or(usize::MAX);
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
    let required = blocks_x
        .checked_mul(blocks_y)
        .and_then(|count| count.checked_mul(16))
        .unwrap_or(usize::MAX);
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

fn dxt5_alpha_table(alpha0: u8, alpha1: u8) -> [u8; 8] {
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
    let required = blocks_x
        .checked_mul(blocks_y)
        .and_then(|count| count.checked_mul(16))
        .unwrap_or(usize::MAX);
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

fn decode_packed<const N: usize>(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
    mut convert: impl FnMut(&[u8]) -> [u8; 4],
) -> Result<usize, TextureDecodeError> {
    require_output(output, width, height)?;
    let pixels = width.checked_mul(height).unwrap_or(usize::MAX);
    let required = pixels.checked_mul(N).unwrap_or(usize::MAX);
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
        assert_eq!(rows, 22);
    }
}
