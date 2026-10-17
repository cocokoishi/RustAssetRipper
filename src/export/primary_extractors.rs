//! `BinaryAssetContentExtractor` and the two unconditional primary-content extractors.
//!
//! The primary-content pipeline's handler table and loop are already ported
//! (`primary_content.rs`); what was missing is what each extractor *decides*. This module ports the
//! decision half for the extractors whose payload is already-decoded bytes, leaving the file write
//! and the asset lookups with their owners.
//!
//! Three of the primary-content extractors do not examine the asset at all:
//!
//! - `EmptyContentExtractor` is registered for twenty interfaces and **claims** each asset, but it
//!   claims into the shared `EmptyExportCollection`. That is not a decline: `CreateCollections`
//!   adds the asset to the queued set first and then drops the empty collection, which is exactly
//!   why an asset reaching an empty handler is never reconsidered by a later handler.
//! - `JsonContentExtractor` claims everything it is reached for and writes one JSON document with
//!   the `json` extension. It is registered first for `IUnityObjectBase`, so in the default stack it
//!   is the fallback that makes `CreateCollection`'s "no exporter can handle this" unreachable.
//! - `BinaryAssetContentExtractor` is the one that inspects the asset, and it is ported below.
//!
//! `BinaryAssetContentExtractor.TryCreateCollection` switches on four interfaces, and each arm
//! requires a **non-empty payload** before it admits: an asset with an empty payload falls through
//! to the next handler rather than exporting an empty file. `VideoClip` differs - it tests
//! `CheckIntegrity()` instead of a length, because its payload streams from an external resource.

use super::asset_path::HostPathPlatform;
use super::resource_extensions::{font_extension, video_export_extension};

/// `TextAssetExportCollection.ExportExtension`.
pub const TEXT_ASSET_EXTENSION: &str = "bytes";
/// `MovieTextureExportCollection.ExportExtension`.
pub const MOVIE_TEXTURE_EXTENSION: &str = "ogv";
/// `JsonExportCollection.ExportExtension`.
pub const JSON_EXTENSION: &str = "json";
/// `VideoClipExtensions.GetExtensionFromPath`'s fallback when the original path has no extension.
pub const VIDEO_CLIP_FALLBACK_EXTENSION: &str = "bytes";
/// `FontExtensions.GetFontExtension`'s fallback for data too short to carry a four-character code.
pub const FONT_FALLBACK_EXTENSION: &str = "ttf";

