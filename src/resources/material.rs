//! `Material` (class 21) object model, ported from `MaterialExtensions`.
//!
//! `MaterialExtensions` reads every texture binding through `IMaterial.SavedProperties_C21` and
//! then branches over three serialized dictionary shapes, in this order:
//!
//! 1. `Has_TexEnvs_AssetDictionary_Utf8String_UnityTexEnv_5` — Unity 5+ with `Utf8String` keys.
//! 2. `Has_TexEnvs_AssetDictionary_FastPropertyName_UnityTexEnv_5` — Unity 5 with `FastPropertyName`
//!    keys.
//! 3. `Has_TexEnvs_AssetDictionary_FastPropertyName_UnityTexEnv_3_5` — Unity 3.5.
//!
//! All three shapes serialize the field as `m_TexEnvs`, so the branch is decided by the decoded
//! key type rather than the field name: a bare UTF-8 string is a `Utf8String` key, and a struct
//! with a single `Name` field is a `FastPropertyName` key. `MaterialTexEnvKeyShape::classify`
//! reproduces that decision, and [`MaterialView::tex_env_key_shape`] returns the shape a payload
//! actually used so a caller can tell which Unity branch it is reading.
//!
//! `FindPropertyNameByCRC28` inverts the dictionary: it maps the CRC of a property name back to the
//! stored name, so a caller holding only a shader-declared CRC can still find the binding.
//!
//! Field-name basis: the committed `material_generated_api_fixture.txt` property dump of the
//! shipped `AssetRipper.SourceGenerated.dll` (probe `Temp/opencode/matprops`). `IMaterial` exposes
//! `Name_C21`, `Shader_C21`, `SavedProperties_C21`, `ShaderKeywords_C21_*`, `ValidKeywords_C21`,
//! `InvalidKeywords_C21`, `CustomRenderQueue_C21`, `LightmapFlags_C21`,
//! `EnableInstancingVariants_C21`, `DoubleSidedGI_C21`, `LockedProperties_C21`,
//! `ModifiedSerializedProperties_C21`, `Parent_C21`, `StringTagMap_C21`, `DisabledShaderPasses_C21`,
//! `BuildTextureStacks_C21`, and `AllowLocking_C21`; the `_C21` suffix is the generated
//! version-disambiguating suffix, not part of the TypeTree name. `UnityPropertySheet` versions
//! `3_5`/`5`/`5_6`/`2021` expose `TexEnvs_*`, `Floats_*`, `Colors_*`, `Ints`, `FlowMappedInYaml`,
//! and `SerializedVersion`. `UnityTexEnv` `3_5`/`5` expose `Offset`, `Scale`, `Texture`, and
//! `FlowMappedInYaml`.

use std::error::Error;
use std::fmt::{self, Display};

use crate::numerics::Vec2;
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};

/// A failure while reading `Material` fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaterialViewError {
    /// A field lookup found duplicate names along the base chain.
    FieldLookup {
        /// The field being read.
        field: &'static str,
        /// The underlying lookup failure.
        source: SerializedValueLookupError,
    },
    /// A field is present but has the wrong value kind.
    WrongType {
        /// The field being read.
        field: &'static str,
        /// The value kind actually decoded.
        actual: &'static str,
    },
    /// A map or array element did not have the expected pair shape.
    MalformedPair {
        /// The field being read.
        field: &'static str,
        /// The zero-based element position.
        index: usize,
        /// What was wrong.
        detail: &'static str,
    },
    /// A property key is not valid UTF-8 and cannot be reported as a name.
    InvalidPropertyName {
        /// The field being read.
        field: &'static str,
        /// The zero-based element position.
        index: usize,
    },
    /// An unsigned field decoded a negative value.
    NegativeValue {
        /// The field being read.
        field: &'static str,
        /// The value found.
        value: i32,
    },
}

impl Display for MaterialViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field, source } => {
                write!(formatter, "could not read Material field '{field}': {source}")
            }
            Self::WrongType { field, actual } => write!(
                formatter,
                "Material field '{field}' has kind {actual}, expected a different kind"
            ),
            Self::MalformedPair {
                field,
                index,
                detail,
            } => write!(
                formatter,
                "Material field '{field}' element {index} is not a valid pair: {detail}"
            ),
            Self::InvalidPropertyName { field, index } => write!(
                formatter,
                "Material field '{field}' element {index} has a property name that is not valid UTF-8"
            ),
            Self::NegativeValue { field, value } => write!(
                formatter,
                "Material field '{field}' is unsigned but decoded the negative value {value}"
            ),
        }
    }
}

impl Error for MaterialViewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FieldLookup { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Which serialized dictionary shape `m_TexEnvs` used.
///
/// `MaterialExtensions` branches on the three `Has_TexEnvs_*` gates in this order; the decoded key
/// type is what distinguishes them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MaterialTexEnvKeyShape {
    /// `TexEnvs_AssetDictionary_Utf8String_UnityTexEnv_5`: bare UTF-8 keys.
    Utf8String,
    /// `TexEnvs_AssetDictionary_FastPropertyName_UnityTexEnv_5` or the 3.5 variant: a struct with
    /// a single `Name` field.
    FastPropertyName,
    /// The payload has no `m_TexEnvs` entries and so matches no gate.
    #[default]
    Absent,
}

impl MaterialTexEnvKeyShape {
    /// Decides the branch from one decoded map key.
    pub fn classify(key: &SerializedValue) -> Option<Self> {
        match key {
            SerializedValue::StringBytes(_) | SerializedValue::Bytes(_) => Some(Self::Utf8String),
            SerializedValue::Struct(_) => Some(Self::FastPropertyName),
            _ => None,
        }
    }

    /// The `Has_*` gate name this shape satisfies, for diagnostics.
    pub const fn gate_name(self) -> Option<&'static str> {
        match self {
            Self::Utf8String => Some("Has_TexEnvs_AssetDictionary_Utf8String_UnityTexEnv_5"),
            Self::FastPropertyName => {
                Some("Has_TexEnvs_AssetDictionary_FastPropertyName_UnityTexEnv_5_or_3_5")
            }
            Self::Absent => None,
        }
    }
}

