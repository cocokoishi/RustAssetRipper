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
}
