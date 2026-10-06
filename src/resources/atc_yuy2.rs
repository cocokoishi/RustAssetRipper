//! ATC and YUY2 decoders ported from the `AssetRipper.TextureDecoder` dependency used by
//! `AssetRipper.Export.Modules.Textures.TextureConverter`.
//!
//! `AtcDecoder.DecompressAtcRgb4`/`DecompressAtcRgba8` and `Yuy2Decoder.DecompressYUY2` are the
//! baseline paths for `TextureFormat.ATC_RGB4`/`ATC_RGBA8`/`YUY2`. The ATC color endpoints use the
//! bit-replicating `Extend` helper, and the ATC alpha table is the same truncating table as DXT5.
//!
//! Differential fixture: `atc_yuy2_fixture.txt` (probe `Temp/opencode/texprobe2`).

use super::texture_decode::{dxt5_alpha_table, require_input, require_output, TextureDecodeError};

type Pixel = [u8; 4];
type Block = [Pixel; 16];

fn clamp255(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

/// `AtcDecoder.Extend`: bit-pattern-replicating scaling.
fn extend(value: i32, from: i32, to: i32) -> i32 {
    (value << (to - from)) | (value >> (from * 2 - to))
}

/// `AtcDecoder.DecodeColors`, returning the four entries as `[blue, green, red]`.
fn decode_colors(c0: i32, c1: i32) -> [[i32; 3]; 4] {
    let mut colors = [[0i32; 3]; 4];
    if (c0 & 0x8000) == 0 {
        colors[0] = [
            extend(c0 & 0x1F, 5, 8),
            extend((c0 >> 5) & 0x1F, 5, 8),
            extend((c0 >> 10) & 0x1F, 5, 8),
        ];
        colors[3] = [
            extend(c1 & 0x1F, 5, 8),
            extend((c1 >> 5) & 0x3F, 6, 8),
            extend((c1 >> 11) & 0x1F, 5, 8),
        ];
        colors[1] = [
            (5 * colors[0][0] + 3 * colors[3][0]) / 8,
            (5 * colors[0][1] + 3 * colors[3][1]) / 8,
            (5 * colors[0][2] + 3 * colors[3][2]) / 8,
        ];
        colors[2] = [
            (3 * colors[0][0] + 5 * colors[3][0]) / 8,
            (3 * colors[0][1] + 5 * colors[3][1]) / 8,
            (3 * colors[0][2] + 5 * colors[3][2]) / 8,
        ];
    } else {
        colors[2] = [
            extend(c0 & 0x1F, 5, 8),
            extend((c0 >> 5) & 0x1F, 5, 8),
            extend((c0 >> 10) & 0x1F, 5, 8),
        ];
        colors[3] = [
            extend(c1 & 0x1F, 5, 8),
            extend((c1 >> 5) & 0x3F, 6, 8),
            extend((c1 >> 11) & 0x1F, 5, 8),
        ];
        colors[1] = [
            0.max(colors[2][0] - colors[3][0] / 4),
            0.max(colors[2][1] - colors[3][1] / 4),
            0.max(colors[2][2] - colors[3][2] / 4),
        ];
    }
    colors
}

fn decode_atc_rgb4_block(input: &[u8], output: &mut Block) {
    let c0 = i32::from(u16::from_le_bytes([input[0], input[1]]));
    let c1 = i32::from(u16::from_le_bytes([input[2], input[3]]));
    let mut cindex = u32::from_le_bytes([input[4], input[5], input[6], input[7]]);
    let colors = decode_colors(c0, c1);
    for pixel in output.iter_mut() {
        let color = colors[(cindex & 3) as usize];
        *pixel = [color[2] as u8, color[1] as u8, color[0] as u8, 0xFF];
        cindex >>= 2;
    }
}

fn decode_atc_rgba8_block(input: &[u8], output: &mut Block) {
    let avalue = u64::from_le_bytes([
        input[0], input[1], input[2], input[3], input[4], input[5], input[6], input[7],
    ]);
    let a0 = (avalue & 0xFF) as u8;
    let a1 = ((avalue >> 8) & 0xFF) as u8;
    let mut aindex = avalue >> 16;
    let alphas = dxt5_alpha_table(a0, a1);
    let c0 = i32::from(u16::from_le_bytes([input[8], input[9]]));
    let c1 = i32::from(u16::from_le_bytes([input[10], input[11]]));
    let mut cindex = u32::from_le_bytes([input[12], input[13], input[14], input[15]]);
    let colors = decode_colors(c0, c1);
    for pixel in output.iter_mut() {
        let color = colors[(cindex & 3) as usize];
        let alpha = alphas[(aindex & 7) as usize];
        *pixel = [color[2] as u8, color[1] as u8, color[0] as u8, alpha];
        cindex >>= 2;
        aindex >>= 3;
    }
}

fn decode_block_image(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
    block_size: usize,
    mut decode_block: impl FnMut(&[u8], &mut Block),
) -> Result<usize, TextureDecodeError> {
    require_output(output, width, height)?;
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);
    let required = blocks_x.saturating_mul(blocks_y).saturating_mul(block_size);
    require_input(input, required)?;
    let clen_last = (width + 3) % 4 + 1;
    let mut buffer: Block = [[0, 0, 0, 0]; 16];
    let mut offset = 0;
    for t in 0..blocks_y {
        for s in 0..blocks_x {
            decode_block(&input[offset..offset + block_size], &mut buffer);
            let clen = if s < blocks_x - 1 { 4 } else { clen_last };
            let output_base = t * 4 * width + s * 4;
            for i in 0..4 {
                let y = t * 4 + i;
                if y >= height {
                    break;
                }
                for j in 0..clen {
                    let dst = output_base + i * width + j;
                    output[dst * 4..dst * 4 + 4].copy_from_slice(&buffer[j + 4 * i]);
                }
            }
            offset += block_size;
        }
    }
    Ok(offset)
}

