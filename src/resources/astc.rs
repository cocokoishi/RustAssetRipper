//! ASTC decoder ported from the `AssetRipper.TextureDecoder` dependency used by
//! `AssetRipper.Export.Modules.Textures.TextureConverter`.
//!
//! `AstcDecoder.DecodeASTC` decodes a configurable block grid (4x4 through 12x12) to RGBA8. The
//! reference decodes the void-extent block (`0xfc`/odd) as a solid colour and otherwise walks the
//! block parameters, endpoints, weights, and partition selection. The lookup tables live in
//! `astc_tables.rs`.
//!
//! Differential fixture: `astc_fixture.txt` (probe `Temp/opencode/astcprobe`).

#![allow(clippy::needless_range_loop)]

use super::astc_tables::{
    BIT_REVERSE, CEM_TABLE_A, CEM_TABLE_B, DE_QUINTS_TABLE, DE_TRITS_TABLE, DI_QUINTS_TABLE,
    DI_TRITS_TABLE, D_IMQ, D_IMT, WEIGHT_PREC_TABLE_A, WEIGHT_PREC_TABLE_B,
};
use super::texture_decode::{require_input, require_output, TextureDecodeError};

type Pixel = [u8; 4];
const MAX_BLOCK_PIXELS: usize = 144;

#[derive(Clone, Copy, Default)]
struct IntSeqData {
    bits: i32,
    nonbits: i32,
}

struct BlockData {
    bw: i32,
    bh: i32,
    width: i32,
    height: i32,
    part_num: i32,
    dual_plane: i32,
    plane_selector: i32,
    weight_range: i32,
    weight_num: i32,
    cem: [i32; 4],
    cem_range: usize,
    endpoint_value_num: i32,
    endpoints: [[i32; 8]; 4],
    weights: [i32; 288],
    partition: [i32; 144],
}

impl Default for BlockData {
    fn default() -> Self {
        Self {
            bw: 0,
            bh: 0,
            width: 0,
            height: 0,
            part_num: 0,
            dual_plane: 0,
            plane_selector: 0,
            weight_range: 0,
            weight_num: 0,
            cem: [0; 4],
            cem_range: 0,
            endpoint_value_num: 0,
            endpoints: [[0; 8]; 4],
            weights: [0; 288],
            partition: [0; 144],
        }
    }
}

fn read_i32_le(input: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ])
}

fn read_u64_le(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
        input[offset + 4],
        input[offset + 5],
        input[offset + 6],
        input[offset + 7],
    ])
}

fn get_bits(input: &[u8], bit: i32, len: i32) -> i32 {
    if bit < 0 || (input.len() * 8) < (bit + len) as usize {
        return 0;
    }
    let slice = &input[(bit / 8) as usize..];
    let bit_buffer = if slice.len() >= 4 {
        read_i32_le(slice, 0)
    } else {
        let mut temp = [0u8; 4];
        temp[..slice.len()].copy_from_slice(slice);
        i32::from_le_bytes(temp)
    };
    (bit_buffer >> (bit % 8)) & ((1 << len) - 1)
}

fn get_bits64(input: &[u8], bit: i32, len: i32) -> u64 {
    let mask = if len == 64 {
        u64::MAX
    } else {
        (1u64 << len) - 1
    };
    if len < 1 {
        0
    } else if bit >= 64 {
        read_u64_le(input, 8) >> (bit - 64) & mask
    } else if bit <= 0 {
        read_u64_le(input, 0) << (-bit) & mask
    } else if bit + len <= 64 {
        read_u64_le(input, 0) >> bit & mask
    } else {
        (read_u64_le(input, 0) >> bit | read_u64_le(input, 8) << (64 - bit)) & mask
    }
}

fn bit_reverse_u8(c: u8, bits: i32) -> i32 {
    i32::from(BIT_REVERSE[c as usize] >> (8 - bits))
}

fn bit_reverse_u64(d: u64, bits: i32) -> u64 {
    let ret = (u64::from(BIT_REVERSE[(d & 0xff) as usize])) << 56
        | (u64::from(BIT_REVERSE[(d >> 8 & 0xff) as usize])) << 48
        | (u64::from(BIT_REVERSE[(d >> 16 & 0xff) as usize])) << 40
        | (u64::from(BIT_REVERSE[(d >> 24 & 0xff) as usize])) << 32
        | (u64::from(BIT_REVERSE[(d >> 32 & 0xff) as usize])) << 24
        | (u64::from(BIT_REVERSE[(d >> 40 & 0xff) as usize])) << 16
        | (u64::from(BIT_REVERSE[(d >> 48 & 0xff) as usize])) << 8
        | u64::from(BIT_REVERSE[(d >> 56 & 0xff) as usize]);
    ret >> (64 - bits)
}

