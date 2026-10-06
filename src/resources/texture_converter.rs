//! `TextureConverter` ported from
//! `AssetRipper.Export.Modules.Textures/TextureConverter.cs`.
//!
//! The reference selects a decoder from `TextureFormat` when the texture carries a non-default
//! format, otherwise from `GraphicsFormat`, decodes each mip/layer into a pixel buffer, and then
//! flips the image vertically (except cubemaps) and optionally unpacks a DXT5nm normal map.
//!
//! This port decodes every format to RGBA8, which is the observable output for the PNG path. The
//! only place the reference's generic pixel type matters is `UnpackNormal`, which it applies only
//! when the bitmap is `ColorRGBA<byte>`; [`is_rgba8_target`] tracks that condition.
//!
//! Crunch decompression (`CrunchHandler`) is not yet ported; a crunched format returns
//! [`TextureConvertError::CrunchUnsupported`] rather than silently decoding wrong data.
//! [`is_use_unity_crunch`] records which external decompressor the reference would call
//! (`UnityCrunch` vs `Crunch`); both live in external NuGet packages with no in-snapshot
//! implementation, so both paths still report `CrunchUnsupported`.

use super::astc::decode_astc;
use super::atc_yuy2::{decode_atc_rgb4, decode_atc_rgba8, decode_yuy2};
use super::bc6h::{decode_bc6h, decode_bc6h_signed};
use super::bc7::decode_bc7;
use super::etc_decode::{
    decode_eac_r_signed, decode_eac_r_unsigned, decode_eac_rg_signed, decode_eac_rg_unsigned,
    decode_etc1, decode_etc2, decode_etc2a1, decode_etc2a8,
};
use super::pvrtc::{decode_pvrtc_2bpp, decode_pvrtc_4bpp};
use super::texture_decode::{
    decode_a16, decode_alpha8, decode_argb32, decode_argb4444, decode_argb_float, decode_bc4,
    decode_bc5, decode_bgr24, decode_bgr24_signed, decode_bgra32, decode_bgra32_signed,
    decode_dxt1, decode_dxt3, decode_dxt5, decode_r16, decode_r16_signed, decode_r32_signed,
    decode_r32_unsigned, decode_r32g32_signed, decode_r32g32_unsigned, decode_r32g32b32_signed,
    decode_r32g32b32_unsigned, decode_r32g32b32a32_signed, decode_r32g32b32a32_unsigned, decode_r8,
    decode_r8_signed, decode_r_float, decode_r_half, decode_rg16, decode_rg16_signed, decode_rg32,
    decode_rg32_signed, decode_rg_float, decode_rg_half, decode_rgb24, decode_rgb24_signed,
    decode_rgb48, decode_rgb48_signed, decode_rgb565, decode_rgb9e5_float, decode_rgb_float,
    decode_rgb_half, decode_rgba32, decode_rgba32_signed, decode_rgba4444, decode_rgba64,
    decode_rgba64_signed, decode_rgba_float, decode_rgba_half, TextureDecodeError,
};
use super::texture_format::is_crunched;
use super::vertex_data_blob::UnityVersion;

/// A texture conversion failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextureConvertError {
    /// The texture has a non-positive dimension.
    InvalidDimensions {
        /// Texture width.
        width: usize,
        /// Texture height.
        height: usize,
        /// Texture depth.
        depth: usize,
    },
    /// `width * height * depth * 4` overflows `i32`.
    TextureTooLarge {
        /// Texture width.
        width: usize,
        /// Texture height.
        height: usize,
        /// Texture depth.
        depth: usize,
    },
    /// The image data is shorter than `ImageSize * Depth`.
    ImageDataTooShort {
        /// Provided byte count.
        actual: usize,
        /// Required byte count.
        required: usize,
    },
    /// The format is block-crunched and Crunch decompression is not yet ported.
    CrunchUnsupported {
        /// The crunched `TextureFormat` discriminant.
        texture_format: i32,
    },
    /// Neither the `TextureFormat` nor the `GraphicsFormat` is handled by the reference switch.
    UnsupportedFormat {
        /// The `TextureFormat` discriminant (`0` when the graphics format is used).
        texture_format: i32,
        /// The `GraphicsFormat` discriminant.
        graphics_format: i32,
    },
    /// A decoder rejected the block or pixel buffer.
    Decode(TextureDecodeError),
}