/// `AtcDecoder.DecompressAtcRgb4`.
pub fn decode_atc_rgb4(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_block_image(input, width, height, output, 8, decode_atc_rgb4_block)
}

/// `AtcDecoder.DecompressAtcRgba8`.
pub fn decode_atc_rgba8(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_block_image(input, width, height, output, 16, decode_atc_rgba8_block)
}

/// `Yuy2Decoder.DecompressYUY2`. Odd widths drop the final column, matching the reference.
pub fn decode_yuy2(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    require_output(output, width, height)?;
    let half_width = width / 2;
    let required = half_width.saturating_mul(4).saturating_mul(height);
    require_input(input, required)?;
    let mut p = 0;
    let mut o = 0;
    for _row in 0..height {
        for _pair in 0..half_width {
            let y0 = i32::from(input[p]);
            let u0 = i32::from(input[p + 1]);
            let y1 = i32::from(input[p + 2]);
            let v0 = i32::from(input[p + 3]);
            p += 4;
            let d = u0 - 128;
            let e = v0 - 128;
            let c = y0 - 16;
            let b0 = clamp255((298 * c + 516 * d + 128) >> 8);
            let g0 = clamp255((298 * c - 100 * d - 208 * e + 128) >> 8);
            let r0 = clamp255((298 * c + 409 * e + 128) >> 8);
            output[o * 4..o * 4 + 4].copy_from_slice(&[r0, g0, b0, 0xFF]);
            o += 1;
            let c = y1 - 16;
            let b1 = clamp255((298 * c + 516 * d + 128) >> 8);
            let g1 = clamp255((298 * c - 100 * d - 208 * e + 128) >> 8);
            let r1 = clamp255((298 * c + 409 * e + 128) >> 8);
            output[o * 4..o * 4 + 4].copy_from_slice(&[r1, g1, b1, 0xFF]);
            o += 1;
        }
    }
    Ok(p)
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
            "ATC_RGB4" => decode_atc_rgb4(input, width, height, &mut output),
            "ATC_RGBA8" => decode_atc_rgba8(input, width, height, &mut output),
            "YUY2" => decode_yuy2(input, width, height, &mut output),
            other => panic!("unknown ATC/YUY2 kind {other}"),
        }
        .unwrap();
        (consumed, output)
    }

    /// Byte-for-byte differential against `AssetRipper.TextureDecoder` (probe
    /// `Temp/opencode/texprobe2`).
    #[test]
    fn atc_and_yuy2_match_the_reference_runtime() {
        let fixture = include_str!("atc_yuy2_fixture.txt");
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
        assert_eq!(rows, 5);
    }

    #[test]
    fn partial_blocks_odd_widths_and_bounds_match_the_source_shapes() {
        // A 5x3 image is two blocks wide with a ragged edge: the copy geometry is
        // checked against the fixture-validated single-block path, which both
        // blocks share here.
        let (_, full) = decode("ATC_RGB4", &[0x11u8; 8], 4, 4);
        let mut output = vec![0xAAu8; 5 * 3 * 4];
        let consumed = decode_atc_rgb4(&[0x11u8; 16], 5, 3, &mut output).unwrap();
        assert_eq!(consumed, 16);
        let mut expected = Vec::with_capacity(5 * 3 * 4);
        for y in 0..3 {
            expected.extend_from_slice(&full[y * 16..y * 16 + 16]);
            expected.extend_from_slice(&full[y * 16..y * 16 + 4]);
        }
        assert_eq!(output, expected);
        // Odd YUY2 widths drop the final column: the pair math consumes
        // `half_width` pairs and the leftover column is never written.
        let mut output = vec![0xAAu8; 5 * 4];
        let consumed = decode_yuy2(&[0x80u8; 8], 5, 1, &mut output).unwrap();
        assert_eq!(consumed, 8);
        assert_eq!(&output[16..20], &[0xAAu8; 4]);
        let (_, even) = decode("YUY2", &[0x80u8; 8], 4, 1);
        assert_eq!(&output[..16], &even[..]);
        // Short inputs and outputs report exact bounds per entry point.
        let mut output = vec![0u8; 64];
        assert_eq!(
            decode_atc_rgb4(&[0u8; 7], 4, 4, &mut output),
            Err(TextureDecodeError::InputTooShort {
                required: 8,
                actual: 7
            })
        );
        assert_eq!(
            decode_atc_rgba8(&[0u8; 15], 4, 4, &mut output),
            Err(TextureDecodeError::InputTooShort {
                required: 16,
                actual: 15
            })
        );
        assert_eq!(
            decode_yuy2(&[0u8; 31], 4, 4, &mut output),
            Err(TextureDecodeError::InputTooShort {
                required: 32,
                actual: 31
            })
        );
        let mut small = vec![0u8; 63];
        assert_eq!(
            decode_atc_rgb4(&[0u8; 8], 4, 4, &mut small),
            Err(TextureDecodeError::OutputTooSmall {
                required: 64,
                actual: 63
            })
        );
    }
}
