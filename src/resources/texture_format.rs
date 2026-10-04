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
}
