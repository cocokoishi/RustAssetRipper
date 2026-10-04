//! Shader export text paths ported from the frozen baseline.
//!
//! The importer classes and parsed shader forms come from the external
//! `AssetRipper.SourceGenerated` package, so generated member lists, `Has_*`
//! thresholds, and parsed-form reads stay open. Everything assembled from snapshot text
//! is pinned here:
//!
//! - Collection admission: the base admits every shader; `SimpleShaderExporter` further
//!   requires a script whose text is nonempty and contains neither `Program` nor
//!   `SubProgram` (compared ordinally).
//! - The YAML collection uses the `asset` extension and applies two version-gated Unity
//!   patches before the base export: the register-shader patch at export version 2018.1+
//!   and the file-locker patch at 3.5+. Gates compare `(major, minor, patch)`
//!   lexicographically; prerelease and build metadata semantics of the source version
//!   type stay open.
//! - `UnityPatches.ApplyPatchFromText` writes `{dir}/Assets/Editor/AssetRipperPatches/`
//!   plus `{name}.cs` as UTF-8 bytes, skipping existing files without overwriting.
//! - The fallback dummy shader, both YAML patch texts, and the four template bodies are
//!   vendored verbatim under `shader_data/` (mechanical extraction from the snapshot;
//!   see `SHADER_DATA_PROVENANCE`). Carriage returns are stripped at load exactly like
//!   the source `.Replace("\r", "")` and `GetTextFromResource` paths.
//! - Template selection (`TemplateList`/`TemplateShader`/`RequiredProperty`): empty
//!   requirement lists match everything, otherwise every required `(name, type-number)`
//!   pair needs a same-named, same-numbered shader property; the winner is the match
//!   with the most requirements (first one wins ties). Property-type numbers are
//!   snapshot facts (Color 0, Vector 1, Single 2, Range 3, Texture 4).
//! - Dummy text assembly: without a parsed form, the script header up to (not including)
//!   the first `SubShader` is followed by the marker line, one tab, the fallback text,
//!   and a closing brace; a missing `SubShader` fails. With a parsed form, the header,
//!   rendered properties, marker, best template or fallback, and optional fallback and
//!   custom-editor lines assemble in source order with caller-rendered fragments.

/// Provenance of `shader_data/`: mechanical raw-string extraction from the snapshot
/// shader sources (closing-delimiter dedent rule verified byte-equal against the
/// original-runtime `FallbackDummyShader` value), plus verbatim copies of the template
/// bodies. All files are GPL text from the frozen baseline, like this workspace.
pub const SHADER_DATA_PROVENANCE: &str = "snapshot shader sources, see validation log";

/// The raw fallback dummy shader literal, carriage returns included.
const FALLBACK_RAW: &str = include_str!("shader_data/fallback_raw.txt");
/// The register-shader YAML patch text, verbatim.
const REGISTER_SHADER_PATCH: &str = include_str!("shader_data/register_shader_patch.txt");
/// The file-locker YAML patch text, verbatim.
const FILE_LOCKER_PATCH: &str = include_str!("shader_data/file_locker_patch.txt");

/// Strip carriage returns like the source `.Replace("\r", "")` calls.
pub fn strip_carriage_returns(text: &str) -> String {
    text.replace('\r', "")
}

/// The fallback dummy shader with source carriage returns removed.
pub fn fallback_dummy_shader() -> String {
    strip_carriage_returns(FALLBACK_RAW)
}

/// Template bodies with source carriage returns removed, mirroring `GetTextFromResource`.
pub mod templates {
    use super::strip_carriage_returns;

    /// Resource prefix for the embedded template texts.
    pub const TEMPLATE_PREFIX: &str = "AssetRipper.Export.UnityProjects.Shaders.Templates.";
    /// Resource extension for the embedded template texts.
    pub const TEMPLATE_EXTENSION: &str = ".txt";
    /// The template catalog resource path.
    pub const TEMPLATES_JSON_PATH: &str =
        "AssetRipper.Export.UnityProjects.Shaders.Templates.Templates.json";

