//! JPEG writer for `DirectBitmap.SaveAsJpeg` (StbImageWriteSharp `ImageWriter.WriteJpg`).
//!
//! The reference passes RGBA8 bytes with `RedGreenBlueAlpha` components and `default`
//! (zero) quality. Zero quality selects the quality-90 quantization tables, sampling is
//! always 4:2:0, and the standard Annex K Huffman tables are emitted. This port
//! reproduces that exact configuration byte-for-byte: fixed JFIF/DQT/SOF/DHT/SOS headers,
//! float RGB-to-YCbCr, 2x2 box-averaged chroma with edge replication, the AAN float DCT,
//! round-half-away-from-zero quantization, zigzag scan, canonical Huffman coding with
//! `0xFF` stuffing, and the EOI marker.

use std::io::Write;

use super::image_export::ImageWriteError;
use super::texture_converter::Bitmap;

/// `DirectBitmap.SaveAsJpeg`.
///
/// Bytes arrive as RGBA8; alpha is ignored exactly like the reference, which hands the
/// RGBA buffer to the encoder with an RGBA component tag while the encoder reads only
/// the color lanes. Layers stack vertically (`Height * Depth`).
pub fn save_jpg<W: Write>(bitmap: &Bitmap, writer: &mut W) -> Result<(), ImageWriteError> {
    let width = bitmap.width;
    let height = bitmap.height.saturating_mul(bitmap.depth);
    if width > u16::MAX as usize || height > u16::MAX as usize {
        // SOF0 stores dimensions as big-endian `u16`. Real textures stay far below
        // this; the reference would truncate the cast, so refuse instead of writing
        // a corrupt header.
        return Err(ImageWriteError::DimensionTooLarge { value: width });
    }
    let pixel_count = width
        .checked_mul(height)
        .ok_or(ImageWriteError::DimensionTooLarge { value: width })?;
    if bitmap.pixels.len() < pixel_count.saturating_mul(4) {
        return Err(ImageWriteError::DimensionTooLarge { value: width });
    }
    writer.write_all(&[0xFF, 0xD8])?;
    writer.write_all(APP0_JFIF)?;
    write_quantization_tables(writer)?;
    write_frame_header(writer, width as u16, height as u16)?;
    write_huffman_tables(writer)?;
    writer.write_all(SCAN_HEADER)?;

    let luma = ComponentTables::luma();
    let chroma = ComponentTables::chroma();
    let encoder = JpegEncoder::new(
        &bitmap.pixels[..pixel_count.saturating_mul(4)],
        width,
        height,
    );
    let mut bits = BitWriter::new(writer);
    let mut predictors = [0i32; 3];
    for block in encoder.blocks() {
        let tables = if block.component == 0 { &luma } else { &chroma };
        let coefficients = quantize_block(&block);
        let diff = coefficients[0].wrapping_sub(predictors[block.component]);
        predictors[block.component] = coefficients[0];
        let category = diff_category(diff);
        bits.write(&tables.dc, category)?;
        bits.write_additional(diff, category)?;
        let mut zeros = 0u32;
        for &value in &coefficients[1..] {
            if value == 0 {
                zeros += 1;
                continue;
            }
            while zeros >= 16 {
                bits.write(&tables.ac, 0xF0)?;
                zeros -= 16;
            }
            let size = diff_category(value);
            bits.write(&tables.ac, (zeros << 4) | size)?;
            bits.write_additional(value, size)?;
            zeros = 0;
        }
        if zeros > 0 {
            bits.write(&tables.ac, 0x00)?;
        }
    }
    bits.finish()?;
    writer.write_all(&[0xFF, 0xD9])?;
    Ok(())
}

/// The fixed JFIF APP0 segment (version 1.1, no density units, no thumbnail).
const APP0_JFIF: &[u8] = &[
    0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01,
    0x00, 0x00,
];

/// The fixed start-of-scan header: three interleaved components, spectral slice
/// `0..63`, successive approximation `0`.
const SCAN_HEADER: &[u8] = &[
    0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
];

