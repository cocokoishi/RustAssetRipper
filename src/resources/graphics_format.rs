//! `GraphicsFormatExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/GraphicsFormatExtensions.cs`
//! `ToTextureFormat`: a large GraphicsFormat-to-TextureFormat table. Entries the baseline cannot
//! convert yield `NotSupported`/`NotImplemented`; an unlisted value yields `ArgumentOutOfRange`.
//! The mapping is not always exact (for example SRGB and UNorm are treated as the same).
//!
//! Differential fixture: `graphics_format_fixture.txt` (probe `Temp/opencode/texprobe`).

/// A GraphicsFormat the baseline cannot convert.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphicsFormatError {
    /// `NotSupportedException`.
    NotSupported,
    /// `NotImplementedException`.
    NotImplemented,
    /// `ArgumentOutOfRangeException` for a value outside the GraphicsFormat enum.
    ArgumentOutOfRange {
        /// The unrecognized GraphicsFormat value.
        format: i32,
    },
}

impl core::fmt::Display for GraphicsFormatError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotSupported => {
                formatter.write_str("GraphicsFormat has no TextureFormat conversion")
            }
            Self::NotImplemented => {
                formatter.write_str("GraphicsFormat conversion is not implemented")
            }
            Self::ArgumentOutOfRange { format } => {
                write!(formatter, "GraphicsFormat value {format} is out of range")
            }
        }
    }
}

impl std::error::Error for GraphicsFormatError {}

