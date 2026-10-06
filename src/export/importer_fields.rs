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
//! - The `TextureExportCollection.AddSprites` importer decisions: Single versus Multiple mode
//!   (with the shipped `SpriteImportMode`/`SpriteAlignment` discriminants read off the
//!   generated DLL by reflection), the sprite-supplied extrude/mesh-type/pivot/border/
//!   pixels-per-unit, and the sprite-sheet build gate. Version-gated application to a
//!   generated importer object stays caller-side.
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

/// Baseline `SpriteImportMode` discriminants (read off the shipped
/// `AssetRipper.SourceGenerated.dll` with a reflection probe: None 0, Single 1,
/// Multiple 2, Polygon 3). `AddSprites` writes only Single and Multiple.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpriteImportMode {
    /// One sprite (or none): per-sprite sheet of exactly one entry.
    Single = 1,
    /// Several sprites, or one sprite that does not cover its texture.
    Multiple = 2,
}

/// Baseline `SpriteAlignment` discriminants used by `AddSprites` (Center 0,
/// Custom 9 in the shipped enums).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpriteAlignment {
    /// The default and multi-sprite alignment.
    Center = 0,
    /// The single matching-sprite alignment.
    Custom = 9,
}

/// Baseline `SpriteMeshType.FullRect` discriminant (0 in the shipped enums).
/// The empty-sprite path writes it; otherwise the sprite render-data mesh type
/// (an opaque generated integer) passes through untouched.
pub const SPRITE_MESH_TYPE_FULL_RECT: i32 = 0;

/// Baseline `TextureImporterType.Sprite` discriminant (8 in the shipped enums),
/// written by every non-empty `AddSprites` path.
pub const TEXTURE_IMPORTER_TYPE_SPRITE: i32 = 8;

/// Caller facts for one sprite in `TextureExportCollection.AddSprites`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteFacts {
    /// The sprite `Rect` (`[x, y, width, height]`).
    pub rect: [f32; 4],
    /// The sprite render-data texture rect in the same layout.
    pub texture_rect: [f32; 4],
    /// Whether the sprite name equals the texture name.
    pub name_matches_texture: bool,
    /// The sprite `Extrude`.
    pub extrude: u32,
    /// The sprite render-data mesh type (opaque generated integer).
    pub mesh_type: i32,
    /// Whether the sprite exposes a pivot.
    pub has_pivot: bool,
    /// The sprite pivot.
    pub pivot: [f32; 2],
    /// Whether the sprite exposes a border.
    pub has_border: bool,
    /// The sprite border.
    pub border: [f32; 4],
    /// The sprite `PixelsToUnits`.
    pub pixels_to_units: f32,
}

/// The importer fields `AddSprites` assigns, before version-gated application.
///
/// Fields the source leaves untouched (the texture type on the empty path, which
/// keeps the `ImporterFactory` default) are reported as `None`/false so callers
/// can distinguish "write" from "leave alone".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteImporterDecision {
    /// `SpriteModeE`.
    pub mode: SpriteImportMode,
    /// Whether `TextureTypeE` is forced to Sprite (empty path leaves it alone).
    pub texture_type_is_sprite: bool,
    /// `SpriteExtrude`.
    pub extrude: u32,
    /// `SpriteMeshType`.
    pub mesh_type: i32,
    /// `Alignment`.
    pub alignment: SpriteAlignment,
    /// `SpritePivot` when the importer exposes it (and the sprite supplies one
    /// on the single-sprite path).
    pub pivot: Option<[f32; 2]>,
    /// `SpriteBorder` when both sides expose it (single-sprite path only).
    pub border: Option<[f32; 4]>,
    /// `SpritePixelsToUnits`.
    pub pixels_to_units: f32,
    /// Whether `AddSpriteSheet`/`AddIDToName` run (`m_exportSprites` with sprites).
    pub build_sprite_sheet: bool,
}

