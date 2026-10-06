//! Class-48 `Shader` field view for `.shader` text export.
//!
//! Ports the field reads behind `SimpleShaderExporter` and the unparsed half of
//! `DummyShaderTextExporter.ExportShader`: the `m_Script` text (whose presence is the
//! generated `Has_Script` gate) and the presence of `m_ParsedForm` (the generated
//! `Has_ParsedForm` gate). The reference gates are version-driven (`Shader_3_5`/`Shader_5`
//! carry `Script` without `ParsedForm`; `Shader_2019_3_0_b0` and later carry `ParsedForm`
//! without `Script`); here both are read by field presence because the decoded payload
//! already reflects its own TypeTree.
//!
//! Script bytes stay bytes end to end: the reference writes `Script.Data` verbatim for
//! the `Simple` path and slices the header up to `SubShader` for the dummy path, so no
//! UTF-8 validity is assumed. `m_Name` is retained for diagnostics only.
//!
//! Also ports the serialized-shader render-state predicates
//! (`SerializedShaderFloatValueExtensions`, `SerializedShaderVectorValueExtensions`,
//! `SerializedStencilOpExtensions`, `SerializedShaderStateExtensions`): float/vector
//! zero/max defaults, the `<noninit>` name gate, stencil-op defaults (`Keep`/`Always`),
//! and the alpha-to-mask/conservative/lighting/stencil state words. No callers of these
//! predicates were found under `AssetRipper/Source`; the kernels pin the decision shapes
//! for the render-state emission.
//!
//! Also ports the serialized-pass program selection (`SerializedPassExtensions` with
//! `ShaderType`, plus `SerializedPassType`): the program mask bits, the program-type
//! strings, the ordered program list, and the pass-type words. The program objects and
//! the per-subprogram shader-model versions stay caller-side.
//!
//! Open: `Name` resolution across the `Utf8String`/`FastPropertyName` backings (needs the
//! generated `Has_*` gates; the resolved name arrives as a caller fact), the generic
//! `GetValue<T>` enum-cast machinery (an `i32` identity on the Rust side),
//! name-or-value string emission (float formatting belongs to the export-side invariant
//! renderer), the remaining typed render-state casts (`BlendMode`, `ZTest`, ...),
//! whose discriminants stay in the snapshot enums, and `MaxShaderModelVersion` (needs the
//! subprogram program-type tables).
//!
//! Differential evidence: `shader_dummy_fixture.txt` (probe `Temp/opencode/shaderexport`,
//! real `Shader.Create` instances per version with `Script` set, calling the shipped
//! `DummyShaderTextExporter.ExportShader`).

use std::error::Error;
use std::fmt::{self, Display};

use crate::serialized_file::object::{
    SerializedFieldValue, SerializedValue, SerializedValueLookupError,
};

/// The serialized shader field carrying source text.
pub const SHADER_SCRIPT_FIELD: &str = "m_Script";

/// The serialized shader field carrying the parsed form.
pub const SHADER_PARSED_FORM_FIELD: &str = "m_ParsedForm";

/// The serialized shader name field.
pub const SHADER_NAME_FIELD: &str = "m_Name";

/// The class ID of `Shader` assets.
pub const SHADER_CLASS_ID: i32 = 48;

/// Byte needle terminating the copied header of the unparsed dummy path.
pub const SUBSHADER_NEEDLE: &[u8] = b"SubShader";

/// Byte needles rejecting the `Simple` decompiled-text admission.
pub const PROGRAM_NEEDLE: &[u8] = b"Program";
pub const SUBPROGRAM_NEEDLE: &[u8] = b"SubProgram";

/// A failure while reading a `Shader` field view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShaderViewError {
    /// A struct field could not be looked up.
    FieldLookup {
        /// The field being read.
        field: &'static str,
        /// The lookup failure.
        source: SerializedValueLookupError,
    },
    /// A present field has an unexpected value kind.
    WrongType {
        /// The field being read.
        field: &'static str,
        /// The observed value kind.
        actual: &'static str,
    },
}

impl Display for ShaderViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field, source } => {
                write!(formatter, "could not read Shader field '{field}': {source}")
            }
            Self::WrongType { field, actual } => {
                write!(
                    formatter,
                    "Shader field '{field}' has kind {actual}, which the exporter cannot read here"
                )
            }
        }
    }
}

