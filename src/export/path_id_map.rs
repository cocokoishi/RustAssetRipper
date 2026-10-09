//! `PathIdMapExporter` ported from the frozen baseline.
//!
//! The fifth post-exporter writes `{AuxiliaryFilesPath}/path_id_map.json`. It walks the
//! `SerializedAssetCollection` objects of `GameBundle.FetchAssetCollections()` in source
//! order and, for each, records every asset that satisfies `IMesh or ITexture or IAudioClip
//! or ITextAsset` (the four "commonly useful" kinds). Each entry carries the asset's
//! `PathID`, its `INamed.Name` when present, and its `ClassName`.
//!
//! The JSON is produced by `System.Text.Json` through a source-generated serializer context
//! with `WriteIndented = true`, so the wire format is not a free choice. The exact layout was
//! measured against the real runtime (net10.0, Windows) with an isolated probe whose output is
//! recorded in `docs/validation-log.md`: two-space indentation, `Environment.NewLine` line
//! breaks (`\r\n` on Windows), a single space after each colon, declaration-order properties
//! (`Files`; `Name` then `Assets`; `PathID`, `Name`, `Type`), `null` for absent strings, `[]`
//! for empty arrays, and no trailing newline. String escaping follows the default
//! `JavaScriptEncoder`, which escapes `"`, `&`, `'`, `+`, `<`, `>`, the backslash, the
//! backtick, every C0 control (with the `\b \t \n \f \r` short forms), DEL, and everything at
//! or above U+0080, using uppercase four-digit `\uXXXX` (with surrogate pairs above U+FFFF).
//!
//! Selection facts (`is ITexture`, `is INamed`, `ClassName`) come from the generated class
//! hierarchy, which lives in the external `AssetRipper.SourceGenerated` package, so the caller
//! supplies them explicitly instead of this module guessing. Textual layout and the asset
//! filter are what the snapshot pins.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// Exact generated-type interface evidence for the class families used by `PathIdMapExporter`.
const GENERATED_TYPE_INTERFACES: &str =
    include_str!("path_id_map_generated_type_interfaces_fixture.txt");

/// The PathIdMap-relevant generated interfaces of one concrete source-generated type.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PathIdMapGeneratedTypeInterfaces {
    pub is_mesh: bool,
    pub is_texture: bool,
    pub is_audio_clip: bool,
    pub is_text_asset: bool,
    pub is_named: bool,
}

impl PathIdMapGeneratedTypeInterfaces {
    pub const fn is_recorded(self) -> bool {
        self.is_mesh || self.is_texture || self.is_audio_clip || self.is_text_asset
    }
}

struct GeneratedTypeInterfaceTable {
    types: HashMap<(i32, &'static str), PathIdMapGeneratedTypeInterfaces>,
    class_ids: HashSet<i32>,
}

fn generated_type_interface_table() -> &'static GeneratedTypeInterfaceTable {
    static TABLE: OnceLock<GeneratedTypeInterfaceTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut types = HashMap::new();
        let mut class_ids = HashSet::new();
        for line in GENERATED_TYPE_INTERFACES.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut columns = line.split('\t');
            assert_eq!(
                columns.next(),
                Some("TYPE"),
                "invalid path-ID type fixture row"
            );
            let class_id = columns
                .next()
                .and_then(|value| value.parse::<i32>().ok())
                .expect("path-ID type fixture class ID is an integer");
            let type_name = columns
                .next()
                .filter(|value| !value.is_empty())
                .expect("path-ID type fixture has a type name");
            let interfaces = columns.next().unwrap_or_default();
            let has = |target: &str| interfaces.split(',').any(|name| name == target);
            let facts = PathIdMapGeneratedTypeInterfaces {
                is_mesh: has("IMesh"),
                is_texture: has("ITexture"),
                is_audio_clip: has("IAudioClip"),
                is_text_asset: has("ITextAsset"),
                is_named: has("INamed"),
            };
            assert!(types.insert((class_id, type_name), facts).is_none());
            class_ids.insert(class_id);
        }
        GeneratedTypeInterfaceTable { types, class_ids }
    })
}

/// Whether this class ID has any generated type implementing a PathIdMap interface.
pub fn has_path_id_map_generated_types(class_id: i32) -> bool {
    generated_type_interface_table()
        .class_ids
        .contains(&class_id)
}

