//! PVRTC decoder ported from the `AssetRipper.TextureDecoder` dependency used by
//! `AssetRipper.Export.Modules.Textures.TextureConverter`.
//!
//! `PvrtcDecoder.DecompressPVRTC` walks the image pixel by pixel, twiddling block coordinates and
//! interpolating the A/B colours per pixel. Two reference quirks are preserved deliberately and
//! confirmed by the isolated probe in `Temp/opencode/pvrtcprobe`:
//!
//! - `Unpack5554Colour` expands the blue channel of the second colour into slot 2 (colour A's blue)
//!   instead of slot 6, so the second colour's blue is not replicated.
//! - `TwiddleUV` prepends the unused high bits of the larger coordinate.
//!
//! Differential fixture: `pvrtc_fixture.txt` (probe `Temp/opencode/pvrtcprobe`).

use super::texture_decode::{require_input, require_output, TextureDecodeError};

const PT_INDEX: i32 = 2;
const BLOCK_Y_SIZE: i32 = 4;
const BLOCK_X_2BPP: i32 = 8;
const BLOCK_X_4BPP: i32 = 4;
const REP_VALS0: [i32; 4] = [0, 3, 5, 8];
const REP_VALS1: [i32; 4] = [0, 4, 4, 8];

#[derive(Clone, Copy)]
struct AmtcBlock {
    data0: u32,
    data1: u32,
}

fn limit_coordinate(value: i32, size: i32) -> i32 {
    value & (size - 1)
}

fn twiddle_uv(y_size: u32, x_size: u32, y_pos: u32, x_pos: u32) -> u32 {
    let (min_dimension, max_value) = if y_size < x_size {
        (y_size, x_pos)
    } else {
        (x_size, y_pos)
    };
    let mut src_bit_pos = 1u32;
    let mut dst_bit_pos = 1u32;
    let mut twiddled = 0u32;
    let mut shift_count = 0u32;
    while src_bit_pos < min_dimension {
        if (y_pos & src_bit_pos) != 0 {
            twiddled |= dst_bit_pos;
        }
        if (x_pos & src_bit_pos) != 0 {
            twiddled |= dst_bit_pos << 1;
        }
        src_bit_pos <<= 1;
        dst_bit_pos <<= 2;
        shift_count += 1;
    }
    twiddled |= (max_value >> shift_count) << (2 * shift_count);
    twiddled
}

fn get_modulation_value(
    x: i32,
    y: i32,
    do2bit_mode: bool,
    vals: &[i32; 128],
    modes: &[i32; 128],
) -> (i32, bool) {
    let y = (y & 0x3) | ((!y & 0x2) << 1);
    let x = if do2bit_mode {
        (x & 0x7) | ((!x & 0x4) << 1)
    } else {
        (x & 0x3) | ((!x & 0x2) << 1)
    };
    let mut do_pt = false;
    let mod_val;
    let index = (y * 16 + x) as usize;
    if modes[index] == 0 {
        mod_val = REP_VALS0[vals[index] as usize];
    } else if do2bit_mode {
        if ((x ^ y) & 1) == 0 {
            mod_val = REP_VALS0[vals[index] as usize];
        } else if modes[index] == 1 {
            mod_val = (REP_VALS0[vals[((y - 1) * 16 + x) as usize] as usize]
                + REP_VALS0[vals[((y + 1) * 16 + x) as usize] as usize]
                + REP_VALS0[vals[(y * 16 + x - 1) as usize] as usize]
                + REP_VALS0[vals[(y * 16 + x + 1) as usize] as usize]
                + 2)
                / 4;
        } else if modes[index] == 2 {
            mod_val = (REP_VALS0[vals[(y * 16 + x - 1) as usize] as usize]
                + REP_VALS0[vals[(y * 16 + x + 1) as usize] as usize]
                + 1)
                / 2;
        } else {
            mod_val = (REP_VALS0[vals[((y - 1) * 16 + x) as usize] as usize]
                + REP_VALS0[vals[((y + 1) * 16 + x) as usize] as usize]
                + 1)
                / 2;
        }
    } else {
        mod_val = REP_VALS1[vals[index] as usize];
        do_pt = vals[index] == PT_INDEX;
    }
    (mod_val, do_pt)
}

