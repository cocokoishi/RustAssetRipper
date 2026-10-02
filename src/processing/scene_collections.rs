//! Classification of serialized scene-collection manager objects.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};

/// AssetRipper's generated ClassIDType extension marks these four classes as
/// inheriting from LevelGameManager.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneManagerKind {
    OcclusionCullingSettings,
    RenderSettings,
    LightmapSettings,
    NavMeshSettings,
}

impl SceneManagerKind {
    pub const fn class_id(self) -> i32 {
        match self {
            Self::OcclusionCullingSettings => 29,
            Self::RenderSettings => 104,
            Self::LightmapSettings => 157,
            Self::NavMeshSettings => 196,
        }
    }
}

/// Return the source LevelGameManager implementation represented by a class ID.
pub const fn scene_manager_kind(class_id: Option<i32>) -> Option<SceneManagerKind> {
    match class_id {
        Some(29) => Some(SceneManagerKind::OcclusionCullingSettings),
        Some(104) => Some(SceneManagerKind::RenderSettings),
        Some(157) => Some(SceneManagerKind::LightmapSettings),
        Some(196) => Some(SceneManagerKind::NavMeshSettings),
        _ => None,
    }
}

/// The four serialized 32-bit words of an OcclusionCullingSettings SceneGUID.
///
/// Keeping the source words avoids introducing a byte-order conversion that is not part of the
/// scene collection model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneGuidWords {
    pub data: [u32; 4],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneGuidError {
    FieldLookup {
        field_name: &'static str,
        source: SerializedValueLookupError,
    },
    DuplicateAliases {
        primary: &'static str,
        alternate: &'static str,
    },
    SceneGuidHasWrongType {
        actual: &'static str,
    },
    MissingGuidWord {
        word_index: usize,
    },
    GuidWordHasWrongType {
        word_index: usize,
        actual: &'static str,
    },
}

impl Display for SceneGuidError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field_name, source } => {
                write!(f, "SceneGUID field {field_name} lookup failed: {source:?}")
            }
            Self::DuplicateAliases { primary, alternate } => {
                write!(f, "both {primary} and {alternate} fields are present")
            }
            Self::SceneGuidHasWrongType { actual } => {
                write!(f, "SceneGUID has type {actual}; expected a struct")
            }
            Self::MissingGuidWord { word_index } => {
                write!(f, "GUID struct is missing data word {word_index}")
            }
            Self::GuidWordHasWrongType { word_index, actual } => write!(
                f,
                "GUID data word {word_index} has type {actual}; expected u32"
            ),
        }
    }
}

impl Error for SceneGuidError {}

/// Extract an optional SceneGUID from a decoded OcclusionCullingSettings root.
///
/// The active TypeTree may omit the field. When present, both the source field name and its four
/// Unity GUID words must be uniquely represented; malformed or ambiguous values are errors.
pub fn extract_occlusion_scene_guid(
    root: &SerializedValue,
) -> Result<Option<SceneGuidWords>, SceneGuidError> {
    let scene_guid = unique_alias_field(root, "SceneGUID", "m_SceneGUID")?;
    let Some(scene_guid) = scene_guid else {
        return Ok(None);
    };
    let SerializedValue::Struct(_) = scene_guid else {
        return Err(SceneGuidError::SceneGuidHasWrongType {
            actual: value_type_name(scene_guid),
        });
    };

    let mut data = [0; 4];
    for (word_index, word) in data.iter_mut().enumerate() {
        let primary = GUID_WORD_FIELDS[word_index];
        let alternate = GUID_WORD_ALIASES[word_index];
        let value = unique_alias_field(scene_guid, primary, alternate)?
            .ok_or(SceneGuidError::MissingGuidWord { word_index })?;
        let SerializedValue::U32(value) = value else {
            return Err(SceneGuidError::GuidWordHasWrongType {
                word_index,
                actual: value_type_name(value),
            });
        };
        *word = *value;
    }
    Ok(Some(SceneGuidWords { data }))
}

const GUID_WORD_FIELDS: [&str; 4] = ["m_Data_0_", "m_Data_1_", "m_Data_2_", "m_Data_3_"];
const GUID_WORD_ALIASES: [&str; 4] = ["Data_0_", "Data_1_", "Data_2_", "Data_3_"];