    /// `(template name, body)` pairs in catalog order.
    pub fn all() -> [(&'static str, String); 4] {
        [
            (
                "Default",
                strip_carriage_returns(include_str!("shader_data/templates/Default.txt")),
            ),
            (
                "SingleColor",
                strip_carriage_returns(include_str!("shader_data/templates/SingleColor.txt")),
            ),
            (
                "SingleTexture",
                strip_carriage_returns(include_str!("shader_data/templates/SingleTexture.txt")),
            ),
            (
                "TransparentDiffuse",
                strip_carriage_returns(include_str!(
                    "shader_data/templates/TransparentDiffuse.txt"
                )),
            ),
        ]
    }
}

/// `YamlShaderExportCollection` extension override: YAML shaders use `asset`, not `shader`.
pub const YAML_SHADER_EXTENSION: &str = "asset";

/// Minimum export version for the register-shader patch (`ShaderUtil.RegisterShader`).
pub const REGISTER_PATCH_MIN_VERSION: (u32, u32, u32) = (2018, 1, 0);
/// Minimum export version for the file-locker patch (`AssetModificationProcessor`).
pub const FILE_LOCKER_PATCH_MIN_VERSION: (u32, u32, u32) = (3, 5, 0);

/// Which YAML shader patches apply at one export version.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct YamlShaderPatches {
    /// Apply the `YamlShaderPostprocessor` register patch.
    pub register_shader: bool,
    /// Apply the `AvoidSavingYamlShaders` file-locker patch.
    pub file_locker: bool,
}

/// The two `GreaterThanOrEquals` gates of `YamlShaderExportCollection.ExportInner`.
pub fn yaml_shader_patches(version: (u32, u32, u32)) -> YamlShaderPatches {
    YamlShaderPatches {
        register_shader: version >= REGISTER_PATCH_MIN_VERSION,
        file_locker: version >= FILE_LOCKER_PATCH_MIN_VERSION,
    }
}

/// Patch names written by the YAML shader collection.
pub mod yaml_patch_names {
    /// `UnityPatches.ApplyPatchFromText(RegisterShaderUnityPatchText, ...)`.
    pub const REGISTER_SHADER: &str = "YamlShaderPostprocessor";
    /// `UnityPatches.ApplyPatchFromText(FileLockerUnityPatchText, ...)`.
    pub const FILE_LOCKER: &str = "AvoidSavingYamlShaders";
}

/// The patch texts selected by [`yaml_shader_patches`], verbatim.
pub fn yaml_shader_patch_texts(patches: YamlShaderPatches) -> Vec<(&'static str, &'static str)> {
    let mut selected = Vec::new();
    if patches.register_shader {
        selected.push((yaml_patch_names::REGISTER_SHADER, REGISTER_SHADER_PATCH));
    }
    if patches.file_locker {
        selected.push((yaml_patch_names::FILE_LOCKER, FILE_LOCKER_PATCH));
    }
    selected
}

/// `UnityPatches` destination mechanics shared by every patch application.
pub mod unity_patches {
    /// `Assets/Editor/AssetRipperPatches/` below the export directory.
    pub const PATCHES_RELATIVE_DIRECTORY: &str = "Assets/Editor/AssetRipperPatches/";
    /// Patch files are `{name}.cs`.
    pub const PATCH_EXTENSION: &str = "cs";

    /// The `{name}.cs` file name for a patch.
    pub fn patch_file_name(name: &str) -> String {
        format!("{name}.{PATCH_EXTENSION}")
    }

    /// Existing patch files are left alone; only missing ones are written.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum PatchWrite {
        /// Write the UTF-8 patch bytes.
        Write,
        /// The patch file already exists: skip without overwriting.
        SkipExists,
    }

    /// The skip-if-exists rule of `ApplyPatchFromText`.
    pub const fn plan_patch_write(patch_exists: bool) -> PatchWrite {
        if patch_exists {
            PatchWrite::SkipExists
        } else {
            PatchWrite::Write
        }
    }
}

/// `PropertyType` numeric values from the snapshot shader sources.
pub mod property_type_numbers {
    /// `Color = 0`.
    pub const COLOR: i32 = 0;
    /// `Vector = 1`.
    pub const VECTOR: i32 = 1;
    /// `Single = 2`.
    pub const SINGLE: i32 = 2;
    /// `Range = 3`.
    pub const RANGE: i32 = 3;
    /// `Texture = 4`.
    pub const TEXTURE: i32 = 4;
}

/// One template requirement: a property name plus its snapshot type number.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TemplateRequirement {
    /// `PropertyName`, compared ordinally.
    pub name: &'static str,
    /// `PropertyType` numeric value.
    pub type_number: i32,
}

