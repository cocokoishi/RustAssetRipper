//! The settings form model (port of `SettingsPage` and its generated `SetProperty`).
//!
//! The frozen product renders one form whose control names are the configuration property
//! names: a checkbox per boolean property, a `<select>` per enum property whose option values
//! are the C# enum member names, and a text input per version property. The post handler sets
//! every boolean from the presence of its key, then walks every submitted pair through
//! `SetProperty`, which parses the enum name, ignores unknown keys, and leaves the enum's
//! zero value in place when a submitted name cannot be parsed.
//!
//! The port keeps the same control names, the same option values, and the same display and
//! description message keys, so a form submitted by either implementation is understood by
//! the other. Two free-edition behaviours are reproduced as well: the
//! `EnableStaticMeshSeparation` and `EnableAssetDeduplication` checkboxes render disabled (so a
//! browser never submits them, and saving clears them), and the `ScriptContentLevel.Level3`
//! and `ShaderExportMode.Decompile` descriptions show the free-edition notice.

use assetripper::export::settings::{
    AudioExportFormat, BundledAssetsExportMode, ImageExportFormat, LightmapTextureExportFormat,
    ScriptContentLevel, ScriptExportMode, ScriptLanguageVersion, ShaderExportMode,
    SpriteExportMode, StreamingAssetsMode, TextExportMode,
};
use assetripper::export::settings_file::{SerializedSettingsFile, SettingsFileLimits, VersionText};
use assetripper::resources::vertex_data_blob::UnityVersion;

/// One `<option>` of a settings dropdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DropdownOption {
    /// The C# enum member name, used as the submitted value.
    pub value: &'static str,
    /// The localization key of the displayed name, or `None` when the dropdown does not
    /// localize its options and shows the enum member name itself (`Enum.GetName`).
    pub display_key: Option<&'static str>,
    /// The localization key of the description paragraph, or `None` when the dropdown has no
    /// description (`GetDescription` returns null).
    pub description_key: Option<&'static str>,
}

/// One settings dropdown: `<select>` name, title key and options in enum-value order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dropdown {
    /// The C# property name used as the control name.
    pub id: &'static str,
    /// The localization key of the title.
    pub title_key: &'static str,
    /// The option list.
    pub options: &'static [DropdownOption],
}

/// The boolean properties of the settings form, in form order.
pub const BOOLEAN_KEYS: &[&str] = &[
    "IgnoreStreamingAssets",
    "EnablePrefabOutlining",
    "EnableStaticMeshSeparation",
    "EnableAssetDeduplication",
    "RemoveNullableAttributes",
    "PublicizeAssemblies",
    "ScriptTypesFullyQualified",
    "ExportUnreadableAssets",
    "PreferOriginalTextureExtension",
    "SaveSettingsToDisk",
];

/// Boolean properties the free edition renders disabled, so saving clears them.
pub const FREE_EDITION_DISABLED_KEYS: &[&str] =
    &["EnableStaticMeshSeparation", "EnableAssetDeduplication"];

/// The text control names the form renders, in page order.
///
/// Only the tests cross-check this list; the form renders each control explicitly.
#[cfg(test)]
pub const TEXT_KEYS: &[&str] = &["DefaultVersion", "TargetVersion"];

/// The localization key of the free-edition notice appended to a disabled label.
pub const PREMIUM_NOTICE_KEY: &str = "premium_feature_notice";

/// The localization key of the description shown for a free-edition-only option.
pub const FREE_EDITION_DESCRIPTION_KEY: &str = "not_available_in_the_free_edition";

const AUDIO_FORMAT_OPTIONS: &[DropdownOption] = &[
    DropdownOption {
        value: "Yaml",
        display_key: Some("audio_format_yaml"),
        description_key: Some("audio_format_yaml_description"),
    },
    DropdownOption {
        value: "Native",
        display_key: Some("audio_format_native"),
        description_key: Some("audio_format_native_description"),
    },
    DropdownOption {
        value: "Default",
        display_key: Some("audio_format_default"),
        description_key: Some("audio_format_default_description"),
    },
    DropdownOption {
        value: "PreferWav",
        display_key: Some("audio_format_force_wav"),
        description_key: Some("audio_format_force_wav_description"),
    },
];

const IMAGE_FORMAT_OPTIONS: &[DropdownOption] = &[
    DropdownOption {
        value: "Bmp",
        display_key: None,
        description_key: Some("image_format_description"),
    },
    DropdownOption {
        value: "Exr",
        display_key: None,
        description_key: Some("image_format_description"),
    },
    DropdownOption {
        value: "Hdr",
        display_key: None,
        description_key: Some("image_format_description"),
    },
    DropdownOption {
        value: "Jpeg",
        display_key: None,
        description_key: Some("image_format_description"),
    },
    DropdownOption {
        value: "Png",
        display_key: None,
        description_key: Some("image_format_description"),
    },
    DropdownOption {
        value: "Tga",
        display_key: None,
        description_key: Some("image_format_description"),
    },
];