impl From<TextureDecodeError> for TextureConvertError {
    fn from(error: TextureDecodeError) -> Self {
        Self::Decode(error)
    }
}

impl core::fmt::Display for TextureConvertError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidDimensions {
                width,
                height,
                depth,
            } => write!(
                formatter,
                "invalid texture dimensions {width}x{height}x{depth}"
            ),
            Self::TextureTooLarge {
                width,
                height,
                depth,
            } => write!(formatter, "texture {width}x{height}x{depth} is too large"),
            Self::ImageDataTooShort { actual, required } => write!(
                formatter,
                "image data has {actual} bytes; needs at least {required}"
            ),
            Self::CrunchUnsupported { texture_format } => write!(
                formatter,
                "crunched texture format {texture_format} is not yet supported"
            ),
            Self::UnsupportedFormat {
                texture_format,
                graphics_format,
            } => write!(
                formatter,
                "unsupported texture format {texture_format} / graphics format {graphics_format}"
            ),
            Self::Decode(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for TextureConvertError {}

/// `CrunchHandler.IsUseUnityCrunch`.
///
/// Unity 2017.3 and later always decompress through `UnityCrunch`; older versions use it
/// only for the ETC crunched formats and fall back to `Crunch` otherwise. Both callees are
/// external NuGet packages (`AssetRipper.Conversions.UnityCrunch` 1.0.3 and
/// `AssetRipper.Conversions.Crunch` 1.0.3) with no implementation in the frozen snapshot,
/// so this gate only records which external decompressor the reference would call.
pub fn is_use_unity_crunch(version: UnityVersion, texture_format: i32) -> bool {
    if version.at_least_at(2017, 3) {
        return true;
    }
    matches!(
        texture_format,
        64 /* ETC_RGB4Crunched */ | 65 /* ETC2_RGBA8Crunched */
    )
}

/// The inputs `TextureConverter.TryConvertToBitmap` reads from an image texture.
#[derive(Clone, Copy, Debug)]
pub struct TextureImage<'a> {
    /// `TextureFormat` discriminant; `0` means the graphics format is authoritative.
    pub texture_format: i32,
    /// `GraphicsFormat` discriminant.
    pub graphics_format: i32,
    /// Texture width.
    pub width: usize,
    /// Texture height.
    pub height: usize,
    /// Layer count (`Depth`).
    pub depth: usize,
    /// `GetCompleteImageSize()` / `ActualImageSize`.
    pub image_size: usize,
    /// Raw image bytes.
    pub data: &'a [u8],
}

/// An RGBA8 image buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bitmap {
    /// Image width.
    pub width: usize,
    /// Image height.
    pub height: usize,
    /// Layer count.
    pub depth: usize,
    /// Whether the reference's pixel type is `ColorRGBA<byte>` (controls `UnpackNormal`).
    pub is_rgba8: bool,
    /// Whether the reference's pixel type has an alpha channel (controls the PNG colour type).
    pub has_alpha: bool,
    /// Row-major RGBA8 pixels, `width * height * depth * 4` bytes.
    pub pixels: Vec<u8>,
}

impl Bitmap {
    /// `DirectBitmap.FlipY`: reverse the rows of each layer in place.
    pub fn flip_y(&mut self) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        let row_bytes = self.width * 4;
        let layer_rows = self.height;
        for layer in 0..self.depth {
            let base = layer * layer_rows * row_bytes;
            for row in 0..layer_rows / 2 {
                let top = base + row * row_bytes;
                let bottom = base + (layer_rows - 1 - row) * row_bytes;
                for i in 0..row_bytes {
                    self.pixels.swap(top + i, bottom + i);
                }
            }
        }
    }

    /// `TextureConverter.UnpackNormal` for a `ColorRGBA<byte>` bitmap.
    ///
    /// The reference reads `(a, g, _, r)`, recomputes blue from the red/green tangent-space
    /// components, and writes `(r, g, b, a)` back. It only acts when the bitmap is RGBA8.
    pub fn unpack_normal(&mut self) {
        if !self.is_rgba8 {
            return;
        }
        const MAGNITUDE_SQR: f64 = 255.0 * 255.0;
        for pixel in self.pixels.chunks_exact_mut(4) {
            let a = pixel[3];
            let g = pixel[1];
            let r = pixel[0];
            let vr = f64::from(r) * 2.0 - 255.0;
            let vg = f64::from(g) * 2.0 - 255.0;
            let hypotenuse_sqr = (vr * vr + vg * vg).min(MAGNITUDE_SQR);
            let vb = (MAGNITUDE_SQR - hypotenuse_sqr).sqrt();
            let b_exact = (vb + 255.0) / 2.0;
            let b = double_to_byte(b_exact / 255.0);
            pixel[0] = r;
            pixel[1] = g;
            pixel[2] = b;
            pixel[3] = a;
        }
    }
}