/// Exact source-generated interface membership for one factory-selected concrete type.
pub fn generated_type_interfaces(
    class_id: i32,
    generated_type_name: &str,
) -> Option<PathIdMapGeneratedTypeInterfaces> {
    generated_type_interface_table()
        .types
        .get(&(class_id, generated_type_name))
        .copied()
}

/// One recorded asset in `path_id_map.json`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedAssetInfo {
    /// `SerializedAssetInfo.PathID`.
    pub path_id: i64,
    /// `(asset as INamed)?.Name`.
    pub name: Option<String>,
    /// `asset.ClassName`.
    pub type_name: Option<String>,
}

/// One `SerializedAssetCollection` recorded in `path_id_map.json`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedFileInfo {
    /// `collection.Name` (a nullable string on the model, so it serializes as `null`).
    pub name: Option<String>,
    /// Recorded assets in collection enumeration order.
    pub assets: Vec<SerializedAssetInfo>,
}

/// The root `SerializedGameInfo` written to `path_id_map.json`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SerializedGameInfo {
    /// Serialized collections in `FetchAssetCollections()` order.
    pub files: Vec<SerializedFileInfo>,
}

/// Caller-supplied facts for one asset, standing in for the generated type checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathIdMapAssetFacts<'a> {
    /// `asset is IMesh`.
    pub is_mesh: bool,
    /// `asset is ITexture`.
    pub is_texture: bool,
    /// `asset is IAudioClip`.
    pub is_audio_clip: bool,
    /// `asset is ITextAsset`.
    pub is_text_asset: bool,
    /// `asset is INamed`, so `Name` may be read.
    pub is_named: bool,
    /// The `INamed.Name` value when [`Self::is_named`]; `None` otherwise or when null.
    pub name: Option<&'a str>,
    /// `asset.ClassName`.
    pub class_name: &'a str,
    /// `asset.PathID`.
    pub path_id: i64,
}

impl PathIdMapAssetFacts<'_> {
    /// `asset is IMesh or ITexture or IAudioClip or ITextAsset`.
    pub const fn is_recorded(&self) -> bool {
        self.is_mesh || self.is_texture || self.is_audio_clip || self.is_text_asset
    }

    /// Project the facts into the recorded entry.
    ///
    /// The source reads `(asset as INamed)?.Name`, so a non-`INamed` asset records `null`
    /// even when it has an underlying name field.
    pub fn to_entry(&self) -> SerializedAssetInfo {
        SerializedAssetInfo {
            path_id: self.path_id,
            name: if self.is_named {
                self.name.map(str::to_owned)
            } else {
                None
            },
            type_name: Some(self.class_name.to_owned()),
        }
    }
}

/// Build one `SerializedFileInfo` from a collection name and its asset facts.
///
/// Assets are filtered by [`PathIdMapAssetFacts::is_recorded`] and keep their input order.
pub fn build_serialized_file(
    name: Option<&str>,
    assets: &[PathIdMapAssetFacts<'_>],
) -> SerializedFileInfo {
    SerializedFileInfo {
        name: name.map(str::to_owned),
        assets: assets
            .iter()
            .filter(|asset| asset.is_recorded())
            .map(PathIdMapAssetFacts::to_entry)
            .collect(),
    }
}

/// The line break `System.Text.Json` uses with `WriteIndented = true`, which is
/// `Environment.NewLine` on the running host.
pub const fn json_newline() -> &'static str {
    if cfg!(windows) {
        "\r\n"
    } else {
        "\n"
    }
}

/// Serialize `path_id_map.json` with the host line break.
pub fn serialize_path_id_map(game: &SerializedGameInfo) -> String {
    serialize_path_id_map_with_newline(game, json_newline())
}

/// Serialize `path_id_map.json` with an explicit line break.
pub fn serialize_path_id_map_with_newline(game: &SerializedGameInfo, newline: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str(newline);
    push_indent(1, &mut out);
    out.push_str("\"Files\": ");
    write_files(&game.files, 1, newline, &mut out);
    out.push_str(newline);
    out.push('}');
    out
}

