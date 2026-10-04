//! Importer field values ported from the frozen baseline's `CreateImporter` overrides.
//!
//! Every export collection writes its `.meta` sidecar around a generated importer object.
//! The importer classes themselves come from the external `AssetRipper.SourceGenerated`
//! package, so this module ports only what the snapshot determines exactly:
//!
//! - The `AssetBundleName` propagation shared by eight importers: when the generated importer
//!   exposes the field (`Has_AssetBundleName_R`) and the source asset has a non-null bundle
//!   name, the name is copied verbatim. The source checks null only, so an empty string would
//!   propagate too; that case is preserved rather than "fixed".
//! - The unconditional literal defaults of `ImporterFactory.GenerateTextureImporter`: mipmap
//!   fades, bump height scale, compression quality, sprite geometry defaults, alpha handling,
//!   cubemap scaling, and the single `DefaultTexturePlatform` platform-settings block.
//! - The `PluginImporter` platform settings written by `ScriptExportCollection.OnAssemblyExported`.
//! - The `PrefabExportCollection` importer branch: a legacy prefab marker selects the native
//!   format importer, otherwise a `PrefabImporter`.
//! - The `CreateImporter` dispatch table: the base `AssetExportCollection<T>` default plus all
//!   nine overrides found by source search, with each collection mapped to its importer kind
//!   and factory. Asset-copied fields and generated `Has_*` gates stay open; only the
//!   dispatch shape and the unconditional literals below are pinned.
//! - The `MonoImporter` execution-order narrowing: `(short)script.ExecutionOrder`, which wraps
//!   exactly like Rust's `as` conversion because C# explicit conversions are unchecked by default.
//!
//! Everything that copies asset state (`TextureImporterData` texture reads, font metrics,
//! audio settings, video dimensions, shader texture maps) or branches on generated
//! `Has_*` version gates stays open: the member lists and the gate thresholds live in the
//! generated package, and porting them from memory would be guessing.

/// Copy an asset bundle name onto an importer exactly like the eight baseline overrides.
///
/// The copy happens only when the generated importer exposes the field and the source name is
/// non-null. An empty-but-present name propagates, matching the source null check.
pub fn propagate_asset_bundle_name(
    importer_has_field: bool,
    source_bundle_name: Option<&str>,
) -> Option<String> {
    if importer_has_field {
        source_bundle_name.map(str::to_owned)
    } else {
        None
    }
}

/// Truncate a script execution order like `(short)script.ExecutionOrder`.
///
/// Both C# unchecked explicit conversions and Rust `as` casts wrap modulo 2^16.
pub fn truncate_execution_order(script_execution_order: i32) -> i16 {
    script_execution_order as i16
}

/// Which importer `PrefabExportCollection.CreateImporter` selects.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PrefabImporterBranch {
    /// A legacy prefab marker: the base native format importer.
    LegacyPrefabAsset,
    /// A modern prefab instance: a `PrefabImporter` carrying the root bundle name.
    ModernPrefabImporter,
}

/// The `EmitPrefabAsset` branch: `Prefab is IPrefabMarker` selects the legacy asset path.
pub fn select_prefab_importer(is_prefab_marker: bool) -> PrefabImporterBranch {
    if is_prefab_marker {
        PrefabImporterBranch::LegacyPrefabAsset
    } else {
        PrefabImporterBranch::ModernPrefabImporter
    }
}

/// Unconditional literals from `ImporterFactory.GenerateTextureImporter`.
pub mod texture_importer {
    /// `MipMaps.MipMapFadeDistanceStart`.
    pub const MIPS_FADE_DISTANCE_START: i32 = 1;
    /// `MipMaps.MipMapFadeDistanceEnd`.
    pub const MIPS_FADE_DISTANCE_END: i32 = 3;
    /// `MipMaps.AlphaTestReferenceValue`.
    pub const MIPS_ALPHA_TEST_REFERENCE_VALUE: f32 = 0.5;
    /// `BumpMap.HeightScale`.
    pub const BUMP_HEIGHT_SCALE: f32 = 0.25;
    /// `StreamingMipmapsPriority` is copied, but `CompressionQuality` is always 50.
    pub const COMPRESSION_QUALITY: i32 = 50;
    /// `SpriteExtrude` for the default and multi-sprite paths.
    pub const SPRITE_EXTRUDE: i32 = 1;
    /// `SpritePixelsToUnits` for the default and multi-sprite paths.
    pub const SPRITE_PIXELS_TO_UNITS: f32 = 100.0;
    /// `SpriteTessellationDetail`.
    pub const SPRITE_TESSELLATION_DETAIL: i32 = -1;
    /// `SpriteGenerateFallbackPhysicsShape`.
    pub const SPRITE_GENERATE_FALLBACK_PHYSICS_SHAPE: i32 = 1;
    /// `AlphaIsTransparency`.
    pub const ALPHA_IS_TRANSPARENCY: i32 = 1;
    /// The single platform-settings block target.
    pub const PLATFORM_BUILD_TARGET: &str = "DefaultTexturePlatform";
}