fn clamp255(n: i32) -> i32 {
    n.clamp(0, 255)
}

fn bit_transfer_signed(a: &mut i32, b: &mut i32) {
    *b = (*b >> 1) | (*a & 0x80);
    *a = (*a >> 1) & 0x3f;
    if (*a & 0x20) != 0 {
        *a -= 0x40;
    }
}

#[allow(clippy::too_many_arguments)]
fn set_endpoint(
    endpoint: &mut [i32; 8],
    r1: i32,
    g1: i32,
    b1: i32,
    a1: i32,
    r2: i32,
    g2: i32,
    b2: i32,
    a2: i32,
) {
    endpoint[0] = r1;
    endpoint[1] = g1;
    endpoint[2] = b1;
    endpoint[3] = a1;
    endpoint[4] = r2;
    endpoint[5] = g2;
    endpoint[6] = b2;
    endpoint[7] = a2;
}

#[allow(clippy::too_many_arguments)]
fn set_endpoint_clamp(
    endpoint: &mut [i32; 8],
    r1: i32,
    g1: i32,
    b1: i32,
    a1: i32,
    r2: i32,
    g2: i32,
    b2: i32,
    a2: i32,
) {
    set_endpoint(
        endpoint,
        clamp255(r1),
        clamp255(g1),
        clamp255(b1),
        clamp255(a1),
        clamp255(r2),
        clamp255(g2),
        clamp255(b2),
        clamp255(a2),
    );
}

#[allow(clippy::too_many_arguments)]
fn set_endpoint_blue(
    endpoint: &mut [i32; 8],
    r1: i32,
    g1: i32,
    b1: i32,
    a1: i32,
    r2: i32,
    g2: i32,
    b2: i32,
    a2: i32,
) {
    set_endpoint(
        endpoint,
        (r1 + b1) >> 1,
        (g1 + b1) >> 1,
        b1,
        a1,
        (r2 + b2) >> 1,
        (g2 + b2) >> 1,
        b2,
        a2,
    );
}

#[allow(clippy::too_many_arguments)]
fn set_endpoint_blue_clamp(
    endpoint: &mut [i32; 8],
    r1: i32,
    g1: i32,
    b1: i32,
    a1: i32,
    r2: i32,
    g2: i32,
    b2: i32,
    a2: i32,
) {
    set_endpoint_clamp(
        endpoint,
        (r1 + b1) >> 1,
        (g1 + b1) >> 1,
        b1,
        a1,
        (r2 + b2) >> 1,
        (g2 + b2) >> 1,
        b2,
        a2,
    );
}

fn select_color(v0: i32, v1: i32, weight: i32) -> u8 {
    ((((((v0 << 8 | v0) * (64 - weight) + (v1 << 8 | v1) * weight + 32) >> 6) * 255 + 32768)
        / 65536) as u8)
}