/// Effective quality-90 luminance table measured from the shipped encoder for a zero
/// `quality` argument (probe `Temp/opencode/jpgprobe`, DQT segment). The first 51
/// entries match the standard IJG quality-50 base scaled by stb (`(base * 20 + 50) /
/// 100`); the tail does not, so the measured bytes are embedded directly.
const LUMA_Q90: [u8; 64] = [
    0x03, 0x02, 0x02, 0x03, 0x02, 0x02, 0x03, 0x03, 0x03, 0x03, 0x04, 0x03, 0x03, 0x04, 0x05, 0x08,
    0x05, 0x05, 0x04, 0x04, 0x05, 0x0a, 0x07, 0x07, 0x06, 0x08, 0x0c, 0x0a, 0x0c, 0x0c, 0x0b, 0x0a,
    0x0b, 0x0b, 0x0d, 0x0e, 0x12, 0x10, 0x0d, 0x0e, 0x11, 0x0e, 0x0b, 0x0b, 0x10, 0x16, 0x10, 0x11,
    0x13, 0x14, 0x15, 0x15, 0x15, 0x0c, 0x0f, 0x17, 0x18, 0x16, 0x14, 0x18, 0x12, 0x14, 0x15, 0x14,
];

/// Effective quality-90 chrominance table measured the same way.
const CHROMA_Q90: [u8; 64] = [
    0x03, 0x04, 0x04, 0x05, 0x04, 0x05, 0x09, 0x05, 0x05, 0x09, 0x14, 0x0d, 0x0b, 0x0d, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14,
];

fn write_quantization_tables<W: Write>(writer: &mut W) -> Result<(), ImageWriteError> {
    writer.write_all(&[0xFF, 0xDB, 0x00, 0x84, 0x00])?;
    writer.write_all(&LUMA_Q90)?;
    writer.write_all(&[0x01])?;
    writer.write_all(&CHROMA_Q90)?;
    Ok(())
}

fn write_frame_header<W: Write>(
    writer: &mut W,
    width: u16,
    height: u16,
) -> Result<(), ImageWriteError> {
    writer.write_all(&[
        0xFF,
        0xC0,
        0x00,
        0x11,
        0x08,
        (height >> 8) as u8,
        height as u8,
        (width >> 8) as u8,
        width as u8,
        0x03,
        0x01,
        0x22,
        0x00,
        0x02,
        0x11,
        0x01,
        0x03,
        0x11,
        0x01,
    ])?;
    Ok(())
}

/// One Huffman table as (code, length) per symbol, built canonically from the
/// emitted (counts, symbols) lists.
struct HuffmanTable {
    codes: [(u16, u8); 256],
}

impl HuffmanTable {
    fn build(counts: &[u8; 16], symbols: &[u8]) -> Self {
        let mut codes = [(0u16, 0u8); 256];
        let mut code = 0u16;
        let mut index = 0;
        for (length, &count) in counts.iter().enumerate() {
            for _ in 0..count {
                codes[symbols[index] as usize] = (code, (length + 1) as u8);
                code += 1;
                index += 1;
            }
            code <<= 1;
        }
        Self { codes }
    }
}

struct ComponentTables {
    dc: HuffmanTable,
    ac: HuffmanTable,
}

impl ComponentTables {
    fn luma() -> Self {
        Self {
            dc: HuffmanTable::build(&DC_LUMA_COUNTS, &DC_SYMBOLS),
            ac: HuffmanTable::build(&AC_LUMA_COUNTS, &AC_LUMA_SYMBOLS),
        }
    }

    fn chroma() -> Self {
        Self {
            dc: HuffmanTable::build(&DC_CHROMA_COUNTS, &DC_SYMBOLS),
            ac: HuffmanTable::build(&AC_CHROMA_COUNTS, &AC_CHROMA_SYMBOLS),
        }
    }
}

