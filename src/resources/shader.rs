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
}