#[allow(clippy::too_many_lines)]
fn decode_block_parameters(input: &[u8], p: &mut BlockData) {
    p.dual_plane = (i32::from(input[1]) & 4) >> 2;
    p.weight_range = (i32::from(input[0]) >> 4 & 1) | (i32::from(input[1]) << 2 & 8);

    if (input[0] & 3) != 0 {
        p.weight_range |= i32::from(input[0]) << 1 & 6;
        match input[0] & 0xc {
            0 => {
                p.width = (read_i32_le(input, 0) >> 7 & 3) + 4;
                p.height = (i32::from(input[0]) >> 5 & 3) + 2;
            }
            4 => {
                p.width = (read_i32_le(input, 0) >> 7 & 3) + 8;
                p.height = (i32::from(input[0]) >> 5 & 3) + 2;
            }
            8 => {
                p.width = (i32::from(input[0]) >> 5 & 3) + 2;
                p.height = (read_i32_le(input, 0) >> 7 & 3) + 8;
            }
            _ => {
                if (input[1] & 1) != 0 {
                    p.width = (i32::from(input[0]) >> 7 & 1) + 2;
                    p.height = (i32::from(input[0]) >> 5 & 3) + 2;
                } else {
                    p.width = (i32::from(input[0]) >> 5 & 3) + 2;
                    p.height = (i32::from(input[0]) >> 7 & 1) + 6;
                }
            }
        }
    } else {
        p.weight_range |= i32::from(input[0]) >> 1 & 6;
        match read_i32_le(input, 0) & 0x180 {
            0 => {
                p.width = 12;
                p.height = (i32::from(input[0]) >> 5 & 3) + 2;
            }
            0x80 => {
                p.width = (i32::from(input[0]) >> 5 & 3) + 2;
                p.height = 12;
            }
            0x100 => {
                p.width = (i32::from(input[0]) >> 5 & 3) + 6;
                p.height = (i32::from(input[1]) >> 1 & 3) + 6;
                p.dual_plane = 0;
                p.weight_range &= 7;
            }
            _ => {
                p.width = if (input[0] & 0x20) != 0 { 10 } else { 6 };
                p.height = if (input[0] & 0x20) != 0 { 6 } else { 10 };
            }
        }
    }

    p.part_num = (i32::from(input[1]) >> 3 & 3) + 1;
    p.weight_num = p.width * p.height;
    if p.dual_plane != 0 {
        p.weight_num *= 2;
    }

    let mut cem_base = 0;
    let config_bits;
    let weight_bits = match WEIGHT_PREC_TABLE_A[p.weight_range as usize] {
        3 => {
            p.weight_num * WEIGHT_PREC_TABLE_B[p.weight_range as usize] + (p.weight_num * 8 + 4) / 5
        }
        5 => {
            p.weight_num * WEIGHT_PREC_TABLE_B[p.weight_range as usize] + (p.weight_num * 7 + 2) / 3
        }
        _ => p.weight_num * WEIGHT_PREC_TABLE_B[p.weight_range as usize],
    };
    if p.part_num == 1 {
        p.cem[0] = read_i32_le(input, 1) >> 5 & 0xf;
        config_bits = 17;
    } else {
        cem_base = read_i32_le(input, 2) >> 7 & 3;
        if cem_base == 0 {
            let cem = i32::from(input[3]) >> 1 & 0xf;
            for i in 0..p.part_num as usize {
                p.cem[i] = cem;
            }
            config_bits = 29;
        } else {
            for i in 0..p.part_num as usize {
                p.cem[i] = ((i32::from(input[3]) >> (i + 1) & 1) + cem_base - 1) << 2;
            }
            match p.part_num {
                2 => {
                    p.cem[0] |= i32::from(input[3]) >> 3 & 3;
                    p.cem[1] |= get_bits(input, 126 - weight_bits, 2);
                }
                3 => {
                    p.cem[0] |= i32::from(input[3]) >> 4 & 1;
                    p.cem[0] |= get_bits(input, 122 - weight_bits, 2) & 2;
                    p.cem[1] |= get_bits(input, 124 - weight_bits, 2);
                    p.cem[2] |= get_bits(input, 126 - weight_bits, 2);
                }
                _ => {
                    for i in 0..4 {
                        p.cem[i] |= get_bits(input, 120 + (i as i32) * 2 - weight_bits, 2);
                    }
                }
            }
            config_bits = 25 + p.part_num * 3;
        }
    }

    if p.dual_plane != 0 {
        p.plane_selector = get_bits(
            input,
            if cem_base != 0 {
                130 - weight_bits - p.part_num * 3
            } else {
                126 - weight_bits
            },
            2,
        );
    }

    let remain_bits = 128 - config_bits - weight_bits;
    p.endpoint_value_num = 0;
    for i in 0..p.part_num as usize {
        p.endpoint_value_num += (p.cem[i] >> 1 & 6) + 2;
    }

    for i in 0..CEM_TABLE_A.len() {
        let endpoint_bits = match CEM_TABLE_A[i] {
            3 => p.endpoint_value_num * CEM_TABLE_B[i] + (p.endpoint_value_num * 8 + 4) / 5,
            5 => p.endpoint_value_num * CEM_TABLE_B[i] + (p.endpoint_value_num * 7 + 2) / 3,
            _ => p.endpoint_value_num * CEM_TABLE_B[i],
        };
        if endpoint_bits <= remain_bits {
            p.cem_range = i;
            break;
        }
    }
}

