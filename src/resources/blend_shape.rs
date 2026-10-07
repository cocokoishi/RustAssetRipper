//! Mesh blend-shape name resolution for animation binding lookup.
//!
//! Ports `MeshBlendShapeExtensions.IsCRCMatch`, `BlendShapeDataExtensions.FindShapeNameByCRC`,
//! and `MeshExtensions.FindBlendShapeNameByCRC` (`AssetRipper.SourceGenerated.Extensions`).
//! `CustomCurveResolver` calls `FindBlendShapeNameByCRC` to turn a `BlendShape` binding
//! attribute CRC into a `blendShape.<name>` attribute; the CRC core is the reflected
//! CRC-32 of `AssetRipper.Checksum.Crc32Algorithm` (the same polynomial `material::crc28`
//! uses, without the 28-bit mask).
//!
//! Serialized field names come from the committed class-43 TypeTree dumps
//! (`mesh_typetree_fixture.txt`: `m_Shapes` holding `BlendShapeData` with `vertices`,
//! `shapes`, `channels`, and `fullWeights`; each `MeshBlendShapeChannel` with `name`,
//! `nameHash`, `frameIndex`, and `frameCount`) and from the generated `IMesh` surface
//! (`mesh_generated_api_fixture.txt`: `Shapes`, `ShapesList`). The lookup only decodes
//! the name-bearing records; vertex deltas, frame ranges, and full weights stay with
//! the mesh-geometry reader. The `SetValues` field assignments mutate a
//! generated channel object and stay caller-side; only its name-hash derivation
//! ports (`blend_shape_channel_name_hash`).
//!
//! Differential evidence: the CRC words reuse the shipped-`AssetRipper.Checksum.dll`
//! probe rows in `crc28_fixture.txt` (`Crc32Algorithm.HashUTF8` plus the `MatchUTF8`
//! accept/reject columns); the branch dispatch is asserted directly.
//!
//! The `BlendShape` branch of `CustomCurveResolver.ToAttributeName` resolves through
//! this file's lookup: unknown paths keep their path, each hierarchy root contributes
//! its `FindBlendShapeNameByCRC` hit in order with misses continuing the walk, and a
//! miss everywhere falls back to the `Crc32Algorithm.ReverseAscii` manufactured name.
//! Scene-graph walks and the reverse checksum stay caller-side (the checksum package
//! is outside the frozen snapshot); `blend_shape_attribute_name` pins the branch
//! composition over those caller facts.
//!
//! Open: channel `frameIndex`/`frameCount` ranges and `fullWeights` have no readers in
//! the frozen snapshot (a whole-`Source` search for `fullWeights`/`FullWeights` finds
//! only serialized declarations), so no frame-selection kernel is asserted here.

use std::error::Error;
use std::fmt::{self, Display};

use super::custom_curve::BLEND_SHAPE_PREFIX;

use crate::serialized_file::object::{
    SerializedFieldValue, SerializedValue, SerializedValueLookupError,
};

/// The serialized mesh field carrying `BlendShapeData`.
pub const BLEND_SHAPES_FIELD: &str = "m_Shapes";

/// The serialized mesh field carrying the legacy shape list (`MeshBlendShape_4_1`).
pub const BLEND_SHAPES_LIST_FIELD: &str = "m_ShapesList";

/// `BlendShapeData` channel list field.
const CHANNELS_FIELD: &str = "channels";

/// `MeshBlendShapeChannel` name field.
const CHANNEL_NAME_FIELD: &str = "name";

/// `MeshBlendShapeChannel` name-hash field.
const CHANNEL_NAME_HASH_FIELD: &str = "nameHash";

/// Legacy `MeshBlendShape_4_1` name field.
const SHAPE_NAME_FIELD: &str = "name";

/// A failure while reading a blend-shape lookup view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BlendShapeViewError {
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

impl Display for BlendShapeViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field, source } => {
                write!(
                    formatter,
                    "could not read blend-shape field '{field}': {source}"
                )
            }
            Self::WrongType { field, actual } => {
                write!(
                    formatter,
                    "blend-shape field '{field}' has kind {actual}, which the lookup cannot read here"
                )
            }
        }
    }
}

