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

/// Resource bounds for mapping streamed-scene names to serialized bundle collections.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneBundleMappingLimits {
    pub max_entries: usize,
    pub max_total_name_and_path_bytes: usize,
}

impl Default for SceneBundleMappingLimits {
    fn default() -> Self {
        Self {
            max_entries: 65_536,
            max_total_name_and_path_bytes: 16 * 1024 * 1024,
        }
    }
}

/// A source-order scene path mapped to a collection index in one serialized bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneBundlePathMapping {
    pub collection_index: usize,
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneBundleMappingError {
    EntryCountLimitExceeded {
        resource: &'static str,
        count: usize,
        limit: usize,
    },
    CountOverflow,
    ByteLimitExceeded {
        requested: usize,
        limit: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    SceneHashCollectionNotFound {
        entry_index: usize,
        collection_name: String,
    },
    ContainerEntryHasNoSceneCollection {
        entry_index: usize,
        starting_collection_index: usize,
    },
}

impl Display for SceneBundleMappingError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntryCountLimitExceeded {
                resource,
                count,
                limit,
            } => write!(
                f,
                "streamed-scene {resource} count {count} exceeds configured limit {limit}"
            ),
            Self::CountOverflow => f.write_str("streamed-scene mapping count overflows"),
            Self::ByteLimitExceeded { requested, limit } => write!(
                f,
                "streamed-scene mapping names and paths use {requested} bytes, exceeding limit {limit}"
            ),
            Self::AllocationFailed { resource, requested } => write!(
                f,
                "could not reserve {requested} streamed-scene mapping entries for {resource}"
            ),
            Self::SceneHashCollectionNotFound {
                entry_index,
                collection_name,
            } => write!(
                f,
                "SceneHashes entry {entry_index} names missing bundle collection {collection_name:?}"
            ),
            Self::ContainerEntryHasNoSceneCollection {
                entry_index,
                starting_collection_index,
            } => write!(
                f,
                "streamed-scene container entry {entry_index} has no scene collection at or after bundle collection index {starting_collection_index}"
            ),
        }
    }
}

impl Error for SceneBundleMappingError {}

/// Map source SceneHashes `(scene path, collection name)` entries to the first matching
/// collection, preserving source order and duplicate-name behavior.
pub fn map_scene_hashes_to_collections(
    scene_hashes: &[(String, String)],
    bundle_collection_names: &[String],
    limits: SceneBundleMappingLimits,
) -> Result<Vec<SceneBundlePathMapping>, SceneBundleMappingError> {
    if scene_hashes.len() > limits.max_entries {
        return Err(SceneBundleMappingError::EntryCountLimitExceeded {
            resource: "SceneHashes",
            count: scene_hashes.len(),
            limit: limits.max_entries,
        });
    }
    if bundle_collection_names.len() > limits.max_entries {
        return Err(SceneBundleMappingError::EntryCountLimitExceeded {
            resource: "bundle collection",
            count: bundle_collection_names.len(),
            limit: limits.max_entries,
        });
    }

    let mut total_bytes = 0usize;
    let mut normalized_names = Vec::new();
    normalized_names
        .try_reserve_exact(bundle_collection_names.len())
        .map_err(|_| SceneBundleMappingError::AllocationFailed {
            resource: "bundle collection names",
            requested: bundle_collection_names.len(),
        })?;
    for (collection_index, name) in bundle_collection_names.iter().enumerate() {
        total_bytes = charge_scene_bundle_bytes(total_bytes, name.len(), limits)?;
        normalized_names.push((fix_file_identifier(name), collection_index));
    }
    normalized_names.sort_unstable();

    let mut mappings = Vec::new();
    mappings
        .try_reserve_exact(scene_hashes.len())
        .map_err(|_| SceneBundleMappingError::AllocationFailed {
            resource: "SceneHashes result",
            requested: scene_hashes.len(),
        })?;
    for (entry_index, (scene_path, collection_name)) in scene_hashes.iter().enumerate() {
        let normalized_collection_name = fix_file_identifier(collection_name);
        total_bytes =
            charge_scene_bundle_bytes(total_bytes, normalized_collection_name.len(), limits)?;
        let first = normalized_names
            .partition_point(|(name, _)| name.as_str() < normalized_collection_name.as_str());
        let Some((_, collection_index)) = normalized_names
            .get(first)
            .filter(|(name, _)| name == &normalized_collection_name)
        else {
            return Err(SceneBundleMappingError::SceneHashCollectionNotFound {
                entry_index,
                collection_name: normalized_collection_name,
            });
        };
        let path = streamed_scene_project_path(scene_path);
        total_bytes = charge_scene_bundle_bytes(total_bytes, path.len(), limits)?;
        mappings.push(SceneBundlePathMapping {
            collection_index: *collection_index,
            path,
        });
    }
    Ok(mappings)
}