/// DC code lengths per category, shared by the luminance and chrominance tables.
const DC_LUMA_COUNTS: [u8; 16] = [
    0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// DC chrominance code lengths: categories shift one bit longer than luminance.
const DC_CHROMA_COUNTS: [u8; 16] = [
    0x00, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// DC symbols are the categories `0..12` for both components.
const DC_SYMBOLS: [u8; 12] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
];

/// Luminance AC code lengths (162 symbols).
const AC_LUMA_COUNTS: [u8; 16] = [
    0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00, 0x01, 0x7d,
];

/// Luminance AC symbols in code order.
const AC_LUMA_SYMBOLS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

/// Chrominance AC code lengths (162 symbols).
const AC_CHROMA_COUNTS: [u8; 16] = [
    0x00, 0x02, 0x01, 0x02, 0x04, 0x04, 0x03, 0x04, 0x07, 0x05, 0x04, 0x04, 0x00, 0x01, 0x02, 0x77,
];

/// Chrominance AC symbols in code order.
const AC_CHROMA_SYMBOLS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

fn write_huffman_tables<W: Write>(writer: &mut W) -> Result<(), ImageWriteError> {
    // DHT length covers the length field itself plus all four tables.
    let length = 2 + (1 + 16 + 12) + (1 + 16 + 162) + (1 + 16 + 12) + (1 + 16 + 162);
    writer.write_all(&[0xFF, 0xC4, (length >> 8) as u8, length as u8])?;
    write_table(writer, 0x00, &DC_LUMA_COUNTS, &DC_SYMBOLS)?;
    write_table(writer, 0x10, &AC_LUMA_COUNTS, &AC_LUMA_SYMBOLS)?;
    write_table(writer, 0x01, &DC_CHROMA_COUNTS, &DC_SYMBOLS)?;
    write_table(writer, 0x11, &AC_CHROMA_COUNTS, &AC_CHROMA_SYMBOLS)?;
    Ok(())
}

fn write_table<W: Write>(
    writer: &mut W,
    class_destination: u8,
    counts: &[u8; 16],
    symbols: &[u8],
) -> Result<(), ImageWriteError> {
    writer.write_all(&[class_destination])?;
    writer.write_all(counts)?;
    writer.write_all(symbols)?;
    Ok(())
}

/// Zigzag scan order: sequence position to natural row-major block index.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// One quantized 8x8 block in zigzag order with its component selector
/// (`0` luma, `1`/`2` chroma).
struct JpegBlock {
    component: usize,
    coefficients: [f32; 64],
}

/// Iterates the 4:2:0 macroblocks in scan order, each yielding four luma blocks
/// followed by one Cb and one Cr block. Out-of-image samples replicate the edge.
struct JpegEncoder<'a> {
    pixels: &'a [u8],
    width: usize,
    height: usize,
    chroma: Vec<f32>,
    chroma_stride: usize,
}

impl<'a> JpegEncoder<'a> {
    fn new(pixels: &'a [u8], width: usize, height: usize) -> Self {
        // Chroma is box-downsampled 2x2 with edge replication: each downsampled
        // sample averages the covered real texels, clamping reads at the border.
        // Empty images carry no planes; `blocks()` is empty for them as well.
        let chroma_stride = width.div_ceil(2).max(1);
        let chroma_rows = height.div_ceil(2).max(1);
        let mut chroma = Vec::new();
        if width > 0 && height > 0 {
            chroma.resize(chroma_stride.saturating_mul(chroma_rows) * 2, 0.0);
            for cy in 0..chroma_rows {
                for cx in 0..chroma_stride {
                    let mut sum_b = 0.0f32;
                    let mut sum_r = 0.0f32;
                    let mut count = 0u32;
                    for oy in 0..2 {
                        for ox in 0..2 {
                            let sx = (cx * 2 + ox).min(width - 1);
                            let sy = (cy * 2 + oy).min(height - 1);
                            let pixel = &pixels[(sy * width + sx) * 4..][..4];
                            let (cb, cr) = rgb_to_chroma(pixel[0], pixel[1], pixel[2]);
                            sum_b += cb;
                            sum_r += cr;
                            count += 1;
                        }
                    }
                    let base = (cy * chroma_stride + cx) * 2;
                    chroma[base] = sum_b / count as f32;
                    chroma[base + 1] = sum_r / count as f32;
                }
            }
        }
        Self {
            pixels,
            width,
            height,
            chroma,
            chroma_stride,
        }
    }

    fn luma_at(&self, x: usize, y: usize) -> f32 {
        let sx = x.min(self.width.saturating_sub(1));
        let sy = y.min(self.height.saturating_sub(1));
        let pixel = &self.pixels[(sy * self.width + sx) * 4..][..4];
        rgb_to_luma(pixel[0], pixel[1], pixel[2])
    }

