//! BC7 decoder ported from the `AssetRipper.TextureDecoder` dependency used by
//! `AssetRipper.Export.Modules.Textures.TextureConverter`.
//!
//! `BcHelpers.DecompressBc7` reads a 128-bit little-endian bit stream, decodes the endpoints,
//! applies the shared P-bits, then reads the primary and (for modes 4/5) secondary index streams.
//! The partition shapes and weights live in `bc7_tables.rs`.
//!
//! Differential fixture: `bc7_fixture.txt` (probe `Temp/opencode/bc7probe`).

use super::bc7_tables::{
    ACTUAL_BITS_COUNT, MODE_HAS_PBITS, PARTITION_SETS, WEIGHT2, WEIGHT3, WEIGHT4,
};
use super::texture_decode::{require_input, require_output, TextureDecodeError};

type Pixel = [u8; 4];
type Block = [Pixel; 16];

struct BitStream {
    value: u128,
}

impl BitStream {
    fn new(input: &[u8]) -> Self {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&input[..16]);
        Self {
            value: u128::from_le_bytes(bytes),
        }
    }

    fn bottom_bits(&self) -> u32 {
        self.value as u32
    }

    fn peak_bits(&self, num_bits: u32) -> u32 {
        self.bottom_bits() & ((1u32 << num_bits) - 1)
    }

    fn advance(&mut self, num_bits: u32) {
        self.value >>= num_bits;
    }

    fn read_bits(&mut self, num_bits: u32) -> u32 {
        let bits = self.peak_bits(num_bits);
        self.advance(num_bits);
        bits
    }

    fn read_bit(&mut self) -> u32 {
        let result = self.bottom_bits() & 1;
        self.advance(1);
        result
    }
}

fn read_bc7_mode(bstream: &mut BitStream) -> u32 {
    let eight_bits = bstream.peak_bits(8);
    let zeros = eight_bits.trailing_zeros();
    let bit_count = if zeros >= 8 { 8 } else { zeros + 1 };
    bstream.advance(bit_count);
    zeros.min(8)
}

fn interpolate(a: u32, b: u32, weights: &[i32], index: i32) -> u32 {
    let weight = weights[index as usize];
    ((a as i32 * (64 - weight)) + (b as i32 * weight) + 32) as u32 >> 6
}

