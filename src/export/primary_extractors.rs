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
use super::settings::ImageExportFormat;

/// `TextAssetExportCollection.ExportExtension`.
pub const TEXT_ASSET_EXTENSION: &str = "bytes";
/// `MovieTextureExportCollection.ExportExtension`.
pub const MOVIE_TEXTURE_EXTENSION: &str = "ogv";
/// `JsonExportCollection.ExportExtension`.
pub const JSON_EXTENSION: &str = "json";
/// `GlbExportCollection.ExportExtension`, shared by all four GLB extractors.
pub const GLB_EXTENSION: &str = "glb";
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

/// Whether `BinaryAssetContentExtractor` admits this arm, without keeping the extension.
///
/// The claim dispatch only needs the gate, while the extension belongs to the producer - a font's
/// needs the real first four bytes and a video clip's needs the original path. This **delegates** to
/// [`binary_asset_claim`] with whatever data the caller has rather than restating the four gates, so
/// the claim and the producer cannot drift apart. The extension it computes on the way is discarded.
pub fn binary_asset_claims(facts: BinaryAssetFacts<'_>) -> bool {
    binary_asset_claim(facts, HostPathPlatform::host()).is_some()
}

// `FontExtensions.GetFontExtension` is already ported at
// `resource_extensions::font_extension`, and `VideoClipExtensions.GetExtensionFromPath` at
// `resource_extensions::video_export_extension`. An earlier version of this module reimplemented
// both, which is exactly the duplication this repository has now been bitten by twice: the second
// copy is where the harder half of a rule goes missing, and its own tests will not notice. They are
// imported above and reused here.

/// The asset facts the four GLB extractors' `TryCreateCollection` calls read.
///
/// One variant per extractor, because each has its own gate and they are registered in a fixed
/// order. Every admitted collection is a `GlbExportCollection`, so the extension is always `glb` and
/// the variant carries only the decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlbAssetFacts {
    /// `GlbMeshExporter`: an `IMesh` whose `MeshExtensions.IsSet()` holds. The caller resolves
    /// `IsSet` through `mesh::is_set`, so the compressed/streaming detail stays with the mesh reader.
    Mesh { mesh_is_set: bool },
    /// `GlbNavMeshExporter`: an `INavMeshData` whose `HeightMeshes` is non-null. A present-but-empty
    /// list is admitted by the source - only `null` declines.
    NavMesh { height_meshes_present: bool },
    /// `GlbTerrainExporter`: an `ITerrainData`, with **no** further check.
    Terrain,
    /// `GlbModelExporter`: it decides on `asset.MainAsset`, admitting only a scene or prefab
    /// hierarchy and declining everything else - so a bare GameObject falls through to a later
    /// handler rather than exporting an empty model.
    Model { main_asset: GlbModelMainAsset },
}

/// `GlbModelExporter`'s `MainAsset` switch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlbModelMainAsset {
    /// `SceneHierarchyObject`, which routes to `GlbSceneModelExportCollection`.
    SceneHierarchy,
    /// `PrefabHierarchyObject`, which routes to `GlbPrefabModelExportCollection`.
    PrefabHierarchy,
    /// Anything else, which declines.
    Other,
}

/// `TryCreateCollection` for one of the four GLB extractors.
///
/// `None` means that extractor declines and the stack continues. The two model variants report which
/// collection they would build, because the scene and prefab collections differ in how they gather
/// their assets even though both export `glb`.
pub fn glb_asset_claim(facts: GlbAssetFacts) -> Option<GlbAssetCollection> {
    match facts {
        GlbAssetFacts::Mesh { mesh_is_set } => mesh_is_set.then_some(GlbAssetCollection {
            extension: GLB_EXTENSION,
            model_main_asset: None,
        }),
        GlbAssetFacts::NavMesh {
            height_meshes_present,
        } => height_meshes_present.then_some(GlbAssetCollection {
            extension: GLB_EXTENSION,
            model_main_asset: None,
        }),
        // `GlbTerrainExporter` admits every `ITerrainData` unconditionally.
        GlbAssetFacts::Terrain => Some(GlbAssetCollection {
            extension: GLB_EXTENSION,
            model_main_asset: None,
        }),
        GlbAssetFacts::Model { main_asset } => main_asset.admits().then_some(GlbAssetCollection {
            extension: GLB_EXTENSION,
            model_main_asset: Some(main_asset),
        }),
    }
}

