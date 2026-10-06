//! `AssetRipper.Settings.json` round-trip for the three configuration records.
//!
//! The frozen baseline persists `SerializedSettings(Import, Processing, Export)` beside the
//! executable with `System.Text.Json` source generation (`WriteIndented = true`, PascalCase
//! property names, enums as numbers, Unity versions as strings, `LanguageCode` as
//! string-or-null, no BOM, CRLF newlines, no trailing newline). The byte shape below follows a
//! probe of the shipped `AssetRipper.Export.dll` (`SerializedSettings.Save`, SDK 10.0.401)
//! whose two outputs are committed as `settings_file_defaults.json` and
//! `settings_file_modified.json`.
//!
//! Two deliberate boundaries, both documented in the ledger row:
//!
//! - Unity versions travel as opaque [`VersionText`] strings. Version ordering and rendering
//!   live in the external primitives package, so no semantic validation happens here beyond
//!   non-empty bounded ASCII; the pinned default rendering is [`DEFAULT_UNITY_VERSION_TEXT`].
//! - String escaping is minimal but valid (`"`, `\`, and C0 controls; STJ's extra
//!   HTML-sensitive escapes are not emitted). Within the schema domain (version and locale
//!   identifiers) the writer is byte-exact with the probe; outside it the output stays
//!   STJ-readable without being byte-identical.
//! - Unknown enum discriminants are explicit [`SettingsFileError::UnknownEnumValue`] errors.
//!   The source would accept and cast them, but a settings file naming no known mode must
//!   fail fast rather than corrupt an export.
//!
//! The reader mirrors the source load behavior otherwise: unknown properties are ignored,
//! missing properties keep record defaults, duplicate keys resolve last-wins, a leading
//! UTF-8 BOM is stripped like `File.ReadAllText` strips it before deserialization, and
//! both CRLF and LF inputs are accepted (committed fixtures check out as LF under `eol=lf`).

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::asset_path::{join_path, HostPathPlatform};
use super::settings::{
    AudioExportFormat, BundledAssetsExportMode, ExportDefaults, ImageExportFormat, ImportDefaults,
    LightmapTextureExportFormat, ProcessingDefaults, ScriptContentLevel, ScriptExportMode,
    ScriptLanguageVersion, ShaderExportMode, SpriteExportMode, StreamingAssetsMode, TextExportMode,
};

/// The settings file name beside the executable (`SerializedSettings.DefaultFileName`).
pub const SETTINGS_FILE_NAME: &str = "AssetRipper.Settings.json";

/// The default Unity version rendering (`0.0.0a0`), pinned by the settings differential.
pub const DEFAULT_UNITY_VERSION_TEXT: &str = "0.0.0a0";

/// Bounds applied to settings-file inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsFileLimits {
    /// Maximum settings-file bytes accepted by the reader.
    pub max_file_bytes: usize,
    /// Maximum decoded bytes for any single string.
    pub max_string_bytes: usize,
    /// Maximum JSON nesting depth accepted by the reader.
    pub max_nesting_depth: usize,
}

impl Default for SettingsFileLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 1024 * 1024,
            max_string_bytes: 64 * 1024,
            max_nesting_depth: 32,
        }
    }
}

/// Error returned when a settings file cannot be written or read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsFileError {
    /// The input exceeds the configured bound.
    InputTooLong { byte_count: usize, limit: usize },
    /// The input is not settings-file JSON; the path names the containing value.
    InvalidJson { path: String, reason: &'static str },
    /// An enum discriminant names no known mode; the path names the property.
    UnknownEnumValue { path: String, value: i32 },
    /// A bounded allocation could not be reserved.
    AllocationFailed { resource: &'static str },
}

impl Display for SettingsFileError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLong { byte_count, limit } => write!(
                formatter,
                "settings file uses {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::InvalidJson { path, reason } => {
                write!(formatter, "invalid settings JSON at {path}: {reason}")
            }
            Self::UnknownEnumValue { path, value } => {
                write!(formatter, "unknown settings enum value {value} at {path}")
            }
            Self::AllocationFailed { resource } => {
                write!(formatter, "could not allocate {resource}")
            }
        }
    }
}

impl Error for SettingsFileError {}

/// An opaque Unity version rendering (`DefaultVersion`, `TargetVersion`).
///
/// Only the text travels here; ordering and parsing stay in the external package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionText(String);

impl VersionText {
    /// Accept a non-empty bounded ASCII rendering.
    pub fn new(text: &str, limits: SettingsFileLimits) -> Result<Self, SettingsFileError> {
        if text.is_empty() || text.len() > limits.max_string_bytes || !text.is_ascii() {
            return Err(SettingsFileError::InvalidJson {
                path: "version".to_owned(),
                reason: "Unity version renderings must be non-empty bounded ASCII",
            });
        }
        Ok(Self(text.to_owned()))
    }

    /// The default rendering written for fresh records.
    pub fn default_rendering() -> Self {
        Self(DEFAULT_UNITY_VERSION_TEXT.to_owned())
    }

    /// The rendering text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `ImportSettings` as stored in the settings file, plus opaque version renderings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportSettingsFile {
    pub script_content_level: ScriptContentLevel,
    pub streaming_assets_mode: StreamingAssetsMode,
    pub default_version: VersionText,
    pub target_version: VersionText,
}

impl ImportSettingsFile {
    /// Fresh records with default version renderings.
    pub fn from_defaults(defaults: ImportDefaults) -> Self {
        Self {
            script_content_level: defaults.script_content_level,
            streaming_assets_mode: defaults.streaming_assets_mode,
            default_version: VersionText::default_rendering(),
            target_version: VersionText::default_rendering(),
        }
    }
}

/// `ProcessingSettings` as stored in the settings file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessingSettingsFile {
    pub enable_prefab_outlining: bool,
    pub enable_static_mesh_separation: bool,
    pub enable_asset_deduplication: bool,
    pub remove_nullable_attributes: bool,
    pub publicize_assemblies: bool,
    pub bundled_assets_export_mode: BundledAssetsExportMode,
}

impl From<ProcessingDefaults> for ProcessingSettingsFile {
    fn from(defaults: ProcessingDefaults) -> Self {
        Self {
            enable_prefab_outlining: defaults.enable_prefab_outlining,
            enable_static_mesh_separation: defaults.enable_static_mesh_separation,
            enable_asset_deduplication: defaults.enable_asset_deduplication,
            remove_nullable_attributes: defaults.remove_nullable_attributes,
            publicize_assemblies: defaults.publicize_assemblies,
            bundled_assets_export_mode: defaults.bundled_assets_export_mode,
        }
    }
}

/// `ExportSettings` as stored in the settings file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportSettingsFile {
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

impl From<ExportDefaults> for ExportSettingsFile {
    fn from(defaults: ExportDefaults) -> Self {
        Self {
            audio_export_format: defaults.audio_export_format,
            image_export_format: defaults.image_export_format,
            lightmap_texture_export_format: defaults.lightmap_texture_export_format,
            script_export_mode: defaults.script_export_mode,
            script_language_version: defaults.script_language_version,
            script_types_fully_qualified: defaults.script_types_fully_qualified,
            shader_export_mode: defaults.shader_export_mode,
            sprite_export_mode: defaults.sprite_export_mode,
            text_export_mode: defaults.text_export_mode,
            export_unreadable_assets: defaults.export_unreadable_assets,
            prefer_original_texture_extension: defaults.prefer_original_texture_extension,
            save_settings_to_disk: defaults.save_settings_to_disk,
            language_code: defaults.language_code,
        }
    }
}