impl Error for BlendShapeViewError {}

/// The reflected CRC-32 of `AssetRipper.Checksum.Crc32Algorithm` (`HashUTF8`/`HashData`).
///
/// Bit-identical to the core of `material::crc28`, without the 28-bit mask: the
/// `crc28_fixture.txt` probe shows every Crc28 word equals the Crc32 word masked to
/// 28 bits, so the unmasked word is the full CRC-32.
pub fn crc32_ieee(bytes: &[u8]) -> u32 {
    !crc32_ieee_update(0xFFFF_FFFF, bytes)
}

/// Update an uncomplemented IEEE CRC-32 state with another byte slice.
///
/// This permits source calls that append multiple spans to one CRC stream
/// without allocating a concatenated buffer. Start with `0xFFFF_FFFF` and
/// complement the final state once.
pub(crate) fn crc32_ieee_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = if crc & 1 != 0 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    crc
}

/// `MeshBlendShapeExtensions.IsCRCMatch`: a present name matches when its UTF-8
/// CRC-32 equals `crc`; an absent name never matches.
pub fn is_crc_match(name: Option<&str>, crc: u32) -> bool {
    name.is_some_and(|text| crc32_ieee(text.as_bytes()) == crc)
}

/// `MeshBlendShapeChannelExtensions.SetValues` name-hash half: the stored `NameHash`
/// is the UTF-8 CRC-32 of the channel name. The name, frame index, and frame count
/// assignments mutate the generated channel object and stay caller-side.
pub fn blend_shape_channel_name_hash(name: &str) -> u32 {
    crc32_ieee(name.as_bytes())
}

/// One `BlendShapeData` channel name record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlendShapeChannelName {
    /// `name`; absent when the record carries no name.
    pub name: Option<String>,
    /// `nameHash`; absent when the record carries no hash.
    pub name_hash: Option<u32>,
}

/// `BlendShapeDataExtensions.FindShapeNameByCRC`: the first channel whose stored
/// `nameHash` equals `crc` supplies the name. A channel without a hash never
/// matches; a matching channel without a name yields no name.
pub fn find_channel_name_by_crc(channels: &[BlendShapeChannelName], crc: u32) -> Option<&str> {
    channels
        .iter()
        .find(|channel| channel.name_hash == Some(crc))
        .and_then(|channel| channel.name.as_deref())
}

/// The name records a mesh lookup reads: `m_Shapes` channels or legacy
/// `m_ShapesList` names.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlendShapeLookupView {
    /// `m_Shapes`/`channels` records, when `m_Shapes` is present.
    pub channels: Option<Vec<BlendShapeChannelName>>,
    /// `m_ShapesList` names, when `m_ShapesList` is present. A `None` entry is a
    /// shape record without a name, which never matches.
    pub shapes_list_names: Option<Vec<Option<String>>>,
}

impl BlendShapeLookupView {
    /// Reads the name records from a decoded `Mesh` payload.
    ///
    /// Absent `m_Shapes`/`m_ShapesList` fields read as absent (the `Has_` gates);
    /// a present-but-wrongly-typed field is an explicit error.
    pub fn from_value(root: &SerializedValue) -> Result<Self, BlendShapeViewError> {
        let channels = match lookup(root, BLEND_SHAPES_FIELD)? {
            None => None,
            Some(value) => {
                let fields = as_struct(value, BLEND_SHAPES_FIELD)?;
                let mut records = Vec::new();
                if let Some(list) = find_field(fields, CHANNELS_FIELD)? {
                    for item in as_array(list, CHANNELS_FIELD)? {
                        records.push(read_channel(item)?);
                    }
                }
                Some(records)
            }
        };
        let shapes_list_names = match lookup(root, BLEND_SHAPES_LIST_FIELD)? {
            None => None,
            Some(value) => {
                let mut names = Vec::new();
                for item in as_array(value, BLEND_SHAPES_LIST_FIELD)? {
                    let fields = as_struct(item, BLEND_SHAPES_LIST_FIELD)?;
                    names.push(optional_lossy_string(fields, SHAPE_NAME_FIELD)?);
                }
                Some(names)
            }
        };
        Ok(Self {
            channels,
            shapes_list_names,
        })
    }