#[allow(clippy::too_many_arguments)]
fn interpolate_colours(
    p: &[i32; 8],
    q: &[i32; 8],
    r: &[i32; 8],
    s: &[i32; 8],
    ci: usize,
    do2bit_mode: bool,
    x: i32,
    y: i32,
    result: &mut [i32; 4],
) {
    let v = ((y & 0x3) | ((!y & 0x2) << 1)) - BLOCK_Y_SIZE / 2;
    let u = if do2bit_mode {
        ((x & 0x7) | ((!x & 0x4) << 1)) - BLOCK_X_2BPP / 2
    } else {
        ((x & 0x3) | ((!x & 0x2) << 1)) - BLOCK_X_4BPP / 2
    };
    let uscale = if do2bit_mode { 8 } else { 4 };
    for i in 0..4 {
        let tmp1 = p[ci * 4 + i] * uscale + u * (q[ci * 4 + i] - p[ci * 4 + i]);
        let tmp2 = r[ci * 4 + i] * uscale + u * (s[ci * 4 + i] - r[ci * 4 + i]);
        result[i] = tmp1 * 4 + v * (tmp2 - tmp1);
    }
    if do2bit_mode {
        for value in result.iter_mut().take(3) {
            *value >>= 2;
        }
        result[3] >>= 1;
    } else {
        for value in result.iter_mut().take(3) {
            *value >>= 1;
        }
    }
    for value in result.iter_mut().take(3) {
        *value += *value >> 5;
    }
    result[3] += result[3] >> 4;
}

fn unpack_modulations(
    block: &AmtcBlock,
    do2bit_mode: bool,
    vals: &mut [i32; 128],
    modes: &mut [i32; 128],
    start_x: usize,
    start_y: usize,
) {
    let block_mod_mode = (block.data1 & 1) as i32;
    let mut modulation_bits = block.data0;
    if do2bit_mode && block_mod_mode != 0 {
        for y in 0..BLOCK_Y_SIZE as usize {
            for x in 0..BLOCK_X_2BPP as usize {
                let index = (y + start_y) * 16 + x + start_x;
                modes[index] = block_mod_mode;
                if ((x ^ y) & 1) == 0 {
                    vals[index] = (modulation_bits & 3) as i32;
                    modulation_bits >>= 2;
                }
            }
        }
    } else if do2bit_mode {
        for y in 0..BLOCK_Y_SIZE as usize {
            for x in 0..BLOCK_X_2BPP as usize {
                let index = (y + start_y) * 16 + x + start_x;
                modes[index] = block_mod_mode;
                vals[index] = if (modulation_bits & 1) != 0 { 3 } else { 0 };
                modulation_bits >>= 1;
            }
        }
    } else {
        for y in 0..BLOCK_Y_SIZE as usize {
            for x in 0..BLOCK_X_4BPP as usize {
                let index = (y + start_y) * 16 + x + start_x;
                modes[index] = block_mod_mode;
                vals[index] = (modulation_bits & 3) as i32;
                modulation_bits >>= 2;
            }
        }
    }
}

fn unpack_5554_colour(block: &AmtcBlock, ab: &mut [i32; 8]) {
    let raw = [block.data1 & 0xFFFE, block.data1 >> 16];
    for i in 0..2usize {
        if (raw[i] & (1 << 15)) != 0 {
            ab[i * 4] = ((raw[i] >> 10) & 0x1F) as i32;
            ab[i * 4 + 1] = ((raw[i] >> 5) & 0x1F) as i32;
            ab[i * 4 + 2] = (raw[i] & 0x1F) as i32;
            if i == 0 {
                ab[2] |= ab[2] >> 4;
            }
            ab[i * 4 + 3] = 0xF;
        } else {
            ab[i * 4] = ((raw[i] >> 7) & 0x1E) as i32;
            ab[i * 4 + 1] = ((raw[i] >> 3) & 0x1E) as i32;
            ab[i * 4] |= ab[i * 4] >> 4;
            ab[i * 4 + 1] |= ab[i * 4 + 1] >> 4;
            ab[i * 4 + 2] = ((raw[i] & 0xF) << 1) as i32;
            if i == 0 {
                ab[2] |= ab[2] >> 3;
            } else {
                ab[2] |= ab[2] >> 4;
            }
            ab[i * 4 + 3] = ((raw[i] >> 11) & 0xE) as i32;
        }
    }
}