impl Error for ShaderViewError {}

/// One parsed shader property: `ISerializedProperty` as the dummy exporter reads it.
///
/// Strings are decoded lossily: property text lands in emitted shader source, so
/// undecodable bytes become the replacement character rather than failing the
/// whole export.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedShaderProperty {
    /// `Name`.
    pub name: String,
    /// `Description`.
    pub description: String,
    /// Stored `Attributes` entries, in order.
    pub attributes: Vec<String>,
    /// `Flags` word; bits 0-5 map to the fixed keyword spellings.
    pub flags: u32,
    /// `SerializedPropertyType` discriminant.
    pub type_number: i32,
    /// `DefValue_0_` through `DefValue_3_`.
    pub default_values: [f32; 4],
    /// `DefTexture.TexDim`.
    pub texture_dimension: i32,
    /// `DefTexture.DefaultName`.
    pub default_texture_name: String,
}

/// A parsed shader form: `ISerializedShader` as the dummy exporter reads it.
///
/// The `m_`-prefixed serialized names follow the public class-48 TypeTree layout
/// (`m_ParsedForm` with `m_Name`, `m_PropInfo`/`m_Props`, `m_FallbackName`,
/// `m_CustomEditorName`; each property with `m_Attributes`, `m_Flags`, `m_Name`,
/// `m_Description`, `m_Type`, `m_DefValue_0..3`, and `m_DefTexture` with
/// `m_TexDim`/`m_DefaultName`). No local class-48 TypeTree dump has confirmed them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedShaderForm {
    /// `Name`.
    pub name: String,
    /// `PropInfo.Props`, in order.
    pub properties: Vec<ParsedShaderProperty>,
    /// `FallbackName`.
    pub fallback_name: String,
    /// `CustomEditorName`.
    pub custom_editor_name: String,
}

/// The fields `SimpleShaderExporter` and `DummyShaderTextExporter` read from a shader.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShaderView {
    /// Raw `m_Name` bytes, when present.
    pub name: Option<Vec<u8>>,
    /// Raw `m_Script` bytes, when present.
    pub script: Option<Vec<u8>>,
    /// Whether `m_ParsedForm` is present.
    pub has_parsed_form: bool,
    /// The decoded parsed form, when `m_ParsedForm` is present.
    pub parsed: Option<ParsedShaderForm>,
}

impl ShaderView {
    /// Reads the export-relevant fields from a decoded `Shader` payload.
    ///
    /// Absent `m_Script`/`m_ParsedForm` fields read as none/absent (the 2019+ layouts
    /// drop `m_Script`); a present-but-wrongly-typed field is an explicit error.
    pub fn from_value(root: &SerializedValue) -> Result<Self, ShaderViewError> {
        let parsed = match lookup(root, SHADER_PARSED_FORM_FIELD)? {
            None => None,
            Some(value) => Some(read_parsed_form(value)?),
        };
        Ok(Self {
            name: optional_string(root, SHADER_NAME_FIELD)?,
            script: optional_string(root, SHADER_SCRIPT_FIELD)?,
            has_parsed_form: parsed.is_some(),
            parsed,
        })
    }
}

fn read_parsed_form(value: &SerializedValue) -> Result<ParsedShaderForm, ShaderViewError> {
    let form = as_struct(value, SHADER_PARSED_FORM_FIELD)?;
    let mut properties = Vec::new();
    if let Some(prop_info) = find_field(form, "m_PropInfo")? {
        let info = as_struct(prop_info, "m_PropInfo")?;
        if let Some(props) = find_field(info, "m_Props")? {
            let items = as_array(props, "m_Props")?;
            for item in items {
                properties.push(read_parsed_property(item)?);
            }
        }
    }
    Ok(ParsedShaderForm {
        name: optional_lossy_string(form, "m_Name")?.unwrap_or_default(),
        properties,
        fallback_name: optional_lossy_string(form, "m_FallbackName")?.unwrap_or_default(),
        custom_editor_name: optional_lossy_string(form, "m_CustomEditorName")?.unwrap_or_default(),
    })
}