impl GlbModelMainAsset {
    /// Whether `GlbModelExporter` admits this main asset.
    const fn admits(self) -> bool {
        matches!(self, Self::SceneHierarchy | Self::PrefabHierarchy)
    }
}

/// What a GLB extractor decided for one asset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlbAssetCollection {
    /// Always `glb`; kept as a field so a caller reads the extension from the decision rather than
    /// assuming it.
    pub extension: &'static str,
    /// Set only for `GlbModelExporter`, which is the one that builds two different collections.
    pub model_main_asset: Option<GlbModelMainAsset>,
}

#[cfg(test)]
mod glb_tests {
    use super::{glb_asset_claim, GlbAssetFacts, GlbModelMainAsset, GLB_EXTENSION};

    /// Each GLB extractor's gate, including the two that admit unconditionally.
    #[test]
    fn glb_extractor_gates_match_the_source() {
        // `GlbMeshExporter` follows `IMesh.IsSet()` rather than merely "is a mesh".
        assert!(glb_asset_claim(GlbAssetFacts::Mesh { mesh_is_set: true }).is_some());
        assert_eq!(
            glb_asset_claim(GlbAssetFacts::Mesh { mesh_is_set: false }),
            None,
            "an unset mesh declines so a later handler can claim it"
        );

        // `GlbNavMeshExporter` tests `HeightMeshes` for null, so a present empty list is admitted.
        assert!(glb_asset_claim(GlbAssetFacts::NavMesh {
            height_meshes_present: true
        })
        .is_some());
        assert_eq!(
            glb_asset_claim(GlbAssetFacts::NavMesh {
                height_meshes_present: false
            }),
            None
        );

        // `GlbTerrainExporter` has no condition at all.
        assert!(glb_asset_claim(GlbAssetFacts::Terrain).is_some());
    }

    /// `GlbModelExporter` admits only a scene or prefab hierarchy main asset.
    #[test]
    fn glb_model_admits_only_hierarchy_main_assets() {
        let scene = glb_asset_claim(GlbAssetFacts::Model {
            main_asset: GlbModelMainAsset::SceneHierarchy,
        })
        .expect("a scene hierarchy is admitted");
        assert_eq!(scene.extension, GLB_EXTENSION);
        assert_eq!(
            scene.model_main_asset,
            Some(GlbModelMainAsset::SceneHierarchy)
        );

        let prefab = glb_asset_claim(GlbAssetFacts::Model {
            main_asset: GlbModelMainAsset::PrefabHierarchy,
        })
        .expect("a prefab hierarchy is admitted");
        assert_eq!(
            prefab.model_main_asset,
            Some(GlbModelMainAsset::PrefabHierarchy),
            "the two hierarchies build different collections"
        );

        assert_eq!(
            glb_asset_claim(GlbAssetFacts::Model {
                main_asset: GlbModelMainAsset::Other
            }),
            None,
            "a bare GameObject declines rather than exporting an empty model"
        );
    }

    /// Every admitted GLB collection reports the same extension.
    #[test]
    fn every_glb_collection_uses_the_glb_extension() {
        let claims = [
            glb_asset_claim(GlbAssetFacts::Mesh { mesh_is_set: true }),
            glb_asset_claim(GlbAssetFacts::NavMesh {
                height_meshes_present: true,
            }),
            glb_asset_claim(GlbAssetFacts::Terrain),
            glb_asset_claim(GlbAssetFacts::Model {
                main_asset: GlbModelMainAsset::SceneHierarchy,
            }),
        ];
        for claim in claims {
            assert_eq!(claim.expect("admitted").extension, GLB_EXTENSION);
        }
    }
}

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