#[allow(clippy::too_many_lines)]
fn decompress(
    input: &[u8],
    x_dim: usize,
    y_dim: usize,
    do2bit_mode: bool,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    require_output(output, x_dim, y_dim)?;
    let x_block_size = if do2bit_mode {
        BLOCK_X_2BPP
    } else {
        BLOCK_X_4BPP
    };
    let block_x_dim = 2.max(x_dim as i32 / x_block_size) as u32;
    let block_y_dim = 2.max(y_dim as i32 / BLOCK_Y_SIZE) as u32;
    let required = (block_x_dim as usize)
        .saturating_mul(block_y_dim as usize)
        .saturating_mul(8);
    require_input(input, required)?;

    let blocks: Vec<AmtcBlock> = input
        .chunks_exact(8)
        .take((block_x_dim * block_y_dim) as usize)
        .map(|chunk| AmtcBlock {
            data0: u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            data1: u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
        })
        .collect();

    let mut pblocki00 = u32::MAX;
    let mut pblocki01 = u32::MAX;
    let mut pblocki10 = u32::MAX;
    let mut pblocki11 = u32::MAX;
    let mut max_block_index_read = 0u32;

    let mut colors = [[0i32; 8]; 4];
    let mut a_sig = [0i32; 4];
    let mut b_sig = [0i32; 4];
    let mut modulation_vals = [0i32; 128];
    let mut modulation_modes = [0i32; 128];

    for y in 0..y_dim as i32 {
        for x in 0..x_dim as i32 {
            let block_x = limit_coordinate(x - x_block_size / 2, x_dim as i32) / x_block_size;
            let block_y = limit_coordinate(y - BLOCK_Y_SIZE / 2, y_dim as i32) / BLOCK_Y_SIZE;
            let block_xp1 = limit_coordinate(block_x + 1, block_x_dim as i32);
            let block_yp1 = limit_coordinate(block_y + 1, block_y_dim as i32);

            let blocki00 = twiddle_uv(block_y_dim, block_x_dim, block_y as u32, block_x as u32);
            let blocki01 = twiddle_uv(block_y_dim, block_x_dim, block_y as u32, block_xp1 as u32);
            let blocki10 = twiddle_uv(block_y_dim, block_x_dim, block_yp1 as u32, block_x as u32);
            let blocki11 = twiddle_uv(block_y_dim, block_x_dim, block_yp1 as u32, block_xp1 as u32);

            let changed = blocki00 != pblocki00
                || blocki01 != pblocki01
                || blocki10 != pblocki10
                || blocki11 != pblocki11;
            if changed {
                let block00 = blocks[blocki00 as usize];
                unpack_5554_colour(&block00, &mut colors[0]);
                unpack_modulations(
                    &block00,
                    do2bit_mode,
                    &mut modulation_vals,
                    &mut modulation_modes,
                    0,
                    0,
                );
                let block01 = blocks[blocki01 as usize];
                unpack_5554_colour(&block01, &mut colors[1]);
                unpack_modulations(
                    &block01,
                    do2bit_mode,
                    &mut modulation_vals,
                    &mut modulation_modes,
                    x_block_size as usize,
                    0,
                );
                let block10 = blocks[blocki10 as usize];
                unpack_5554_colour(&block10, &mut colors[2]);
                unpack_modulations(
                    &block10,
                    do2bit_mode,
                    &mut modulation_vals,
                    &mut modulation_modes,
                    0,
                    BLOCK_Y_SIZE as usize,
                );
                let block11 = blocks[blocki11 as usize];
                unpack_5554_colour(&block11, &mut colors[3]);
                unpack_modulations(
                    &block11,
                    do2bit_mode,
                    &mut modulation_vals,
                    &mut modulation_modes,
                    x_block_size as usize,
                    BLOCK_Y_SIZE as usize,
                );

                pblocki00 = blocki00;
                pblocki01 = blocki01;
                pblocki10 = blocki10;
                pblocki11 = blocki11;
                max_block_index_read = max_block_index_read
                    .max(blocki00)
                    .max(blocki01)
                    .max(blocki10)
                    .max(blocki11);
            }

            interpolate_colours(
                &colors[0],
                &colors[1],
                &colors[2],
                &colors[3],
                0,
                do2bit_mode,
                x,
                y,
                &mut a_sig,
            );
            interpolate_colours(
                &colors[0],
                &colors[1],
                &colors[2],
                &colors[3],
                1,
                do2bit_mode,
                x,
                y,
                &mut b_sig,
            );
            let (mod_val, do_pt) =
                get_modulation_value(x, y, do2bit_mode, &modulation_vals, &modulation_modes);

            let red = ((a_sig[0] * 8 + mod_val * (b_sig[0] - a_sig[0])) >> 3) as u8;
            let green = ((a_sig[1] * 8 + mod_val * (b_sig[1] - a_sig[1])) >> 3) as u8;
            let blue = ((a_sig[2] * 8 + mod_val * (b_sig[2] - a_sig[2])) >> 3) as u8;
            let alpha = if do_pt {
                0
            } else {
                ((a_sig[3] * 8 + mod_val * (b_sig[3] - a_sig[3])) >> 3) as u8
            };
            let pixel = (x + y * x_dim as i32) as usize;
            output[pixel * 4..pixel * 4 + 4].copy_from_slice(&[red, green, blue, alpha]);
        }
    }

    Ok(((max_block_index_read + 1) * 8) as usize)
}