fn decode_intseq(
    input: &[u8],
    offset: i32,
    a: i32,
    b: i32,
    count: i32,
    reverse: bool,
    out: &mut [IntSeqData],
) {
    if count <= 0 {
        return;
    }
    let mut n = 0usize;
    if a == 3 {
        let mask = (1 << b) - 1;
        let block_count = (count + 4) / 5;
        let last_block_count = (count + 4) % 5 + 1;
        let block_size = 8 + 5 * b;
        let last_block_size = (block_size * last_block_count + 4) / 5;
        let mut p = offset;
        for i in 0..block_count {
            let now_size = if i < block_count - 1 {
                block_size
            } else {
                last_block_size
            };
            let d = if reverse {
                bit_reverse_u64(get_bits64(input, p - now_size, now_size), now_size)
            } else {
                get_bits64(input, p, now_size)
            };
            let x = ((d >> (b as u32) & 3)
                | (d >> ((b * 2) as u32) & 0xc)
                | (d >> ((b * 3) as u32) & 0x10)
                | (d >> ((b * 4) as u32) & 0x60)
                | (d >> ((b * 5) as u32) & 0x80)) as usize;
            let mut j = 0;
            while j < 5 && n < count as usize {
                let bits = (d >> ((D_IMT[j] + b * (j as i32)) as u32)) as u32 as i32 & mask;
                out[n] = IntSeqData {
                    bits,
                    nonbits: DI_TRITS_TABLE[j * 256 + x],
                };
                j += 1;
                n += 1;
            }
            p += if reverse { -block_size } else { block_size };
        }
    } else if a == 5 {
        let mask = (1 << b) - 1;
        let block_count = (count + 2) / 3;
        let last_block_count = (count + 2) % 3 + 1;
        let block_size = 7 + 3 * b;
        let last_block_size = (block_size * last_block_count + 2) / 3;
        let mut p = offset;
        for i in 0..block_count {
            let now_size = if i < block_count - 1 {
                block_size
            } else {
                last_block_size
            };
            let d = if reverse {
                bit_reverse_u64(get_bits64(input, p - now_size, now_size), now_size)
            } else {
                get_bits64(input, p, now_size)
            };
            let x = ((d >> (b as u32) & 7)
                | (d >> ((b * 2) as u32) & 0x18)
                | (d >> ((b * 3) as u32) & 0x60)) as usize;
            let mut j = 0;
            while j < 3 && n < count as usize {
                let bits = (d as u32 as i32) >> ((D_IMQ[j] + b * (j as i32)) as u32) & mask;
                out[n] = IntSeqData {
                    bits,
                    nonbits: DI_QUINTS_TABLE[j * 128 + x],
                };
                j += 1;
                n += 1;
            }
            p += if reverse { -block_size } else { block_size };
        }
    } else if reverse {
        let mut p = offset - b;
        while n < count as usize {
            out[n] = IntSeqData {
                bits: bit_reverse_u8(get_bits(input, p, b) as u8, b),
                nonbits: 0,
            };
            n += 1;
            p -= b;
        }
    } else {
        let mut p = offset;
        while n < count as usize {
            out[n] = IntSeqData {
                bits: get_bits(input, p, b),
                nonbits: 0,
            };
            n += 1;
            p += b;
        }
    }
}

