//! Product settings ported from the frozen baseline's configuration records.
//!
//! Three records drive import, processing, and export, all persisted as JSON singletons:
//!
//! - `ImportSettings`: `ScriptContentLevel` (default `Level2`), `StreamingAssetsMode` (default
//!   `Extract`) with the `IgnoreStreamingAssets` get/set shorthand, and the package-defaulted
//!   `DefaultVersion`/`TargetVersion` Unity versions. The shorthand feeds the
//!   game-structure collection and streaming-assets post-export skip flags caller-side;
//!   settings-file persistence keeps using `StreamingAssetsMode` itself, matching the
//!   baseline `JsonIgnore`. `CoreConfiguration.DisableScriptImport` (level `Level0` only)
//!   rides on the content level the same way for the scripting-backend skip flag.
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
//! carries the full shipped member list with discriminants, and the automatic selection
//! (`ToCSharpLanguageVersion`, `GetAutomaticCSharpLanguageVersion`) is implemented over
//! caller-supplied version facts; release-type ordering itself stays with the version type.

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

impl ScriptContentLevel {
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Level0 => 0,
            Self::Level1 => 1,
            Self::Level2 => 2,
            Self::Level3 => 3,
        }
    }

    /// Reverse of [`ScriptContentLevel::discriminant`]: unknown values stay unmapped instead
    /// of silently becoming a valid level.
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Level0),
            1 => Some(Self::Level1),
            2 => Some(Self::Level2),
            3 => Some(Self::Level3),
            _ => None,
        }
    }

    /// Whether this level disables script import.
    ///
    /// Mirrors `CoreConfiguration.DisableScriptImport`
    /// (`AssetRipper/Source/AssetRipper.Import/Configuration/CoreConfiguration.cs`, line
    /// 14): only `Level0` (scripts are not loaded) disables; every higher level keeps the
    /// scripting-backend and assembly-collection path enabled.
    pub const fn disables_script_import(self) -> bool {
        matches!(self, Self::Level0)
    }
}

/// How the StreamingAssets folder is handled; default `Extract`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StreamingAssetsMode {
    Ignore,
    Extract,
}

impl StreamingAssetsMode {
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Ignore => 0,
            Self::Extract => 1,
        }
    }

    /// Reverse of [`StreamingAssetsMode::discriminant`].
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Ignore),
            1 => Some(Self::Extract),
            _ => None,
        }
    }

    /// Whether this mode skips the StreamingAssets tree.
    ///
    /// Mirrors the `ImportSettings.IgnoreStreamingAssets` getter
    /// (`AssetRipper/Source/AssetRipper.Import/Configuration/ImportSettings.cs`, lines
    /// 17-24): only `Ignore` skips. The game-structure collection and the streaming-assets
    /// post-exporter read this decision through their caller-side skip flags.
    pub const fn skip_streaming_assets(self) -> bool {
        matches!(self, Self::Ignore)
    }
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

impl BundledAssetsExportMode {
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::GroupByAssetType => 0,
            Self::GroupByBundleName => 1,
            Self::DirectExport => 2,
        }
    }

    /// Reverse of [`BundledAssetsExportMode::discriminant`].
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::GroupByAssetType),
            1 => Some(Self::GroupByBundleName),
            2 => Some(Self::DirectExport),
            _ => None,
        }
    }
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