/// The `Templates.json` catalog: template names with their requirement lists, in order.
pub const TEMPLATE_CATALOG: &[(&str, &[TemplateRequirement])] = &[
    ("Default", &[]),
    (
        "SingleTexture",
        &[TemplateRequirement {
            name: "_MainTex",
            type_number: property_type_numbers::TEXTURE,
        }],
    ),
    (
        "SingleColor",
        &[TemplateRequirement {
            name: "_Color",
            type_number: property_type_numbers::COLOR,
        }],
    ),
    (
        "TransparentDiffuse",
        &[
            TemplateRequirement {
                name: "_MainTex",
                type_number: property_type_numbers::TEXTURE,
            },
            TemplateRequirement {
                name: "_Color",
                type_number: property_type_numbers::COLOR,
            },
        ],
    ),
];

/// One shader property fact: its name plus its snapshot type number.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ShaderPropertyFact<'a> {
    /// Property name, compared ordinally.
    pub name: &'a str,
    /// Snapshot `PropertyType` numeric value.
    pub type_number: i32,
}

/// `RequiredProperty.IsMatch`: ordinal name equality plus type-number equality.
pub const fn requirement_matches(
    requirement: TemplateRequirement,
    property: ShaderPropertyFact<'_>,
) -> bool {
    // `&str` equality is ordinal, matching the source `==` on strings.
    requirement.type_number == property.type_number
        && string_equals(requirement.name, property.name)
}

/// Const-compatible ordinal string equality.
const fn string_equals(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// `TemplateShader.IsMatch` against caller-supplied property facts.
///
/// A null requirement list throws on the source side; an empty list matches everything;
/// otherwise every requirement needs at least one matching shader property.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TemplateMatchError {
    /// `RequiredProperties` is null.
    NullRequirements,
}

