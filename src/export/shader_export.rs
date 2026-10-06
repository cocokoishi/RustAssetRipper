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
//!   snapshot facts (Color 0, Vector 1, Single 2, Range 3, Texture 4). Template bodies
//!   resolve through the manifest resource paths below, and requirement names are
//!   non-empty like the source constructor guard.
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

/// `RequiredProperty` construction rejects null or empty names
/// (`ArgumentException.ThrowIfNullOrEmpty`); there is no null in Rust, so the
/// kernel pins the non-empty half for hand-built catalog entries.
pub const fn template_requirement_is_valid(name: &str) -> bool {
    !name.is_empty()
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

/// Manifest resource prefix of `TemplateList.LoadTemplates`
/// (`ShaderTemplatePrefix`).
pub const TEMPLATE_RESOURCE_PREFIX: &str = "AssetRipper.Export.UnityProjects.Shaders.Templates.";
/// Template body resource extension (`ShaderTemplateExtension`).
pub const TEMPLATE_RESOURCE_EXTENSION: &str = ".txt";
/// The catalog JSON resource path (`TemplatesJsonPath`).
pub const TEMPLATES_JSON_PATH: &str =
    "AssetRipper.Export.UnityProjects.Shaders.Templates.Templates.json";

/// Template body lookup: `ShaderTemplatePrefix + template name + extension`.
/// Resource loading itself stays caller-side; the bodies are vendored here.
pub fn template_resource_path(template_name: &str) -> String {
    let mut path = String::with_capacity(
        TEMPLATE_RESOURCE_PREFIX.len() + template_name.len() + TEMPLATE_RESOURCE_EXTENSION.len(),
    );
    path.push_str(TEMPLATE_RESOURCE_PREFIX);
    path.push_str(template_name);
    path.push_str(TEMPLATE_RESOURCE_EXTENSION);
    path
}

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
/// Only the keyword selection is pinned here; value rendering lives in
/// [`render_parsed_property`] with invariant float formatting.
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

/// A `.shader` text export failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShaderTextExportError {
    /// The field view could not be built.
    View(crate::resources::shader::ShaderViewError),
    /// The shader has neither script text nor a parsed form, so neither the `Simple`
    /// nor the dummy path applies.
    MissingScriptAndParsedForm,
    /// A serialized property has an unmapped type number.
    UnsupportedPropertyType {
        /// The raw `SerializedPropertyType` discriminant.
        value: i32,
    },
    /// A texture property has an unmapped dimension.
    UnsupportedTextureDimension {
        /// The raw `TexDim` value.
        value: i32,
    },
    /// No template body matches the best-template name.
    MissingTemplateBody {
        /// The template name.
        name: String,
    },
    /// The script has no `SubShader`, so the unparsed dummy path fails like the source
    /// `subshaderIndex < 0` return.
    MissingSubShader,
    /// `ShaderExportMode::Yaml` produces a YAML asset, not `.shader` text.
    YamlModeWritesAsset,
}

impl core::fmt::Display for ShaderTextExportError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::View(error) => write!(formatter, "{error}"),
            Self::MissingScriptAndParsedForm => write!(
                formatter,
                "shader has neither script text nor a parsed form"
            ),
            Self::UnsupportedPropertyType { value } => write!(
                formatter,
                "serialized shader property type {value} isn't supported"
            ),
            Self::UnsupportedTextureDimension { value } => {
                write!(formatter, "texture dimension {value} isn't supported")
            }
            Self::MissingTemplateBody { name } => {
                write!(formatter, "no template body for '{name}'")
            }
            Self::MissingSubShader => write!(
                formatter,
                "shader script has no SubShader for the dummy path"
            ),
            Self::YamlModeWritesAsset => write!(
                formatter,
                "ShaderExportMode::Yaml writes a YAML asset, not .shader text"
            ),
        }
    }
}

impl std::error::Error for ShaderTextExportError {}

impl From<crate::resources::shader::ShaderViewError> for ShaderTextExportError {
    fn from(error: crate::resources::shader::ShaderViewError) -> Self {
        Self::View(error)
    }
}

/// A parsed shader property and form live in `crate::resources::shader` next to the
/// field view that decodes them; rendering stays here.
pub use crate::resources::shader::{ParsedShaderForm, ParsedShaderProperty};

