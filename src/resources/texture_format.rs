//! `TextureFormatExtensions` and the importer texture-settings dual fields.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/TextureFormatExtensions.cs`. The three
//! classifiers answer whether a texture format is block-crunched, DXT-family, or carried as
//! RGBA-family data. `Alpha8` is classified as RGB by the baseline despite being single-channel.
//!
//! Also mirrors the version-union readers and writers of
//! `AssetRipper.SourceGenerated.Extensions/TextureSettingsExtensions.cs`: several settings
//! exist as both an `Int32` and a `UInt32`/`Boolean` spelling, and the getters prefer one
//! spelling while the setters write both. The `ColorSpace`-valued members stay open: the
//! `ColorSpace`, `TextureImporterCompression`, and `FilterMode` member values live in the
//! published `AssetRipper.SourceGenerated` package rather than the frozen baseline, so only
//! the `GetSRGB` fallback reads the color space through a caller-resolved gamma fact.
//!
//! Differential fixture: `texture_format_fixture.txt` (probe `Temp/opencode/texprobe`).

/// `TextureFormatExtensions.IsCrunched`.
pub fn is_crunched(format: i32) -> bool {
    matches!(
        format,
        28 /* DXT1Crunched */ | 29 /* DXT5Crunched */ | 64 /* ETC_RGB4Crunched */ | 65 /* ETC2_RGBA8Crunched */
    )
}

/// `TextureFormatExtensions.IsDxt`.
pub fn is_dxt(format: i32) -> bool {
    matches!(
        format,
        10 /* DXT1 */ | 11 /* DXT3 */ | 12 /* DXT5 */ | 28 /* DXT1Crunched */ | 29 /* DXT5Crunched */
    )
}

/// `TextureFormatExtensions.IsRgb`.
pub fn is_rgb(format: i32) -> bool {
    matches!(
        format,
        1 /* Alpha8 */ | 2 /* ARGB4444 */ | 3 /* RGB24 */ | 4 /* RGBA32 */ | 5 /* ARGB32 */
            | 7 /* RGB565 */ | 9 /* R16 */ | 13 /* RGBA4444 */ | 14 /* BGRA32_14 */ | 15 /* RHalf */
            | 16 /* RGHalf */ | 17 /* RGBAHalf */ | 18 /* RFloat */ | 19 /* RGFloat */ | 20 /* RGBAFloat */
            | 22 /* RGB9e5Float */ | 37 /* BGRA32_37 */ | 62 /* RG16 */ | 63 /* R8 */ | 72 /* RG32 */
            | 73 /* RGB48 */ | 74 /* RGBA64 */ | 75 /* R8_SIGNED */ | 76 /* RG16_SIGNED */
            | 77 /* RGB24_SIGNED */ | 78 /* RGBA32_SIGNED */ | 79 /* R16_SIGNED */ | 80 /* RG32_SIGNED */
            | 81 /* RGB48_SIGNED */ | 82 /* RGBA64_SIGNED */
    )
}

/// `BuildTarget.Xbox360`, the only platform `IsSwapBytes` applies to.
pub const XBOX360_BUILD_TARGET: i32 = 11;

/// `BuildTarget.Switch`, the only platform `GetImageData` unswizzles for. The
/// gate is an exact discriminant match: `Switch2` (48) does not unswizzle.
pub const SWITCH_BUILD_TARGET: i32 = 38;

/// `BuildTarget.Switch2`, pinned only to document the exact-equality gate above.
pub const SWITCH2_BUILD_TARGET: i32 = 48;

/// `Texture2DExtensions.IsSwapBytes`: Xbox360 textures in a 16-bit or DXT layout
/// are stored with swapped byte pairs.
pub fn is_swap_bytes(platform: i32, format: i32) -> bool {
    platform == XBOX360_BUILD_TARGET
        && matches!(
            format,
            2 /* ARGB4444 */ | 7 /* RGB565 */ | 10 /* DXT1 */ | 28 /* DXT1Crunched */
            | 11 /* DXT3 */ | 12 /* DXT5 */ | 29 /* DXT5Crunched */
        )
}