    fn chroma_at(&self, channel: usize, x: usize, y: usize) -> f32 {
        let rows = self.chroma.len() / (self.chroma_stride * 2).max(1);
        let sx = x.min(self.chroma_stride.saturating_sub(1));
        let sy = y.min(rows.saturating_sub(1));
        self.chroma[(sy * self.chroma_stride + sx) * 2 + channel]
    }

    fn blocks(&self) -> Vec<JpegBlock> {
        let mut blocks = Vec::new();
        let mcus_x = self.width.div_ceil(16);
        let mcus_y = self.height.div_ceil(16);
        for my in 0..mcus_y {
            for mx in 0..mcus_x {
                for by in 0..2 {
                    for bx in 0..2 {
                        blocks.push(self.luma_block(mx * 16 + bx * 8, my * 16 + by * 8));
                    }
                }
                blocks.push(self.chroma_block(0, mx * 8, my * 8));
                blocks.push(self.chroma_block(1, mx * 8, my * 8));
            }
        }
        blocks
    }

    fn luma_block(&self, ox: usize, oy: usize) -> JpegBlock {
        let mut samples = [0.0f32; 64];
        for y in 0..8 {
            for x in 0..8 {
                samples[y * 8 + x] = self.luma_at(ox + x, oy + y) - 128.0;
            }
        }
        JpegBlock {
            component: 0,
            coefficients: forward_dct(&samples),
        }
    }

    fn chroma_block(&self, channel: usize, ox: usize, oy: usize) -> JpegBlock {
        let mut samples = [0.0f32; 64];
        for y in 0..8 {
            for x in 0..8 {
                samples[y * 8 + x] = self.chroma_at(channel, ox + x, oy + y) - 128.0;
            }
        }
        JpegBlock {
            component: channel + 1,
            coefficients: forward_dct(&samples),
        }
    }
}

/// JFIF luminance from 8-bit sRGB lanes.
fn rgb_to_luma(r: u8, g: u8, b: u8) -> f32 {
    f32::from(r) * 0.299 + f32::from(g) * 0.587 + f32::from(b) * 0.114
}

/// JFIF chrominance pair from 8-bit sRGB lanes.
fn rgb_to_chroma(r: u8, g: u8, b: u8) -> (f32, f32) {
    let cb = f32::from(r) * -0.168736 + f32::from(g) * -0.331264 + f32::from(b) * 0.5 + 128.0;
    let cr = f32::from(r) * 0.5 + f32::from(g) * -0.418688 + f32::from(b) * -0.081312 + 128.0;
    (cb, cr)
}

/// The textbook 8-point DCT basis: `COS[u][x] = C(u) * cos(PI * (2x + 1) * u / 16)
/// / 2`, folding one of the two `1/2` factors of the `(1/4) * C(u) * C(v)`
/// normalization into each separable pass (`C(0) = 1/sqrt(2)`, else `1`).
/// A fast AAN factorization was tried first, but differential testing against the
/// shipped encoder showed its AC responses deviate from the true DCT while the
/// reference matches the textbook transform exactly, so the direct (obviously
/// correct, precomputed-basis) form is used.
fn dct_basis(u: usize, x: usize) -> f32 {
    use std::f32::consts::{FRAC_1_SQRT_2, PI};
    let normalized = if u == 0 { FRAC_1_SQRT_2 } else { 1.0 };
    normalized * ((2 * x + 1) as f32 * u as f32 * PI / 16.0).cos() / 2.0
}

/// The true JPEG DCT over level-shifted samples: separable row/column passes
/// with the precomputed basis, giving `mean * 8` in the DC term of a flat block.
fn forward_dct(samples: &[f32; 64]) -> [f32; 64] {
    let mut basis = [[0.0f32; 8]; 8];
    for u in 0..8 {
        for x in 0..8 {
            basis[u][x] = dct_basis(u, x);
        }
    }
    let mut rows = [0.0f32; 64];
    for y in 0..8 {
        for u in 0..8 {
            let mut sum = 0.0f32;
            for x in 0..8 {
                sum += basis[u][x] * samples[y * 8 + x];
            }
            rows[y * 8 + u] = sum;
        }
    }
    let mut out = [0.0f32; 64];
    for u in 0..8 {
        for v in 0..8 {
            let mut sum = 0.0f32;
            for y in 0..8 {
                sum += basis[v][y] * rows[y * 8 + u];
            }
            out[v * 8 + u] = sum;
        }
    }
    out
}