/// `DummyShaderTextExporter.Export(ISerializedProperty)`: one property line with a
/// two-tab indent, stored attributes plus flag keywords in source order, the
/// `name ("description", type) = default` shape, and invariant float rendering.
pub fn render_parsed_property(
    property: &ParsedShaderProperty,
) -> Result<String, ShaderTextExportError> {
    use crate::export::unity_yaml::format_dotnet_f32;

    let mut line = String::from("\t\t");
    for attribute in &property.attributes {
        line.push_str(&format!("[{attribute}] "));
    }
    const FLAG_KEYWORDS: &[(u32, &str)] = &[
        (1 << 0, "[HideInInspector] "),
        (1 << 1, "[PerRendererData] "),
        (1 << 2, "[NoScaleOffset] "),
        (1 << 3, "[Normal] "),
        (1 << 4, "[HDR] "),
        (1 << 5, "[Gamma] "),
    ];
    for (bit, keyword) in FLAG_KEYWORDS {
        if property.flags & bit != 0 {
            line.push_str(keyword);
        }
    }
    let values = property.default_values;
    let (type_word, default_word) = match property.type_number {
        0 | 1 => (
            "Vector".to_owned(),
            format!(
                "({},{},{},{})",
                format_dotnet_f32(values[0]),
                format_dotnet_f32(values[1]),
                format_dotnet_f32(values[2]),
                format_dotnet_f32(values[3])
            ),
        ),
        2 => ("Float".to_owned(), format_dotnet_f32(values[0])),
        3 => (
            format!(
                "Range({}, {})",
                format_dotnet_f32(values[1]),
                format_dotnet_f32(values[2])
            ),
            format_dotnet_f32(values[0]),
        ),
        4 => (
            texture_dimension_keyword(property.texture_dimension)
                .ok_or(ShaderTextExportError::UnsupportedTextureDimension {
                    value: property.texture_dimension,
                })?
                .to_owned(),
            format!("\"{}\" {{}}", property.default_texture_name),
        ),
        5 => ("Int".to_owned(), format_dotnet_f32(values[0])),
        other => {
            return Err(ShaderTextExportError::UnsupportedPropertyType { value: other });
        }
    };
    line.push_str(&format!(
        "{} (\"{}\", {type_word}) = {default_word}\n",
        property.name, property.description
    ));
    Ok(line)
}

/// `DummyShaderTextExporter.Export(ISerializedProperties)`: the indented
/// `Properties` block.
pub fn render_parsed_properties(
    properties: &[ParsedShaderProperty],
) -> Result<String, ShaderTextExportError> {
    let mut block = String::from("\tProperties {\n");
    for property in properties {
        block.push_str(&render_parsed_property(property)?);
    }
    block.push_str("\t}\n");
    Ok(block)
}

/// The parsed-form dummy body: the best template text for the property facts, or
/// the fallback body when the catalog is empty. Templates match by name against
/// the vendored bodies, since the catalog and body orders differ.
pub fn parsed_dummy_body(form: &ParsedShaderForm) -> Result<String, ShaderTextExportError> {
    let facts: Vec<ShaderPropertyFact<'_>> = form
        .properties
        .iter()
        .map(|property| ShaderPropertyFact {
            name: property.name.as_str(),
            type_number: property.type_number,
        })
        .collect();
    let best = best_template_index(TEMPLATE_CATALOG, &facts).map(|index| TEMPLATE_CATALOG[index].0);
    match best {
        Some(name) => templates::all()
            .into_iter()
            .find(|(template_name, _)| *template_name == name)
            .map(|(_, body)| body)
            .ok_or_else(|| ShaderTextExportError::MissingTemplateBody {
                name: name.to_owned(),
            }),
        None => Ok(fallback_dummy_shader()),
    }
}

/// The parsed-form dummy output: header, rendered properties, marker, best template
/// or fallback body, and the optional fallback and custom-editor trailers.
pub fn export_parsed_dummy(form: &ParsedShaderForm) -> Result<String, ShaderTextExportError> {
    let properties_block = render_parsed_properties(&form.properties)?;
    let body = parsed_dummy_body(form)?;
    Ok(render_parsed_dummy(
        &form.name,
        &properties_block,
        &body,
        &form.fallback_name,
        &form.custom_editor_name,
    ))
}