fn decode_bc7_block(input: &[u8], output: &mut Block) {
    let mut bstream = BitStream::new(input);
    let mut endpoints = [[0u32; 4]; 6];
    let mut indices = [[0i32; 4]; 4];

    let mode = read_bc7_mode(&mut bstream);
    if mode >= 8 {
        *output = [[0, 0, 0, 0]; 16];
        return;
    }

    let mut partition = 0u32;
    let mut num_partitions = 1u32;
    let mut rotation = 0u32;
    let mut index_selection_bit = 0u32;

    match mode {
        0 => {
            num_partitions = 3;
            partition = bstream.read_bits(4);
        }
        1 | 3 | 7 => {
            num_partitions = 2;
            partition = bstream.read_bits(6);
        }
        2 => {
            num_partitions = 3;
            partition = bstream.read_bits(6);
        }
        4 => {
            rotation = bstream.read_bits(2);
            index_selection_bit = bstream.read_bit();
        }
        5 => {
            rotation = bstream.read_bits(2);
        }
        _ => {}
    }

    let num_endpoints = (num_partitions * 2) as usize;

    for i in 0..3 {
        for endpoint in endpoints.iter_mut().take(num_endpoints) {
            endpoint[i] = bstream.read_bits(ACTUAL_BITS_COUNT[0][mode as usize]);
        }
    }
    if ACTUAL_BITS_COUNT[1][mode as usize] > 0 {
        for endpoint in endpoints.iter_mut().take(num_endpoints) {
            endpoint[3] = bstream.read_bits(ACTUAL_BITS_COUNT[1][mode as usize]);
        }
    }

    if mode == 0 || mode == 1 || mode == 3 || mode == 6 || mode == 7 {
        for endpoint in endpoints.iter_mut().take(num_endpoints) {
            for value in endpoint.iter_mut() {
                *value <<= 1;
            }
        }
        if mode == 1 {
            let i = bstream.read_bit();
            let j = bstream.read_bit();
            for value in &mut endpoints[0][..3] {
                *value |= i;
            }
            for value in &mut endpoints[1][..3] {
                *value |= i;
            }
            for value in &mut endpoints[2][..3] {
                *value |= j;
            }
            for value in &mut endpoints[3][..3] {
                *value |= j;
            }
        } else if (MODE_HAS_PBITS & (1 << mode)) != 0 {
            for endpoint in endpoints.iter_mut().take(num_endpoints) {
                let j = bstream.read_bit();
                for value in endpoint.iter_mut() {
                    *value |= j;
                }
            }
        }
    }

    for endpoint in endpoints.iter_mut().take(num_endpoints) {
        let rgb_bits = ACTUAL_BITS_COUNT[0][mode as usize] + ((MODE_HAS_PBITS >> mode) & 1);
        for value in &mut endpoint[..3] {
            *value <<= 8 - rgb_bits;
            *value |= *value >> rgb_bits;
        }
        let alpha_bits = ACTUAL_BITS_COUNT[1][mode as usize] + ((MODE_HAS_PBITS >> mode) & 1);
        endpoint[3] <<= 8 - alpha_bits;
        endpoint[3] |= endpoint[3] >> alpha_bits;
    }

    if ACTUAL_BITS_COUNT[1][mode as usize] == 0 {
        for endpoint in endpoints.iter_mut().take(num_endpoints) {
            endpoint[3] = 0xFF;
        }
    }

    let index_bits: u32 = match mode {
        0 | 1 => 3,
        6 => 4,
        _ => 2,
    };
    let index_bits2: u32 = match mode {
        4 => 3,
        5 => 2,
        _ => 0,
    };
    let weights: &[i32] = match index_bits {
        2 => &WEIGHT2,
        3 => &WEIGHT3,
        _ => &WEIGHT4,
    };
    let weights2: &[i32] = if index_bits2 == 2 { &WEIGHT2 } else { &WEIGHT3 };

    for i in 0..4usize {
        for j in 0..4usize {
            let partition_set = if num_partitions == 1 {
                if (i | j) != 0 {
                    0
                } else {
                    128
                }
            } else {
                PARTITION_SETS[(num_partitions - 2) as usize][partition as usize][i][j]
            };
            let mut bits = if mode == 0 || mode == 1 {
                3
            } else if mode == 6 {
                4
            } else {
                2
            };
            if (partition_set & 0x80) != 0 {
                bits -= 1;
            }
            indices[i][j] = bstream.read_bits(bits) as i32;
        }
    }

    for i in 0..4usize {
        for j in 0..4usize {
            let partition_set = if num_partitions == 1 {
                if (i | j) != 0 {
                    0
                } else {
                    128
                }
            } else {
                PARTITION_SETS[(num_partitions - 2) as usize][partition as usize][i][j]
            };
            let partition_set = (partition_set & 0x03) as usize;
            let index = indices[i][j];
            let (mut r, mut g, mut b, mut a);
            if index_bits2 == 0 {
                r = interpolate(
                    endpoints[partition_set * 2][0],
                    endpoints[partition_set * 2 + 1][0],
                    weights,
                    index,
                );
                g = interpolate(
                    endpoints[partition_set * 2][1],
                    endpoints[partition_set * 2 + 1][1],
                    weights,
                    index,
                );
                b = interpolate(
                    endpoints[partition_set * 2][2],
                    endpoints[partition_set * 2 + 1][2],
                    weights,
                    index,
                );
                a = interpolate(
                    endpoints[partition_set * 2][3],
                    endpoints[partition_set * 2 + 1][3],
                    weights,
                    index,
                );
            } else {
                let index2 = bstream.read_bits(if (i | j) != 0 {
                    index_bits2
                } else {
                    index_bits2 - 1
                }) as i32;
                if index_selection_bit == 0 {
                    r = interpolate(
                        endpoints[partition_set * 2][0],
                        endpoints[partition_set * 2 + 1][0],
                        weights,
                        index,
                    );
                    g = interpolate(
                        endpoints[partition_set * 2][1],
                        endpoints[partition_set * 2 + 1][1],
                        weights,
                        index,
                    );
                    b = interpolate(
                        endpoints[partition_set * 2][2],
                        endpoints[partition_set * 2 + 1][2],
                        weights,
                        index,
                    );
                    a = interpolate(
                        endpoints[partition_set * 2][3],
                        endpoints[partition_set * 2 + 1][3],
                        weights2,
                        index2,
                    );
                } else {
                    r = interpolate(
                        endpoints[partition_set * 2][0],
                        endpoints[partition_set * 2 + 1][0],
                        weights2,
                        index2,
                    );
                    g = interpolate(
                        endpoints[partition_set * 2][1],
                        endpoints[partition_set * 2 + 1][1],
                        weights2,
                        index2,
                    );
                    b = interpolate(
                        endpoints[partition_set * 2][2],
                        endpoints[partition_set * 2 + 1][2],
                        weights2,
                        index2,
                    );
                    a = interpolate(
                        endpoints[partition_set * 2][3],
                        endpoints[partition_set * 2 + 1][3],
                        weights,
                        index,
                    );
                }
            }

            match rotation {
                1 => core::mem::swap(&mut a, &mut r),
                2 => core::mem::swap(&mut a, &mut g),
                3 => core::mem::swap(&mut a, &mut b),
                _ => {}
            }

            output[i * 4 + j] = [r as u8, g as u8, b as u8, a as u8];
        }
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

/// `Bc7.Decompress`.
pub fn decode_bc7(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_block_image(input, width, height, output, 16, decode_bc7_block)
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
    /// `Temp/opencode/bc7probe`), covering every BC7 mode.
    #[test]
    fn bc7_matches_the_reference_runtime() {
        let fixture = include_str!("bc7_fixture.txt");
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
            let consumed = decode_bc7(&input, width, height, &mut output).unwrap();
            assert_eq!(consumed, expected_consumed, "consumed {line}");
            assert_eq!(output, expected_output, "output {line}");
            rows += 1;
        }
        assert!(rows >= 12, "expected at least 12 BC7 rows, got {rows}");
    }
}
