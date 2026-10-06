//! Image writers for `DirectBitmap.Save`.
//!
//! The reference dispatches on `ImageExportFormat` and uses FPng, `BmpWriter`, `ExrWriter`, and
//! StbImageWriteSharp. This port writes PNG, BMP, TGA, Radiance HDR, OpenEXR, and JPEG
//! directly; every `ImageExportFormat` member now has a writer.
//!
//! The BMP writer reproduces `BmpWriter.WriteBmp` byte-for-byte (a 108-byte `BITMAPV4HEADER`,
//! BGRA rows bottom-up). The PNG writer emits filter-0 scanlines; it is not byte-identical to
//! FPng but carries identical pixels. The HDR writer reproduces StbImageWriteSharp `WriteHdr`
//! byte-for-byte (fixed header, shared-exponent RGBE pixels, new-style RLE per scanline). The
//! EXR writer reproduces `ExrWriter.Write<ColorRGBA<byte>, byte>` byte-for-byte (fixed 331-byte
//! header, per-scanline offset table, premultiplied half-float pixels planar in A/B/G/R order).

use std::io::{self, Write};

use super::texture_converter::Bitmap;

/// `ImageExportFormat`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageExportFormat {
    /// Windows bitmap.
    Bmp,
    /// OpenEXR.
    Exr,
    /// Radiance HDR.
    Hdr,
    /// JPEG.
    Jpeg,
    /// PNG.
    Png,
    /// Truevision TGA.
    Tga,
}

impl ImageExportFormat {
    /// `ImageExportFormatExtensions.GetFileExtension`.
    pub fn file_extension(self) -> &'static str {
        match self {
            Self::Bmp => "bmp",
            Self::Exr => "exr",
            Self::Hdr => "hdr",
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Tga => "tga",
        }
    }

    /// `ImageExportFormatExtensions.TryGetFromExtension`.
    pub fn try_from_extension(extension: &str) -> Option<Self> {
        match extension {
            "bmp" => Some(Self::Bmp),
            "exr" => Some(Self::Exr),
            "hdr" => Some(Self::Hdr),
            "jpeg" | "jpg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "tga" => Some(Self::Tga),
            _ => None,
        }
    }

    /// Bridge from the settings record's `ImageExportFormat` to the writer dispatch.
    ///
    /// Both enums carry the same six members in the same order; the settings side owns the
    /// discriminants while this side owns the codecs.
    pub fn from_settings_format(format: crate::export::settings::ImageExportFormat) -> Self {
        match format {
            crate::export::settings::ImageExportFormat::Bmp => Self::Bmp,
            crate::export::settings::ImageExportFormat::Exr => Self::Exr,
            crate::export::settings::ImageExportFormat::Hdr => Self::Hdr,
            crate::export::settings::ImageExportFormat::Jpeg => Self::Jpeg,
            crate::export::settings::ImageExportFormat::Png => Self::Png,
            crate::export::settings::ImageExportFormat::Tga => Self::Tga,
        }
    }
}

/// An image write failure.
#[derive(Debug)]
pub enum ImageWriteError {
    /// The underlying stream failed.
    Io(io::Error),
    /// A requested format has no writer (all current formats are covered).
    Unsupported {
        /// The requested format.
        format: ImageExportFormat,
    },
    /// A dimension does not fit the format's field width.
    DimensionTooLarge {
        /// The offending dimension.
        value: usize,
    },
}

impl From<io::Error> for ImageWriteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl core::fmt::Display for ImageWriteError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Unsupported { format } => {
                write!(formatter, "image format {format:?} is not yet supported")
            }
            Self::DimensionTooLarge { value } => {
                write!(formatter, "image dimension {value} is too large")
            }
        }
    }
}

impl std::error::Error for ImageWriteError {}

impl Bitmap {
    /// `DirectBitmap.Save`.
    ///
    /// Layers are stacked vertically, matching the reference's `Width` by `Height * Depth`.
    pub fn save<W: Write>(
        &self,
        writer: &mut W,
        format: ImageExportFormat,
    ) -> Result<(), ImageWriteError> {
        match format {
            ImageExportFormat::Png => self.save_png(writer),
            ImageExportFormat::Bmp => self.save_bmp(writer),
            ImageExportFormat::Tga => self.save_tga(writer),
            ImageExportFormat::Exr => self.save_exr(writer),
            ImageExportFormat::Jpeg => self.save_jpg(writer),
            ImageExportFormat::Hdr => self.save_hdr(writer),
        }
    }

    /// `DirectBitmap.SaveAsJpeg` (`ImageWriter.WriteJpg` with zero quality).
    pub fn save_jpg<W: Write>(&self, writer: &mut W) -> Result<(), ImageWriteError> {
        super::jpeg::save_jpg(self, writer)
    }

    fn stacked_height(&self) -> usize {
        self.height * self.depth
    }