/// `SerializedSettings` as stored in `AssetRipper.Settings.json`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedSettingsFile {
    pub import: ImportSettingsFile,
    pub processing: ProcessingSettingsFile,
    pub export: ExportSettingsFile,
}

impl SerializedSettingsFile {
    /// Fresh records with default version renderings.
    pub fn from_defaults(
        import: ImportDefaults,
        processing: ProcessingDefaults,
        export: ExportDefaults,
    ) -> Self {
        Self {
            import: ImportSettingsFile::from_defaults(import),
            processing: ProcessingSettingsFile::from(processing),
            export: ExportSettingsFile::from(export),
        }
    }
}

/// Serialize settings exactly as `SerializedSettings.Save` writes them: two-space indented,
/// CRLF newlines, declaration order, no BOM, no trailing newline.
pub fn write_settings_file(settings: &SerializedSettingsFile) -> Result<String, SettingsFileError> {
    let mut output = String::new();
    output.push_str("{\r\n");
    write_section(
        &mut output,
        "Import",
        &[
            (
                "ScriptContentLevel",
                JsonScalar::Integer(settings.import.script_content_level.discriminant()),
            ),
            (
                "StreamingAssetsMode",
                JsonScalar::Integer(settings.import.streaming_assets_mode.discriminant()),
            ),
            (
                "DefaultVersion",
                JsonScalar::Text(settings.import.default_version.as_str()),
            ),
            (
                "TargetVersion",
                JsonScalar::Text(settings.import.target_version.as_str()),
            ),
        ],
        false,
    )?;
    write_section(
        &mut output,
        "Processing",
        &[
            (
                "EnablePrefabOutlining",
                JsonScalar::Boolean(settings.processing.enable_prefab_outlining),
            ),
            (
                "EnableStaticMeshSeparation",
                JsonScalar::Boolean(settings.processing.enable_static_mesh_separation),
            ),
            (
                "EnableAssetDeduplication",
                JsonScalar::Boolean(settings.processing.enable_asset_deduplication),
            ),
            (
                "RemoveNullableAttributes",
                JsonScalar::Boolean(settings.processing.remove_nullable_attributes),
            ),
            (
                "PublicizeAssemblies",
                JsonScalar::Boolean(settings.processing.publicize_assemblies),
            ),
            (
                "BundledAssetsExportMode",
                JsonScalar::Integer(
                    settings
                        .processing
                        .bundled_assets_export_mode
                        .discriminant(),
                ),
            ),
        ],
        false,
    )?;
    write_section(
        &mut output,
        "Export",
        &[
            (
                "AudioExportFormat",
                JsonScalar::Integer(settings.export.audio_export_format.discriminant()),
            ),
            (
                "ImageExportFormat",
                JsonScalar::Integer(settings.export.image_export_format.discriminant()),
            ),
            (
                "LightmapTextureExportFormat",
                JsonScalar::Integer(
                    settings
                        .export
                        .lightmap_texture_export_format
                        .discriminant(),
                ),
            ),
            (
                "ScriptExportMode",
                JsonScalar::Integer(settings.export.script_export_mode.discriminant()),
            ),
            (
                "ScriptLanguageVersion",
                JsonScalar::Integer(settings.export.script_language_version.discriminant()),
            ),
            (
                "ScriptTypesFullyQualified",
                JsonScalar::Boolean(settings.export.script_types_fully_qualified),
            ),
            (
                "ShaderExportMode",
                JsonScalar::Integer(settings.export.shader_export_mode.discriminant()),
            ),
            (
                "SpriteExportMode",
                JsonScalar::Integer(settings.export.sprite_export_mode.discriminant()),
            ),
            (
                "TextExportMode",
                JsonScalar::Integer(settings.export.text_export_mode.discriminant()),
            ),
            (
                "ExportUnreadableAssets",
                JsonScalar::Boolean(settings.export.export_unreadable_assets),
            ),
            (
                "PreferOriginalTextureExtension",
                JsonScalar::Boolean(settings.export.prefer_original_texture_extension),
            ),
            (
                "SaveSettingsToDisk",
                JsonScalar::Boolean(settings.export.save_settings_to_disk),
            ),
            (
                "LanguageCode",
                settings
                    .export
                    .language_code
                    .as_deref()
                    .map(JsonScalar::Text)
                    .unwrap_or(JsonScalar::Null),
            ),
        ],
        true,
    )?;
    output.push('}');
    Ok(output)
}

/// One scalar member value for the fixed settings schema.
enum JsonScalar<'a> {
    Integer(i32),
    Boolean(bool),
    Text(&'a str),
    Null,
}

fn write_section(
    output: &mut String,
    name: &str,
    members: &[(&str, JsonScalar<'_>)],
    is_last: bool,
) -> Result<(), SettingsFileError> {
    output.push_str("  \"");
    output.push_str(name);
    output.push_str("\": {\r\n");
    for (index, (member, value)) in members.iter().enumerate() {
        output.push_str("    \"");
        output.push_str(member);
        output.push_str("\": ");
        match value {
            JsonScalar::Integer(number) => {
                output.push_str(&number.to_string());
            }
            JsonScalar::Boolean(flag) => {
                output.push_str(if *flag { "true" } else { "false" });
            }
            JsonScalar::Text(text) => {
                push_json_string(output, text).map_err(|_| {
                    SettingsFileError::AllocationFailed {
                        resource: "settings file text",
                    }
                })?;
            }
            JsonScalar::Null => {
                output.push_str("null");
            }
        }
        if index + 1 < members.len() {
            output.push(',');
        }
        output.push_str("\r\n");
    }
    output.push_str("  }");
    if !is_last {
        output.push(',');
    }
    output.push_str("\r\n");
    Ok(())
}

/// Append a JSON string with minimal valid escaping (`"`, `\`, C0 controls).
fn push_json_string(output: &mut String, text: &str) -> Result<(), ()> {
    output.try_reserve(text.len() + 2).map_err(|_| ())?;
    output.push('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{09}' => output.push_str("\\t"),
            '\u{0A}' => output.push_str("\\n"),
            '\u{0C}' => output.push_str("\\f"),
            '\u{0D}' => output.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                output.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => output.push(c),
        }
    }
    output.push('"');
    Ok(())
}

/// `SerializedSettings.DefaultFilePath`: the executing directory joined with the file name.
///
/// `AppContext.BaseDirectory` arrives as a caller fact; process queries stay in product code.
pub fn default_settings_file_path(executing_directory: &str, platform: HostPathPlatform) -> String {
    join_path(executing_directory, SETTINGS_FILE_NAME, platform)
}

/// What posting the settings form does with the default file (`SettingsPage.HandlePostRequest`
/// plus `FullConfiguration.MaybeSaveToDefaultPath`): a checked box saves, an unchecked box
/// deletes any stored file, and background saves only happen when checked.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SettingsPersistenceAction {
    /// `Configuration.SaveToDefaultPath()`.
    SaveToDefaultPath,
    /// `SerializedSettings.DeleteDefaultPath()`.
    DeleteDefaultPath,
    /// No file write; the stored file (if any) is left alone.
    LeaveStoredFile,
}

