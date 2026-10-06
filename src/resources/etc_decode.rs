//! ETC1/ETC2/EAC block decoders ported from the `AssetRipper.TextureDecoder` dependency used by
//! `AssetRipper.Export.Modules.Textures.TextureConverter`.
//!
//! The baseline dispatches `TextureFormat.ETC_RGB4`/`ETC2_*`/`EAC_*` to this package. The port
//! mirrors the reference's block permutation (`WriteOrderTable`) and its `WriteOrderTableRev`
//! alpha order exactly, including the punch-through mask and the `+1`-free integer interpolation.
//!
//! Differential fixture: `etc_decode_fixture.txt` (probe `Temp/opencode/etcprobe`).

use super::texture_decode::{require_input, require_output, TextureDecodeError};

const WRITE_ORDER_TABLE: [usize; 16] = [0, 4, 8, 12, 1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15];
const WRITE_ORDER_TABLE_REV: [usize; 16] = [15, 11, 7, 3, 14, 10, 6, 2, 13, 9, 5, 1, 12, 8, 4, 0];
const ETC1_SUBBLOCK_TABLE: [usize; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1,
];
const ETC1_MODIFIER_TABLE: [i32; 32] = [
    2, 8, -2, -8, 5, 17, -5, -17, 9, 29, -9, -29, 13, 42, -13, -42, 18, 60, -18, -60, 24, 80, -24,
    -80, 33, 106, -33, -106, 47, 183, -47, -183,
];
const PUNCHTHROUGH_MODIFIER_TABLE: [i32; 32] = [
    0, 8, 0, -8, 0, 17, 0, -17, 0, 29, 0, -29, 0, 42, 0, -42, 0, 60, 0, -60, 0, 80, 0, -80, 0, 106,
    0, -106, 0, 183, 0, -183,
];
const ETC2_DISTANCE_TABLE: [i32; 8] = [3, 6, 11, 16, 23, 32, 41, 64];
const ETC2_ALPHA_MOD_TABLE: [i32; 128] = [
    -3, -6, -9, -15, 2, 5, 8, 14, -3, -7, -10, -13, 2, 6, 9, 12, -2, -5, -8, -13, 1, 4, 7, 12, -2,
    -4, -6, -13, 1, 3, 5, 12, -3, -6, -8, -12, 2, 5, 7, 11, -3, -7, -9, -11, 2, 6, 8, 10, -4, -7,
    -8, -11, 3, 6, 7, 10, -3, -5, -8, -11, 2, 4, 7, 10, -2, -6, -8, -10, 1, 5, 7, 9, -2, -5, -8,
    -10, 1, 4, 7, 9, -2, -4, -8, -10, 1, 3, 7, 9, -2, -5, -7, -10, 1, 4, 6, 9, -3, -4, -7, -10, 2,
    3, 6, 9, -1, -2, -3, -10, 0, 1, 2, 9, -4, -6, -8, -9, 3, 5, 7, 8, -3, -5, -7, -9, 2, 4, 6, 8,
];
const PUNCHTHROUGH_BOOLEAN_TABLE: [bool; 4] = [true, true, false, true];

type Pixel = [u8; 4];
type Block = [Pixel; 16];