    /// `DirectBitmap.SaveAsPng`.
    pub fn save_png<W: Write>(&self, writer: &mut W) -> Result<(), ImageWriteError> {
        let width = self.width;
        let height = self.stacked_height();
        if width > u32::MAX as usize || height > u32::MAX as usize {
            return Err(ImageWriteError::DimensionTooLarge { value: width });
        }
        let color_type = if self.has_alpha { 6u8 } else { 2u8 };
        let channels = if self.has_alpha { 4 } else { 3 };

        writer.write_all(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])?;

        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&(width as u32).to_be_bytes());
        ihdr.extend_from_slice(&(height as u32).to_be_bytes());
        ihdr.push(8); // bit depth
        ihdr.push(color_type);
        ihdr.push(0); // compression
        ihdr.push(0); // filter
        ihdr.push(0); // interlace
        write_chunk(writer, b"IHDR", &ihdr)?;

        let mut raw = Vec::with_capacity(height * (1 + width * channels));
        for y in 0..height {
            raw.push(0); // filter: None
            let row = &self.pixels[y * width * 4..(y + 1) * width * 4];
            if channels == 4 {
                raw.extend_from_slice(row);
            } else {
                for pixel in row.chunks_exact(4) {
                    raw.extend_from_slice(&pixel[..3]);
                }
            }
        }
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw)?;
        let compressed = encoder.finish()?;
        write_chunk(writer, b"IDAT", &compressed)?;
        write_chunk(writer, b"IEND", &[])?;
        Ok(())
    }

    /// `BmpWriter.WriteBmp`: a 108-byte `BITMAPV4HEADER` with BGRA rows bottom-up.
    pub fn save_bmp<W: Write>(&self, writer: &mut W) -> Result<(), ImageWriteError> {
        let width = self.width;
        let height = self.stacked_height();
        if width > i32::MAX as usize || height > i32::MAX as usize {
            return Err(ImageWriteError::DimensionTooLarge { value: width });
        }
        let raw_size = 4 * width * height;
        let total_size = 14 + 108 + raw_size;

        let mut header = Vec::with_capacity(14 + 108);
        header.extend_from_slice(b"BM");
        header.extend_from_slice(&(total_size as u32).to_le_bytes());
        header.extend_from_slice(&[0u8; 4]);
        header.extend_from_slice(&((14 + 108) as u32).to_le_bytes());
        header.extend_from_slice(&108u32.to_le_bytes());
        header.extend_from_slice(&(width as i32).to_le_bytes());
        header.extend_from_slice(&(height as i32).to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&32u16.to_le_bytes());
        header.extend_from_slice(&3u32.to_le_bytes());
        header.extend_from_slice(&(raw_size as u32).to_le_bytes());
        header.extend_from_slice(&0x0B13u32.to_le_bytes());
        header.extend_from_slice(&0x0B13u32.to_le_bytes());
        header.extend_from_slice(&[0u8; 8]);
        header.extend_from_slice(&0x00FF_0000u32.to_le_bytes()); // R
        header.extend_from_slice(&0x0000_FF00u32.to_le_bytes()); // G
        header.extend_from_slice(&0x0000_00FFu32.to_le_bytes()); // B
        header.extend_from_slice(&0xFF00_0000u32.to_le_bytes()); // A
        header.extend_from_slice(&[0x42, 0x47, 0x52, 0x73]); // sRGB
        header.extend_from_slice(&[0u8; 12]);
        header.extend_from_slice(&[0u8; 12]);
        header.extend_from_slice(&[0u8; 12]);
        header.extend_from_slice(&[0u8; 4]);
        header.extend_from_slice(&[0u8; 4]);
        header.extend_from_slice(&[0u8; 4]);
        writer.write_all(&header)?;

        // BGRA rows bottom-up.
        for y in (0..height).rev() {
            let row = &self.pixels[y * width * 4..(y + 1) * width * 4];
            let mut bgra = Vec::with_capacity(width * 4);
            for pixel in row.chunks_exact(4) {
                bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
            writer.write_all(&bgra)?;
        }
        Ok(())
    }

    /// StbImageWriteSharp-compatible TGA: uncompressed, top-down, BGRA when alpha is present.
    pub fn save_tga<W: Write>(&self, writer: &mut W) -> Result<(), ImageWriteError> {
        let width = self.width;
        let height = self.stacked_height();
        if width > u16::MAX as usize || height > u16::MAX as usize {
            return Err(ImageWriteError::DimensionTooLarge { value: width });
        }
        let mut header = [0u8; 18];
        header[2] = 2; // uncompressed true-colour
        header[12] = (width & 0xFF) as u8;
        header[13] = (width >> 8) as u8;
        header[14] = (height & 0xFF) as u8;
        header[15] = (height >> 8) as u8;
        if self.has_alpha {
            header[16] = 32;
            header[17] = 0x28; // 8 alpha bits + top-left origin
        } else {
            header[16] = 24;
            header[17] = 0x20; // top-left origin
        }
        writer.write_all(&header)?;

        for y in 0..height {
            let row = &self.pixels[y * width * 4..(y + 1) * width * 4];
            let mut bgra = Vec::with_capacity(width * 4);
            for pixel in row.chunks_exact(4) {
                if self.has_alpha {
                    bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                } else {
                    bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
                }
            }
            writer.write_all(&bgra)?;
        }
        Ok(())
    }

    /// StbImageWriteSharp `WriteHdr`: Radiance RGBE with new-style RLE scanlines.
    ///
    /// Bytes arrive as RGBA8; alpha is ignored exactly like the reference, which
    /// converts to RGB(A) bytes and lets the encoder read only the color lanes.
    /// Scanlines run top-down in stacked (`Height * Depth`) order.
    pub fn save_hdr<W: Write>(&self, writer: &mut W) -> Result<(), ImageWriteError> {
        let width = self.width;
        let height = self.stacked_height();
        if width > i32::MAX as usize || height > i32::MAX as usize {
            return Err(ImageWriteError::DimensionTooLarge { value: width });
        }
        let pixel_count = width
            .checked_mul(height)
            .ok_or(ImageWriteError::DimensionTooLarge { value: width })?;
        if self.pixels.len() < pixel_count.saturating_mul(4) {
            return Err(ImageWriteError::DimensionTooLarge { value: width });
        }
        writer.write_all(HDR_HEADER_PREFIX)?;
        writer.write_all(format!("-Y {height} +X {width}\n").as_bytes())?;

        let mut rgbe_row = vec![0u8; width.saturating_mul(4)];
        let mut encoded = Vec::new();
        for y in 0..height {
            let row = &self.pixels[y * width * 4..(y + 1) * width * 4];
            for (pixel, slot) in row.chunks_exact(4).zip(rgbe_row.chunks_exact_mut(4)) {
                slot.copy_from_slice(&rgb8_to_rgbe(pixel[0], pixel[1], pixel[2]));
            }
            if is_hdr_rle_width(width) {
                encoded.clear();
                encoded.reserve(width * 4 + 4);
                encode_hdr_scanline_rle(&rgbe_row, &mut encoded);
                writer.write_all(&encoded)?;
            } else {
                writer.write_all(&rgbe_row)?;
            }
        }
        Ok(())
    }

    /// `ExrWriter.Write<ColorRGBA<byte>, byte>`: OpenEXR scanline file, no compression.
    ///
    /// Bytes arrive as RGBA8. Alpha stays linear; RGB passes through the sRGB transfer
    /// applied to the premultiplied product (`byte/255 * alpha/255`), so the stored
    /// pixels are alpha-associated. Each scanline is channel planar in chlist order
    /// (A, B, G, R) as little-endian halves. Layers stack vertically like the other
    /// writers. Only the byte RGBA path is ported: the Rust bitmap is always RGBA8,
    /// matching the only instantiation the product pipeline can reach.
    pub fn save_exr<W: Write>(&self, writer: &mut W) -> Result<(), ImageWriteError> {
        let width = self.width;
        let height = self.stacked_height();
        if width > i32::MAX as usize || height > i32::MAX as usize {
            return Err(ImageWriteError::DimensionTooLarge { value: width });
        }
        let row_halves = width
            .checked_mul(8)
            .filter(|bytes| *bytes <= i32::MAX as usize)
            .ok_or(ImageWriteError::DimensionTooLarge { value: width })?;
        let pixel_count = width
            .checked_mul(height)
            .ok_or(ImageWriteError::DimensionTooLarge { value: width })?;
        if self.pixels.len() < pixel_count.saturating_mul(4) {
            return Err(ImageWriteError::DimensionTooLarge { value: width });
        }
        let width_i32 = width as i32;
        let height_i32 = height as i32;
        let mut header = Vec::with_capacity(331);
        write_exr_header(&mut header, width_i32, height_i32);

        // Offset table: each scanline block starts after the header, the table
        // itself, and every earlier block (`y`/`size` prefix plus pixel bytes).
        let table_start = header.len() as u64;
        writer.write_all(&header)?;
        let block_len = 8u64.saturating_add(row_halves as u64);
        for row in 0..height as u64 {
            writer.write_all(
                &(table_start
                    .saturating_add(height as u64 * 8)
                    .saturating_add(row.saturating_mul(block_len)))
                .to_le_bytes(),
            )?;
        }

        let mut halves = vec![0u16; width.saturating_mul(4)];
        for y in 0..height {
            writer.write_all(&(y as i32).to_le_bytes())?;
            writer.write_all(&(row_halves as i32).to_le_bytes())?;
            let row = &self.pixels[y * width * 4..(y + 1) * width * 4];
            for (pixel, slot) in row.chunks_exact(4).zip(halves.chunks_exact_mut(4)) {
                let alpha = f32::from(pixel[3]) / 255.0;
                slot[0] = f32_to_half_bits(alpha);
                slot[1] = rgba_byte_to_half(pixel[2], alpha);
                slot[2] = rgba_byte_to_half(pixel[1], alpha);
                slot[3] = rgba_byte_to_half(pixel[0], alpha);
            }
            // Channel planar in chlist order: all of A, then B, G, R.
            for channel in 0..4 {
                for pixel in halves.chunks_exact(4) {
                    writer.write_all(&pixel[channel].to_le_bytes())?;
                }
            }
        }
        Ok(())
    }
}