/// Platform settings from `ScriptExportCollection.OnAssemblyExported`.
pub mod plugin_importer {
    /// The always-enabled `Any` platform entry and its empty CPU value.
    pub const ANY_PLATFORM_NAME: &str = "Any";
    /// The disabled `Editor` platform entry and its CPU value.
    pub const EDITOR_PLATFORM_NAME: &str = "Editor";
    pub const EDITOR_CPU: &str = "Editor";
    /// `Settings["DefaultValueInitialized"]` on the editor entry.
    pub const DEFAULT_VALUE_INITIALIZED: (&str, &str) = ("DefaultValueInitialized", "true");
}

/// Which importer object a collection's `CreateImporter` override constructs.
///
/// Variant names are Rust descriptions; the `importer` strings in the table below carry the
/// exact baseline importer type names.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CreateImporterKind {
    /// `AssetExportCollection<T>`: a fresh `NativeFormatImporter` carrying the main export ID.
    NativeFormat,
    /// `AudioExportCollection`: `AudioImporter` with asset-copied fields.
    Audio,
    /// `FontAssetExportCollection`: `TrueTypeFontImporter` with font-copied fields.
    TrueTypeFont,
    /// `TextAssetExportCollection`: a bare `TextScriptImporter` plus bundle propagation.
    TextScript,
    /// `VideoClipExportCollection`: `VideoClipImporter` with frame fields and `ImportAudio`.
    VideoClip,
    /// `PrefabExportCollection`: base native importer or a fresh `PrefabImporter`.
    Prefab,
    /// `ShaderExportCollection`: `ShaderImporter` with a gated texture-map copy.
    Shader,
    /// `ImporterFactory.GenerateTextureImporter` result (`TextureImporter`).
    GeneratedTexture,
    /// `ImporterFactory.GenerateIHVImporter` result (`IHVImageFormatImporter`).
    GeneratedIhv,
}

/// One `CreateImporter` override in the frozen baseline.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CreateImporterRegistration {
    /// The exporting collection type name.
    pub collection: &'static str,
    /// The importer type name the override constructs.
    pub importer: &'static str,
    /// Which factory shape builds it.
    pub kind: CreateImporterKind,
    /// True when the override delegates to `ImporterFactory` instead of constructing directly.
    pub via_importer_factory: bool,
}

/// Every `CreateImporter` override found by source search, in file-discovery order.
///
/// Subclasses without their own override (for example the concrete audio/texture collection
/// subclasses) inherit the nearest row above; the table records only methods the snapshot
/// actually declares.
pub const CREATE_IMPORTER_TABLE: &[CreateImporterRegistration] = &[
    CreateImporterRegistration {
        collection: "AssetExportCollection<T>",
        importer: "NativeFormatImporter",
        kind: CreateImporterKind::NativeFormat,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "AudioExportCollection",
        importer: "AudioImporter",
        kind: CreateImporterKind::Audio,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "FontAssetExportCollection",
        importer: "TrueTypeFontImporter",
        kind: CreateImporterKind::TrueTypeFont,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "TextAssetExportCollection",
        importer: "TextScriptImporter",
        kind: CreateImporterKind::TextScript,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "VideoClipExportCollection",
        importer: "VideoClipImporter",
        kind: CreateImporterKind::VideoClip,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "PrefabExportCollection",
        importer: "PrefabImporter",
        kind: CreateImporterKind::Prefab,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "ShaderExportCollection",
        importer: "ShaderImporter",
        kind: CreateImporterKind::Shader,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "LightmapExportCollection",
        importer: "TextureImporter",
        kind: CreateImporterKind::GeneratedTexture,
        via_importer_factory: true,
    },
    CreateImporterRegistration {
        collection: "TextureArrayAssetExportCollection",
        importer: "TextureImporter",
        kind: CreateImporterKind::GeneratedTexture,
        via_importer_factory: true,
    },
    CreateImporterRegistration {
        collection: "TextureExportCollection",
        importer: "TextureImporter",
        kind: CreateImporterKind::GeneratedTexture,
        via_importer_factory: true,
    },
];