const LIGHTMAP_FORMAT_OPTIONS: &[DropdownOption] = &[
    DropdownOption {
        value: "Exr",
        display_key: None,
        description_key: Some("lightmap_texture_format_description"),
    },
    DropdownOption {
        value: "Image",
        display_key: None,
        description_key: Some("lightmap_texture_format_description"),
    },
    DropdownOption {
        value: "Yaml",
        display_key: None,
        description_key: Some("lightmap_texture_format_description"),
    },
];

const BUNDLED_ASSETS_MODE_OPTIONS: &[DropdownOption] = &[
    DropdownOption {
        value: "GroupByAssetType",
        display_key: Some("bundled_assets_export_group_by_asset_type"),
        description_key: Some("bundled_assets_export_group_by_asset_type_description"),
    },
    DropdownOption {
        value: "GroupByBundleName",
        display_key: Some("bundled_assets_export_group_by_bundle_name"),
        description_key: Some("bundled_assets_export_group_by_bundle_name_description"),
    },
    DropdownOption {
        value: "DirectExport",
        display_key: Some("bundled_assets_export_direct_export"),
        description_key: Some("bundled_assets_export_direct_export_description"),
    },
];

const SCRIPT_CONTENT_LEVEL_OPTIONS: &[DropdownOption] = &[
    DropdownOption {
        value: "Level0",
        display_key: Some("script_content_level_0"),
        description_key: Some("script_content_level_0_description"),
    },
    DropdownOption {
        value: "Level1",
        display_key: Some("script_content_level_1"),
        description_key: Some("script_content_level_1_description"),
    },
    DropdownOption {
        value: "Level2",
        display_key: Some("script_content_level_2"),
        description_key: Some("script_content_level_2_description"),
    },
    DropdownOption {
        value: "Level3",
        display_key: Some("script_content_level_3"),
        description_key: Some(FREE_EDITION_DESCRIPTION_KEY),
    },
];

const SCRIPT_EXPORT_MODE_OPTIONS: &[DropdownOption] = &[
    DropdownOption {
        value: "Decompiled",
        display_key: Some("script_export_format_decompiled"),
        description_key: Some("script_export_format_decompiled_description"),
    },
    DropdownOption {
        value: "Hybrid",
        display_key: Some("script_export_format_hybrid"),
        description_key: Some("script_export_format_hybrid_description"),
    },
    DropdownOption {
        value: "DllExportWithRenaming",
        display_key: Some("script_export_format_dll_with_renaming"),
        description_key: Some("not_implemented_yet"),
    },
    DropdownOption {
        value: "DllExportWithoutRenaming",
        display_key: Some("script_export_format_dll_without_renaming"),
        description_key: Some("script_export_format_dll_without_renaming_description"),
    },
];

const SHADER_EXPORT_MODE_OPTIONS: &[DropdownOption] = &[
    DropdownOption {
        value: "Dummy",
        display_key: Some("shader_asset_format_dummy"),
        description_key: Some("shader_asset_format_dummy_description"),
    },
    DropdownOption {
        value: "Yaml",
        display_key: Some("shader_asset_format_yaml"),
        description_key: Some("shader_asset_format_yaml_description"),
    },
    DropdownOption {
        value: "Decompile",
        display_key: Some("shader_asset_format_decompile"),
        description_key: Some(FREE_EDITION_DESCRIPTION_KEY),
    },
];

const SPRITE_EXPORT_MODE_OPTIONS: &[DropdownOption] = &[
    DropdownOption {
        value: "Yaml",
        display_key: Some("sprite_format_yaml"),
        description_key: Some("sprite_format_yaml_description"),
    },
    DropdownOption {
        value: "Native",
        display_key: Some("sprite_format_native"),
        description_key: Some("sprite_format_native_description"),
    },
    DropdownOption {
        value: "Texture2D",
        display_key: Some("sprite_format_texture"),
        description_key: Some("sprite_format_texture_description"),
    },
];

const TEXT_EXPORT_MODE_OPTIONS: &[DropdownOption] = &[
    DropdownOption {
        value: "Bytes",
        display_key: Some("text_asset_format_binary"),
        description_key: Some("text_asset_format_binary_description"),
    },
    DropdownOption {
        value: "Txt",
        display_key: Some("text_asset_format_text"),
        description_key: Some("text_asset_format_text_description"),
    },
    DropdownOption {
        value: "Parse",
        display_key: Some("text_asset_format_parse"),
        description_key: Some("text_asset_format_parse_description"),
    },
];

