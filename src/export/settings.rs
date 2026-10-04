//! Product settings ported from the frozen baseline's configuration records.
//!
//! Three records drive import, processing, and export, all persisted as JSON singletons:
//!
//! - `ImportSettings`: `ScriptContentLevel` (default `Level2`), `StreamingAssetsMode` (default
//!   `Extract` with an `IgnoreStreamingAssets` shorthand), and the package-defaulted
//!   `DefaultVersion`/`TargetVersion` Unity versions.
//! - `ProcessingSettings`: prefab outlining off, static mesh separation on, asset
//!   deduplication off, nullable removal off, assembly publicizing off, and
//!   `BundledAssetsExportMode::DirectExport`.
//! - `ExportSettings`: `AudioExportFormat::Default`, `ImageExportFormat::Png`,
//!   `LightmapTextureExportMode::Yaml`, `ScriptExportMode::Hybrid`,
//!   `ScriptLanguageVersion::AutoSafe`, unqualified script types, `ShaderExportMode::Dummy`,
//!   `SpriteExportMode::Yaml`, `TextExportMode::Parse`, unreadable assets off, original
//!   texture extensions preferred, settings persistence off, and no language code.
//!
//! The `ImageExportFormat` extension selection is fully determined by the snapshot
//! (`GetFileExtension`, `TryGetFromExtension` including `jpg` for JPEG, and the
//! prefer-original rule over `GetBestExtension`), so it is implemented rather than described.
//! `TerrainExportMode` is declared in the export configuration but consumed nowhere outside a
//! GUI dropdown: no `ExportSettings` field and no exporter reads it. `ScriptLanguageVersion`
//! named variants mirror the external ILSpy `LanguageVersion` values and the automatic
//! selection needs full Unity-version ordering from the external primitives package, so only
//! the snapshot-pinned `AutoExperimental = -2` and `AutoSafe = -1` discriminants plus the
//! documented version thresholds are recorded here.

/// Script loading level in `ImportSettings`; default `Level2`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScriptContentLevel {
    /// Scripts are not loaded.
    Level0,
    /// Methods are stubbed during processing.
    Level1,
    /// Full methods for Mono games, empty methods for IL2Cpp games.
    Level2,
    /// IL2Cpp methods are safely recovered where possible.
    Level3,
}

/// How the StreamingAssets folder is handled; default `Extract`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StreamingAssetsMode {
    Ignore,
    Extract,
}

/// Bundled-asset grouping in `ProcessingSettings`; default `DirectExport`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BundledAssetsExportMode {
    /// Bundled assets are treated like assets from other files.
    GroupByAssetType,
    /// Bundled assets are grouped by their asset bundle name.
    GroupByBundleName,
    /// Bundled assets are exported without grouping.
    DirectExport,
}

/// Audio export format in `ExportSettings`; default `Default` (source compression, usually OGG).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AudioExportFormat {
    /// YAML asset plus resource file; the safe fallback.
    Yaml,
    /// Native format, usually FSB, which the Unity Editor cannot use.
    Native,
    /// Source compression; the recommended option.
    Default,
    /// Convert to WAV when possible.
    PreferWav,
}

/// Image export format in `ExportSettings`; default `Png`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ImageExportFormat {
    Bmp,
    Exr,
    Hdr,
    Jpeg,
    Png,
    Tga,
}

impl ImageExportFormat {
    /// `ImageExportFormatExtensions.GetFileExtension`.
    pub const fn file_extension(self) -> &'static str {
        match self {
            Self::Bmp => "bmp",
            Self::Exr => "exr",
            Self::Hdr => "hdr",
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Tga => "tga",
        }
    }

    /// `ImageExportFormatExtensions.TryGetFromExtension`, including `jpg` for JPEG.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "bmp" => Some(Self::Bmp),
            "exr" => Some(Self::Exr),
            "hdr" => Some(Self::Hdr),
            "jpeg" | "jpg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "tga" => Some(Self::Tga),
            _ => None,
        }
    }

    /// `GetTextureExtension`: the original extension wins when preferred and recognized,
    /// otherwise the selected format's extension is used.
    pub fn texture_extension(
        self,
        prefer_original_texture_extension: bool,
        best_extension: Option<&str>,
    ) -> String {
        if prefer_original_texture_extension {
            if let Some(original) = best_extension {
                if Self::from_extension(original).is_some() {
                    return original.to_owned();
                }
            }
        }
        self.file_extension().to_owned()
    }
}