fn double_to_byte(value: f64) -> u8 {
    let x = value * 255.0;
    if x > 255.0 {
        255
    } else if x > 0.0 {
        x as u8
    } else {
        0
    }
}

/// `TextureConverter.TryConvertToBitmap`: decode, then flip and optionally unpack a normal map.
pub fn try_convert_to_bitmap(
    image: &TextureImage<'_>,
    flip_y: bool,
    unpack_normal: bool,
) -> Result<Bitmap, TextureConvertError> {
    let mut bitmap = convert_to_bitmap(image)?;
    if flip_y {
        bitmap.flip_y();
    }
    if unpack_normal {
        bitmap.unpack_normal();
    }
    Ok(bitmap)
}

/// `TextureConverter.TryConvertToBitmap<TColor, TChannelValue>` for the pixel buffer only.
pub fn convert_to_bitmap(image: &TextureImage<'_>) -> Result<Bitmap, TextureConvertError> {
    if image.width == 0 || image.height == 0 || image.depth == 0 {
        return Err(TextureConvertError::InvalidDimensions {
            width: image.width,
            height: image.height,
            depth: image.depth,
        });
    }

    let pixel_bytes = image
        .width
        .checked_mul(image.height)
        .and_then(|v| v.checked_mul(image.depth))
        .and_then(|v| v.checked_mul(4))
        .ok_or(TextureConvertError::TextureTooLarge {
            width: image.width,
            height: image.height,
            depth: image.depth,
        })?;
    if pixel_bytes > i32::MAX as usize {
        return Err(TextureConvertError::TextureTooLarge {
            width: image.width,
            height: image.height,
            depth: image.depth,
        });
    }

    if is_crunched(image.texture_format) {
        return Err(TextureConvertError::CrunchUnsupported {
            texture_format: image.texture_format,
        });
    }

    // `bytesPerLayer`: `None` mirrors the source's `-1` for a Texture3D whose data is exactly one
    // image, where the per-layer stride is taken from the decoder's consumed byte count.
    let bytes_per_layer = if image.data.len() == image.image_size {
        None
    } else if image.data.len() < image.image_size.saturating_mul(image.depth) {
        return Err(TextureConvertError::ImageDataTooShort {
            actual: image.data.len(),
            required: image.image_size.saturating_mul(image.depth),
        });
    } else {
        Some(image.image_size)
    };

    let layer_bytes = image.width * image.height * 4;
    let mut bitmap = Bitmap {
        width: image.width,
        height: image.height,
        depth: image.depth,
        is_rgba8: is_rgba8_target(image.texture_format),
        has_alpha: has_alpha(image.texture_format),
        pixels: vec![0; pixel_bytes],
    };

    let mut input_offset = 0usize;
    for layer in 0..image.depth {
        let input_span = match bytes_per_layer {
            Some(stride) => &image.data[input_offset..input_offset + stride],
            None => &image.data[input_offset..],
        };
        let output_span = &mut bitmap.pixels[layer * layer_bytes..(layer + 1) * layer_bytes];
        let bytes_read = decode_texture(
            image.texture_format,
            image.graphics_format,
            input_span,
            image.width,
            image.height,
            output_span,
        )?;
        input_offset += bytes_per_layer.unwrap_or(bytes_read);
    }

    Ok(bitmap)
}

/// The reference's outer `TryConvertToBitmap` pixel type is `ColorRGBA<byte>` for these formats.
pub fn is_rgba8_target(texture_format: i32) -> bool {
    if texture_format == 0 {
        return true;
    }
    !matches!(
        texture_format,
        1 | 2
            | 3
            | 5
            | 6
            | 7
            | 8
            | 9
            | 13
            | 14
            | 15
            | 16
            | 17
            | 18
            | 19
            | 20
            | 22
            | 37
            | 62
            | 63
            | 72
            | 73
            | 74
            | 75
            | 76
            | 77
            | 78
            | 79
            | 80
            | 81
            | 82
    )
}