/// Assemble the unparsed dummy output on raw bytes: the script header up to (not
/// including) the first `SubShader`, then the marker line, one tab, the fallback text,
/// and a closing brace. A missing `SubShader` fails.
///
/// Byte-equal to [`render_unparsed_dummy`] on valid UTF-8; the byte form additionally
/// covers non-UTF-8 scripts without a lossy conversion.
pub fn render_unparsed_dummy_bytes(header: &[u8], fallback: &[u8]) -> Option<Vec<u8>> {
    let index = find_needle(header, crate::resources::shader::SUBSHADER_NEEDLE)?;
    let mut output = Vec::with_capacity(index + fallback.len() + 32);
    output.extend_from_slice(&header[..index]);
    output.extend_from_slice(DUMMY_MARKER_LINE.as_bytes());
    output.push(b'\t');
    output.extend_from_slice(fallback);
    output.push(b'}');
    Some(output)
}

fn find_needle(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// `SimpleShaderExporter.Export` wins plus the mode-exporter fallback, in stack order.
///
/// The stack tries `Simple` first under every mode (decompiled script text wins even
/// under `Yaml`); otherwise `Yaml` writes a YAML asset rather than text, and `Dummy`
/// (which `Decompile` falls back to, since this snapshot has no decompiler exporter)
/// renders the unparsed dummy form or fails explicitly.
pub fn export_shader_text(
    view: &crate::resources::shader::ShaderView,
    mode: crate::export::settings::ShaderExportMode,
) -> Result<Vec<u8>, ShaderTextExportError> {
    let simple_admits = view
        .script
        .as_deref()
        .is_some_and(crate::resources::shader::script_has_decompiled_text);
    match crate::export::exporter_match::select_shader_exporter(mode, simple_admits) {
        crate::export::exporter_match::ShaderExporterChoice::Simple => {
            Ok(view.script.clone().unwrap_or_default())
        }
        crate::export::exporter_match::ShaderExporterChoice::Yaml => {
            Err(ShaderTextExportError::YamlModeWritesAsset)
        }
        crate::export::exporter_match::ShaderExporterChoice::Dummy => {
            if let Some(form) = &view.parsed {
                return Ok(export_parsed_dummy(form)?.into_bytes());
            }
            let Some(script) = view.script.as_deref() else {
                return Err(ShaderTextExportError::MissingScriptAndParsedForm);
            };
            render_unparsed_dummy_bytes(script, fallback_dummy_shader().as_bytes())
                .ok_or(ShaderTextExportError::MissingSubShader)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unity_patches::{patch_file_name, plan_patch_write, PatchWrite};
    use super::{
        best_template_index, export_parsed_dummy, export_shader_text, fallback_dummy_shader,
        property_declaration_keyword, render_parsed_dummy, render_unparsed_dummy,
        render_unparsed_dummy_bytes, requirement_matches, strip_carriage_returns, template_matches,
        template_requirement_is_valid, template_resource_path, templates,
        texture_dimension_keyword, yaml_shader_patch_texts, yaml_shader_patches, ParsedShaderForm,
        ShaderPropertyFact, ShaderTextExportError, TemplateMatchError, TemplateRequirement,
        DUMMY_MARKER_LINE, FILE_LOCKER_PATCH_MIN_VERSION, PROPERTY_FLAG_KEYWORDS,
        REGISTER_PATCH_MIN_VERSION, TEMPLATES_JSON_PATH, TEMPLATE_CATALOG,
        TEMPLATE_RESOURCE_EXTENSION, TEMPLATE_RESOURCE_PREFIX, YAML_SHADER_EXTENSION,
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
    fn template_resources_follow_the_manifest_paths() {
        assert_eq!(
            TEMPLATE_RESOURCE_PREFIX,
            "AssetRipper.Export.UnityProjects.Shaders.Templates."
        );
        assert_eq!(TEMPLATE_RESOURCE_EXTENSION, ".txt");
        assert_eq!(
            TEMPLATES_JSON_PATH,
            "AssetRipper.Export.UnityProjects.Shaders.Templates.Templates.json"
        );
        assert!(TEMPLATES_JSON_PATH.starts_with(TEMPLATE_RESOURCE_PREFIX));
        assert_eq!(
            template_resource_path("SingleTexture"),
            "AssetRipper.Export.UnityProjects.Shaders.Templates.SingleTexture.txt"
        );
        // Catalog order and names mirror Templates.json.
        let names: Vec<&str> = TEMPLATE_CATALOG.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                "Default",
                "SingleTexture",
                "SingleColor",
                "TransparentDiffuse"
            ]
        );
        // Every catalog requirement name passes the constructor guard.
        for (_, requirements) in TEMPLATE_CATALOG {
            for requirement in *requirements {
                assert!(template_requirement_is_valid(requirement.name));
            }
        }
        assert!(!template_requirement_is_valid(""));
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

    /// The probe scripts behind each `shader_dummy_fixture.txt` tag.
    fn fixture_script(tag: &str) -> &'static str {
        match tag {
            "simple" => "Shader \"Test/Simple\" {\n\tSubShader {\n\t\tPass {}\n\t}\n}",
            "nosub" => "Shader \"Test/NoSub\" {\n\t// nothing\n}",
            "program" => {
                "Shader \"Test/Prog\" {\n\tSubShader {\n\t\tProgram \"vp\" {\n\t\t}\n\t}\n}"
            }
            _ => panic!("unexpected fixture tag {tag}"),
        }
    }

    fn unescape_fixture_text(escaped: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(escaped.len());
        let mut chars = escaped.chars();
        while let Some(char) = chars.next() {
            if char != '\\' {
                let mut buffer = [0; 4];
                bytes.extend_from_slice(char.encode_utf8(&mut buffer).as_bytes());
                continue;
            }
            match chars.next() {
                Some('n') => bytes.push(b'\n'),
                Some('t') => bytes.push(b'\t'),
                Some('r') => bytes.push(b'\r'),
                Some('\\') => bytes.push(b'\\'),
                other => panic!("unexpected fixture escape {other:?}"),
            }
        }
        bytes
    }

    /// Byte-for-byte differential against the reference `DummyShaderTextExporter`
    /// (probe `Temp/opencode/shaderexport`): real per-version `Shader.Create` instances
    /// with `Script` set, calling the shipped `ExportShader(IShader, TextWriter`).
    #[test]
    fn dummy_text_matches_the_reference_runtime() {
        use crate::export::settings::ShaderExportMode;
        use crate::resources::shader::ShaderView;

        let fixture = include_str!("shader_dummy_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(6, '|');
            assert_eq!(parts.next(), Some("SEXPORT"), "{line}");
            let version = parts.next().expect("a probe version");
            let tag = parts.next().expect("a probe tag");
            let simple_admits = parts.next().expect("a Simple admission word");
            let dummy_ok = parts.next().expect("a dummy result word");
            let escaped = parts.next().expect("escaped dummy text");
            assert!(parts.next().is_none(), "{line}");
            // The probed versions carry Script without a parsed form.
            assert!(
                version == "3.5.0a0" || version == "5.0.0f1",
                "probed unparsed-path version {version}"
            );
            let script = fixture_script(tag);
            let view = ShaderView {
                name: None,
                script: Some(script.as_bytes().to_vec()),
                has_parsed_form: false,
                parsed: None,
            };
            let admits = simple_admits == "simple-admits=1";
            assert_eq!(
                crate::resources::shader::script_has_decompiled_text(script.as_bytes()),
                admits,
                "{version} {tag}"
            );
            let expected = unescape_fixture_text(escaped);
            // The probe called the dummy exporter directly, bypassing the stack, so
            // the Dummy choice reproduces the probed text (or its explicit failure).
            match dummy_ok {
                "dummy-ok=1" => {
                    assert_eq!(
                        render_unparsed_dummy_bytes(
                            script.as_bytes(),
                            fallback_dummy_shader().as_bytes()
                        ),
                        Some(expected.clone()),
                        "{version} {tag}"
                    );
                }
                "dummy-ok=0" => {
                    assert!(expected.is_empty(), "{version} {tag}");
                    assert_eq!(
                        render_unparsed_dummy_bytes(
                            script.as_bytes(),
                            fallback_dummy_shader().as_bytes()
                        ),
                        None,
                        "{version} {tag}"
                    );
                }
                _ => panic!("unexpected dummy result word {dummy_ok}"),
            }
            // Through the stack, decompiled text wins as raw `Script` bytes under
            // every mode; otherwise Dummy (and Decompile, which falls back to it)
            // renders the probed text and Yaml has no text form.
            if admits {
                for mode in [
                    ShaderExportMode::Dummy,
                    ShaderExportMode::Yaml,
                    ShaderExportMode::Decompile,
                ] {
                    assert_eq!(
                        export_shader_text(&view, mode),
                        Ok(script.as_bytes().to_vec()),
                        "{version} {tag} {mode:?}"
                    );
                }
            } else {
                assert_eq!(
                    export_shader_text(&view, ShaderExportMode::Dummy),
                    Ok(expected.clone()),
                    "{version} {tag}"
                );
                assert_eq!(
                    export_shader_text(&view, ShaderExportMode::Decompile),
                    Ok(expected),
                    "{version} {tag}"
                );
                assert_eq!(
                    export_shader_text(&view, ShaderExportMode::Yaml),
                    Err(ShaderTextExportError::YamlModeWritesAsset),
                    "{version} {tag}"
                );
            }
            rows += 1;
        }
        assert_eq!(rows, 6, "all six probed rows are asserted");
    }

    /// Byte-for-byte differential against the reference parsed-form dummy path
    /// (probe `Temp/opencode/shaderexport`): real 2019.4 `Shader.Create` instances
    /// with a `ParsedForm` built through the generated API, calling the shipped
    /// `DummyShaderTextExporter.ExportShader`. Property lines, template selection,
    /// and fallback/custom-editor trailers must match exactly.
    #[test]
    fn parsed_dummy_text_matches_the_reference_runtime() {
        use crate::export::settings::ShaderExportMode;
        use crate::resources::shader::{ParsedShaderForm, ParsedShaderProperty, ShaderView};

        let fixture = include_str!("shader_parsed_fixture.txt");
        let mut current_tag = String::new();
        let mut form = ParsedShaderForm::default();
        let mut expected: Vec<u8> = Vec::new();
        let mut rows = 0;
        let flush = |tag: &str, form: &ParsedShaderForm, expected: &[u8]| {
            assert!(!expected.is_empty(), "{tag} has no probed output");
            // Direct assembly matches the probed text exactly: the shipped DLL
            // templates agree with the frozen snapshot sources (Default spaced,
            // the other three unspaced), so no normalization applies.
            let probed = String::from_utf8(expected.to_vec()).unwrap();
            // Direct assembly matches the probed text exactly.
            assert_eq!(export_parsed_dummy(form), Ok(probed.clone()), "{tag}");
            // Through the stack, the parsed form wins under Dummy and Decompile
            // (no decompiled script is present); Yaml has no text form.
            let view = ShaderView {
                name: None,
                script: None,
                has_parsed_form: true,
                parsed: Some(form.clone()),
            };
            let expected_bytes = probed.into_bytes();
            assert_eq!(
                export_shader_text(&view, ShaderExportMode::Dummy)
                    .map_err(|error| error.to_string()),
                Ok(expected_bytes.clone()),
                "{tag}"
            );
            assert_eq!(
                export_shader_text(&view, ShaderExportMode::Decompile)
                    .map_err(|error| error.to_string()),
                Ok(expected_bytes),
                "{tag}"
            );
            assert_eq!(
                export_shader_text(&view, ShaderExportMode::Yaml),
                Err(ShaderTextExportError::YamlModeWritesAsset),
                "{tag}"
            );
        };
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(4, '|');
            assert_eq!(parts.next(), Some("SPARSED"), "{line}");
            let tag = parts.next().expect("a probe tag").to_owned();
            if tag != current_tag && !current_tag.is_empty() {
                flush(&current_tag, &form, &expected);
                rows += 1;
                form = ParsedShaderForm::default();
                expected = Vec::new();
            }
            current_tag = tag;
            match parts.next().expect("a row kind") {
                "ok" => assert_eq!(parts.next(), Some("1"), "{line}"),
                "meta" => {
                    let rest = parts.next().expect("meta fields");
                    let mut fields = rest.splitn(3, '|');
                    form.name = fields.next().expect("a shader name").to_owned();
                    form.fallback_name = fields.next().expect("a fallback name").to_owned();
                    form.custom_editor_name =
                        fields.next().expect("a custom editor name").to_owned();
                }
                "prop" => {
                    let rest = parts.next().expect("property fields");
                    let mut fields = rest.split('|');
                    let name = fields.next().expect("a property name").to_owned();
                    let description = fields.next().expect("a description").to_owned();
                    let type_number: i32 = fields.next().expect("a type").parse().unwrap();
                    let mut defaults = [0.0f32; 4];
                    for slot in &mut defaults {
                        *slot = f32::from_bits(
                            u32::from_str_radix(fields.next().expect("default bits"), 16).unwrap(),
                        );
                    }
                    let flags: u32 = fields.next().expect("flags").parse().unwrap();
                    let attributes = fields
                        .next()
                        .expect("attributes")
                        .split(',')
                        .filter(|attribute| !attribute.is_empty())
                        .map(str::to_owned)
                        .collect();
                    let texture_dimension: i32 = fields.next().expect("a texdim").parse().unwrap();
                    let default_texture_name = fields.next().expect("a default name").to_owned();
                    assert!(fields.next().is_none(), "{line}");
                    form.properties.push(ParsedShaderProperty {
                        name,
                        description,
                        attributes,
                        flags,
                        type_number,
                        default_values: defaults,
                        texture_dimension,
                        default_texture_name,
                    });
                }
                "text" => {
                    expected = unescape_fixture_text(parts.next().expect("escaped text"));
                }
                other => panic!("unexpected row kind {other}"),
            }
        }
        flush(&current_tag, &form, &expected);
        rows += 1;
        assert_eq!(rows, 3, "all probed parsed cases are asserted");
    }

    #[test]
    fn shader_text_choices_without_decompiled_text() {
        use crate::export::settings::ShaderExportMode;
        use crate::resources::shader::ShaderView;

        // A compiled script reaches the mode exporter; Yaml has no text form.
        let compiled = ShaderView {
            name: None,
            script: Some(b"Shader \"x\" {\nProgram \"vp\" {}\n}".to_vec()),
            has_parsed_form: false,
            parsed: None,
        };
        assert!(matches!(
            export_shader_text(&compiled, ShaderExportMode::Yaml),
            Err(ShaderTextExportError::YamlModeWritesAsset)
        ));
        // A decoded parsed form renders through the stack like the reference.
        let parsed = ShaderView {
            name: None,
            script: None,
            has_parsed_form: true,
            parsed: Some(ParsedShaderForm {
                name: "x".to_owned(),
                properties: Vec::new(),
                fallback_name: String::new(),
                custom_editor_name: String::new(),
            }),
        };
        assert_eq!(
            export_shader_text(&parsed, ShaderExportMode::Dummy)
                .map(|bytes| String::from_utf8(bytes).unwrap()),
            Ok(
                "Shader \"x\" {\n\tProperties {\n\t}\n\t//DummyShaderTextExporter\n".to_owned()
                    + &templates::all()[0].1
                    + "\n}"
            )
        );
        // Neither script nor parsed form reaches neither text path.
        let empty = ShaderView {
            name: None,
            script: None,
            has_parsed_form: false,
            parsed: None,
        };
        assert_eq!(
            export_shader_text(&empty, ShaderExportMode::Dummy),
            Err(ShaderTextExportError::MissingScriptAndParsedForm)
        );
        // An empty script is not decompiled text and has no SubShader.
        let blank = ShaderView {
            name: None,
            script: Some(Vec::new()),
            has_parsed_form: false,
            parsed: None,
        };
        assert_eq!(
            export_shader_text(&blank, ShaderExportMode::Dummy),
            Err(ShaderTextExportError::MissingSubShader)
        );
    }

    #[test]
    fn byte_and_str_dummy_assembly_agree_on_valid_utf8() {
        let cases = [
            ("Shader \"x\" {\n}\n", "FB"),
            ("Shader \"x\" {\nSubShader {\n}\n}\n", "FB"),
            ("prefix SubShader middle SubShader", "--"),
        ];
        for (script, fallback) in cases {
            assert_eq!(
                render_unparsed_dummy_bytes(script.as_bytes(), fallback.as_bytes()),
                render_unparsed_dummy(script, fallback).map(|text| text.into_bytes()),
                "{script:?}"
            );
        }
        assert_eq!(render_unparsed_dummy_bytes(b"no marker", b"FB"), None);
    }
}