/// `GraphicsFormatExtensions.ToTextureFormat`.
pub fn to_texture_format(format: i32) -> Result<i32, GraphicsFormatError> {
    match format {
        0 => Err(GraphicsFormatError::NotSupported),    // None
        1 => Ok(63),                                    // R8_SRGB
        2 => Ok(62),                                    // R8G8_SRGB
        3 => Ok(3),                                     // R8G8B8_SRGB
        4 => Ok(4),                                     // R8G8B8A8_SRGB
        5 => Ok(63),                                    // R8_UNorm
        6 => Ok(62),                                    // R8G8_UNorm
        7 => Ok(3),                                     // R8G8B8_UNorm
        8 => Ok(4),                                     // R8G8B8A8_UNorm
        9 => Ok(75),                                    // R8_SNorm
        10 => Ok(76),                                   // R8G8_SNorm
        11 => Ok(77),                                   // R8G8B8_SNorm
        12 => Ok(78),                                   // R8G8B8A8_SNorm
        13 => Ok(63),                                   // R8_UInt
        14 => Ok(62),                                   // R8G8_UInt
        15 => Ok(3),                                    // R8G8B8_UInt
        16 => Ok(4),                                    // R8G8B8A8_UInt
        17 => Ok(75),                                   // R8_SInt
        18 => Ok(76),                                   // R8G8_SInt
        19 => Ok(77),                                   // R8G8B8_SInt
        20 => Ok(78),                                   // R8G8B8A8_SInt
        21 => Ok(9),                                    // R16_UNorm
        22 => Ok(72),                                   // R16G16_UNorm
        23 => Ok(73),                                   // R16G16B16_UNorm
        24 => Ok(74),                                   // R16G16B16A16_UNorm
        25 => Ok(79),                                   // R16_SNorm
        26 => Ok(80),                                   // R16G16_SNorm
        27 => Ok(81),                                   // R16G16B16_SNorm
        28 => Ok(82),                                   // R16G16B16A16_SNorm
        29 => Ok(9),                                    // R16_UInt
        30 => Ok(72),                                   // R16G16_UInt
        31 => Ok(73),                                   // R16G16B16_UInt
        32 => Ok(74),                                   // R16G16B16A16_UInt
        33 => Ok(79),                                   // R16_SInt
        34 => Ok(80),                                   // R16G16_SInt
        35 => Ok(81),                                   // R16G16B16_SInt
        36 => Ok(82),                                   // R16G16B16A16_SInt
        37 => Err(GraphicsFormatError::NotSupported),   // R32_UInt
        38 => Err(GraphicsFormatError::NotSupported),   // R32G32_UInt
        39 => Err(GraphicsFormatError::NotSupported),   // R32G32B32_UInt
        40 => Err(GraphicsFormatError::NotSupported),   // R32G32B32A32_UInt
        41 => Err(GraphicsFormatError::NotSupported),   // R32_SInt
        42 => Err(GraphicsFormatError::NotSupported),   // R32G32_SInt
        43 => Err(GraphicsFormatError::NotSupported),   // R32G32B32_SInt
        44 => Err(GraphicsFormatError::NotSupported),   // R32G32B32A32_SInt
        45 => Err(GraphicsFormatError::NotSupported),   // R16_SFloat
        46 => Err(GraphicsFormatError::NotSupported),   // R16G16_SFloat
        47 => Err(GraphicsFormatError::NotSupported),   // R16G16B16_SFloat
        48 => Err(GraphicsFormatError::NotSupported),   // R16G16B16A16_SFloat
        49 => Err(GraphicsFormatError::NotSupported),   // R32_SFloat
        50 => Err(GraphicsFormatError::NotSupported),   // R32G32_SFloat
        51 => Err(GraphicsFormatError::NotSupported),   // R32G32B32_SFloat
        52 => Err(GraphicsFormatError::NotSupported),   // R32G32B32A32_SFloat
        53 => Ok(1),                                    // L8_UNorm
        54 => Ok(1),                                    // A8_UNorm
        55 => Err(GraphicsFormatError::NotImplemented), // A16_UNorm
        56 => Err(GraphicsFormatError::NotSupported),   // B8G8R8_SRGB
        57 => Ok(14),                                   // B8G8R8A8_SRGB
        58 => Err(GraphicsFormatError::NotSupported),   // B8G8R8_UNorm
        59 => Ok(14),                                   // B8G8R8A8_UNorm
        60 => Err(GraphicsFormatError::NotSupported),   // B8G8R8_SNorm
        61 => Ok(14),                                   // B8G8R8A8_SNorm
        62 => Err(GraphicsFormatError::NotSupported),   // B8G8R8_UInt
        63 => Ok(14),                                   // B8G8R8A8_UInt
        64 => Err(GraphicsFormatError::NotSupported),   // B8G8R8_SInt
        65 => Ok(14),                                   // B8G8R8A8_SInt
        66 => Err(GraphicsFormatError::NotSupported),   // R4G4B4A4_UNormPack16
        67 => Err(GraphicsFormatError::NotSupported),   // B4G4R4A4_UNormPack16
        68 => Ok(7),                                    // R5G6B5_UNormPack16
        69 => Err(GraphicsFormatError::NotSupported),   // B5G6R5_UNormPack16
        70 => Err(GraphicsFormatError::NotSupported),   // R5G5B5A1_UNormPack16
        71 => Err(GraphicsFormatError::NotSupported),   // B5G5R5A1_UNormPack16
        72 => Err(GraphicsFormatError::NotSupported),   // A1R5G5B5_UNormPack16
        73 => Err(GraphicsFormatError::NotSupported),   // E5B9G9R9_UFloatPack32
        74 => Err(GraphicsFormatError::NotSupported),   // B10G11R11_UFloatPack32
        75 => Err(GraphicsFormatError::NotSupported),   // A2B10G10R10_UNormPack32
        76 => Err(GraphicsFormatError::NotSupported),   // A2B10G10R10_UIntPack32
        77 => Err(GraphicsFormatError::NotSupported),   // A2B10G10R10_SIntPack32
        78 => Err(GraphicsFormatError::NotSupported),   // A2R10G10B10_UNormPack32
        79 => Err(GraphicsFormatError::NotSupported),   // A2R10G10B10_UIntPack32
        80 => Err(GraphicsFormatError::NotSupported),   // A2R10G10B10_SIntPack32
        81 => Err(GraphicsFormatError::NotSupported),   // A2R10G10B10_XRSRGBPack32
        82 => Err(GraphicsFormatError::NotSupported),   // A2R10G10B10_XRUNormPack32
        83 => Err(GraphicsFormatError::NotSupported),   // R10G10B10_XRSRGBPack32
        84 => Err(GraphicsFormatError::NotSupported),   // R10G10B10_XRUNormPack32
        85 => Err(GraphicsFormatError::NotSupported),   // A10R10G10B10_XRSRGBPack32
        86 => Err(GraphicsFormatError::NotSupported),   // A10R10G10B10_XRUNormPack32
        87 => Ok(5),                                    // A8R8G8B8_SRGB
        88 => Ok(5),                                    // A8R8G8B8_UNorm
        89 => Err(GraphicsFormatError::NotImplemented), // A32R32G32B32_SFloat
        90 => Err(GraphicsFormatError::NotSupported),   // D16_UNorm
        91 => Err(GraphicsFormatError::NotSupported),   // D24_UNorm
        92 => Err(GraphicsFormatError::NotSupported),   // D24_UNorm_S8_UInt
        93 => Err(GraphicsFormatError::NotSupported),   // D32_SFloat
        94 => Err(GraphicsFormatError::NotSupported),   // D32_SFloat_S8_Uint
        95 => Ok(1),                                    // S8_Uint
        96 => Ok(10),                                   // RGB_DXT1_SRGB
        97 => Ok(10),                                   // RGBA_DXT1_UNorm
        98 => Ok(11),                                   // RGBA_DXT3_SRGB
        99 => Ok(11),                                   // RGBA_DXT3_UNorm
        100 => Ok(12),                                  // RGBA_DXT5_SRGB
        101 => Ok(12),                                  // RGBA_DXT5_UNorm
        102 => Ok(26),                                  // R_BC4_UNorm
        103 => Ok(26),                                  // R_BC4_SNorm
        104 => Ok(27),                                  // RG_BC5_UNorm
        105 => Ok(27),                                  // RG_BC5_SNorm
        106 => Ok(24),                                  // RGB_BC6H_UFloat
        107 => Ok(24),                                  // RGB_BC6H_SFloat
        108 => Ok(25),                                  // RGBA_BC7_SRGB
        109 => Ok(25),                                  // RGBA_BC7_UNorm
        110 => Ok(30),                                  // RGB_PVRTC_2Bpp_SRGB
        111 => Ok(30),                                  // RGB_PVRTC_2Bpp_UNorm
        112 => Ok(32),                                  // RGB_PVRTC_4Bpp_SRGB
        113 => Ok(32),                                  // RGB_PVRTC_4Bpp_UNorm
        114 => Ok(31),                                  // RGBA_PVRTC_2Bpp_SRGB
        115 => Ok(31),                                  // RGBA_PVRTC_2Bpp_UNorm
        116 => Ok(33),                                  // RGBA_PVRTC_4Bpp_SRGB
        117 => Ok(33),                                  // RGBA_PVRTC_4Bpp_UNorm
        118 => Ok(34),                                  // RGB_ETC_UNorm
        119 => Ok(45),                                  // RGB_ETC2_SRGB
        120 => Ok(45),                                  // RGB_ETC2_UNorm
        121 => Ok(46),                                  // RGB_A1_ETC2_SRGB
        122 => Ok(46),                                  // RGB_A1_ETC2_UNorm
        123 => Ok(47),                                  // RGBA_ETC2_SRGB
        124 => Ok(47),                                  // RGBA_ETC2_UNorm
        125 => Ok(41),                                  // R_EAC_UNorm
        126 => Ok(41),                                  // R_EAC_SNorm
        127 => Ok(43),                                  // RG_EAC_UNorm
        128 => Ok(43),                                  // RG_EAC_SNorm
        129 => Ok(48),                                  // RGBA_ASTC4X4_SRGB
        130 => Ok(48),                                  // RGBA_ASTC4X4_UNorm
        131 => Ok(49),                                  // RGBA_ASTC5X5_SRGB
        132 => Ok(49),                                  // RGBA_ASTC5X5_UNorm
        133 => Ok(50),                                  // RGBA_ASTC6X6_SRGB
        134 => Ok(50),                                  // RGBA_ASTC6X6_UNorm
        135 => Ok(51),                                  // RGBA_ASTC8X8_SRGB
        136 => Ok(51),                                  // RGBA_ASTC8X8_UNorm
        137 => Ok(52),                                  // RGBA_ASTC10X10_SRGB
        138 => Ok(52),                                  // RGBA_ASTC10X10_UNorm
        139 => Ok(53),                                  // RGBA_ASTC12X12_SRGB
        140 => Ok(53),                                  // RGBA_ASTC12X12_UNorm
        141 => Err(GraphicsFormatError::NotSupported),  // YUV2
        142 => Err(GraphicsFormatError::NotSupported),  // DepthAuto
        143 => Err(GraphicsFormatError::NotSupported),  // ShadowAuto
        144 => Err(GraphicsFormatError::NotSupported),  // VideoAuto
        145 => Ok(48),                                  // RGBA_ASTC4X4_UFloat
        146 => Ok(49),                                  // RGBA_ASTC5X5_UFloat
        147 => Ok(50),                                  // RGBA_ASTC6X6_UFloat
        148 => Ok(51),                                  // RGBA_ASTC8X8_UFloat
        149 => Ok(52),                                  // RGBA_ASTC10X10_UFloat
        150 => Ok(53),                                  // RGBA_ASTC12X12_UFloat
        151 => Err(GraphicsFormatError::NotSupported),  // D16_UNorm_S8_UInt
        other => Err(GraphicsFormatError::ArgumentOutOfRange { format: other }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `GraphicsFormatExtensions` (probe
    /// `Temp/opencode/texprobe`).
    #[test]
    fn graphics_format_mapping_matches_the_reference_runtime() {
        let fixture = include_str!("graphics_format_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "GF", "{line}");
            let format: i32 = parts[2].parse().unwrap();
            let expected = match parts[3] {
                "NotSupportedException" => Err(GraphicsFormatError::NotSupported),
                "NotImplementedException" => Err(GraphicsFormatError::NotImplemented),
                value => Ok(value.parse::<i32>().unwrap()),
            };
            assert_eq!(to_texture_format(format), expected, "{line}");
            rows += 1;
        }
        assert_eq!(rows, 156);
        assert_eq!(
            to_texture_format(1_000_000),
            Err(GraphicsFormatError::ArgumentOutOfRange { format: 1_000_000 })
        );
    }

    #[test]
    fn graphics_format_edges_cover_negatives_and_messages() {
        // Negative values fall through to the out-of-range arm, like the
        // source switch: no unsigned conversion may silently accept them.
        assert_eq!(
            to_texture_format(-1),
            Err(GraphicsFormatError::ArgumentOutOfRange { format: -1 })
        );
        assert_eq!(
            to_texture_format(i32::MIN),
            Err(GraphicsFormatError::ArgumentOutOfRange { format: i32::MIN })
        );
        // The error messages pin the reporting strings.
        assert_eq!(
            GraphicsFormatError::NotSupported.to_string(),
            "GraphicsFormat has no TextureFormat conversion"
        );
        assert_eq!(
            GraphicsFormatError::NotImplemented.to_string(),
            "GraphicsFormat conversion is not implemented"
        );
        assert_eq!(
            GraphicsFormatError::ArgumentOutOfRange { format: 152 }.to_string(),
            "GraphicsFormat value 152 is out of range"
        );
    }
}