impl AudioExportFormat {
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Yaml => 0,
            Self::Native => 1,
            Self::Default => 2,
            Self::PreferWav => 3,
        }
    }

    /// Reverse of [`AudioExportFormat::discriminant`].
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Yaml),
            1 => Some(Self::Native),
            2 => Some(Self::Default),
            3 => Some(Self::PreferWav),
            _ => None,
        }
    }
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
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Bmp => 0,
            Self::Exr => 1,
            Self::Hdr => 2,
            Self::Jpeg => 3,
            Self::Png => 4,
            Self::Tga => 5,
        }
    }

    /// Reverse of [`ImageExportFormat::discriminant`].
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Bmp),
            1 => Some(Self::Exr),
            2 => Some(Self::Hdr),
            3 => Some(Self::Jpeg),
            4 => Some(Self::Png),
            5 => Some(Self::Tga),
            _ => None,
        }
    }
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

    /// `GetTextureExportFormat`: the original extension's format wins when preferred and
    /// recognized, otherwise the configured format is used.
    ///
    /// The asset's best extension (`OverrideExtension ?? OriginalExtension`) arrives as a
    /// caller fact; bundle-decoded assets carry no original path, so callers pass `None`.
    pub fn texture_export_format(
        prefer_original_texture_extension: bool,
        best_extension: Option<&str>,
        configured: Self,
    ) -> Self {
        if prefer_original_texture_extension {
            if let Some(original) = best_extension {
                if let Some(format) = Self::from_extension(original) {
                    return format;
                }
            }
        }
        configured
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

impl LightmapTextureExportFormat {
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Exr => 0,
            Self::Image => 1,
            Self::Yaml => 2,
        }
    }

    /// Reverse of [`LightmapTextureExportFormat::discriminant`].
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Exr),
            1 => Some(Self::Image),
            2 => Some(Self::Yaml),
            _ => None,
        }
    }
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

impl ScriptExportMode {
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Decompiled => 0,
            Self::Hybrid => 1,
            Self::DllExportWithRenaming => 2,
            Self::DllExportWithoutRenaming => 3,
        }
    }

    /// Reverse of [`ScriptExportMode::discriminant`].
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Decompiled),
            1 => Some(Self::Hybrid),
            2 => Some(Self::DllExportWithRenaming),
            3 => Some(Self::DllExportWithoutRenaming),
            _ => None,
        }
    }
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

impl ShaderExportMode {
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Dummy => 0,
            Self::Yaml => 1,
            Self::Decompile => 2,
        }
    }

    /// Reverse of [`ShaderExportMode::discriminant`].
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Dummy),
            1 => Some(Self::Yaml),
            2 => Some(Self::Decompile),
            _ => None,
        }
    }
}

/// Sprite export mode in `ExportSettings`; default `Yaml`, the only metadata-precise mode.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpriteExportMode {
    Yaml,
    Native,
    Texture2D,
}

impl SpriteExportMode {
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Yaml => 0,
            Self::Native => 1,
            Self::Texture2D => 2,
        }
    }

    /// Reverse of [`SpriteExportMode::discriminant`].
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Yaml),
            1 => Some(Self::Native),
            2 => Some(Self::Texture2D),
            _ => None,
        }
    }
}

/// Text asset export mode in `ExportSettings`; default `Parse`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextExportMode {
    Bytes,
    Txt,
    Parse,
}

impl TextExportMode {
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Bytes => 0,
            Self::Txt => 1,
            Self::Parse => 2,
        }
    }

    /// Reverse of [`TextExportMode::discriminant`].
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Bytes),
            1 => Some(Self::Txt),
            2 => Some(Self::Parse),
            _ => None,
        }
    }
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

impl TerrainExportMode {
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Yaml => 0,
            Self::Mesh => 1,
            Self::Heatmap => 2,
        }
    }
}

/// The C# language version for decompiled scripts (`ScriptLanguageVersion`); default `AutoSafe`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScriptLanguageVersion {
    CSharp1,
    CSharp2,
    CSharp3,
    CSharp4,
    CSharp5,
    CSharp6,
    CSharp7,
    CSharp7_1,
    CSharp7_2,
    CSharp7_3,
    CSharp8_0,
    CSharp9_0,
    CSharp10_0,
    CSharp11_0,
    CSharp12_0,
    CSharp13_0,
    CSharp14_0,
    CSharp15_0,
    Latest,
    AutoExperimental,
    AutoSafe,
}

