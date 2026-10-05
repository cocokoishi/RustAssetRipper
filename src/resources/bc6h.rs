//! BC6H (HDR) decoder ported from the `AssetRipper.TextureDecoder` dependency used by
//! `AssetRipper.Export.Modules.Textures.TextureConverter`.
//!
//! `BcHelpers.DecompressBc6h` reads a 128-bit little-endian bit stream, parses one of fourteen
//! endpoint modes, applies the shared/extended exponents, unquantizes, and interpolates to 16-bit
//! per-channel values. The block yields three `ushort` channels, which the caller converts to the
//! requested output color (here RGBA8).
//!
//! Differential fixture: `bc6h_fixture.txt` (probe `Temp/opencode/bc6hprobe`).

use super::bc6h_tables::{ACTUAL_BITS_COUNT, PARTITION_SETS, WEIGHT3, WEIGHT4};
use super::bc7::BitStream;
use super::texture_decode::{require_input, require_output, ushort_to_byte, TextureDecodeError};

type Pixel = [u8; 4];
type Block = [Pixel; 16];

fn extend_sign(val: u32, bits: u32) -> u32 {
    (((val as i32) << (32 - bits)) >> (32 - bits)) as u32
}

fn transform_inverse(val: u32, a0: u32, bits: u32, is_signed: bool) -> u32 {
    let val = val.wrapping_add(a0) & ((1u32 << bits) - 1);
    if is_signed {
        extend_sign(val, bits)
    } else {
        val
    }
}

fn unquantize(val: i32, bits: u32, is_signed: bool) -> i32 {
    let mut val = val;
    if !is_signed {
        if bits >= 15 {
            val
        } else if val == 0 {
            0
        } else if val == (1i32 << bits) - 1 {
            0xFFFF
        } else {
            ((val << 16) + 0x8000) >> bits
        }
    } else if bits >= 16 {
        val
    } else {
        let mut s = 0;
        if val < 0 {
            s = 1;
            val = -val;
        }
        let mut unq = if val == 0 {
            0
        } else if val >= (1i32 << (bits - 1)) - 1 {
            0x7FFF
        } else {
            ((val << 15) + 0x4000) >> (bits - 1)
        };
        if s != 0 {
            unq = -unq;
        }
        unq
    }
}

fn interpolate(a: i32, b: i32, weights: &[i32], index: i32) -> i32 {
    let weight = weights[index as usize];
    ((a * (64 - weight)) + (b * weight) + 32) >> 6
}

fn finish_unquantize(val: i32, is_signed: bool) -> u16 {
    if !is_signed {
        ((val * 31) >> 6) as u16
    } else {
        let mut val = if val < 0 {
            ((-val) * 31) >> 5
        } else {
            (val * 31) >> 5
        };
        let mut s = 0;
        if val < 0 {
            s = 0x8000;
            val = -val;
        }
        (s | val) as u16
    }
}

fn read_bc6h_mode_bits(bstream: &mut BitStream) -> u32 {
    let two_bits = bstream.read_bits(2);
    if two_bits > 1 {
        let three_bits = bstream.read_bits(3);
        (three_bits << 2) | two_bits
    } else {
        two_bits
    }
}