/// A texture binding from `m_SavedProperties.m_TexEnvs`.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialTextureProperty {
    /// The shader property name.
    pub name: String,
    /// The texture reference: the file-table index and signed PathID.
    ///
    /// `None` means the payload stored a null pointer, which is distinct from a missing entry.
    pub texture: Option<(i32, i64)>,
    /// `UnityTexEnv.Offset`.
    pub offset: Vec2,
    /// `UnityTexEnv.Scale`.
    pub scale: Vec2,
}

/// A float binding from `m_SavedProperties.m_Floats`.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialFloatProperty {
    /// The shader property name.
    pub name: String,
    /// The stored value.
    pub value: f32,
}

/// A colour binding from `m_SavedProperties.m_Colors`.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialColorProperty {
    /// The shader property name.
    pub name: String,
    /// The stored RGBA components.
    pub value: [f32; 4],
}

/// An integer binding from `m_SavedProperties.m_Ints`.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialIntProperty {
    /// The shader property name.
    pub name: String,
    /// The stored value.
    pub value: i32,
}

/// The `UnityPropertySheet` fields `MaterialExtensions` and the exporters read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialPropertySheetView {
    /// `m_FlowMappedInYaml`, present on `UnityPropertySheet_5` and later.
    pub flow_mapped_in_yaml: Option<bool>,
    /// `m_Floats`.
    pub floats: Vec<MaterialFloatProperty>,
    /// `m_Colors`.
    pub colors: Vec<MaterialColorProperty>,
    /// `m_Ints`, absent on `UnityPropertySheet_3_5`.
    pub ints: Vec<MaterialIntProperty>,
    /// The dictionary shape `m_TexEnvs` used.
    pub tex_env_shape: MaterialTexEnvKeyShape,
    /// `m_TexEnvs`, in serialized order.
    pub tex_envs: Vec<MaterialTextureProperty>,
}

/// The `Material` (class 21) fields the exporters read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialView {
    /// `m_Name`.
    pub name: Option<String>,
    /// `m_Shader`, as the file-table index and signed PathID; `None` is a null pointer.
    pub shader: Option<(i32, i64)>,
    /// `m_CustomRenderQueue`.
    pub custom_render_queue: Option<i32>,
    /// `m_LightmapFlags`.
    pub lightmap_flags: Option<u32>,
    /// `m_EnableInstancingVariants`.
    pub enable_instancing_variants: Option<bool>,
    /// `m_DoubleSidedGI`.
    pub double_sided_gi: Option<bool>,
    /// `m_ShaderKeywords`, including the `AssetList<Utf8String>` spelling.
    pub shader_keywords: Vec<String>,
    /// `m_ValidKeywords`.
    pub valid_keywords: Vec<String>,
    /// `m_InvalidKeywords`.
    pub invalid_keywords: Vec<String>,
    /// `m_LockedProperties`.
    pub locked_properties: Option<String>,
    /// `m_ModifiedSerializedProperties`.
    pub modified_serialized_properties: Option<u32>,
    /// `m_SavedProperties`.
    pub saved_properties: MaterialPropertySheetView,
}

/// The CRC AssetRipper's shader export uses to match a property name.
pub const CRC28_MASK_FIELD: u32 = 0x0FFF_FFFF;

impl MaterialView {
    /// Reads the export-relevant fields from a decoded `Material` payload.
    pub fn from_value(root: &SerializedValue) -> Result<Self, MaterialViewError> {
        let saved_properties = match lookup(root, "m_SavedProperties")? {
            Some(value) => read_property_sheet(value)?,
            None => MaterialPropertySheetView::default(),
        };
        Ok(Self {
            name: read_string(root, "m_Name")?,
            shader: read_pptr(root, "m_Shader")?,
            custom_render_queue: read_i32(root, "m_CustomRenderQueue")?,
            lightmap_flags: read_u32(root, "m_LightmapFlags")?,
            enable_instancing_variants: read_bool(root, "m_EnableInstancingVariants")?,
            double_sided_gi: read_bool(root, "m_DoubleSidedGI")?,
            shader_keywords: read_string_array(root, "m_ShaderKeywords")?,
            valid_keywords: read_string_array(root, "m_ValidKeywords")?,
            invalid_keywords: read_string_array(root, "m_InvalidKeywords")?,
            locked_properties: read_string(root, "m_LockedProperties")?,
            modified_serialized_properties: read_u32(root, "m_ModifiedSerializedProperties")?,
            saved_properties,
        })
    }

    /// `MaterialExtensions.GetTextureProperties`.
    pub fn texture_properties(&self) -> &[MaterialTextureProperty] {
        &self.saved_properties.tex_envs
    }

    /// `MaterialExtensions.TryGetTextureProperty`.
    ///
    /// Returns `None` for both a missing key and a stored null texture pointer; use
    /// [`MaterialTextureProperty::texture`] on a successful lookup to tell them apart.
    pub fn try_get_texture_property(
        &self,
        property_name: &str,
    ) -> Option<&MaterialTextureProperty> {
        self.saved_properties
            .tex_envs
            .iter()
            .find(|property| property.name == property_name)
    }

