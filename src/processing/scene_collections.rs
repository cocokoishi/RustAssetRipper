//! Classification of serialized scene-collection manager objects.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::special_names::fix_file_identifier;
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};

use super::scene_naming::{
    file_name_to_scene_index, scene_path_from_build_settings, SceneUnityVersion,
};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildSettingsScenePathSet<'a> {
    Missing,
    Unique(&'a [String]),
    Ambiguous,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneCollectionPathStatus {
    BuildSettingsMissing,
    BuildSettingsAmbiguous,
    UnityVersionUnavailable,
    CollectionNameNotMapped,
    StaleBuildSettingsIndex {
        scene_index: usize,
        scene_count: usize,
    },
    BuildSettingsPathUnusable {
        scene_index: usize,
    },
    Resolved {
        scene_index: usize,
        path: String,
        duplicate_path: bool,
    },
}

impl Display for SceneCollectionPathStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuildSettingsMissing => f.write_str("BuildSettings unavailable"),
            Self::BuildSettingsAmbiguous => f.write_str("multiple conflicting BuildSettings assets"),
            Self::UnityVersionUnavailable => f.write_str("Unity version unavailable"),
            Self::CollectionNameNotMapped => {
                f.write_str("collection name has no BuildSettings index")
            }
            Self::StaleBuildSettingsIndex {
                scene_index,
                scene_count,
            } => write!(
                f,
                "collection maps to stale BuildSettings index {scene_index} (only {scene_count} scenes)"
            ),
            Self::BuildSettingsPathUnusable { scene_index } => write!(
                f,
                "BuildSettings path at scene index {scene_index} cannot be recovered"
            ),
            Self::Resolved {
                scene_index,
                path,
                duplicate_path,
            } => write!(
                f,
                "{path} (BuildSettings index {scene_index}, duplicate path={duplicate_path})"
            ),
        }
    }
}

/// Resolve a serialized scene collection name through the source naming and BuildSettings rules.
///
/// SerializedFile names are normalized like the baseline FileBase.NameFixed property before the
/// version-specific `maindata`/`levelN` mapping is applied. Ambiguity and missing metadata remain
/// distinct from stale indexes and unusable paths.
pub fn resolve_scene_collection_path(
    file_name: &str,
    unity_version_text: Option<&str>,
    build_settings: BuildSettingsScenePathSet<'_>,
) -> SceneCollectionPathStatus {
    let build_settings_scenes = match build_settings {
        BuildSettingsScenePathSet::Missing => {
            return SceneCollectionPathStatus::BuildSettingsMissing
        }
        BuildSettingsScenePathSet::Ambiguous => {
            return SceneCollectionPathStatus::BuildSettingsAmbiguous;
        }
        BuildSettingsScenePathSet::Unique(scenes) => scenes,
    };
    let Some(version) = unity_version_text.and_then(SceneUnityVersion::parse) else {
        return SceneCollectionPathStatus::UnityVersionUnavailable;
    };
    let collection_name = fix_file_identifier(file_name);
    let Some(scene_index) = file_name_to_scene_index(&collection_name, version) else {
        return SceneCollectionPathStatus::CollectionNameNotMapped;
    };
    let Some(scene_path) = build_settings_scenes.get(scene_index) else {
        return SceneCollectionPathStatus::StaleBuildSettingsIndex {
            scene_index,
            scene_count: build_settings_scenes.len(),
        };
    };
    let Some(path) = scene_path_from_build_settings(scene_path) else {
        return SceneCollectionPathStatus::BuildSettingsPathUnusable { scene_index };
    };
    let duplicate_path = build_settings_scenes
        .iter()
        .enumerate()
        .any(|(index, candidate)| index != scene_index && candidate == scene_path);
    SceneCollectionPathStatus::Resolved {
        scene_index,
        path,
        duplicate_path,
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
        extract_occlusion_scene_guid, resolve_scene_collection_path, scene_manager_kind,
        BuildSettingsScenePathSet, SceneCollectionPathStatus, SceneGuidError, SceneGuidWords,
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

    #[test]
    fn resolves_scene_collection_names_to_build_settings_paths() {
        let scenes = [
            "Assets/Scenes/Main.unity".to_owned(),
            "Assets/Scenes/Next.unity".to_owned(),
        ];
        assert_eq!(
            resolve_scene_collection_path(
                "MAINDATA",
                Some("5.2.4f1"),
                BuildSettingsScenePathSet::Unique(&scenes),
            ),
            SceneCollectionPathStatus::Resolved {
                scene_index: 0,
                path: "Assets/Scenes/Main".to_owned(),
                duplicate_path: false,
            }
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                Some("5.3.0f1"),
                BuildSettingsScenePathSet::Unique(&scenes),
            ),
            SceneCollectionPathStatus::Resolved {
                scene_index: 0,
                path: "Assets/Scenes/Main".to_owned(),
                duplicate_path: false,
            }
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                Some("5.2.4f1"),
                BuildSettingsScenePathSet::Unique(&scenes),
            ),
            SceneCollectionPathStatus::Resolved {
                scene_index: 1,
                path: "Assets/Scenes/Next".to_owned(),
                duplicate_path: false,
            }
        );
    }

    #[test]
    fn keeps_missing_ambiguous_stale_and_unusable_scene_paths_distinct() {
        let paths = ["".to_owned()];
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                Some("2022.3.1f1"),
                BuildSettingsScenePathSet::Missing
            ),
            SceneCollectionPathStatus::BuildSettingsMissing
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                Some("2022.3.1f1"),
                BuildSettingsScenePathSet::Ambiguous
            ),
            SceneCollectionPathStatus::BuildSettingsAmbiguous
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                None,
                BuildSettingsScenePathSet::Unique(&paths)
            ),
            SceneCollectionPathStatus::UnityVersionUnavailable
        );
        assert_eq!(
            resolve_scene_collection_path(
                "other",
                Some("2022.3.1f1"),
                BuildSettingsScenePathSet::Unique(&paths)
            ),
            SceneCollectionPathStatus::CollectionNameNotMapped
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level1",
                Some("2022.3.1f1"),
                BuildSettingsScenePathSet::Unique(&paths)
            ),
            SceneCollectionPathStatus::StaleBuildSettingsIndex {
                scene_index: 1,
                scene_count: 1
            }
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                Some("2022.3.1f1"),
                BuildSettingsScenePathSet::Unique(&paths)
            ),
            SceneCollectionPathStatus::BuildSettingsPathUnusable { scene_index: 0 }
        );
    }

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }
}