/// The form-post gate: checked saves, unchecked deletes.
pub const fn decide_settings_persistence(save_settings_to_disk: bool) -> SettingsPersistenceAction {
    if save_settings_to_disk {
        SettingsPersistenceAction::SaveToDefaultPath
    } else {
        SettingsPersistenceAction::DeleteDefaultPath
    }
}

/// The background auto-save gate: only a checked box writes.
pub const fn decide_settings_auto_save(save_settings_to_disk: bool) -> SettingsPersistenceAction {
    if save_settings_to_disk {
        SettingsPersistenceAction::SaveToDefaultPath
    } else {
        SettingsPersistenceAction::LeaveStoredFile
    }
}

/// `CoreConfiguration.LogConfigurationValues` (plus `FullConfiguration`'s override)
/// as message texts, in source order.
///
/// The export root path is logged from the live configuration rather than the
/// settings file, so it arrives as a caller fact; logger dispatch stays
/// caller-side. Only the source-logged subset appears: record members the source
/// never logs (`SaveSettingsToDisk`, `LanguageCode`, `ScriptTypesFullyQualified`,
/// `RemoveNullableAttributes`, `PublicizeAssemblies`) are omitted here too. Enums
/// render by member name (the `{:?}` layouts pinned in `settings.rs`), booleans
/// render capitalized like .NET, and versions render as their stored text.
pub fn settings_log_messages(
    export_root_path: &str,
    settings: &SerializedSettingsFile,
) -> Vec<String> {
    let mut messages = Vec::with_capacity(20);
    messages.push("Configuration Settings:".to_owned());
    messages.push(format!("ExportRootPath: {export_root_path}"));
    let import = &settings.import;
    messages.push(format!(
        "ScriptContentLevel: {:?}",
        import.script_content_level
    ));
    messages.push(format!(
        "StreamingAssetsMode: {:?}",
        import.streaming_assets_mode
    ));
    messages.push(format!(
        "DefaultVersion: {}",
        import.default_version.as_str()
    ));
    messages.push(format!("TargetVersion: {}", import.target_version.as_str()));
    let processing = &settings.processing;
    messages.push(format!(
        "EnablePrefabOutlining: {}",
        dotnet_bool(processing.enable_prefab_outlining)
    ));
    messages.push(format!(
        "EnableStaticMeshSeparation: {}",
        dotnet_bool(processing.enable_static_mesh_separation)
    ));
    messages.push(format!(
        "EnableAssetDeduplication: {}",
        dotnet_bool(processing.enable_asset_deduplication)
    ));
    messages.push(format!(
        "BundledAssetsExportMode: {:?}",
        processing.bundled_assets_export_mode
    ));
    let export = &settings.export;
    messages.push(format!(
        "AudioExportFormat: {:?}",
        export.audio_export_format
    ));
    messages.push(format!(
        "ImageExportFormat: {:?}",
        export.image_export_format
    ));
    messages.push(format!(
        "LightmapTextureExportFormat: {:?}",
        export.lightmap_texture_export_format
    ));
    messages.push(format!("ScriptExportMode: {:?}", export.script_export_mode));
    messages.push(format!(
        "ScriptLanguageVersion: {:?}",
        export.script_language_version
    ));
    messages.push(format!("ShaderExportMode: {:?}", export.shader_export_mode));
    messages.push(format!("SpriteExportMode: {:?}", export.sprite_export_mode));
    messages.push(format!("TextExportMode: {:?}", export.text_export_mode));
    messages.push(format!(
        "ExportUnreadableAssets: {}",
        dotnet_bool(export.export_unreadable_assets)
    ));
    messages.push(format!(
        "PreferOriginalTextureExtension: {}",
        dotnet_bool(export.prefer_original_texture_extension)
    ));
    messages
}

/// .NET renders booleans capitalized; Rust does not.
const fn dotnet_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

/// Parse `AssetRipper.Settings.json` back into records.
///
/// Unknown properties are ignored, missing properties keep defaults, and duplicate keys
/// resolve last-wins, matching `System.Text.Json` deserialization. Both CRLF and LF inputs
/// are accepted.
pub fn parse_settings_file(
    text: &str,
    limits: SettingsFileLimits,
) -> Result<SerializedSettingsFile, SettingsFileError> {
    // `File.ReadAllText` strips a UTF-8 BOM before STJ parses; editors (notably Windows
    // Notepad) add one when users hand-edit the file, so accept it here too.
    let text = strip_leading_bom(text);
    if text.len() > limits.max_file_bytes {
        return Err(SettingsFileError::InputTooLong {
            byte_count: text.len(),
            limit: limits.max_file_bytes,
        });
    }
    let mut parser = JsonParser {
        bytes: text.as_bytes(),
        position: 0,
        depth: 0,
        limits,
    };
    let value = parser.parse_value("$")?;
    parser.skip_whitespace();
    if parser.position != parser.bytes.len() {
        return Err(SettingsFileError::InvalidJson {
            path: "$".to_owned(),
            reason: "trailing content after the root value",
        });
    }
    let root = value.as_object("$")?;
    let import = map_import(lookup(root, "Import", "$")?.as_object("$.Import")?, limits)?;
    let processing = map_processing(
        lookup(root, "Processing", "$")?.as_object("$.Processing")?,
        limits,
    )?;
    let export = map_export(lookup(root, "Export", "$")?.as_object("$.Export")?, limits)?;
    Ok(SerializedSettingsFile {
        import,
        processing,
        export,
    })
}

