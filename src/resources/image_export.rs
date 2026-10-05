//! Image writers for `DirectBitmap.Save`.
//!
//! The reference dispatches on `ImageExportFormat` and uses FPng, `BmpWriter`, `ExrWriter`, and
//! StbImageWriteSharp. This port writes PNG, BMP, and TGA directly; EXR, HDR, and JPEG return
//! [`ImageWriteError::Unsupported`] until their codecs are ported.
//!
//! The BMP writer reproduces `BmpWriter.WriteBmp` byte-for-byte (a 108-byte `BITMAPV4HEADER`,
//! BGRA rows bottom-up). The PNG writer emits filter-0 scanlines; it is not byte-identical to
//! FPng but carries identical pixels.

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
    /// The format's codec is not yet ported.
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
            ImageExportFormat::Exr | ImageExportFormat::Hdr | ImageExportFormat::Jpeg => {
                Err(ImageWriteError::Unsupported { format })
            }
        }
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
    fn unsupported_formats_are_reported() {
        let image = bitmap(1, 1, true, vec![1, 2, 3, 4]);
        let mut output = Vec::new();
        assert!(matches!(
            image.save(&mut output, ImageExportFormat::Exr),
            Err(ImageWriteError::Unsupported {
                format: ImageExportFormat::Exr
            })
        ));
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