#[allow(clippy::too_many_lines)]
fn decode_bc6h_block(input: &[u8], output: &mut [[u16; 3]; 16], is_signed: bool) {
    let mut bstream = BitStream::new(input);
    let mut r = [0u32; 4];
    let mut g = [0u32; 4];
    let mut b = [0u32; 4];

    let mode: i32;
    let partition: u32;

    match read_bc6h_mode_bits(&mut bstream) {
        0b00 => {
            g[2] |= bstream.read_bit() << 4;
            b[2] |= bstream.read_bit() << 4;
            b[3] |= bstream.read_bit() << 4;
            r[0] |= bstream.read_bits(10);
            g[0] |= bstream.read_bits(10);
            b[0] |= bstream.read_bits(10);
            r[1] |= bstream.read_bits(5);
            g[3] |= bstream.read_bit() << 4;
            g[2] |= bstream.read_bits(4);
            g[1] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit();
            g[3] |= bstream.read_bits(4);
            b[1] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 1;
            b[2] |= bstream.read_bits(4);
            r[2] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 2;
            r[3] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 3;
            partition = bstream.read_bits(5);
            mode = 0;
        }
        0b01 => {
            g[2] |= bstream.read_bit() << 5;
            g[3] |= bstream.read_bit() << 4;
            g[3] |= bstream.read_bit() << 5;
            r[0] |= bstream.read_bits(7);
            b[3] |= bstream.read_bit();
            b[3] |= bstream.read_bit() << 1;
            b[2] |= bstream.read_bit() << 4;
            g[0] |= bstream.read_bits(7);
            b[2] |= bstream.read_bit() << 5;
            b[3] |= bstream.read_bit() << 2;
            g[2] |= bstream.read_bit() << 4;
            b[0] |= bstream.read_bits(7);
            b[3] |= bstream.read_bit() << 3;
            b[3] |= bstream.read_bit() << 5;
            b[3] |= bstream.read_bit() << 4;
            r[1] |= bstream.read_bits(6);
            g[2] |= bstream.read_bits(4);
            g[1] |= bstream.read_bits(6);
            g[3] |= bstream.read_bits(4);
            b[1] |= bstream.read_bits(6);
            b[2] |= bstream.read_bits(4);
            r[2] |= bstream.read_bits(6);
            r[3] |= bstream.read_bits(6);
            partition = bstream.read_bits(5);
            mode = 1;
        }
        0b00010 => {
            r[0] |= bstream.read_bits(10);
            g[0] |= bstream.read_bits(10);
            b[0] |= bstream.read_bits(10);
            r[1] |= bstream.read_bits(5);
            r[0] |= bstream.read_bit() << 10;
            g[2] |= bstream.read_bits(4);
            g[1] |= bstream.read_bits(4);
            g[0] |= bstream.read_bit() << 10;
            b[3] |= bstream.read_bit();
            g[3] |= bstream.read_bits(4);
            b[1] |= bstream.read_bits(4);
            b[0] |= bstream.read_bit() << 10;
            b[3] |= bstream.read_bit() << 1;
            b[2] |= bstream.read_bits(4);
            r[2] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 2;
            r[3] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 3;
            partition = bstream.read_bits(5);
            mode = 2;
        }
        0b00110 => {
            r[0] |= bstream.read_bits(10);
            g[0] |= bstream.read_bits(10);
            b[0] |= bstream.read_bits(10);
            r[1] |= bstream.read_bits(4);
            r[0] |= bstream.read_bit() << 10;
            g[3] |= bstream.read_bit() << 4;
            g[2] |= bstream.read_bits(4);
            g[1] |= bstream.read_bits(5);
            g[0] |= bstream.read_bit() << 10;
            g[3] |= bstream.read_bits(4);
            b[1] |= bstream.read_bits(4);
            b[0] |= bstream.read_bit() << 10;
            b[3] |= bstream.read_bit() << 1;
            b[2] |= bstream.read_bits(4);
            r[2] |= bstream.read_bits(4);
            b[3] |= bstream.read_bit();
            b[3] |= bstream.read_bit() << 2;
            r[3] |= bstream.read_bits(4);
            g[2] |= bstream.read_bit() << 4;
            b[3] |= bstream.read_bit() << 3;
            partition = bstream.read_bits(5);
            mode = 3;
        }
        0b01010 => {
            r[0] |= bstream.read_bits(10);
            g[0] |= bstream.read_bits(10);
            b[0] |= bstream.read_bits(10);
            r[1] |= bstream.read_bits(4);
            r[0] |= bstream.read_bit() << 10;
            b[2] |= bstream.read_bit() << 4;
            g[2] |= bstream.read_bits(4);
            g[1] |= bstream.read_bits(4);
            g[0] |= bstream.read_bit() << 10;
            b[3] |= bstream.read_bit();
            g[3] |= bstream.read_bits(4);
            b[1] |= bstream.read_bits(5);
            b[0] |= bstream.read_bit() << 10;
            b[2] |= bstream.read_bits(4);
            r[2] |= bstream.read_bits(4);
            b[3] |= bstream.read_bit() << 1;
            b[3] |= bstream.read_bit() << 2;
            r[3] |= bstream.read_bits(4);
            b[3] |= bstream.read_bit() << 4;
            b[3] |= bstream.read_bit() << 3;
            partition = bstream.read_bits(5);
            mode = 4;
        }
        0b01110 => {
            r[0] |= bstream.read_bits(9);
            b[2] |= bstream.read_bit() << 4;
            g[0] |= bstream.read_bits(9);
            g[2] |= bstream.read_bit() << 4;
            b[0] |= bstream.read_bits(9);
            b[3] |= bstream.read_bit() << 4;
            r[1] |= bstream.read_bits(5);
            g[3] |= bstream.read_bit() << 4;
            g[2] |= bstream.read_bits(4);
            g[1] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit();
            g[3] |= bstream.read_bits(4);
            b[1] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 1;
            b[2] |= bstream.read_bits(4);
            r[2] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 2;
            r[3] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 3;
            partition = bstream.read_bits(5);
            mode = 5;
        }
        0b10010 => {
            r[0] |= bstream.read_bits(8);
            g[3] |= bstream.read_bit() << 4;
            b[2] |= bstream.read_bit() << 4;
            g[0] |= bstream.read_bits(8);
            b[3] |= bstream.read_bit() << 2;
            g[2] |= bstream.read_bit() << 4;
            b[0] |= bstream.read_bits(8);
            b[3] |= bstream.read_bit() << 3;
            b[3] |= bstream.read_bit() << 4;
            r[1] |= bstream.read_bits(6);
            g[2] |= bstream.read_bits(4);
            g[1] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit();
            g[3] |= bstream.read_bits(4);
            b[1] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 1;
            b[2] |= bstream.read_bits(4);
            r[2] |= bstream.read_bits(6);
            r[3] |= bstream.read_bits(6);
            partition = bstream.read_bits(5);
            mode = 6;
        }
        0b10110 => {
            r[0] |= bstream.read_bits(8);
            b[3] |= bstream.read_bit();
            b[2] |= bstream.read_bit() << 4;
            g[0] |= bstream.read_bits(8);
            g[2] |= bstream.read_bit() << 5;
            g[2] |= bstream.read_bit() << 4;
            b[0] |= bstream.read_bits(8);
            g[3] |= bstream.read_bit() << 5;
            b[3] |= bstream.read_bit() << 4;
            r[1] |= bstream.read_bits(5);
            g[3] |= bstream.read_bit() << 4;
            g[2] |= bstream.read_bits(4);
            g[1] |= bstream.read_bits(6);
            g[3] |= bstream.read_bits(4);
            b[1] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 1;
            b[2] |= bstream.read_bits(4);
            r[2] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 2;
            r[3] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 3;
            partition = bstream.read_bits(5);
            mode = 7;
        }
        0b11010 => {
            r[0] |= bstream.read_bits(8);
            b[3] |= bstream.read_bit() << 1;
            b[2] |= bstream.read_bit() << 4;
            g[0] |= bstream.read_bits(8);
            b[2] |= bstream.read_bit() << 5;
            g[2] |= bstream.read_bit() << 4;
            b[0] |= bstream.read_bits(8);
            b[3] |= bstream.read_bit() << 5;
            b[3] |= bstream.read_bit() << 4;
            r[1] |= bstream.read_bits(5);
            g[3] |= bstream.read_bit() << 4;
            g[2] |= bstream.read_bits(4);
            g[1] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit();
            g[3] |= bstream.read_bits(4);
            b[1] |= bstream.read_bits(6);
            b[2] |= bstream.read_bits(4);
            r[2] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 2;
            r[3] |= bstream.read_bits(5);
            b[3] |= bstream.read_bit() << 3;
            partition = bstream.read_bits(5);
            mode = 8;
        }
        0b11110 => {
            r[0] |= bstream.read_bits(6);
            g[3] |= bstream.read_bit() << 4;
            b[3] |= bstream.read_bit();
            b[3] |= bstream.read_bit() << 1;
            b[2] |= bstream.read_bit() << 4;
            g[0] |= bstream.read_bits(6);
            g[2] |= bstream.read_bit() << 5;
            b[2] |= bstream.read_bit() << 5;
            b[3] |= bstream.read_bit() << 2;
            g[2] |= bstream.read_bit() << 4;
            b[0] |= bstream.read_bits(6);
            g[3] |= bstream.read_bit() << 5;
            b[3] |= bstream.read_bit() << 3;
            b[3] |= bstream.read_bit() << 5;
            b[3] |= bstream.read_bit() << 4;
            r[1] |= bstream.read_bits(6);
            g[2] |= bstream.read_bits(4);
            g[1] |= bstream.read_bits(6);
            g[3] |= bstream.read_bits(4);
            b[1] |= bstream.read_bits(6);
            b[2] |= bstream.read_bits(4);
            r[2] |= bstream.read_bits(6);
            r[3] |= bstream.read_bits(6);
            partition = bstream.read_bits(5);
            mode = 9;
        }
        0b00011 => {
            r[0] |= bstream.read_bits(10);
            g[0] |= bstream.read_bits(10);
            b[0] |= bstream.read_bits(10);
            r[1] |= bstream.read_bits(10);
            g[1] |= bstream.read_bits(10);
            b[1] |= bstream.read_bits(10);
            partition = 0;
            mode = 10;
        }
        0b00111 => {
            r[0] |= bstream.read_bits(10);
            g[0] |= bstream.read_bits(10);
            b[0] |= bstream.read_bits(10);
            r[1] |= bstream.read_bits(9);
            r[0] |= bstream.read_bit() << 10;
            g[1] |= bstream.read_bits(9);
            g[0] |= bstream.read_bit() << 10;
            b[1] |= bstream.read_bits(9);
            b[0] |= bstream.read_bit() << 10;
            partition = 0;
            mode = 11;
        }
        0b01011 => {
            r[0] |= bstream.read_bits(10);
            g[0] |= bstream.read_bits(10);
            b[0] |= bstream.read_bits(10);
            r[1] |= bstream.read_bits(8);
            r[0] |= bstream.read_bits_reversed(2) << 10;
            g[1] |= bstream.read_bits(8);
            g[0] |= bstream.read_bits_reversed(2) << 10;
            b[1] |= bstream.read_bits(8);
            b[0] |= bstream.read_bits_reversed(2) << 10;
            partition = 0;
            mode = 12;
        }
        0b01111 => {
            r[0] |= bstream.read_bits(10);
            g[0] |= bstream.read_bits(10);
            b[0] |= bstream.read_bits(10);
            r[1] |= bstream.read_bits(4);
            r[0] |= bstream.read_bits_reversed(6) << 10;
            g[1] |= bstream.read_bits(4);
            g[0] |= bstream.read_bits_reversed(6) << 10;
            b[1] |= bstream.read_bits(4);
            b[0] |= bstream.read_bits_reversed(6) << 10;
            partition = 0;
            mode = 13;
        }
        _ => {
            *output = [[0, 0, 0]; 16];
            return;
        }
    }

    let num_partitions: usize = if mode >= 10 { 0 } else { 1 };
    let actual_bits0 = ACTUAL_BITS_COUNT[0][mode as usize];
    if is_signed {
        r[0] = extend_sign(r[0], actual_bits0);
        g[0] = extend_sign(g[0], actual_bits0);
        b[0] = extend_sign(b[0], actual_bits0);
    }

    if (mode != 9 && mode != 10) || is_signed {
        for i in 1..(num_partitions + 1) * 2 {
            r[i] = extend_sign(r[i], ACTUAL_BITS_COUNT[1][mode as usize]);
            g[i] = extend_sign(g[i], ACTUAL_BITS_COUNT[2][mode as usize]);
            b[i] = extend_sign(b[i], ACTUAL_BITS_COUNT[3][mode as usize]);
        }
    }

    if mode != 9 && mode != 10 {
        for i in 1..(num_partitions + 1) * 2 {
            r[i] = transform_inverse(r[i], r[0], actual_bits0, is_signed);
            g[i] = transform_inverse(g[i], g[0], actual_bits0, is_signed);
            b[i] = transform_inverse(b[i], b[0], actual_bits0, is_signed);
        }
    }

    for i in 0..(num_partitions + 1) * 2 {
        r[i] = unquantize(r[i] as i32, actual_bits0, is_signed) as u32;
        g[i] = unquantize(g[i] as i32, actual_bits0, is_signed) as u32;
        b[i] = unquantize(b[i] as i32, actual_bits0, is_signed) as u32;
    }

    let weights: &[i32] = if mode >= 10 { &WEIGHT4 } else { &WEIGHT3 };
    let mut pixel = 0;
    for i in 0..4usize {
        for j in 0..4usize {
            let mut partition_set = if mode >= 10 {
                if (i | j) != 0 {
                    0
                } else {
                    128
                }
            } else {
                i32::from(PARTITION_SETS[partition as usize][i][j])
            };
            let mut index_bits: u32 = if mode >= 10 { 4 } else { 3 };
            if (partition_set & 0x80) != 0 {
                index_bits -= 1;
            }
            partition_set &= 0x01;
            let index = bstream.read_bits(index_bits) as i32;
            let ep = (partition_set * 2) as usize;
            let red = finish_unquantize(
                interpolate(r[ep] as i32, r[ep + 1] as i32, weights, index),
                is_signed,
            );
            let green = finish_unquantize(
                interpolate(g[ep] as i32, g[ep + 1] as i32, weights, index),
                is_signed,
            );
            let blue = finish_unquantize(
                interpolate(b[ep] as i32, b[ep + 1] as i32, weights, index),
                is_signed,
            );
            output[pixel + j] = [red, green, blue];
        }
        pixel += 4;
    }
}