/// `PvrtcDecoder.DecompressPVRTC` for PVRTC4 (`do2bitMode = false`).
pub fn decode_pvrtc_4bpp(
    input: &[u8],
    x_dim: usize,
    y_dim: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decompress(input, x_dim, y_dim, false, output)
}

/// `PvrtcDecoder.DecompressPVRTC` for PVRTC2 (`do2bitMode = true`).
pub fn decode_pvrtc_2bpp(
    input: &[u8],
    x_dim: usize,
    y_dim: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decompress(input, x_dim, y_dim, true, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::texture_decode::TextureDecodeError;

    fn parse_hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks_exact(2)
            .map(|chunk| {
                let text = std::str::from_utf8(chunk).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }

    /// Byte-for-byte differential against `AssetRipper.TextureDecoder` (probe
    /// `Temp/opencode/pvrtcprobe`) for PVRTC2 and PVRTC4.
    #[test]
    fn pvrtc_matches_the_reference_runtime() {
        let fixture = include_str!("pvrtc_fixture.txt");
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
            let mut output = vec![0u8; width * height * 4];
            let consumed = match parts[0] {
                "PVRTC4" => decode_pvrtc_4bpp(&input, width, height, &mut output),
                "PVRTC2" => decode_pvrtc_2bpp(&input, width, height, &mut output),
                other => panic!("unknown PVRTC kind {other}"),
            }
            .unwrap();
            assert_eq!(consumed, expected_consumed, "consumed {line}");
            assert_eq!(output, expected_output, "output {line}");
            rows += 1;
        }
        assert_eq!(rows, 4);
    }

    #[test]
    fn decode_edges_report_short_buffers() {
        let mut output = vec![0u8; 8 * 8 * 4];
        // 8x8 needs the full 2x2-block floor: 32 bytes.
        assert_eq!(
            decode_pvrtc_4bpp(&[0u8; 31], 8, 8, &mut output),
            Err(TextureDecodeError::InputTooShort {
                required: 32,
                actual: 31,
            })
        );
        assert!(decode_pvrtc_4bpp(&[0u8; 32], 8, 8, &mut output).is_ok());
        // The output must hold every pixel.
        let mut small = vec![0u8; 8 * 8 * 4 - 1];
        assert!(matches!(
            decode_pvrtc_4bpp(&[0u8; 128], 8, 8, &mut small),
            Err(TextureDecodeError::OutputTooSmall { .. })
        ));
        // 4x4 images floor to the same 2x2 blocks.
        let mut tiny = vec![0u8; 4 * 4 * 4];
        assert_eq!(
            decode_pvrtc_4bpp(&[0u8; 31], 4, 4, &mut tiny),
            Err(TextureDecodeError::InputTooShort {
                required: 32,
                actual: 31,
            })
        );
        assert!(decode_pvrtc_4bpp(&[0u8; 32], 4, 4, &mut tiny).is_ok());
        // 2bpp floors each axis independently: 8x4 still needs 32 bytes.
        let mut wide = vec![0u8; 8 * 4 * 4];
        assert_eq!(
            decode_pvrtc_2bpp(&[0u8; 31], 8, 4, &mut wide),
            Err(TextureDecodeError::InputTooShort {
                required: 32,
                actual: 31,
            })
        );
        assert!(decode_pvrtc_2bpp(&[0u8; 32], 8, 4, &mut wide).is_ok());
    }
}
