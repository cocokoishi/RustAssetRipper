//! `TextureFormatExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/TextureFormatExtensions.cs`. The three
//! classifiers answer whether a texture format is block-crunched, DXT-family, or carried as
//! RGBA-family data. `Alpha8` is classified as RGB by the baseline despite being single-channel.
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
}