/// Quantizes one block with the effective quality table, rounding half away from
/// zero, and returns zigzag-ordered integers.
fn quantize_block(block: &JpegBlock) -> [i32; 64] {
    let table = if block.component == 0 {
        &LUMA_Q90
    } else {
        &CHROMA_Q90
    };
    let mut out = [0i32; 64];
    for (index, &natural) in ZIGZAG.iter().enumerate() {
        let scaled = block.coefficients[natural] / f32::from(table[index]);
        let offset = if scaled < 0.0 { -0.5 } else { 0.5 };
        out[index] = (scaled + offset) as i32;
    }
    out
}

/// JPEG magnitude category: zero maps to `0`, otherwise the bit length of the
/// absolute value.
fn diff_category(value: i32) -> u32 {
    if value == 0 {
        0
    } else {
        32 - value.unsigned_abs().leading_zeros()
    }
}

/// MSB-first bit writer with `0xFF` stuffing and one-fill termination.
struct BitWriter<'w, W: Write> {
    writer: &'w mut W,
    buffer: u32,
    pending: u8,
}

impl<'w, W: Write> BitWriter<'w, W> {
    fn new(writer: &'w mut W) -> Self {
        Self {
            writer,
            buffer: 0,
            pending: 0,
        }
    }

    fn write(&mut self, table: &HuffmanTable, symbol: u32) -> Result<(), ImageWriteError> {
        let (code, length) = table.codes[symbol as usize];
        self.write_bits(u32::from(code), length)?;
        Ok(())
    }

    fn write_additional(&mut self, value: i32, category: u32) -> Result<(), ImageWriteError> {
        if category == 0 {
            return Ok(());
        }
        let bits = if value >= 0 {
            value as u32
        } else {
            (value - 1) as u32
        } & ((1u32 << category) - 1);
        self.write_bits(bits, category as u8)?;
        Ok(())
    }