/// Strip one leading UTF-8 BOM, mirroring the `File.ReadAllText` decoding the source
/// deserializer sees. Only a prefix is stripped; interior zero-width no-break spaces
/// keep their normal string handling on both sides.
fn strip_leading_bom(text: &str) -> &str {
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

fn map_import(
    members: &[(String, JsonValue)],
    limits: SettingsFileLimits,
) -> Result<ImportSettingsFile, SettingsFileError> {
    let mut settings = ImportSettingsFile::from_defaults(ImportDefaults::default());
    if let Some(value) = find_member(members, "ScriptContentLevel") {
        settings.script_content_level = map_enum(
            value,
            "$.Import.ScriptContentLevel",
            ScriptContentLevel::from_discriminant,
        )?;
    }
    if let Some(value) = find_member(members, "StreamingAssetsMode") {
        settings.streaming_assets_mode = map_enum(
            value,
            "$.Import.StreamingAssetsMode",
            StreamingAssetsMode::from_discriminant,
        )?;
    }
    if let Some(value) = find_member(members, "DefaultVersion") {
        settings.default_version = map_version(value, "$.Import.DefaultVersion", limits)?;
    }
    if let Some(value) = find_member(members, "TargetVersion") {
        settings.target_version = map_version(value, "$.Import.TargetVersion", limits)?;
    }
    Ok(settings)
}

fn map_processing(
    members: &[(String, JsonValue)],
    limits: SettingsFileLimits,
) -> Result<ProcessingSettingsFile, SettingsFileError> {
    let _ = limits;
    let mut settings = ProcessingSettingsFile::from(ProcessingDefaults::default());
    if let Some(value) = find_member(members, "EnablePrefabOutlining") {
        settings.enable_prefab_outlining = value.as_bool("$.Processing.EnablePrefabOutlining")?;
    }
    if let Some(value) = find_member(members, "EnableStaticMeshSeparation") {
        settings.enable_static_mesh_separation =
            value.as_bool("$.Processing.EnableStaticMeshSeparation")?;
    }
    if let Some(value) = find_member(members, "EnableAssetDeduplication") {
        settings.enable_asset_deduplication =
            value.as_bool("$.Processing.EnableAssetDeduplication")?;
    }
    if let Some(value) = find_member(members, "RemoveNullableAttributes") {
        settings.remove_nullable_attributes =
            value.as_bool("$.Processing.RemoveNullableAttributes")?;
    }
    if let Some(value) = find_member(members, "PublicizeAssemblies") {
        settings.publicize_assemblies = value.as_bool("$.Processing.PublicizeAssemblies")?;
    }
    if let Some(value) = find_member(members, "BundledAssetsExportMode") {
        settings.bundled_assets_export_mode = map_enum(
            value,
            "$.Processing.BundledAssetsExportMode",
            BundledAssetsExportMode::from_discriminant,
        )?;
    }
    Ok(settings)
}

#[allow(clippy::too_many_lines)]
fn map_export(
    members: &[(String, JsonValue)],
    limits: SettingsFileLimits,
) -> Result<ExportSettingsFile, SettingsFileError> {
    let _ = limits;
    let mut settings = ExportSettingsFile::from(ExportDefaults::default());
    if let Some(value) = find_member(members, "AudioExportFormat") {
        settings.audio_export_format = map_enum(
            value,
            "$.Export.AudioExportFormat",
            AudioExportFormat::from_discriminant,
        )?;
    }
    if let Some(value) = find_member(members, "ImageExportFormat") {
        settings.image_export_format = map_enum(
            value,
            "$.Export.ImageExportFormat",
            ImageExportFormat::from_discriminant,
        )?;
    }
    if let Some(value) = find_member(members, "LightmapTextureExportFormat") {
        settings.lightmap_texture_export_format = map_enum(
            value,
            "$.Export.LightmapTextureExportFormat",
            LightmapTextureExportFormat::from_discriminant,
        )?;
    }
    if let Some(value) = find_member(members, "ScriptExportMode") {
        settings.script_export_mode = map_enum(
            value,
            "$.Export.ScriptExportMode",
            ScriptExportMode::from_discriminant,
        )?;
    }
    if let Some(value) = find_member(members, "ScriptLanguageVersion") {
        settings.script_language_version = map_enum(
            value,
            "$.Export.ScriptLanguageVersion",
            ScriptLanguageVersion::from_discriminant,
        )?;
    }
    if let Some(value) = find_member(members, "ScriptTypesFullyQualified") {
        settings.script_types_fully_qualified =
            value.as_bool("$.Export.ScriptTypesFullyQualified")?;
    }
    if let Some(value) = find_member(members, "ShaderExportMode") {
        settings.shader_export_mode = map_enum(
            value,
            "$.Export.ShaderExportMode",
            ShaderExportMode::from_discriminant,
        )?;
    }
    if let Some(value) = find_member(members, "SpriteExportMode") {
        settings.sprite_export_mode = map_enum(
            value,
            "$.Export.SpriteExportMode",
            SpriteExportMode::from_discriminant,
        )?;
    }
    if let Some(value) = find_member(members, "TextExportMode") {
        settings.text_export_mode = map_enum(
            value,
            "$.Export.TextExportMode",
            TextExportMode::from_discriminant,
        )?;
    }
    if let Some(value) = find_member(members, "ExportUnreadableAssets") {
        settings.export_unreadable_assets = value.as_bool("$.Export.ExportUnreadableAssets")?;
    }
    if let Some(value) = find_member(members, "PreferOriginalTextureExtension") {
        settings.prefer_original_texture_extension =
            value.as_bool("$.Export.PreferOriginalTextureExtension")?;
    }
    if let Some(value) = find_member(members, "SaveSettingsToDisk") {
        settings.save_settings_to_disk = value.as_bool("$.Export.SaveSettingsToDisk")?;
    }
    if let Some(value) = find_member(members, "LanguageCode") {
        settings.language_code = match value {
            JsonValue::Null => None,
            JsonValue::Str(text) => {
                if text.len() > limits.max_string_bytes {
                    return Err(SettingsFileError::InputTooLong {
                        byte_count: text.len(),
                        limit: limits.max_string_bytes,
                    });
                }
                Some(text.clone())
            }
            _ => {
                return Err(SettingsFileError::InvalidJson {
                    path: "$.Export.LanguageCode".to_owned(),
                    reason: "expected a string or null",
                });
            }
        };
    }
    Ok(settings)
}

fn map_enum<T>(
    value: &JsonValue,
    path: &str,
    convert: impl FnOnce(i32) -> Option<T>,
) -> Result<T, SettingsFileError> {
    match value {
        JsonValue::Integer(number) => {
            let discriminant = enum_discriminant(*number, path)?;
            convert(discriminant).ok_or(SettingsFileError::UnknownEnumValue {
                path: path.to_owned(),
                value: discriminant,
            })
        }
        _ => Err(SettingsFileError::InvalidJson {
            path: path.to_owned(),
            reason: "expected an integer discriminant",
        }),
    }
}

/// Checked `i64`-to-`i32` narrowing for enum discriminants over caller facts: JSON
/// integers are 64-bit here, but the source enums are 32-bit, so out-of-range values
/// fail with the naming path instead of wrapping into a valid discriminant. Every enum
/// property funnels through `map_enum`, giving all of them one decision point; the
/// overflow arms are unreachable in realistic files, so the kernel keeps them provable.
fn enum_discriminant(number: i64, path: &str) -> Result<i32, SettingsFileError> {
    i32::try_from(number).map_err(|_| SettingsFileError::InvalidJson {
        path: path.to_owned(),
        reason: "expected a 32-bit integer",
    })
}

fn map_version(
    value: &JsonValue,
    path: &str,
    limits: SettingsFileLimits,
) -> Result<VersionText, SettingsFileError> {
    match value {
        JsonValue::Str(text) => {
            VersionText::new(text, limits).map_err(|_| SettingsFileError::InvalidJson {
                path: path.to_owned(),
                reason: "Unity version renderings must be non-empty bounded ASCII",
            })
        }
        _ => Err(SettingsFileError::InvalidJson {
            path: path.to_owned(),
            reason: "expected a version string",
        }),
    }
}

/// Last-wins member lookup, matching deserialization.
fn find_member<'a>(members: &'a [(String, JsonValue)], name: &str) -> Option<&'a JsonValue> {
    members
        .iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