/// `ImageTextureExtensions.CheckAssetIntegrity`: an image texture has bytes somewhere.
///
/// Inline `ImageData` wins outright. Otherwise the texture needs an `m_StreamData` descriptor *and*
/// that descriptor must pass its collection check - which is why the check is a closure rather than a
/// value: the source only consults the collection when there is no inline data, and the caller owns
/// the file lookup that answers it.
///
/// The closure returns `None` when `Has_StreamData_C189()` is false, and `Some(result)` otherwise.
/// Returning a plain `bool` here instead would force the caller to perform the streamed-file lookup
/// even for a texture whose bytes are already in memory.
pub fn image_texture_check_asset_integrity(
    image_data_len: usize,
    stream_check: impl FnOnce() -> Option<bool>,
) -> bool {
    if image_data_len > 0 {
        return true;
    }
    stream_check() == Some(true)
}

/// The collection `AudioContentExtractor` admits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioClipCollection {
    /// The extension the decoder reported, which becomes the export extension.
    pub extension: String,
}

/// `AudioExportCollection.TryCreate`: decode, then require the decoded bytes to be non-empty.
///
/// `decoded` is the outcome of `AudioClipDecoder.TryDecode` - `Some((extension, byte length))` when
/// it succeeded. Both gates matter and the source treats them as separate branches with their own
/// warnings: a clip whose decode fails and one that decodes to nothing are different failures, but
/// both decline so that a later handler can claim the asset.
///
/// The source decodes twice, once here and once in `ExportInner`, to avoid holding the decoded bytes;
/// only the decision is ported, so the caller supplies the first decode's result.
pub fn audio_clip_claim(decoded: Option<(&str, usize)>) -> Option<AudioClipCollection> {
    let (extension, data_len) = decoded?;
    if data_len == 0 {
        return None;
    }
    Some(AudioClipCollection {
        extension: extension.to_owned(),
    })
}

/// What `ScriptContentExtractor` did with a MonoScript.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptClaim {
    /// The first MonoScript creates the script collection, which emits the assemblies, the
    /// decompiled sources and the solution file.
    ScriptCollection,
    /// Every later MonoScript claims into the shared empty collection instead, so the scripts are
    /// not written once per script.
    EmptyCollection,
}

/// `ScriptContentExtractor`'s one-shot claim flag.
///
/// The flag is the only state in the source's extractor, and its exact rule is easy to get wrong: it
/// is cleared **only** when a MonoScript is claimed. A non-MonoScript returns `false` and leaves the
/// flag set, so the first MonoScript that reaches this extractor creates the collection no matter how
/// many other assets passed through it first. Clearing the flag on every call - the reading that
/// "the first asset wins" suggests - would have made the script export depend on registration order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScriptExtractorState {
    first: bool,
}

impl ScriptExtractorState {
    /// A fresh extractor, whose first MonoScript creates the collection.
    pub const fn new() -> Self {
        Self { first: true }
    }

    /// `TryCreateCollection`. `None` means the extractor declines and the stack continues.
    pub fn claim(&mut self, is_mono_script: bool) -> Option<ScriptClaim> {
        if !is_mono_script {
            return None;
        }
        let claim = if self.first {
            ScriptClaim::ScriptCollection
        } else {
            ScriptClaim::EmptyCollection
        };
        self.first = false;
        Some(claim)
    }
}

impl Default for ScriptExtractorState {
    fn default() -> Self {
        Self::new()
    }
}

/// `DeletedAssetsExporter.TryCreateCollection`.
///
/// The caller answers `asset.MainAsset is DeletedAssetsInformation`. The collection it builds
/// reports `Exportable => false`, so the claim exists to keep the deleted assets grouped with their
/// information object rather than to write files - which is why this is a separate extractor from
/// the Unity project pipeline's deleted-assets exporter even though the predicate is the same.
pub const fn deleted_assets_claim(main_asset_is_deleted_information: bool) -> bool {
    main_asset_is_deleted_information
}

