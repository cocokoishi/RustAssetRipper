//! Typed extraction of scene-related metadata from serialized Unity values.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str;

use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};

use super::scene_naming::{scene_path_for_collection, SceneUnityVersion};

/// Limits for materializing BuildSettings scene paths from one decoded object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuildSettingsSceneLimits {
    pub max_scenes: usize,
    pub max_total_path_bytes: usize,
}

impl Default for BuildSettingsSceneLimits {
    fn default() -> Self {
        Self {
            max_scenes: 65_536,
            max_total_path_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Extract ordered scene paths from a decoded BuildSettings object.
///
/// Current TypeTrees use `m_Scenes`; older layouts used `m_Levels`, which the source generator
/// normalizes to `m_Scenes`. Entries may be serialized strings or structs with a `path` field.
pub fn extract_build_settings_scene_paths(
    root: &SerializedValue,
    limits: BuildSettingsSceneLimits,
) -> Result<Vec<String>, BuildSettingsSceneError> {
    let scenes_field = root
        .unique_field_in_base_chain("m_Scenes")
        .map_err(|source| BuildSettingsSceneError::FieldLookup {
            field_name: "m_Scenes",
            source,
        })?;
    let levels_field = root
        .unique_field_in_base_chain("m_Levels")
        .map_err(|source| BuildSettingsSceneError::FieldLookup {
            field_name: "m_Levels",
            source,
        })?;
    let entries = match (scenes_field, levels_field) {
        (Some(_), Some(_)) => return Err(BuildSettingsSceneError::BothSceneArrayFieldsPresent),
        (Some(value), None) | (None, Some(value)) => value,
        (None, None) => return Err(BuildSettingsSceneError::MissingSceneArray),
    };
    let Some(entries) = entries.array_elements() else {
        return Err(BuildSettingsSceneError::SceneArrayHasWrongType {
            actual: value_type_name(entries),
        });
    };
    if entries.len() > limits.max_scenes {
        return Err(BuildSettingsSceneError::SceneCountLimitExceeded {
            count: entries.len(),
            limit: limits.max_scenes,
        });
    }

    let mut paths = Vec::new();
    paths.try_reserve_exact(entries.len()).map_err(|_| {
        BuildSettingsSceneError::AllocationFailed {
            resource: "scene path list",
            requested: entries.len(),
        }
    })?;
    let mut total_path_bytes = 0usize;
    for (scene_index, entry) in entries.iter().enumerate() {
        let path_bytes = scene_path_bytes(entry, scene_index)?;
        total_path_bytes = total_path_bytes.checked_add(path_bytes.len()).ok_or(
            BuildSettingsSceneError::CountOverflow {
                resource: "BuildSettings scene path bytes",
            },
        )?;
        if total_path_bytes > limits.max_total_path_bytes {
            return Err(BuildSettingsSceneError::PathBytesLimitExceeded {
                requested: total_path_bytes,
                limit: limits.max_total_path_bytes,
            });
        }
        let path = str::from_utf8(path_bytes).map_err(|error| {
            BuildSettingsSceneError::InvalidScenePathUtf8 {
                scene_index,
                valid_up_to: error.valid_up_to(),
            }
        })?;
        let mut owned_path = String::new();
        owned_path.try_reserve_exact(path.len()).map_err(|_| {
            BuildSettingsSceneError::AllocationFailed {
                resource: "BuildSettings scene path",
                requested: path.len(),
            }
        })?;
        owned_path.push_str(path);
        paths.push(owned_path);
    }
    Ok(paths)
}

/// Resolve one collection name against a decoded BuildSettings object.
pub fn scene_path_from_build_settings_value(
    collection_name: &str,
    version: SceneUnityVersion,
    build_settings_root: &SerializedValue,
    limits: BuildSettingsSceneLimits,
) -> Result<Option<String>, BuildSettingsSceneError> {
    let scenes = extract_build_settings_scene_paths(build_settings_root, limits)?;
    Ok(scene_path_for_collection(collection_name, version, &scenes))
}

fn scene_path_bytes(
    entry: &SerializedValue,
    scene_index: usize,
) -> Result<&[u8], BuildSettingsSceneError> {
    if let SerializedValue::StringBytes(bytes) = entry {
        return Ok(bytes);
    }
    let SerializedValue::Struct(_) = entry else {
        return Err(BuildSettingsSceneError::SceneEntryHasWrongType {
            scene_index,
            actual: value_type_name(entry),
        });
    };
    let path = entry.unique_field_in_base_chain("path").map_err(|source| {
        BuildSettingsSceneError::FieldLookup {
            field_name: "path",
            source,
        }
    })?;
    match path {
        Some(SerializedValue::StringBytes(bytes)) => Ok(bytes),
        Some(other) => Err(BuildSettingsSceneError::ScenePathHasWrongType {
            scene_index,
            actual: value_type_name(other),
        }),
        None => Err(BuildSettingsSceneError::MissingScenePath { scene_index }),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildSettingsSceneError {
    FieldLookup {
        field_name: &'static str,
        source: SerializedValueLookupError,
    },
    BothSceneArrayFieldsPresent,
    MissingSceneArray,
    SceneArrayHasWrongType {
        actual: &'static str,
    },
    SceneCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    SceneEntryHasWrongType {
        scene_index: usize,
        actual: &'static str,
    },
    MissingScenePath {
        scene_index: usize,
    },
    ScenePathHasWrongType {
        scene_index: usize,
        actual: &'static str,
    },
    InvalidScenePathUtf8 {
        scene_index: usize,
        valid_up_to: usize,
    },
    PathBytesLimitExceeded {
        requested: usize,
        limit: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for BuildSettingsSceneError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field_name, source } => {
                write!(f, "could not read BuildSettings field '{field_name}': {source}")
            }
            Self::BothSceneArrayFieldsPresent => {
                f.write_str("BuildSettings contains both m_Scenes and legacy m_Levels")
            }
            Self::MissingSceneArray => {
                f.write_str("BuildSettings contains neither m_Scenes nor legacy m_Levels")
            }
            Self::SceneArrayHasWrongType { actual } => write!(
                f,
                "BuildSettings scene list has type {actual}; expected an array"
            ),
            Self::SceneCountLimitExceeded { count, limit } => write!(
                f,
                "BuildSettings scene count {count} exceeds configured limit {limit}"
            ),
            Self::SceneEntryHasWrongType { scene_index, actual } => write!(
                f,
                "BuildSettings scene entry {scene_index} has type {actual}; expected a string or struct"
            ),
            Self::MissingScenePath { scene_index } => write!(
                f,
                "BuildSettings scene entry {scene_index} has no path field"
            ),
            Self::ScenePathHasWrongType { scene_index, actual } => write!(
                f,
                "BuildSettings scene entry {scene_index} path has type {actual}; expected a string"
            ),
            Self::InvalidScenePathUtf8 {
                scene_index,
                valid_up_to,
            } => write!(
                f,
                "BuildSettings scene entry {scene_index} path has invalid UTF-8 beginning at byte {valid_up_to}"
            ),
            Self::PathBytesLimitExceeded { requested, limit } => write!(
                f,
                "BuildSettings scene path bytes {requested} exceed configured limit {limit}"
            ),
            Self::CountOverflow { resource } => {
                write!(f, "count overflows while reading {resource}")
            }
            Self::AllocationFailed { resource, requested } => write!(
                f,
                "could not reserve {requested} entries for {resource}"
            ),
        }
    }
}

impl Error for BuildSettingsSceneError {}

#[cfg(test)]
mod tests {
    use super::{
        extract_build_settings_scene_paths, scene_path_from_build_settings_value,
        BuildSettingsSceneError, BuildSettingsSceneLimits,
    };
    use crate::processing::scene_naming::SceneUnityVersion;
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn scene_path(path: &[u8]) -> SerializedValue {
        SerializedValue::Struct(vec![
            field("enabled", SerializedValue::Bool(true)),
            field("path", SerializedValue::StringBytes(path.to_vec())),
        ])
    }

    #[test]
    fn extracts_current_and_legacy_scene_array_shapes_in_order() {
        let current = SerializedValue::Struct(vec![field(
            "m_Scenes",
            SerializedValue::Array(vec![
                scene_path(b"Assets/Scenes/First.unity"),
                scene_path(b"Assets/Scenes/Second.unity"),
            ]),
        )]);
        assert_eq!(
            extract_build_settings_scene_paths(&current, BuildSettingsSceneLimits::default())
                .unwrap(),
            ["Assets/Scenes/First.unity", "Assets/Scenes/Second.unity"]
        );

        let legacy = SerializedValue::Struct(vec![field(
            "Base",
            SerializedValue::Struct(vec![field(
                "m_Levels",
                SerializedValue::Array(vec![
                    SerializedValue::StringBytes(b"Assets/Scenes/Old.unity".to_vec()),
                    SerializedValue::StringBytes(b"Library/PackageCache/pkg/Old2.unity".to_vec()),
                ]),
            )]),
        )]);
        assert_eq!(
            extract_build_settings_scene_paths(&legacy, BuildSettingsSceneLimits::default())
                .unwrap(),
            [
                "Assets/Scenes/Old.unity",
                "Library/PackageCache/pkg/Old2.unity"
            ]
        );
    }

    #[test]
    fn resolves_collection_path_from_build_settings_values() {
        let root = SerializedValue::Struct(vec![field(
            "m_Scenes",
            SerializedValue::Array(vec![
                scene_path(b"Assets/Scenes/Main.unity"),
                scene_path(b"Assets/Scenes/Next.unity"),
            ]),
        )]);
        let old_version = SceneUnityVersion::parse("5.2.4f1").unwrap();
        assert_eq!(
            scene_path_from_build_settings_value(
                "level0",
                old_version,
                &root,
                BuildSettingsSceneLimits::default()
            )
            .unwrap()
            .as_deref(),
            Some("Assets/Scenes/Next")
        );
    }

    #[test]
    fn rejects_ambiguous_or_malformed_scene_metadata_explicitly() {
        let both = SerializedValue::Struct(vec![
            field("m_Scenes", SerializedValue::Array(Vec::new())),
            field("m_Levels", SerializedValue::Array(Vec::new())),
        ]);
        assert_eq!(
            extract_build_settings_scene_paths(&both, BuildSettingsSceneLimits::default()),
            Err(BuildSettingsSceneError::BothSceneArrayFieldsPresent)
        );

        let wrong_array =
            SerializedValue::Struct(vec![field("m_Scenes", SerializedValue::Bool(true))]);
        assert!(matches!(
            extract_build_settings_scene_paths(&wrong_array, BuildSettingsSceneLimits::default()),
            Err(BuildSettingsSceneError::SceneArrayHasWrongType { actual: "bool" })
        ));

        let missing_path = SerializedValue::Struct(vec![field(
            "m_Scenes",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                "enabled",
                SerializedValue::Bool(true),
            )])]),
        )]);
        assert!(matches!(
            extract_build_settings_scene_paths(&missing_path, BuildSettingsSceneLimits::default()),
            Err(BuildSettingsSceneError::MissingScenePath { scene_index: 0 })
        ));

        let invalid_utf8 = SerializedValue::Struct(vec![field(
            "m_Scenes",
            SerializedValue::Array(vec![scene_path(&[0xff])]),
        )]);
        assert!(matches!(
            extract_build_settings_scene_paths(&invalid_utf8, BuildSettingsSceneLimits::default()),
            Err(BuildSettingsSceneError::InvalidScenePathUtf8 {
                scene_index: 0,
                valid_up_to: 0,
            })
        ));
    }

    #[test]
    fn enforces_scene_count_and_total_path_byte_limits() {
        let root = SerializedValue::Struct(vec![field(
            "m_Scenes",
            SerializedValue::Array(vec![scene_path(b"a"), scene_path(b"b")]),
        )]);
        assert!(matches!(
            extract_build_settings_scene_paths(
                &root,
                BuildSettingsSceneLimits {
                    max_scenes: 1,
                    max_total_path_bytes: 10,
                }
            ),
            Err(BuildSettingsSceneError::SceneCountLimitExceeded { count: 2, limit: 1 })
        ));
        assert!(matches!(
            extract_build_settings_scene_paths(
                &root,
                BuildSettingsSceneLimits {
                    max_scenes: 2,
                    max_total_path_bytes: 1,
                }
            ),
            Err(BuildSettingsSceneError::PathBytesLimitExceeded {
                requested: 2,
                limit: 1,
            })
        ));
    }
}