#[allow(clippy::too_many_lines)]
fn decode_endpoints(input: &[u8], p: &mut BlockData) {
    let mut ep_seq = [IntSeqData::default(); 32];
    decode_intseq(
        input,
        if p.part_num == 1 { 17 } else { 29 },
        CEM_TABLE_A[p.cem_range],
        CEM_TABLE_B[p.cem_range],
        p.endpoint_value_num,
        false,
        &mut ep_seq,
    );

    let mut ev = [0i32; 32];
    match CEM_TABLE_A[p.cem_range] {
        3 => {
            let c = DE_TRITS_TABLE[CEM_TABLE_B[p.cem_range] as usize];
            for i in 0..p.endpoint_value_num as usize {
                let a = (ep_seq[i].bits & 1) * 0x1ff;
                let x = ep_seq[i].bits >> 1;
                let b = match CEM_TABLE_B[p.cem_range] {
                    1 => 0,
                    2 => 0b100010110 * x,
                    3 => x << 7 | x << 2 | x,
                    4 => x << 6 | x,
                    5 => x << 5 | x >> 2,
                    _ => x << 4 | x >> 4,
                };
                ev[i] = (a & 0x80) | ((ep_seq[i].nonbits * c + b) ^ a) >> 2;
            }
        }
        5 => {
            let c = DE_QUINTS_TABLE[CEM_TABLE_B[p.cem_range] as usize];
            for i in 0..p.endpoint_value_num as usize {
                let a = (ep_seq[i].bits & 1) * 0x1ff;
                let x = ep_seq[i].bits >> 1;
                let b = match CEM_TABLE_B[p.cem_range] {
                    1 => 0,
                    2 => 0b100001100 * x,
                    3 => x << 7 | x << 1 | x >> 1,
                    4 => x << 6 | x >> 1,
                    _ => x << 5 | x >> 3,
                };
                ev[i] = (a & 0x80) | ((ep_seq[i].nonbits * c + b) ^ a) >> 2;
            }
        }
        _ => match CEM_TABLE_B[p.cem_range] {
            1 => {
                for i in 0..p.endpoint_value_num as usize {
                    ev[i] = ep_seq[i].bits * 0xff;
                }
            }
            2 => {
                for i in 0..p.endpoint_value_num as usize {
                    ev[i] = ep_seq[i].bits * 0x55;
                }
            }
            3 => {
                for i in 0..p.endpoint_value_num as usize {
                    ev[i] = ep_seq[i].bits << 5 | ep_seq[i].bits << 2 | ep_seq[i].bits >> 1;
                }
            }
            4 => {
                for i in 0..p.endpoint_value_num as usize {
                    ev[i] = ep_seq[i].bits << 4 | ep_seq[i].bits;
                }
            }
            5 => {
                for i in 0..p.endpoint_value_num as usize {
                    ev[i] = ep_seq[i].bits << 3 | ep_seq[i].bits >> 2;
                }
            }
            6 => {
                for i in 0..p.endpoint_value_num as usize {
                    ev[i] = ep_seq[i].bits << 2 | ep_seq[i].bits >> 4;
                }
            }
            7 => {
                for i in 0..p.endpoint_value_num as usize {
                    ev[i] = ep_seq[i].bits << 1 | ep_seq[i].bits >> 6;
                }
            }
            _ => {
                for i in 0..p.endpoint_value_num as usize {
                    ev[i] = ep_seq[i].bits;
                }
            }
        },
    }

    let mut v_off = 0usize;
    for cem in 0..p.part_num as usize {
        let v0 = ev[v_off];
        let v1 = ev[v_off + 1];
        let v2 = ev[v_off + 2];
        let v3 = ev[v_off + 3];
        let v4 = ev[v_off + 4];
        let v5 = ev[v_off + 5];
        let v6 = ev[v_off + 6];
        let v7 = ev[v_off + 7];
        match p.cem[cem] {
            0 => set_endpoint(&mut p.endpoints[cem], v0, v0, v0, 255, v1, v1, v1, 255),
            1 => {
                let l0 = (v0 >> 2) | (v1 & 0xc0);
                let l1 = clamp255(l0 + (v1 & 0x3f));
                set_endpoint(&mut p.endpoints[cem], l0, l0, l0, 255, l1, l1, l1, 255);
            }
            4 => set_endpoint(&mut p.endpoints[cem], v0, v0, v0, v2, v1, v1, v1, v3),
            5 => {
                let (mut a0, mut b0) = (v0, v1);
                let (mut a2, mut b2) = (v2, v3);
                bit_transfer_signed(&mut a0, &mut b0);
                bit_transfer_signed(&mut a2, &mut b2);
                let b0 = b0 + a0;
                set_endpoint_clamp(&mut p.endpoints[cem], a0, a0, a0, a2, b0, b0, b0, a2 + b2);
            }
            6 => set_endpoint(
                &mut p.endpoints[cem],
                (v0 * v3) >> 8,
                (v1 * v3) >> 8,
                (v2 * v3) >> 8,
                255,
                v0,
                v1,
                v2,
                255,
            ),
            8 => {
                if v0 + v2 + v4 <= v1 + v3 + v5 {
                    set_endpoint(&mut p.endpoints[cem], v0, v2, v4, 255, v1, v3, v5, 255);
                } else {
                    set_endpoint_blue(&mut p.endpoints[cem], v1, v3, v5, 255, v0, v2, v4, 255);
                }
            }
            9 => {
                let (mut a0, mut b0) = (v0, v1);
                let (mut a2, mut b2) = (v2, v3);
                let (mut a4, mut b4) = (v4, v5);
                bit_transfer_signed(&mut a0, &mut b0);
                bit_transfer_signed(&mut a2, &mut b2);
                bit_transfer_signed(&mut a4, &mut b4);
                if b0 + b2 + b4 >= 0 {
                    set_endpoint_clamp(
                        &mut p.endpoints[cem],
                        a0,
                        a2,
                        a4,
                        255,
                        a0 + b0,
                        a2 + b2,
                        a4 + b4,
                        255,
                    );
                } else {
                    set_endpoint_blue_clamp(
                        &mut p.endpoints[cem],
                        a0 + b0,
                        a2 + b2,
                        a4 + b4,
                        255,
                        a0,
                        a2,
                        a4,
                        255,
                    );
                }
            }
            10 => set_endpoint(
                &mut p.endpoints[cem],
                (v0 * v3) >> 8,
                (v1 * v3) >> 8,
                (v2 * v3) >> 8,
                v4,
                v0,
                v1,
                v2,
                v5,
            ),
            12 => {
                if v0 + v2 + v4 <= v1 + v3 + v5 {
                    set_endpoint(&mut p.endpoints[cem], v0, v2, v4, v6, v1, v3, v5, v7);
                } else {
                    set_endpoint_blue(&mut p.endpoints[cem], v1, v3, v5, v7, v0, v2, v4, v6);
                }
            }
            13 => {
                let (mut a0, mut b0) = (v0, v1);
                let (mut a2, mut b2) = (v2, v3);
                let (mut a4, mut b4) = (v4, v5);
                let (mut a6, mut b6) = (v6, v7);
                bit_transfer_signed(&mut a0, &mut b0);
                bit_transfer_signed(&mut a2, &mut b2);
                bit_transfer_signed(&mut a4, &mut b4);
                bit_transfer_signed(&mut a6, &mut b6);
                if b0 + b2 + b4 >= 0 {
                    set_endpoint_clamp(
                        &mut p.endpoints[cem],
                        a0,
                        a2,
                        a4,
                        a6,
                        a0 + b0,
                        a2 + b2,
                        a4 + b4,
                        a6 + b6,
                    );
                } else {
                    set_endpoint_blue_clamp(
                        &mut p.endpoints[cem],
                        a0 + b0,
                        a2 + b2,
                        a4 + b4,
                        a6 + b6,
                        a0,
                        a2,
                        a4,
                        a6,
                    );
                }
            }
            _ => {}
        }
        v_off += ((p.cem[cem] / 4 + 1) * 2) as usize;
    }
}