fn read_parsed_property(value: &SerializedValue) -> Result<ParsedShaderProperty, ShaderViewError> {
    let prop = as_struct(value, "m_Props")?;
    let mut attributes = Vec::new();
    if let Some(list) = find_field(prop, "m_Attributes")? {
        for item in as_array(list, "m_Attributes")? {
            match item {
                SerializedValue::StringBytes(bytes) => {
                    attributes.push(String::from_utf8_lossy(bytes).into_owned());
                }
                other => {
                    return Err(ShaderViewError::WrongType {
                        field: "m_Attributes",
                        actual: serialized_kind(other),
                    });
                }
            }
        }
    }
    let mut default_values = [0.0f32; 4];
    for (index, field) in [
        "m_DefValue_0_",
        "m_DefValue_1_",
        "m_DefValue_2_",
        "m_DefValue_3_",
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(value) = find_field(prop, field)? {
            match value {
                SerializedValue::F32(number) => default_values[index] = *number,
                other => {
                    return Err(ShaderViewError::WrongType {
                        field: "m_DefValue",
                        actual: serialized_kind(other),
                    });
                }
            }
        }
    }
    let (texture_dimension, default_texture_name) = match find_field(prop, "m_DefTexture")? {
        None => (0, String::new()),
        Some(texture) => {
            let fields = as_struct(texture, "m_DefTexture")?;
            (
                optional_i32_in(fields, "m_TexDim")?.unwrap_or(0),
                optional_lossy_string(fields, "m_DefaultName")?.unwrap_or_default(),
            )
        }
    };
    Ok(ParsedShaderProperty {
        name: optional_lossy_string(prop, "m_Name")?.unwrap_or_default(),
        description: optional_lossy_string(prop, "m_Description")?.unwrap_or_default(),
        attributes,
        flags: optional_u32_in(prop, "m_Flags")?.unwrap_or(0),
        type_number: optional_i32_in(prop, "m_Type")?.unwrap_or(0),
        default_values,
        texture_dimension,
        default_texture_name,
    })
}

fn find_field<'a>(
    fields: &'a [SerializedFieldValue],
    name: &'static str,
) -> Result<Option<&'a SerializedValue>, ShaderViewError> {
    let mut matching = fields.iter().filter(|field| field.name == name);
    let Some(first) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        let match_count = fields.iter().filter(|field| field.name == name).count();
        return Err(ShaderViewError::FieldLookup {
            field: name,
            source: SerializedValueLookupError::DuplicateField {
                name: name.to_owned(),
                match_count,
            },
        });
    }
    Ok(Some(&first.value))
}

fn as_struct<'a>(
    value: &'a SerializedValue,
    field: &'static str,
) -> Result<&'a [SerializedFieldValue], ShaderViewError> {
    match value {
        SerializedValue::Struct(fields) => Ok(fields),
        other => Err(ShaderViewError::WrongType {
            field,
            actual: serialized_kind(other),
        }),
    }
}

fn as_array<'a>(
    value: &'a SerializedValue,
    field: &'static str,
) -> Result<&'a [SerializedValue], ShaderViewError> {
    match value {
        SerializedValue::Array(items) => Ok(items),
        other => Err(ShaderViewError::WrongType {
            field,
            actual: serialized_kind(other),
        }),
    }
}

fn optional_lossy_string(
    fields: &[SerializedFieldValue],
    name: &'static str,
) -> Result<Option<String>, ShaderViewError> {
    match find_field(fields, name)? {
        None => Ok(None),
        Some(SerializedValue::StringBytes(bytes)) => {
            Ok(Some(String::from_utf8_lossy(bytes).into_owned()))
        }
        Some(other) => Err(ShaderViewError::WrongType {
            field: "m_Name",
            actual: serialized_kind(other),
        }),
    }
}

fn optional_i32_in(
    fields: &[SerializedFieldValue],
    name: &'static str,
) -> Result<Option<i32>, ShaderViewError> {
    match find_field(fields, name)? {
        None => Ok(None),
        Some(SerializedValue::I32(value)) => Ok(Some(*value)),
        Some(SerializedValue::U32(value)) => Ok(Some(*value as i32)),
        Some(other) => Err(ShaderViewError::WrongType {
            field: "m_Type",
            actual: serialized_kind(other),
        }),
    }
}