    /// `MeshExtensions.FindBlendShapeNameByCRC`: with `m_Shapes` present the
    /// channel table decides (even when it finds nothing); otherwise the legacy
    /// list is scanned with `IsCRCMatch` semantics. No shape data means no name.
    pub fn find_shape_name_by_crc(&self, crc: u32) -> Option<&str> {
        if let Some(channels) = &self.channels {
            return find_channel_name_by_crc(channels, crc);
        }
        if let Some(names) = &self.shapes_list_names {
            for name in names {
                if is_crc_match(name.as_deref(), crc) {
                    return name.as_deref();
                }
            }
        }
        None
    }
}

/// The `BindingCustomType.BlendShape` branch of
/// `CustomCurveResolver.ToAttributeName` over caller facts.
///
/// An unknown path (see `custom_curve::is_unknown_path`) bypasses lookup and keeps the
/// path verbatim after the prefix. Otherwise each hierarchy root contributes its
/// `FindBlendShapeNameByCRC` hit in walk order: a root without a child, skin, mesh, or
/// matching name contributes `None` and lookup continues, and the first name wins. With
/// no hit anywhere, the source falls back to the `Crc32Algorithm.ReverseAscii`
/// manufactured name, which stays caller-side and arrives here as `fallback_name`
/// (`AssetRipper/Source/AssetRipper.Processing/AnimationClips/CustomCurveResolver.cs`
/// lines 41-76).
pub fn blend_shape_attribute_name(
    path_is_unknown: bool,
    path: &str,
    resolved_names: &[Option<&str>],
    fallback_name: &str,
) -> String {
    if path_is_unknown {
        return format!("{BLEND_SHAPE_PREFIX}{path}");
    }
    if let Some(name) = resolved_names.iter().flatten().next() {
        return format!("{BLEND_SHAPE_PREFIX}{name}");
    }
    format!("{BLEND_SHAPE_PREFIX}{fallback_name}")
}

fn read_channel(value: &SerializedValue) -> Result<BlendShapeChannelName, BlendShapeViewError> {
    let fields = as_struct(value, CHANNELS_FIELD)?;
    Ok(BlendShapeChannelName {
        name: optional_lossy_string(fields, CHANNEL_NAME_FIELD)?,
        name_hash: optional_u32(fields, CHANNEL_NAME_HASH_FIELD)?,
    })
}

fn find_field<'a>(
    fields: &'a [SerializedFieldValue],
    name: &'static str,
) -> Result<Option<&'a SerializedValue>, BlendShapeViewError> {
    let mut matching = fields.iter().filter(|field| field.name == name);
    let Some(first) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        let match_count = fields.iter().filter(|field| field.name == name).count();
        return Err(BlendShapeViewError::FieldLookup {
            field: name,
            source: SerializedValueLookupError::DuplicateField {
                name: name.to_owned(),
                match_count,
            },
        });
    }
    Ok(Some(&first.value))
}

fn lookup<'a>(
    root: &'a SerializedValue,
    name: &'static str,
) -> Result<Option<&'a SerializedValue>, BlendShapeViewError> {
    root.unique_field_in_base_chain(name)
        .map_err(|source| BlendShapeViewError::FieldLookup {
            field: name,
            source,
        })
}

fn as_struct<'a>(
    value: &'a SerializedValue,
    field: &'static str,
) -> Result<&'a [SerializedFieldValue], BlendShapeViewError> {
    match value {
        SerializedValue::Struct(fields) => Ok(fields),
        other => Err(BlendShapeViewError::WrongType {
            field,
            actual: serialized_kind(other),
        }),
    }
}