/// Why texture image bytes cannot be byte-swapped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwapBytesError {
    /// An odd byte count has no partner for its final byte. The source loop would
    /// index out of range here; valid swapped layouts always carry even counts.
    OddByteCount { byte_count: usize },
}

impl core::fmt::Display for SwapBytesError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OddByteCount { byte_count } => write!(
                formatter,
                "cannot swap adjacent bytes of {byte_count} bytes: the final byte has no partner"
            ),
        }
    }
}

impl std::error::Error for SwapBytesError {}

/// The `GetImageData` Xbox360 step: swap every adjacent byte pair.
///
/// The source swaps in place over the fetched buffer (inline or streamed); the
/// copy-before-swap stays caller-side. Operates on a copy and returns it.
pub fn swap_adjacent_bytes(data: &[u8]) -> Result<Vec<u8>, SwapBytesError> {
    if !data.len().is_multiple_of(2) {
        return Err(SwapBytesError::OddByteCount {
            byte_count: data.len(),
        });
    }
    let mut swapped = data.to_vec();
    for pair in swapped.chunks_exact_mut(2) {
        pair.swap(0, 1);
    }
    Ok(swapped)
}

/// An importer texture-settings view (`ITextureSettings`).
///
/// Each setting carries the presence and value of its serialized spellings; the color-space
/// field itself stays caller-side and only its resolved gamma fact is kept.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextureSettingsView {
    /// `Has_SRGB_Boolean`.
    pub has_srgb_boolean: bool,
    /// `SRGB_Boolean`.
    pub srgb_boolean: bool,
    /// `Has_SRGB_Int32`.
    pub has_srgb_int32: bool,
    /// `SRGB_Int32`.
    pub srgb_int32: i32,
    /// Whether the stored `ColorSpace` equals `ColorSpace.Gamma`, resolved by the caller.
    pub color_space_is_gamma: bool,
    /// `Has_AnisoLevel_Int32`.
    pub has_aniso_int32: bool,
    /// `AnisoLevel_Int32`.
    pub aniso_int32: i32,
    /// `AnisoLevel_UInt32`.
    pub aniso_uint32: u32,
    /// `Has_CompressionQuality_Int32`.
    pub has_compression_quality_int32: bool,
    /// `CompressionQuality_Int32`.
    pub compression_quality_int32: i32,
    /// `CompressionQuality_UInt32`.
    pub compression_quality_uint32: u32,
    /// `Has_MaxTextureSize_Int32`.
    pub has_max_texture_size_int32: bool,
    /// `MaxTextureSize_Int32`.
    pub max_texture_size_int32: i32,
    /// `MaxTextureSize_UInt32`.
    pub max_texture_size_uint32: u32,
    /// `Has_GenerateMipMaps_Int32`.
    pub has_generate_mip_maps_int32: bool,
    /// `GenerateMipMaps_Int32`.
    pub generate_mip_maps_int32: i32,
    /// `GenerateMipMaps_Boolean`.
    pub generate_mip_maps_boolean: bool,
    /// `Has_Readable_Int32`.
    pub has_readable_int32: bool,
    /// `Readable_Int32`.
    pub readable_int32: i32,
    /// `Readable_Boolean`.
    pub readable_boolean: bool,
    /// `Has_CrunchedCompression_Int32`.
    pub has_crunched_compression_int32: bool,
    /// `CrunchedCompression_Int32`.
    pub crunched_compression_int32: i32,
    /// `CrunchedCompression_Boolean`.
    pub crunched_compression_boolean: bool,
}

impl TextureSettingsView {
    /// `TextureSettingsExtensions.GetSRGB`: the boolean spelling wins, then the integer
    /// spelling, then anything but `Gamma` counts as sRGB.
    pub fn get_srgb(&self) -> bool {
        if self.has_srgb_boolean {
            self.srgb_boolean
        } else if self.has_srgb_int32 {
            self.srgb_int32 != 0
        } else {
            !self.color_space_is_gamma
        }
    }

    /// `TextureSettingsExtensions.GetAnisoLevel`, with the source unchecked cast.
    pub fn get_aniso_level(&self) -> i32 {
        if self.has_aniso_int32 {
            self.aniso_int32
        } else {
            self.aniso_uint32 as i32
        }
    }