fn optional_u32_in(
    fields: &[SerializedFieldValue],
    name: &'static str,
) -> Result<Option<u32>, ShaderViewError> {
    match find_field(fields, name)? {
        None => Ok(None),
        Some(SerializedValue::U32(value)) => Ok(Some(*value)),
        Some(SerializedValue::I32(value)) => Ok(Some(*value as u32)),
        Some(other) => Err(ShaderViewError::WrongType {
            field: "m_Flags",
            actual: serialized_kind(other),
        }),
    }
}

/// `SimpleShaderExporter` admission on raw bytes: nonempty text with neither `Program`
/// nor `SubProgram` inside.
///
/// This agrees with `export::exporter_match::has_decompiled_shader_text` on valid
/// UTF-8; the byte form additionally covers non-UTF-8 scripts without a lossy
/// conversion (an ASCII needle can never match across a lossy boundary).
pub fn script_has_decompiled_text(script: &[u8]) -> bool {
    !script.is_empty()
        && !contains_needle(script, PROGRAM_NEEDLE)
        && !contains_needle(script, SUBPROGRAM_NEEDLE)
}

fn contains_needle(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Name sentinel marking an uninitialized shader state name.
///
/// Mirrors the `"<noninit>"` comparison in `HasName`: a render-state name that is
/// empty or equal to this sentinel counts as nameless.
pub const SHADER_UNINITIALIZED_NAME: &str = "<noninit>";

/// `HasName`: a shader state name is set when it is neither empty nor the
/// uninitialized sentinel.
///
/// Shared by `SerializedShaderFloatValue.HasName` and
/// `SerializedShaderVectorValue.HasName`. The `Name` getter itself (choosing
/// between the `Utf8String` and `FastPropertyName` backings) needs the generated
/// `Has_*` gates and stays caller-side; here the resolved name is a caller fact.
pub fn shader_state_name_is_set(name: &str) -> bool {
    !name.is_empty() && name != SHADER_UNINITIALIZED_NAME
}

/// `SerializedShaderFloatValue.IsZero`: exact float equality with zero
/// (negative zero compares equal, matching the reference `==`).
pub fn shader_float_is_zero(value: f32) -> bool {
    value == 0.0
}

/// `SerializedShaderFloatValue.IsMax`: exact float equality with 255.
pub fn shader_float_is_max(value: f32) -> bool {
    value == 255.0
}

/// `SerializedShaderFloatValue.IsZeroAndNameless`.
pub fn shader_float_is_zero_and_nameless(value: f32, name: &str) -> bool {
    shader_float_is_zero(value) && !shader_state_name_is_set(name)
}

/// `SerializedShaderFloatValue.IsMaxAndNameless`.
pub fn shader_float_is_max_and_nameless(value: f32, name: &str) -> bool {
    shader_float_is_max(value) && !shader_state_name_is_set(name)
}

/// `SerializedShaderFloatValue.IsDefault<T>`: the truncated value equals the
/// expected discriminant and the value is nameless.
///
/// The reference bit-casts `(int)value` to the enum; the Rust side compares the
/// truncated `i32` against the caller-supplied discriminant. Truncation agrees
/// for finite in-`i32`-range inputs, which covers every enum-typed shader state;
/// NaN and out-of-range magnitudes convert differently across runtimes and are
/// out of scope here.
pub fn shader_float_is_default(value: f32, name: &str, default: i32) -> bool {
    value as i32 == default && !shader_state_name_is_set(name)
}

/// `SerializedShaderVectorValue.IsZero`: every lane is exactly zero.
pub fn shader_vector_is_zero(value: [f32; 4]) -> bool {
    value == [0.0, 0.0, 0.0, 0.0]
}

/// `SerializedShaderVectorValue.IsZeroAndNameless`.
pub fn shader_vector_is_zero_and_nameless(value: [f32; 4], name: &str) -> bool {
    shader_vector_is_zero(value) && !shader_state_name_is_set(name)
}

/// `StencilOp.Keep` discriminant (`StencilOp.cs`).
pub const STENCIL_OP_KEEP: i32 = 0;

/// `StencilComp.Always` discriminant (`StencilComp.cs`).
pub const STENCIL_COMP_ALWAYS: i32 = 8;

/// `SerializedStencilOp.IsDefault`: pass, fail, and z-fail are `Keep` and the
/// comparison is `Always`.
///
/// The pass/fail/z-fail/comparison facts are the `GetValue<T>` discriminants as
/// `i32`; the generic enum-cast machinery itself is an identity on the Rust side
/// and stays caller-side.
pub fn stencil_op_is_default(pass: i32, fail: i32, z_fail: i32, comparison: i32) -> bool {
    pass == STENCIL_OP_KEEP
        && fail == STENCIL_OP_KEEP
        && z_fail == STENCIL_OP_KEEP
        && comparison == STENCIL_COMP_ALWAYS
}

/// `StencilRefIsDefault`: the stencil reference is zero and nameless.
pub fn stencil_ref_is_default(value: f32, name: &str) -> bool {
    shader_float_is_zero_and_nameless(value, name)
}

/// Shared `StencilReadMaskIsDefault`/`StencilWriteMaskIsDefault` shape: the mask
/// is max (255) and nameless.
pub fn stencil_mask_is_default(value: f32, name: &str) -> bool {
    shader_float_is_max_and_nameless(value, name)
}

/// `AlphaToMaskValue`: the alpha-to-mask flag is any nonzero integer.
pub fn shader_alpha_to_mask_enabled(value: i32) -> bool {
    value != 0
}

/// `ConservativeValue`: the optional conservative flag is present and nonzero.
pub fn shader_conservative_enabled(value: Option<i32>) -> bool {
    matches!(value, Some(flag) if flag != 0)
}

/// `LightingValue`: the lighting flag renders as `On` or `Off`.
pub fn shader_lighting_word(enabled: bool) -> &'static str {
    if enabled {
        "On"
    } else {
        "Off"
    }
}