    /// `GlbLevelBuilder.GetTextures`: the main/normal texture bindings for scene GLB export.
    ///
    /// The reference walks the texture properties in serialized order: a property named
    /// `_MainTex`, `texture`, `Texture`, or `_Texture` supplies the main texture (first wins); a
    /// property named `_Normal`, `Normal`, or `normal` supplies the normal texture (first wins);
    /// any other property supplies the main-texture replacement (first wins), which fills in an
    /// absent main texture. The reference additionally resolves each selected PPtr against the
    /// asset collection and keeps only `ITexture2D` targets; that resolution needs the collection
    /// and stays with the caller, so this returns the selected bindings.
    pub fn glb_material_textures(
        &self,
    ) -> (
        Option<&MaterialTextureProperty>,
        Option<&MaterialTextureProperty>,
    ) {
        let mut main: Option<&MaterialTextureProperty> = None;
        let mut normal: Option<&MaterialTextureProperty> = None;
        let mut replacement: Option<&MaterialTextureProperty> = None;
        for property in &self.saved_properties.tex_envs {
            match property.name.as_str() {
                "_MainTex" | "texture" | "Texture" | "_Texture" => {
                    if main.is_none() {
                        main = Some(property);
                    }
                }
                "_Normal" | "Normal" | "normal" => {
                    if normal.is_none() {
                        normal = Some(property);
                    }
                }
                _ => {
                    if replacement.is_none() {
                        replacement = Some(property);
                    }
                }
            }
        }
        (main.or(replacement), normal)
    }

    /// The dictionary shape `m_TexEnvs` used.
    pub fn tex_env_key_shape(&self) -> MaterialTexEnvKeyShape {
        self.saved_properties.tex_env_shape
    }

    /// `UnityPropertySheet.FindPropertyNameByCRC28`: the name whose CRC matches `crc`.
    ///
    /// The reference searches texture-environment names first, appending `_HDR`, `_ST`, and
    /// `_TexelSize` to each in that order, then float names, then integer names, then colour names,
    /// returning the first match.
    pub fn find_property_name_by_crc28(&self, crc: u32) -> Option<String> {
        let sheet = &self.saved_properties;
        for property in &sheet.tex_envs {
            for postfix in CRC28_TEX_ENV_POSTFIXES {
                let candidate = format!("{}{postfix}", property.name);
                if crc28(candidate.as_bytes()) == crc {
                    return Some(candidate);
                }
            }
        }
        for property in &sheet.floats {
            if crc28(property.name.as_bytes()) == crc {
                return Some(property.name.clone());
            }
        }
        for property in &sheet.ints {
            if crc28(property.name.as_bytes()) == crc {
                return Some(property.name.clone());
            }
        }
        for property in &sheet.colors {
            if crc28(property.name.as_bytes()) == crc {
                return Some(property.name.clone());
            }
        }
        None
    }
}

/// `UnityPropertySheetExtensions.HDRPostfixName`, `STPostfixName`, and
/// `TexelSizePostfixName`, in the order `FindPropertyNameByCRC28` tries them.
pub const CRC28_TEX_ENV_POSTFIXES: [&str; 3] = ["_HDR", "_ST", "_TexelSize"];

/// `AssetRipper.Checksum.Crc28Algorithm.Mask`: the 28-bit mask the reference applies.
pub const CRC28_MASK: u32 = 0x0FFF_FFFF;

/// `AssetRipper.Checksum.Crc28Algorithm.HashData`: the reflected CRC-32 of `bytes` masked to 28
/// bits.
///
/// The reference exposes `HashData`, `HashUTF8`, `MatchAscii`, and `MatchUTF8` over a `Mask` field
/// of `0x0FFFFFFF`, so every Crc28 word is the corresponding `Crc32Algorithm` word with its top
/// four bits cleared. Differential fixture: `crc28_fixture.txt` (probe `Temp/opencode/crcprobe`
/// against the shipped `AssetRipper.Checksum.dll`).
pub fn crc28(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = if crc & 1 != 0 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc & CRC28_MASK
}

fn lookup<'a>(
    root: &'a SerializedValue,
    name: &'static str,
) -> Result<Option<&'a SerializedValue>, MaterialViewError> {
    root.unique_field_in_base_chain(name)
        .map_err(|source| MaterialViewError::FieldLookup {
            field: name,
            source,
        })
}

fn serialized_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "SInt8",
        SerializedValue::U8(_) => "UInt8",
        SerializedValue::I16(_) => "SInt16",
        SerializedValue::U16(_) => "UInt16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "SInt32",
        SerializedValue::U32(_) => "UInt32",
        SerializedValue::I64(_) => "SInt64",
        SerializedValue::U64(_) => "UInt64",
        SerializedValue::F32(_) => "float",
        SerializedValue::F64(_) => "double",
        SerializedValue::StringBytes(_) => "string bytes",
        SerializedValue::Bytes(_) => "byte data",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::Raw { .. } => "raw data",
    }
}

fn read_i32(root: &SerializedValue, name: &'static str) -> Result<Option<i32>, MaterialViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::I32(value)) => Ok(Some(*value)),
        Some(SerializedValue::U32(value)) => Ok(Some(*value as i32)),
        Some(other) => Err(MaterialViewError::WrongType {
            field: name,
            actual: serialized_kind(other),
        }),
    }
}

fn read_u32(root: &SerializedValue, name: &'static str) -> Result<Option<u32>, MaterialViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::U32(value)) => Ok(Some(*value)),
        Some(SerializedValue::I32(value)) if *value >= 0 => Ok(Some(*value as u32)),
        // The TypeTree declares this field unsigned, so a negative value is a payload that
        // disagrees with its schema rather than a value to drop.
        Some(SerializedValue::I32(value)) => Err(MaterialViewError::NegativeValue {
            field: name,
            value: *value,
        }),
        Some(other) => Err(MaterialViewError::WrongType {
            field: name,
            actual: serialized_kind(other),
        }),
    }
}

fn read_bool(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<bool>, MaterialViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::Bool(value)) => Ok(Some(*value)),
        Some(SerializedValue::U8(value)) => Ok(Some(*value != 0)),
        Some(other) => Err(MaterialViewError::WrongType {
            field: name,
            actual: serialized_kind(other),
        }),
    }
}

fn read_pptr(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<(i32, i64)>, MaterialViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        // A null pointer keeps the `(0, 0)` identity the baseline uses.
        Some(SerializedValue::PPtr { file_id, path_id }) => {
            if *file_id == 0 && *path_id == 0 {
                Ok(None)
            } else {
                Ok(Some((*file_id, *path_id)))
            }
        }
        Some(other) => Err(MaterialViewError::WrongType {
            field: name,
            actual: serialized_kind(other),
        }),
    }
}