fn write_files(files: &[SerializedFileInfo], level: usize, newline: &str, out: &mut String) {
    if files.is_empty() {
        out.push_str("[]");
        return;
    }
    out.push('[');
    out.push_str(newline);
    for (index, file) in files.iter().enumerate() {
        push_indent(level + 1, out);
        write_file(file, level + 1, newline, out);
        if index + 1 < files.len() {
            out.push(',');
        }
        out.push_str(newline);
    }
    push_indent(level, out);
    out.push(']');
}

fn write_file(file: &SerializedFileInfo, level: usize, newline: &str, out: &mut String) {
    out.push('{');
    out.push_str(newline);
    push_property(
        "Name",
        &write_optional_string(&file.name),
        level + 1,
        newline,
        true,
        out,
    );
    push_indent(level + 1, out);
    out.push_str("\"Assets\": ");
    write_assets(&file.assets, level + 1, newline, out);
    out.push_str(newline);
    push_indent(level, out);
    out.push('}');
}

fn write_assets(assets: &[SerializedAssetInfo], level: usize, newline: &str, out: &mut String) {
    if assets.is_empty() {
        out.push_str("[]");
        return;
    }
    out.push('[');
    out.push_str(newline);
    for (index, asset) in assets.iter().enumerate() {
        push_indent(level + 1, out);
        write_asset(asset, level + 1, newline, out);
        if index + 1 < assets.len() {
            out.push(',');
        }
        out.push_str(newline);
    }
    push_indent(level, out);
    out.push(']');
}

fn write_asset(asset: &SerializedAssetInfo, level: usize, newline: &str, out: &mut String) {
    out.push('{');
    out.push_str(newline);
    push_property(
        "PathID",
        &asset.path_id.to_string(),
        level + 1,
        newline,
        true,
        out,
    );
    push_property(
        "Name",
        &write_optional_string(&asset.name),
        level + 1,
        newline,
        true,
        out,
    );
    push_property(
        "Type",
        &write_optional_string(&asset.type_name),
        level + 1,
        newline,
        false,
        out,
    );
    push_indent(level, out);
    out.push('}');
}

/// Write `"key": value` at `level`, with a trailing comma when `comma` is set.
fn push_property(
    key: &str,
    value: &str,
    level: usize,
    newline: &str,
    comma: bool,
    out: &mut String,
) {
    push_indent(level, out);
    out.push('"');
    out.push_str(key);
    out.push_str("\": ");
    out.push_str(value);
    if comma {
        out.push(',');
    }
    out.push_str(newline);
}

fn push_indent(level: usize, out: &mut String) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

/// Render an optional string the way `System.Text.Json` does: `null` or a quoted string.
fn write_optional_string(value: &Option<String>) -> String {
    match value {
        Some(text) => {
            let mut rendered = String::with_capacity(text.len() + 2);
            rendered.push('"');
            rendered.push_str(&encode_json_string(text));
            rendered.push('"');
            rendered
        }
        None => "null".to_owned(),
    }
}

/// The default `System.Text.Json`/`JavaScriptEncoder` escape for one string's contents.
///
/// Quotes are not added. The escaped set was measured against the real runtime; see the
/// module docs. `\uXXXX` escapes use uppercase hex digits.
pub fn encode_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\u0022"),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{000C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '&' => out.push_str("\\u0026"),
            '\'' => out.push_str("\\u0027"),
            '+' => out.push_str("\\u002B"),
            '<' => out.push_str("\\u003C"),
            '>' => out.push_str("\\u003E"),
            '`' => out.push_str("\\u0060"),
            other => {
                let code = other as u32;
                if (0x20..0x7F).contains(&code) {
                    out.push(other);
                } else {
                    push_unicode_escape(code, &mut out);
                }
            }
        }
    }
    out
}