/// `StencilIsDefault`: every stencil sub-state is default.
///
/// Mirrors the six-way conjunction in `SerializedShaderStateExtensions`: the
/// reference, the read mask, the write mask, and the three stencil operations.
pub fn shader_stencil_is_default(
    ref_is_default: bool,
    read_mask_is_default: bool,
    write_mask_is_default: bool,
    op_is_default: bool,
    op_front_is_default: bool,
    op_back_is_default: bool,
) -> bool {
    ref_is_default
        && read_mask_is_default
        && write_mask_is_default
        && op_is_default
        && op_front_is_default
        && op_back_is_default
}

/// `ShaderType` discriminants (`Enums/Shader/ShaderType.cs`); `RayTracing`
/// exists from Unity 2019.3.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShaderProgramType {
    /// No program.
    None,
    /// Vertex program.
    Vertex,
    /// Fragment program.
    Fragment,
    /// Geometry program.
    Geometry,
    /// Hull program.
    Hull,
    /// Domain program.
    Domain,
    /// Ray-tracing program.
    RayTracing,
}

impl ShaderProgramType {
    /// The raw discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Vertex => 1,
            Self::Fragment => 2,
            Self::Geometry => 3,
            Self::Hull => 4,
            Self::Domain => 5,
            Self::RayTracing => 6,
        }
    }

    /// `ShaderTypeExtensions.ToProgramMask`: `1 << type`.
    pub const fn to_program_mask(self) -> i32 {
        1 << self.discriminant()
    }

    /// `ShaderTypeExtensions.ToProgramTypeString`: `None` has no program string
    /// and throws on the source side.
    pub const fn to_program_type_string(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Vertex => Some("vp"),
            Self::Fragment => Some("fp"),
            Self::Geometry => Some("gp"),
            Self::Hull => Some("hp"),
            Self::Domain => Some("dp"),
            Self::RayTracing => Some("rtp"),
        }
    }
}

/// `SerializedPassExtensions.HasProgram`: the program's mask bit is set.
pub const fn pass_has_program(program_mask: i32, program: ShaderProgramType) -> bool {
    program_mask & program.to_program_mask() != 0
}