/// `TextureExportCollection.AddSprites` over caller facts.
///
/// With no sprites the importer takes Single mode, extrude 1, `FullRect`, center
/// alignment, a centered pivot, and 100 pixels per unit, leaving the texture
/// type at its factory default. With exactly one sprite the mode is Single only
/// when the sprite rect equals its render-data texture rect and the sprite name
/// equals the texture name, else Multiple; the sprite's extrude, mesh type,
/// pivot, border, and pixels-per-unit apply. With several sprites the first
/// sprite supplies extrude, mesh type, and pixels-per-unit under Multiple mode
/// with center alignment. The sprite sheet and ID table build only when
/// `export_sprites` is set and at least one sprite is present.
pub fn decide_sprite_importer(
    sprites: &[SpriteFacts],
    export_sprites: bool,
    importer_has_pivot: bool,
    importer_has_border: bool,
) -> SpriteImporterDecision {
    match sprites {
        [] => SpriteImporterDecision {
            mode: SpriteImportMode::Single,
            texture_type_is_sprite: false,
            extrude: texture_importer::SPRITE_EXTRUDE as u32,
            mesh_type: SPRITE_MESH_TYPE_FULL_RECT,
            alignment: SpriteAlignment::Center,
            pivot: importer_has_pivot.then_some([0.5, 0.5]),
            border: None,
            pixels_to_units: texture_importer::SPRITE_PIXELS_TO_UNITS,
            build_sprite_sheet: false,
        },
        [sprite] => {
            let single = sprite.rect == sprite.texture_rect && sprite.name_matches_texture;
            SpriteImporterDecision {
                mode: if single {
                    SpriteImportMode::Single
                } else {
                    SpriteImportMode::Multiple
                },
                texture_type_is_sprite: true,
                extrude: sprite.extrude,
                mesh_type: sprite.mesh_type,
                alignment: SpriteAlignment::Custom,
                pivot: (importer_has_pivot && sprite.has_pivot).then_some(sprite.pivot),
                border: (importer_has_border && sprite.has_border).then_some(sprite.border),
                pixels_to_units: sprite.pixels_to_units,
                build_sprite_sheet: export_sprites,
            }
        }
        [first, ..] => SpriteImporterDecision {
            mode: SpriteImportMode::Multiple,
            texture_type_is_sprite: true,
            extrude: first.extrude,
            mesh_type: first.mesh_type,
            alignment: SpriteAlignment::Center,
            pivot: importer_has_pivot.then_some([0.5, 0.5]),
            border: None,
            pixels_to_units: first.pixels_to_units,
            build_sprite_sheet: export_sprites,
        },
    }
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
    /// `SceneExportCollection.ExportScene`: `DefaultImporter` with bundle propagation.
    Default,
    /// `ScriptExportCollectionBase.OnScriptExported`: `MonoImporter` with truncated order.
    Mono,
    /// `ScriptExportCollection.OnAssemblyExported`: `PluginImporter` with platform entries.
    Plugin,
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
    use super::{
        decide_sprite_importer, propagate_asset_bundle_name, select_prefab_importer,
        truncate_execution_order, PrefabImporterBranch, SpriteAlignment, SpriteFacts,
        SpriteImportMode, SPRITE_MESH_TYPE_FULL_RECT, TEXTURE_IMPORTER_TYPE_SPRITE,
    };
    use super::{plugin_importer, texture_importer};

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
    fn sprite_importer_follows_add_sprites_branches() {
        fn sprite(matching: bool) -> SpriteFacts {
            SpriteFacts {
                rect: [1.0, 2.0, 32.0, 16.0],
                texture_rect: if matching {
                    [1.0, 2.0, 32.0, 16.0]
                } else {
                    [0.0, 0.0, 64.0, 64.0]
                },
                name_matches_texture: matching,
                extrude: 3,
                mesh_type: 7,
                has_pivot: true,
                pivot: [0.25, 0.75],
                has_border: true,
                border: [1.0, 2.0, 3.0, 4.0],
                pixels_to_units: 50.0,
            }
        }

        // No sprites: Single mode, factory extrude, FullRect, center alignment,
        // a centered pivot, 100 pixels per unit, texture type untouched, no sheet.
        let empty = decide_sprite_importer(&[], true, true, true);
        assert_eq!(empty.mode, SpriteImportMode::Single);
        assert_eq!(empty.mode as i32, 1);
        assert!(!empty.texture_type_is_sprite);
        assert_eq!(empty.extrude, 1);
        assert_eq!(empty.mesh_type, SPRITE_MESH_TYPE_FULL_RECT);
        assert_eq!(empty.mesh_type, 0);
        assert_eq!(empty.alignment, SpriteAlignment::Center);
        assert_eq!(empty.alignment as i32, 0);
        assert_eq!(empty.pivot, Some([0.5, 0.5]));
        assert_eq!(empty.border, None);
        assert_eq!(empty.pixels_to_units, 100.0);
        assert!(!empty.build_sprite_sheet);
        // Without an importer pivot field there is no pivot either.
        assert_eq!(decide_sprite_importer(&[], true, false, true).pivot, None);

        // One matching sprite: Single mode with the sprite's own geometry.
        let single = decide_sprite_importer(&[sprite(true)], true, true, true);
        assert_eq!(single.mode, SpriteImportMode::Single);
        assert!(single.texture_type_is_sprite);
        assert_eq!(TEXTURE_IMPORTER_TYPE_SPRITE, 8);
        assert_eq!(single.extrude, 3);
        assert_eq!(single.mesh_type, 7);
        assert_eq!(single.alignment, SpriteAlignment::Custom);
        assert_eq!(single.alignment as i32, 9);
        assert_eq!(single.pivot, Some([0.25, 0.75]));
        assert_eq!(single.border, Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(single.pixels_to_units, 50.0);
        assert!(single.build_sprite_sheet);
        // The sheet builds only when sprite export is enabled.
        assert!(!decide_sprite_importer(&[sprite(true)], false, true, true).build_sprite_sheet);

        // One sprite whose rect or name differs: Multiple mode, same sprite values.
        for other in [sprite(false), sprite(true)] {
            let mut other = other;
            if other.rect == other.texture_rect {
                other.name_matches_texture = false;
            }
            let multiple = decide_sprite_importer(&[other], true, true, true);
            assert_eq!(multiple.mode, SpriteImportMode::Multiple);
            assert_eq!(multiple.mode as i32, 2);
            assert_eq!(multiple.alignment, SpriteAlignment::Custom);
            assert_eq!(multiple.extrude, 3);
        }

        // Missing sprite pivot or border suppresses that copy only.
        let mut no_pivot = sprite(true);
        no_pivot.has_pivot = false;
        let gated = decide_sprite_importer(&[no_pivot], true, true, true);
        assert_eq!(gated.pivot, None);
        assert_eq!(gated.border, Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(
            decide_sprite_importer(&[sprite(true)], true, true, false).border,
            None
        );

        // Several sprites: Multiple mode with the first sprite's values.
        let many = decide_sprite_importer(&[sprite(false), sprite(true)], true, true, true);
        assert_eq!(many.mode, SpriteImportMode::Multiple);
        assert!(many.texture_type_is_sprite);
        assert_eq!(many.extrude, 3);
        assert_eq!(many.mesh_type, 7);
        assert_eq!(many.alignment, SpriteAlignment::Center);
        assert_eq!(many.pivot, Some([0.5, 0.5]));
        assert_eq!(many.border, None);
        assert_eq!(many.pixels_to_units, 50.0);
        assert!(many.build_sprite_sheet);
        assert!(
            !decide_sprite_importer(&[sprite(false), sprite(true)], false, true, true)
                .build_sprite_sheet
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