/// Empty requirement slices match everything; `None` mirrors the throwing null list.
pub fn template_matches(
    requirements: Option<&[TemplateRequirement]>,
    shader_properties: &[ShaderPropertyFact<'_>],
) -> Result<bool, TemplateMatchError> {
    let required = requirements.ok_or(TemplateMatchError::NullRequirements)?;
    if required.is_empty() {
        return Ok(true);
    }
    if shader_properties.is_empty() {
        return Ok(false);
    }
    for requirement in required {
        let matched = shader_properties
            .iter()
            .any(|property| requirement_matches(*requirement, *property));
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `TemplateList.GetBestTemplate`: the index of the matching template with the most
/// requirements, or `None` when nothing matches. Ties keep the earliest template, like
/// the source `MaxBy` over the catalog order.
pub fn best_template_index(
    catalog: &[(&str, &[TemplateRequirement])],
    shader_properties: &[ShaderPropertyFact<'_>],
) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (index, (_, requirements)) in catalog.iter().enumerate() {
        if template_matches(Some(requirements), shader_properties).unwrap_or(false) {
            let count = requirements.len();
            if best.is_none_or(|(_, best_count)| count > best_count) {
                best = Some((index, count));
            }
        }
    }
    best.map(|(index, _)| index)
}

/// The `//DummyShaderTextExporter` marker line written before fallback bodies.
pub const DUMMY_MARKER_LINE: &str = "\t//DummyShaderTextExporter\n";

/// Assemble the no-parsed-form dummy output: the script header up to (not including) the
/// first `SubShader`, then the marker line, one tab, the fallback text, and `}`.
/// A missing `SubShader` fails, mirroring the source `subshaderIndex < 0` return.
pub fn render_unparsed_dummy(script_text: &str, fallback_text: &str) -> Option<String> {
    let index = script_text.find("SubShader")?;
    let mut output = String::with_capacity(index + fallback_text.len() + 32);
    output.push_str(&script_text[..index]);
    output.push_str(DUMMY_MARKER_LINE);
    output.push('\t');
    output.push_str(fallback_text);
    output.push('}');
    Some(output)
}

/// Assemble the parsed-form dummy output from caller-rendered fragments.
///
/// Property-block and template rendering need generated shader objects and stay
/// caller-side; only the header, marker, body-selection, trailing newline, and optional
/// fallback and custom-editor lines are pinned here.
pub fn render_parsed_dummy(
    shader_name: &str,
    properties_block: &str,
    body_text: &str,
    fallback_name: &str,
    custom_editor_name: &str,
) -> String {
    let mut output = String::new();
    output.push_str(&format!("Shader \"{shader_name}\" {{\n"));
    output.push_str(properties_block);
    output.push_str(DUMMY_MARKER_LINE);
    output.push_str(body_text);
    output.push('\n');
    if !fallback_name.is_empty() {
        output.push_str(&format!("\tFallback \"{fallback_name}\"\n"));
    }
    if !custom_editor_name.is_empty() {
        output.push_str(&format!("\t//CustomEditor \"{custom_editor_name}\"\n"));
    }
    output.push('}');
    output
}

/// Declaration keywords of the dummy property serializer, by snapshot type number.
///
/// Value rendering needs caller floats with invariant formatting and stays open; only
/// the keyword selection is pinned.
pub const fn property_declaration_keyword(type_number: i32) -> Option<&'static str> {
    match type_number {
        n if n == property_type_numbers::COLOR || n == property_type_numbers::VECTOR => {
            Some("Vector")
        }
        n if n == property_type_numbers::SINGLE => Some("Float"),
        n if n == property_type_numbers::RANGE => Some("Range"),
        n if n == property_type_numbers::TEXTURE => Some("Texture"),
        _ => None,
    }
}

/// Texture-dimension keywords of the dummy property serializer.
pub const fn texture_dimension_keyword(tex_dim: i32) -> Option<&'static str> {
    match tex_dim {
        1 => Some("any"),
        2 => Some("2D"),
        3 => Some("3D"),
        4 => Some("Cube"),
        5 => Some("2DArray"),
        6 => Some("CubeArray"),
        _ => None,
    }
}

/// Attribute flag names emitted by the dummy property serializer.
///
/// The generated flag bit values stay open, so only the emitted spellings are pinned.
pub const PROPERTY_FLAG_KEYWORDS: &[&str] = &[
    "[HideInInspector] ",
    "[PerRendererData] ",
    "[NoScaleOffset] ",
    "[Normal] ",
    "[HDR] ",
    "[Gamma] ",
];

#[cfg(test)]
mod tests {
    use super::unity_patches::{patch_file_name, plan_patch_write, PatchWrite};
    use super::{
        best_template_index, fallback_dummy_shader, property_declaration_keyword,
        render_parsed_dummy, render_unparsed_dummy, requirement_matches, strip_carriage_returns,
        template_matches, templates, texture_dimension_keyword, yaml_shader_patch_texts,
        yaml_shader_patches, ShaderPropertyFact, TemplateMatchError, TemplateRequirement,
        DUMMY_MARKER_LINE, FILE_LOCKER_PATCH_MIN_VERSION, PROPERTY_FLAG_KEYWORDS,
        REGISTER_PATCH_MIN_VERSION, TEMPLATE_CATALOG, YAML_SHADER_EXTENSION,
    };

    #[test]
    fn yaml_shader_patches_follow_export_version_gates() {
        assert_eq!(REGISTER_PATCH_MIN_VERSION, (2018, 1, 0));
        assert_eq!(FILE_LOCKER_PATCH_MIN_VERSION, (3, 5, 0));
        let both = yaml_shader_patches((2022, 3, 0));
        assert!(both.register_shader && both.file_locker);
        assert_eq!(yaml_shader_patch_texts(both).len(), 2);
        let locker_only = yaml_shader_patches((2017, 4, 0));
        assert!(!locker_only.register_shader && locker_only.file_locker);
        assert_eq!(yaml_shader_patch_texts(locker_only).len(), 1);
        let none = yaml_shader_patches((3, 4, 0));
        assert!(!none.register_shader && !none.file_locker);
        assert!(yaml_shader_patch_texts(none).is_empty());
        // Boundary versions apply their patch.
        assert!(yaml_shader_patches((2018, 1, 0)).register_shader);
        assert!(yaml_shader_patches((3, 5, 0)).file_locker);
        assert_eq!(YAML_SHADER_EXTENSION, "asset");
        // Patch destinations skip existing files without overwriting.
        assert_eq!(
            patch_file_name("YamlShaderPostprocessor"),
            "YamlShaderPostprocessor.cs"
        );
        assert_eq!(plan_patch_write(true), PatchWrite::SkipExists);
        assert_eq!(plan_patch_write(false), PatchWrite::Write);
    }

    #[test]
    fn vendored_texts_match_runtime_values_and_strip_rules() {
        // The fallback strips carriage returns, has no CR left, and matches the probed
        // 317-byte runtime value in length and boundary lines.
        let fallback = fallback_dummy_shader();
        assert!(!fallback.contains('\r'));
        assert_eq!(fallback.len(), 317);
        assert!(fallback.starts_with("\n\tSubShader{\n"));
        assert!(fallback.ends_with("\t}\n"));
        assert!(fallback.contains("#pragma surface surf Lambert\n"));
        // Template bodies strip carriage returns exactly like GetTextFromResource.
        for (name, body) in templates::all() {
            assert!(!body.contains('\r'), "{name}");
            assert!(
                body.contains("SubShader") || body.contains("Shader"),
                "{name}"
            );
        }
        assert_eq!(templates::all().len(), 4);
        // Patch texts are verbatim const literals, CRLF included.
        let both = yaml_shader_patch_texts(yaml_shader_patches((2022, 3, 0)));
        assert!(both[0].1.contains("\r\n"));
        assert!(both[0].1.contains("YamlShaderPostprocessor"));
        assert!(both[1].1.contains("AvoidSavingYamlShaders"));
        assert_eq!(strip_carriage_returns("a\rb\r\nc"), "ab\nc");
    }

    #[test]
    fn template_matching_selects_by_requirement_count() {
        let main_tex = ShaderPropertyFact {
            name: "_MainTex",
            type_number: 4,
        };
        let color = ShaderPropertyFact {
            name: "_Color",
            type_number: 0,
        };
        // Empty requirements match everything; null requirements fail like the source.
        assert_eq!(template_matches(Some(&[]), &[]), Ok(true));
        assert_eq!(
            template_matches(None, &[main_tex]),
            Err(TemplateMatchError::NullRequirements)
        );
        // Requirements need same-named, same-numbered properties.
        let requirement = TemplateRequirement {
            name: "_MainTex",
            type_number: 4,
        };
        assert!(requirement_matches(requirement, main_tex));
        assert!(!requirement_matches(requirement, color));
        assert!(!requirement_matches(
            requirement,
            ShaderPropertyFact {
                name: "_MainTex",
                type_number: 0,
            }
        ));
        // Selection prefers the most requirements: color-only picks SingleColor.
        assert_eq!(
            best_template_index(TEMPLATE_CATALOG, &[color]).map(|i| TEMPLATE_CATALOG[i].0),
            Some("SingleColor")
        );
        // Both properties pick the two-requirement template.
        assert_eq!(
            best_template_index(TEMPLATE_CATALOG, &[main_tex, color])
                .map(|i| TEMPLATE_CATALOG[i].0),
            Some("TransparentDiffuse")
        );
        // No properties match only the empty template.
        assert_eq!(
            best_template_index(TEMPLATE_CATALOG, &[]).map(|i| TEMPLATE_CATALOG[i].0),
            Some("Default")
        );
        // Ties keep the earliest catalog entry.
        let tied: &[(&str, &[TemplateRequirement])] = &[("First", &[]), ("Second", &[])];
        assert_eq!(best_template_index(tied, &[]), Some(0));
    }

    #[test]
    fn dummy_assembly_matches_source_write_order() {
        // A missing SubShader marker fails without writing anything.
        assert_eq!(render_unparsed_dummy("Shader \"x\" {\n}\n", "FB"), None);
        // Otherwise the header prefix, marker, tab, fallback, and brace assemble exactly.
        assert_eq!(
            render_unparsed_dummy("Shader \"x\" {\nSubShader {\n}\n}\n", "FB"),
            Some("Shader \"x\" {\n\t//DummyShaderTextExporter\n\tFB}".to_owned())
        );
        assert_eq!(DUMMY_MARKER_LINE, "\t//DummyShaderTextExporter\n");
        // The parsed-form order pins headers, marker, body, and optional trailers.
        assert_eq!(
            render_parsed_dummy("Lit", "PROPS", "BODY", "Diffuse", "LitGUI"),
            "Shader \"Lit\" {\nPROPS\t//DummyShaderTextExporter\nBODY\n\tFallback \"Diffuse\"\n\t//CustomEditor \"LitGUI\"\n}"
        );
        assert_eq!(
            render_parsed_dummy("Lit", "", "BODY", "", ""),
            "Shader \"Lit\" {\n\t//DummyShaderTextExporter\nBODY\n}"
        );
        // Declaration keywords key off snapshot type numbers; unknown numbers stay open.
        assert_eq!(property_declaration_keyword(0), Some("Vector"));
        assert_eq!(property_declaration_keyword(1), Some("Vector"));
        assert_eq!(property_declaration_keyword(2), Some("Float"));
        assert_eq!(property_declaration_keyword(3), Some("Range"));
        assert_eq!(property_declaration_keyword(4), Some("Texture"));
        assert_eq!(property_declaration_keyword(9), None);
        assert_eq!(texture_dimension_keyword(2), Some("2D"));
        assert_eq!(texture_dimension_keyword(6), Some("CubeArray"));
        assert_eq!(texture_dimension_keyword(7), None);
        assert_eq!(PROPERTY_FLAG_KEYWORDS.len(), 6);
    }
}