/// The OpenEXR file magic and scanline version word.
const EXR_MAGIC_VERSION: [u8; 8] = [0x76, 0x2F, 0x31, 0x01, 0x02, 0x00, 0x00, 0x00];

/// Appends the fixed `ExrWriter` header: magic, version, the A/B/G/R half-float
/// channel list, no compression, matching data/display windows, increasing-Y line
/// order, unit aspect, centered screen window of width one, and the attribute
/// terminator. The byte layout reproduces the reference writer exactly.
fn write_exr_header(header: &mut Vec<u8>, width: i32, height: i32) {
    header.extend_from_slice(&EXR_MAGIC_VERSION);
    write_exr_string(header, "channels");
    write_exr_string(header, "chlist");
    // Four 18-byte entries plus the chlist terminator.
    header.extend_from_slice(&73i32.to_le_bytes());
    for channel in *b"ABGR" {
        header.push(channel);
        header.push(0);
        header.extend_from_slice(&1i32.to_le_bytes()); // half float
        header.push(0); // perceptually linear
        header.extend_from_slice(&[0, 0, 0]); // reserved
        header.extend_from_slice(&1i32.to_le_bytes()); // x sampling
        header.extend_from_slice(&1i32.to_le_bytes()); // y sampling
    }
    header.push(0); // chlist terminator
    write_exr_attribute(header, "compression", "compression", &[0]);
    write_exr_box_attribute(header, "dataWindow", width, height);
    write_exr_box_attribute(header, "displayWindow", width, height);
    write_exr_attribute(header, "lineOrder", "lineOrder", &[0]); // increasing Y
    write_exr_float_attribute(header, "pixelAspectRatio", 1.0);
    header.extend_from_slice(b"screenWindowCenter\0v2f\0");
    header.extend_from_slice(&8i32.to_le_bytes());
    header.extend_from_slice(&[0; 8]);
    write_exr_float_attribute(header, "screenWindowWidth", 1.0);
    header.push(0); // attribute terminator
}