    fn write_bits(&mut self, bits: u32, count: u8) -> Result<(), ImageWriteError> {
        self.buffer = (self.buffer << count) | (bits & ((1u32 << count) - 1));
        self.pending += count;
        while self.pending >= 8 {
            self.pending -= 8;
            let byte = (self.buffer >> self.pending) as u8;
            self.writer.write_all(&[byte])?;
            if byte == 0xFF {
                self.writer.write_all(&[0x00])?;
            }
            self.buffer &= (1u32 << self.pending) - 1;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(), ImageWriteError> {
        if self.pending > 0 {
            let fill = 8 - self.pending;
            self.write_bits((1u32 << fill) - 1, fill)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::image_export::ImageExportFormat;
    use super::*;

    /// Minimal scan decoder used only to cross-check the encoder: decodes one
    /// interleaved scan with the emitted Huffman tables back to per-block
    /// zigzag coefficients.
    struct BitReader<'a> {
        bytes: &'a [u8],
        position: usize,
    }

    impl<'a> BitReader<'a> {
        fn new(bytes: &'a [u8]) -> Self {
            Self { bytes, position: 0 }
        }

        fn get_bit(&mut self) -> Option<u32> {
            let byte = *self.bytes.get(self.position / 8)?;
            let bit = (byte >> (7 - (self.position % 8))) & 1;
            self.position += 1;
            Some(u32::from(bit))
        }
    }

    fn decode_symbol(reader: &mut BitReader, table: &HuffmanTable) -> Option<u8> {
        let mut code = 0u16;
        for length in 1..=16u8 {
            code = (code << 1) | reader.get_bit()? as u16;
            for (symbol, &(entry_code, entry_length)) in table.codes.iter().enumerate() {
                if entry_length == length && entry_code == code {
                    return Some(symbol as u8);
                }
            }
        }
        None
    }

    fn decode_additional(reader: &mut BitReader, size: u32) -> Option<i32> {
        if size == 0 {
            return Some(0);
        }
        let mut bits = 0u32;
        for _ in 0..size {
            bits = (bits << 1) | reader.get_bit()?;
        }
        if bits >> (size - 1) == 1 {
            Some(bits as i32)
        } else {
            Some(bits as i32 - ((1 << size) - 1))
        }
    }

    fn decode_scan(
        scan: &[u8],
        luma: &ComponentTables,
        chroma: &ComponentTables,
        block_count: usize,
    ) -> Option<Vec<[i32; 64]>> {
        let mut reader = BitReader::new(scan);
        let mut predictors = [0i32; 3];
        let mut blocks = Vec::new();
        for index in 0..block_count {
            // Scan order mirrors the encoder: Y Y Y Y Cb Cr per macroblock.
            let component = match index % 6 {
                4 | 5 => (index % 6) - 3,
                _ => 0,
            };
            let tables = if component == 0 { luma } else { chroma };
            let mut coefficients = [0i32; 64];
            let size = decode_symbol(&mut reader, &tables.dc)? as u32;
            let diff = decode_additional(&mut reader, size)?;
            predictors[component] += diff;
            coefficients[0] = predictors[component];
            let mut position = 1;
            loop {
                if position >= 64 {
                    // Ran past the last coefficient without an end-of-block: the
                    // block is complete (a nonzero at zigzag 63 needs no EOB).
                    break;
                }
                let symbol = decode_symbol(&mut reader, &tables.ac)?;
                if symbol == 0x00 {
                    break;
                }
                if symbol == 0xF0 {
                    position += 16;
                    continue;
                }
                position += (symbol >> 4) as usize;
                let size = (symbol & 0x0F) as u32;
                let value = decode_additional(&mut reader, size)?;
                if position >= 64 {
                    return None;
                }
                coefficients[position] = value;
                position += 1;
            }
            blocks.push(coefficients);
        }
        Some(blocks)
    }

    fn scan_bytes(file: &[u8]) -> &[u8] {
        let start = file
            .windows(4)
            .position(|w| w == [0xFF, 0xDA, 0x00, 0x0C])
            .expect("SOS");
        let scan_start = start + 14;
        let end = file.len() - 2;
        assert_eq!(&file[end..], &[0xFF, 0xD9]);
        &file[scan_start..end]
    }

    fn parse_hex_bytes(text: &str) -> Vec<u8> {
        assert!(text.len().is_multiple_of(2), "odd hex length");
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digit = |c: u8| {
                    (c as char)
                        .to_digit(16)
                        .unwrap_or_else(|| panic!("bad hex digit {c}"))
                };
                (digit(pair[0]) * 16 + digit(pair[1])) as u8
            })
            .collect()
    }

    #[test]
    fn scan_round_trips_through_the_decoder() {
        let luma = ComponentTables::luma();
        let chroma = ComponentTables::chroma();
        let row = include_str!("jpg_fixture.txt")
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("JPG|impulse8|"))
            .expect("impulse8 row");
        let mut parts = row.split('|');
        parts.next();
        let tag = parts.next().unwrap();
        let width: usize = parts.next().unwrap().parse().unwrap();
        let height: usize = parts.next().unwrap().parse().unwrap();
        let pixels = parse_hex_bytes(parts.next().unwrap());
        let expected = parse_hex_bytes(parts.next().unwrap());
        assert_eq!(tag, "impulse8");

        // The decoder reproduces the encoder's own quantized coefficients.
        let bitmap = Bitmap {
            width,
            height,
            depth: 1,
            is_rgba8: true,
            has_alpha: true,
            pixels: pixels.clone(),
        };
        let mut mine = Vec::new();
        save_jpg(&bitmap, &mut mine).unwrap();
        let my_blocks = decode_scan(scan_bytes(&mine), &luma, &chroma, 6).expect("decode mine");
        let encoder = JpegEncoder::new(&pixels, width, height);
        for (index, block) in encoder.blocks().iter().enumerate() {
            assert_eq!(
                my_blocks[index],
                quantize_block(block),
                "round-trip block {index}"
            );
        }

        // The reference scan decodes to the same coefficients the encoder writes
        // for the impulse image (all ten fixture files match byte-for-byte below).
        let ref_blocks = decode_scan(scan_bytes(&expected), &luma, &chroma, 6).expect("decode ref");
        for (index, block) in encoder.blocks().iter().enumerate() {
            assert_eq!(
                ref_blocks[index],
                quantize_block(block),
                "reference block {index}"
            );
        }
    }