/// The reference's outer `TryConvertToBitmap` pixel type has an alpha channel for these formats.
pub fn has_alpha(texture_format: i32) -> bool {
    if texture_format == 0 {
        return true;
    }
    !matches!(
        texture_format,
        3 | 7 | 8 | 9 | 15 | 16 | 18 | 19 | 22 | 62 | 63 | 72 | 73 | 75 | 76 | 77 | 79 | 80 | 81
    )
}

type Decoder = fn(&[u8], usize, usize, &mut [u8]) -> Result<usize, TextureDecodeError>;

fn decode_astc_4x4(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_astc(input, width, height, 4, 4, output)
}

fn decode_astc_5x5(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_astc(input, width, height, 5, 5, output)
}

fn decode_astc_6x6(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_astc(input, width, height, 6, 6, output)
}

fn decode_astc_8x8(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_astc(input, width, height, 8, 8, output)
}

fn decode_astc_10x10(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_astc(input, width, height, 10, 10, output)
}

fn decode_astc_12x12(
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureDecodeError> {
    decode_astc(input, width, height, 12, 12, output)
}

/// `TextureConverter.DecodeTexture`.
pub fn decode_texture(
    texture_format: i32,
    graphics_format: i32,
    input: &[u8],
    width: usize,
    height: usize,
    output: &mut [u8],
) -> Result<usize, TextureConvertError> {
    if texture_format != 0 {
        let decoder: Decoder = match texture_format {
            // ASTC
            48 | 54 => decode_astc_4x4,
            49 | 55 => decode_astc_5x5,
            50 | 56 => decode_astc_6x6,
            51 | 57 => decode_astc_8x8,
            52 | 58 => decode_astc_10x10,
            53 | 59 => decode_astc_12x12,
            // ATC
            35 => decode_atc_rgb4,
            36 => decode_atc_rgba8,
            // BC
            26 => decode_bc4,
            27 => decode_bc5,
            24 => decode_bc6h,
            25 => decode_bc7,
            // DXT
            10 | 28 => decode_dxt1,
            11 => decode_dxt3,
            12 | 29 => decode_dxt5,
            // ETC
            34 | 60 | 64 => decode_etc1,
            41 => decode_eac_r_unsigned,
            42 => decode_eac_r_signed,
            43 => decode_eac_rg_unsigned,
            44 => decode_eac_rg_signed,
            45 => decode_etc2,
            46 => decode_etc2a1,
            47 | 61 | 65 => decode_etc2a8,
            // PVRTC
            30 | 31 => decode_pvrtc_2bpp,
            32 | 33 => decode_pvrtc_4bpp,
            // YUY2
            21 => decode_yuy2,
            // RGB
            1 => decode_alpha8,
            2 => decode_argb4444,
            13 => decode_rgba4444,
            7 => decode_rgb565,
            63 => decode_r8,
            62 => decode_rg16,
            3 => decode_rgb24,
            4 => decode_rgba32,
            5 => decode_argb32,
            6 => decode_argb_float,
            8 => decode_bgr24,
            14 | 37 => decode_bgra32,
            9 => decode_r16,
            72 => decode_rg32,
            73 => decode_rgb48,
            74 => decode_rgba64,
            15 => decode_r_half,
            16 => decode_rg_half,
            17 => decode_rgba_half,
            18 => decode_r_float,
            19 => decode_rg_float,
            20 => decode_rgba_float,
            22 => decode_rgb9e5_float,
            75 => decode_r8_signed,
            76 => decode_rg16_signed,
            77 => decode_rgb24_signed,
            78 => decode_rgba32_signed,
            79 => decode_r16_signed,
            80 => decode_rg32_signed,
            81 => decode_rgb48_signed,
            82 => decode_rgba64_signed,
            _ => {
                return Err(TextureConvertError::UnsupportedFormat {
                    texture_format,
                    graphics_format,
                })
            }
        };
        return decoder(input, width, height, output).map_err(Into::into);
    }

    let decoder: Decoder = match graphics_format {
        1 | 5 | 13 => decode_r8,
        2 | 6 | 14 => decode_rg16,
        3 | 7 | 15 => decode_rgb24,
        4 | 8 | 16 => decode_rgba32,
        9 | 17 => decode_r8_signed,
        10 | 18 => decode_rg16_signed,
        11 | 19 => decode_rgb24_signed,
        12 | 20 => decode_rgba32_signed,
        21 | 29 => decode_r16,
        22 | 30 => decode_rg32,
        23 | 31 => decode_rgb48,
        24 | 32 => decode_rgba64,
        25 | 33 => decode_r16_signed,
        26 | 34 => decode_rg32_signed,
        27 | 35 => decode_rgb48_signed,
        28 | 36 => decode_rgba64_signed,
        37 => decode_r32_unsigned,
        38 => decode_r32g32_unsigned,
        39 => decode_r32g32b32_unsigned,
        40 => decode_r32g32b32a32_unsigned,
        41 => decode_r32_signed,
        42 => decode_r32g32_signed,
        43 => decode_r32g32b32_signed,
        44 => decode_r32g32b32a32_signed,
        45 => decode_r_half,
        46 => decode_rg_half,
        47 => decode_rgb_half,
        48 => decode_rgba_half,
        49 => decode_r_float,
        50 => decode_rg_float,
        51 => decode_rgb_float,
        52 => decode_rgba_float,
        54 => decode_alpha8,
        55 => decode_a16,
        56 | 58 | 62 => decode_bgr24,
        57 | 59 | 63 => decode_bgra32,
        60 | 64 => decode_bgr24_signed,
        61 | 65 => decode_bgra32_signed,
        66 => decode_rgba4444,
        68 => decode_rgb565,
        87 | 88 => decode_argb32,
        89 => decode_argb_float,
        90 => decode_r16,
        93 => decode_r_float,
        95 => decode_alpha8,
        96 | 97 => decode_dxt1,
        98 | 99 => decode_dxt3,
        100 | 101 => decode_dxt5,
        102 | 103 => decode_bc4,
        104 | 105 => decode_bc5,
        106 => decode_bc6h,
        107 => decode_bc6h_signed,
        108 | 109 => decode_bc7,
        110 | 111 | 114 | 115 => decode_pvrtc_2bpp,
        112 | 113 | 116 | 117 => decode_pvrtc_4bpp,
        118 => decode_etc1,
        119 | 120 => decode_etc2,
        121 | 122 => decode_etc2a1,
        123 | 124 => decode_etc2a8,
        125 => decode_eac_r_unsigned,
        126 => decode_eac_r_signed,
        127 => decode_eac_rg_unsigned,
        128 => decode_eac_rg_signed,
        129 | 130 => decode_astc_4x4,
        131 | 132 => decode_astc_5x5,
        133 | 134 => decode_astc_6x6,
        135 | 136 => decode_astc_8x8,
        137 | 138 => decode_astc_10x10,
        139 | 140 => decode_astc_12x12,
        _ => {
            return Err(TextureConvertError::UnsupportedFormat {
                texture_format,
                graphics_format,
            })
        }
    };
    decoder(input, width, height, output).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(texture_format: i32, width: usize, height: usize, data: &[u8]) -> Bitmap {
        let image = TextureImage {
            texture_format,
            graphics_format: 0,
            width,
            height,
            depth: 1,
            image_size: data.len(),
            data,
        };
        convert_to_bitmap(&image).unwrap()
    }

    #[test]
    fn flip_y_reverses_rows_per_layer() {
        let mut bitmap = Bitmap {
            width: 2,
            height: 2,
            depth: 1,
            is_rgba8: true,
            has_alpha: true,
            pixels: (0u8..16).collect(),
        };
        bitmap.flip_y();
        assert_eq!(
            bitmap.pixels,
            vec![8, 9, 10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn rgba32_is_decoded_verbatim() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8];
        let bitmap = solid(4, 2, 1, &data);
        assert_eq!(bitmap.pixels, data);
        assert!(bitmap.is_rgba8);
    }

    #[test]
    fn rgb24_gets_an_opaque_alpha() {
        let data = [10, 20, 30, 40, 50, 60];
        let bitmap = solid(3, 2, 1, &data);
        assert_eq!(bitmap.pixels, vec![10, 20, 30, 255, 40, 50, 60, 255]);
        assert!(!bitmap.is_rgba8);
    }

    #[test]
    fn short_image_data_is_rejected() {
        let image = TextureImage {
            texture_format: 4,
            graphics_format: 0,
            width: 4,
            height: 4,
            depth: 1,
            image_size: 64,
            data: &[0; 32],
        };
        assert!(matches!(
            convert_to_bitmap(&image),
            Err(TextureConvertError::ImageDataTooShort { .. })
        ));
    }

    #[test]
    fn crunched_format_is_reported() {
        let image = TextureImage {
            texture_format: 28,
            graphics_format: 0,
            width: 4,
            height: 4,
            depth: 1,
            image_size: 8,
            data: &[0; 8],
        };
        assert_eq!(
            convert_to_bitmap(&image),
            Err(TextureConvertError::CrunchUnsupported { texture_format: 28 })
        );
    }

    #[test]
    fn crunch_gate_selects_the_external_decompressor() {
        let new = UnityVersion::parse("2017.3.0f1").unwrap();
        let old = UnityVersion::parse("2017.2.5f1").unwrap();
        let ancient = UnityVersion::parse("5.6.7f1").unwrap();
        // Unity 2017.3+ always uses UnityCrunch, even for non-ETC crunched formats.
        for format in [
            28, /* DXT1Crunched */
            29, /* DXT5Crunched */
            64, 65,
        ] {
            assert!(is_use_unity_crunch(new, format), "format {format}");
        }
        // Older versions use UnityCrunch only for the ETC crunched formats.
        assert!(is_use_unity_crunch(old, 64));
        assert!(is_use_unity_crunch(old, 65));
        assert!(is_use_unity_crunch(ancient, 64));
        assert!(is_use_unity_crunch(ancient, 65));
        assert!(!is_use_unity_crunch(old, 28));
        assert!(!is_use_unity_crunch(old, 29));
        assert!(!is_use_unity_crunch(ancient, 28));
        // The version gate dominates: on 2017.3+ the answer is true before the format is
        // considered (the caller only invokes the gate for crunched formats).
        assert!(is_use_unity_crunch(new, 10 /* DXT1 */));
        assert!(!is_use_unity_crunch(old, 10));
    }

    #[test]
    fn unpack_normal_reconstructs_blue() {
        // A tangent-space normal of (128, 128) reconstructs blue to 254 under the reference's
        // truncating `Convert<double, byte>`.
        let mut bitmap = Bitmap {
            width: 1,
            height: 1,
            depth: 1,
            is_rgba8: true,
            has_alpha: true,
            pixels: vec![128, 128, 0, 255],
        };
        bitmap.unpack_normal();
        assert_eq!(bitmap.pixels, vec![128, 128, 254, 255]);
    }

    #[test]
    fn unpack_normal_is_ignored_for_non_rgba8_targets() {
        let mut bitmap = Bitmap {
            width: 1,
            height: 1,
            depth: 1,
            is_rgba8: false,
            has_alpha: false,
            pixels: vec![128, 128, 0, 255],
        };
        bitmap.unpack_normal();
        assert_eq!(bitmap.pixels, vec![128, 128, 0, 255]);
    }

    #[test]
    fn graphics_format_dispatch_decodes_rgba() {
        let image = TextureImage {
            texture_format: 0,
            graphics_format: 4,
            width: 1,
            height: 1,
            depth: 1,
            image_size: 4,
            data: &[9, 8, 7, 6],
        };
        let bitmap = convert_to_bitmap(&image).unwrap();
        assert_eq!(bitmap.pixels, vec![9, 8, 7, 6]);
        assert!(bitmap.is_rgba8);
    }

    #[test]
    fn unsupported_format_is_reported() {
        let image = TextureImage {
            texture_format: 200,
            graphics_format: 0,
            width: 1,
            height: 1,
            depth: 1,
            image_size: 4,
            data: &[0; 4],
        };
        assert!(matches!(
            convert_to_bitmap(&image),
            Err(TextureConvertError::UnsupportedFormat {
                texture_format: 200,
                ..
            })
        ));
    }

    #[test]
    fn multi_layer_textures_decode_each_layer() {
        // Two RGBA32 layers of one pixel each.
        let data = [1, 2, 3, 4, 5, 6, 7, 8];
        let image = TextureImage {
            texture_format: 4,
            graphics_format: 0,
            width: 1,
            height: 1,
            depth: 2,
            image_size: 4,
            data: &data,
        };
        let bitmap = convert_to_bitmap(&image).unwrap();
        assert_eq!(bitmap.depth, 2);
        assert_eq!(bitmap.pixels, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