/// Lightmap texture export format in `ExportSettings`; default `Yaml`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LightmapTextureExportFormat {
    /// Use `ImageExportFormat::Exr`.
    Exr,
    /// Use `ExportSettings::ImageExportFormat`.
    Image,
    /// The internal Unity format.
    Yaml,
}

/// MonoScript export mode in `ExportSettings`; default `Hybrid`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScriptExportMode {
    /// ILSpy-decompiled C# scripts.
    Decompiled,
    /// Special assemblies decompiled, others saved as DLLs.
    Hybrid,
    /// Assemblies saved as DLLs with compatible renamed names.
    DllExportWithRenaming,
    /// Assemblies saved as compiled DLLs without renaming.
    DllExportWithoutRenaming,
}

/// Shader export mode in `ExportSettings`; default `Dummy`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShaderExportMode {
    /// Dummy shaders that compile in the editor.
    Dummy,
    /// YAML assets viewable in the editor.
    Yaml,
    /// Decompiled HLSL.
    Decompile,
}

/// Sprite export mode in `ExportSettings`; default `Yaml`, the only metadata-precise mode.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpriteExportMode {
    Yaml,
    Native,
    Texture2D,
}

/// Text asset export mode in `ExportSettings`; default `Parse`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextExportMode {
    Bytes,
    Txt,
    Parse,
}

/// Terrain export mode: declared but consumed nowhere outside a GUI dropdown.
///
/// No `ExportSettings` field holds it and no exporter or processor reads it, so it cannot
/// affect an export. It is recorded here so the audit does not silently drop it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TerrainExportMode {
    /// The only mode Unity can import for terrains.
    Yaml,
    /// Terrain data converted to a mesh Unity cannot import.
    Mesh,
    /// A height heatmap for visual representation only.
    Heatmap,
}

/// Snapshot-pinned `ScriptLanguageVersion` automatic discriminants.
///
/// Named C# variants mirror the external ILSpy `LanguageVersion` values and the automatic
/// selection needs full Unity-version ordering from the external primitives package, so only
/// `AutoExperimental = -2` and `AutoSafe = -1` (the default) are pinned here.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScriptLanguageSelection {
    AutoExperimental,
    AutoSafe,
}

impl ScriptLanguageSelection {
    /// The snapshot discriminant, matching the source enum values.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::AutoExperimental => -2,
            Self::AutoSafe => -1,
        }
    }
}

/// Default `ImportSettings` values from the source record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportDefaults {
    pub script_content_level: ScriptContentLevel,
    pub streaming_assets_mode: StreamingAssetsMode,
}

impl Default for ImportDefaults {
    fn default() -> Self {
        Self {
            script_content_level: ScriptContentLevel::Level2,
            streaming_assets_mode: StreamingAssetsMode::Extract,
        }
    }
}

/// Default `ProcessingSettings` values from the source record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessingDefaults {
    pub enable_prefab_outlining: bool,
    pub enable_static_mesh_separation: bool,
    pub enable_asset_deduplication: bool,
    pub remove_nullable_attributes: bool,
    pub publicize_assemblies: bool,
    pub bundled_assets_export_mode: BundledAssetsExportMode,
}

impl Default for ProcessingDefaults {
    fn default() -> Self {
        Self {
            enable_prefab_outlining: false,
            enable_static_mesh_separation: true,
            enable_asset_deduplication: false,
            remove_nullable_attributes: false,
            publicize_assemblies: false,
            bundled_assets_export_mode: BundledAssetsExportMode::DirectExport,
        }
    }
}

/// Default `ExportSettings` values from the source record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportDefaults {
    pub audio_export_format: AudioExportFormat,
    pub image_export_format: ImageExportFormat,
    pub lightmap_texture_export_format: LightmapTextureExportFormat,
    pub script_export_mode: ScriptExportMode,
    pub script_language_selection: ScriptLanguageSelection,
    pub script_types_fully_qualified: bool,
    pub shader_export_mode: ShaderExportMode,
    pub sprite_export_mode: SpriteExportMode,
    pub text_export_mode: TextExportMode,
    pub export_unreadable_assets: bool,
    pub prefer_original_texture_extension: bool,
    pub save_settings_to_disk: bool,
    pub language_code: Option<String>,
}