/// `SerializedPassExtensions.GetPrograms` selection order over caller facts:
/// Vertex, Fragment, Geometry, Hull, and Domain programs by mask bit, then the
/// ray-tracing program only when the mask hits and the pass serializes
/// `ProgRayTracing`. The program objects stay caller-side.
pub fn pass_program_types(program_mask: i32, has_prog_ray_tracing: bool) -> Vec<ShaderProgramType> {
    let mut programs = Vec::new();
    for program in [
        ShaderProgramType::Vertex,
        ShaderProgramType::Fragment,
        ShaderProgramType::Geometry,
        ShaderProgramType::Hull,
        ShaderProgramType::Domain,
    ] {
        if pass_has_program(program_mask, program) {
            programs.push(program);
        }
    }
    if has_prog_ray_tracing && pass_has_program(program_mask, ShaderProgramType::RayTracing) {
        programs.push(ShaderProgramType::RayTracing);
    }
    programs
}

/// `SerializedPassType` discriminants.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SerializedPassType {
    /// A regular pass.
    Pass,
    /// A `UsePass` reference.
    UsePass,
    /// A grab pass.
    GrabPass,
}

impl SerializedPassType {
    /// `SerializedPassExtensions.GetType_`: the raw discriminant selects the variant.
    pub const fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Pass),
            1 => Some(Self::UsePass),
            2 => Some(Self::GrabPass),
            _ => None,
        }
    }

    /// `SerializedPassTypeExtensions.ToSerializedString`.
    pub const fn to_serialized_string(self) -> &'static str {
        match self {
            Self::Pass => "Pass",
            Self::UsePass => "UsePass",
            Self::GrabPass => "GrabPass",
        }
    }
}

fn serialized_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "int8",
        SerializedValue::U8(_) => "uint8",
        SerializedValue::I16(_) => "int16",
        SerializedValue::U16(_) => "uint16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "int",
        SerializedValue::U32(_) => "unsigned int",
        SerializedValue::I64(_) => "long",
        SerializedValue::U64(_) => "ulong",
        SerializedValue::F32(_) => "float",
        SerializedValue::F64(_) => "double",
        SerializedValue::StringBytes(_) => "string",
        SerializedValue::Bytes(_) => "typeless data",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::Raw { .. } => "raw data",
    }
}

fn lookup<'a>(
    root: &'a SerializedValue,
    name: &'static str,
) -> Result<Option<&'a SerializedValue>, ShaderViewError> {
    root.unique_field_in_base_chain(name)
        .map_err(|source| ShaderViewError::FieldLookup {
            field: name,
            source,
        })
}