fn read_string(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<String>, MaterialViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::StringBytes(bytes)) | Some(SerializedValue::Bytes(bytes)) => {
            match std::str::from_utf8(bytes) {
                Ok(text) => Ok(Some(text.to_owned())),
                Err(_) => Err(MaterialViewError::InvalidPropertyName {
                    field: name,
                    index: 0,
                }),
            }
        }
        Some(other) => Err(MaterialViewError::WrongType {
            field: name,
            actual: serialized_kind(other),
        }),
    }
}

fn read_string_array(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Vec<String>, MaterialViewError> {
    let Some(value) = lookup(root, name)? else {
        return Ok(Vec::new());
    };
    let values = match value {
        SerializedValue::Array(values) => values.as_slice(),
        // Unity serializes `m_ShaderKeywords` as a single space-separated string before the list
        // form; splitting reproduces the list AssetRipper reads from either shape.
        SerializedValue::StringBytes(bytes) | SerializedValue::Bytes(bytes) => {
            let text =
                std::str::from_utf8(bytes).map_err(|_| MaterialViewError::InvalidPropertyName {
                    field: name,
                    index: 0,
                })?;
            return Ok(text
                .split(' ')
                .filter(|keyword| !keyword.is_empty())
                .map(str::to_owned)
                .collect());
        }
        other => {
            return Err(MaterialViewError::WrongType {
                field: name,
                actual: serialized_kind(other),
            })
        }
    };
    let mut keywords = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let bytes = match value {
            SerializedValue::StringBytes(bytes) | SerializedValue::Bytes(bytes) => bytes.as_slice(),
            SerializedValue::Char(value) => {
                // A single-character element is a one-byte string.
                let buffer = u16::to_le_bytes(*value);
                keywords.push(
                    std::str::from_utf8(&buffer[..1])
                        .map(str::to_owned)
                        .unwrap_or_default(),
                );
                continue;
            }
            other => {
                return Err(MaterialViewError::WrongType {
                    field: name,
                    actual: serialized_kind(other),
                })
            }
        };
        let text = std::str::from_utf8(bytes)
            .map_err(|_| MaterialViewError::InvalidPropertyName { field: name, index })?;
        keywords.push(text.to_owned());
    }
    Ok(keywords)
}

/// Reads the `first`/`second` pair sequence of a serialized Unity map.
fn read_map<'a>(
    value: &'a SerializedValue,
    field: &'static str,
) -> Result<Vec<(&'a SerializedValue, &'a SerializedValue)>, MaterialViewError> {
    let elements: &[SerializedValue] = match value {
        SerializedValue::Array(elements) => elements.as_slice(),
        SerializedValue::Struct(fields) => match fields
            .iter()
            .find(|field| field.name == "Array")
            .map(|field| &field.value)
        {
            Some(SerializedValue::Array(elements)) => elements.as_slice(),
            _ => {
                return Err(MaterialViewError::WrongType {
                    field,
                    actual: serialized_kind(value),
                })
            }
        },
        other => {
            return Err(MaterialViewError::WrongType {
                field,
                actual: serialized_kind(other),
            })
        }
    };
    let mut pairs = Vec::with_capacity(elements.len());
    for (index, element) in elements.iter().enumerate() {
        let SerializedValue::Struct(fields) = element else {
            return Err(MaterialViewError::MalformedPair {
                field,
                index,
                detail: "the element is not a struct",
            });
        };
        let key = fields
            .iter()
            .find(|entry| entry.name == "first")
            .map(|entry| &entry.value);
        let value = fields
            .iter()
            .find(|entry| entry.name == "second")
            .map(|entry| &entry.value);
        match (key, value) {
            (Some(key), Some(value)) => pairs.push((key, value)),
            _ => {
                return Err(MaterialViewError::MalformedPair {
                    field,
                    index,
                    detail: "the element is missing a 'first' or 'second' field",
                })
            }
        }
    }
    Ok(pairs)
}

/// Resolves a `Utf8String` or `FastPropertyName` map key to its property name.
fn read_property_name(
    key: &SerializedValue,
    field: &'static str,
    index: usize,
) -> Result<String, MaterialViewError> {
    match key {
        SerializedValue::StringBytes(bytes) | SerializedValue::Bytes(bytes) => {
            std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| MaterialViewError::InvalidPropertyName { field, index })
        }
        SerializedValue::Struct(fields) => {
            let name = fields
                .iter()
                .find(|entry| entry.name == "Name")
                .map(|entry| &entry.value)
                .ok_or(MaterialViewError::MalformedPair {
                    field,
                    index,
                    detail: "the key struct is missing its 'Name' field",
                })?;
            match name {
                SerializedValue::StringBytes(bytes) | SerializedValue::Bytes(bytes) => {
                    std::str::from_utf8(bytes)
                        .map(str::to_owned)
                        .map_err(|_| MaterialViewError::InvalidPropertyName { field, index })
                }
                other => Err(MaterialViewError::WrongType {
                    field,
                    actual: serialized_kind(other),
                }),
            }
        }
        other => Err(MaterialViewError::WrongType {
            field,
            actual: serialized_kind(other),
        }),
    }
}

fn read_vec2(value: &SerializedValue, field: &'static str) -> Result<Vec2, MaterialViewError> {
    let read = |name: &'static str| -> Result<f32, MaterialViewError> {
        match lookup(value, name)? {
            Some(SerializedValue::F32(value)) => Ok(*value),
            Some(other) => Err(MaterialViewError::WrongType {
                field,
                actual: serialized_kind(other),
            }),
            None => Ok(0.0),
        }
    };
    Ok(Vec2 {
        x: read("x")?,
        y: read("y")?,
    })
}