impl ScriptLanguageVersion {
    /// The source enum discriminant, matching the shipped enum values.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::CSharp1 => 1,
            Self::CSharp2 => 2,
            Self::CSharp3 => 3,
            Self::CSharp4 => 4,
            Self::CSharp5 => 5,
            Self::CSharp6 => 6,
            Self::CSharp7 => 7,
            Self::CSharp7_1 => 701,
            Self::CSharp7_2 => 702,
            Self::CSharp7_3 => 703,
            Self::CSharp8_0 => 800,
            Self::CSharp9_0 => 900,
            Self::CSharp10_0 => 1000,
            Self::CSharp11_0 => 1100,
            Self::CSharp12_0 => 1200,
            Self::CSharp13_0 => 1300,
            Self::CSharp14_0 => 1400,
            Self::CSharp15_0 => 1500,
            Self::Latest => 2_147_483_647,
            Self::AutoExperimental => -2,
            Self::AutoSafe => -1,
        }
    }

    /// Reverse of [`ScriptLanguageVersion::discriminant`].
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            1 => Some(Self::CSharp1),
            2 => Some(Self::CSharp2),
            3 => Some(Self::CSharp3),
            4 => Some(Self::CSharp4),
            5 => Some(Self::CSharp5),
            6 => Some(Self::CSharp6),
            7 => Some(Self::CSharp7),
            701 => Some(Self::CSharp7_1),
            702 => Some(Self::CSharp7_2),
            703 => Some(Self::CSharp7_3),
            800 => Some(Self::CSharp8_0),
            900 => Some(Self::CSharp9_0),
            1000 => Some(Self::CSharp10_0),
            1100 => Some(Self::CSharp11_0),
            1200 => Some(Self::CSharp12_0),
            1300 => Some(Self::CSharp13_0),
            1400 => Some(Self::CSharp14_0),
            1500 => Some(Self::CSharp15_0),
            2_147_483_647 => Some(Self::Latest),
            -2 => Some(Self::AutoExperimental),
            -1 => Some(Self::AutoSafe),
            _ => None,
        }
    }

    /// `ToCSharpLanguageVersion(unityVersion)`: the automatic members resolve against
    /// caller-supplied version facts; concrete members pass through like the source cast.
    pub const fn resolve_with_version(self, facts: AutomaticLanguageVersionFacts) -> Self {
        match self {
            Self::AutoExperimental => automatic_script_language_version(true, facts),
            Self::AutoSafe => automatic_script_language_version(false, facts),
            concrete => concrete,
        }
    }

    /// `ToCSharpLanguageVersion()` without a Unity version: both automatic members become
    /// `Latest`; concrete members pass through.
    pub const fn resolve_without_version(self) -> Self {
        match self {
            Self::AutoExperimental | Self::AutoSafe => Self::Latest,
            concrete => concrete,
        }
    }
}

/// Caller facts for `GetAutomaticCSharpLanguageVersion`: whether the Unity version reaches
/// each threshold, in source evaluation order. The release-type-sensitive gates (notably
/// `HasCSharp9Support`, which needs alpha/beta ordering) arrive pre-evaluated because
/// version ordering lives outside the settings records.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AutomaticLanguageVersionFacts {
    /// `HasCSharp9Support`: at least 2022.1.0a3, or at least 2021.2.0b6 within major 2021.
    pub has_csharp9_support: bool,
    /// At least 2020.2.
    pub at_least_2020_2: bool,
    /// At least 2019.2.
    pub at_least_2019_2: bool,
    /// At least 2018.3.
    pub at_least_2018_3: bool,
    /// At least 2017.1.
    pub at_least_2017_1: bool,
}