/// Appends one NUL-terminated OpenEXR short name.
fn write_exr_string(header: &mut Vec<u8>, value: &str) {
    header.extend_from_slice(value.as_bytes());
    header.push(0);
}

/// Appends an OpenEXR attribute with a raw payload.
fn write_exr_attribute(header: &mut Vec<u8>, name: &str, type_name: &str, value: &[u8]) {
    write_exr_string(header, name);
    write_exr_string(header, type_name);
    header.extend_from_slice(&(value.len() as i32).to_le_bytes());
    header.extend_from_slice(value);
}

/// Appends an OpenEXR `box2i` attribute covering `(0, 0)` to `(width - 1, height - 1)`.
fn write_exr_box_attribute(header: &mut Vec<u8>, name: &str, width: i32, height: i32) {
    write_exr_string(header, name);
    write_exr_string(header, "box2i");
    header.extend_from_slice(&16i32.to_le_bytes());
    header.extend_from_slice(&0i32.to_le_bytes());
    header.extend_from_slice(&0i32.to_le_bytes());
    header.extend_from_slice(&width.saturating_sub(1).to_le_bytes());
    header.extend_from_slice(&height.saturating_sub(1).to_le_bytes());
}

/// Appends an OpenEXR `float` attribute.
fn write_exr_float_attribute(header: &mut Vec<u8>, name: &str, value: f32) {
    write_exr_string(header, name);
    write_exr_string(header, "float");
    header.extend_from_slice(&4i32.to_le_bytes());
    header.extend_from_slice(&value.to_le_bytes());
}

/// An RGBA8 color lane through the `ExrWriter` pixel pipeline: the sRGB transfer
/// applied to the premultiplied product, rounded to a half-float.
///
/// `alpha` is the already-normalized alpha (`byte/255`); the reference multiplies
/// the normalized byte by it before the transfer, so the stored pixels are
/// alpha-associated.
fn rgba_byte_to_half(byte: u8, alpha: f32) -> u16 {
    let linear = f32::from(byte) / 255.0 * alpha;
    f32_to_half_bits(srgb_decode(linear))
}