/// Map serialized AssetBundle Container scene paths to scene collections in bundle order.
///
/// The fallback starts at collection zero, picks the next collection already known to be a scene,
/// and advances past it for the next Container entry, matching SceneDefinitionProcessor.IndexOf.
pub fn map_container_paths_to_scene_collections(
    container_scene_paths: &[String],
    bundle_collection_is_scene: &[bool],
    limits: SceneBundleMappingLimits,
) -> Result<Vec<SceneBundlePathMapping>, SceneBundleMappingError> {
    if container_scene_paths.len() > limits.max_entries {
        return Err(SceneBundleMappingError::EntryCountLimitExceeded {
            resource: "Container scene path",
            count: container_scene_paths.len(),
            limit: limits.max_entries,
        });
    }
    if bundle_collection_is_scene.len() > limits.max_entries {
        return Err(SceneBundleMappingError::EntryCountLimitExceeded {
            resource: "bundle collection",
            count: bundle_collection_is_scene.len(),
            limit: limits.max_entries,
        });
    }
    let mut mappings = Vec::new();
    mappings
        .try_reserve_exact(container_scene_paths.len())
        .map_err(|_| SceneBundleMappingError::AllocationFailed {
            resource: "Container result",
            requested: container_scene_paths.len(),
        })?;
    let mut total_bytes = 0usize;
    let mut starting_collection_index = 0usize;
    for (entry_index, scene_path) in container_scene_paths.iter().enumerate() {
        let Some(relative_index) = bundle_collection_is_scene[starting_collection_index..]
            .iter()
            .position(|is_scene| *is_scene)
        else {
            return Err(
                SceneBundleMappingError::ContainerEntryHasNoSceneCollection {
                    entry_index,
                    starting_collection_index,
                },
            );
        };
        let collection_index = starting_collection_index + relative_index;
        let path = streamed_scene_project_path(scene_path);
        total_bytes = charge_scene_bundle_bytes(total_bytes, path.len(), limits)?;
        mappings.push(SceneBundlePathMapping {
            collection_index,
            path,
        });
        starting_collection_index = collection_index
            .checked_add(1)
            .ok_or(SceneBundleMappingError::CountOverflow)?;
    }
    Ok(mappings)
}