fn push_unicode_escape(code: u32, out: &mut String) {
    if code <= 0xFFFF {
        out.push_str(&format!("\\u{code:04X}"));
    } else {
        let offset = code - 0x0001_0000;
        let high = 0xD800 + (offset >> 10);
        let low = 0xDC00 + (offset & 0x3FF);
        out.push_str(&format!("\\u{high:04X}\\u{low:04X}"));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_serialized_file, encode_json_string, generated_type_interfaces,
        has_path_id_map_generated_types, serialize_path_id_map_with_newline, PathIdMapAssetFacts,
        SerializedAssetInfo, SerializedFileInfo, SerializedGameInfo,
    };

    const CRLF: &str = "\r\n";

    fn texture<'a>(is_named: bool, name: Option<&'a str>, path_id: i64) -> PathIdMapAssetFacts<'a> {
        PathIdMapAssetFacts {
            is_mesh: false,
            is_texture: true,
            is_audio_clip: false,
            is_text_asset: false,
            is_named,
            name,
            class_name: "Texture2D",
            path_id,
        }
    }

    // The exact bytes produced by the net10.0 `PathIdMapExporter` probe (recorded in
    // docs/validation-log.md). The template uses LF and is lifted to CRLF so the literal
    // survives `.gitattributes` normalization.
    const PROBE_TEMPLATE: &str = r#"{
  "Files": [
    {
      "Name": "bundle_a",
      "Assets": [
        {
          "PathID": 1,
          "Name": "MeshA",
          "Type": "Mesh"
        },
        {
          "PathID": -9223372036854775808,
          "Name": null,
          "Type": "Texture2D"
        },
        {
          "PathID": 12,
          "Name": "\u56DE\u308B\u7A7A\u3046\u3055\u304E",
          "Type": "AudioClip"
        }
      ]
    },
    {
      "Name": null,
      "Assets": []
    },
    {
      "Name": "a\u003Cb\u003E\u0026\u0027\u002Bc",
      "Assets": [
        {
          "PathID": 0,
          "Name": "",
          "Type": "TextAsset"
        }
      ]
    }
  ]
}"#;

    #[test]
    fn serialized_bytes_match_the_dotnet_probe() {
        let game = SerializedGameInfo {
            files: vec![
                SerializedFileInfo {
                    name: Some("bundle_a".to_owned()),
                    assets: vec![
                        SerializedAssetInfo {
                            path_id: 1,
                            name: Some("MeshA".to_owned()),
                            type_name: Some("Mesh".to_owned()),
                        },
                        SerializedAssetInfo {
                            path_id: i64::MIN,
                            name: None,
                            type_name: Some("Texture2D".to_owned()),
                        },
                        SerializedAssetInfo {
                            path_id: 12,
                            name: Some("回る空うさぎ".to_owned()),
                            type_name: Some("AudioClip".to_owned()),
                        },
                    ],
                },
                SerializedFileInfo {
                    name: None,
                    assets: Vec::new(),
                },
                SerializedFileInfo {
                    name: Some("a<b>&'+c".to_owned()),
                    assets: vec![SerializedAssetInfo {
                        path_id: 0,
                        name: Some(String::new()),
                        type_name: Some("TextAsset".to_owned()),
                    }],
                },
            ],
        };
        let expected = PROBE_TEMPLATE.replace('\n', CRLF);
        assert_eq!(serialize_path_id_map_with_newline(&game, CRLF), expected);
    }

    #[test]
    fn empty_game_serializes_an_empty_files_array() {
        assert_eq!(
            serialize_path_id_map_with_newline(&SerializedGameInfo::default(), CRLF),
            "{\r\n  \"Files\": []\r\n}"
        );
    }

    #[test]
    fn escaping_matches_the_measured_default_encoder() {
        // Bytes 0x20..=0x7E, then DEL, U+0080, U+00A0, the two line separators, and an
        // astral emoji. This is the second probe's `Name` value verbatim.
        let ascii: String = (0x20u32..=0x7E)
            .map(|code| char::from_u32(code).expect("ascii"))
            .collect();
        let input = format!("{ascii}\u{7F}\u{80}\u{A0}\u{2028}\u{2029}\u{1F600}");
        let expected = " !\\u0022#$%\\u0026\\u0027()*\\u002B,-./0123456789:;\\u003C=\\u003E?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\\\]^_\\u0060abcdefghijklmnopqrstuvwxyz{|}~\\u007F\\u0080\\u00A0\\u2028\\u2029\\uD83D\\uDE00";
        assert_eq!(encode_json_string(&input), expected);
    }

    #[test]
    fn control_characters_use_short_forms_and_uppercase_escapes() {
        let input: String = (0u32..0x20)
            .map(|code| char::from_u32(code).expect("control"))
            .collect();
        let expected = "\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007\\b\\t\\n\\u000B\\f\\r\\u000E\\u000F\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017\\u0018\\u0019\\u001A\\u001B\\u001C\\u001D\\u001E\\u001F";
        assert_eq!(encode_json_string(&input), expected);
    }

    #[test]
    fn only_the_four_commonly_useful_kinds_are_recorded() {
        let assets = [
            PathIdMapAssetFacts {
                is_mesh: false,
                is_texture: false,
                is_audio_clip: false,
                is_text_asset: false,
                is_named: true,
                name: Some("Skip"),
                class_name: "GameObject",
                path_id: 1,
            },
            PathIdMapAssetFacts {
                is_mesh: true,
                is_texture: false,
                is_audio_clip: false,
                is_text_asset: false,
                is_named: true,
                name: Some("MeshA"),
                class_name: "Mesh",
                path_id: 2,
            },
            texture(true, Some("Tex"), 3),
            PathIdMapAssetFacts {
                is_mesh: false,
                is_texture: false,
                is_audio_clip: true,
                is_text_asset: false,
                is_named: true,
                name: Some("Clip"),
                class_name: "AudioClip",
                path_id: 4,
            },
            PathIdMapAssetFacts {
                is_mesh: false,
                is_texture: false,
                is_audio_clip: false,
                is_text_asset: true,
                is_named: true,
                name: Some("Text"),
                class_name: "TextAsset",
                path_id: 5,
            },
        ];
        let file = build_serialized_file(Some("coll"), &assets);
        assert_eq!(file.name.as_deref(), Some("coll"));
        let path_ids: Vec<i64> = file.assets.iter().map(|entry| entry.path_id).collect();
        assert_eq!(path_ids, [2, 3, 4, 5]);
    }

    #[test]
    fn unnamed_assets_record_a_null_name() {
        // is_named controls the `(asset as INamed)?.Name` cast; a non-INamed asset gets null
        // even when a name string happens to be supplied.
        let named = texture(true, Some("Has"), 1);
        let unnamed = texture(false, Some("Ignored"), 2);
        assert_eq!(named.to_entry().name.as_deref(), Some("Has"));
        assert_eq!(unnamed.to_entry().name, None);
        assert_eq!(named.to_entry().type_name.as_deref(), Some("Texture2D"));
    }

    #[test]
    fn generated_type_facts_preserve_the_shader_text_asset_version_boundary() {
        let legacy = generated_type_interfaces(48, "Shader_5")
            .expect("Unity 5 Shader type is in the frozen generated assembly");
        assert!(legacy.is_text_asset);
        assert!(legacy.is_named);
        let current = generated_type_interfaces(48, "Shader_5_5")
            .expect("Unity 5.5 Shader type is in the frozen generated assembly");
        assert!(!current.is_text_asset);
        assert!(current.is_named);
        assert!(!current.is_recorded());

        let legacy_include = generated_type_interfaces(109, "CGProgram_2020")
            .expect("Unity 2020 CGProgram type is in the frozen generated assembly");
        assert!(legacy_include.is_text_asset);
        let newer_include = generated_type_interfaces(109, "ShaderInclude_2021_2_0_a8")
            .expect("Unity 2021 ShaderInclude type is in the frozen generated assembly");
        assert!(!newer_include.is_text_asset);
        assert!(has_path_id_map_generated_types(109));
        assert!(!has_path_id_map_generated_types(1));
    }

    #[test]
    fn path_id_map_fixture_covers_every_source_factory_type_in_its_class_families() {
        let fixture = include_str!("../processing/source_factory_dispatch_fixture.txt");
        let mut checked = 0usize;
        for line in fixture.lines().filter(|line| line.starts_with("GEN|")) {
            let mut columns = line.split('|');
            assert_eq!(columns.next(), Some("GEN"));
            let class_id = columns
                .next()
                .and_then(|value| value.parse::<i32>().ok())
                .expect("source factory row has a class ID");
            let _version = columns
                .next()
                .expect("source factory row has a Unity version");
            let _version_hex = columns
                .next()
                .expect("source factory row has a version key");
            let type_name = columns
                .next()
                .expect("source factory row has a generated type");
            if !has_path_id_map_generated_types(class_id) || type_name.starts_with("THROW:") {
                continue;
            }
            assert!(
                generated_type_interfaces(class_id, type_name).is_some(),
                "source factory generated type {class_id}:{type_name} is missing from the path-ID interface map"
            );
            checked += 1;
        }
        assert_eq!(
            checked, 309,
            "fixture covers {checked} source factory-generated types in PathIdMap class families"
        );
    }
}