/// The sRGB electro-optical transfer used by the reference writer.
fn srgb_decode(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Round-to-nearest-even `f32` to IEEE-754 binary16 bits.
///
/// Inputs from the EXR pipeline are finite values in `[0, 1]`, so only the
/// normal, subnormal, and zero arms are reachable; the infinite/NaN arms follow
/// the standard convention for completeness.
fn f32_to_half_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xFF) as i32;
    let mantissa = bits & 0x007F_FFFF;
    if exponent == 0xFF {
        // Infinity or NaN.
        return if mantissa == 0 {
            sign | 0x7C00
        } else {
            sign | 0x7E00
        };
    }
    let half_exponent = exponent - 112;
    if half_exponent >= 31 {
        // Overflow of the half range.
        sign | 0x7C00
    } else if half_exponent <= 0 {
        if half_exponent < -10 {
            // Too small even for a half subnormal.
            sign
        } else {
            // Subnormal half: the rounded result is `mantissa * 2^(half_exponent - 14)`
            // (at most ten bits), so shift the restored 24-bit mantissa right by
            // `14 - half_exponent` with round-to-nearest-even on the dropped bits.
            let shifted = mantissa | 0x0080_0000;
            let shift = (14 - half_exponent) as u32;
            let kept = shifted >> shift;
            let dropped_mask = (1u32 << shift) - 1;
            let dropped = shifted & dropped_mask;
            let halfway = 1u32 << (shift - 1);
            let round_up = dropped > halfway || (dropped == halfway && (kept & 1) == 1);
            debug_assert!(kept <= 0x3FF, "subnormal half must fit ten bits");
            sign | (kept as u16).wrapping_add(u16::from(round_up))
        }
    } else {
        // Normal half: round the 23-bit mantissa down to 10 bits.
        let kept = mantissa >> 13;
        let dropped = mantissa & 0x1FFF;
        let round_up = dropped > 0x1000 || (dropped == 0x1000 && (kept & 1) == 1);
        let rounded = kept.wrapping_add(u32::from(round_up));
        if rounded == 0x400 {
            // Mantissa overflow carries into the exponent.
            sign | (((half_exponent + 1) as u16) << 10)
        } else {
            sign | ((half_exponent as u16) << 10) | (rounded as u16)
        }
    }
}

/// The fixed StbImageWriteSharp HDR header; the `-Y {height} +X {width}` line follows.
const HDR_HEADER_PREFIX: &[u8] = b"#?RADIANCE\n# Written by stb_image_write.h\nFORMAT=32-bit_rle_rgbe\nEXPOSURE=          1.0000000000000\n\n";
/// The smallest scanline width StbImageWriteSharp encodes with RLE.
const HDR_RLE_MIN_WIDTH: usize = 8;
/// Scanlines at or above this width fall back to flat RGBE (`0x8000` in the source).
const HDR_RLE_MAX_WIDTH: usize = 0x8000;
/// The longest run or literal chunk in new-style Radiance RLE.
const HDR_RLE_MAX_CHUNK: usize = 127;
/// The longest literal chunk (the count byte also covers 128).
const HDR_LITERAL_MAX_CHUNK: usize = 128;
/// A run of at least this length encodes as a run; shorter matches stay literal.
const HDR_MIN_RUN: usize = 3;

/// Whether a scanline uses new-style RLE (`8 <= width < 0x8000` in the source).
fn is_hdr_rle_width(width: usize) -> bool {
    (HDR_RLE_MIN_WIDTH..HDR_RLE_MAX_WIDTH).contains(&width)
}

/// The shared-exponent RGBE pixel for 8-bit color lanes.
///
/// Bytes scale by `1/255` exactly like the reference float path; the exponent is
/// the `frexp` exponent biased by 128, and zero maps to all-zero bytes. Mantissa
/// truncation matches the reference float-to-byte cast.
fn rgb8_to_rgbe(red: u8, green: u8, blue: u8) -> [u8; 4] {
    let r = f32::from(red) / 255.0;
    let g = f32::from(green) / 255.0;
    let b = f32::from(blue) / 255.0;
    let brightest = r.max(g).max(b);
    if brightest < 1e-32 {
        return [0, 0, 0, 0];
    }
    // Inputs are finite normals in `[1/255, 1]`, so the raw exponent field is
    // never all-clear (subnormal) or all-set (infinite/NaN).
    let bits = brightest.to_bits();
    let exponent = (((bits >> 23) & 0xFF) as i32) - 126;
    let mantissa = f32::from_bits((bits & 0x007F_FFFF) | (126 << 23));
    let scale = mantissa * 256.0 / brightest;
    [
        (r * scale) as u8,
        (g * scale) as u8,
        (b * scale) as u8,
        (exponent + 128) as u8,
    ]
}

