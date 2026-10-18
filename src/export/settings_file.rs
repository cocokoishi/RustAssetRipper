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
use crate::json::{parse_json, JsonError, JsonLimits, JsonValue};

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
        &import_settings_members(&settings.import),
        false,
    )?;
    write_section(
        &mut output,
        "Processing",
        &processing_settings_members(&settings.processing),
        false,
    )?;
    write_section(
        &mut output,
        "Export",
        &export_settings_members(&settings.export),
        true,
    )?;
    output.push('}');
    Ok(output)
}

/// Serialize one `JsonDataInstance<ImportSettings>.Text` value from the configuration page.
pub fn write_import_settings_text(
    settings: &ImportSettingsFile,
) -> Result<String, SettingsFileError> {
    write_record(&import_settings_members(settings))
}

/// Serialize one `JsonDataInstance<ProcessingSettings>.Text` value from the configuration page.
pub fn write_processing_settings_text(
    settings: &ProcessingSettingsFile,
) -> Result<String, SettingsFileError> {
    write_record(&processing_settings_members(settings))
}

/// Serialize one `JsonDataInstance<ExportSettings>.Text` value from the configuration page.
pub fn write_export_settings_text(
    settings: &ExportSettingsFile,
) -> Result<String, SettingsFileError> {
    write_record(&export_settings_members(settings))
}

fn import_settings_members(settings: &ImportSettingsFile) -> [(&'static str, JsonScalar<'_>); 4] {
    [
        (
            "ScriptContentLevel",
            JsonScalar::Integer(settings.script_content_level.discriminant()),
        ),
        (
            "StreamingAssetsMode",
            JsonScalar::Integer(settings.streaming_assets_mode.discriminant()),
        ),
        (
            "DefaultVersion",
            JsonScalar::Text(settings.default_version.as_str()),
        ),
        (
            "TargetVersion",
            JsonScalar::Text(settings.target_version.as_str()),
        ),
    ]
}

fn processing_settings_members(
    settings: &ProcessingSettingsFile,
) -> [(&'static str, JsonScalar<'_>); 6] {
    [
        (
            "EnablePrefabOutlining",
            JsonScalar::Boolean(settings.enable_prefab_outlining),
        ),
        (
            "EnableStaticMeshSeparation",
            JsonScalar::Boolean(settings.enable_static_mesh_separation),
        ),
        (
            "EnableAssetDeduplication",
            JsonScalar::Boolean(settings.enable_asset_deduplication),
        ),
        (
            "RemoveNullableAttributes",
            JsonScalar::Boolean(settings.remove_nullable_attributes),
        ),
        (
            "PublicizeAssemblies",
            JsonScalar::Boolean(settings.publicize_assemblies),
        ),
        (
            "BundledAssetsExportMode",
            JsonScalar::Integer(settings.bundled_assets_export_mode.discriminant()),
        ),
    ]
}

fn export_settings_members(settings: &ExportSettingsFile) -> [(&'static str, JsonScalar<'_>); 13] {
    [
        (
            "AudioExportFormat",
            JsonScalar::Integer(settings.audio_export_format.discriminant()),
        ),
        (
            "ImageExportFormat",
            JsonScalar::Integer(settings.image_export_format.discriminant()),
        ),
        (
            "LightmapTextureExportFormat",
            JsonScalar::Integer(settings.lightmap_texture_export_format.discriminant()),
        ),
        (
            "ScriptExportMode",
            JsonScalar::Integer(settings.script_export_mode.discriminant()),
        ),
        (
            "ScriptLanguageVersion",
            JsonScalar::Integer(settings.script_language_version.discriminant()),
        ),
        (
            "ScriptTypesFullyQualified",
            JsonScalar::Boolean(settings.script_types_fully_qualified),
        ),
        (
            "ShaderExportMode",
            JsonScalar::Integer(settings.shader_export_mode.discriminant()),
        ),
        (
            "SpriteExportMode",
            JsonScalar::Integer(settings.sprite_export_mode.discriminant()),
        ),
        (
            "TextExportMode",
            JsonScalar::Integer(settings.text_export_mode.discriminant()),
        ),
        (
            "ExportUnreadableAssets",
            JsonScalar::Boolean(settings.export_unreadable_assets),
        ),
        (
            "PreferOriginalTextureExtension",
            JsonScalar::Boolean(settings.prefer_original_texture_extension),
        ),
        (
            "SaveSettingsToDisk",
            JsonScalar::Boolean(settings.save_settings_to_disk),
        ),
        (
            "LanguageCode",
            settings
                .language_code
                .as_deref()
                .map(JsonScalar::Text)
                .unwrap_or(JsonScalar::Null),
        ),
    ]
}