fn as_array<'a>(
    value: &'a SerializedValue,
    field: &'static str,
) -> Result<&'a [SerializedValue], BlendShapeViewError> {
    match value {
        SerializedValue::Array(items) => Ok(items),
        other => Err(BlendShapeViewError::WrongType {
            field,
            actual: serialized_kind(other),
        }),
    }
}

fn optional_lossy_string(
    fields: &[SerializedFieldValue],
    name: &'static str,
) -> Result<Option<String>, BlendShapeViewError> {
    match find_field(fields, name)? {
        None => Ok(None),
        Some(SerializedValue::StringBytes(bytes)) => {
            Ok(Some(String::from_utf8_lossy(bytes).into_owned()))
        }
        Some(other) => Err(BlendShapeViewError::WrongType {
            field: name,
            actual: serialized_kind(other),
        }),
    }
}

fn optional_u32(
    fields: &[SerializedFieldValue],
    name: &'static str,
) -> Result<Option<u32>, BlendShapeViewError> {
    match find_field(fields, name)? {
        None => Ok(None),
        Some(SerializedValue::U32(value)) => Ok(Some(*value)),
        Some(SerializedValue::I32(value)) => Ok(Some(*value as u32)),
        Some(other) => Err(BlendShapeViewError::WrongType {
            field: name,
            actual: serialized_kind(other),
        }),
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
        SerializedValue::ManagedReferenceId { .. } => "managed reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
        SerializedValue::Raw { .. } => "raw data",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn struct_value(fields: Vec<SerializedFieldValue>) -> SerializedValue {
        SerializedValue::Struct(fields)
    }

    fn channel(name: Option<&str>, hash: Option<u32>) -> SerializedValue {
        let mut fields = Vec::new();
        if let Some(text) = name {
            fields.push(field(
                CHANNEL_NAME_FIELD,
                SerializedValue::StringBytes(text.as_bytes().to_vec()),
            ));
        }
        if let Some(word) = hash {
            fields.push(field(CHANNEL_NAME_HASH_FIELD, SerializedValue::U32(word)));
        }
        struct_value(fields)
    }

    fn shapes_root(channels: Vec<SerializedValue>) -> SerializedValue {
        struct_value(vec![field(
            BLEND_SHAPES_FIELD,
            struct_value(vec![field(
                CHANNELS_FIELD,
                SerializedValue::Array(channels),
            )]),
        )])
    }

    fn shapes_list_root(names: Vec<Option<&str>>) -> SerializedValue {
        struct_value(vec![field(
            BLEND_SHAPES_LIST_FIELD,
            SerializedValue::Array(
                names
                    .into_iter()
                    .map(|name| {
                        let mut fields = Vec::new();
                        if let Some(text) = name {
                            fields.push(field(
                                SHAPE_NAME_FIELD,
                                SerializedValue::StringBytes(text.as_bytes().to_vec()),
                            ));
                        }
                        struct_value(fields)
                    })
                    .collect(),
            ),
        )])
    }

    /// Differential against the shipped `AssetRipper.Checksum.dll` words committed
    /// in `crc28_fixture.txt` (probe `Temp/opencode/crcprobe`): the `Crc32` column
    /// pins `HashUTF8`, and the `MatchUTF8` columns pin accept/reject, which is the
    /// whole `IsCRCMatch` semantics over a present name.
    #[test]
    fn crc32_and_match_reproduce_the_shipped_checksum_words() {
        let fixture = include_str!("crc28_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts.len(), 7, "{line}");
            let input = parts[1];
            let crc = u32::from_str_radix(parts[4], 16).unwrap();
            assert_eq!(crc32_ieee(input.as_bytes()), crc, "{line}");
            assert_eq!(parts[5], "True", "{line}");
            assert_eq!(parts[6], "False", "{line}");
            assert!(is_crc_match(Some(input), crc), "{line}");
            assert!(!is_crc_match(Some(input), crc ^ 1), "{line}");
            assert!(!is_crc_match(None, crc), "{line}");
            rows += 1;
        }
        assert!(rows >= 12, "the fixture pins at least twelve words");
    }

    #[test]
    fn channel_lookup_returns_the_first_hash_match() {
        let blink = crc32_ieee(b"Blink_L");
        let smile = crc32_ieee(b"Smile");
        let channels = vec![
            BlendShapeChannelName {
                name: Some("Smile".to_owned()),
                name_hash: Some(smile),
            },
            BlendShapeChannelName {
                name: Some("Blink_L".to_owned()),
                name_hash: Some(blink),
            },
            // A stored hash without a name never yields a name.
            BlendShapeChannelName {
                name: None,
                name_hash: Some(0x1234_5678),
            },
        ];
        assert_eq!(find_channel_name_by_crc(&channels, smile), Some("Smile"));
        assert_eq!(find_channel_name_by_crc(&channels, blink), Some("Blink_L"));
        assert_eq!(find_channel_name_by_crc(&channels, 0x1234_5678), None);
        assert_eq!(find_channel_name_by_crc(&channels, 0xDEAD_BEEF), None);
        assert_eq!(find_channel_name_by_crc(&[], smile), None);
    }

    #[test]
    fn mesh_lookup_prefers_channels_without_fallthrough() {
        let smile = crc32_ieee(b"Smile");
        let view = BlendShapeLookupView {
            channels: Some(vec![BlendShapeChannelName {
                name: Some("Smile".to_owned()),
                name_hash: Some(smile),
            }]),
            shapes_list_names: Some(vec![Some("Smile".to_owned())]),
        };
        assert_eq!(view.find_shape_name_by_crc(smile), Some("Smile"));
        // Channels present but silent: the source returns at once and never
        // consults the legacy list, even though it holds the same name.
        let mismatched = BlendShapeLookupView {
            channels: Some(vec![BlendShapeChannelName {
                name: Some("Frown".to_owned()),
                name_hash: Some(crc32_ieee(b"Frown")),
            }]),
            shapes_list_names: Some(vec![Some("Smile".to_owned())]),
        };
        assert_eq!(mismatched.find_shape_name_by_crc(smile), None);
    }

    #[test]
    fn mesh_lookup_scans_the_legacy_list_with_crc_match() {
        let blink = crc32_ieee(b"Blink_L");
        let view = BlendShapeLookupView {
            channels: None,
            shapes_list_names: Some(vec![None, Some("Blink_L".to_owned())]),
        };
        assert_eq!(view.find_shape_name_by_crc(blink), Some("Blink_L"));
        assert_eq!(view.find_shape_name_by_crc(0xDEAD_BEEF), None);
        let empty = BlendShapeLookupView::default();
        assert_eq!(empty.find_shape_name_by_crc(blink), None);
    }

    #[test]
    fn view_decodes_both_name_records() {
        let smile = crc32_ieee(b"Smile");
        let root = struct_value(vec![
            field(
                BLEND_SHAPES_FIELD,
                struct_value(vec![field(
                    CHANNELS_FIELD,
                    SerializedValue::Array(vec![
                        channel(Some("Smile"), Some(smile)),
                        channel(None, Some(0x1234_5678)),
                    ]),
                )]),
            ),
            field(
                BLEND_SHAPES_LIST_FIELD,
                SerializedValue::Array(vec![struct_value(vec![field(
                    SHAPE_NAME_FIELD,
                    SerializedValue::StringBytes(b"Smile".to_vec()),
                )])]),
            ),
        ]);
        let view = BlendShapeLookupView::from_value(&root).unwrap();
        assert_eq!(view.channels.as_ref().map(Vec::len), Some(2));
        assert_eq!(view.shapes_list_names.as_ref().map(Vec::len), Some(1));
        assert_eq!(view.find_shape_name_by_crc(smile), Some("Smile"));
    }

    #[test]
    fn view_rejects_wrongly_typed_fields() {
        let root = struct_value(vec![field(
            BLEND_SHAPES_FIELD,
            SerializedValue::Array(vec![]),
        )]);
        assert!(matches!(
            BlendShapeLookupView::from_value(&root),
            Err(BlendShapeViewError::WrongType { .. })
        ));
        let root = shapes_root(vec![struct_value(vec![field(
            CHANNEL_NAME_HASH_FIELD,
            SerializedValue::StringBytes(b"nope".to_vec()),
        )])]);
        assert!(matches!(
            BlendShapeLookupView::from_value(&root),
            Err(BlendShapeViewError::WrongType { .. })
        ));
        let root = struct_value(vec![
            field(BLEND_SHAPES_FIELD, struct_value(vec![])),
            field(BLEND_SHAPES_FIELD, struct_value(vec![])),
        ]);
        assert!(matches!(
            BlendShapeLookupView::from_value(&root),
            Err(BlendShapeViewError::FieldLookup { .. })
        ));
    }

    #[test]
    fn absent_shape_fields_read_as_absent() {
        let view = BlendShapeLookupView::from_value(&struct_value(vec![])).unwrap();
        assert_eq!(view, BlendShapeLookupView::default());
        // A present shape block without a channel list behaves as an empty table.
        let view = BlendShapeLookupView::from_value(&struct_value(vec![field(
            BLEND_SHAPES_FIELD,
            struct_value(vec![]),
        )]))
        .unwrap();
        assert_eq!(view.channels, Some(Vec::new()));
    }

    #[test]
    fn legacy_list_decodes_through_the_view() {
        let view =
            BlendShapeLookupView::from_value(&shapes_list_root(vec![Some("Smile"), None])).unwrap();
        assert_eq!(
            view.shapes_list_names,
            Some(vec![Some("Smile".to_owned()), None])
        );
        assert_eq!(
            view.find_shape_name_by_crc(crc32_ieee(b"Smile")),
            Some("Smile")
        );
    }

    #[test]
    fn blend_shape_branch_composes_unknown_first_hit_and_fallback() {
        // Unknown paths bypass lookup entirely, even when resolutions exist.
        assert_eq!(
            blend_shape_attribute_name(true, "path_0x1A2B_HJKLMNN", &[Some("Smile")], "fallback"),
            "blendShape.path_0x1A2B_HJKLMNN"
        );
        // The first hit in walk order wins; later roots are ignored.
        assert_eq!(
            blend_shape_attribute_name(
                false,
                "Hips/Face",
                &[Some("Smile"), Some("Frown")],
                "fallback"
            ),
            "blendShape.Smile"
        );
        // Misses continue the walk to the next root.
        assert_eq!(
            blend_shape_attribute_name(false, "Hips/Face", &[None, Some("Blink_L")], "fallback"),
            "blendShape.Blink_L"
        );
        // No hit anywhere falls back to the manufactured name.
        assert_eq!(
            blend_shape_attribute_name(false, "Hips/Face", &[None, None], "path_0xDEAD_XYZABC"),
            "blendShape.path_0xDEAD_XYZABC"
        );
        assert_eq!(
            blend_shape_attribute_name(false, "Hips/Face", &[], "path_0xDEAD_XYZABC"),
            "blendShape.path_0xDEAD_XYZABC"
        );
    }

    /// The `SetValues` hash half composes with the lookup: a hashed name resolves
    /// back through the channel table and the legacy CRC scan alike.
    #[test]
    fn channel_name_hash_round_trips_through_lookup() {
        let hash = blend_shape_channel_name_hash("Smile");
        assert_eq!(hash, crc32_ieee(b"Smile"));
        let channels = [BlendShapeChannelName {
            name: Some("Smile".to_owned()),
            name_hash: Some(hash),
        }];
        assert_eq!(find_channel_name_by_crc(&channels, hash), Some("Smile"));
        assert_eq!(find_channel_name_by_crc(&channels, hash ^ 1), None);
        let view = BlendShapeLookupView {
            channels: None,
            shapes_list_names: Some(vec![Some("Smile".to_owned())]),
        };
        assert_eq!(view.find_shape_name_by_crc(hash), Some("Smile"));
        assert!(is_crc_match(Some("Smile"), hash));
    }
}