    #[test]
    fn embedded_quant_tables_match_the_probe_dqt() {
        let row = include_str!("jpg_fixture.txt")
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("JPG|black8|"))
            .expect("black8 row");
        let file = parse_hex_bytes(row.split('|').nth(5).expect("file hex"));
        // DQT sits right after SOI + APP0.
        assert_eq!(&file[..2], &[0xFF, 0xD8]);
        assert_eq!(&file[2..4], &[0xFF, 0xE0]);
        let app0_length = u16::from_be_bytes([file[4], file[5]]) as usize;
        let dqt = &file[4 + app0_length..];
        assert_eq!(&dqt[..4], &[0xFF, 0xDB, 0x00, 0x84]);
        assert_eq!(&dqt[5..5 + 64], &LUMA_Q90);
        assert_eq!(dqt[5 + 64], 0x01);
        assert_eq!(&dqt[6 + 64..6 + 128], &CHROMA_Q90);
    }

    #[test]
    fn huffman_tables_match_the_canonical_assignments() {
        // Spot-check canonical assignments both components share for DC.
        assert_eq!(ComponentTables::luma().dc.codes[0], (0b00, 2));
        assert_eq!(ComponentTables::luma().dc.codes[9], (0b1111110, 7));
        assert_eq!(ComponentTables::chroma().dc.codes[0], (0b00, 2));
        assert_eq!(ComponentTables::chroma().dc.codes[9], (0b111111110, 9));
        // EOB and ZRL land on the standard short codes.
        assert_eq!(ComponentTables::luma().ac.codes[0x00], (0b1010, 4));
        assert_eq!(ComponentTables::chroma().ac.codes[0x00], (0b00, 2));
        assert_eq!(ComponentTables::luma().ac.codes[0xF0], (0b11111111001, 11));
    }

    #[test]
    fn jpg_matches_the_reference_files_byte_for_byte() {
        let fixture = include_str!("jpg_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('|');
            assert_eq!(parts.next(), Some("JPG"));
            let tag = parts
                .next()
                .unwrap_or_else(|| panic!("missing tag in {line}"));
            let width: usize = parts
                .next()
                .unwrap_or_else(|| panic!("missing width in {line}"))
                .parse()
                .unwrap();
            let height: usize = parts
                .next()
                .unwrap_or_else(|| panic!("missing height in {line}"))
                .parse()
                .unwrap();
            let pixels = parse_hex_bytes(
                parts
                    .next()
                    .unwrap_or_else(|| panic!("missing pixels in {line}")),
            );
            let expected = parse_hex_bytes(
                parts
                    .next()
                    .unwrap_or_else(|| panic!("missing file in {line}")),
            );
            assert_eq!(pixels.len(), width * height * 4, "pixel length in {tag}");
            let bitmap = Bitmap {
                width,
                height,
                depth: 1,
                is_rgba8: true,
                has_alpha: true,
                pixels,
            };
            let mut output = Vec::new();
            save_jpg(&bitmap, &mut output).unwrap();
            assert_eq!(output, expected, "JPEG bytes for {tag}");
            rows += 1;
        }
        assert_eq!(rows, 10, "expected ten reference JPEG files");
    }

    #[test]
    fn jpg_saves_through_the_format_dispatch() {
        let bitmap = Bitmap {
            width: 2,
            height: 2,
            depth: 1,
            is_rgba8: true,
            has_alpha: true,
            pixels: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 0, 255],
        };
        let mut output = Vec::new();
        bitmap
            .save(&mut output, ImageExportFormat::Jpeg)
            .expect("jpeg dispatch");
        assert_eq!(&output[..2], &[0xFF, 0xD8]);
        assert_eq!(&output[output.len() - 2..], &[0xFF, 0xD9]);
    }
}