#[allow(clippy::too_many_lines)]
fn decode_weights(input: &[u8], p: &mut BlockData) {
    let mut w_seq = [IntSeqData::default(); 128];
    decode_intseq(
        input,
        128,
        WEIGHT_PREC_TABLE_A[p.weight_range as usize],
        WEIGHT_PREC_TABLE_B[p.weight_range as usize],
        p.weight_num,
        true,
        &mut w_seq,
    );

    let mut wv = [0i32; 128];
    let a_prec = WEIGHT_PREC_TABLE_A[p.weight_range as usize];
    let b_prec = WEIGHT_PREC_TABLE_B[p.weight_range as usize];
    if a_prec == 0 {
        match b_prec {
            1 => {
                for i in 0..p.weight_num as usize {
                    wv[i] = if w_seq[i].bits != 0 { 63 } else { 0 };
                }
            }
            2 => {
                for i in 0..p.weight_num as usize {
                    wv[i] = w_seq[i].bits << 4 | w_seq[i].bits << 2 | w_seq[i].bits;
                }
            }
            3 => {
                for i in 0..p.weight_num as usize {
                    wv[i] = w_seq[i].bits << 3 | w_seq[i].bits;
                }
            }
            4 => {
                for i in 0..p.weight_num as usize {
                    wv[i] = w_seq[i].bits << 2 | w_seq[i].bits >> 2;
                }
            }
            _ => {
                for i in 0..p.weight_num as usize {
                    wv[i] = w_seq[i].bits << 1 | w_seq[i].bits >> 4;
                }
            }
        }
        for i in 0..p.weight_num as usize {
            if wv[i] > 32 {
                wv[i] += 1;
            }
        }
    } else if b_prec == 0 {
        let s = if a_prec == 3 { 32 } else { 16 };
        for i in 0..p.weight_num as usize {
            wv[i] = w_seq[i].nonbits * s;
        }
    } else {
        if a_prec == 3 {
            match b_prec {
                1 => {
                    for i in 0..p.weight_num as usize {
                        wv[i] = w_seq[i].nonbits * 50;
                    }
                }
                2 => {
                    for i in 0..p.weight_num as usize {
                        wv[i] = w_seq[i].nonbits * 23;
                        if (w_seq[i].bits & 2) != 0 {
                            wv[i] += 0b1000101;
                        }
                    }
                }
                _ => {
                    for i in 0..p.weight_num as usize {
                        wv[i] = w_seq[i].nonbits * 11
                            + ((w_seq[i].bits << 4 | w_seq[i].bits >> 1) & 0b1100011);
                    }
                }
            }
        } else if a_prec == 5 {
            match b_prec {
                1 => {
                    for i in 0..p.weight_num as usize {
                        wv[i] = w_seq[i].nonbits * 28;
                    }
                }
                _ => {
                    for i in 0..p.weight_num as usize {
                        wv[i] = w_seq[i].nonbits * 13;
                        if (w_seq[i].bits & 2) != 0 {
                            wv[i] += 0b1000010;
                        }
                    }
                }
            }
        }
        for i in 0..p.weight_num as usize {
            let a = (w_seq[i].bits & 1) * 0x7f;
            wv[i] = (a & 0x20) | ((wv[i] ^ a) >> 2);
            if wv[i] > 32 {
                wv[i] += 1;
            }
        }
    }

    let ds = (1024 + p.bw / 2) / (p.bw - 1);
    let dt = (1024 + p.bh / 2) / (p.bh - 1);
    let pn = if p.dual_plane != 0 { 2 } else { 1 };

    let mut i = 0;
    for t in 0..p.bh {
        for s in 0..p.bw {
            let gs = (ds * s * (p.width - 1) + 32) >> 6;
            let gt = (dt * t * (p.height - 1) + 32) >> 6;
            let fs = gs & 0xf;
            let ft = gt & 0xf;
            let v = (gs >> 4) + (gt >> 4) * p.width;
            let w11 = (fs * ft + 8) >> 4;
            let w10 = ft - w11;
            let w01 = fs - w11;
            let w00 = 16 - fs - ft + w11;
            for q in 0..pn {
                let p00 = wv[(v * pn + q) as usize];
                let p01 = wv[((v + 1) * pn + q) as usize];
                let p10 = wv[((v + p.width) * pn + q) as usize];
                let p11 = wv[((v + p.width + 1) * pn + q) as usize];
                p.weights[(i * 2 + q) as usize] =
                    (p00 * w00 + p01 * w01 + p10 * w10 + p11 * w11 + 8) >> 4;
            }
            i += 1;
        }
    }
}