/// One scalar member value for the fixed settings schema.
enum JsonScalar<'a> {
    Integer(i32),
    Boolean(bool),
    Text(&'a str),
    Null,
}

fn write_record(members: &[(&str, JsonScalar<'_>)]) -> Result<String, SettingsFileError> {
    let mut output = String::new();
    output.push_str("{\r\n");
    write_members(&mut output, members, "  ")?;
    output.push('}');
    Ok(output)
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
    write_members(output, members, "    ")?;
    output.push_str("  }");
    if !is_last {
        output.push(',');
    }
    output.push_str("\r\n");
    Ok(())
}

fn write_members(
    output: &mut String,
    members: &[(&str, JsonScalar<'_>)],
    indentation: &str,
) -> Result<(), SettingsFileError> {
    for (index, (member, value)) in members.iter().enumerate() {
        output.push_str(indentation);
        output.push('\"');
        output.push_str(member);
        output.push_str("\": ");
        write_scalar(output, value)?;
        if index + 1 < members.len() {
            output.push(',');
        }
        output.push_str("\r\n");
    }
    Ok(())
}

fn write_scalar(output: &mut String, value: &JsonScalar<'_>) -> Result<(), SettingsFileError> {
    match value {
        JsonScalar::Integer(number) => output.push_str(&number.to_string()),
        JsonScalar::Boolean(flag) => output.push_str(if *flag { "true" } else { "false" }),
        JsonScalar::Text(text) => {
            push_json_string(output, text).map_err(|_| SettingsFileError::AllocationFailed {
                resource: "settings file text",
            })?
        }
        JsonScalar::Null => output.push_str("null"),
    }
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
    let value = parse_json(
        text,
        JsonLimits {
            max_string_bytes: limits.max_string_bytes,
            max_nesting_depth: limits.max_nesting_depth,
        },
    )
    .map_err(map_json_error)?;
    let root = as_object(&value, "$")?;
    let import = map_import(as_object(lookup(root, "Import", "$")?, "$.Import")?, limits)?;
    let processing = map_processing(
        as_object(lookup(root, "Processing", "$")?, "$.Processing")?,
        limits,
    )?;
    let export = map_export(as_object(lookup(root, "Export", "$")?, "$.Export")?, limits)?;
    Ok(SerializedSettingsFile {
        import,
        processing,
        export,
    })
}

/// Deserialize one `JsonDataInstance<ImportSettings>.Text` value.
///
/// Invalid input is returned to the caller so the configuration layer can apply the source
/// serializer's `CreateNew` fallback. An empty string directly creates the default record, as
/// `JsonDataSerializer.Deserialize` does.
pub fn parse_import_settings_text(
    text: &str,
    limits: SettingsFileLimits,
) -> Result<ImportSettingsFile, SettingsFileError> {
    if text.is_empty() {
        return Ok(ImportSettingsFile::from_defaults(ImportDefaults::default()));
    }
    let value = parse_settings_record(text, limits)?;
    map_import(as_object(&value, "$")?, limits)
}

/// Deserialize one `JsonDataInstance<ProcessingSettings>.Text` value.
pub fn parse_processing_settings_text(
    text: &str,
    limits: SettingsFileLimits,
) -> Result<ProcessingSettingsFile, SettingsFileError> {
    if text.is_empty() {
        return Ok(ProcessingSettingsFile::from(ProcessingDefaults::default()));
    }
    let value = parse_settings_record(text, limits)?;
    map_processing(as_object(&value, "$")?, limits)
}

/// Deserialize one `JsonDataInstance<ExportSettings>.Text` value.
pub fn parse_export_settings_text(
    text: &str,
    limits: SettingsFileLimits,
) -> Result<ExportSettingsFile, SettingsFileError> {
    if text.is_empty() {
        return Ok(ExportSettingsFile::from(ExportDefaults::default()));
    }
    let value = parse_settings_record(text, limits)?;
    map_export(as_object(&value, "$")?, limits)
}

fn parse_settings_record(
    text: &str,
    limits: SettingsFileLimits,
) -> Result<JsonValue, SettingsFileError> {
    let text = strip_leading_bom(text);
    if text.len() > limits.max_file_bytes {
        return Err(SettingsFileError::InputTooLong {
            byte_count: text.len(),
            limit: limits.max_file_bytes,
        });
    }
    parse_json(
        text,
        JsonLimits {
            max_string_bytes: limits.max_string_bytes,
            max_nesting_depth: limits.max_nesting_depth,
        },
    )
    .map_err(map_json_error)
}

/// Strip one leading UTF-8 BOM, mirroring the `File.ReadAllText` decoding the source
/// deserializer sees. Only a prefix is stripped; interior zero-width no-break spaces
/// keep their normal string handling on both sides.
fn strip_leading_bom(text: &str) -> &str {
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

/// Map the shared JSON reader's errors onto the settings-file error surface so callers
/// keep a single error type. `StringTooLong` is a bound failure, not a syntax failure.
fn map_json_error(error: JsonError) -> SettingsFileError {
    match error {
        JsonError::InvalidJson { path, reason } => SettingsFileError::InvalidJson { path, reason },
        JsonError::StringTooLong { byte_count, limit } => {
            SettingsFileError::InputTooLong { byte_count, limit }
        }
    }
}

/// Object accessor that keeps the settings-file error type.
fn as_object<'a>(
    value: &'a JsonValue,
    path: &str,
) -> Result<&'a [(String, JsonValue)], SettingsFileError> {
    value.as_object(path).map_err(map_json_error)
}

/// Boolean accessor that keeps the settings-file error type.
fn as_bool(value: &JsonValue, path: &str) -> Result<bool, SettingsFileError> {
    value.as_bool(path).map_err(map_json_error)
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
        settings.enable_prefab_outlining = as_bool(value, "$.Processing.EnablePrefabOutlining")?;
    }
    if let Some(value) = find_member(members, "EnableStaticMeshSeparation") {
        settings.enable_static_mesh_separation =
            as_bool(value, "$.Processing.EnableStaticMeshSeparation")?;
    }
    if let Some(value) = find_member(members, "EnableAssetDeduplication") {
        settings.enable_asset_deduplication =
            as_bool(value, "$.Processing.EnableAssetDeduplication")?;
    }
    if let Some(value) = find_member(members, "RemoveNullableAttributes") {
        settings.remove_nullable_attributes =
            as_bool(value, "$.Processing.RemoveNullableAttributes")?;
    }
    if let Some(value) = find_member(members, "PublicizeAssemblies") {
        settings.publicize_assemblies = as_bool(value, "$.Processing.PublicizeAssemblies")?;
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
            as_bool(value, "$.Export.ScriptTypesFullyQualified")?;
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
        settings.export_unreadable_assets = as_bool(value, "$.Export.ExportUnreadableAssets")?;
    }
    if let Some(value) = find_member(members, "PreferOriginalTextureExtension") {
        settings.prefer_original_texture_extension =
            as_bool(value, "$.Export.PreferOriginalTextureExtension")?;
    }
    if let Some(value) = find_member(members, "SaveSettingsToDisk") {
        settings.save_settings_to_disk = as_bool(value, "$.Export.SaveSettingsToDisk")?;
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

#[cfg(test)]
mod tests {
    use super::{
        parse_export_settings_text, parse_import_settings_text, parse_processing_settings_text,
        parse_settings_file, settings_log_messages, write_export_settings_text,
        write_import_settings_text, write_processing_settings_text, write_settings_file,
        ExportSettingsFile, ImportSettingsFile, ProcessingSettingsFile, SerializedSettingsFile,
        SettingsFileError, SettingsFileLimits, VersionText, DEFAULT_UNITY_VERSION_TEXT,
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

    const IMPORT_DATA_TEXT: &str = "{\r\n  \"ScriptContentLevel\": 2,\r\n  \"StreamingAssetsMode\": 1,\r\n  \"DefaultVersion\": \"0.0.0a0\",\r\n  \"TargetVersion\": \"0.0.0a0\"\r\n}";
    const PROCESSING_DATA_TEXT: &str = "{\r\n  \"EnablePrefabOutlining\": false,\r\n  \"EnableStaticMeshSeparation\": true,\r\n  \"EnableAssetDeduplication\": false,\r\n  \"RemoveNullableAttributes\": false,\r\n  \"PublicizeAssemblies\": false,\r\n  \"BundledAssetsExportMode\": 2\r\n}";
    const EXPORT_DATA_TEXT: &str = "{\r\n  \"AudioExportFormat\": 2,\r\n  \"ImageExportFormat\": 4,\r\n  \"LightmapTextureExportFormat\": 2,\r\n  \"ScriptExportMode\": 1,\r\n  \"ScriptLanguageVersion\": -1,\r\n  \"ScriptTypesFullyQualified\": false,\r\n  \"ShaderExportMode\": 0,\r\n  \"SpriteExportMode\": 0,\r\n  \"TextExportMode\": 2,\r\n  \"ExportUnreadableAssets\": false,\r\n  \"PreferOriginalTextureExtension\": true,\r\n  \"SaveSettingsToDisk\": false,\r\n  \"LanguageCode\": null\r\n}";

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
    fn typed_configuration_data_text_matches_the_source_page_defaults() {
        let settings = SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        );
        assert_eq!(
            write_import_settings_text(&settings.import).unwrap(),
            IMPORT_DATA_TEXT
        );
        assert_eq!(
            write_processing_settings_text(&settings.processing).unwrap(),
            PROCESSING_DATA_TEXT
        );
        assert_eq!(
            write_export_settings_text(&settings.export).unwrap(),
            EXPORT_DATA_TEXT
        );
        assert_eq!(
            parse_import_settings_text(IMPORT_DATA_TEXT, SettingsFileLimits::default()).unwrap(),
            settings.import
        );
        assert_eq!(
            parse_processing_settings_text(PROCESSING_DATA_TEXT, SettingsFileLimits::default())
                .unwrap(),
            settings.processing
        );
        assert_eq!(
            parse_export_settings_text(EXPORT_DATA_TEXT, SettingsFileLimits::default()).unwrap(),
            settings.export
        );
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
    fn decoded_strings_respect_the_byte_bound() {
        // The decoded-string bound kernel now lives in the shared JSON reader; the
        // settings path maps its bound failure onto `InputTooLong`.
        assert!(matches!(
            super::map_json_error(crate::json::JsonError::StringTooLong {
                byte_count: 9,
                limit: 8
            }),
            SettingsFileError::InputTooLong {
                byte_count: 9,
                limit: 8
            }
        ));
        // The production path enforces the same bound: a code exactly at the limit
        // parses, one byte more fails while scanning.
        let limits = SettingsFileLimits::default();
        let at_limit = "a".repeat(limits.max_string_bytes);
        let over_limit = "a".repeat(limits.max_string_bytes + 1);
        for text in [at_limit, over_limit] {
            let input = format!(
                "{{\"Import\": {{}}, \"Processing\": {{}}, \"Export\": {{\"LanguageCode\": \"{text}\"}}}}"
            );
            let parsed = parse_settings_file(&input, limits);
            if text.len() > limits.max_string_bytes {
                assert!(
                    matches!(
                        parsed,
                        Err(SettingsFileError::InputTooLong { limit, .. })
                            if limit == limits.max_string_bytes
                    ),
                    "over-limit string must fail"
                );
            } else {
                assert!(parsed.is_ok(), "at-limit string must parse");
            }
        }
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