/// `GetAutomaticCSharpLanguageVersion(unityVersion, experimental)`: C# 9 on supported
/// versions, then 8.0 from 2020.2 and 7.3 from 2019.2 (or from 2018.3 in experimental
/// mode); experimental mode additionally reaches 6 from 2017.1; anything older falls
/// back to C# 4.
pub const fn automatic_script_language_version(
    experimental: bool,
    facts: AutomaticLanguageVersionFacts,
) -> ScriptLanguageVersion {
    if facts.has_csharp9_support {
        ScriptLanguageVersion::CSharp9_0
    } else if facts.at_least_2020_2 {
        ScriptLanguageVersion::CSharp8_0
    } else if facts.at_least_2019_2 || (experimental && facts.at_least_2018_3) {
        ScriptLanguageVersion::CSharp7_3
    } else if experimental && facts.at_least_2017_1 {
        ScriptLanguageVersion::CSharp6
    } else {
        ScriptLanguageVersion::CSharp4
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

impl ImportDefaults {
    /// `ImportSettings.IgnoreStreamingAssets` getter: only `Ignore` mode skips.
    pub const fn ignore_streaming_assets(self) -> bool {
        self.streaming_assets_mode.skip_streaming_assets()
    }

    /// `CoreConfiguration.DisableScriptImport` over these defaults: only a `Level0`
    /// content level disables script import. Callers feed the result to the
    /// scripting-backend and assembly-collection skip flags.
    pub const fn disable_script_import(self) -> bool {
        self.script_content_level.disables_script_import()
    }

    /// `ImportSettings.IgnoreStreamingAssets` setter: `true` selects `Ignore`,
    /// `false` unconditionally reselects `Extract`, like the source. The property
    /// carries `JsonIgnore` in the baseline, so settings-file persistence keeps
    /// reading and writing `StreamingAssetsMode` itself.
    pub fn set_ignore_streaming_assets(&mut self, ignore: bool) {
        self.streaming_assets_mode = if ignore {
            StreamingAssetsMode::Ignore
        } else {
            StreamingAssetsMode::Extract
        };
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
    pub script_language_version: ScriptLanguageVersion,
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
            script_language_version: ScriptLanguageVersion::AutoSafe,
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
        ScriptExportMode, ScriptLanguageVersion, ShaderExportMode, SpriteExportMode,
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
            export.script_language_version,
            ScriptLanguageVersion::AutoSafe
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
    fn texture_export_format_prefers_recognized_originals_only() {
        use ImageExportFormat as Format;

        // A recognized original extension's format wins when preferred.
        assert_eq!(
            Format::texture_export_format(true, Some("tga"), Format::Png),
            Format::Tga
        );
        assert_eq!(
            Format::texture_export_format(true, Some("jpg"), Format::Png),
            Format::Jpeg
        );
        // An unrecognized original or no original falls back to the configured format.
        assert_eq!(
            Format::texture_export_format(true, Some("dds"), Format::Png),
            Format::Png
        );
        assert_eq!(
            Format::texture_export_format(true, None, Format::Bmp),
            Format::Bmp
        );
        // Without preference the configured format always wins.
        assert_eq!(
            Format::texture_export_format(false, Some("png"), Format::Exr),
            Format::Exr
        );
    }

    #[test]
    fn automatic_language_selection_follows_the_version_thresholds() {
        use super::{automatic_script_language_version, AutomaticLanguageVersionFacts};
        use ScriptLanguageVersion::{
            AutoExperimental, AutoSafe, CSharp11_0, CSharp4, CSharp6, CSharp7_3, CSharp8_0,
            CSharp9_0, Latest,
        };
        fn facts(
            has_csharp9_support: bool,
            at_least_2020_2: bool,
            at_least_2019_2: bool,
            at_least_2018_3: bool,
            at_least_2017_1: bool,
        ) -> AutomaticLanguageVersionFacts {
            AutomaticLanguageVersionFacts {
                has_csharp9_support,
                at_least_2020_2,
                at_least_2019_2,
                at_least_2018_3,
                at_least_2017_1,
            }
        }
        let none = facts(false, false, false, false, false);
        // Thresholds apply in source order; higher support wins regardless of mode.
        assert_eq!(
            automatic_script_language_version(false, facts(true, true, true, true, true)),
            CSharp9_0
        );
        assert_eq!(
            automatic_script_language_version(true, facts(false, true, true, true, true)),
            CSharp8_0
        );
        assert_eq!(
            automatic_script_language_version(false, facts(false, false, true, true, true)),
            CSharp7_3
        );
        // The 2018.3 and 2017.1 branches need experimental mode.
        assert_eq!(
            automatic_script_language_version(true, facts(false, false, false, true, true)),
            CSharp7_3
        );
        assert_eq!(
            automatic_script_language_version(true, facts(false, false, false, false, true)),
            CSharp6
        );
        assert_eq!(
            automatic_script_language_version(false, facts(false, false, false, true, true)),
            CSharp4
        );
        assert_eq!(
            automatic_script_language_version(false, facts(false, false, false, false, true)),
            CSharp4
        );
        assert_eq!(automatic_script_language_version(true, none), CSharp4);
        assert_eq!(automatic_script_language_version(false, none), CSharp4);
        // The member-level dispatch picks the experimental flag from the variant.
        assert_eq!(
            AutoSafe.resolve_with_version(facts(false, false, false, true, true)),
            CSharp4
        );
        assert_eq!(
            AutoExperimental.resolve_with_version(facts(false, false, false, true, true)),
            CSharp7_3
        );
        assert_eq!(
            AutoSafe.resolve_with_version(facts(false, true, false, false, false)),
            CSharp8_0
        );
        // Concrete members pass through untouched on both entry points.
        assert_eq!(CSharp11_0.resolve_with_version(none), CSharp11_0);
        assert_eq!(CSharp11_0.resolve_without_version(), CSharp11_0);
        // Without a version, both automatic members become Latest.
        assert_eq!(AutoSafe.resolve_without_version(), Latest);
        assert_eq!(AutoExperimental.resolve_without_version(), Latest);
    }

    #[test]
    fn pinned_language_discriminants_and_dead_terrain_mode_are_recorded() {
        assert_eq!(ScriptLanguageVersion::AutoExperimental.discriminant(), -2);
        assert_eq!(ScriptLanguageVersion::AutoSafe.discriminant(), -1);
        assert_eq!(ScriptLanguageVersion::Latest.discriminant(), 2_147_483_647);
        assert_eq!(ScriptLanguageVersion::CSharp7_3.discriminant(), 703);
        // TerrainExportMode exists so the audit keeps it; nothing consumes it.
        let _ = TerrainExportMode::Yaml;
        let _ = TerrainExportMode::Mesh;
        let _ = TerrainExportMode::Heatmap;
    }

    #[test]
    fn disable_script_import_follows_the_source_computed_property() {
        // Only `Level0` (scripts are not loaded) disables; higher levels keep the
        // scripting path enabled.
        assert!(ScriptContentLevel::Level0.disables_script_import());
        assert!(!ScriptContentLevel::Level1.disables_script_import());
        assert!(!ScriptContentLevel::Level2.disables_script_import());
        assert!(!ScriptContentLevel::Level3.disables_script_import());
        // The defaults read goes through the content level like
        // `CoreConfiguration.DisableScriptImport` reads `ImportSettings`.
        assert!(!ImportDefaults::default().disable_script_import());
        for level in [
            ScriptContentLevel::Level0,
            ScriptContentLevel::Level1,
            ScriptContentLevel::Level2,
            ScriptContentLevel::Level3,
        ] {
            let defaults = ImportDefaults {
                script_content_level: level,
                ..ImportDefaults::default()
            };
            assert_eq!(
                defaults.disable_script_import(),
                level == ScriptContentLevel::Level0
            );
        }
    }

    #[test]
    fn ignore_streaming_assets_shorthand_follows_the_source_property() {
        use super::{ImportDefaults, StreamingAssetsMode};
        // The getter reads the mode: only `Ignore` skips.
        assert!(StreamingAssetsMode::Ignore.skip_streaming_assets());
        assert!(!StreamingAssetsMode::Extract.skip_streaming_assets());
        assert!(!ImportDefaults::default().ignore_streaming_assets());
        // The setter maps back onto the mode, unconditionally reselecting
        // `Extract` on `false`, like the source.
        for start in [StreamingAssetsMode::Ignore, StreamingAssetsMode::Extract] {
            let mut defaults = ImportDefaults {
                streaming_assets_mode: start,
                ..ImportDefaults::default()
            };
            defaults.set_ignore_streaming_assets(true);
            assert_eq!(defaults.streaming_assets_mode, StreamingAssetsMode::Ignore);
            assert!(defaults.ignore_streaming_assets());
            defaults.set_ignore_streaming_assets(false);
            assert_eq!(defaults.streaming_assets_mode, StreamingAssetsMode::Extract);
            assert!(!defaults.ignore_streaming_assets());
        }
    }

    /// Differential against the shipped configuration assemblies
    /// (`Temp/opencode/settingprobe`, SDK 10.0.401): every enum discriminant in
    /// `settings_fixture.txt` plus the `ExportSettings`/`ImportSettings`/
    /// `ProcessingSettings` default dumps. The version literals (`0.0.0a0`) pin the
    /// default Unity version rendering for future version work; the Rust version
    /// type does not model defaults yet.
    #[test]
    fn enum_discriminants_and_record_defaults_match_the_reference_runtime() {
        let fixture = include_str!("settings_fixture.txt");
        let mut enums = 0;
        let mut defs: Vec<(String, String, String)> = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('|');
            match parts.next() {
                Some("ENUM") => {
                    let owner = parts.next().expect("a type");
                    let member = parts.next().expect("a member");
                    let discriminant: i32 = parts.next().expect("a discriminant").parse().unwrap();
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(discriminant_of(owner, member), discriminant, "{line}");
                    enums += 1;
                }
                Some("DEF") => {
                    let record = parts.next().expect("a record").to_owned();
                    let property = parts.next().expect("a property").to_owned();
                    let value = parts.next().expect("a value").to_owned();
                    assert!(parts.next().is_none(), "{line}");
                    defs.push((record, property, value));
                }
                other => panic!("unexpected row kind {other:?}"),
            }
        }
        assert_eq!(enums, 59, "all probed enum rows are asserted");
        assert_default_dump(&defs);
    }

    fn discriminant_of(owner: &str, member: &str) -> i32 {
        match owner {
            "AudioExportFormat" => match member {
                "Yaml" => AudioExportFormat::Yaml.discriminant(),
                "Native" => AudioExportFormat::Native.discriminant(),
                "Default" => AudioExportFormat::Default.discriminant(),
                "PreferWav" => AudioExportFormat::PreferWav.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            "ImageExportFormat" => match member {
                "Bmp" => ImageExportFormat::Bmp.discriminant(),
                "Exr" => ImageExportFormat::Exr.discriminant(),
                "Hdr" => ImageExportFormat::Hdr.discriminant(),
                "Jpeg" => ImageExportFormat::Jpeg.discriminant(),
                "Png" => ImageExportFormat::Png.discriminant(),
                "Tga" => ImageExportFormat::Tga.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            "LightmapTextureExportFormat" => match member {
                "Exr" => LightmapTextureExportFormat::Exr.discriminant(),
                "Image" => LightmapTextureExportFormat::Image.discriminant(),
                "Yaml" => LightmapTextureExportFormat::Yaml.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            "ScriptExportMode" => match member {
                "Decompiled" => ScriptExportMode::Decompiled.discriminant(),
                "Hybrid" => ScriptExportMode::Hybrid.discriminant(),
                "DllExportWithRenaming" => ScriptExportMode::DllExportWithRenaming.discriminant(),
                "DllExportWithoutRenaming" => {
                    ScriptExportMode::DllExportWithoutRenaming.discriminant()
                }
                _ => panic!("unknown member {member}"),
            },
            "ScriptLanguageVersion" => match member {
                "CSharp1" => ScriptLanguageVersion::CSharp1.discriminant(),
                "CSharp2" => ScriptLanguageVersion::CSharp2.discriminant(),
                "CSharp3" => ScriptLanguageVersion::CSharp3.discriminant(),
                "CSharp4" => ScriptLanguageVersion::CSharp4.discriminant(),
                "CSharp5" => ScriptLanguageVersion::CSharp5.discriminant(),
                "CSharp6" => ScriptLanguageVersion::CSharp6.discriminant(),
                "CSharp7" => ScriptLanguageVersion::CSharp7.discriminant(),
                "CSharp7_1" => ScriptLanguageVersion::CSharp7_1.discriminant(),
                "CSharp7_2" => ScriptLanguageVersion::CSharp7_2.discriminant(),
                "CSharp7_3" => ScriptLanguageVersion::CSharp7_3.discriminant(),
                "CSharp8_0" => ScriptLanguageVersion::CSharp8_0.discriminant(),
                "CSharp9_0" => ScriptLanguageVersion::CSharp9_0.discriminant(),
                "CSharp10_0" => ScriptLanguageVersion::CSharp10_0.discriminant(),
                "CSharp11_0" => ScriptLanguageVersion::CSharp11_0.discriminant(),
                "CSharp12_0" => ScriptLanguageVersion::CSharp12_0.discriminant(),
                "CSharp13_0" => ScriptLanguageVersion::CSharp13_0.discriminant(),
                "CSharp14_0" => ScriptLanguageVersion::CSharp14_0.discriminant(),
                "CSharp15_0" => ScriptLanguageVersion::CSharp15_0.discriminant(),
                "Latest" => ScriptLanguageVersion::Latest.discriminant(),
                "AutoExperimental" => ScriptLanguageVersion::AutoExperimental.discriminant(),
                "AutoSafe" => ScriptLanguageVersion::AutoSafe.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            "ShaderExportMode" => match member {
                "Dummy" => ShaderExportMode::Dummy.discriminant(),
                "Yaml" => ShaderExportMode::Yaml.discriminant(),
                "Decompile" => ShaderExportMode::Decompile.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            "SpriteExportMode" => match member {
                "Yaml" => SpriteExportMode::Yaml.discriminant(),
                "Native" => SpriteExportMode::Native.discriminant(),
                "Texture2D" => SpriteExportMode::Texture2D.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            "TextExportMode" => match member {
                "Bytes" => TextExportMode::Bytes.discriminant(),
                "Txt" => TextExportMode::Txt.discriminant(),
                "Parse" => TextExportMode::Parse.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            "TerrainExportMode" => match member {
                "Yaml" => TerrainExportMode::Yaml.discriminant(),
                "Mesh" => TerrainExportMode::Mesh.discriminant(),
                "Heatmap" => TerrainExportMode::Heatmap.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            "ScriptContentLevel" => match member {
                "Level0" => ScriptContentLevel::Level0.discriminant(),
                "Level1" => ScriptContentLevel::Level1.discriminant(),
                "Level2" => ScriptContentLevel::Level2.discriminant(),
                "Level3" => ScriptContentLevel::Level3.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            "StreamingAssetsMode" => match member {
                "Ignore" => StreamingAssetsMode::Ignore.discriminant(),
                "Extract" => StreamingAssetsMode::Extract.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            "BundledAssetsExportMode" => match member {
                "GroupByAssetType" => BundledAssetsExportMode::GroupByAssetType.discriminant(),
                "GroupByBundleName" => BundledAssetsExportMode::GroupByBundleName.discriminant(),
                "DirectExport" => BundledAssetsExportMode::DirectExport.discriminant(),
                _ => panic!("unknown member {member}"),
            },
            _ => panic!("unknown enum {owner}"),
        }
    }

    fn assert_default_dump(defs: &[(String, String, String)]) {
        /// .NET renders booleans capitalized; Rust does not.
        fn dotnet_bool(value: bool) -> &'static str {
            if value {
                "True"
            } else {
                "False"
            }
        }
        let lookup = |record: &str, property: &str| -> &str {
            defs.iter()
                .find(|(r, p, _)| r == record && p == property)
                .map(|(_, _, v)| v.as_str())
                .expect("a probed default")
        };
        let export = ExportDefaults::default();
        assert_eq!(
            lookup("ExportSettings", "AudioExportFormat"),
            format!(
                "{:?}={}",
                export.audio_export_format,
                export.audio_export_format.discriminant()
            )
        );
        assert_eq!(
            lookup("ExportSettings", "ImageExportFormat"),
            format!(
                "{:?}={}",
                export.image_export_format,
                export.image_export_format.discriminant()
            )
        );
        assert_eq!(
            lookup("ExportSettings", "LightmapTextureExportFormat"),
            format!(
                "{:?}={}",
                export.lightmap_texture_export_format,
                export.lightmap_texture_export_format.discriminant()
            )
        );
        assert_eq!(
            lookup("ExportSettings", "ScriptExportMode"),
            format!(
                "{:?}={}",
                export.script_export_mode,
                export.script_export_mode.discriminant()
            )
        );
        assert_eq!(
            lookup("ExportSettings", "ScriptLanguageVersion"),
            format!(
                "{:?}={}",
                export.script_language_version,
                export.script_language_version.discriminant()
            )
        );
        assert_eq!(
            lookup("ExportSettings", "ScriptTypesFullyQualified"),
            dotnet_bool(export.script_types_fully_qualified)
        );
        assert_eq!(
            lookup("ExportSettings", "ShaderExportMode"),
            format!(
                "{:?}={}",
                export.shader_export_mode,
                export.shader_export_mode.discriminant()
            )
        );
        assert_eq!(
            lookup("ExportSettings", "SpriteExportMode"),
            format!(
                "{:?}={}",
                export.sprite_export_mode,
                export.sprite_export_mode.discriminant()
            )
        );
        assert_eq!(
            lookup("ExportSettings", "TextExportMode"),
            format!(
                "{:?}={}",
                export.text_export_mode,
                export.text_export_mode.discriminant()
            )
        );
        assert_eq!(
            lookup("ExportSettings", "ExportUnreadableAssets"),
            dotnet_bool(export.export_unreadable_assets)
        );
        assert_eq!(
            lookup("ExportSettings", "PreferOriginalTextureExtension"),
            dotnet_bool(export.prefer_original_texture_extension)
        );
        assert_eq!(
            lookup("ExportSettings", "SaveSettingsToDisk"),
            dotnet_bool(export.save_settings_to_disk)
        );
        assert_eq!(
            lookup("ExportSettings", "LanguageCode"),
            export.language_code.as_deref().unwrap_or("<null>")
        );
        let import = ImportDefaults::default();
        assert_eq!(
            lookup("ImportSettings", "ScriptContentLevel"),
            format!(
                "{:?}={}",
                import.script_content_level,
                import.script_content_level.discriminant()
            )
        );
        assert_eq!(
            lookup("ImportSettings", "StreamingAssetsMode"),
            format!(
                "{:?}={}",
                import.streaming_assets_mode,
                import.streaming_assets_mode.discriminant()
            )
        );
        assert_eq!(lookup("ImportSettings", "IgnoreStreamingAssets"), "False");
        // The default Unity versions render as 0.0.0a0; the Rust version type
        // does not model record defaults yet, so the probe literal is pinned here.
        assert_eq!(lookup("ImportSettings", "DefaultVersion"), "0.0.0a0");
        assert_eq!(lookup("ImportSettings", "TargetVersion"), "0.0.0a0");
        let processing = ProcessingDefaults::default();
        assert_eq!(
            lookup("ProcessingSettings", "BundledAssetsExportMode"),
            format!(
                "{:?}={}",
                processing.bundled_assets_export_mode,
                processing.bundled_assets_export_mode.discriminant()
            )
        );
        assert_eq!(
            lookup("ProcessingSettings", "EnableAssetDeduplication"),
            dotnet_bool(processing.enable_asset_deduplication)
        );
        assert_eq!(
            lookup("ProcessingSettings", "EnablePrefabOutlining"),
            dotnet_bool(processing.enable_prefab_outlining)
        );
        assert_eq!(
            lookup("ProcessingSettings", "EnableStaticMeshSeparation"),
            dotnet_bool(processing.enable_static_mesh_separation)
        );
        assert_eq!(
            lookup("ProcessingSettings", "PublicizeAssemblies"),
            dotnet_bool(processing.publicize_assemblies)
        );
        assert_eq!(
            lookup("ProcessingSettings", "RemoveNullableAttributes"),
            dotnet_bool(processing.remove_nullable_attributes)
        );
    }
}