const SCRIPT_LANGUAGE_VERSION_OPTIONS: &[DropdownOption] = &[
    DropdownOption {
        value: "AutoExperimental",
        display_key: Some("c_sharp_langage_version_config_auto_experimental"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "AutoSafe",
        display_key: Some("c_sharp_langage_version_config_auto_safe"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp1",
        display_key: Some("c_sharp_langage_version_config_1"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp2",
        display_key: Some("c_sharp_langage_version_config_2"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp3",
        display_key: Some("c_sharp_langage_version_config_3"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp4",
        display_key: Some("c_sharp_langage_version_config_4"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp5",
        display_key: Some("c_sharp_langage_version_config_5"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp6",
        display_key: Some("c_sharp_langage_version_config_6"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp7",
        display_key: Some("c_sharp_langage_version_config_7"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp7_1",
        display_key: Some("c_sharp_langage_version_config_7_1"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp7_2",
        display_key: Some("c_sharp_langage_version_config_7_2"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp7_3",
        display_key: Some("c_sharp_langage_version_config_7_3"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp8_0",
        display_key: Some("c_sharp_langage_version_config_8_0"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp9_0",
        display_key: Some("c_sharp_langage_version_config_9_0"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp10_0",
        display_key: Some("c_sharp_langage_version_config_10_0"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp11_0",
        display_key: Some("c_sharp_langage_version_config_11_0"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp12_0",
        display_key: Some("c_sharp_langage_version_config_12_0"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp13_0",
        display_key: Some("c_sharp_langage_version_config_13_0"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp14_0",
        display_key: Some("c_sharp_langage_version_config_14_0"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "CSharp15_0",
        display_key: Some("c_sharp_langage_version_config_15_0"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
    DropdownOption {
        value: "Latest",
        display_key: Some("c_sharp_langage_version_config_latest"),
        description_key: Some("c_sharp_language_version_config_description"),
    },
];

/// Every settings dropdown in the order the form renders them.
pub const DROPDOWNS: &[Dropdown] = &[
    Dropdown {
        id: "BundledAssetsExportMode",
        title_key: "bundled_assets_export_title",
        options: BUNDLED_ASSETS_MODE_OPTIONS,
    },
    Dropdown {
        id: "ScriptContentLevel",
        title_key: "script_content_level_title",
        options: SCRIPT_CONTENT_LEVEL_OPTIONS,
    },
    Dropdown {
        id: "AudioExportFormat",
        title_key: "audio_export_title",
        options: AUDIO_FORMAT_OPTIONS,
    },
    Dropdown {
        id: "ImageExportFormat",
        title_key: "image_export_title",
        options: IMAGE_FORMAT_OPTIONS,
    },
    Dropdown {
        id: "LightmapTextureExportFormat",
        title_key: "lightmap_texture_export_title",
        options: LIGHTMAP_FORMAT_OPTIONS,
    },
    Dropdown {
        id: "SpriteExportMode",
        title_key: "sprite_export_title",
        options: SPRITE_EXPORT_MODE_OPTIONS,
    },
    Dropdown {
        id: "ShaderExportMode",
        title_key: "shader_asset_export_title",
        options: SHADER_EXPORT_MODE_OPTIONS,
    },
    Dropdown {
        id: "TextExportMode",
        title_key: "text_asset_export_title",
        options: TEXT_EXPORT_MODE_OPTIONS,
    },
    Dropdown {
        id: "ScriptLanguageVersion",
        title_key: "script_language_version_title",
        options: SCRIPT_LANGUAGE_VERSION_OPTIONS,
    },
    Dropdown {
        id: "ScriptExportMode",
        title_key: "script_export_title",
        options: SCRIPT_EXPORT_MODE_OPTIONS,
    },
];

/// The C# enum member name of the current value of one dropdown.
pub fn selected_value(settings: &SerializedSettingsFile, id: &str) -> &'static str {
    match id {
        "BundledAssetsExportMode" => match settings.processing.bundled_assets_export_mode {
            BundledAssetsExportMode::GroupByAssetType => "GroupByAssetType",
            BundledAssetsExportMode::GroupByBundleName => "GroupByBundleName",
            BundledAssetsExportMode::DirectExport => "DirectExport",
        },
        "ScriptContentLevel" => match settings.import.script_content_level {
            ScriptContentLevel::Level0 => "Level0",
            ScriptContentLevel::Level1 => "Level1",
            ScriptContentLevel::Level2 => "Level2",
            ScriptContentLevel::Level3 => "Level3",
        },
        "AudioExportFormat" => match settings.export.audio_export_format {
            AudioExportFormat::Yaml => "Yaml",
            AudioExportFormat::Native => "Native",
            AudioExportFormat::Default => "Default",
            AudioExportFormat::PreferWav => "PreferWav",
        },
        "ImageExportFormat" => match settings.export.image_export_format {
            ImageExportFormat::Bmp => "Bmp",
            ImageExportFormat::Exr => "Exr",
            ImageExportFormat::Hdr => "Hdr",
            ImageExportFormat::Jpeg => "Jpeg",
            ImageExportFormat::Png => "Png",
            ImageExportFormat::Tga => "Tga",
        },
        "LightmapTextureExportFormat" => match settings.export.lightmap_texture_export_format {
            LightmapTextureExportFormat::Exr => "Exr",
            LightmapTextureExportFormat::Image => "Image",
            LightmapTextureExportFormat::Yaml => "Yaml",
        },
        "ScriptExportMode" => match settings.export.script_export_mode {
            ScriptExportMode::Decompiled => "Decompiled",
            ScriptExportMode::Hybrid => "Hybrid",
            ScriptExportMode::DllExportWithRenaming => "DllExportWithRenaming",
            ScriptExportMode::DllExportWithoutRenaming => "DllExportWithoutRenaming",
        },
        "SpriteExportMode" => match settings.export.sprite_export_mode {
            SpriteExportMode::Yaml => "Yaml",
            SpriteExportMode::Native => "Native",
            SpriteExportMode::Texture2D => "Texture2D",
        },
        "ShaderExportMode" => match settings.export.shader_export_mode {
            ShaderExportMode::Dummy => "Dummy",
            ShaderExportMode::Yaml => "Yaml",
            ShaderExportMode::Decompile => "Decompile",
        },
        "TextExportMode" => match settings.export.text_export_mode {
            TextExportMode::Bytes => "Bytes",
            TextExportMode::Txt => "Txt",
            TextExportMode::Parse => "Parse",
        },
        "ScriptLanguageVersion" => match settings.export.script_language_version {
            ScriptLanguageVersion::AutoExperimental => "AutoExperimental",
            ScriptLanguageVersion::AutoSafe => "AutoSafe",
            ScriptLanguageVersion::CSharp1 => "CSharp1",
            ScriptLanguageVersion::CSharp2 => "CSharp2",
            ScriptLanguageVersion::CSharp3 => "CSharp3",
            ScriptLanguageVersion::CSharp4 => "CSharp4",
            ScriptLanguageVersion::CSharp5 => "CSharp5",
            ScriptLanguageVersion::CSharp6 => "CSharp6",
            ScriptLanguageVersion::CSharp7 => "CSharp7",
            ScriptLanguageVersion::CSharp7_1 => "CSharp7_1",
            ScriptLanguageVersion::CSharp7_2 => "CSharp7_2",
            ScriptLanguageVersion::CSharp7_3 => "CSharp7_3",
            ScriptLanguageVersion::CSharp8_0 => "CSharp8_0",
            ScriptLanguageVersion::CSharp9_0 => "CSharp9_0",
            ScriptLanguageVersion::CSharp10_0 => "CSharp10_0",
            ScriptLanguageVersion::CSharp11_0 => "CSharp11_0",
            ScriptLanguageVersion::CSharp12_0 => "CSharp12_0",
            ScriptLanguageVersion::CSharp13_0 => "CSharp13_0",
            ScriptLanguageVersion::CSharp14_0 => "CSharp14_0",
            ScriptLanguageVersion::CSharp15_0 => "CSharp15_0",
            ScriptLanguageVersion::Latest => "Latest",
        },
        _ => "",
    }
}

/// The current value of one boolean property.
pub fn boolean_value(settings: &SerializedSettingsFile, key: &str) -> bool {
    match key {
        "IgnoreStreamingAssets" => settings
            .import
            .streaming_assets_mode
            .skip_streaming_assets(),
        "EnablePrefabOutlining" => settings.processing.enable_prefab_outlining,
        "EnableStaticMeshSeparation" => settings.processing.enable_static_mesh_separation,
        "EnableAssetDeduplication" => settings.processing.enable_asset_deduplication,
        "RemoveNullableAttributes" => settings.processing.remove_nullable_attributes,
        "PublicizeAssemblies" => settings.processing.publicize_assemblies,
        "ScriptTypesFullyQualified" => settings.export.script_types_fully_qualified,
        "ExportUnreadableAssets" => settings.export.export_unreadable_assets,
        "PreferOriginalTextureExtension" => settings.export.prefer_original_texture_extension,
        "SaveSettingsToDisk" => settings.export.save_settings_to_disk,
        _ => false,
    }
}

/// The current text of one version property.
pub fn version_text<'a>(settings: &'a SerializedSettingsFile, key: &str) -> &'a str {
    match key {
        "DefaultVersion" => settings.import.default_version.as_str(),
        "TargetVersion" => settings.import.target_version.as_str(),
        _ => "",
    }
}

/// Apply one submitted form, mirroring `HandlePostRequest`.
///
/// Boolean properties are set from the presence of their key, then every submitted pair is
/// applied through the equivalent of `SetProperty`.
pub fn apply_form(settings: &mut SerializedSettingsFile, form: &[(String, String)]) {
    for key in BOOLEAN_KEYS {
        let present = form.iter().any(|(name, _)| name == key);
        set_boolean(settings, key, present);
    }
    for (key, value) in form {
        set_property(settings, key, value);
    }
}

fn set_boolean(settings: &mut SerializedSettingsFile, key: &str, value: bool) {
    match key {
        "IgnoreStreamingAssets" => {
            settings.import.streaming_assets_mode = if value {
                StreamingAssetsMode::Ignore
            } else {
                StreamingAssetsMode::Extract
            };
        }
        "EnablePrefabOutlining" => settings.processing.enable_prefab_outlining = value,
        "EnableStaticMeshSeparation" => settings.processing.enable_static_mesh_separation = value,
        "EnableAssetDeduplication" => settings.processing.enable_asset_deduplication = value,
        "RemoveNullableAttributes" => settings.processing.remove_nullable_attributes = value,
        "PublicizeAssemblies" => settings.processing.publicize_assemblies = value,
        "ScriptTypesFullyQualified" => settings.export.script_types_fully_qualified = value,
        "ExportUnreadableAssets" => settings.export.export_unreadable_assets = value,
        "PreferOriginalTextureExtension" => {
            settings.export.prefer_original_texture_extension = value
        }
        "SaveSettingsToDisk" => settings.export.save_settings_to_disk = value,
        _ => {}
    }
}

/// The `SetProperty` switch: unknown keys are ignored.
///
/// `TryParseEnum<T>` assigns the enum's zero value when the text is not a member name, so an
/// unparseable value selects the enum member whose value is zero. Every dropdown enum here
/// defines that member except `ScriptLanguageVersion`, whose zero is unnamed in the source; that
/// one case keeps the current value instead of taking an unnamed value the port cannot represent.
fn set_property(settings: &mut SerializedSettingsFile, key: &str, value: &str) {
    match key {
        "ScriptContentLevel" => {
            settings.import.script_content_level =
                parse_script_content_level(value).unwrap_or(ScriptContentLevel::Level0);
        }
        "StreamingAssetsMode" => {
            settings.import.streaming_assets_mode =
                parse_streaming_assets_mode(value).unwrap_or(StreamingAssetsMode::Ignore);
        }
        "DefaultVersion" => settings.import.default_version = parse_version_text(value),
        "TargetVersion" => settings.import.target_version = parse_version_text(value),
        "BundledAssetsExportMode" => {
            settings.processing.bundled_assets_export_mode =
                parse_bundled_assets_export_mode(value)
                    .unwrap_or(BundledAssetsExportMode::GroupByAssetType);
        }
        "AudioExportFormat" => {
            settings.export.audio_export_format =
                parse_audio_export_format(value).unwrap_or(AudioExportFormat::Yaml);
        }
        "ImageExportFormat" => {
            settings.export.image_export_format =
                parse_image_export_format(value).unwrap_or(ImageExportFormat::Bmp);
        }
        "LightmapTextureExportFormat" => {
            settings.export.lightmap_texture_export_format =
                parse_lightmap_texture_export_format(value)
                    .unwrap_or(LightmapTextureExportFormat::Exr);
        }
        "ScriptExportMode" => {
            settings.export.script_export_mode =
                parse_script_export_mode(value).unwrap_or(ScriptExportMode::Decompiled);
        }
        "ScriptLanguageVersion" => {
            if let Some(parsed) = parse_script_language_version(value) {
                settings.export.script_language_version = parsed;
            }
        }
        "ShaderExportMode" => {
            settings.export.shader_export_mode =
                parse_shader_export_mode(value).unwrap_or(ShaderExportMode::Dummy);
        }
        "SpriteExportMode" => {
            settings.export.sprite_export_mode =
                parse_sprite_export_mode(value).unwrap_or(SpriteExportMode::Yaml);
        }
        "TextExportMode" => {
            settings.export.text_export_mode =
                parse_text_export_mode(value).unwrap_or(TextExportMode::Bytes);
        }
        "LanguageCode" => settings.export.language_code = Some(value.to_owned()),
        _ => {}
    }
}

/// The enum member named `value`, or `None` when the text names no member.
///
/// The callers apply `TryParseEnum`'s zero-value fallback for `None`.
fn parse_script_content_level(value: &str) -> Option<ScriptContentLevel> {
    match value {
        "Level0" => Some(ScriptContentLevel::Level0),
        "Level1" => Some(ScriptContentLevel::Level1),
        "Level2" => Some(ScriptContentLevel::Level2),
        "Level3" => Some(ScriptContentLevel::Level3),
        _ => None,
    }
}

fn parse_streaming_assets_mode(value: &str) -> Option<StreamingAssetsMode> {
    match value {
        "Ignore" => Some(StreamingAssetsMode::Ignore),
        "Extract" => Some(StreamingAssetsMode::Extract),
        _ => None,
    }
}

fn parse_bundled_assets_export_mode(value: &str) -> Option<BundledAssetsExportMode> {
    match value {
        "GroupByAssetType" => Some(BundledAssetsExportMode::GroupByAssetType),
        "GroupByBundleName" => Some(BundledAssetsExportMode::GroupByBundleName),
        "DirectExport" => Some(BundledAssetsExportMode::DirectExport),
        _ => None,
    }
}

fn parse_audio_export_format(value: &str) -> Option<AudioExportFormat> {
    match value {
        "Yaml" => Some(AudioExportFormat::Yaml),
        "Native" => Some(AudioExportFormat::Native),
        "Default" => Some(AudioExportFormat::Default),
        "PreferWav" => Some(AudioExportFormat::PreferWav),
        _ => None,
    }
}

fn parse_image_export_format(value: &str) -> Option<ImageExportFormat> {
    match value {
        "Bmp" => Some(ImageExportFormat::Bmp),
        "Exr" => Some(ImageExportFormat::Exr),
        "Hdr" => Some(ImageExportFormat::Hdr),
        "Jpeg" => Some(ImageExportFormat::Jpeg),
        "Png" => Some(ImageExportFormat::Png),
        "Tga" => Some(ImageExportFormat::Tga),
        _ => None,
    }
}

fn parse_lightmap_texture_export_format(value: &str) -> Option<LightmapTextureExportFormat> {
    match value {
        "Exr" => Some(LightmapTextureExportFormat::Exr),
        "Image" => Some(LightmapTextureExportFormat::Image),
        "Yaml" => Some(LightmapTextureExportFormat::Yaml),
        _ => None,
    }
}

fn parse_script_export_mode(value: &str) -> Option<ScriptExportMode> {
    match value {
        "Decompiled" => Some(ScriptExportMode::Decompiled),
        "Hybrid" => Some(ScriptExportMode::Hybrid),
        "DllExportWithRenaming" => Some(ScriptExportMode::DllExportWithRenaming),
        "DllExportWithoutRenaming" => Some(ScriptExportMode::DllExportWithoutRenaming),
        _ => None,
    }
}

fn parse_shader_export_mode(value: &str) -> Option<ShaderExportMode> {
    match value {
        "Dummy" => Some(ShaderExportMode::Dummy),
        "Yaml" => Some(ShaderExportMode::Yaml),
        "Decompile" => Some(ShaderExportMode::Decompile),
        _ => None,
    }
}

fn parse_sprite_export_mode(value: &str) -> Option<SpriteExportMode> {
    match value {
        "Yaml" => Some(SpriteExportMode::Yaml),
        "Native" => Some(SpriteExportMode::Native),
        "Texture2D" => Some(SpriteExportMode::Texture2D),
        _ => None,
    }
}

fn parse_text_export_mode(value: &str) -> Option<TextExportMode> {
    match value {
        "Bytes" => Some(TextExportMode::Bytes),
        "Txt" => Some(TextExportMode::Txt),
        "Parse" => Some(TextExportMode::Parse),
        _ => None,
    }
}

/// The C# member names of `ScriptLanguageVersion`, all of which the port maps.
fn parse_script_language_version(value: &str) -> Option<ScriptLanguageVersion> {
    let parsed = match value {
        "AutoExperimental" => ScriptLanguageVersion::AutoExperimental,
        "AutoSafe" => ScriptLanguageVersion::AutoSafe,
        "CSharp1" => ScriptLanguageVersion::CSharp1,
        "CSharp2" => ScriptLanguageVersion::CSharp2,
        "CSharp3" => ScriptLanguageVersion::CSharp3,
        "CSharp4" => ScriptLanguageVersion::CSharp4,
        "CSharp5" => ScriptLanguageVersion::CSharp5,
        "CSharp6" => ScriptLanguageVersion::CSharp6,
        "CSharp7" => ScriptLanguageVersion::CSharp7,
        "CSharp7_1" => ScriptLanguageVersion::CSharp7_1,
        "CSharp7_2" => ScriptLanguageVersion::CSharp7_2,
        "CSharp7_3" => ScriptLanguageVersion::CSharp7_3,
        "CSharp8_0" => ScriptLanguageVersion::CSharp8_0,
        "CSharp9_0" => ScriptLanguageVersion::CSharp9_0,
        "CSharp10_0" => ScriptLanguageVersion::CSharp10_0,
        "CSharp11_0" => ScriptLanguageVersion::CSharp11_0,
        "CSharp12_0" => ScriptLanguageVersion::CSharp12_0,
        "CSharp13_0" => ScriptLanguageVersion::CSharp13_0,
        "CSharp14_0" => ScriptLanguageVersion::CSharp14_0,
        "CSharp15_0" => ScriptLanguageVersion::CSharp15_0,
        "Latest" => ScriptLanguageVersion::Latest,
        // The source leaves the enum's zero value in place for an unparseable name; the port
        // keeps the current selection instead, because that value cannot be represented.
        _ => return None,
    };
    Some(parsed)
}

/// `TryParseUnityVersion` plus `ToString`: an unparseable text becomes the default rendering.
fn parse_version_text(value: &str) -> VersionText {
    if value.is_empty() || UnityVersion::parse(value).is_none() {
        return VersionText::default_rendering();
    }
    VersionText::new(value, SettingsFileLimits::default())
        .unwrap_or_else(|_| VersionText::default_rendering())
}

#[cfg(test)]
mod tests {
    use super::{
        apply_form, boolean_value, selected_value, version_text, BOOLEAN_KEYS, DROPDOWNS,
        FREE_EDITION_DISABLED_KEYS, TEXT_KEYS,
    };
    use assetripper::export::settings::{
        ExportDefaults, ImportDefaults, ProcessingDefaults, ScriptContentLevel,
        StreamingAssetsMode, TextExportMode,
    };
    use assetripper::export::settings_file::SerializedSettingsFile;

    fn settings() -> SerializedSettingsFile {
        SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        )
    }

    #[test]
    fn every_control_name_is_unique_and_every_option_is_well_formed() {
        let mut ids: Vec<&str> = Vec::new();
        for dropdown in DROPDOWNS {
            assert!(!ids.contains(&dropdown.id), "duplicate {}", dropdown.id);
            ids.push(dropdown.id);
            assert!(
                !dropdown.options.is_empty(),
                "{} has no options",
                dropdown.id
            );
            let mut values: Vec<&str> = Vec::new();
            for option in dropdown.options {
                assert!(
                    !values.contains(&option.value),
                    "{} duplicate option {}",
                    dropdown.id,
                    option.value
                );
                values.push(option.value);
                // Both keys are optional: `None` means the dropdown shows the enum name or has
                // no description, so only a present key must be non-empty.
                if let Some(key) = option.display_key {
                    assert!(!key.is_empty());
                }
                if let Some(key) = option.description_key {
                    assert!(!key.is_empty());
                }
            }
        }
        for key in BOOLEAN_KEYS.iter().chain(TEXT_KEYS) {
            assert!(!ids.contains(key), "duplicate control {key}");
            ids.push(key);
        }
        assert_eq!(DROPDOWNS.len(), 10);
        assert_eq!(BOOLEAN_KEYS.len(), 10);
        assert_eq!(TEXT_KEYS.len(), 2);
    }

    #[test]
    fn every_dropdown_text_key_exists_in_the_message_tables() {
        use crate::web::localization::{Messages, FALLBACK_LANGUAGE, MISSING_MARKER_PREFIX};
        let messages = Messages::load().expect("tables load");
        let resolves = |key: &str| {
            let text = messages.text(FALLBACK_LANGUAGE, key);
            assert!(
                !text.starts_with(MISSING_MARKER_PREFIX),
                "the settings form uses the unknown message key {key}"
            );
            !text.is_empty()
        };
        for dropdown in DROPDOWNS {
            assert!(resolves(dropdown.title_key), "{} title", dropdown.id);
            for option in dropdown.options {
                // A `None` display key means the dropdown shows the enum member name itself.
                if let Some(key) = option.display_key {
                    assert!(resolves(key), "{} option {}", dropdown.id, option.value);
                }
                if let Some(key) = option.description_key {
                    assert!(
                        resolves(key),
                        "{} option {} description",
                        dropdown.id,
                        option.value
                    );
                }
            }
        }
    }

    #[test]
    fn the_selected_value_is_one_of_the_rendered_options() {
        let settings = settings();
        for dropdown in DROPDOWNS {
            let selected = selected_value(&settings, dropdown.id);
            assert!(
                dropdown
                    .options
                    .iter()
                    .any(|option| option.value == selected),
                "{} selects {selected:?}, which is not rendered",
                dropdown.id
            );
        }
        assert_eq!(selected_value(&settings, "ImageExportFormat"), "Png");
        assert_eq!(selected_value(&settings, "AudioExportFormat"), "Default");
        assert_eq!(
            selected_value(&settings, "ScriptLanguageVersion"),
            "AutoSafe"
        );
        assert_eq!(selected_value(&settings, "ScriptContentLevel"), "Level2");
        assert_eq!(selected_value(&settings, "TextExportMode"), "Parse");
        assert_eq!(selected_value(&settings, "Nope"), "");
    }

    #[test]
    fn free_edition_boolean_defaults_match_the_source_records() {
        let settings = settings();
        for key in BOOLEAN_KEYS {
            let value = boolean_value(&settings, key);
            let expected = matches!(
                *key,
                "EnableStaticMeshSeparation" | "PreferOriginalTextureExtension"
            );
            assert_eq!(value, expected, "{key}");
        }
        // The two premium-only controls are the ones the free edition renders disabled.
        assert_eq!(
            FREE_EDITION_DISABLED_KEYS,
            &["EnableStaticMeshSeparation", "EnableAssetDeduplication"]
        );
        // Their record values differ, so a disabled control cannot be tied to "renders unchecked
        // because the value is false": `WriteCheckBox` clears `checked` whenever it disables.
        assert!(boolean_value(&settings, "EnableStaticMeshSeparation"));
        assert!(!boolean_value(&settings, "EnableAssetDeduplication"));
        assert_eq!(version_text(&settings, "DefaultVersion"), "0.0.0a0");
        assert_eq!(version_text(&settings, "TargetVersion"), "0.0.0a0");
    }

    #[test]
    fn a_posted_form_sets_every_boolean_from_key_presence() {
        let mut settings = settings();
        let form = vec![
            ("IgnoreStreamingAssets".to_owned(), "true".to_owned()),
            ("SaveSettingsToDisk".to_owned(), "true".to_owned()),
            // The two free-edition-disabled checkboxes are never submitted, so a save clears
            // them, exactly like the baseline.
            ("ScriptContentLevel".to_owned(), "Level1".to_owned()),
            ("ImageExportFormat".to_owned(), "Tga".to_owned()),
            ("TextExportMode".to_owned(), "Txt".to_owned()),
            ("DefaultVersion".to_owned(), "2021.3.16f1".to_owned()),
            ("UnknownKey".to_owned(), "ignored".to_owned()),
        ];
        apply_form(&mut settings, &form);
        assert_eq!(
            settings.import.streaming_assets_mode,
            StreamingAssetsMode::Ignore
        );
        assert!(settings.export.save_settings_to_disk);
        assert!(!settings.processing.enable_static_mesh_separation);
        assert!(!settings.processing.enable_asset_deduplication);
        assert_eq!(
            settings.import.script_content_level,
            ScriptContentLevel::Level1
        );
        assert_eq!(selected_value(&settings, "ImageExportFormat"), "Tga");
        assert_eq!(settings.export.text_export_mode, TextExportMode::Txt);
        assert_eq!(settings.import.default_version.as_str(), "2021.3.16f1");
        // The untouched target version keeps its rendering.
        assert_eq!(settings.import.target_version.as_str(), "0.0.0a0");
    }

    #[test]
    fn unparseable_values_fall_back_like_try_parse_helpers() {
        let mut settings = settings();
        apply_form(
            &mut settings,
            &[
                ("AudioExportFormat".to_owned(), "NotAFormat".to_owned()),
                ("TargetVersion".to_owned(), "not a version".to_owned()),
            ],
        );
        // `TryParseEnum` leaves the enum's zero value, which is `Yaml` for audio.
        assert_eq!(selected_value(&settings, "AudioExportFormat"), "Yaml");
        // `TryParseUnityVersion` falls back to the default rendering.
        assert_eq!(version_text(&settings, "TargetVersion"), "0.0.0a0");
    }

    #[test]
    fn an_unparseable_script_language_version_keeps_the_selection() {
        let mut settings = settings();
        apply_form(
            &mut settings,
            &[("ScriptLanguageVersion".to_owned(), "CSharp99".to_owned())],
        );
        assert_eq!(
            selected_value(&settings, "ScriptLanguageVersion"),
            "AutoSafe"
        );
    }

    #[test]
    fn language_code_pairs_pass_through() {
        let mut settings = settings();
        apply_form(
            &mut settings,
            &[("LanguageCode".to_owned(), "de".to_owned())],
        );
        assert_eq!(settings.export.language_code.as_deref(), Some("de"));
    }
}