/// The asset facts `BinaryAssetContentExtractor.TryCreateCollection` reads.
///
/// One variant per arm of the reference's type switch. Each carries only what the corresponding
/// collection needs, so a caller cannot accidentally supply a payload the reference would not have
/// looked at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryAssetFacts<'a> {
    /// `ITextAsset`, with `Script_C49.Data`.
    TextAsset { script_data: &'a [u8] },
    /// `IFont`, with `FontData`.
    Font { font_data: &'a [u8] },
    /// `IMovieTexture`. `movie_data` is `None` when `Has_MovieData_C152()` is false, which the
    /// reference distinguishes from a present-but-empty payload.
    MovieTexture { movie_data: Option<&'a [u8]> },
    /// `IVideoClip`. `intact` is `CheckIntegrity()`, the reference's gate for this arm.
    VideoClip {
        intact: bool,
        original_path: &'a str,
    },
}

/// What `BinaryAssetContentExtractor` decided for one asset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryAssetCollection {
    /// The `ExportExtension` the collection reports, which becomes the file suffix.
    pub extension: String,
}

impl BinaryAssetCollection {
    fn new(extension: impl Into<String>) -> Self {
        Self {
            extension: extension.into(),
        }
    }
}

/// `BinaryAssetContentExtractor.TryCreateCollection`.
///
/// `None` means the extractor declined and the handler stack continues - which is different from
/// the empty handler's claim, and the distinction is the reason each arm tests its payload instead
/// of admitting unconditionally. `VideoClip` needs the platform because its extension is taken from
/// the serialized path with `Path.GetExtension` semantics.
pub fn binary_asset_claim(
    facts: BinaryAssetFacts<'_>,
    platform: HostPathPlatform,
) -> Option<BinaryAssetCollection> {
    match facts {
        // `TextAssetExportCollection.Create`: admits only non-empty script data.
        BinaryAssetFacts::TextAsset { script_data } => {
            (!script_data.is_empty()).then(|| BinaryAssetCollection::new(TEXT_ASSET_EXTENSION))
        }
        // `FontExportCollection.Create`: admits only non-empty font data, and the extension is read
        // from the payload.
        BinaryAssetFacts::Font { font_data } => {
            (!font_data.is_empty()).then(|| BinaryAssetCollection::new(font_extension(font_data)))
        }
        // `MovieTextureExportCollection.Create`: the reference requires BOTH `Has_MovieData_C152()`
        // and a non-empty payload, so an absent payload declines even though an empty one would
        // too - the two conditions are separate in the source and are kept separate here.
        BinaryAssetFacts::MovieTexture { movie_data } => movie_data
            .filter(|data| !data.is_empty())
            .map(|_| BinaryAssetCollection::new(MOVIE_TEXTURE_EXTENSION)),
        // `VideoClipExportCollection.Create`: gated on integrity, not on a length.
        BinaryAssetFacts::VideoClip {
            intact,
            original_path,
        } => intact
            .then(|| BinaryAssetCollection::new(video_export_extension(original_path, platform))),
    }
}

// `FontExtensions.GetFontExtension` is already ported at
// `resource_extensions::font_extension`, and `VideoClipExtensions.GetExtensionFromPath` at
// `resource_extensions::video_export_extension`. An earlier version of this module reimplemented
// both, which is exactly the duplication this repository has now been bitten by twice: the second
// copy is where the harder half of a rule goes missing, and its own tests will not notice. They are
// imported above and reused here.

#[cfg(test)]
mod tests {
    use super::{
        binary_asset_claim, font_extension, video_export_extension, BinaryAssetFacts,
        MOVIE_TEXTURE_EXTENSION, TEXT_ASSET_EXTENSION, VIDEO_CLIP_FALLBACK_EXTENSION,
    };
    use crate::export::asset_path::HostPathPlatform;

    const WINDOWS: HostPathPlatform = HostPathPlatform::Windows;

    fn extension_of(facts: BinaryAssetFacts<'_>) -> Option<String> {
        binary_asset_claim(facts, WINDOWS).map(|collection| collection.extension)
    }

    /// Each arm admits only a non-empty payload, and each declined arm is a fall-through.
    #[test]
    fn binary_arms_require_a_non_empty_payload() {
        assert_eq!(
            extension_of(BinaryAssetFacts::TextAsset {
                script_data: b"data"
            }),
            Some(TEXT_ASSET_EXTENSION.to_owned())
        );
        assert_eq!(
            extension_of(BinaryAssetFacts::TextAsset { script_data: b"" }),
            None,
            "an empty TextAsset declines so the stack continues"
        );

        assert_eq!(
            extension_of(BinaryAssetFacts::Font {
                font_data: b"\x00\x01\x00\x00rest"
            }),
            Some("ttf".to_owned())
        );
        assert_eq!(
            extension_of(BinaryAssetFacts::Font { font_data: b"" }),
            None
        );

        assert_eq!(
            extension_of(BinaryAssetFacts::MovieTexture {
                movie_data: Some(b"ogv")
            }),
            Some(MOVIE_TEXTURE_EXTENSION.to_owned())
        );
        // The two conditions are separate in the source: absent declines, and so does empty.
        assert_eq!(
            extension_of(BinaryAssetFacts::MovieTexture { movie_data: None }),
            None
        );
        assert_eq!(
            extension_of(BinaryAssetFacts::MovieTexture {
                movie_data: Some(b"")
            }),
            None
        );

        // A VideoClip with intact resources is admitted regardless of any length, and one without
        // them declines even when its path looks like a video.
        assert_eq!(
            extension_of(BinaryAssetFacts::VideoClip {
                intact: true,
                original_path: "clip.mp4"
            }),
            Some("mp4".to_owned())
        );
        assert_eq!(
            extension_of(BinaryAssetFacts::VideoClip {
                intact: false,
                original_path: "clip.mp4"
            }),
            None
        );
    }

    /// `GetFontExtension` selects on the payload's four-character code.
    #[test]
    fn font_extension_reads_the_payload_four_character_code() {
        // `OTTO` little-endian: O=0x4F, T=0x54, T=0x54, O=0x4F.
        assert_eq!(font_extension(b"OTTOrest of the font"), "otf");
        // The same four bytes in the other order are not the code, so they stay `ttf`.
        assert_eq!(font_extension(b"OTTO"), "otf");
        assert_eq!(font_extension(b"otto"), "ttf");
        // TrueType's own code is not special-cased: everything but OTTO is `ttf`.
        assert_eq!(font_extension(b"\x00\x01\x00\x00"), "ttf");
        // Shorter than the code: the fallback the reference documents as "just in case".
        assert_eq!(font_extension(b""), "ttf");
        assert_eq!(font_extension(b"OTT"), "ttf");
        assert_eq!(font_extension(b"OTT"), font_extension(b""));
    }

    /// `GetExtensionFromPath` strips the dot and falls back to `bytes`.
    #[test]
    fn video_clip_extension_strips_the_dot_and_falls_back() {
        assert_eq!(video_export_extension("clip.mp4", WINDOWS), "mp4");
        assert_eq!(video_export_extension("a.b/clip.webm", WINDOWS), "webm");
        assert_eq!(
            video_export_extension("clip", WINDOWS),
            VIDEO_CLIP_FALLBACK_EXTENSION
        );
        // A trailing dot yields the empty extension from `Path.GetExtension`, so the fallback
        // applies; this is the edge the dot-strip would otherwise turn into a panic.
        assert_eq!(
            video_export_extension("clip.", WINDOWS),
            VIDEO_CLIP_FALLBACK_EXTENSION
        );
        // A leading dot is an extension when it is the only dot, and the strip leaves `bashrc`.
        assert_eq!(video_export_extension(".bashrc", WINDOWS), "bashrc");
        // The extension is not trimmed.
        assert_eq!(video_export_extension("clip. mp4", WINDOWS), " mp4");
        // On Unix a backslash is not a separator, so the dot after it wins.
        assert_eq!(
            video_export_extension("C:\\dir.d\\x", HostPathPlatform::Unix),
            "d\\x"
        );
        assert_eq!(
            video_export_extension("C:\\dir.d\\x", WINDOWS),
            VIDEO_CLIP_FALLBACK_EXTENSION
        );
    }
}