/// Apply the baseline streamed-scene path conversion: remove the extension, make rooted paths
/// relative by retaining their Assets segment when present, then ensure the Assets/ prefix.
pub fn streamed_scene_project_path(scene_path: &str) -> String {
    let path_without_extension = remove_scene_extension(scene_path);
    let path = if is_rooted_scene_path(path_without_extension) {
        let components = path_without_extension.split('/');
        let asset_index = components
            .clone()
            .position(|component| component.eq_ignore_ascii_case("Assets"));
        if let Some(asset_index) = asset_index {
            path_without_extension
                .split('/')
                .skip(asset_index)
                .collect::<Vec<_>>()
                .join("/")
        } else {
            String::new()
        }
    } else {
        path_without_extension.to_owned()
    };
    if path.starts_with("Assets/") {
        path
    } else if path
        .get(.."Assets/".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Assets/"))
    {
        format!("Assets/{}", &path["Assets/".len()..])
    } else {
        format!("Assets/{path}")
    }
}

fn charge_scene_bundle_bytes(
    current: usize,
    amount: usize,
    limits: SceneBundleMappingLimits,
) -> Result<usize, SceneBundleMappingError> {
    let requested = current
        .checked_add(amount)
        .ok_or(SceneBundleMappingError::CountOverflow)?;
    if requested > limits.max_total_name_and_path_bytes {
        return Err(SceneBundleMappingError::ByteLimitExceeded {
            requested,
            limit: limits.max_total_name_and_path_bytes,
        });
    }
    Ok(requested)
}

fn remove_scene_extension(path: &str) -> &str {
    let filename_start = path.rfind(['/', '\\']).map_or(0, |index| index + 1);
    let filename = &path[filename_start..];
    let Some(extension_start) = filename.rfind('.') else {
        return path;
    };
    if matches!(filename, "." | "..") || extension_start == 0 {
        return path;
    }
    &path[..filename_start + extension_start]
}

#[cfg(windows)]
fn is_rooted_scene_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with('\\')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

#[cfg(not(windows))]
fn is_rooted_scene_path(path: &str) -> bool {
    path.starts_with('/')
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
        extract_occlusion_scene_guid, map_container_paths_to_scene_collections,
        map_scene_hashes_to_collections, resolve_scene_collection_path, scene_manager_kind,
        streamed_scene_project_path, BuildSettingsScenePathSet, SceneBundleMappingError,
        SceneBundleMappingLimits, SceneBundlePathMapping, SceneCollectionPathStatus,
        SceneGuidError, SceneGuidWords, SceneManagerKind,
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

    #[test]
    fn maps_scene_hashes_to_first_normalized_bundle_collection() {
        let collection_names = ["other".to_owned(), "Level0".to_owned(), "LEVEL0".to_owned()];
        let scene_hashes = [
            (
                "/build/project/Assets/Scenes/Main.unity".to_owned(),
                "archive:/bundle/LEVEL0".to_owned(),
            ),
            ("assets/Scenes/Next.unity".to_owned(), "level0".to_owned()),
        ];
        assert_eq!(
            map_scene_hashes_to_collections(
                &scene_hashes,
                &collection_names,
                SceneBundleMappingLimits::default()
            )
            .unwrap(),
            [
                SceneBundlePathMapping {
                    collection_index: 1,
                    path: "Assets/Scenes/Main".to_owned(),
                },
                SceneBundlePathMapping {
                    collection_index: 1,
                    path: "Assets/Scenes/Next".to_owned(),
                }
            ]
        );
        assert_eq!(
            streamed_scene_project_path("/project/outside/Scene.unity"),
            "Assets/"
        );
    }

    #[test]
    fn maps_container_paths_in_order_to_the_next_known_scene_collection() {
        let container_paths = [
            "Assets/Scenes/First.unity".to_owned(),
            "Second.unity".to_owned(),
        ];
        let collection_is_scene = [false, true, false, true];
        assert_eq!(
            map_container_paths_to_scene_collections(
                &container_paths,
                &collection_is_scene,
                SceneBundleMappingLimits::default()
            )
            .unwrap(),
            [
                SceneBundlePathMapping {
                    collection_index: 1,
                    path: "Assets/Scenes/First".to_owned(),
                },
                SceneBundlePathMapping {
                    collection_index: 3,
                    path: "Assets/Second".to_owned(),
                }
            ]
        );
        assert_eq!(
            map_container_paths_to_scene_collections(
                &["Scene.unity".to_owned()],
                &[false],
                SceneBundleMappingLimits::default()
            ),
            Err(
                SceneBundleMappingError::ContainerEntryHasNoSceneCollection {
                    entry_index: 0,
                    starting_collection_index: 0,
                }
            )
        );
    }

    #[test]
    fn streamed_scene_mapping_reports_missing_collections_and_limits() {
        let scene_hashes = [("Scene.unity".to_owned(), "missing".to_owned())];
        assert_eq!(
            map_scene_hashes_to_collections(
                &scene_hashes,
                &["present".to_owned()],
                SceneBundleMappingLimits::default()
            ),
            Err(SceneBundleMappingError::SceneHashCollectionNotFound {
                entry_index: 0,
                collection_name: "missing".to_owned(),
            })
        );
        assert_eq!(
            map_scene_hashes_to_collections(
                &scene_hashes,
                &["present".to_owned()],
                SceneBundleMappingLimits {
                    max_entries: 0,
                    max_total_name_and_path_bytes: usize::MAX,
                }
            ),
            Err(SceneBundleMappingError::EntryCountLimitExceeded {
                resource: "SceneHashes",
                count: 1,
                limit: 0,
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