/// Required section lookup; sections are mandatory in `SerializedSettings`.
fn lookup<'a>(
    members: &'a [(String, JsonValue)],
    name: &str,
    parent: &str,
) -> Result<&'a JsonValue, SettingsFileError> {
    find_member(members, name).ok_or_else(|| SettingsFileError::InvalidJson {
        path: parent.to_owned(),
        reason: "missing settings section",
    })
}

/// A minimal JSON value model for the fixed settings schema.
#[derive(Clone, Debug, PartialEq)]
enum JsonValue {
    Null,
    Bool(bool),
    Integer(i64),
    Str(String),
    Object(Vec<(String, JsonValue)>),
    /// Only traversed to skip unknown properties; the schema itself has no arrays.
    Array(Vec<JsonValue>),
}

impl JsonValue {
    fn as_object(&self, path: &str) -> Result<&[(String, JsonValue)], SettingsFileError> {
        match self {
            Self::Object(members) => Ok(members),
            _ => Err(SettingsFileError::InvalidJson {
                path: path.to_owned(),
                reason: "expected an object",
            }),
        }
    }

    fn as_bool(&self, path: &str) -> Result<bool, SettingsFileError> {
        match self {
            Self::Bool(flag) => Ok(*flag),
            _ => Err(SettingsFileError::InvalidJson {
                path: path.to_owned(),
                reason: "expected true or false",
            }),
        }
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    position: usize,
    depth: usize,
    limits: SettingsFileLimits,
}

impl<'a> JsonParser<'a> {
    /// One nesting level for objects and arrays against the depth bound, so both
    /// containers share a single decision point.
    fn enter_nesting(&mut self, path: &str) -> Result<(), SettingsFileError> {
        self.depth += 1;
        if self.depth > self.limits.max_nesting_depth {
            return Err(self.error(path, "nesting depth exceeds the limit"));
        }
        Ok(())
    }

    fn parse_value(&mut self, path: &str) -> Result<JsonValue, SettingsFileError> {
        self.skip_whitespace();
        match self.bytes.get(self.position) {
            Some(b'{') => self.parse_object(path),
            Some(b'[') => self.parse_array(path),
            Some(b'"') => Ok(JsonValue::Str(self.parse_string(path)?)),
            Some(b't') => self.parse_literal(path, "true", JsonValue::Bool(true)),
            Some(b'f') => self.parse_literal(path, "false", JsonValue::Bool(false)),
            Some(b'n') => self.parse_literal(path, "null", JsonValue::Null),
            Some(b'-' | b'0'..=b'9') => Ok(JsonValue::Integer(self.parse_integer(path)?)),
            _ => Err(self.error(path, "expected a JSON value")),
        }
    }