    /// `TextureSettingsExtensions.SetAnisoLevel`: writes both spellings unconditionally.
    pub fn set_aniso_level(&mut self, value: i32) {
        self.aniso_int32 = value;
        self.aniso_uint32 = value as u32;
    }

    /// `TextureSettingsExtensions.GetCompressionQuality`, with the source unchecked cast.
    pub fn get_compression_quality(&self) -> i32 {
        if self.has_compression_quality_int32 {
            self.compression_quality_int32
        } else {
            self.compression_quality_uint32 as i32
        }
    }

    /// `TextureSettingsExtensions.SetCompressionQuality`: writes both spellings.
    pub fn set_compression_quality(&mut self, value: i32) {
        self.compression_quality_int32 = value;
        self.compression_quality_uint32 = value as u32;
    }

    /// `TextureSettingsExtensions.GetMaxTextureSize`, with the source unchecked cast.
    pub fn get_max_texture_size(&self) -> i32 {
        if self.has_max_texture_size_int32 {
            self.max_texture_size_int32
        } else {
            self.max_texture_size_uint32 as i32
        }
    }

    /// `TextureSettingsExtensions.SetMaxTextureSize`: writes both spellings.
    pub fn set_max_texture_size(&mut self, value: i32) {
        self.max_texture_size_int32 = value;
        self.max_texture_size_uint32 = value as u32;
    }

    /// `TextureSettingsExtensions.GetGenerateMipMaps`.
    pub fn get_generate_mip_maps(&self) -> bool {
        if self.has_generate_mip_maps_int32 {
            self.generate_mip_maps_int32 != 0
        } else {
            self.generate_mip_maps_boolean
        }
    }

    /// `TextureSettingsExtensions.SetGenerateMipMaps`: writes both spellings.
    pub fn set_generate_mip_maps(&mut self, value: bool) {
        self.generate_mip_maps_int32 = i32::from(value);
        self.generate_mip_maps_boolean = value;
    }

    /// `TextureSettingsExtensions.GetReadable`.
    pub fn get_readable(&self) -> bool {
        if self.has_readable_int32 {
            self.readable_int32 != 0
        } else {
            self.readable_boolean
        }
    }

    /// `TextureSettingsExtensions.SetReadable`: writes both spellings.
    pub fn set_readable(&mut self, value: bool) {
        self.readable_int32 = i32::from(value);
        self.readable_boolean = value;
    }

    /// `TextureSettingsExtensions.GetCrunchedCompression`.
    pub fn get_crunched_compression(&self) -> bool {
        if self.has_crunched_compression_int32 {
            self.crunched_compression_int32 != 0
        } else {
            self.crunched_compression_boolean
        }
    }