/// The `TextureExportCollection` convert flag selects between the two `ImporterFactory`
/// entry points: a converting export uses `GenerateTextureImporter`, a non-converting one
/// uses `GenerateIHVImporter` (which warns below Unity 5.6 and calls `SetToDefault`).
pub fn select_texture_collection_importer(convert: bool) -> CreateImporterKind {
    if convert {
        CreateImporterKind::GeneratedTexture
    } else {
        CreateImporterKind::GeneratedIhv
    }
}

/// Unconditional literals inside the font, video, and IHV importer paths.
///
/// Asset-copied values (font sizes, audio settings, video dimensions, texture data) and the
/// numeric values of generated enums stay open; only the literal names and fixed booleans the
/// snapshot spells out are pinned here.
pub mod fixed_importer_literals {
    /// `TrueTypeFontImporter.IncludeFontData` is always set to true.
    pub const FONT_INCLUDE_FONT_DATA: bool = true;
    /// `TrueTypeFontImporter.AscentCalculationModeE` literal name.
    pub const FONT_ASCENT_CALCULATION_MODE: &str = "FaceAscender";
    /// `TrueTypeFontImporter.ForceTextureCaseE` literal name.
    pub const FONT_FORCE_TEXTURE_CASE: &str = "Dynamic";
    /// `VideoClipImporter.ImportAudio` is always set to true.
    pub const VIDEO_IMPORT_AUDIO: bool = true;
    /// `VideoClipImporter.FrameCount` always equals the just-written `EndFrame`.
    pub const VIDEO_FRAME_COUNT_EQUALS_END_FRAME: bool = true;
    /// `GenerateIHVImporter` warns when the export version is below this Unity version.
    pub const IHV_MINIMUM_VERSION: (u32, u32) = (5, 6);
    /// `GenerateTextureImporter` platform-settings literal names.
    pub const TEXTURE_RESIZE_ALGORITHM: &str = "Mitchell";
    pub const TEXTURE_PLATFORM_FORMAT: &str = "Automatic";
    pub const TEXTURE_PLATFORM_COMPRESSION: &str = "CompressedHQ";
    /// `GenerateTextureImporter` swizzle order literal names.
    pub const TEXTURE_SWIZZLE_ORDER: [&str; 4] = ["R", "G", "B", "A"];
}

#[cfg(test)]
mod tests {
    use super::{plugin_importer, texture_importer};
    use super::{
        propagate_asset_bundle_name, select_prefab_importer, truncate_execution_order,
        PrefabImporterBranch,
    };

    #[test]
    fn bundle_name_propagates_only_with_field_and_non_null_source() {
        assert_eq!(
            propagate_asset_bundle_name(true, Some("bundle-a")),
            Some("bundle-a".to_owned())
        );
        // The source checks null only, so an empty name propagates verbatim.
        assert_eq!(
            propagate_asset_bundle_name(true, Some("")),
            Some(String::new())
        );
        assert_eq!(propagate_asset_bundle_name(true, None), None);
        assert_eq!(propagate_asset_bundle_name(false, Some("bundle-a")), None);
        assert_eq!(propagate_asset_bundle_name(false, None), None);
    }

    #[test]
    fn execution_order_truncation_wraps_like_the_source_cast() {
        assert_eq!(truncate_execution_order(0), 0);
        assert_eq!(truncate_execution_order(300), 300);
        assert_eq!(truncate_execution_order(-5), -5);
        assert_eq!(truncate_execution_order(0x1_8001), -32767);
        assert_eq!(truncate_execution_order(i32::MAX), -1);
        assert_eq!(truncate_execution_order(i32::MIN), 0);
    }

    #[test]
    fn prefab_branch_follows_the_legacy_marker() {
        assert_eq!(
            select_prefab_importer(true),
            PrefabImporterBranch::LegacyPrefabAsset
        );
        assert_eq!(
            select_prefab_importer(false),
            PrefabImporterBranch::ModernPrefabImporter
        );
    }