fn unique_alias_field<'a>(
    root: &'a SerializedValue,
    primary: &'static str,
    alternate: &'static str,
) -> Result<Option<&'a SerializedValue>, SceneGuidError> {
    let primary_value =
        root.unique_field_in_base_chain(primary)
            .map_err(|source| SceneGuidError::FieldLookup {
                field_name: primary,
                source,
            })?;
    let alternate_value = root
        .unique_field_in_base_chain(alternate)
        .map_err(|source| SceneGuidError::FieldLookup {
            field_name: alternate,
            source,
        })?;
    match (primary_value, alternate_value) {
        (Some(_), Some(_)) => Err(SceneGuidError::DuplicateAliases { primary, alternate }),
        (Some(value), None) | (None, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    }
}

fn value_type_name(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "i8",
        SerializedValue::U8(_) => "u8",
        SerializedValue::I16(_) => "i16",
        SerializedValue::U16(_) => "u16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "i32",
        SerializedValue::U32(_) => "u32",
        SerializedValue::I64(_) => "i64",
        SerializedValue::U64(_) => "u64",
        SerializedValue::F32(_) => "f32",
        SerializedValue::F64(_) => "f64",
        SerializedValue::StringBytes(_) => "string",
        SerializedValue::Bytes(_) => "bytes",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::Raw { .. } => "raw",
    }
}

#[cfg(test)]
mod tests {
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

    use super::{
        extract_occlusion_scene_guid, scene_manager_kind, SceneGuidError, SceneGuidWords,
        SceneManagerKind,
    };

    #[test]
    fn identifies_the_baseline_level_game_manager_class_ids() {
        for (class_id, expected) in [
            (29, SceneManagerKind::OcclusionCullingSettings),
            (104, SceneManagerKind::RenderSettings),
            (157, SceneManagerKind::LightmapSettings),
            (196, SceneManagerKind::NavMeshSettings),
        ] {
            assert_eq!(scene_manager_kind(Some(class_id)), Some(expected));
            assert_eq!(expected.class_id(), class_id);
        }
        for class_id in [None, Some(1), Some(4), Some(141), Some(238), Some(1045)] {
            assert_eq!(scene_manager_kind(class_id), None);
        }
    }

    #[test]
    fn extracts_scene_guid_words_through_inherited_fields() {
        let root = SerializedValue::Struct(vec![field(
            "Base",
            SerializedValue::Struct(vec![field(
                "m_SceneGUID",
                SerializedValue::Struct(vec![
                    field("m_Data_0_", SerializedValue::U32(0x0123_4567)),
                    field("m_Data_1_", SerializedValue::U32(0x89ab_cdef)),
                    field("m_Data_2_", SerializedValue::U32(0x1020_3040)),
                    field("m_Data_3_", SerializedValue::U32(0x5060_7080)),
                ]),
            )]),
        )]);
        assert_eq!(
            extract_occlusion_scene_guid(&root).unwrap(),
            Some(SceneGuidWords {
                data: [0x0123_4567, 0x89ab_cdef, 0x1020_3040, 0x5060_7080]
            })
        );
    }

    #[test]
    fn scene_guid_extraction_preserves_absence_and_rejects_malformed_shapes() {
        assert_eq!(
            extract_occlusion_scene_guid(&SerializedValue::Struct(Vec::new())).unwrap(),
            None
        );

        let duplicate_aliases = SerializedValue::Struct(vec![
            field("SceneGUID", SerializedValue::Struct(Vec::new())),
            field("m_SceneGUID", SerializedValue::Struct(Vec::new())),
        ]);
        assert_eq!(
            extract_occlusion_scene_guid(&duplicate_aliases),
            Err(SceneGuidError::DuplicateAliases {
                primary: "SceneGUID",
                alternate: "m_SceneGUID"
            })
        );

        let missing_word = SerializedValue::Struct(vec![field(
            "SceneGUID",
            SerializedValue::Struct(vec![field("m_Data_0_", SerializedValue::U32(1))]),
        )]);
        assert_eq!(
            extract_occlusion_scene_guid(&missing_word),
            Err(SceneGuidError::MissingGuidWord { word_index: 1 })
        );

        let wrong_word_type = SerializedValue::Struct(vec![field(
            "SceneGUID",
            SerializedValue::Struct(vec![
                field("m_Data_0_", SerializedValue::U32(1)),
                field("m_Data_1_", SerializedValue::I32(2)),
                field("m_Data_2_", SerializedValue::U32(3)),
                field("m_Data_3_", SerializedValue::U32(4)),
            ]),
        )]);
        assert_eq!(
            extract_occlusion_scene_guid(&wrong_word_type),
            Err(SceneGuidError::GuidWordHasWrongType {
                word_index: 1,
                actual: "i32"
            })
        );
    }

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }
}