fn clamp255(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

/// The high nibble replicated into the low nibble (`input & 0xf0 | input >> 4`).
fn hi_nibble(value: u8) -> u8 {
    (value & 0xF0) | (value >> 4)
}

/// The low nibble replicated into the high nibble (`input & 0x0f | input << 4`, truncated).
fn lo_nibble(value: u8) -> u8 {
    ((value as i32 & 0x0F) | ((value as i32) << 4)) as u8
}

fn shift(value: u8, count: u32) -> i32 {
    (value as i32) << count
}

fn applicate_color(c: &[u8], o: usize, m: i32) -> Pixel {
    [
        clamp255(i32::from(c[o * 3]) + m),
        clamp255(i32::from(c[o * 3 + 1]) + m),
        clamp255(i32::from(c[o * 3 + 2]) + m),
        0xFF,
    ]
}

fn applicate_color_raw(c: &[u8], o: usize) -> Pixel {
    [c[o * 3], c[o * 3 + 1], c[o * 3 + 2], 0xFF]
}

fn decode_etc1_block(input: &[u8], output: &mut Block) {
    let i3 = input[3] as i32;
    let code = [(i3 >> 5) * 4, ((i3 >> 2) & 7) * 4];
    let mut c = [0u8; 6];
    let ti = ((i3 & 1) * 16) as usize;
    if (i3 & 2) != 0 {
        c[0] = input[0] & 0xF8;
        c[1] = input[1] & 0xF8;
        c[2] = input[2] & 0xF8;
        c[3] = (i32::from(c[0]) + (shift(input[0], 3) & 0x18) - (shift(input[0], 3) & 0x20)) as u8;
        c[4] = (i32::from(c[1]) + (shift(input[1], 3) & 0x18) - (shift(input[1], 3) & 0x20)) as u8;
        c[5] = (i32::from(c[2]) + (shift(input[2], 3) & 0x18) - (shift(input[2], 3) & 0x20)) as u8;
        for value in &mut c {
            *value |= *value >> 5;
        }
    } else {
        c[0] = hi_nibble(input[0]);
        c[3] = lo_nibble(input[0]);
        c[1] = hi_nibble(input[1]);
        c[4] = lo_nibble(input[1]);
        c[2] = hi_nibble(input[2]);
        c[5] = lo_nibble(input[2]);
    }

    let mut j = shift(input[6], 8) | i32::from(input[7]);
    let mut k = shift(input[4], 8) | i32::from(input[5]);
    for i in 0..16 {
        let s = ETC1_SUBBLOCK_TABLE[ti + i];
        let index = (((k << 1) & 2) | (j & 1)) as usize;
        let cd = code[s] as usize;
        let m = ETC1_MODIFIER_TABLE[cd + index];
        output[WRITE_ORDER_TABLE[i]] = applicate_color(&c, s, m);
        j >>= 1;
        k >>= 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn write_etc1_style(
    output: &mut Block,
    c: &[u8],
    code: [i32; 2],
    ti: usize,
    mut j: i32,
    mut k: i32,
    modifiers: &[i32; 32],
    mask: Option<&[bool; 4]>,
) {
    for i in 0..16 {
        let s = ETC1_SUBBLOCK_TABLE[ti + i];
        let index = (((k << 1) & 2) | (j & 1)) as usize;
        let cd = code[s] as usize;
        let m = modifiers[cd + index];
        output[WRITE_ORDER_TABLE[i]] = match mask {
            Some(table) if !table[index] => [0, 0, 0, 0],
            _ => applicate_color(c, s, m),
        };
        j >>= 1;
        k >>= 1;
    }
}

#[allow(clippy::too_many_lines)]
fn decode_etc2_block(input: &[u8], output: &mut Block) {
    let mut j = shift(input[6], 8) | i32::from(input[7]);
    let mut k = shift(input[4], 8) | i32::from(input[5]);
    let mut c = [0u8; 9];

    if (input[3] & 2) != 0 {
        let r = i32::from(input[0] & 0xF8);
        let dr = (shift(input[0], 3) & 0x18) - (shift(input[0], 3) & 0x20);
        if r + dr < 0 || r + dr > 255 {
            // T mode.
            c[0] = ((shift(input[0], 3) & 0xC0)
                | (shift(input[0], 4) & 0x30)
                | (i32::from(input[0]) >> 1 & 0xC)
                | (i32::from(input[0]) & 3)) as u8;
            c[1] = hi_nibble(input[1]);
            c[2] = lo_nibble(input[1]);
            c[3] = hi_nibble(input[2]);
            c[4] = lo_nibble(input[2]);
            c[5] = hi_nibble(input[3]);
            let ti = ((i32::from(input[3]) >> 1 & 6) | (i32::from(input[3]) & 1)) as usize;
            let d = ETC2_DISTANCE_TABLE[ti];
            let color_set = [
                applicate_color_raw(&c, 0),
                applicate_color(&c, 1, d),
                applicate_color_raw(&c, 1),
                applicate_color(&c, 1, -d),
            ];
            for i in 0..16 {
                let index = (((k << 1) & 2) | (j & 1)) as usize;
                output[WRITE_ORDER_TABLE[i]] = color_set[index];
                j >>= 1;
                k >>= 1;
            }
        } else {
            let g = i32::from(input[1] & 0xF8);
            let dg = (shift(input[1], 3) & 0x18) - (shift(input[1], 3) & 0x20);
            if g + dg < 0 || g + dg > 255 {
                // H mode.
                c[0] = ((shift(input[0], 1) & 0xF0) | (i32::from(input[0]) >> 3 & 0xF)) as u8;
                c[1] = ((shift(input[0], 5) & 0xE0) | (i32::from(input[1]) & 0x10)) as u8;
                c[1] |= c[1] >> 4;
                c[2] = ((i32::from(input[1]) & 8)
                    | (shift(input[1], 1) & 6)
                    | (i32::from(input[2]) >> 7)) as u8;
                c[2] |= c[2] << 4;
                c[3] = ((shift(input[2], 1) & 0xF0) | (i32::from(input[2]) >> 3 & 0xF)) as u8;
                c[4] = ((shift(input[2], 5) & 0xE0) | (i32::from(input[3]) >> 3 & 0x10)) as u8;
                c[4] |= c[4] >> 4;
                c[5] = ((shift(input[3], 1) & 0xF0) | (i32::from(input[3]) >> 3 & 0xF)) as u8;
                let mut di = ((i32::from(input[3]) & 4) | (shift(input[3], 1) & 2)) as usize;
                if c[0] > c[3] || (c[0] == c[3] && (c[1] > c[4] || (c[1] == c[4] && c[2] >= c[5])))
                {
                    di += 1;
                }
                let d = ETC2_DISTANCE_TABLE[di];
                let color_set = [
                    applicate_color(&c, 0, d),
                    applicate_color(&c, 0, -d),
                    applicate_color(&c, 1, d),
                    applicate_color(&c, 1, -d),
                ];
                for i in 0..16 {
                    let index = (((k << 1) & 2) | (j & 1)) as usize;
                    output[WRITE_ORDER_TABLE[i]] = color_set[index];
                    j >>= 1;
                    k >>= 1;
                }
            } else {
                let b = i32::from(input[2] & 0xF8);
                let db = (shift(input[2], 3) & 0x18) - (shift(input[2], 3) & 0x20);
                if b + db < 0 || b + db > 255 {
                    // Planar mode.
                    c[0] = ((shift(input[0], 1) & 0xFC) | (i32::from(input[0]) >> 5 & 3)) as u8;
                    c[1] = ((shift(input[0], 7) & 0x80)
                        | (i32::from(input[1]) & 0x7E)
                        | (i32::from(input[0]) & 1)) as u8;
                    c[2] = ((shift(input[1], 7) & 0x80)
                        | (shift(input[2], 2) & 0x60)
                        | (shift(input[2], 3) & 0x18)
                        | (i32::from(input[3]) >> 5 & 4)) as u8;
                    c[2] |= c[2] >> 6;
                    c[3] = ((shift(input[3], 1) & 0xF8)
                        | (shift(input[3], 2) & 4)
                        | (i32::from(input[3]) >> 5 & 3)) as u8;
                    c[4] = ((i32::from(input[4]) & 0xFE) | (i32::from(input[4]) >> 7)) as u8;
                    c[5] = ((shift(input[4], 7) & 0x80) | (i32::from(input[5]) >> 1 & 0x7C)) as u8;
                    c[5] |= c[5] >> 6;
                    c[6] = ((shift(input[5], 5) & 0xE0)
                        | (i32::from(input[6]) >> 3 & 0x1C)
                        | (i32::from(input[5]) >> 1 & 3)) as u8;
                    c[7] = ((shift(input[6], 3) & 0xF8)
                        | (i32::from(input[7]) >> 5 & 6)
                        | (i32::from(input[6]) >> 4 & 1)) as u8;
                    c[8] = (shift(input[7], 2) | (i32::from(input[7]) >> 4 & 3)) as u8;
                    for y in 0..4usize {
                        for x in 0..4usize {
                            let i = y * 4 + x;
                            let red = clamp255(
                                (x as i32 * (i32::from(c[3]) - i32::from(c[0]))
                                    + y as i32 * (i32::from(c[6]) - i32::from(c[0]))
                                    + 4 * i32::from(c[0])
                                    + 2)
                                    >> 2,
                            );
                            let green = clamp255(
                                (x as i32 * (i32::from(c[4]) - i32::from(c[1]))
                                    + y as i32 * (i32::from(c[7]) - i32::from(c[1]))
                                    + 4 * i32::from(c[1])
                                    + 2)
                                    >> 2,
                            );
                            let blue = clamp255(
                                (x as i32 * (i32::from(c[5]) - i32::from(c[2]))
                                    + y as i32 * (i32::from(c[8]) - i32::from(c[2]))
                                    + 4 * i32::from(c[2])
                                    + 2)
                                    >> 2,
                            );
                            output[i] = [red, green, blue, 0xFF];
                        }
                    }
                } else {
                    // Differential mode.
                    let code = [
                        (i32::from(input[3]) >> 5) * 4,
                        (i32::from(input[3]) >> 2 & 7) * 4,
                    ];
                    let ti = ((i32::from(input[3]) & 1) * 16) as usize;
                    c[0] = (r | r >> 5) as u8;
                    c[1] = (g | g >> 5) as u8;
                    c[2] = (b | b >> 5) as u8;
                    c[3] = (r + dr) as u8;
                    c[4] = (g + dg) as u8;
                    c[5] = (b + db) as u8;
                    c[3] |= c[3] >> 5;
                    c[4] |= c[4] >> 5;
                    c[5] |= c[5] >> 5;
                    write_etc1_style(output, &c, code, ti, j, k, &ETC1_MODIFIER_TABLE, None);
                }
            }
        }
    } else {
        // Individual mode.
        let code = [
            (i32::from(input[3]) >> 5) * 4,
            (i32::from(input[3]) >> 2 & 7) * 4,
        ];
        let ti = ((i32::from(input[3]) & 1) * 16) as usize;
        c[0] = hi_nibble(input[0]);
        c[3] = lo_nibble(input[0]);
        c[1] = hi_nibble(input[1]);
        c[4] = lo_nibble(input[1]);
        c[2] = hi_nibble(input[2]);
        c[5] = lo_nibble(input[2]);
        write_etc1_style(output, &c, code, ti, j, k, &ETC1_MODIFIER_TABLE, None);
    }
}

#[allow(clippy::too_many_lines)]
fn decode_etc2_punch_through_block(input: &[u8], output: &mut Block) {
    let mut j = shift(input[6], 8) | i32::from(input[7]);
    let mut k = shift(input[4], 8) | i32::from(input[5]);
    let mut c = [0u8; 9];
    let r = i32::from(input[0] & 0xF8);
    let dr = (shift(input[0], 3) & 0x18) - (shift(input[0], 3) & 0x20);
    if r + dr < 0 || r + dr > 255 {
        c[0] = ((shift(input[0], 3) & 0xC0)
            | (shift(input[0], 4) & 0x30)
            | (i32::from(input[0]) >> 1 & 0xC)
            | (i32::from(input[0]) & 3)) as u8;
        c[1] = hi_nibble(input[1]);
        c[2] = lo_nibble(input[1]);
        c[3] = hi_nibble(input[2]);
        c[4] = lo_nibble(input[2]);
        c[5] = hi_nibble(input[3]);
        let ti = ((i32::from(input[3]) >> 1 & 6) | (i32::from(input[3]) & 1)) as usize;
        let d = ETC2_DISTANCE_TABLE[ti];
        let color_set = [
            applicate_color_raw(&c, 0),
            applicate_color(&c, 1, d),
            applicate_color_raw(&c, 1),
            applicate_color(&c, 1, -d),
        ];
        for i in 0..16 {
            let index = (((k << 1) & 2) | (j & 1)) as usize;
            output[WRITE_ORDER_TABLE[i]] = if PUNCHTHROUGH_BOOLEAN_TABLE[index] {
                color_set[index]
            } else {
                [0, 0, 0, 0]
            };
            j >>= 1;
            k >>= 1;
        }
    } else {
        let g = i32::from(input[1] & 0xF8);
        let dg = (shift(input[1], 3) & 0x18) - (shift(input[1], 3) & 0x20);
        if g + dg < 0 || g + dg > 255 {
            c[0] = ((shift(input[0], 1) & 0xF0) | (i32::from(input[0]) >> 3 & 0xF)) as u8;
            c[1] = ((shift(input[0], 5) & 0xE0) | (i32::from(input[1]) & 0x10)) as u8;
            c[1] |= c[1] >> 4;
            c[2] = ((i32::from(input[1]) & 8)
                | (shift(input[1], 1) & 6)
                | (i32::from(input[2]) >> 7)) as u8;
            c[2] |= c[2] << 4;
            c[3] = ((shift(input[2], 1) & 0xF0) | (i32::from(input[2]) >> 3 & 0xF)) as u8;
            c[4] = ((shift(input[2], 5) & 0xE0) | (i32::from(input[3]) >> 3 & 0x10)) as u8;
            c[4] |= c[4] >> 4;
            c[5] = ((shift(input[3], 1) & 0xF0) | (i32::from(input[3]) >> 3 & 0xF)) as u8;
            let mut di = ((i32::from(input[3]) & 4) | (shift(input[3], 1) & 2)) as usize;
            if c[0] > c[3] || (c[0] == c[3] && (c[1] > c[4] || (c[1] == c[4] && c[2] >= c[5]))) {
                di += 1;
            }
            let d = ETC2_DISTANCE_TABLE[di];
            let color_set = [
                applicate_color(&c, 0, d),
                applicate_color(&c, 0, -d),
                applicate_color(&c, 1, d),
                applicate_color(&c, 1, -d),
            ];
            for i in 0..16 {
                let index = (((k << 1) & 2) | (j & 1)) as usize;
                output[WRITE_ORDER_TABLE[i]] = if PUNCHTHROUGH_BOOLEAN_TABLE[index] {
                    color_set[index]
                } else {
                    [0, 0, 0, 0]
                };
                j >>= 1;
                k >>= 1;
            }
        } else {
            let b = i32::from(input[2] & 0xF8);
            let db = (shift(input[2], 3) & 0x18) - (shift(input[2], 3) & 0x20);
            if b + db < 0 || b + db > 255 {
                c[0] = ((shift(input[0], 1) & 0xFC) | (i32::from(input[0]) >> 5 & 3)) as u8;
                c[1] = ((shift(input[0], 7) & 0x80)
                    | (i32::from(input[1]) & 0x7E)
                    | (i32::from(input[0]) & 1)) as u8;
                c[2] = ((shift(input[1], 7) & 0x80)
                    | (shift(input[2], 2) & 0x60)
                    | (shift(input[2], 3) & 0x18)
                    | (i32::from(input[3]) >> 5 & 4)) as u8;
                c[2] |= c[2] >> 6;
                c[3] = ((shift(input[3], 1) & 0xF8)
                    | (shift(input[3], 2) & 4)
                    | (i32::from(input[3]) >> 5 & 3)) as u8;
                c[4] = ((i32::from(input[4]) & 0xFE) | (i32::from(input[4]) >> 7)) as u8;
                c[5] = ((shift(input[4], 7) & 0x80) | (i32::from(input[5]) >> 1 & 0x7C)) as u8;
                c[5] |= c[5] >> 6;
                c[6] = ((shift(input[5], 5) & 0xE0)
                    | (i32::from(input[6]) >> 3 & 0x1C)
                    | (i32::from(input[5]) >> 1 & 3)) as u8;
                c[7] = ((shift(input[6], 3) & 0xF8)
                    | (i32::from(input[7]) >> 5 & 6)
                    | (i32::from(input[6]) >> 4 & 1)) as u8;
                c[8] = (shift(input[7], 2) | (i32::from(input[7]) >> 4 & 3)) as u8;
                for y in 0..4usize {
                    for x in 0..4usize {
                        let i = y * 4 + x;
                        let red = clamp255(
                            (x as i32 * (i32::from(c[3]) - i32::from(c[0]))
                                + y as i32 * (i32::from(c[6]) - i32::from(c[0]))
                                + 4 * i32::from(c[0])
                                + 2)
                                >> 2,
                        );
                        let green = clamp255(
                            (x as i32 * (i32::from(c[4]) - i32::from(c[1]))
                                + y as i32 * (i32::from(c[7]) - i32::from(c[1]))
                                + 4 * i32::from(c[1])
                                + 2)
                                >> 2,
                        );
                        let blue = clamp255(
                            (x as i32 * (i32::from(c[5]) - i32::from(c[2]))
                                + y as i32 * (i32::from(c[8]) - i32::from(c[2]))
                                + 4 * i32::from(c[2])
                                + 2)
                                >> 2,
                        );
                        output[i] = [red, green, blue, 0xFF];
                    }
                }
            } else {
                let code = [
                    (i32::from(input[3]) >> 5) * 4,
                    (i32::from(input[3]) >> 2 & 7) * 4,
                ];
                let ti = ((i32::from(input[3]) & 1) * 16) as usize;
                c[0] = (r | r >> 5) as u8;
                c[1] = (g | g >> 5) as u8;
                c[2] = (b | b >> 5) as u8;
                c[3] = (r + dr) as u8;
                c[4] = (g + dg) as u8;
                c[5] = (b + db) as u8;
                c[3] |= c[3] >> 5;
                c[4] |= c[4] >> 5;
                c[5] |= c[5] >> 5;
                for i in 0..16 {
                    let index = (((k << 1) & 2) | (j & 1)) as usize;
                    if PUNCHTHROUGH_BOOLEAN_TABLE[index] {
                        let s = ETC1_SUBBLOCK_TABLE[ti + i];
                        let cd = code[s] as usize;
                        let m = PUNCHTHROUGH_MODIFIER_TABLE[cd + index];
                        output[WRITE_ORDER_TABLE[i]] = applicate_color(&c, s, m);
                    } else {
                        output[WRITE_ORDER_TABLE[i]] = [0, 0, 0, 0];
                    }
                    j >>= 1;
                    k >>= 1;
                }
            }
        }
    }
}

fn decode_etc2a1_block(input: &[u8], output: &mut Block) {
    if (input[3] & 2) != 0 {
        decode_etc2_block(input, output);
    } else {
        decode_etc2_punch_through_block(input, output);
    }
}

fn get6_swapped_bytes(input: &[u8]) -> u64 {
    u64::from(input[7])
        | u64::from(input[6]) << 8
        | u64::from(input[5]) << 16
        | u64::from(input[4]) << 24
        | u64::from(input[3]) << 32
        | u64::from(input[2]) << 40
}

fn decode_eac11_constant(output: &mut Block, channel: usize, base: i32) {
    for &pixel in &WRITE_ORDER_TABLE_REV {
        output[pixel][channel] = base as u8;
    }
}

fn decode_eac11_table(
    output: &mut Block,
    channel: usize,
    base: i32,
    table: usize,
    mul: i32,
    l: u64,
) {
    let mut l = l;
    let modifiers = &ETC2_ALPHA_MOD_TABLE[table * 8..table * 8 + 8];
    for &pixel in &WRITE_ORDER_TABLE_REV {
        let value = base + mul * modifiers[(l & 7) as usize];
        output[pixel][channel] = clamp255(value);
        l >>= 3;
    }
}

fn decode_eac_unsigned_block(input: &[u8], output: &mut Block, channel: usize) {
    let base = i32::from(input[0]);
    let data1 = i32::from(input[1]);
    let mul = data1 >> 4;
    if mul == 0 {
        decode_eac11_constant(output, channel, base);
    } else {
        let table = (data1 & 0xF) as usize;
        decode_eac11_table(output, channel, base, table, mul, get6_swapped_bytes(input));
    }
}

fn decode_eac_signed_block(input: &[u8], output: &mut Block, channel: usize) {
    let base = 127 + i32::from(input[0] as i8);
    let data1 = i32::from(input[1]);
    let mul = data1 >> 4;
    if mul == 0 {
        decode_eac11_constant(output, channel, base);
    } else {
        let table = (data1 & 0xF) as usize;
        decode_eac11_table(output, channel, base, table, mul, get6_swapped_bytes(input));
    }
}

fn decode_etc2a8_block(input: &[u8], output: &mut Block) {
    let base = i32::from(input[0]);
    let data1 = i32::from(input[1]);
    let mul = data1 >> 4;
    if mul == 0 {
        decode_eac11_constant(output, 3, base);
    } else {
        let table = (data1 & 0xF) as usize;
        decode_eac11_table(output, 3, base, table, mul, get6_swapped_bytes(input));
    }
}

fn decode_etc_family(
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

/// `EtcDecoder.DecompressETC`.
pub fn decode_etc1(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_etc_family(input, width, height, output, 8, decode_etc1_block)
}

/// `EtcDecoder.DecompressETC2`.
pub fn decode_etc2(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_etc_family(input, width, height, output, 8, decode_etc2_block)
}

/// `EtcDecoder.DecompressETC2A1`.
pub fn decode_etc2a1(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_etc_family(input, width, height, output, 8, decode_etc2a1_block)
}

/// `EtcDecoder.DecompressETC2A8`.
pub fn decode_etc2a8(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_etc_family(input, width, height, output, 16, |block, buffer| {
        decode_etc2_block(&block[8..16], buffer);
        decode_etc2a8_block(&block[0..8], buffer);
    })
}

/// `EtcDecoder.DecompressEACRUnsigned`.
pub fn decode_eac_r_unsigned(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_etc_family(input, width, height, output, 8, |block, buffer| {
        *buffer = [[0, 0, 0, 0xFF]; 16];
        decode_eac_unsigned_block(block, buffer, 0);
    })
}

/// `EtcDecoder.DecompressEACRSigned`.
pub fn decode_eac_r_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_etc_family(input, width, height, output, 8, |block, buffer| {
        *buffer = [[0, 0, 0, 0xFF]; 16];
        decode_eac_signed_block(block, buffer, 0);
    })
}

/// `EtcDecoder.DecompressEACRGUnsigned`.
pub fn decode_eac_rg_unsigned(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_etc_family(input, width, height, output, 16, |block, buffer| {
        *buffer = [[0, 0, 0, 0xFF]; 16];
        decode_eac_unsigned_block(&block[0..8], buffer, 0);
        decode_eac_unsigned_block(&block[8..16], buffer, 1);
    })
}

/// `EtcDecoder.DecompressEACRGSigned`.
pub fn decode_eac_rg_signed(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_etc_family(input, width, height, output, 16, |block, buffer| {
        *buffer = [[0, 0, 0, 0xFF]; 16];
        decode_eac_signed_block(&block[0..8], buffer, 0);
        decode_eac_signed_block(&block[8..16], buffer, 1);
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
            "ETC1" => decode_etc1(input, width, height, &mut output),
            "ETC2" => decode_etc2(input, width, height, &mut output),
            "ETC2A1" => decode_etc2a1(input, width, height, &mut output),
            "ETC2A8" => decode_etc2a8(input, width, height, &mut output),
            "EAC_R" => decode_eac_r_unsigned(input, width, height, &mut output),
            "EAC_RS" => decode_eac_r_signed(input, width, height, &mut output),
            "EAC_RG" => decode_eac_rg_unsigned(input, width, height, &mut output),
            "EAC_RGS" => decode_eac_rg_signed(input, width, height, &mut output),
            other => panic!("unknown ETC kind {other}"),
        }
        .unwrap();
        (consumed, output)
    }

    /// Byte-for-byte differential against `AssetRipper.TextureDecoder` (probe
    /// `Temp/opencode/etcprobe`), covering the ETC1 individual/differential modes, the ETC2
    /// T/H/planar/differential/individual modes, ETC2A1 punch-through, ETC2A8, and EAC R/RG
    /// unsigned/signed.
    #[test]
    fn etc_decoders_match_the_reference_runtime() {
        let fixture = include_str!("etc_decode_fixture.txt");
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
        assert_eq!(rows, 23);
    }

    /// Partial block grids and buffer bounds, beyond the 4x4 differential rows.
    ///
    /// The expected pixels are hand-derived from the block formats (an all-zero ETC1
    /// block is uniform `[2, 2, 2, 255]` through the first modifier-table entry; a
    /// zero-multiplier EAC block is a flat channel fill), and the consumed counts
    /// follow the ceiling block grid exactly.
    #[test]
    fn partial_dimensions_and_bounds_match_the_family_layout() {
        // 5x5 spans a 2x2 block grid with partial right and bottom edges.
        let input = vec![0u8; 32];
        let mut output = vec![0u8; 5 * 5 * 4];
        assert_eq!(decode_etc1(&input, 5, 5, &mut output).unwrap(), 32);
        assert!(output.chunks_exact(4).all(|pixel| pixel == [2, 2, 2, 255]));
        // A 1x1 image still consumes one whole block.
        let mut tiny = vec![0u8; 4];
        assert_eq!(decode_etc1(&input[..8], 1, 1, &mut tiny).unwrap(), 8);
        assert_eq!(tiny, vec![2, 2, 2, 255]);
        // A flat EAC unsigned block fills the channel over a 2x1 grid.
        let eac = [0xAB, 0x00, 0, 0, 0, 0, 0, 0, 0xAB, 0x00, 0, 0, 0, 0, 0, 0];
        let mut output = vec![0u8; 6 * 3 * 4];
        assert_eq!(decode_eac_r_unsigned(&eac, 6, 3, &mut output).unwrap(), 16);
        assert!(output
            .chunks_exact(4)
            .all(|pixel| pixel == [0xAB, 0, 0, 255]));
        // Short inputs and outputs report bounds instead of panicking.
        let mut output = vec![0u8; 4 * 4 * 4];
        assert!(decode_etc1(&[0u8; 7], 4, 4, &mut output).is_err());
        let mut short = vec![0u8; 4 * 4 * 4 - 1];
        assert!(decode_etc1(&[0u8; 8], 4, 4, &mut short).is_err());
    }
}