#[allow(clippy::too_many_lines)]
fn select_partition(input: &[u8], p: &mut BlockData) {
    let small_block = p.bw * p.bh < 31;
    let seed = (read_i32_le(input, 0) >> 13 & 0x3ff) | (p.part_num - 1) << 10;

    let mut rnum = seed as u32;
    rnum ^= rnum >> 15;
    rnum = rnum.wrapping_sub(rnum << 17);
    rnum = rnum.wrapping_add(rnum << 7);
    rnum = rnum.wrapping_add(rnum << 4);
    rnum ^= rnum >> 5;
    rnum = rnum.wrapping_add(rnum << 16);
    rnum ^= rnum >> 7;
    rnum ^= rnum >> 3;
    rnum ^= rnum << 6;
    rnum ^= rnum >> 17;

    let mut seeds = [0i32; 8];
    for i in 0..8 {
        seeds[i] = ((rnum >> (i * 4)) & 0xF) as i32;
        seeds[i] *= seeds[i];
    }

    let sh = [
        if (seed & 2) != 0 { 4 } else { 5 },
        if p.part_num == 3 { 6 } else { 5 },
    ];
    if (seed & 1) != 0 {
        for i in 0..8 {
            seeds[i] >>= sh[i % 2];
        }
    } else {
        for i in 0..8 {
            seeds[i] >>= sh[1 - i % 2];
        }
    }

    let mut i = 0;
    for t in 0..p.bh {
        for s in 0..p.bw {
            let (x, y) = if small_block {
                (s << 1, t << 1)
            } else {
                (s, t)
            };
            let a = (seeds[0] * x + seeds[1] * y + ((rnum >> 14) as i32)) & 0x3f;
            let b = (seeds[2] * x + seeds[3] * y + ((rnum >> 10) as i32)) & 0x3f;
            let c = if p.part_num < 3 {
                0
            } else {
                (seeds[4] * x + seeds[5] * y + ((rnum >> 6) as i32)) & 0x3f
            };
            let d = if p.part_num < 4 {
                0
            } else {
                (seeds[6] * x + seeds[7] * y + ((rnum >> 2) as i32)) & 0x3f
            };
            p.partition[i] = if a >= b && a >= c && a >= d {
                0
            } else if b >= c && b >= d {
                1
            } else if c >= d {
                2
            } else {
                3
            };
            i += 1;
        }
    }
}

fn applicate_color(p: &BlockData, output: &mut [Pixel; MAX_BLOCK_PIXELS]) {
    let n = (p.bw * p.bh) as usize;
    if p.dual_plane != 0 {
        let mut ps = [0usize; 4];
        ps[p.plane_selector as usize] = 1;
        if p.part_num > 1 {
            for i in 0..n {
                let part = p.partition[i] as usize;
                output[i] = [
                    select_color(
                        p.endpoints[part][0],
                        p.endpoints[part][4],
                        p.weights[i * 2 + ps[0]],
                    ),
                    select_color(
                        p.endpoints[part][1],
                        p.endpoints[part][5],
                        p.weights[i * 2 + ps[1]],
                    ),
                    select_color(
                        p.endpoints[part][2],
                        p.endpoints[part][6],
                        p.weights[i * 2 + ps[2]],
                    ),
                    select_color(
                        p.endpoints[part][3],
                        p.endpoints[part][7],
                        p.weights[i * 2 + ps[3]],
                    ),
                ];
            }
        } else {
            for i in 0..n {
                output[i] = [
                    select_color(
                        p.endpoints[0][0],
                        p.endpoints[0][4],
                        p.weights[i * 2 + ps[0]],
                    ),
                    select_color(
                        p.endpoints[0][1],
                        p.endpoints[0][5],
                        p.weights[i * 2 + ps[1]],
                    ),
                    select_color(
                        p.endpoints[0][2],
                        p.endpoints[0][6],
                        p.weights[i * 2 + ps[2]],
                    ),
                    select_color(
                        p.endpoints[0][3],
                        p.endpoints[0][7],
                        p.weights[i * 2 + ps[3]],
                    ),
                ];
            }
        }
    } else if p.part_num > 1 {
        for i in 0..n {
            let part = p.partition[i] as usize;
            output[i] = [
                select_color(p.endpoints[part][0], p.endpoints[part][4], p.weights[i * 2]),
                select_color(p.endpoints[part][1], p.endpoints[part][5], p.weights[i * 2]),
                select_color(p.endpoints[part][2], p.endpoints[part][6], p.weights[i * 2]),
                select_color(p.endpoints[part][3], p.endpoints[part][7], p.weights[i * 2]),
            ];
        }
    } else {
        for i in 0..n {
            output[i] = [
                select_color(p.endpoints[0][0], p.endpoints[0][4], p.weights[i * 2]),
                select_color(p.endpoints[0][1], p.endpoints[0][5], p.weights[i * 2]),
                select_color(p.endpoints[0][2], p.endpoints[0][6], p.weights[i * 2]),
                select_color(p.endpoints[0][3], p.endpoints[0][7], p.weights[i * 2]),
            ];
        }
    }
}