/// New-style Radiance RLE for one RGBE scanline: the `02 02 hi lo` marker, then
/// each color lane separately. A match of [`HDR_MIN_RUN`] or more encodes as
/// [`HDR_RLE_MAX_CHUNK`]-capped runs (an already-classified run emits even a
/// 1-2 tail chunk); anything else accumulates [`HDR_LITERAL_MAX_CHUNK`]-capped
/// literals that stop at the next run.
fn encode_hdr_scanline_rle(rgbe_row: &[u8], output: &mut Vec<u8>) {
    let width = rgbe_row.len() / 4;
    output.push(2);
    output.push(2);
    output.push((width >> 8) as u8);
    output.push((width & 0xFF) as u8);
    let mut lane = vec![0u8; width];
    for channel in 0..4 {
        for (index, slot) in lane.iter_mut().enumerate() {
            *slot = rgbe_row[index * 4 + channel];
        }
        encode_hdr_lane(&lane, output);
    }
}

fn encode_hdr_lane(lane: &[u8], output: &mut Vec<u8>) {
    let width = lane.len();
    let mut index = 0;
    while index < width {
        let mut run = 1;
        while index + run < width && lane[index + run] == lane[index] {
            run += 1;
        }
        if run >= HDR_MIN_RUN {
            let mut remaining = run;
            while remaining > 0 {
                let chunk = remaining.min(HDR_RLE_MAX_CHUNK);
                output.push(0x80 | chunk as u8);
                output.push(lane[index]);
                remaining -= chunk;
            }
            index += run;
        } else {
            let start = index;
            index += 1;
            while index < width && index - start < HDR_LITERAL_MAX_CHUNK {
                let mut ahead = 1;
                while index + ahead < width && lane[index + ahead] == lane[index] {
                    ahead += 1;
                }
                if ahead >= HDR_MIN_RUN {
                    break;
                }
                index += 1;
            }
            output.push((index - start) as u8);
            output.extend_from_slice(&lane[start..index]);
        }
    }
}

fn write_chunk<W: Write>(writer: &mut W, kind: &[u8; 4], data: &[u8]) -> io::Result<()> {
    writer.write_all(&(data.len() as u32).to_be_bytes())?;
    writer.write_all(kind)?;
    writer.write_all(data)?;
    let mut crc = Crc32::new();
    crc.update(kind);
    crc.update(data);
    writer.write_all(&crc.finalize().to_be_bytes())?;
    Ok(())
}

/// The PNG CRC-32 (polynomial `0xEDB88320`).
struct Crc32 {
    state: u32,
}