    /// `TextureSettingsExtensions.SetCrunchedCompression`: writes both spellings.
    pub fn set_crunched_compression(&mut self, value: bool) {
        self.crunched_compression_int32 = i32::from(value);
        self.crunched_compression_boolean = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `TextureFormatExtensions` (probe
    /// `Temp/opencode/texprobe`).
    #[test]
    fn texture_format_classification_matches_the_reference_runtime() {
        let fixture = include_str!("texture_format_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "TF", "{line}");
            let format: i32 = parts[2].parse().unwrap();
            assert_eq!(is_crunched(format), parts[3] == "1", "{line}");
            assert_eq!(is_dxt(format), parts[4] == "1", "{line}");
            assert_eq!(is_rgb(format), parts[5] == "1", "{line}");
            rows += 1;
        }
        assert_eq!(rows, 91);
    }

    #[test]
    fn swap_rule_matches_the_xbox360_layouts() {
        // All seven swapped layouts on Xbox360.
        for format in [2, 7, 10, 11, 12, 28, 29] {
            assert!(is_swap_bytes(XBOX360_BUILD_TARGET, format), "{format}");
        }
        // Other formats and other platforms never swap.
        assert!(!is_swap_bytes(XBOX360_BUILD_TARGET, 4));
        assert!(!is_swap_bytes(XBOX360_BUILD_TARGET, 64));
        assert!(!is_swap_bytes(0, 10));
        assert!(!is_swap_bytes(13, 12));
    }

    #[test]
    fn adjacent_swap_exchanges_every_pair() {
        assert_eq!(swap_adjacent_bytes(&[]), Ok(Vec::new()));
        assert_eq!(swap_adjacent_bytes(&[0x12, 0x34]), Ok(vec![0x34, 0x12]));
        assert_eq!(swap_adjacent_bytes(&[1, 2, 3, 4]), Ok(vec![2, 1, 4, 3]));
        assert_eq!(
            swap_adjacent_bytes(&[1, 2, 3]),
            Err(SwapBytesError::OddByteCount { byte_count: 3 })
        );
    }

    /// The dual-spelling readers prefer their first spelling and the writers fill both.
    #[test]
    fn texture_settings_union_fields_match_the_source_branches() {
        // sRGB: boolean, then integer, then the gamma fallback.
        let boolean = TextureSettingsView {
            has_srgb_boolean: true,
            srgb_boolean: true,
            has_srgb_int32: true,
            color_space_is_gamma: true,
            ..TextureSettingsView::default()
        };
        assert!(boolean.get_srgb());
        let integer = TextureSettingsView {
            has_srgb_int32: true,
            srgb_int32: 2,
            color_space_is_gamma: true,
            ..TextureSettingsView::default()
        };
        assert!(integer.get_srgb());
        let off = TextureSettingsView {
            has_srgb_int32: true,
            color_space_is_gamma: false,
            ..TextureSettingsView::default()
        };
        assert!(!off.get_srgb());
        assert!(!TextureSettingsView {
            color_space_is_gamma: true,
            ..TextureSettingsView::default()
        }
        .get_srgb());
        assert!(TextureSettingsView::default().get_srgb());

        // Integer unions: present spelling wins, otherwise the other wraps like the
        // source unchecked cast.
        let present = TextureSettingsView {
            has_aniso_int32: true,
            aniso_int32: -3,
            aniso_uint32: 9,
            has_compression_quality_int32: true,
            compression_quality_int32: 50,
            compression_quality_uint32: 60,
            has_max_texture_size_int32: true,
            max_texture_size_int32: 1024,
            max_texture_size_uint32: 2048,
            ..TextureSettingsView::default()
        };
        assert_eq!(present.get_aniso_level(), -3);
        assert_eq!(present.get_compression_quality(), 50);
        assert_eq!(present.get_max_texture_size(), 1024);
        let wrapped = TextureSettingsView {
            aniso_uint32: u32::MAX,
            compression_quality_uint32: u32::MAX,
            max_texture_size_uint32: u32::MAX,
            ..TextureSettingsView::default()
        };
        assert_eq!(wrapped.get_aniso_level(), -1);
        assert_eq!(wrapped.get_compression_quality(), -1);
        assert_eq!(wrapped.get_max_texture_size(), -1);

        // Boolean unions: nonzero integers count as set.
        let flags = TextureSettingsView {
            has_generate_mip_maps_int32: true,
            generate_mip_maps_int32: 7,
            has_readable_int32: true,
            has_crunched_compression_int32: true,
            crunched_compression_int32: -1,
            ..TextureSettingsView::default()
        };
        assert!(flags.get_generate_mip_maps());
        assert!(!flags.get_readable());
        assert!(flags.get_crunched_compression());

        // Writers fill both spellings, including the wrapping and 0/1 encodings.
        let mut written = TextureSettingsView::default();
        written.set_aniso_level(-1);
        written.set_compression_quality(50);
        written.set_max_texture_size(2048);
        written.set_generate_mip_maps(true);
        written.set_readable(false);
        written.set_crunched_compression(true);
        assert_eq!(written.aniso_int32, -1);
        assert_eq!(written.aniso_uint32, u32::MAX);
        assert_eq!(written.compression_quality_int32, 50);
        assert_eq!(written.compression_quality_uint32, 50);
        assert_eq!(written.max_texture_size_int32, 2048);
        assert_eq!(written.max_texture_size_uint32, 2048);
        assert_eq!(written.generate_mip_maps_int32, 1);
        assert!(written.generate_mip_maps_boolean);
        assert_eq!(written.readable_int32, 0);
        assert!(!written.readable_boolean);
        assert_eq!(written.crunched_compression_int32, 1);
        assert!(written.crunched_compression_boolean);
    }
}