    #[test]
    fn texture_literals_match_generate_texture_importer() {
        assert_eq!(texture_importer::MIPS_FADE_DISTANCE_START, 1);
        assert_eq!(texture_importer::MIPS_FADE_DISTANCE_END, 3);
        assert_eq!(texture_importer::MIPS_ALPHA_TEST_REFERENCE_VALUE, 0.5);
        assert_eq!(texture_importer::BUMP_HEIGHT_SCALE, 0.25);
        assert_eq!(texture_importer::COMPRESSION_QUALITY, 50);
        assert_eq!(texture_importer::SPRITE_EXTRUDE, 1);
        assert_eq!(texture_importer::SPRITE_PIXELS_TO_UNITS, 100.0);
        assert_eq!(texture_importer::SPRITE_TESSELLATION_DETAIL, -1);
        assert_eq!(texture_importer::SPRITE_GENERATE_FALLBACK_PHYSICS_SHAPE, 1);
        assert_eq!(texture_importer::ALPHA_IS_TRANSPARENCY, 1);
        assert_eq!(
            texture_importer::PLATFORM_BUILD_TARGET,
            "DefaultTexturePlatform"
        );
    }

    #[test]
    fn plugin_platform_settings_match_on_assembly_exported() {
        assert_eq!(plugin_importer::ANY_PLATFORM_NAME, "Any");
        assert_eq!(plugin_importer::EDITOR_PLATFORM_NAME, "Editor");
        assert_eq!(plugin_importer::EDITOR_CPU, "Editor");
        assert_eq!(
            plugin_importer::DEFAULT_VALUE_INITIALIZED,
            ("DefaultValueInitialized", "true")
        );
    }

    #[test]
    fn create_importer_table_covers_every_snapshot_override() {
        use super::select_texture_collection_importer;
        use super::{fixed_importer_literals, CreateImporterKind, CREATE_IMPORTER_TABLE};
        // The base default plus the nine overrides found by source search.
        assert_eq!(CREATE_IMPORTER_TABLE.len(), 10);
        assert_eq!(
            CREATE_IMPORTER_TABLE[0].collection,
            "AssetExportCollection<T>"
        );
        assert_eq!(CREATE_IMPORTER_TABLE[0].importer, "NativeFormatImporter");
        assert_eq!(
            CREATE_IMPORTER_TABLE[0].kind,
            CreateImporterKind::NativeFormat
        );
        // Only the texture collections delegate to ImporterFactory.
        let factory_collections: Vec<&str> = CREATE_IMPORTER_TABLE
            .iter()
            .filter(|entry| entry.via_importer_factory)
            .map(|entry| entry.collection)
            .collect();
        assert_eq!(
            factory_collections,
            [
                "LightmapExportCollection",
                "TextureArrayAssetExportCollection",
                "TextureExportCollection"
            ]
        );
        // The prefab row records the modern branch; the legacy branch reuses the base
        // native importer through select_prefab_importer.
        let prefab = CREATE_IMPORTER_TABLE
            .iter()
            .find(|entry| entry.collection == "PrefabExportCollection")
            .unwrap();
        assert_eq!(prefab.importer, "PrefabImporter");
        // The texture convert flag selects between the two factory entry points.
        assert_eq!(
            select_texture_collection_importer(true),
            CreateImporterKind::GeneratedTexture
        );
        assert_eq!(
            select_texture_collection_importer(false),
            CreateImporterKind::GeneratedIhv
        );
        // Fixed literals are pinned by name, not by generated numeric value.
        let font_include_font_data = fixed_importer_literals::FONT_INCLUDE_FONT_DATA;
        assert!(font_include_font_data);
        assert_eq!(
            fixed_importer_literals::FONT_ASCENT_CALCULATION_MODE,
            "FaceAscender"
        );
        assert_eq!(fixed_importer_literals::FONT_FORCE_TEXTURE_CASE, "Dynamic");
        let video_import_audio = fixed_importer_literals::VIDEO_IMPORT_AUDIO;
        assert!(video_import_audio);
        let frame_count_follows_end_frame =
            fixed_importer_literals::VIDEO_FRAME_COUNT_EQUALS_END_FRAME;
        assert!(frame_count_follows_end_frame);
        assert_eq!(fixed_importer_literals::IHV_MINIMUM_VERSION, (5, 6));
        assert_eq!(
            fixed_importer_literals::TEXTURE_SWIZZLE_ORDER,
            ["R", "G", "B", "A"]
        );
    }
}