fn decode_block(
    input: &[u8],
    block_width: i32,
    block_height: i32,
    output: &mut [Pixel; MAX_BLOCK_PIXELS],
) {
    if input[0] == 0xfc && (input[1] & 1) == 1 {
        let color = [input[9], input[11], input[13], input[15]];
        let n = (block_width * block_height) as usize;
        for pixel in output[..n].iter_mut() {
            *pixel = color;
        }
    } else {
        let mut block = BlockData::default();
        block.bw = block_width;
        block.bh = block_height;
        decode_block_parameters(input, &mut block);
        decode_endpoints(input, &mut block);
        decode_weights(input, &mut block);
        if block.part_num > 1 {
            select_partition(input, &mut block);
        }
        applicate_color(&block, output);
    }
}

/// `AstcDecoder.DecodeASTC`.
pub fn decode_astc(
    input: &[u8],
    width: usize,
    height: usize,
    block_width: usize,
    block_height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    if !(4..=12).contains(&block_width) || !(4..=12).contains(&block_height) {
        return Err(TextureDecodeError::InputTooShort {
            required: block_width,
            actual: block_height,
        });
    }
    require_output(output, width, height)?;
    let bw = block_width as i32;
    let bh = block_height as i32;
    let bcw = (width + block_width - 1) / block_width;
    let bch = (height + block_height - 1) / block_height;
    let clen_last = (width + block_width - 1) % block_width + 1;
    let required = bcw.saturating_mul(bch).saturating_mul(16);
    require_input(input, required)?;
    let mut buffer: [Pixel; MAX_BLOCK_PIXELS] = [[0, 0, 0, 0]; MAX_BLOCK_PIXELS];
    let mut input_offset = 0;
    for t in 0..bch {
        for s in 0..bcw {
            decode_block(&input[input_offset..], bw, bh, &mut buffer);
            let clen = if s < bcw - 1 { block_width } else { clen_last };
            for i in 0..block_height {
                let y = t * block_height + i;
                if y >= height {
                    break;
                }
                let output_base = t * block_height * width + s * block_width + i * width;
                for j in 0..clen {
                    let dst = output_base + j;
                    output[dst * 4..dst * 4 + 4].copy_from_slice(&buffer[j + i * block_width]);
                }
            }
            input_offset += 16;
        }
    }
    Ok(input_offset)
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
    /// `Temp/opencode/astcprobe`).
    #[test]
    fn astc_matches_the_reference_runtime() {
        let fixture = include_str!("astc_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            let block_width: usize = parts[1].parse().unwrap();
            let block_height: usize = parts[2].parse().unwrap();
            let width: usize = parts[3].parse().unwrap();
            let height: usize = parts[4].parse().unwrap();
            let input = parse_hex(parts[5]);
            let result: Vec<&str> = parts[6].split(':').collect();
            let expected_consumed: usize = result[0].parse().unwrap();
            let expected_output = parse_hex(result[1]);
            let mut output = vec![0u8; width * height * 4];
            let consumed = decode_astc(
                &input,
                width,
                height,
                block_width,
                block_height,
                &mut output,
            )
            .unwrap();
            assert_eq!(consumed, expected_consumed, "consumed {line}");
            assert_eq!(output, expected_output, "output {line}");
            rows += 1;
        }
        assert!(rows >= 8, "expected at least 8 ASTC rows, got {rows}");
    }
}
