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
//! Crunch payloads use the pure-Rust `texture2ddecoder` implementation. The serialized Unity
//! version selects classic Crunch or Unity Crunch exactly as the frozen `CrunchHandler` does.

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
use texture2ddecoder::CrnTextureInfo;

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
    /// The format is block-crunched, but no source Unity version was available to select its decoder.
    CrunchUnsupported {
        /// The crunched `TextureFormat` discriminant.
        texture_format: i32,
    },
    /// The CRN header or its codec does not agree with the serialized Texture2D.
    CrunchDecodeFailed {
        /// Decoder reason or failed structural invariant.
        reason: &'static str,
    },
    /// A CRN payload is too large for the decoder's 32-bit length parameter.
    CrunchInputTooLarge {
        /// Payload byte count.
        byte_count: usize,
    },
    /// A bounded output allocation could not be reserved.
    AllocationFailed {
        /// Requested byte count.
        requested_bytes: usize,
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
                "crunched texture format {texture_format} requires the source Unity version"
            ),
            Self::CrunchDecodeFailed { reason } => {
                write!(formatter, "Crunch texture could not be decoded: {reason}")
            }
            Self::CrunchInputTooLarge { byte_count } => write!(
                formatter,
                "Crunch payload has {byte_count} bytes; the decoder accepts at most {}",
                u32::MAX
            ),
            Self::AllocationFailed { requested_bytes } => write!(
                formatter,
                "could not reserve {requested_bytes} bytes for decoded texture pixels"
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
/// only for the ETC crunched formats and fall back to `Crunch` otherwise. The same gate selects
/// the matching pure-Rust decoder path below.
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
    /// The reference calls `GetChannels(out a, out g, out _, out r)` whose out
    /// parameters are ordered `(r, g, b, a)`, so the locals receive the input
    /// red channel in `a` and the input alpha channel in `r`; it then writes
    /// `SetChannels(r, g, b, a)`, swapping alpha and red. Blue is recomputed
    /// from the swapped red/green tangent-space components. It only acts when
    /// the bitmap is RGBA8. Pinned by the live-cubemap differential
    /// (`cubemap_convert_fixture.txt`, probe `Temp/opencode/cubeconvprobe`).
    pub fn unpack_normal(&mut self) {
        if !self.is_rgba8 {
            return;
        }
        const MAGNITUDE_SQR: f64 = 255.0 * 255.0;
        for pixel in self.pixels.chunks_exact_mut(4) {
            let swapped_r = pixel[3];
            let g = pixel[1];
            let swapped_a = pixel[0];
            let vr = f64::from(swapped_r) * 2.0 - 255.0;
            let vg = f64::from(g) * 2.0 - 255.0;
            let hypotenuse_sqr = (vr * vr + vg * vg).min(MAGNITUDE_SQR);
            let vb = (MAGNITUDE_SQR - hypotenuse_sqr).sqrt();
            let b_exact = (vb + 255.0) / 2.0;
            let b = double_to_byte(b_exact / 255.0);
            pixel[0] = swapped_r;
            pixel[1] = g;
            pixel[2] = b;
            pixel[3] = swapped_a;
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
    try_convert_to_bitmap_with_unity_version(image, flip_y, unpack_normal, None)
}

/// `TextureConverter.TryConvertToBitmap` with the SerializedFile's Unity version for Crunch.
pub fn try_convert_to_bitmap_with_unity_version(
    image: &TextureImage<'_>,
    flip_y: bool,
    unpack_normal: bool,
    unity_version: Option<UnityVersion>,
) -> Result<Bitmap, TextureConvertError> {
    let mut bitmap = convert_to_bitmap_with_unity_version(image, unity_version)?;
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
    convert_to_bitmap_with_unity_version(image, None)
}

/// Pixel conversion with the source Unity version used to select the Crunch container variant.
pub fn convert_to_bitmap_with_unity_version(
    image: &TextureImage<'_>,
    unity_version: Option<UnityVersion>,
) -> Result<Bitmap, TextureConvertError> {
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
        let unity_version = unity_version.ok_or(TextureConvertError::CrunchUnsupported {
            texture_format: image.texture_format,
        })?;
        return convert_crunched_texture(image, unity_version, pixel_bytes);
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

fn convert_crunched_texture(
    image: &TextureImage<'_>,
    unity_version: UnityVersion,
    pixel_bytes: usize,
) -> Result<Bitmap, TextureConvertError> {
    let data_size =
        u32::try_from(image.data.len()).map_err(|_| TextureConvertError::CrunchInputTooLarge {
            byte_count: image.data.len(),
        })?;
    let mut info = CrnTextureInfo::default();
    // The vendored decoder indexes attacker-controlled header offsets without
    // bounds checks (header level loop, table and palette section slices), so
    // malformed input can panic inside header parsing. The decoder keeps no
    // global state (verified: no statics in the vendored crate), so catching
    // the panic here converts a crash into the same loud invalid-header error
    // a clean rejection produces.
    let header_ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        info.crnd_get_texture_info(image.data, data_size)
    }))
    .unwrap_or(false);
    if !header_ok {
        return Err(TextureConvertError::CrunchDecodeFailed {
            reason: "invalid CRN header",
        });
    }
    if info.width == 0
        || info.height == 0
        || info.levels == 0
        || usize::try_from(info.faces).ok() != Some(image.depth)
    {
        return Err(TextureConvertError::CrunchDecodeFailed {
            reason: "CRN header has invalid dimensions, mip count, or face count",
        });
    }
    if usize::try_from(info.width).ok() != Some(image.width)
        || usize::try_from(info.height).ok() != Some(image.height)
    {
        return Err(TextureConvertError::CrunchDecodeFailed {
            reason: "CRN dimensions do not match the serialized Texture2D",
        });
    }
    let mut max_mip_count = 1_u32;
    let mut largest_dimension = info.width.max(info.height);
    while largest_dimension > 1 {
        largest_dimension >>= 1;
        max_mip_count += 1;
    }
    if info.levels > max_mip_count {
        return Err(TextureConvertError::CrunchDecodeFailed {
            reason: "CRN mip count exceeds the dimensions' full mip chain",
        });
    }

    // `CrnFormat` is intentionally not re-exported by texture2ddecoder 0.1.2. These pinned
    // ordinals mirror its public enum declaration and reject a CRN codec that disagrees with
    // Unity's serialized TextureFormat before either decoder is called.
    const CRN_DXT1: u32 = 0;
    const CRN_DXT5_FIRST: u32 = 2;
    const CRN_DXT5_LAST: u32 = 6;
    const CRN_ETC1: u32 = 10;
    const CRN_ETC1S: u32 = 13;
    const CRN_ETC2A: u32 = 12;
    const CRN_ETC2AS: u32 = 14;
    let crn_format = info.format as u32;
    let unity_crunch = is_use_unity_crunch(unity_version, image.texture_format);
    let matches_texture_format = match image.texture_format {
        28 => crn_format == CRN_DXT1,
        29 => (CRN_DXT5_FIRST..=CRN_DXT5_LAST).contains(&crn_format),
        64 => unity_crunch && matches!(crn_format, CRN_ETC1 | CRN_ETC1S),
        65 => unity_crunch && matches!(crn_format, CRN_ETC2A | CRN_ETC2AS),
        _ => false,
    };
    if !matches_texture_format {
        return Err(TextureConvertError::CrunchDecodeFailed {
            reason: "CRN codec does not match the serialized Unity TextureFormat",
        });
    }

    let pixel_count = image
        .width
        .checked_mul(image.height)
        .and_then(|count| count.checked_mul(image.depth))
        .ok_or(TextureConvertError::TextureTooLarge {
            width: image.width,
            height: image.height,
            depth: image.depth,
        })?;
    let decoded_pixel_bytes = pixel_count.checked_mul(core::mem::size_of::<u32>()).ok_or(
        TextureConvertError::TextureTooLarge {
            width: image.width,
            height: image.height,
            depth: image.depth,
        },
    )?;
    let mut decoded_pixels = Vec::new();
    decoded_pixels.try_reserve_exact(pixel_count).map_err(|_| {
        TextureConvertError::AllocationFailed {
            requested_bytes: decoded_pixel_bytes,
        }
    })?;
    decoded_pixels.resize(pixel_count, 0_u32);
    // Same containment as header parsing: section and level unpacking slice
    // attacker-controlled offsets, so a crafted header that survives the
    // checks above can still panic inside the decoder. A caught panic maps to
    // the same loud decode failure an error return produces.
    let decode_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if unity_crunch {
            texture2ddecoder::decode_unity_crunch_faces(
                image.data,
                image.width,
                image.height,
                image.depth,
                &mut decoded_pixels,
            )
        } else {
            texture2ddecoder::decode_crunch_faces(
                image.data,
                image.width,
                image.height,
                image.depth,
                &mut decoded_pixels,
            )
        }
    }));
    let decode_result = match decode_result {
        Ok(inner) => inner,
        Err(_) => Err("CRN decoder panicked on malformed input"),
    };
    decode_result.map_err(|reason| TextureConvertError::CrunchDecodeFailed { reason })?;

    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_bytes)
        .map_err(|_| TextureConvertError::AllocationFailed {
            requested_bytes: pixel_bytes,
        })?;
    for packed in decoded_pixels {
        // texture2ddecoder stores each packed color as little-endian BGRA bytes.
        let [blue, green, red, alpha] = packed.to_le_bytes();
        pixels.extend_from_slice(&[red, green, blue, alpha]);
    }

    Ok(Bitmap {
        width: image.width,
        height: image.height,
        depth: image.depth,
        is_rgba8: is_rgba8_target(image.texture_format),
        has_alpha: has_alpha(image.texture_format),
        pixels,
    })
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
    fn versioned_crunched_texture_reaches_the_crn_decoder() {
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
            convert_to_bitmap_with_unity_version(
                &image,
                Some(UnityVersion::parse("2022.3.22f1").unwrap()),
            ),
            Err(TextureConvertError::CrunchDecodeFailed {
                reason: "invalid CRN header"
            })
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
    fn valid_crunch_payloads_match_independent_rgba_oracle_hashes() {
        let cases: [(&str, i32, &str, &[u8], u64); 6] = [
            (
                "classic_dxt1.crn",
                28,
                "2017.2.0f1",
                &include_bytes!("../../tests/fixtures/crunch/classic_dxt1.crn")[..],
                0x141e_cbae_e8fd_eb6b,
            ),
            (
                "classic_dxt5.crn",
                29,
                "2017.2.0f1",
                &include_bytes!("../../tests/fixtures/crunch/classic_dxt5.crn")[..],
                0xf6bb_d719_6ee7_b293,
            ),
            (
                "unity_dxt1.crn",
                28,
                "2017.3.0f1",
                &include_bytes!("../../tests/fixtures/crunch/unity_dxt1.crn")[..],
                0x2325_b896_a579_1851,
            ),
            (
                "unity_dxt5.crn",
                29,
                "2017.3.0f1",
                &include_bytes!("../../tests/fixtures/crunch/unity_dxt5.crn")[..],
                0xb69e_1abe_cd80_0da1,
            ),
            (
                "unity_etc1.crn",
                64,
                "2017.2.0f1",
                &include_bytes!("../../tests/fixtures/crunch/unity_etc1.crn")[..],
                0x794f_5132_6238_5ed4,
            ),
            (
                "unity_etc2a.crn",
                65,
                "2017.2.0f1",
                &include_bytes!("../../tests/fixtures/crunch/unity_etc2a.crn")[..],
                0xb00c_f611_3233_e6db,
            ),
        ];

        for (name, texture_format, unity_version, data, expected_hash) in cases {
            let image = TextureImage {
                texture_format,
                graphics_format: 0,
                width: 512,
                height: 512,
                depth: 1,
                image_size: data.len(),
                data,
            };
            let version = UnityVersion::parse(unity_version).unwrap();
            let bitmap = convert_to_bitmap_with_unity_version(&image, Some(version))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(
                (bitmap.width, bitmap.height, bitmap.depth),
                (512, 512, 1),
                "{name}"
            );
            assert_eq!(bitmap.pixels.len(), 512 * 512 * 4, "{name}");
            let actual_hash = bitmap
                .pixels
                .iter()
                .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                    (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
                });
            assert_eq!(actual_hash, expected_hash, "{name} full RGBA oracle hash");
        }
    }

    #[test]
    fn crunched_header_and_routing_mismatches_fail_loudly() {
        // Caller-arg mismatches against a valid CRN fixture exercise the
        // production guard arms without new fixtures: face count, serialized
        // dimensions, codec agreement, and header validity each fail with
        // their own reason instead of mis-decoding.
        let data = &include_bytes!("../../tests/fixtures/crunch/classic_dxt1.crn")[..];
        let version = UnityVersion::parse("2017.2.0f1").unwrap();
        fn image(
            texture_format: i32,
            width: usize,
            height: usize,
            depth: usize,
            data: &[u8],
        ) -> TextureImage<'_> {
            TextureImage {
                texture_format,
                graphics_format: 0,
                width,
                height,
                depth,
                image_size: data.len(),
                data,
            }
        }
        for (name, texture_format, width, height, depth, bytes, reason) in [
            (
                "faces",
                28,
                512,
                512,
                6,
                data,
                "CRN header has invalid dimensions, mip count, or face count",
            ),
            (
                "dimensions",
                28,
                4,
                4,
                1,
                data,
                "CRN dimensions do not match the serialized Texture2D",
            ),
            (
                "codec",
                29,
                512,
                512,
                1,
                data,
                "CRN codec does not match the serialized Unity TextureFormat",
            ),
            (
                "header",
                28,
                512,
                512,
                1,
                &[0; 64][..],
                "invalid CRN header",
            ),
        ] {
            assert_eq!(
                convert_to_bitmap_with_unity_version(
                    &image(texture_format, width, height, depth, bytes),
                    Some(version)
                ),
                Err(TextureConvertError::CrunchDecodeFailed { reason }),
                "{name}"
            );
        }
    }

    #[test]
    fn truncated_crn_headers_and_payloads_fail_cleanly() {
        // Truncated input must fail loudly at every cut point: neither a
        // panic nor a short-but-Ok bitmap may escape the production wrapper.
        let data = &include_bytes!("../../tests/fixtures/crunch/classic_dxt1.crn")[..];
        let version = UnityVersion::parse("2017.2.0f1").unwrap();
        for cut in [0usize, 1, 16, 64, 128, 512, 1024, 4096] {
            let end = cut.min(data.len());
            let image = TextureImage {
                texture_format: 28,
                graphics_format: 0,
                width: 512,
                height: 512,
                depth: 1,
                image_size: end,
                data: &data[..end],
            };
            assert!(
                convert_to_bitmap_with_unity_version(&image, Some(version)).is_err(),
                "cut {cut} must fail loudly"
            );
        }
    }

    #[test]
    fn malicious_crn_headers_fail_loudly_instead_of_panicking() {
        // The vendored decoder slices attacker-controlled header offsets
        // without bounds checks. Both bombs below panicked before the
        // boundary containment landed; each must now fail with a loud error.
        let data = &include_bytes!("../../tests/fixtures/crunch/classic_dxt1.crn")[..];
        let version = UnityVersion::parse("2017.2.0f1").unwrap();
        // A short buffer with a large level count drives the header level
        // loop past the buffer end.
        let mut level_bomb = data[..96].to_vec();
        level_bomb[16] = 8;
        let image = TextureImage {
            texture_format: 28,
            graphics_format: 0,
            width: 512,
            height: 512,
            depth: 1,
            image_size: level_bomb.len(),
            data: &level_bomb,
        };
        assert_eq!(
            convert_to_bitmap_with_unity_version(&image, Some(version)),
            Err(TextureConvertError::CrunchDecodeFailed {
                reason: "invalid CRN header"
            }),
            "level bomb"
        );
        // A full-length buffer with a huge table offset survives header
        // parsing and reaches section unpacking.
        let mut tables_bomb = data.to_vec();
        tables_bomb[67..70].copy_from_slice(&[0xFF; 3]);
        let image = TextureImage {
            texture_format: 28,
            graphics_format: 0,
            width: 512,
            height: 512,
            depth: 1,
            image_size: tables_bomb.len(),
            data: &tables_bomb,
        };
        assert_eq!(
            convert_to_bitmap_with_unity_version(&image, Some(version)),
            Err(TextureConvertError::CrunchDecodeFailed {
                reason: "CRN decoder panicked on malformed input"
            }),
            "tables bomb"
        );
    }

    #[test]
    fn short_inputs_fail_loudly_across_all_texture_decoders() {
        // Every decoder must reject truncated input with an error, never a
        // panic and never a silent short decode. Dimensions stay valid so
        // only the input length varies; seven bytes starve even the smallest
        // 4x4 block format.
        let texture_formats = [
            48, 35, 26, 24, 25, 10, 11, 12, 34, 41, 42, 43, 44, 45, 46, 47, 30, 32, 21, 1, 2, 13,
            7, 63, 62, 3, 4, 5, 6, 8, 14, 9, 72, 73, 74, 15, 16, 17, 18, 19, 20, 22, 75, 76, 77,
            78, 79, 80, 81, 82,
        ];
        let graphics_formats = [
            1, 3, 4, 9, 21, 37, 45, 49, 54, 55, 56, 57, 60, 66, 68, 87, 89, 90, 93, 95, 96, 98,
            100, 102, 104, 106, 107, 108, 110, 112, 118, 119, 121, 123, 125, 126, 127, 128, 129,
            131, 133, 135, 137, 139,
        ];
        let mut output = [0u8; 64];
        for format in texture_formats {
            for len in [0usize, 1, 7] {
                let input = vec![0xABu8; len];
                assert!(
                    decode_texture(format, 0, &input, 4, 4, &mut output).is_err(),
                    "texture format {format} accepted {len} input bytes"
                );
            }
        }
        for format in graphics_formats {
            for len in [0usize, 1, 7] {
                let input = vec![0xABu8; len];
                assert!(
                    decode_texture(0, format, &input, 4, 4, &mut output).is_err(),
                    "graphics format {format} accepted {len} input bytes"
                );
            }
        }
    }

    #[test]
    fn unpack_normal_reconstructs_blue() {
        // The reference swaps alpha and red: the output red is the input
        // alpha and the output alpha is the input red; blue is recomputed
        // from the swapped pair. A tangent-space normal of (128, 128) with
        // alpha 255 reconstructs blue to 127 under the truncating conversion.
        let mut bitmap = Bitmap {
            width: 1,
            height: 1,
            depth: 1,
            is_rgba8: true,
            has_alpha: true,
            pixels: vec![128, 128, 0, 255],
        };
        bitmap.unpack_normal();
        assert_eq!(bitmap.pixels, vec![255, 128, 127, 128]);
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

    /// Byte-for-byte differential against the shipped `TextureConverter` over
    /// live generated objects (probe `Temp/opencode/cubeconvprobe`): a real
    /// class-89 `Cubemap` and a class-28 `Texture2D` built through the
    /// generated `Create` API with hand-crafted DXT5 blocks. The replay drives
    /// `try_convert_to_bitmap` with the source's own gates (no `FlipY` for the
    /// cubemap, flip for the plain texture, `UnpackNormal` only for the
    /// `NormalmapDXT5nm` lightmap) and must reproduce every output pixel.
    /// This differential caught and fixed the swapped red/alpha channels in
    /// `unpack_normal`.
    #[test]
    fn live_cubemap_conversion_matches_the_reference_runtime() {
        fn decode_hex(text: &str) -> Vec<u8> {
            let bytes = text.as_bytes();
            assert!(bytes.len().is_multiple_of(2), "odd hex input");
            bytes
                .chunks_exact(2)
                .map(|pair| {
                    let digit = |c: u8| (c as char).to_digit(16).expect("hex digit") as u8;
                    digit(pair[0]) << 4 | digit(pair[1])
                })
                .collect()
        }

        let fixture = include_str!("cubemap_convert_fixture.txt");
        struct Case {
            tag: String,
            lightmap: i32,
            input: Vec<u8>,
            width: usize,
            height: usize,
            depth: usize,
            expected: Vec<u8>,
        }
        let mut cases: Vec<Case> = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(4, '|');
            assert_eq!(parts.next(), Some("CCASE"), "{line}");
            let tag = parts.next().expect("a probe tag");
            if cases.is_empty() || cases.last().expect("a case").tag != tag {
                cases.push(Case {
                    tag: tag.to_owned(),
                    lightmap: -1,
                    input: Vec::new(),
                    width: 0,
                    height: 0,
                    depth: 0,
                    expected: Vec::new(),
                });
            }
            let case = cases.last_mut().expect("a case");
            match parts.next().expect("a row kind") {
                "lightmap" => {
                    let mut fields = parts.next().expect("lightmap fields").split('|');
                    fields.next().expect("a lightmap name");
                    case.lightmap = fields.next().expect("a lightmap value").parse().unwrap();
                }
                "input" => {
                    let mut fields = parts.next().expect("input fields").split('|');
                    let length: usize = fields.next().expect("an input length").parse().unwrap();
                    case.input = decode_hex(fields.next().expect("input hex"));
                    assert_eq!(case.input.len(), length, "{line}");
                }
                "ok" => assert_eq!(parts.next(), Some("1"), "{line}"),
                "size" => {
                    let mut fields = parts.next().expect("size fields").split('|');
                    case.width = fields.next().expect("a width").parse().unwrap();
                    case.height = fields.next().expect("a height").parse().unwrap();
                    case.depth = fields.next().expect("a depth").parse().unwrap();
                    assert_eq!(fields.next(), Some("4"), "{line}");
                }
                "pixels" => {
                    case.expected = decode_hex(parts.next().expect("pixel hex"));
                }
                other => panic!("unexpected row kind {other}"),
            }
        }
        assert_eq!(cases.len(), 3, "all probed conversions are asserted");
        for case in &cases {
            let cubemap = !case.tag.starts_with("tex2d");
            let image = TextureImage {
                texture_format: 12, // DXT5, like the probed objects.
                graphics_format: 0,
                width: case.width,
                height: case.height,
                depth: case.depth,
                image_size: case.input.len(),
                data: &case.input,
            };
            let bitmap = try_convert_to_bitmap(&image, !cubemap, case.lightmap == 3)
                .expect("probed conversion succeeds");
            assert_eq!(bitmap.width, case.width, "{tag}", tag = case.tag);
            assert_eq!(bitmap.height, case.height, "{tag}", tag = case.tag);
            assert_eq!(bitmap.depth, case.depth, "{tag}", tag = case.tag);
            assert_eq!(bitmap.pixels, case.expected, "{tag}", tag = case.tag);
        }
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

    #[test]
    fn conversion_guards_and_texture3d_stride_match_the_source() {
        // Zero dimensions fail before any decoding.
        let empty = TextureImage {
            texture_format: 4,
            graphics_format: 0,
            width: 0,
            height: 4,
            depth: 1,
            image_size: 0,
            data: &[],
        };
        assert!(matches!(
            convert_to_bitmap(&empty),
            Err(TextureConvertError::InvalidDimensions { .. })
        ));
        // `width * height * depth * 4` past `i32::MAX` fails.
        let huge = TextureImage {
            texture_format: 4,
            graphics_format: 0,
            width: 1 << 16,
            height: 1 << 16,
            depth: 1 << 8,
            image_size: 0,
            data: &[],
        };
        assert!(matches!(
            convert_to_bitmap(&huge),
            Err(TextureConvertError::TextureTooLarge { .. })
        ));
        // A Texture3D whose data is exactly one image takes the per-layer
        // stride from the decoder's consumed byte count.
        let data = [1, 2, 3, 4, 5, 6, 7, 8];
        let image = TextureImage {
            texture_format: 4,
            graphics_format: 0,
            width: 1,
            height: 1,
            depth: 2,
            image_size: 8,
            data: &data,
        };
        let bitmap = convert_to_bitmap(&image).unwrap();
        assert_eq!(bitmap.depth, 2);
        assert_eq!(bitmap.pixels, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        // The wrapper applies the caller flip and normal-map flags.
        let tall = TextureImage {
            texture_format: 4,
            graphics_format: 0,
            width: 1,
            height: 2,
            depth: 1,
            image_size: 8,
            data: &data,
        };
        let bitmap = try_convert_to_bitmap(&tall, true, false).unwrap();
        assert_eq!(bitmap.pixels, vec![5, 6, 7, 8, 1, 2, 3, 4]);
        let normal = TextureImage {
            texture_format: 4,
            graphics_format: 0,
            width: 1,
            height: 1,
            depth: 1,
            image_size: 4,
            data: &[128, 128, 0, 255],
        };
        let bitmap = try_convert_to_bitmap(&normal, false, true).unwrap();
        assert_eq!(bitmap.pixels, vec![255, 128, 127, 128]);
    }
}