/// The extension `TerrainHeatmapExportCollection` reports: the configured image format's.
///
/// **This function has no caller in the baseline.** `TerrainHeatmapExporter` is complete and
/// claims every `ITerrainData` with no condition, but `PrimaryContentExporter.CreateDefault` never
/// registers it and nothing constructs it, so a terrain asset always reaches `GlbTerrainExporter`.
/// The function is kept because it is a real ported rule, and marked because its only user is a
/// preview path rather than the primary-content pipeline.
pub fn terrain_heatmap_extension(format: ImageExportFormat) -> &'static str {
    format.file_extension()
}

#[cfg(test)]
mod remaining_extractor_tests {
    use super::{
        audio_clip_claim, deleted_assets_claim, image_texture_check_asset_integrity,
        terrain_heatmap_extension, ScriptClaim, ScriptExtractorState,
    };
    use crate::export::settings::ImageExportFormat;

    /// Inline data wins and the streamed lookup is not consulted.
    #[test]
    fn texture_integrity_prefers_inline_data_and_skips_the_lookup() {
        assert!(image_texture_check_asset_integrity(4, || panic!(
            "inline data must not trigger the streamed lookup"
        )));

        // No inline data: the descriptor decides.
        assert!(image_texture_check_asset_integrity(0, || Some(true)));
        assert!(!image_texture_check_asset_integrity(0, || Some(false)));
        // `None` is the "no m_StreamData at all" answer, which declines.
        assert!(!image_texture_check_asset_integrity(0, || None));
    }

    /// Both audio gates decline, and they decline for different reasons.
    #[test]
    fn audio_requires_a_successful_decode_of_non_empty_data() {
        assert_eq!(
            audio_clip_claim(Some(("ogg", 128))),
            Some(super::AudioClipCollection {
                extension: "ogg".to_owned()
            })
        );
        assert_eq!(
            audio_clip_claim(Some(("wav", 0))),
            None,
            "a decode to nothing declines"
        );
        assert_eq!(audio_clip_claim(None), None, "a failed decode declines");
    }

    /// The script flag is consumed by a MonoScript and by nothing else.
    #[test]
    fn script_flag_is_consumed_only_by_a_mono_script() {
        let mut state = ScriptExtractorState::default();
        // Assets that are not MonoScripts decline and leave the flag alone.
        for _ in 0..3 {
            assert_eq!(state.claim(false), None);
        }
        // So the first MonoScript, however late, still creates the collection.
        assert_eq!(state.claim(true), Some(ScriptClaim::ScriptCollection));
        assert_eq!(state.claim(true), Some(ScriptClaim::EmptyCollection));
        assert_eq!(state.claim(true), Some(ScriptClaim::EmptyCollection));
        // And a decline after the flag is spent still declines.
        assert_eq!(state.claim(false), None);
        assert_eq!(state.claim(true), Some(ScriptClaim::EmptyCollection));
    }

    /// The heatmap's extension is the configured format's, with the reference's exact spellings.
    #[test]
    fn terrain_heatmap_extension_follows_the_image_format() {
        assert_eq!(terrain_heatmap_extension(ImageExportFormat::Bmp), "bmp");
        assert_eq!(terrain_heatmap_extension(ImageExportFormat::Exr), "exr");
        assert_eq!(terrain_heatmap_extension(ImageExportFormat::Hdr), "hdr");
        assert_eq!(terrain_heatmap_extension(ImageExportFormat::Jpeg), "jpeg");
        assert_eq!(terrain_heatmap_extension(ImageExportFormat::Png), "png");
        assert_eq!(terrain_heatmap_extension(ImageExportFormat::Tga), "tga");
    }

    /// The deleted-assets claim is exactly the main-asset test.
    #[test]
    fn deleted_assets_claim_tracks_the_main_asset() {
        assert!(deleted_assets_claim(true));
        assert!(!deleted_assets_claim(false));
    }
}