fn optional_string(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<Vec<u8>>, ShaderViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::StringBytes(bytes)) => Ok(Some(bytes.clone())),
        Some(value) => Err(ShaderViewError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn string_value(text: &str) -> SerializedValue {
        SerializedValue::StringBytes(text.as_bytes().to_vec())
    }

    fn shader(script: Option<&str>, parsed_form: bool) -> SerializedValue {
        let mut fields = vec![field("m_Name", string_value("Test/Simple"))];
        if let Some(script) = script {
            fields.push(field("m_Script", string_value(script)));
        }
        if parsed_form {
            fields.push(field("m_ParsedForm", SerializedValue::Struct(Vec::new())));
        }
        SerializedValue::Struct(fields)
    }

    #[test]
    fn view_reads_script_name_and_parsed_form_presence() {
        let view = ShaderView::from_value(&shader(Some("code"), true)).unwrap();
        assert_eq!(view.name, Some(b"Test/Simple".to_vec()));
        assert_eq!(view.script, Some(b"code".to_vec()));
        assert!(view.has_parsed_form);
        let view = ShaderView::from_value(&shader(None, false)).unwrap();
        assert_eq!(view.script, None);
        assert!(!view.has_parsed_form);
    }

    #[test]
    fn non_string_script_is_an_explicit_error() {
        let root = SerializedValue::Struct(vec![field("m_Script", SerializedValue::I32(3))]);
        assert_eq!(
            ShaderView::from_value(&root),
            Err(ShaderViewError::WrongType {
                field: "m_Script",
                actual: "int"
            })
        );
    }

    #[test]
    fn decompiled_text_gate_matches_the_reference_checks() {
        assert!(!script_has_decompiled_text(b""));
        assert!(!script_has_decompiled_text(
            b"Shader \"x\" {\nProgram \"vp\" {\n}\n}"
        ));
        assert!(!script_has_decompiled_text(
            b"Shader \"x\" {\nSubShader {\nSubProgram \"d3d11\" {\n}\n}\n}"
        ));
        // "SubShader" alone does not contain "SubProgram".
        assert!(script_has_decompiled_text(
            b"Shader \"x\" {\nSubShader {\nPass {\n}\n}\n}"
        ));
        // Non-UTF-8 bytes pass through without a lossy conversion.
        assert!(script_has_decompiled_text(&[0xFF, 0xFE, b'a']));
        assert!(!script_has_decompiled_text(b"\xFFProgram"));
    }

    #[test]
    fn byte_gate_agrees_with_the_str_gate_on_valid_utf8() {
        let cases = [
            "",
            "Shader \"x\" {\nSubShader {}\n}",
            "Shader \"x\" {\nProgram \"vp\" {}\n}",
            "SubProgram",
            "PROGRAM",
            "Subshader",
        ];
        for case in cases {
            assert_eq!(
                script_has_decompiled_text(case.as_bytes()),
                crate::export::exporter_match::has_decompiled_shader_text(case),
                "{case:?}"
            );
        }
    }

    #[test]
    fn state_name_gate_rejects_empty_and_uninitialized_names() {
        assert!(shader_state_name_is_set("_MainTex"));
        assert!(shader_state_name_is_set(" "));
        assert!(!shader_state_name_is_set(""));
        assert!(!shader_state_name_is_set(SHADER_UNINITIALIZED_NAME));
        // Ordinal comparison: only the exact sentinel counts as uninitialized.
        assert!(shader_state_name_is_set("<NONINIT>"));
    }

    #[test]
    fn float_zero_max_and_default_predicates_follow_the_reference() {
        assert!(shader_float_is_zero(0.0));
        assert!(shader_float_is_zero(-0.0));
        assert!(!shader_float_is_zero(f32::MIN_POSITIVE));
        assert!(!shader_float_is_zero(1.0));
        assert!(shader_float_is_max(255.0));
        assert!(!shader_float_is_max(254.999_99));
        assert!(!shader_float_is_max(255.000_01));
        assert!(shader_float_is_zero_and_nameless(0.0, ""));
        // The sentinel counts as nameless, matching `HasName`.
        assert!(shader_float_is_zero_and_nameless(
            0.0,
            SHADER_UNINITIALIZED_NAME
        ));
        assert!(!shader_float_is_zero_and_nameless(0.0, "_MainTex"));
        assert!(!shader_float_is_zero_and_nameless(1.0, ""));
        assert!(shader_float_is_max_and_nameless(255.0, ""));
        assert!(!shader_float_is_max_and_nameless(255.0, "_MainTex"));
        // Truncation toward zero, like the reference `(int)` cast.
        assert!(shader_float_is_default(2.0, "", 2));
        assert!(shader_float_is_default(2.9, "", 2));
        assert!(shader_float_is_default(-0.0, "", 0));
        assert!(!shader_float_is_default(2.0, "_MainTex", 2));
        assert!(!shader_float_is_default(3.0, "", 2));
    }

    #[test]
    fn vector_zero_predicates_require_every_lane() {
        assert!(shader_vector_is_zero([0.0, 0.0, 0.0, 0.0]));
        assert!(shader_vector_is_zero([-0.0, -0.0, -0.0, -0.0]));
        assert!(!shader_vector_is_zero([0.0, 0.0, 0.0, f32::MIN_POSITIVE]));
        assert!(shader_vector_is_zero_and_nameless([0.0, 0.0, 0.0, 0.0], ""));
        assert!(!shader_vector_is_zero_and_nameless(
            [0.0, 0.0, 0.0, 0.0],
            "_Color"
        ));
    }

    #[test]
    fn stencil_op_default_needs_keep_and_always() {
        assert_eq!(STENCIL_OP_KEEP, 0);
        assert_eq!(STENCIL_COMP_ALWAYS, 8);
        assert!(stencil_op_is_default(0, 0, 0, 8));
        assert!(!stencil_op_is_default(1, 0, 0, 8));
        assert!(!stencil_op_is_default(0, 2, 0, 8));
        assert!(!stencil_op_is_default(0, 0, 5, 8));
        assert!(!stencil_op_is_default(0, 0, 0, 1));
        assert!(stencil_ref_is_default(0.0, ""));
        assert!(!stencil_ref_is_default(0.0, "_StencilRef"));
        assert!(stencil_mask_is_default(255.0, SHADER_UNINITIALIZED_NAME));
        assert!(!stencil_mask_is_default(254.0, ""));
    }

    #[test]
    fn render_state_words_and_stencil_conjunction_match_the_reference() {
        assert!(shader_alpha_to_mask_enabled(1));
        assert!(shader_alpha_to_mask_enabled(-1));
        assert!(!shader_alpha_to_mask_enabled(0));
        assert!(shader_conservative_enabled(Some(1)));
        assert!(!shader_conservative_enabled(Some(0)));
        assert!(!shader_conservative_enabled(None));
        assert_eq!(shader_lighting_word(true), "On");
        assert_eq!(shader_lighting_word(false), "Off");
        assert!(shader_stencil_is_default(
            true, true, true, true, true, true
        ));
        assert!(!shader_stencil_is_default(
            false, true, true, true, true, true
        ));
        assert!(!shader_stencil_is_default(
            true, true, true, true, true, false
        ));
        assert!(!shader_stencil_is_default(
            true, false, true, true, true, true
        ));
    }

    #[test]
    fn pass_program_selection_follows_mask_bits_and_source_order() {
        use ShaderProgramType::{Domain, Fragment, Geometry, Hull, RayTracing, Vertex};
        // Discriminants mirror the source enum.
        assert_eq!(
            [
                ShaderProgramType::None.discriminant(),
                Vertex.discriminant(),
                Fragment.discriminant(),
                Geometry.discriminant(),
                Hull.discriminant(),
                Domain.discriminant(),
                RayTracing.discriminant(),
            ],
            [0, 1, 2, 3, 4, 5, 6]
        );
        // One mask bit per type.
        assert_eq!(Vertex.to_program_mask(), 2);
        assert_eq!(Fragment.to_program_mask(), 4);
        assert_eq!(RayTracing.to_program_mask(), 64);
        // Program strings; `None` has none and throws on the source side.
        assert_eq!(Vertex.to_program_type_string(), Some("vp"));
        assert_eq!(Fragment.to_program_type_string(), Some("fp"));
        assert_eq!(Geometry.to_program_type_string(), Some("gp"));
        assert_eq!(Hull.to_program_type_string(), Some("hp"));
        assert_eq!(Domain.to_program_type_string(), Some("dp"));
        assert_eq!(RayTracing.to_program_type_string(), Some("rtp"));
        assert_eq!(ShaderProgramType::None.to_program_type_string(), None);
        // Mask membership.
        assert!(pass_has_program(0b0000_0110, Vertex));
        assert!(pass_has_program(0b0000_0110, Fragment));
        assert!(!pass_has_program(0b0000_0110, Geometry));
        // Selection order skips unset bits and gates ray tracing on the
        // serialized program.
        assert_eq!(
            pass_program_types(0b0111_1110, true),
            vec![Vertex, Fragment, Geometry, Hull, Domain, RayTracing]
        );
        assert_eq!(
            pass_program_types(0b0111_1110, false),
            vec![Vertex, Fragment, Geometry, Hull, Domain]
        );
        assert_eq!(
            pass_program_types(0b0100_0100, true),
            vec![Fragment, RayTracing]
        );
        assert!(pass_program_types(0, true).is_empty());
        // Pass types round-trip through discriminants and words.
        assert_eq!(
            SerializedPassType::from_discriminant(0),
            Some(SerializedPassType::Pass)
        );
        assert_eq!(
            SerializedPassType::from_discriminant(1),
            Some(SerializedPassType::UsePass)
        );
        assert_eq!(
            SerializedPassType::from_discriminant(2),
            Some(SerializedPassType::GrabPass)
        );
        assert_eq!(SerializedPassType::from_discriminant(3), None);
        assert_eq!(SerializedPassType::Pass.to_serialized_string(), "Pass");
        assert_eq!(
            SerializedPassType::UsePass.to_serialized_string(),
            "UsePass"
        );
        assert_eq!(
            SerializedPassType::GrabPass.to_serialized_string(),
            "GrabPass"
        );
    }
}