impl Default for ExportDefaults {
    fn default() -> Self {
        Self {
            audio_export_format: AudioExportFormat::Default,
            image_export_format: ImageExportFormat::Png,
            lightmap_texture_export_format: LightmapTextureExportFormat::Yaml,
            script_export_mode: ScriptExportMode::Hybrid,
            script_language_selection: ScriptLanguageSelection::AutoSafe,
            script_types_fully_qualified: false,
            shader_export_mode: ShaderExportMode::Dummy,
            sprite_export_mode: SpriteExportMode::Yaml,
            text_export_mode: TextExportMode::Parse,
            export_unreadable_assets: false,
            prefer_original_texture_extension: true,
            save_settings_to_disk: false,
            language_code: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AudioExportFormat, BundledAssetsExportMode, ExportDefaults, ImageExportFormat,
        ImportDefaults, LightmapTextureExportFormat, ProcessingDefaults, ScriptContentLevel,
        ScriptExportMode, ScriptLanguageSelection, ShaderExportMode, SpriteExportMode,
        StreamingAssetsMode, TerrainExportMode, TextExportMode,
    };

    #[test]
    fn record_defaults_match_the_source_records() {
        assert_eq!(
            ImportDefaults::default(),
            ImportDefaults {
                script_content_level: ScriptContentLevel::Level2,
                streaming_assets_mode: StreamingAssetsMode::Extract,
            }
        );
        assert_eq!(
            ProcessingDefaults::default().bundled_assets_export_mode,
            BundledAssetsExportMode::DirectExport
        );
        assert!(ProcessingDefaults::default().enable_static_mesh_separation);
        assert!(!ProcessingDefaults::default().enable_prefab_outlining);
        assert!(!ProcessingDefaults::default().enable_asset_deduplication);
        assert!(!ProcessingDefaults::default().remove_nullable_attributes);
        assert!(!ProcessingDefaults::default().publicize_assemblies);
        let export = ExportDefaults::default();
        assert_eq!(export.audio_export_format, AudioExportFormat::Default);
        assert_eq!(export.image_export_format, ImageExportFormat::Png);
        assert_eq!(
            export.lightmap_texture_export_format,
            LightmapTextureExportFormat::Yaml
        );
        assert_eq!(export.script_export_mode, ScriptExportMode::Hybrid);
        assert_eq!(
            export.script_language_selection,
            ScriptLanguageSelection::AutoSafe
        );
        assert!(!export.script_types_fully_qualified);
        assert_eq!(export.shader_export_mode, ShaderExportMode::Dummy);
        assert_eq!(export.sprite_export_mode, SpriteExportMode::Yaml);
        assert_eq!(export.text_export_mode, TextExportMode::Parse);
        assert!(!export.export_unreadable_assets);
        assert!(export.prefer_original_texture_extension);
        assert!(!export.save_settings_to_disk);
        assert_eq!(export.language_code, None);
    }

    #[test]
    fn image_extensions_cover_every_format_with_jpg_recognized() {
        assert_eq!(ImageExportFormat::Bmp.file_extension(), "bmp");
        assert_eq!(ImageExportFormat::Exr.file_extension(), "exr");
        assert_eq!(ImageExportFormat::Hdr.file_extension(), "hdr");
        assert_eq!(ImageExportFormat::Jpeg.file_extension(), "jpeg");
        assert_eq!(ImageExportFormat::Png.file_extension(), "png");
        assert_eq!(ImageExportFormat::Tga.file_extension(), "tga");
        assert_eq!(
            ImageExportFormat::from_extension("jpg"),
            Some(ImageExportFormat::Jpeg)
        );
        assert_eq!(
            ImageExportFormat::from_extension("png"),
            Some(ImageExportFormat::Png)
        );
        assert_eq!(ImageExportFormat::from_extension("PNG"), None);
        assert_eq!(ImageExportFormat::from_extension(""), None);
    }

    #[test]
    fn texture_extension_prefers_recognized_originals_only() {
        // A recognized original extension wins when preferred.
        assert_eq!(
            ImageExportFormat::Png.texture_extension(true, Some("tga")),
            "tga"
        );
        // An unrecognized original falls back to the selected format.
        assert_eq!(
            ImageExportFormat::Png.texture_extension(true, Some("dds")),
            "png"
        );
        assert_eq!(ImageExportFormat::Png.texture_extension(true, None), "png");
        // Without preference the selected format always wins.
        assert_eq!(
            ImageExportFormat::Exr.texture_extension(false, Some("png")),
            "exr"
        );
    }

    #[test]
    fn pinned_language_discriminants_and_dead_terrain_mode_are_recorded() {
        assert_eq!(ScriptLanguageSelection::AutoExperimental.discriminant(), -2);
        assert_eq!(ScriptLanguageSelection::AutoSafe.discriminant(), -1);
        // TerrainExportMode exists so the audit keeps it; nothing consumes it.
        let _ = TerrainExportMode::Yaml;
        let _ = TerrainExportMode::Mesh;
        let _ = TerrainExportMode::Heatmap;
    }
}