fn read_property_sheet(
    value: &SerializedValue,
) -> Result<MaterialPropertySheetView, MaterialViewError> {
    let flow_mapped_in_yaml = read_bool(value, "m_FlowMappedInYaml")?;

    let mut floats = Vec::new();
    if let Some(entry) = lookup(value, "m_Floats")? {
        for (index, (key, item)) in read_map(entry, "m_Floats")?.into_iter().enumerate() {
            let name = read_property_name(key, "m_Floats", index)?;
            let amount = match item {
                SerializedValue::F32(amount) => *amount,
                SerializedValue::I32(amount) => *amount as f32,
                other => {
                    return Err(MaterialViewError::WrongType {
                        field: "m_Floats",
                        actual: serialized_kind(other),
                    })
                }
            };
            floats.push(MaterialFloatProperty {
                name,
                value: amount,
            });
        }
    }

    let mut colors = Vec::new();
    if let Some(entry) = lookup(value, "m_Colors")? {
        for (index, (key, item)) in read_map(entry, "m_Colors")?.into_iter().enumerate() {
            let name = read_property_name(key, "m_Colors", index)?;
            let mut channels = [0.0f32; 4];
            if let SerializedValue::Struct(fields) = item {
                for (slot, field_name) in ["r", "g", "b", "a"].into_iter().enumerate() {
                    let field_value = fields
                        .iter()
                        .find(|entry| entry.name == field_name)
                        .map(|entry| &entry.value);
                    match field_value {
                        Some(SerializedValue::F32(value)) => channels[slot] = *value,
                        Some(other) => {
                            return Err(MaterialViewError::WrongType {
                                field: "m_Colors",
                                actual: serialized_kind(other),
                            })
                        }
                        None => {}
                    }
                }
            } else {
                return Err(MaterialViewError::WrongType {
                    field: "m_Colors",
                    actual: serialized_kind(item),
                });
            }
            colors.push(MaterialColorProperty {
                name,
                value: channels,
            });
        }
    }

    let mut ints = Vec::new();
    if let Some(entry) = lookup(value, "m_Ints")? {
        for (index, (key, item)) in read_map(entry, "m_Ints")?.into_iter().enumerate() {
            let name = read_property_name(key, "m_Ints", index)?;
            let amount = match item {
                SerializedValue::I32(amount) => *amount,
                SerializedValue::U32(amount) => *amount as i32,
                other => {
                    return Err(MaterialViewError::WrongType {
                        field: "m_Ints",
                        actual: serialized_kind(other),
                    })
                }
            };
            ints.push(MaterialIntProperty {
                name,
                value: amount,
            });
        }
    }

    let mut tex_envs = Vec::new();
    let mut tex_env_shape = MaterialTexEnvKeyShape::Absent;
    if let Some(entry) = lookup(value, "m_TexEnvs")? {
        for (index, (key, item)) in read_map(entry, "m_TexEnvs")?.into_iter().enumerate() {
            let name = read_property_name(key, "m_TexEnvs", index)?;
            let shape =
                MaterialTexEnvKeyShape::classify(key).ok_or(MaterialViewError::MalformedPair {
                    field: "m_TexEnvs",
                    index,
                    detail: "the key is neither a Utf8String nor a FastPropertyName",
                })?;
            if tex_env_shape == MaterialTexEnvKeyShape::Absent {
                tex_env_shape = shape;
            }
            let offset = match lookup(item, "m_Offset")? {
                Some(offset) => read_vec2(offset, "m_Offset")?,
                None => Vec2::default(),
            };
            let scale = match lookup(item, "m_Scale")? {
                Some(scale) => read_vec2(scale, "m_Scale")?,
                None => Vec2::default(),
            };
            let texture = match lookup(item, "m_Texture")? {
                Some(SerializedValue::PPtr { file_id, path_id }) => {
                    if *file_id == 0 && *path_id == 0 {
                        None
                    } else {
                        Some((*file_id, *path_id))
                    }
                }
                Some(other) => {
                    return Err(MaterialViewError::WrongType {
                        field: "m_Texture",
                        actual: serialized_kind(other),
                    })
                }
                None => None,
            };
            tex_envs.push(MaterialTextureProperty {
                name,
                texture,
                offset,
                scale,
            });
        }
    }

    Ok(MaterialPropertySheetView {
        flow_mapped_in_yaml,
        floats,
        colors,
        ints,
        tex_env_shape,
        tex_envs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialized_file::object::SerializedFieldValue;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn string(value: &str) -> SerializedValue {
        SerializedValue::StringBytes(value.as_bytes().to_vec())
    }

    fn fast_property_name(value: &str) -> SerializedValue {
        SerializedValue::Struct(vec![field("Name", string(value))])
    }

    fn pair(key: SerializedValue, value: SerializedValue) -> SerializedValue {
        SerializedValue::Struct(vec![field("first", key), field("second", value)])
    }

    fn tex_env(texture: Option<(i32, i64)>) -> SerializedValue {
        let texture = match texture {
            Some((file_id, path_id)) => SerializedValue::PPtr { file_id, path_id },
            None => SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            },
        };
        SerializedValue::Struct(vec![
            field(
                "m_Offset",
                SerializedValue::Struct(vec![
                    field("x", SerializedValue::F32(0.25)),
                    field("y", SerializedValue::F32(0.5)),
                ]),
            ),
            field(
                "m_Scale",
                SerializedValue::Struct(vec![
                    field("x", SerializedValue::F32(2.0)),
                    field("y", SerializedValue::F32(3.0)),
                ]),
            ),
            field("m_Texture", texture),
        ])
    }

    fn material_with_tex_envs(key: fn(&str) -> SerializedValue) -> MaterialView {
        let sheet = SerializedValue::Struct(vec![
            field("m_FlowMappedInYaml", SerializedValue::Bool(true)),
            field(
                "m_TexEnvs",
                SerializedValue::Array(vec![
                    pair(key("_MainTex"), tex_env(Some((2, 100)))),
                    pair(key("_BumpMap"), tex_env(None)),
                ]),
            ),
            field(
                "m_Floats",
                SerializedValue::Array(vec![pair(key("_Glossiness"), SerializedValue::F32(0.25))]),
            ),
            field(
                "m_Colors",
                SerializedValue::Array(vec![pair(
                    key("_Color"),
                    SerializedValue::Struct(vec![
                        field("r", SerializedValue::F32(1.0)),
                        field("g", SerializedValue::F32(0.5)),
                        field("b", SerializedValue::F32(0.25)),
                        field("a", SerializedValue::F32(1.0)),
                    ]),
                )]),
            ),
        ]);
        let root = SerializedValue::Struct(vec![
            field("m_Name", string("Standard")),
            field(
                "m_Shader",
                SerializedValue::PPtr {
                    file_id: 1,
                    path_id: 5,
                },
            ),
            field("m_CustomRenderQueue", SerializedValue::I32(-1)),
            field("m_LightmapFlags", SerializedValue::U32(4)),
            field("m_EnableInstancingVariants", SerializedValue::Bool(true)),
            field("m_DoubleSidedGI", SerializedValue::Bool(false)),
            field(
                "m_ShaderKeywords",
                SerializedValue::Array(vec![
                    string("NORMALMAP"),
                    string("_SPECULARHIGHLIGHTS_OFF"),
                ]),
            ),
            field("m_ValidKeywords", SerializedValue::Array(vec![])),
            field("m_InvalidKeywords", SerializedValue::Array(vec![])),
            field("m_SavedProperties", sheet),
        ]);
        MaterialView::from_value(&root).unwrap()
    }

    /// Builds a `MaterialView` whose tex-env list is exactly `names`, in order.
    fn material_with_named_tex_envs(names: &[&str]) -> MaterialView {
        let sheet = SerializedValue::Struct(vec![field(
            "m_TexEnvs",
            SerializedValue::Array(
                names
                    .iter()
                    .map(|name| pair(string(name), tex_env(Some((2, 100)))))
                    .collect(),
            ),
        )]);
        let root = SerializedValue::Struct(vec![field("m_SavedProperties", sheet)]);
        MaterialView::from_value(&root).unwrap()
    }

    #[test]
    fn glb_material_textures_select_main_and_normal_by_name() {
        let view = material_with_named_tex_envs(&["_BumpMap", "_MainTex", "_Normal"]);
        let (main, normal) = view.glb_material_textures();
        assert_eq!(
            main.map(|property| property.name.as_str()),
            Some("_MainTex")
        );
        assert_eq!(
            normal.map(|property| property.name.as_str()),
            Some("_Normal")
        );
    }

    #[test]
    fn glb_material_textures_fall_back_to_the_first_other_texture() {
        let view = material_with_named_tex_envs(&["_BumpMap", "_DetailAlbedoMap"]);
        let (main, normal) = view.glb_material_textures();
        assert_eq!(
            main.map(|property| property.name.as_str()),
            Some("_BumpMap")
        );
        assert_eq!(normal, None);
    }

    #[test]
    fn glb_material_textures_prefer_main_over_replacement() {
        let view = material_with_named_tex_envs(&["_BumpMap", "Texture"]);
        let (main, _) = view.glb_material_textures();
        assert_eq!(main.map(|property| property.name.as_str()), Some("Texture"));
    }

    #[test]
    fn glb_material_textures_keep_the_first_match_of_each_kind() {
        let view = material_with_named_tex_envs(&["_MainTex", "texture", "Normal", "_Normal"]);
        let (main, normal) = view.glb_material_textures();
        assert_eq!(
            main.map(|property| property.name.as_str()),
            Some("_MainTex")
        );
        assert_eq!(
            normal.map(|property| property.name.as_str()),
            Some("Normal")
        );
    }

    #[test]
    fn glb_material_textures_accept_all_spellings() {
        for main_name in ["_MainTex", "texture", "Texture", "_Texture"] {
            let view = material_with_named_tex_envs(&[main_name]);
            assert_eq!(
                view.glb_material_textures()
                    .0
                    .map(|property| property.name.as_str()),
                Some(main_name)
            );
        }
        for normal_name in ["_Normal", "Normal", "normal"] {
            let view = material_with_named_tex_envs(&[normal_name]);
            assert_eq!(
                view.glb_material_textures()
                    .1
                    .map(|property| property.name.as_str()),
                Some(normal_name)
            );
        }
    }

    #[test]
    fn glb_material_textures_are_empty_without_tex_envs() {
        let view = material_with_named_tex_envs(&[]);
        assert_eq!(view.glb_material_textures(), (None, None));
    }

    #[test]
    fn utf8_string_tex_env_branch_is_selected() {
        let view = material_with_tex_envs(string);
        assert_eq!(view.tex_env_key_shape(), MaterialTexEnvKeyShape::Utf8String);
        assert_eq!(
            view.tex_env_key_shape().gate_name(),
            Some("Has_TexEnvs_AssetDictionary_Utf8String_UnityTexEnv_5")
        );
        let properties = view.texture_properties();
        assert_eq!(properties.len(), 2);
        assert_eq!(properties[0].name, "_MainTex");
        assert_eq!(properties[0].texture, Some((2, 100)));
        assert_eq!(properties[0].offset, Vec2 { x: 0.25, y: 0.5 });
        assert_eq!(properties[0].scale, Vec2 { x: 2.0, y: 3.0 });
        assert_eq!(
            properties[1].texture, None,
            "a stored null pointer is not missing"
        );
    }

    #[test]
    fn fast_property_name_tex_env_branch_is_selected() {
        let view = material_with_tex_envs(fast_property_name);
        assert_eq!(
            view.tex_env_key_shape(),
            MaterialTexEnvKeyShape::FastPropertyName
        );
        assert_eq!(view.texture_properties().len(), 2);
        assert_eq!(view.texture_properties()[0].name, "_MainTex");
    }

    #[test]
    fn try_get_texture_property_distinguishes_present_and_absent() {
        let view = material_with_tex_envs(string);
        let present = view.try_get_texture_property("_BumpMap").unwrap();
        assert_eq!(present.name, "_BumpMap");
        assert_eq!(present.texture, None);
        assert!(view.try_get_texture_property("_Missing").is_none());
    }

    #[test]
    fn floats_and_colors_are_read() {
        let view = material_with_tex_envs(string);
        assert_eq!(view.saved_properties.flow_mapped_in_yaml, Some(true));
        assert_eq!(view.saved_properties.floats.len(), 1);
        assert_eq!(view.saved_properties.floats[0].name, "_Glossiness");
        assert_eq!(view.saved_properties.floats[0].value, 0.25);
        assert_eq!(view.saved_properties.colors.len(), 1);
        assert_eq!(view.saved_properties.colors[0].name, "_Color");
        assert_eq!(view.saved_properties.colors[0].value, [1.0, 0.5, 0.25, 1.0]);
    }

    #[test]
    fn header_fields_and_keywords_are_read() {
        let view = material_with_tex_envs(string);
        assert_eq!(view.name.as_deref(), Some("Standard"));
        assert_eq!(view.shader, Some((1, 5)));
        assert_eq!(view.custom_render_queue, Some(-1));
        assert_eq!(view.lightmap_flags, Some(4));
        assert_eq!(view.enable_instancing_variants, Some(true));
        assert_eq!(view.double_sided_gi, Some(false));
        assert_eq!(
            view.shader_keywords,
            vec!["NORMALMAP", "_SPECULARHIGHLIGHTS_OFF"]
        );
        assert!(view.valid_keywords.is_empty());
        assert!(view.invalid_keywords.is_empty());
    }

    #[test]
    fn space_separated_shader_keywords_are_split() {
        let root = SerializedValue::Struct(vec![field(
            "m_ShaderKeywords",
            string("NORMALMAP _ENVIRONMENTREFLECTION"),
        )]);
        let view = MaterialView::from_value(&root).unwrap();
        assert_eq!(
            view.shader_keywords,
            vec!["NORMALMAP", "_ENVIRONMENTREFLECTION"]
        );
    }

    #[test]
    fn find_property_name_by_crc28_tries_the_tex_env_postfixes_first() {
        let view = material_with_tex_envs(string);
        // The reference appends `_HDR`, `_ST`, and `_TexelSize` to each texture name.
        assert_eq!(
            view.find_property_name_by_crc28(crc28(b"_MainTex_HDR")),
            Some("_MainTex_HDR".to_owned())
        );
        assert_eq!(
            view.find_property_name_by_crc28(crc28(b"_MainTex_ST")),
            Some("_MainTex_ST".to_owned())
        );
        assert_eq!(
            view.find_property_name_by_crc28(crc28(b"_BumpMap_TexelSize")),
            Some("_BumpMap_TexelSize".to_owned())
        );
        // A bare texture name is not a candidate: only its postfixed forms are.
        assert_eq!(view.find_property_name_by_crc28(crc28(b"_MainTex")), None);
        // Float and colour names are checked verbatim, after the texture group.
        assert_eq!(
            view.find_property_name_by_crc28(crc28(b"_Glossiness")),
            Some("_Glossiness".to_owned())
        );
        assert_eq!(
            view.find_property_name_by_crc28(crc28(b"_Color")),
            Some("_Color".to_owned())
        );
        assert_eq!(view.find_property_name_by_crc28(0), None);
    }

    #[test]
    fn find_property_name_by_crc28_checks_ints_before_colors() {
        let sheet = SerializedValue::Struct(vec![
            field(
                "m_Ints",
                SerializedValue::Array(vec![pair(string("_QueueOffset"), SerializedValue::I32(7))]),
            ),
            field(
                "m_Colors",
                SerializedValue::Array(vec![pair(
                    string("_Color"),
                    SerializedValue::Struct(vec![field("r", SerializedValue::F32(1.0))]),
                )]),
            ),
        ]);
        let root = SerializedValue::Struct(vec![field("m_SavedProperties", sheet)]);
        let view = MaterialView::from_value(&root).unwrap();
        assert_eq!(view.saved_properties.ints.len(), 1);
        assert_eq!(view.saved_properties.ints[0].value, 7);
        assert_eq!(view.saved_properties.colors[0].value, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(
            view.find_property_name_by_crc28(crc28(b"_QueueOffset")),
            Some("_QueueOffset".to_owned())
        );
    }

    /// Differential fixture produced by the isolated dotnet probe `Temp/opencode/crcprobe` against
    /// the shipped `AssetRipper.Checksum.dll` (`Crc28Algorithm` and `Crc32Algorithm`).
    #[test]
    fn crc28_matches_the_reference_probe_words() {
        let fixture = include_str!("crc28_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts.len(), 7, "{line}");
            assert_eq!(parts[0], "PROBE", "{line}");
            let input = parts[1];
            let expected = u32::from_str_radix(parts[2], 16).unwrap();
            let data_expected = u32::from_str_radix(parts[3], 16).unwrap();
            let crc32_expected = u32::from_str_radix(parts[4], 16).unwrap();
            assert_eq!(expected, data_expected, "{line}");
            assert_eq!(
                parts[5], "True",
                "MatchUTF8 accepted the exact word for {line}"
            );
            assert_eq!(
                parts[6], "False",
                "MatchUTF8 rejected a perturbed word for {line}"
            );
            // Every Crc28 word is the Crc32 word masked to 28 bits.
            assert_eq!(crc32_expected & CRC28_MASK, expected, "{line}");
            assert_eq!(crc28(input.as_bytes()), expected, "{line}");
            rows += 1;
        }
        assert!(
            rows >= 12,
            "the fixture pins at least twelve words, got {rows}"
        );
    }

    #[test]
    fn crc28_masks_to_twenty_eight_bits() {
        assert_eq!(crc28(b""), 0x0000_0000);
        assert_eq!(crc28(b"a"), 0x08B7_BE43);
        assert_eq!(crc28(b"abc"), 0x0524_41C2);
        assert_eq!(crc28(b"_MainTex"), 0x0D6A_83C9);
        assert_eq!(crc28(b"_MainTex_TexelSize"), 0x01D2_4D00);
        // A non-ASCII property name is hashed as UTF-8 bytes.
        assert_eq!(crc28("_MainTex_\u{e9}".as_bytes()), 0x04CB_C132);
    }

    #[test]
    fn absent_saved_properties_yields_an_empty_sheet() {
        let root = SerializedValue::Struct(vec![field("m_Name", string("Bare"))]);
        let view = MaterialView::from_value(&root).unwrap();
        assert_eq!(view.name.as_deref(), Some("Bare"));
        assert_eq!(view.shader, None);
        assert_eq!(view.tex_env_key_shape(), MaterialTexEnvKeyShape::Absent);
        assert!(view.texture_properties().is_empty());
    }

    #[test]
    fn a_null_shader_pointer_is_distinguished_from_a_missing_field() {
        let root = SerializedValue::Struct(vec![field(
            "m_Shader",
            SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            },
        )]);
        let view = MaterialView::from_value(&root).unwrap();
        assert_eq!(
            view.shader, None,
            "a stored null pointer reads as no shader"
        );
    }

    #[test]
    fn a_malformed_pair_is_an_explicit_error() {
        let sheet = SerializedValue::Struct(vec![field(
            "m_TexEnvs",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                "first",
                string("_MainTex"),
            )])]),
        )]);
        let root = SerializedValue::Struct(vec![field("m_SavedProperties", sheet)]);
        assert_eq!(
            MaterialView::from_value(&root),
            Err(MaterialViewError::MalformedPair {
                field: "m_TexEnvs",
                index: 0,
                detail: "the element is missing a 'first' or 'second' field",
            })
        );
    }

    #[test]
    fn a_fast_property_name_key_without_a_name_is_an_error() {
        let sheet = SerializedValue::Struct(vec![field(
            "m_TexEnvs",
            SerializedValue::Array(vec![pair(
                SerializedValue::Struct(vec![field("Other", string("_MainTex"))]),
                tex_env(None),
            )]),
        )]);
        let root = SerializedValue::Struct(vec![field("m_SavedProperties", sheet)]);
        assert_eq!(
            MaterialView::from_value(&root),
            Err(MaterialViewError::MalformedPair {
                field: "m_TexEnvs",
                index: 0,
                detail: "the key struct is missing its 'Name' field",
            })
        );
    }

    #[test]
    fn a_wrong_field_kind_is_an_explicit_error() {
        let root = SerializedValue::Struct(vec![field("m_CustomRenderQueue", string("-1"))]);
        assert!(matches!(
            MaterialView::from_value(&root),
            Err(MaterialViewError::WrongType {
                field: "m_CustomRenderQueue",
                ..
            })
        ));
    }

    /// Generated-API evidence for the serialized field-name mapping.
    #[test]
    fn generated_api_fixture_supports_the_field_name_mapping() {
        let fixture = include_str!("material_generated_api_fixture.txt");
        let mut sections: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut current = String::new();
        for line in fixture.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix("### ") {
                current = name.split(" => ").next().unwrap_or(name).to_owned();
            } else if let Some(name) = line.strip_prefix("TYPE ") {
                current = name.to_owned();
                sections.entry(current.clone()).or_default();
            } else if let Some(property) = line.strip_prefix("  ") {
                sections.entry(current.clone()).or_default().push(
                    property
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_owned(),
                );
            } else if !line.is_empty() && !current.is_empty() {
                sections.entry(current.clone()).or_default().push(
                    line.split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_owned(),
                );
            }
        }
        let properties =
            |section: &str| -> Vec<String> { sections.get(section).cloned().unwrap_or_default() };

        let material = properties("AssetRipper.SourceGenerated.Classes.ClassID_21.IMaterial");
        for expected in [
            "Name_C21",
            "Shader_C21",
            "SavedProperties_C21",
            "ShaderKeywords_C21_AssetList_Utf8String",
            "ShaderKeywords_C21_Utf8String",
            "ValidKeywords_C21",
            "InvalidKeywords_C21",
            "CustomRenderQueue_C21",
            "LightmapFlags_C21",
            "EnableInstancingVariants_C21",
            "DoubleSidedGI_C21",
            "LockedProperties_C21",
            "ModifiedSerializedProperties_C21",
            "Parent_C21",
            "StringTagMap_C21",
            "DisabledShaderPasses_C21",
        ] {
            assert!(material.contains(&expected.to_owned()), "{expected}");
        }

        let sheet = properties(
            "AssetRipper.SourceGenerated.Subclasses.UnityPropertySheet.UnityPropertySheet_5",
        );
        for expected in [
            "TexEnvs_AssetDictionary_FastPropertyName_UnityTexEnv_3_5",
            "TexEnvs_AssetDictionary_FastPropertyName_UnityTexEnv_5",
            "TexEnvs_AssetDictionary_Utf8String_UnityTexEnv_5",
            "Floats_AssetDictionary_FastPropertyName_Single",
            "Floats_AssetDictionary_Utf8String_Single",
            "Colors_AssetDictionary_FastPropertyName_ColorRGBAf",
            "Colors_AssetDictionary_Utf8String_ColorRGBAf",
            "Ints",
            "FlowMappedInYaml",
        ] {
            assert!(sheet.contains(&expected.to_owned()), "{expected}");
        }

        let tex_env =
            properties("AssetRipper.SourceGenerated.Subclasses.UnityTexEnv.UnityTexEnv_3_5");
        for expected in ["Offset", "Scale", "Texture", "FlowMappedInYaml"] {
            assert!(tex_env.contains(&expected.to_owned()), "{expected}");
        }

        let tex_env_5 =
            properties("AssetRipper.SourceGenerated.Subclasses.UnityTexEnv.UnityTexEnv_5");
        for expected in ["Offset", "Scale", "Texture"] {
            assert!(tex_env_5.contains(&expected.to_owned()), "{expected}");
        }
    }
}