impl Crc32 {
    fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.state ^= u32::from(byte);
            for _ in 0..8 {
                let mask = (self.state & 1).wrapping_neg();
                self.state = (self.state >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
    }

    fn finalize(self) -> u32 {
        !self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(width: usize, height: usize, has_alpha: bool, pixels: Vec<u8>) -> Bitmap {
        Bitmap {
            width,
            height,
            depth: 1,
            is_rgba8: has_alpha,
            has_alpha,
            pixels,
        }
    }

    #[test]
    fn png_starts_with_the_signature_and_chunks() {
        let image = bitmap(1, 1, true, vec![1, 2, 3, 4]);
        let mut output = Vec::new();
        image.save_png(&mut output).unwrap();
        assert_eq!(
            &output[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
        assert!(output.windows(4).any(|w| w == b"IHDR"));
        assert!(output.windows(4).any(|w| w == b"IDAT"));
        assert!(output.windows(4).any(|w| w == b"IEND"));
        // IHDR colour type 6 (RGBA) for an alpha bitmap.
        assert_eq!(output[8 + 8 + 9], 6);
    }

    #[test]
    fn png_uses_rgb_for_non_alpha_bitmaps() {
        let image = bitmap(1, 1, false, vec![1, 2, 3, 255]);
        let mut output = Vec::new();
        image.save_png(&mut output).unwrap();
        assert_eq!(output[8 + 8 + 9], 2);
    }

    #[test]
    fn bmp_matches_the_reference_layout() {
        let image = bitmap(1, 1, true, vec![10, 20, 30, 40]);
        let mut output = Vec::new();
        image.save_bmp(&mut output).unwrap();
        assert_eq!(&output[..2], b"BM");
        assert_eq!(output.len(), 14 + 108 + 4);
        // Bottom-left BGRA pixel: B=30, G=20, R=10, A=40.
        assert_eq!(&output[122..], &[30, 20, 10, 40]);
    }

    #[test]
    fn tga_writes_a_top_down_bgra_image() {
        let image = bitmap(1, 1, true, vec![10, 20, 30, 40]);
        let mut output = Vec::new();
        image.save_tga(&mut output).unwrap();
        assert_eq!(output[2], 2);
        assert_eq!(output[16], 32);
        assert_eq!(output[17], 0x28);
        assert_eq!(&output[18..], &[30, 20, 10, 40]);
    }

    #[test]
    fn settings_formats_bridge_to_the_matching_writer_format() {
        use crate::export::settings::ImageExportFormat as SettingsFormat;

        for settings in [
            SettingsFormat::Bmp,
            SettingsFormat::Exr,
            SettingsFormat::Hdr,
            SettingsFormat::Jpeg,
            SettingsFormat::Png,
            SettingsFormat::Tga,
        ] {
            let writer = ImageExportFormat::from_settings_format(settings);
            // Same member, same file extension, same discriminant on both sides.
            assert_eq!(writer.file_extension(), settings.file_extension());
            assert_eq!(
                ImageExportFormat::try_from_extension(settings.file_extension()),
                Some(writer)
            );
        }
    }

    #[test]
    fn jpeg_saves_through_the_format_dispatch() {
        let image = bitmap(1, 1, true, vec![255, 0, 0, 255]);
        let mut output = Vec::new();
        image.save(&mut output, ImageExportFormat::Jpeg).unwrap();
        assert_eq!(&output[..2], &[0xFF, 0xD8]);
        assert_eq!(&output[output.len() - 2..], &[0xFF, 0xD9]);
    }

    #[test]
    fn exr_saves_through_the_format_dispatch() {
        let image = bitmap(1, 1, true, vec![128, 128, 128, 255]);
        let mut output = Vec::new();
        image.save(&mut output, ImageExportFormat::Exr).unwrap();
        assert_eq!(&output[..4], &[0x76, 0x2F, 0x31, 0x01]);
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

    fn parse_half_words(text: &str) -> Vec<u16> {
        assert!(text.len().is_multiple_of(4), "odd half-word hex length");
        parse_hex_bytes(text)
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    #[test]
    fn exr_matches_the_reference_files_byte_for_byte() {
        let fixture = include_str!("exr_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !line.starts_with("FILE|") {
                continue;
            }
            let mut parts = line.split('|');
            assert_eq!(parts.next(), Some("FILE"));
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
            let image = bitmap(width, height, true, pixels);
            let mut output = Vec::new();
            image.save_exr(&mut output).unwrap();
            assert_eq!(output, expected, "EXR bytes for {tag}");
            rows += 1;
        }
        assert_eq!(rows, 4, "expected four reference EXR files");
    }

    #[test]
    fn exr_conversion_tables_match_the_reference() {
        let fixture = include_str!("exr_fixture.txt");
        let mut srgb: Option<Vec<u16>> = None;
        let mut alpha: Option<Vec<u16>> = None;
        for line in fixture.lines() {
            let line = line.trim();
            if let Some(table) = line.strip_prefix("SRGB|") {
                srgb = Some(parse_half_words(table));
            } else if let Some(table) = line.strip_prefix("ALPHA|") {
                alpha = Some(parse_half_words(table));
            }
        }
        let srgb = srgb.expect("SRGB table");
        let alpha = alpha.expect("ALPHA table");
        assert_eq!(srgb.len(), 256, "sRGB table length");
        assert_eq!(alpha.len(), 256, "alpha table length");
        for (byte, expected) in srgb.iter().enumerate() {
            // Gray ramp pixels carry alpha 255, so the product is the byte alone.
            assert_eq!(
                rgba_byte_to_half(byte as u8, 1.0),
                *expected,
                "sRGB half for byte {byte}"
            );
        }
        for (byte, expected) in alpha.iter().enumerate() {
            assert_eq!(
                f32_to_half_bits(f32::from(byte as u8) / 255.0),
                *expected,
                "linear half for alpha {byte}"
            );
        }
        // Premultiplied spot checks from the grid and threshold probes, covering the
        // sRGB linear/pow sides on both the direct (alpha 255) and scaled paths:
        // (byte, alpha).
        for ((byte, alpha), expected) in [
            ((1u8, 4u8), 0x0050u16),
            ((2, 4), 0x00A0),
            ((3, 4), 0x00F0),
            ((15, 16), 0x0CAE),
            ((15, 240), 0x1C8B),
            ((11, 16), 0x0ADD),
            ((164, 16), 0x1A66),
            ((165, 16), 0x1A70),
        ] {
            assert_eq!(
                rgba_byte_to_half(byte, f32::from(alpha) / 255.0),
                expected,
                "premultiplied half for byte {byte}, alpha {alpha}"
            );
        }
    }

    #[test]
    fn exr_stacks_depth_layers_as_scanlines() {
        // Two 1x1 layers stack to a two-scanline file with chained offsets.
        let image = Bitmap {
            width: 1,
            height: 1,
            depth: 2,
            is_rgba8: true,
            has_alpha: true,
            pixels: vec![255, 255, 255, 255, 0, 0, 0, 0],
        };
        let mut output = Vec::new();
        image.save_exr(&mut output).unwrap();
        // dataWindow/displayWindow cover (0, 0) to (0, stacked height - 1).
        assert_eq!(output.len(), 331 + 16 + 2 * (8 + 8));
        let offset0 = u64::from_le_bytes(output[331..339].try_into().unwrap());
        let offset1 = u64::from_le_bytes(output[339..347].try_into().unwrap());
        assert_eq!(offset0, 331 + 16);
        assert_eq!(offset1, offset0 + 8 + 8);
        // First scanline is white (four 1.0 halves), second is transparent black.
        let row0 = offset0 as usize;
        assert_eq!(&output[row0..row0 + 8], &[0, 0, 0, 0, 8, 0, 0, 0]);
        assert_eq!(
            &output[row0 + 8..row0 + 16],
            &[0x00, 0x3C, 0x00, 0x3C, 0x00, 0x3C, 0x00, 0x3C]
        );
        let row1 = offset1 as usize;
        assert_eq!(&output[row1..row1 + 8], &[1, 0, 0, 0, 8, 0, 0, 0]);
        assert_eq!(&output[row1 + 8..row1 + 16], &[0; 8]);
    }

    #[test]
    fn hdr_saves_through_the_format_dispatch() {
        let image = bitmap(1, 1, true, vec![255, 255, 255, 255]);
        let mut output = Vec::new();
        image.save(&mut output, ImageExportFormat::Hdr).unwrap();
        assert!(output.starts_with(b"#?RADIANCE\n"));
    }

    #[test]
    fn hdr_matches_the_reference_writer_byte_for_byte() {
        let fixture = include_str!("hdr_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts.len(), 6, "{line:?}");
            assert_eq!(parts[0], "VEC");
            let width: usize = parts[2].parse().unwrap();
            let height: usize = parts[3].parse().unwrap();
            let pixels = parse_hex(parts[4]);
            let expected = parse_hex(parts[5]);
            assert_eq!(pixels.len(), width * height * 4, "{}", parts[1]);
            let image = bitmap(width, height, true, pixels);
            let mut output = Vec::new();
            image.save_hdr(&mut output).unwrap();
            assert_eq!(output, expected, "{}", parts[1]);
            rows += 1;
        }
        assert_eq!(rows, 22, "all fixture vectors are asserted");
    }

    #[test]
    fn hdr_boundary_widths_follow_the_flat_rle_rule() {
        // 32767-wide solid pixels stay RLE: 115-byte header, the `02 02 7F FF`
        // marker, then `R127 x 258 + R1` per lane. The probe reported 2191 bytes.
        let pixels = [200u8, 100, 50, 255].repeat(32767);
        let image = bitmap(32767, 1, true, pixels);
        let mut output = Vec::new();
        image.save_hdr(&mut output).unwrap();
        assert_eq!(output.len(), 2191);
        let marker = [2u8, 2, 0x7F, 0xFF];
        assert_eq!(&output[115..119], &marker);
        let mut lane = Vec::new();
        lane.extend_from_slice(&[0xFF, 200].repeat(258));
        lane.extend_from_slice(&[0x81, 200]);
        assert_eq!(&output[119..119 + 518], lane.as_slice());
        // 32768-wide pixels go flat: 115-byte header plus raw RGBE. The probe
        // reported 131187 bytes with an `C8643280` tail.
        let pixels = [200u8, 100, 50, 255].repeat(32768);
        let image = bitmap(32768, 1, true, pixels);
        let mut output = Vec::new();
        image.save_hdr(&mut output).unwrap();
        assert_eq!(output.len(), 115 + 32768 * 4);
        assert_eq!(&output[115..119], &[0xC8, 0x64, 0x32, 0x80]);
        assert_eq!(&output[output.len() - 4..], &[0xC8, 0x64, 0x32, 0x80]);
    }

    #[test]
    fn hdr_ignores_alpha_like_the_reference() {
        let opaque = bitmap(2, 1, true, vec![200, 100, 50, 255, 10, 20, 30, 255]);
        let transparent = bitmap(2, 1, true, vec![200, 100, 50, 0, 10, 20, 30, 0]);
        let mut first = Vec::new();
        let mut second = Vec::new();
        opaque.save_hdr(&mut first).unwrap();
        transparent.save_hdr(&mut second).unwrap();
        assert_eq!(first, second);
    }

    fn parse_hex(text: &str) -> Vec<u8> {
        assert!(text.len().is_multiple_of(2), "even hex length");
        let mut bytes = Vec::with_capacity(text.len() / 2);
        let digits = text.as_bytes();
        for pair in digits.chunks_exact(2) {
            bytes.push(hex_value(pair[0]) << 4 | hex_value(pair[1]));
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

    #[test]
    fn extensions_round_trip() {
        assert_eq!(ImageExportFormat::Png.file_extension(), "png");
        assert_eq!(
            ImageExportFormat::try_from_extension("jpg"),
            Some(ImageExportFormat::Jpeg)
        );
        assert_eq!(ImageExportFormat::try_from_extension("tif"), None);
    }
}