    fn parse_object(&mut self, path: &str) -> Result<JsonValue, SettingsFileError> {
        self.enter_nesting(path)?;
        // `position` is at `{`.
        self.position += 1;
        let mut members = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.position) == Some(&b'}') {
            self.position += 1;
            self.depth -= 1;
            return Ok(JsonValue::Object(members));
        }
        loop {
            self.skip_whitespace();
            if self.bytes.get(self.position) != Some(&b'"') {
                return Err(self.error(path, "expected a member name"));
            }
            let name = self.parse_string(path)?;
            self.skip_whitespace();
            if self.bytes.get(self.position) != Some(&b':') {
                return Err(self.error(path, "expected a colon"));
            }
            self.position += 1;
            let value = self.parse_value(path)?;
            members.push((name, value));
            self.skip_whitespace();
            match self.bytes.get(self.position) {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b'}') => {
                    self.position += 1;
                    self.depth -= 1;
                    return Ok(JsonValue::Object(members));
                }
                _ => return Err(self.error(path, "expected a comma or closing brace")),
            }
        }
    }

    fn parse_array(&mut self, path: &str) -> Result<JsonValue, SettingsFileError> {
        self.enter_nesting(path)?;
        // `position` is at `[`.
        self.position += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.position) == Some(&b']') {
            self.position += 1;
            self.depth -= 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            let value = self.parse_value(path)?;
            items.push(value);
            self.skip_whitespace();
            match self.bytes.get(self.position) {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b']') => {
                    self.position += 1;
                    self.depth -= 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(self.error(path, "expected a comma or closing bracket")),
            }
        }
    }

    fn parse_string(&mut self, path: &str) -> Result<String, SettingsFileError> {
        // `position` is at the opening quote.
        self.position += 1;
        let mut output = String::new();
        loop {
            let byte =
                *self
                    .bytes
                    .get(self.position)
                    .ok_or_else(|| SettingsFileError::InvalidJson {
                        path: path.to_owned(),
                        reason: "unterminated string",
                    })?;
            match byte {
                b'"' => {
                    self.position += 1;
                    if output.len() > self.limits.max_string_bytes {
                        return Err(SettingsFileError::InputTooLong {
                            byte_count: output.len(),
                            limit: self.limits.max_string_bytes,
                        });
                    }
                    return Ok(output);
                }
                b'\\' => {
                    self.position += 1;
                    let escaped = *self.bytes.get(self.position).ok_or_else(|| {
                        SettingsFileError::InvalidJson {
                            path: path.to_owned(),
                            reason: "unterminated escape",
                        }
                    })?;
                    self.position += 1;
                    match escaped {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{08}'),
                        b'f' => output.push('\u{0C}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => {
                            let unit = self.parse_hex4(path)?;
                            if (0xD800..0xDC00).contains(&unit) {
                                if self.bytes.get(self.position) == Some(&b'\\')
                                    && self.bytes.get(self.position + 1) == Some(&b'u')
                                {
                                    self.position += 2;
                                    let low = self.parse_hex4(path)?;
                                    if (0xDC00..0xE000).contains(&low) {
                                        let scalar = 0x10000
                                            + ((u32::from(unit) - 0xD800) << 10)
                                            + (u32::from(low) - 0xDC00);
                                        output.push(char::from_u32(scalar).ok_or_else(|| {
                                            SettingsFileError::InvalidJson {
                                                path: path.to_owned(),
                                                reason: "invalid surrogate pair",
                                            }
                                        })?);
                                    } else {
                                        return Err(self.error(path, "lone leading surrogate"));
                                    }
                                } else {
                                    return Err(self.error(path, "lone leading surrogate"));
                                }
                            } else if (0xDC00..0xE000).contains(&unit) {
                                return Err(self.error(path, "lone trailing surrogate"));
                            } else {
                                output.push(char::from_u32(u32::from(unit)).ok_or_else(|| {
                                    SettingsFileError::InvalidJson {
                                        path: path.to_owned(),
                                        reason: "invalid escape unit",
                                    }
                                })?);
                            }
                        }
                        _ => {
                            return Err(self.error(path, "unknown escape"));
                        }
                    }
                }
                0x00..=0x1F => {
                    return Err(self.error(path, "unescaped control character"));
                }
                _ => {
                    let rest = &self.bytes[self.position..];
                    let text =
                        core::str::from_utf8(rest).map_err(|_| SettingsFileError::InvalidJson {
                            path: path.to_owned(),
                            reason: "invalid UTF-8",
                        })?;
                    let character =
                        text.chars()
                            .next()
                            .ok_or_else(|| SettingsFileError::InvalidJson {
                                path: path.to_owned(),
                                reason: "unterminated string",
                            })?;
                    output.push(character);
                    self.position += character.len_utf8();
                }
            }
            if output.len() > self.limits.max_string_bytes {
                return Err(SettingsFileError::InputTooLong {
                    byte_count: output.len(),
                    limit: self.limits.max_string_bytes,
                });
            }
        }
    }

    fn parse_hex4(&mut self, path: &str) -> Result<u16, SettingsFileError> {
        if self.position + 4 > self.bytes.len() {
            return Err(self.error(path, "truncated escape"));
        }
        let mut unit: u16 = 0;
        for byte in &self.bytes[self.position..self.position + 4] {
            unit = unit.wrapping_mul(16).wrapping_add(match byte {
                b'0'..=b'9' => (byte - b'0') as u16,
                b'a'..=b'f' => (byte - b'a' + 10) as u16,
                b'A'..=b'F' => (byte - b'A' + 10) as u16,
                _ => return Err(self.error(path, "invalid escape unit")),
            });
        }
        self.position += 4;
        Ok(unit)
    }

    fn parse_literal(
        &mut self,
        path: &str,
        literal: &str,
        value: JsonValue,
    ) -> Result<JsonValue, SettingsFileError> {
        if self.bytes.len() - self.position < literal.len()
            || &self.bytes[self.position..self.position + literal.len()] != literal.as_bytes()
        {
            return Err(self.error(path, "invalid literal"));
        }
        self.position += literal.len();
        Ok(value)
    }

    /// Strict JSON integers: optional minus, no leading zeros, no fractions or exponents.
    /// Non-integral numbers are rejected exactly as the source deserializer rejects them.
    fn parse_integer(&mut self, path: &str) -> Result<i64, SettingsFileError> {
        let start = self.position;
        if self.bytes.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        let digits = self.position;
        while matches!(self.bytes.get(self.position), Some(b'0'..=b'9')) {
            self.position += 1;
        }
        if self.position == digits {
            return Err(self.error(path, "expected digits"));
        }
        if self.bytes[digits] == b'0' && self.position - digits > 1 {
            return Err(self.error(path, "leading zeros are not valid JSON"));
        }
        if matches!(self.bytes.get(self.position), Some(b'.' | b'e' | b'E')) {
            return Err(self.error(path, "fractional numbers are not valid here"));
        }
        let text = core::str::from_utf8(&self.bytes[start..self.position]).map_err(|_| {
            SettingsFileError::InvalidJson {
                path: path.to_owned(),
                reason: "invalid number",
            }
        })?;
        text.parse::<i64>()
            .map_err(|_| self.error(path, "integer out of range"))
    }

    fn skip_whitespace(&mut self) {
        while matches!(
            self.bytes.get(self.position),
            Some(b' ' | b'\t' | b'\n' | b'\r')
        ) {
            self.position += 1;
        }
    }

    fn error(&self, path: &str, reason: &'static str) -> SettingsFileError {
        SettingsFileError::InvalidJson {
            path: path.to_owned(),
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_settings_file, settings_log_messages, write_settings_file, ExportSettingsFile,
        ImportSettingsFile, ProcessingSettingsFile, SerializedSettingsFile, SettingsFileError,
        SettingsFileLimits, VersionText, DEFAULT_UNITY_VERSION_TEXT,
    };
    use crate::export::settings::{
        AudioExportFormat, BundledAssetsExportMode, ExportDefaults, ImageExportFormat,
        ImportDefaults, LightmapTextureExportFormat, ProcessingDefaults, ScriptContentLevel,
        ScriptExportMode, ScriptLanguageVersion, ShaderExportMode, SpriteExportMode,
        StreamingAssetsMode, TextExportMode,
    };

    // Byte-exact expectations transcribed from the shipped-`AssetRipper.Export.dll` probe
    // (`Temp/opencode/settingprobe2`, SDK 10.0.401): CRLF newlines, no BOM, no trailing
    // newline. Literals carry `\r\n` explicitly so `eol=lf` checkouts cannot corrupt them.
    const DEFAULTS_JSON: &str = "{\r\n  \"Import\": {\r\n    \"ScriptContentLevel\": 2,\r\n    \"StreamingAssetsMode\": 1,\r\n    \"DefaultVersion\": \"0.0.0a0\",\r\n    \"TargetVersion\": \"0.0.0a0\"\r\n  },\r\n  \"Processing\": {\r\n    \"EnablePrefabOutlining\": false,\r\n    \"EnableStaticMeshSeparation\": true,\r\n    \"EnableAssetDeduplication\": false,\r\n    \"RemoveNullableAttributes\": false,\r\n    \"PublicizeAssemblies\": false,\r\n    \"BundledAssetsExportMode\": 2\r\n  },\r\n  \"Export\": {\r\n    \"AudioExportFormat\": 2,\r\n    \"ImageExportFormat\": 4,\r\n    \"LightmapTextureExportFormat\": 2,\r\n    \"ScriptExportMode\": 1,\r\n    \"ScriptLanguageVersion\": -1,\r\n    \"ScriptTypesFullyQualified\": false,\r\n    \"ShaderExportMode\": 0,\r\n    \"SpriteExportMode\": 0,\r\n    \"TextExportMode\": 2,\r\n    \"ExportUnreadableAssets\": false,\r\n    \"PreferOriginalTextureExtension\": true,\r\n    \"SaveSettingsToDisk\": false,\r\n    \"LanguageCode\": null\r\n  }\r\n}";

    const MODIFIED_JSON: &str = "{\r\n  \"Import\": {\r\n    \"ScriptContentLevel\": 3,\r\n    \"StreamingAssetsMode\": 0,\r\n    \"DefaultVersion\": \"2019.4.40f1\",\r\n    \"TargetVersion\": \"2021.3.5f1\"\r\n  },\r\n  \"Processing\": {\r\n    \"EnablePrefabOutlining\": true,\r\n    \"EnableStaticMeshSeparation\": false,\r\n    \"EnableAssetDeduplication\": true,\r\n    \"RemoveNullableAttributes\": true,\r\n    \"PublicizeAssemblies\": true,\r\n    \"BundledAssetsExportMode\": 1\r\n  },\r\n  \"Export\": {\r\n    \"AudioExportFormat\": 3,\r\n    \"ImageExportFormat\": 0,\r\n    \"LightmapTextureExportFormat\": 0,\r\n    \"ScriptExportMode\": 0,\r\n    \"ScriptLanguageVersion\": 1,\r\n    \"ScriptTypesFullyQualified\": true,\r\n    \"ShaderExportMode\": 2,\r\n    \"SpriteExportMode\": 1,\r\n    \"TextExportMode\": 0,\r\n    \"ExportUnreadableAssets\": true,\r\n    \"PreferOriginalTextureExtension\": false,\r\n    \"SaveSettingsToDisk\": true,\r\n    \"LanguageCode\": \"ja\"\r\n  }\r\n}";

    fn modified_settings() -> SerializedSettingsFile {
        SerializedSettingsFile {
            import: ImportSettingsFile {
                script_content_level: ScriptContentLevel::Level3,
                streaming_assets_mode: StreamingAssetsMode::Ignore,
                default_version: VersionText::new("2019.4.40f1", SettingsFileLimits::default())
                    .unwrap(),
                target_version: VersionText::new("2021.3.5f1", SettingsFileLimits::default())
                    .unwrap(),
            },
            processing: ProcessingSettingsFile {
                enable_prefab_outlining: true,
                enable_static_mesh_separation: false,
                enable_asset_deduplication: true,
                remove_nullable_attributes: true,
                publicize_assemblies: true,
                bundled_assets_export_mode: BundledAssetsExportMode::GroupByBundleName,
            },
            export: ExportSettingsFile {
                audio_export_format: AudioExportFormat::PreferWav,
                image_export_format: ImageExportFormat::Bmp,
                lightmap_texture_export_format: LightmapTextureExportFormat::Exr,
                script_export_mode: ScriptExportMode::Decompiled,
                script_language_version: ScriptLanguageVersion::CSharp1,
                script_types_fully_qualified: true,
                shader_export_mode: ShaderExportMode::Decompile,
                sprite_export_mode: SpriteExportMode::Native,
                text_export_mode: TextExportMode::Bytes,
                export_unreadable_assets: true,
                prefer_original_texture_extension: false,
                save_settings_to_disk: true,
                language_code: Some("ja".to_owned()),
            },
        }
    }

    #[test]
    fn writer_matches_the_probe_bytes_for_defaults() {
        let settings = SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        );
        assert_eq!(DEFAULT_UNITY_VERSION_TEXT, "0.0.0a0");
        assert_eq!(write_settings_file(&settings).unwrap(), DEFAULTS_JSON);
    }

    #[test]
    fn writer_matches_the_probe_bytes_for_modified_records() {
        assert_eq!(
            write_settings_file(&modified_settings()).unwrap(),
            MODIFIED_JSON
        );
    }

    #[test]
    fn reader_round_trips_the_committed_probe_fixtures() {
        // Fixtures check out as LF under `eol=lf`; the reader accepts both line endings.
        for (fixture, expected, expected_json) in [
            (
                include_str!("settings_file_defaults.json"),
                SerializedSettingsFile::from_defaults(
                    ImportDefaults::default(),
                    ProcessingDefaults::default(),
                    ExportDefaults::default(),
                ),
                DEFAULTS_JSON,
            ),
            (
                include_str!("settings_file_modified.json"),
                modified_settings(),
                MODIFIED_JSON,
            ),
        ] {
            let parsed = parse_settings_file(fixture, SettingsFileLimits::default()).unwrap();
            assert_eq!(parsed, expected);
            // Re-serializing restores the CRLF probe bytes exactly.
            assert_eq!(write_settings_file(&parsed).unwrap(), expected_json);
        }
    }

    #[test]
    fn reader_mirrors_source_load_tolerances() {
        // Unknown properties are ignored and missing ones keep defaults.
        let parsed = parse_settings_file(
            "{\"Import\": {\"UnknownFuture\": [1, 2]}, \"Processing\": {}, \"Export\": {\"LanguageCode\": null}}",
            SettingsFileLimits::default(),
        )
        .unwrap();
        assert_eq!(
            parsed,
            SerializedSettingsFile::from_defaults(
                ImportDefaults::default(),
                ProcessingDefaults::default(),
                ExportDefaults::default(),
            )
        );
        // Duplicate keys resolve last-wins, matching deserialization.
        let parsed = parse_settings_file(
            "{\"Import\": {\"ScriptContentLevel\": 0, \"ScriptContentLevel\": 3}, \"Processing\": {}, \"Export\": {}}",
            SettingsFileLimits::default(),
        )
        .unwrap();
        assert_eq!(
            parsed.import.script_content_level,
            ScriptContentLevel::Level3
        );
        // Escapes decode, including surrogate pairs.
        let parsed = parse_settings_file(
            "{\"Import\": {}, \"Processing\": {}, \"Export\": {\"LanguageCode\": \"A\\u0022\\u00e9\\uD83D\\uDE00\"}}",
            SettingsFileLimits::default(),
        )
        .unwrap();
        assert_eq!(
            parsed.export.language_code.as_deref(),
            Some("A\"\u{e9}\u{1f600}")
        );
        // A leading BOM is stripped like `File.ReadAllText` strips it; only the prefix
        // is special, and a lone BOM is still an empty document.
        let parsed = parse_settings_file(
            "\u{FEFF}{\"Import\": {}, \"Processing\": {}, \"Export\": {}}",
            SettingsFileLimits::default(),
        )
        .unwrap();
        assert_eq!(
            parsed,
            SerializedSettingsFile::from_defaults(
                ImportDefaults::default(),
                ProcessingDefaults::default(),
                ExportDefaults::default(),
            )
        );
        assert!(parse_settings_file("\u{FEFF}", SettingsFileLimits::default()).is_err());
        assert!(parse_settings_file("\u{FEFF}\u{FEFF}{}", SettingsFileLimits::default()).is_err());
    }

    #[test]
    fn reader_rejects_malformed_inputs_with_paths() {
        for (input, path) in [
            ("", "$"),
            ("{\"Import\": {}}", "$"),
            ("{\"Import\": {}, \"Processing\": {}, \"Export\": {}, \"Trailing\": 1} trailing", "$"),
            (
                "{\"Import\": {}, \"Processing\": {}, \"Export\": {\"AudioExportFormat\": 99}}",
                "$.Export.AudioExportFormat",
            ),
            (
                "{\"Import\": {}, \"Processing\": {}, \"Export\": {\"AudioExportFormat\": \"Yaml\"}}",
                "$.Export.AudioExportFormat",
            ),
            (
                "{\"Import\": {}, \"Processing\": {}, \"Export\": {\"AudioExportFormat\": 2.0}}",
                "$",
            ),
            (
                "{\"Import\": {}, \"Processing\": {}, \"Export\": {\"SaveSettingsToDisk\": 1}}",
                "$.Export.SaveSettingsToDisk",
            ),
            (
                "{\"Import\": {}, \"Processing\": {}, \"Export\": {\"LanguageCode\": 1}}",
                "$.Export.LanguageCode",
            ),
            (
                "{\"Import\": {\"DefaultVersion\": \"\"}, \"Processing\": {}, \"Export\": {}}",
                "$.Import.DefaultVersion",
            ),
            (
                "{\"Import\": {}, \"Processing\": {}, \"Export\": null}",
                "$.Export",
            ),
        ] {
            let error = parse_settings_file(input, SettingsFileLimits::default())
                .expect_err(&format!("input must fail: {input}"));
            match error {
                SettingsFileError::InvalidJson { path: actual, .. } => {
                    assert_eq!(actual, path, "{input}")
                }
                SettingsFileError::UnknownEnumValue { path: actual, value } => {
                    assert_eq!(actual, path, "{input}");
                    assert_eq!(value, 99, "{input}");
                }
                other => panic!("wrong error for {input}: {other:?}"),
            }
        }
        // Bounds apply before parsing.
        assert!(matches!(
            parse_settings_file(
                "{}",
                SettingsFileLimits {
                    max_file_bytes: 1,
                    ..Default::default()
                }
            ),
            Err(SettingsFileError::InputTooLong { .. })
        ));
        assert!(matches!(
            VersionText::new("", SettingsFileLimits::default()),
            Err(SettingsFileError::InvalidJson { .. })
        ));
    }

    #[test]
    fn nesting_depth_guard_fires_through_both_containers() {
        let limits = SettingsFileLimits {
            max_nesting_depth: 3,
            ..Default::default()
        };
        // Objects and arrays share one decision point.
        for deep in ["[[[[]]]]", "{\"a\":{\"b\":{\"c\":{}}}}"] {
            let error = parse_settings_file(deep, limits).unwrap_err();
            assert!(
                matches!(
                    error,
                    SettingsFileError::InvalidJson {
                        reason: "nesting depth exceeds the limit",
                        ..
                    }
                ),
                "{deep}: {error:?}"
            );
        }
        // Exactly at the boundary the guard stays silent (later stages fail instead).
        for shallow in ["[[[]]]", "{\"a\":{\"b\":1}}"] {
            let error = parse_settings_file(shallow, limits).unwrap_err();
            assert!(
                !matches!(
                    error,
                    SettingsFileError::InvalidJson {
                        reason: "nesting depth exceeds the limit",
                        ..
                    }
                ),
                "{shallow}: {error:?}"
            );
        }
    }

    #[test]
    fn enum_discriminants_narrow_to_i32_or_fail() {
        assert_eq!(
            super::enum_discriminant(0, "$.Export.AudioExportFormat"),
            Ok(0)
        );
        assert_eq!(
            super::enum_discriminant(-1, "$.Export.ScriptLanguageVersion"),
            Ok(-1)
        );
        assert_eq!(super::enum_discriminant(i32::MAX as i64, "$"), Ok(i32::MAX));
        assert_eq!(super::enum_discriminant(i32::MIN as i64, "$"), Ok(i32::MIN));
        for out_of_range in [i32::MAX as i64 + 1, i32::MIN as i64 - 1, 9_999_999_999] {
            let error =
                super::enum_discriminant(out_of_range, "$.Export.AudioExportFormat").unwrap_err();
            assert!(
                matches!(
                    error,
                    SettingsFileError::InvalidJson { ref path, reason }
                        if path == "$.Export.AudioExportFormat"
                            && reason == "expected a 32-bit integer"
                ),
                "{out_of_range}: {error:?}"
            );
        }
        // The production path reports the same decision for every enum property.
        let error = parse_settings_file(
            "{\"Import\": {}, \"Processing\": {}, \"Export\": {\"AudioExportFormat\": 9999999999}}",
            SettingsFileLimits::default(),
        )
        .unwrap_err();
        assert!(
            matches!(
                error,
                SettingsFileError::InvalidJson { ref path, reason }
                    if path == "$.Export.AudioExportFormat"
                        && reason == "expected a 32-bit integer"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn persistence_gate_and_default_path_follow_the_source() {
        use super::{
            decide_settings_auto_save, decide_settings_persistence, default_settings_file_path,
            SettingsPersistenceAction, SETTINGS_FILE_NAME,
        };
        use crate::export::asset_path::HostPathPlatform;

        assert_eq!(SETTINGS_FILE_NAME, "AssetRipper.Settings.json");
        // Form post: checked saves, unchecked deletes.
        assert_eq!(
            decide_settings_persistence(true),
            SettingsPersistenceAction::SaveToDefaultPath
        );
        assert_eq!(
            decide_settings_persistence(false),
            SettingsPersistenceAction::DeleteDefaultPath
        );
        // Background auto-save: checked saves, unchecked leaves the file alone.
        assert_eq!(
            decide_settings_auto_save(true),
            SettingsPersistenceAction::SaveToDefaultPath
        );
        assert_eq!(
            decide_settings_auto_save(false),
            SettingsPersistenceAction::LeaveStoredFile
        );
        // Default path joins the executing directory on either platform.
        assert_eq!(
            default_settings_file_path("C:\\app", HostPathPlatform::Windows),
            "C:\\app\\AssetRipper.Settings.json"
        );
        assert_eq!(
            default_settings_file_path("/opt/app/", HostPathPlatform::Unix),
            "/opt/app/AssetRipper.Settings.json"
        );
        assert_eq!(
            default_settings_file_path("", HostPathPlatform::Unix),
            "AssetRipper.Settings.json"
        );
    }

    #[test]
    fn string_escapes_stay_valid() {
        let mut settings = modified_settings();
        settings.export.language_code = Some("q\"\\é\x01".to_owned());
        let written = write_settings_file(&settings).unwrap();
        assert!(written.contains("\"LanguageCode\": \"q\\\"\\\\é\\u0001\""));
        let parsed = parse_settings_file(&written, SettingsFileLimits::default()).unwrap();
        assert_eq!(parsed, settings);
    }

    #[test]
    fn log_messages_follow_the_source_rows_in_order() {
        let parsed = parse_settings_file(
            include_str!("settings_file_defaults.json"),
            SettingsFileLimits::default(),
        )
        .unwrap();
        assert_eq!(
            settings_log_messages("C:\\rip", &parsed),
            [
                "Configuration Settings:",
                "ExportRootPath: C:\\rip",
                "ScriptContentLevel: Level2",
                "StreamingAssetsMode: Extract",
                "DefaultVersion: 0.0.0a0",
                "TargetVersion: 0.0.0a0",
                "EnablePrefabOutlining: False",
                "EnableStaticMeshSeparation: True",
                "EnableAssetDeduplication: False",
                "BundledAssetsExportMode: DirectExport",
                "AudioExportFormat: Default",
                "ImageExportFormat: Png",
                "LightmapTextureExportFormat: Yaml",
                "ScriptExportMode: Hybrid",
                "ScriptLanguageVersion: AutoSafe",
                "ShaderExportMode: Dummy",
                "SpriteExportMode: Yaml",
                "TextExportMode: Parse",
                "ExportUnreadableAssets: False",
                "PreferOriginalTextureExtension: True",
            ]
        );
    }

    #[test]
    fn log_messages_render_modified_values_like_dotnet() {
        assert_eq!(
            settings_log_messages("/tmp/rip", &modified_settings()),
            [
                "Configuration Settings:",
                "ExportRootPath: /tmp/rip",
                "ScriptContentLevel: Level3",
                "StreamingAssetsMode: Ignore",
                "DefaultVersion: 2019.4.40f1",
                "TargetVersion: 2021.3.5f1",
                "EnablePrefabOutlining: True",
                "EnableStaticMeshSeparation: False",
                "EnableAssetDeduplication: True",
                "BundledAssetsExportMode: GroupByBundleName",
                "AudioExportFormat: PreferWav",
                "ImageExportFormat: Bmp",
                "LightmapTextureExportFormat: Exr",
                "ScriptExportMode: Decompiled",
                "ScriptLanguageVersion: CSharp1",
                "ShaderExportMode: Decompile",
                "SpriteExportMode: Native",
                "TextExportMode: Bytes",
                "ExportUnreadableAssets: True",
                "PreferOriginalTextureExtension: False",
            ]
        );
    }
}