fn decode_block_image(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
    is_signed: bool,
) -> Result<usize, TextureDecodeError> {
    require_output(output, width, height)?;
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);
    let required = blocks_x.saturating_mul(blocks_y).saturating_mul(16);
    require_input(input, required)?;
    let clen_last = (width + 3) % 4 + 1;
    let mut ushorts = [[0u16; 3]; 16];
    let mut buffer: Block = [[0, 0, 0, 0]; 16];
    let mut offset = 0;
    for t in 0..blocks_y {
        for s in 0..blocks_x {
            decode_bc6h_block(&input[offset..offset + 16], &mut ushorts, is_signed);
            for (index, pixel) in buffer.iter_mut().enumerate() {
                *pixel = [
                    ushort_to_byte(ushorts[index][0]),
                    ushort_to_byte(ushorts[index][1]),
                    ushort_to_byte(ushorts[index][2]),
                    0xFF,
                ];
            }
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
            offset += 16;
        }
    }
    Ok(offset)
}

/// `Bc6h.Decompress` with `isSigned = false`.
pub fn decode_bc6h(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_block_image(input, width, height, output, false)
}

/// `Bc6h.Decompress` with `isSigned = true`.
pub fn decode_bc6h_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_block_image(input, width, height, output, true)
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

    /// Byte-for-byte differential against `AssetRipper.TextureDecoder` (probe
    /// `Temp/opencode/bc6hprobe`), covering every BC6H mode for the unsigned path.
    #[test]
    fn bc6h_matches_the_reference_runtime() {
        let fixture = include_str!("bc6h_fixture.txt");
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
            let consumed = decode_bc6h(&input, width, height, &mut output).unwrap();
            assert_eq!(consumed, expected_consumed, "consumed {line}");
            assert_eq!(output, expected_output, "output {line}");
            rows += 1;
        }
        assert!(rows >= 15, "expected at least 15 BC6H rows, got {rows}");
    }
}
