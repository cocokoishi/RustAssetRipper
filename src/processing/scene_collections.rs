//! Classification of serialized scene-collection manager objects.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::special_names::fix_file_identifier;
use crate::serialized_file::object::{
    SerializedFieldValue, SerializedValue, SerializedValueLookupError,
};

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

/// Scene-name metadata decoded from one class ID 142 AssetBundle object.
///
/// Optional fields remain optional because older or stripped TypeTrees can omit them. Callers
/// should only apply scene mappings when `is_streamed_scene_asset_bundle == Some(true)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamedSceneBundleMetadata {
    pub is_streamed_scene_asset_bundle: Option<bool>,
    /// Ordered `(scene path, serialized collection name)` entries from `SceneHashes`.
    pub scene_hashes: Option<Vec<(String, String)>>,
    /// Ordered scene path keys from the `Container` map.
    pub container_paths: Option<Vec<String>>,
}

/// One ordered AssetBundle Container key and its serialized `AssetInfo.Asset` pointer.
///
/// `file_id == 0` identifies a local asset in the AssetBundle's collection. Nonzero file IDs
/// identify shared-file references and must not be treated as local container assets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetBundleContainerReference {
    pub path: String,
    pub file_id: i32,
    pub path_id: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetBundleContainerDecodeError {
    ContainerMetadata(StreamedSceneBundleDecodeError),
    MissingContainerField,
    MissingAssetPointer { entry_index: usize },
}

impl Display for AssetBundleContainerDecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContainerMetadata(source) => Display::fmt(source, f),
            Self::MissingContainerField => {
                f.write_str("AssetBundle has neither m_Container nor Container")
            }
            Self::MissingAssetPointer { entry_index } => write!(
                f,
                "AssetBundle Container entry {entry_index} has no AssetInfo asset pointer"
            ),
        }
    }
}

impl Error for AssetBundleContainerDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ContainerMetadata(source) => Some(source),
            Self::MissingContainerField | Self::MissingAssetPointer { .. } => None,
        }
    }
}

/// Decode AssetBundle Container entries without discarding their asset identity pointers.
///
/// Scene processing only needs the ordered path keys, while `OriginalPathProcessor` also uses
/// `AssetInfo.Asset.FileID` and `PathID` to assign bundle metadata to local assets. This decoder
/// keeps those distinctions available to later processing stages.
pub fn extract_asset_bundle_container_references(
    root: &SerializedValue,
    limits: SceneBundleMappingLimits,
) -> Result<Vec<AssetBundleContainerReference>, AssetBundleContainerDecodeError> {
    let container = unique_bundle_alias(root, "m_Container", "Container")
        .map_err(AssetBundleContainerDecodeError::ContainerMetadata)?
        .ok_or(AssetBundleContainerDecodeError::MissingContainerField)?;
    let mut total_entries = 0usize;
    let mut total_bytes = 0usize;
    let entries = bundle_array(container, "m_Container", limits, &mut total_entries)
        .map_err(AssetBundleContainerDecodeError::ContainerMetadata)?;
    let mut decoded = reserve_bundle_entries(entries.len(), "m_Container")
        .map_err(AssetBundleContainerDecodeError::ContainerMetadata)?;
    for (entry_index, entry) in entries.iter().enumerate() {
        let entry_path = format!("m_Container[{entry_index}]");
        let pair = bundle_struct(entry, &entry_path)
            .map_err(AssetBundleContainerDecodeError::ContainerMetadata)?;
        let path = required_pair_field(pair, &entry_path, "first")
            .map_err(AssetBundleContainerDecodeError::ContainerMetadata)?;
        let asset_info = required_pair_field(pair, &entry_path, "second")
            .map_err(AssetBundleContainerDecodeError::ContainerMetadata)?;
        bundle_struct(asset_info, &format!("{entry_path}.second"))
            .map_err(AssetBundleContainerDecodeError::ContainerMetadata)?;
        let path = decode_bundle_string(
            path,
            &format!("{entry_path}.first"),
            &mut total_bytes,
            limits,
        )
        .map_err(AssetBundleContainerDecodeError::ContainerMetadata)?;
        let asset_pointer = asset_info
            .unique_field_in_base_chain("asset")
            .map_err(|source| {
                AssetBundleContainerDecodeError::ContainerMetadata(
                    StreamedSceneBundleDecodeError::FieldLookup {
                        field_path: format!("{entry_path}.second.asset"),
                        source,
                    },
                )
            })?
            .ok_or(AssetBundleContainerDecodeError::MissingAssetPointer { entry_index })?;
        let SerializedValue::PPtr { file_id, path_id } = asset_pointer else {
            return Err(AssetBundleContainerDecodeError::ContainerMetadata(
                StreamedSceneBundleDecodeError::WrongType {
                    field_path: format!("{entry_path}.second.asset"),
                    expected: "PPtr",
                    actual: value_type_name(asset_pointer),
                },
            ));
        };
        decoded.push(AssetBundleContainerReference {
            path,
            file_id: *file_id,
            path_id: *path_id,
        });
    }
    Ok(decoded)
}

/// One ordered ResourceManager Container key and its serialized object pointer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceManagerContainerReference {
    pub path: String,
    pub file_id: i32,
    pub path_id: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceManagerContainerDecodeError {
    Metadata(StreamedSceneBundleDecodeError),
    MissingContainerField,
    MissingObjectPointer { entry_index: usize },
}

impl Display for ResourceManagerContainerDecodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(source) => {
                write!(
                    formatter,
                    "ResourceManager Container metadata is malformed: {source}"
                )
            }
            Self::MissingContainerField => {
                formatter.write_str("ResourceManager has neither m_Container nor Container")
            }
            Self::MissingObjectPointer { entry_index } => write!(
                formatter,
                "ResourceManager Container entry {entry_index} has no object pointer"
            ),
        }
    }
}

impl Error for ResourceManagerContainerDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Metadata(source) => Some(source),
            Self::MissingContainerField | Self::MissingObjectPointer { .. } => None,
        }
    }
}

/// Decode the ordered `map<string, PPtr<Object>>` from a class-ID-147 ResourceManager.
pub fn extract_resource_manager_container_references(
    root: &SerializedValue,
    limits: SceneBundleMappingLimits,
) -> Result<Vec<ResourceManagerContainerReference>, ResourceManagerContainerDecodeError> {
    let container = unique_bundle_alias(root, "m_Container", "Container")
        .map_err(ResourceManagerContainerDecodeError::Metadata)?
        .ok_or(ResourceManagerContainerDecodeError::MissingContainerField)?;
    let mut total_entries = 0usize;
    let mut total_bytes = 0usize;
    let entries = bundle_array(container, "m_Container", limits, &mut total_entries)
        .map_err(ResourceManagerContainerDecodeError::Metadata)?;
    let mut decoded = reserve_bundle_entries(entries.len(), "m_Container")
        .map_err(ResourceManagerContainerDecodeError::Metadata)?;
    for (entry_index, entry) in entries.iter().enumerate() {
        let entry_path = format!("m_Container[{entry_index}]");
        let pair = bundle_struct(entry, &entry_path)
            .map_err(ResourceManagerContainerDecodeError::Metadata)?;
        let path = required_pair_field(pair, &entry_path, "first")
            .map_err(ResourceManagerContainerDecodeError::Metadata)?;
        let pointer = required_pair_field(pair, &entry_path, "second")
            .map_err(ResourceManagerContainerDecodeError::Metadata)?;
        let path = decode_bundle_string(
            path,
            &format!("{entry_path}.first"),
            &mut total_bytes,
            limits,
        )
        .map_err(ResourceManagerContainerDecodeError::Metadata)?;
        let SerializedValue::PPtr { file_id, path_id } = pointer else {
            return Err(ResourceManagerContainerDecodeError::Metadata(
                StreamedSceneBundleDecodeError::WrongType {
                    field_path: format!("{entry_path}.second"),
                    expected: "PPtr",
                    actual: value_type_name(pointer),
                },
            ));
        };
        decoded.push(ResourceManagerContainerReference {
            path,
            file_id: *file_id,
            path_id: *path_id,
        });
    }
    Ok(decoded)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamedSceneBundleDecodeError {
    FieldLookup {
        field_path: String,
        source: SerializedValueLookupError,
    },
    DuplicateAliases {
        primary: &'static str,
        alternate: &'static str,
    },
    WrongType {
        field_path: String,
        expected: &'static str,
        actual: &'static str,
    },
    MissingPairField {
        field_path: String,
        member: &'static str,
    },
    EntryCountLimitExceeded {
        field_path: String,
        count: usize,
        limit: usize,
    },
    ByteLimitExceeded {
        requested: usize,
        limit: usize,
    },
    CountOverflow,
    InvalidUtf8 {
        field_path: String,
    },
    AllocationFailed {
        field_path: String,
        requested: usize,
    },
}

impl Display for StreamedSceneBundleDecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field_path, source } => {
                write!(
                    f,
                    "AssetBundle field {field_path} lookup failed: {source:?}"
                )
            }
            Self::DuplicateAliases { primary, alternate } => write!(
                f,
                "AssetBundle contains both {primary} and {alternate} fields"
            ),
            Self::WrongType {
                field_path,
                expected,
                actual,
            } => write!(
                f,
                "AssetBundle field {field_path} has type {actual}; expected {expected}"
            ),
            Self::MissingPairField { field_path, member } => {
                write!(
                    f,
                    "AssetBundle field {field_path} is missing pair member {member}"
                )
            }
            Self::EntryCountLimitExceeded {
                field_path,
                count,
                limit,
            } => write!(
                f,
                "AssetBundle field {field_path} has {count} entries, exceeding limit {limit}"
            ),
            Self::ByteLimitExceeded { requested, limit } => write!(
                f,
                "AssetBundle scene metadata uses {requested} bytes, exceeding limit {limit}"
            ),
            Self::CountOverflow => f.write_str("AssetBundle scene metadata size overflows"),
            Self::InvalidUtf8 { field_path } => {
                write!(f, "AssetBundle field {field_path} is not valid UTF-8")
            }
            Self::AllocationFailed {
                field_path,
                requested,
            } => write!(
                f,
                "could not reserve size {requested} for AssetBundle field {field_path}"
            ),
        }
    }
}

impl Error for StreamedSceneBundleDecodeError {}

/// Decode the scene-related fields from a TypeTree-decoded AssetBundle object.
///
/// Unity's serialized pair records use `first`/`second`; AssetBundle fields may be represented
/// either by their serialized `m_` name or the generated property name. All reads are unique and
/// inherited TypeTree `Base` fields are followed explicitly. The caller retains the original
/// decoded object, so fields outside this scene processor remain available to later stages.
pub fn extract_streamed_scene_bundle_metadata(
    root: &SerializedValue,
    limits: SceneBundleMappingLimits,
) -> Result<StreamedSceneBundleMetadata, StreamedSceneBundleDecodeError> {
    let streamed_field = unique_bundle_alias(
        root,
        "m_IsStreamedSceneAssetBundle",
        "IsStreamedSceneAssetBundle",
    )?;
    let is_streamed_scene_asset_bundle = match streamed_field {
        Some(SerializedValue::Bool(value)) => Some(*value),
        Some(value) => {
            return Err(StreamedSceneBundleDecodeError::WrongType {
                field_path: "m_IsStreamedSceneAssetBundle".to_owned(),
                expected: "bool",
                actual: value_type_name(value),
            });
        }
        None => None,
    };

    let scene_hashes_field = unique_bundle_alias(root, "m_SceneHashes", "SceneHashes")?;
    let mut total_entries = 0usize;
    let mut total_bytes = 0usize;
    let scene_hashes = match scene_hashes_field {
        Some(value) => Some(decode_scene_hash_pairs(
            value,
            limits,
            &mut total_entries,
            &mut total_bytes,
        )?),
        None => None,
    };

    let container_field = unique_bundle_alias(root, "m_Container", "Container")?;
    let container_paths = match container_field {
        Some(value) => Some(decode_container_paths(
            value,
            limits,
            &mut total_entries,
            &mut total_bytes,
        )?),
        None => None,
    };

    Ok(StreamedSceneBundleMetadata {
        is_streamed_scene_asset_bundle,
        scene_hashes,
        container_paths,
    })
}

fn unique_bundle_alias<'a>(
    root: &'a SerializedValue,
    primary: &'static str,
    alternate: &'static str,
) -> Result<Option<&'a SerializedValue>, StreamedSceneBundleDecodeError> {
    let primary_value = root.unique_field_in_base_chain(primary).map_err(|source| {
        StreamedSceneBundleDecodeError::FieldLookup {
            field_path: primary.to_owned(),
            source,
        }
    })?;
    let alternate_value = root
        .unique_field_in_base_chain(alternate)
        .map_err(|source| StreamedSceneBundleDecodeError::FieldLookup {
            field_path: alternate.to_owned(),
            source,
        })?;
    match (primary_value, alternate_value) {
        (Some(_), Some(_)) => {
            Err(StreamedSceneBundleDecodeError::DuplicateAliases { primary, alternate })
        }
        (Some(value), None) | (None, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    }
}

fn decode_scene_hash_pairs(
    value: &SerializedValue,
    limits: SceneBundleMappingLimits,
    total_entries: &mut usize,
    total_bytes: &mut usize,
) -> Result<Vec<(String, String)>, StreamedSceneBundleDecodeError> {
    let field_path = "m_SceneHashes";
    let entries = bundle_array(value, field_path, limits, total_entries)?;
    let mut decoded = reserve_bundle_entries(entries.len(), field_path)?;
    for (index, entry) in entries.iter().enumerate() {
        let entry_path = format!("{field_path}[{index}]");
        let pair = bundle_struct(entry, &entry_path)?;
        let scene_path = required_pair_field(pair, &entry_path, "first")?;
        let collection_name = required_pair_field(pair, &entry_path, "second")?;
        let scene_path = decode_bundle_string(
            scene_path,
            &format!("{entry_path}.first"),
            total_bytes,
            limits,
        )?;
        let collection_name = decode_bundle_string(
            collection_name,
            &format!("{entry_path}.second"),
            total_bytes,
            limits,
        )?;
        decoded.push((scene_path, collection_name));
    }
    Ok(decoded)
}

fn decode_container_paths(
    value: &SerializedValue,
    limits: SceneBundleMappingLimits,
    total_entries: &mut usize,
    total_bytes: &mut usize,
) -> Result<Vec<String>, StreamedSceneBundleDecodeError> {
    let field_path = "m_Container";
    let entries = bundle_array(value, field_path, limits, total_entries)?;
    let mut decoded = reserve_bundle_entries(entries.len(), field_path)?;
    for (index, entry) in entries.iter().enumerate() {
        let entry_path = format!("{field_path}[{index}]");
        let pair = bundle_struct(entry, &entry_path)?;
        let path = required_pair_field(pair, &entry_path, "first")?;
        let asset_info = required_pair_field(pair, &entry_path, "second")?;
        bundle_struct(asset_info, &format!("{entry_path}.second"))?;
        decoded.push(decode_bundle_string(
            path,
            &format!("{entry_path}.first"),
            total_bytes,
            limits,
        )?);
    }
    Ok(decoded)
}

fn bundle_array<'a>(
    value: &'a SerializedValue,
    field_path: &str,
    limits: SceneBundleMappingLimits,
    total_entries: &mut usize,
) -> Result<&'a [SerializedValue], StreamedSceneBundleDecodeError> {
    let SerializedValue::Array(entries) = value else {
        return Err(StreamedSceneBundleDecodeError::WrongType {
            field_path: field_path.to_owned(),
            expected: "array",
            actual: value_type_name(value),
        });
    };
    let requested = total_entries
        .checked_add(entries.len())
        .ok_or(StreamedSceneBundleDecodeError::CountOverflow)?;
    if requested > limits.max_entries {
        return Err(StreamedSceneBundleDecodeError::EntryCountLimitExceeded {
            field_path: field_path.to_owned(),
            count: requested,
            limit: limits.max_entries,
        });
    }
    *total_entries = requested;
    Ok(entries)
}

fn bundle_struct<'a>(
    value: &'a SerializedValue,
    field_path: &str,
) -> Result<&'a [SerializedFieldValue], StreamedSceneBundleDecodeError> {
    let SerializedValue::Struct(fields) = value else {
        return Err(StreamedSceneBundleDecodeError::WrongType {
            field_path: field_path.to_owned(),
            expected: "struct",
            actual: value_type_name(value),
        });
    };
    Ok(fields)
}

fn required_pair_field<'a>(
    fields: &'a [SerializedFieldValue],
    entry_path: &str,
    member: &'static str,
) -> Result<&'a SerializedValue, StreamedSceneBundleDecodeError> {
    unique_struct_field(fields, member)
        .map_err(|source| StreamedSceneBundleDecodeError::FieldLookup {
            field_path: format!("{entry_path}.{member}"),
            source,
        })?
        .ok_or_else(|| StreamedSceneBundleDecodeError::MissingPairField {
            field_path: entry_path.to_owned(),
            member,
        })
}

fn unique_struct_field<'a>(
    fields: &'a [SerializedFieldValue],
    name: &str,
) -> Result<Option<&'a SerializedValue>, SerializedValueLookupError> {
    let mut matching = fields.iter().filter(|field| field.name == name);
    let Some(first) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        let match_count = fields.iter().filter(|field| field.name == name).count();
        return Err(SerializedValueLookupError::DuplicateField {
            name: name.to_owned(),
            match_count,
        });
    }
    Ok(Some(&first.value))
}

fn decode_bundle_string(
    value: &SerializedValue,
    field_path: &str,
    total_bytes: &mut usize,
    limits: SceneBundleMappingLimits,
) -> Result<String, StreamedSceneBundleDecodeError> {
    let SerializedValue::StringBytes(bytes) = value else {
        return Err(StreamedSceneBundleDecodeError::WrongType {
            field_path: field_path.to_owned(),
            expected: "string",
            actual: value_type_name(value),
        });
    };
    let requested = total_bytes
        .checked_add(bytes.len())
        .ok_or(StreamedSceneBundleDecodeError::CountOverflow)?;
    if requested > limits.max_total_name_and_path_bytes {
        return Err(StreamedSceneBundleDecodeError::ByteLimitExceeded {
            requested,
            limit: limits.max_total_name_and_path_bytes,
        });
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| StreamedSceneBundleDecodeError::InvalidUtf8 {
            field_path: field_path.to_owned(),
        })?;
    let mut decoded = String::new();
    decoded.try_reserve_exact(text.len()).map_err(|_| {
        StreamedSceneBundleDecodeError::AllocationFailed {
            field_path: field_path.to_owned(),
            requested: text.len(),
        }
    })?;
    decoded.push_str(text);
    *total_bytes = requested;
    Ok(decoded)
}

fn reserve_bundle_entries<T>(
    count: usize,
    field_path: &str,
) -> Result<Vec<T>, StreamedSceneBundleDecodeError> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| {
        StreamedSceneBundleDecodeError::AllocationFailed {
            field_path: field_path.to_owned(),
            requested: count,
        }
    })?;
    Ok(values)
}

/// A source-order scene path mapped to a collection index in one serialized bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneBundlePathMapping {
    pub collection_index: usize,
    pub path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneBundlePathMappingSource {
    SceneHashes,
    Container,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamedSceneBundlePathMappings {
    pub source: SceneBundlePathMappingSource,
    pub mappings: Vec<SceneBundlePathMapping>,
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
    MissingContainerScenePaths,
    CollectionMetadataCountMismatch {
        collection_name_count: usize,
        scene_flag_count: usize,
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
            Self::MissingContainerScenePaths => f.write_str(
                "streamed-scene bundle has no nonempty SceneHashes and no decoded Container paths",
            ),
            Self::CollectionMetadataCountMismatch {
                collection_name_count,
                scene_flag_count,
            } => write!(
                f,
                "bundle collection names ({collection_name_count}) and scene flags ({scene_flag_count}) have different counts"
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

/// Select the baseline mapping path for one decoded streamed-scene AssetBundle object.
///
/// A nonempty SceneHashes map takes precedence. Otherwise the ordered Container keys are assigned
/// to the next scene collections. Missing streamed-scene flags leave the object unselected, while
/// a selected streamed bundle with no usable path source is reported explicitly.
pub fn map_streamed_scene_bundle_paths(
    metadata: &StreamedSceneBundleMetadata,
    bundle_collection_names: &[String],
    bundle_collection_is_scene: &[bool],
    limits: SceneBundleMappingLimits,
) -> Result<Option<StreamedSceneBundlePathMappings>, SceneBundleMappingError> {
    if metadata.is_streamed_scene_asset_bundle != Some(true) {
        return Ok(None);
    }
    if bundle_collection_names.len() != bundle_collection_is_scene.len() {
        return Err(SceneBundleMappingError::CollectionMetadataCountMismatch {
            collection_name_count: bundle_collection_names.len(),
            scene_flag_count: bundle_collection_is_scene.len(),
        });
    }
    if let Some(scene_hashes) = metadata
        .scene_hashes
        .as_ref()
        .filter(|scene_hashes| !scene_hashes.is_empty())
    {
        return Ok(Some(StreamedSceneBundlePathMappings {
            source: SceneBundlePathMappingSource::SceneHashes,
            mappings: map_scene_hashes_to_collections(
                scene_hashes,
                bundle_collection_names,
                limits,
            )?,
        }));
    }
    let Some(container_paths) = metadata.container_paths.as_deref() else {
        return Err(SceneBundleMappingError::MissingContainerScenePaths);
    };
    Ok(Some(StreamedSceneBundlePathMappings {
        source: SceneBundlePathMappingSource::Container,
        mappings: map_container_paths_to_scene_collections(
            container_paths,
            bundle_collection_is_scene,
            limits,
        )?,
    }))
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

impl SceneGuidWords {
    pub const fn is_zero(self) -> bool {
        self.data[0] == 0 && self.data[1] == 0 && self.data[2] == 0 && self.data[3] == 0
    }
}

/// Source path choice used when planning one baseline SceneDefinition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneDefinitionPathSource {
    StreamedSceneBundle,
    BuildSettings { scene_index: usize },
    CollectionNameFallback,
}

impl Display for SceneDefinitionPathSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::StreamedSceneBundle => f.write_str("streamed-scene AssetBundle"),
            Self::BuildSettings { scene_index } => {
                write!(f, "BuildSettings scene index {scene_index}")
            }
            Self::CollectionNameFallback => f.write_str("SceneDefinition.FromName fallback"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneGuidGenerationReason {
    MissingFromInput,
    ZeroInInput,
}

/// Scene GUID handling that mirrors SceneDefinition's zero-GUID randomization without inventing
/// a random value during inspection or planning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneDefinitionGuidPlan {
    Preserve(SceneGuidWords),
    GenerateAtExport(SceneGuidGenerationReason),
}

impl Display for SceneDefinitionGuidPlan {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preserve(guid) => write!(
                f,
                "preserve [0x{:08x}, 0x{:08x}, 0x{:08x}, 0x{:08x}]",
                guid.data[0], guid.data[1], guid.data[2], guid.data[3]
            ),
            Self::GenerateAtExport(SceneGuidGenerationReason::MissingFromInput) => {
                f.write_str("generate at export (SceneGUID absent)")
            }
            Self::GenerateAtExport(SceneGuidGenerationReason::ZeroInInput) => {
                f.write_str("generate at export (input SceneGUID is zero)")
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneDefinitionPlan {
    /// Stable identity of the source collection within this loaded workspace.
    pub collection_file_index: usize,
    /// Collection name after the baseline FileBase.NameFixed normalization.
    pub collection_name: String,
    pub name: String,
    pub path: String,
    pub path_source: SceneDefinitionPathSource,
    pub guid: SceneDefinitionGuidPlan,
}

/// Workspace representation of one source `SceneDefinition` and its collection membership.
/// The GUID remains a plan so zero or missing input GUIDs are not replaced before export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneDefinition {
    pub plan: SceneDefinitionPlan,
    collection_file_indices: Vec<usize>,
}

impl WorkspaceSceneDefinition {
    pub fn collection_file_indices(&self) -> &[usize] {
        &self.collection_file_indices
    }
}

/// Scene definitions indexed by workspace collection identity, with source `AddCollection`
/// ownership checks for future generated or derived collections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneDefinitions {
    definitions: Vec<WorkspaceSceneDefinition>,
    scene_by_collection: Vec<Option<usize>>,
    collection_assignment_count: usize,
    limits: WorkspaceSceneDefinitionLimits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneDefinitionLimits {
    pub max_scene_definitions: usize,
    pub max_collection_assignments: usize,
    pub max_workspace_collections: usize,
}

impl Default for WorkspaceSceneDefinitionLimits {
    fn default() -> Self {
        Self {
            max_scene_definitions: 1_000_000,
            max_collection_assignments: 1_000_000,
            max_workspace_collections: 1_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneDefinitionError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    SceneCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    CollectionAssignmentLimitExceeded {
        count: usize,
        limit: usize,
    },
    WorkspaceCollectionLimitExceeded {
        count: usize,
        limit: usize,
    },
    SceneIndexOutOfBounds {
        index: usize,
        count: usize,
    },
    CollectionFileIndexOutOfBounds {
        index: usize,
        count: usize,
    },
    CollectionAlreadyAssigned {
        collection_file_index: usize,
        scene_index: usize,
    },
}

impl Display for WorkspaceSceneDefinitionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(f, "could not allocate {requested} entries for {resource}"),
            Self::SceneCountLimitExceeded { count, limit } => write!(
                f,
                "SceneDefinition count {count} exceeds limit {limit}"
            ),
            Self::CollectionAssignmentLimitExceeded { count, limit } => write!(
                f,
                "SceneDefinition collection assignment count {count} exceeds limit {limit}"
            ),
            Self::WorkspaceCollectionLimitExceeded { count, limit } => write!(
                f,
                "workspace collection count {count} exceeds limit {limit}"
            ),
            Self::SceneIndexOutOfBounds { index, count } => {
                write!(f, "SceneDefinition index {index} is outside {count} definitions")
            }
            Self::CollectionFileIndexOutOfBounds { index, count } => write!(
                f,
                "workspace collection index {index} is outside {count} collections"
            ),
            Self::CollectionAlreadyAssigned {
                collection_file_index,
                scene_index,
            } => write!(
                f,
                "workspace collection {collection_file_index} already belongs to SceneDefinition {scene_index}"
            ),
        }
    }
}

impl Error for WorkspaceSceneDefinitionError {}

impl WorkspaceSceneDefinitions {
    pub fn from_plans(
        plans: Vec<SceneDefinitionPlan>,
        workspace_collection_count: usize,
        limits: WorkspaceSceneDefinitionLimits,
    ) -> Result<Self, WorkspaceSceneDefinitionError> {
        if plans.len() > limits.max_scene_definitions {
            return Err(WorkspaceSceneDefinitionError::SceneCountLimitExceeded {
                count: plans.len(),
                limit: limits.max_scene_definitions,
            });
        }
        if plans.len() > limits.max_collection_assignments {
            return Err(
                WorkspaceSceneDefinitionError::CollectionAssignmentLimitExceeded {
                    count: plans.len(),
                    limit: limits.max_collection_assignments,
                },
            );
        }
        if workspace_collection_count > limits.max_workspace_collections {
            return Err(
                WorkspaceSceneDefinitionError::WorkspaceCollectionLimitExceeded {
                    count: workspace_collection_count,
                    limit: limits.max_workspace_collections,
                },
            );
        }

        let mut definitions = Vec::new();
        definitions.try_reserve_exact(plans.len()).map_err(|_| {
            WorkspaceSceneDefinitionError::AllocationFailed {
                resource: "workspace scene definitions",
                requested: plans.len(),
            }
        })?;
        definitions.extend(plans.into_iter().map(|plan| WorkspaceSceneDefinition {
            plan,
            collection_file_indices: Vec::new(),
        }));

        let mut scene_by_collection = Vec::new();
        scene_by_collection
            .try_reserve_exact(workspace_collection_count)
            .map_err(|_| WorkspaceSceneDefinitionError::AllocationFailed {
                resource: "workspace collection-to-scene index",
                requested: workspace_collection_count,
            })?;
        scene_by_collection.resize(workspace_collection_count, None);

        let mut result = Self {
            definitions,
            scene_by_collection,
            collection_assignment_count: 0,
            limits,
        };
        for scene_index in 0..result.definitions.len() {
            let collection_file_index = result.definitions[scene_index].plan.collection_file_index;
            result.add_collection(scene_index, collection_file_index)?;
        }
        Ok(result)
    }

    pub fn definitions(&self) -> &[WorkspaceSceneDefinition] {
        &self.definitions
    }

    pub fn workspace_collection_count(&self) -> usize {
        self.scene_by_collection.len()
    }

    pub fn scene_index_for_collection(&self, collection_file_index: usize) -> Option<usize> {
        self.scene_by_collection
            .get(collection_file_index)
            .copied()
            .flatten()
    }

    /// Adds a collection to one scene, matching `SceneDefinition.AddCollection` ownership rules.
    pub fn add_collection(
        &mut self,
        scene_index: usize,
        collection_file_index: usize,
    ) -> Result<(), WorkspaceSceneDefinitionError> {
        let scene_count = self.definitions.len();
        let Some(scene) = self.definitions.get_mut(scene_index) else {
            return Err(WorkspaceSceneDefinitionError::SceneIndexOutOfBounds {
                index: scene_index,
                count: scene_count,
            });
        };
        let collection_count = self.scene_by_collection.len();
        let Some(assigned_scene) = self.scene_by_collection.get(collection_file_index) else {
            return Err(
                WorkspaceSceneDefinitionError::CollectionFileIndexOutOfBounds {
                    index: collection_file_index,
                    count: collection_count,
                },
            );
        };
        if let Some(existing_scene_index) = assigned_scene {
            return Err(WorkspaceSceneDefinitionError::CollectionAlreadyAssigned {
                collection_file_index,
                scene_index: *existing_scene_index,
            });
        }
        let requested_assignments = self.collection_assignment_count.checked_add(1).ok_or(
            WorkspaceSceneDefinitionError::CollectionAssignmentLimitExceeded {
                count: usize::MAX,
                limit: self.limits.max_collection_assignments,
            },
        )?;
        if requested_assignments > self.limits.max_collection_assignments {
            return Err(
                WorkspaceSceneDefinitionError::CollectionAssignmentLimitExceeded {
                    count: requested_assignments,
                    limit: self.limits.max_collection_assignments,
                },
            );
        }
        scene.collection_file_indices.try_reserve(1).map_err(|_| {
            WorkspaceSceneDefinitionError::AllocationFailed {
                resource: "SceneDefinition collection indexes",
                requested: scene.collection_file_indices.len().saturating_add(1),
            }
        })?;
        self.scene_by_collection[collection_file_index] = Some(scene_index);
        scene.collection_file_indices.push(collection_file_index);
        self.collection_assignment_count = requested_assignments;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneDefinitionPlanLimits {
    pub max_collection_name_bytes: usize,
    pub max_scene_name_bytes: usize,
    pub max_scene_path_bytes: usize,
}

impl Default for SceneDefinitionPlanLimits {
    fn default() -> Self {
        Self {
            max_collection_name_bytes: 1024 * 1024,
            max_scene_name_bytes: 16 * 1024 * 1024,
            max_scene_path_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneDefinitionPlanError {
    CollectionNameLimitExceeded {
        length: usize,
        limit: usize,
    },
    SceneNameLimitExceeded {
        length: usize,
        limit: usize,
    },
    ScenePathLimitExceeded {
        length: usize,
        limit: usize,
    },
    LengthOverflow {
        resource: &'static str,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for SceneDefinitionPlanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::CollectionNameLimitExceeded { length, limit } => write!(
                f,
                "scene collection name is {length} bytes; limit is {limit}"
            ),
            Self::SceneNameLimitExceeded { length, limit } => {
                write!(
                    f,
                    "SceneDefinition name is {length} bytes; limit is {limit}"
                )
            }
            Self::ScenePathLimitExceeded { length, limit } => {
                write!(f, "scene path is {length} bytes; limit is {limit}")
            }
            Self::LengthOverflow { resource } => {
                write!(f, "{resource} length overflows usize")
            }
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} bytes for {resource}")
            }
        }
    }
}

impl Error for SceneDefinitionPlanError {}

/// Build the same name/path choice as SceneDefinitionProcessor while leaving zero GUID creation
/// for the eventual exporter. Streamed-scene paths override BuildSettings paths; unresolved
/// BuildSettings metadata follows the source `FromName` fallback.
pub fn plan_scene_definition(
    collection_file_index: usize,
    raw_collection_name: &str,
    build_settings_path: &SceneCollectionPathStatus,
    streamed_scene_path: Option<&str>,
    scene_guid: Option<SceneGuidWords>,
    limits: SceneDefinitionPlanLimits,
) -> Result<SceneDefinitionPlan, SceneDefinitionPlanError> {
    if raw_collection_name.len() > limits.max_collection_name_bytes {
        return Err(SceneDefinitionPlanError::CollectionNameLimitExceeded {
            length: raw_collection_name.len(),
            limit: limits.max_collection_name_bytes,
        });
    }
    let collection_name = fix_file_identifier(raw_collection_name);
    if collection_name.len() > limits.max_collection_name_bytes {
        return Err(SceneDefinitionPlanError::CollectionNameLimitExceeded {
            length: collection_name.len(),
            limit: limits.max_collection_name_bytes,
        });
    }

    let (path, path_source) = if let Some(path) = streamed_scene_path {
        (
            copy_scene_string(path, "streamed-scene path", limits)?,
            SceneDefinitionPathSource::StreamedSceneBundle,
        )
    } else if let SceneCollectionPathStatus::Resolved {
        scene_index, path, ..
    } = build_settings_path
    {
        (
            copy_scene_string(path, "BuildSettings scene path", limits)?,
            SceneDefinitionPathSource::BuildSettings {
                scene_index: *scene_index,
            },
        )
    } else {
        let path_length = "Assets/Scenes/"
            .len()
            .checked_add(collection_name.len())
            .ok_or(SceneDefinitionPlanError::LengthOverflow {
                resource: "fallback scene path",
            })?;
        if path_length > limits.max_scene_path_bytes {
            return Err(SceneDefinitionPlanError::ScenePathLimitExceeded {
                length: path_length,
                limit: limits.max_scene_path_bytes,
            });
        }
        let mut path = String::new();
        path.try_reserve_exact(path_length).map_err(|_| {
            SceneDefinitionPlanError::AllocationFailed {
                resource: "fallback scene path",
                requested: path_length,
            }
        })?;
        path.push_str("Assets/Scenes/");
        path.push_str(&collection_name);
        (path, SceneDefinitionPathSource::CollectionNameFallback)
    };

    let name_source = match path_source {
        SceneDefinitionPathSource::CollectionNameFallback => collection_name.as_str(),
        SceneDefinitionPathSource::StreamedSceneBundle
        | SceneDefinitionPathSource::BuildSettings { .. } => {
            path.rsplit(['/', '\\']).next().unwrap_or_default()
        }
    };
    let name = copy_scene_name(name_source, limits.max_scene_name_bytes)?;
    let guid = match scene_guid {
        Some(guid) if !guid.is_zero() => SceneDefinitionGuidPlan::Preserve(guid),
        Some(_) => {
            SceneDefinitionGuidPlan::GenerateAtExport(SceneGuidGenerationReason::ZeroInInput)
        }
        None => {
            SceneDefinitionGuidPlan::GenerateAtExport(SceneGuidGenerationReason::MissingFromInput)
        }
    };

    Ok(SceneDefinitionPlan {
        collection_file_index,
        collection_name,
        name,
        path,
        path_source,
        guid,
    })
}

fn copy_scene_string(
    value: &str,
    resource: &'static str,
    limits: SceneDefinitionPlanLimits,
) -> Result<String, SceneDefinitionPlanError> {
    if value.len() > limits.max_scene_path_bytes {
        return Err(SceneDefinitionPlanError::ScenePathLimitExceeded {
            length: value.len(),
            limit: limits.max_scene_path_bytes,
        });
    }
    try_copy_scene_string(value, resource)
}

fn copy_scene_name(value: &str, limit: usize) -> Result<String, SceneDefinitionPlanError> {
    if value.len() > limit {
        return Err(SceneDefinitionPlanError::SceneNameLimitExceeded {
            length: value.len(),
            limit,
        });
    }
    try_copy_scene_string(value, "SceneDefinition name")
}

fn try_copy_scene_string(
    value: &str,
    resource: &'static str,
) -> Result<String, SceneDefinitionPlanError> {
    let mut copy = String::new();
    copy.try_reserve_exact(value.len()).map_err(|_| {
        SceneDefinitionPlanError::AllocationFailed {
            resource,
            requested: value.len(),
        }
    })?;
    copy.push_str(value);
    Ok(copy)
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
        SerializedValue::ManagedReferenceId { .. } => "managed reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
        SerializedValue::Raw { .. } => "raw",
    }
}

#[cfg(test)]
mod tests {
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

    use super::{
        extract_asset_bundle_container_references, extract_occlusion_scene_guid,
        extract_resource_manager_container_references, extract_streamed_scene_bundle_metadata,
        map_container_paths_to_scene_collections, map_scene_hashes_to_collections,
        map_streamed_scene_bundle_paths, plan_scene_definition, resolve_scene_collection_path,
        scene_manager_kind, streamed_scene_project_path, AssetBundleContainerDecodeError,
        AssetBundleContainerReference, BuildSettingsScenePathSet,
        ResourceManagerContainerDecodeError, ResourceManagerContainerReference,
        SceneBundleMappingError, SceneBundleMappingLimits, SceneBundlePathMapping,
        SceneBundlePathMappingSource, SceneCollectionPathStatus, SceneDefinitionGuidPlan,
        SceneDefinitionPathSource, SceneDefinitionPlanError, SceneDefinitionPlanLimits,
        SceneGuidError, SceneGuidGenerationReason, SceneGuidWords, SceneManagerKind,
        StreamedSceneBundleDecodeError, StreamedSceneBundleMetadata,
        StreamedSceneBundlePathMappings, WorkspaceSceneDefinitionError,
        WorkspaceSceneDefinitionLimits, WorkspaceSceneDefinitions,
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
    fn scene_definition_plan_applies_streamed_build_and_name_precedence() {
        let build_path = SceneCollectionPathStatus::Resolved {
            scene_index: 3,
            path: "Assets/Build/BuildName".to_owned(),
            duplicate_path: false,
        };
        let streamed = plan_scene_definition(
            17,
            "LIBRARY/Level0.assets",
            &build_path,
            Some("Assets/Streamed/BundleScene"),
            None,
            SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(streamed.collection_file_index, 17);
        assert_eq!(streamed.collection_name, "level0.assets");
        assert_eq!(streamed.name, "BundleScene");
        assert_eq!(streamed.path, "Assets/Streamed/BundleScene");
        assert_eq!(
            streamed.path_source,
            SceneDefinitionPathSource::StreamedSceneBundle
        );
        assert_eq!(
            streamed.guid,
            SceneDefinitionGuidPlan::GenerateAtExport(SceneGuidGenerationReason::MissingFromInput)
        );

        let build_settings = plan_scene_definition(
            17,
            "level0.assets",
            &build_path,
            None,
            None,
            SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(build_settings.name, "BuildName");
        assert_eq!(build_settings.path, "Assets/Build/BuildName");
        assert_eq!(
            build_settings.path_source,
            SceneDefinitionPathSource::BuildSettings { scene_index: 3 }
        );

        let windows_separator = plan_scene_definition(
            17,
            "level0.assets",
            &SceneCollectionPathStatus::Resolved {
                scene_index: 4,
                path: r"Assets\Build\WindowsName".to_owned(),
                duplicate_path: false,
            },
            None,
            None,
            SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(windows_separator.name, "WindowsName");

        let name_fallback = plan_scene_definition(
            17,
            "LIBRARY/Level0.assets",
            &SceneCollectionPathStatus::BuildSettingsMissing,
            None,
            None,
            SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(name_fallback.name, "level0.assets");
        assert_eq!(name_fallback.path, "Assets/Scenes/level0.assets");
        assert_eq!(
            name_fallback.path_source,
            SceneDefinitionPathSource::CollectionNameFallback
        );
    }

    #[test]
    fn scene_definition_plan_preserves_nonzero_guid_and_defers_zero_guid_generation() {
        let status = SceneCollectionPathStatus::BuildSettingsMissing;
        let limits = SceneDefinitionPlanLimits::default();
        let nonzero = SceneGuidWords { data: [1, 2, 0, 4] };
        let preserved =
            plan_scene_definition(0, "CAB-Scene", &status, None, Some(nonzero), limits).unwrap();
        assert_eq!(preserved.guid, SceneDefinitionGuidPlan::Preserve(nonzero));

        let zero = plan_scene_definition(
            0,
            "CAB-Scene",
            &status,
            None,
            Some(SceneGuidWords { data: [0; 4] }),
            limits,
        )
        .unwrap();
        assert_eq!(
            zero.guid,
            SceneDefinitionGuidPlan::GenerateAtExport(SceneGuidGenerationReason::ZeroInInput)
        );
        assert!(SceneGuidWords { data: [0; 4] }.is_zero());
    }

    #[test]
    fn workspace_scene_definitions_preserve_collection_ownership() {
        let status = SceneCollectionPathStatus::BuildSettingsMissing;
        let plans = vec![
            plan_scene_definition(
                1,
                "level0",
                &status,
                None,
                None,
                SceneDefinitionPlanLimits::default(),
            )
            .unwrap(),
            plan_scene_definition(
                4,
                "level1",
                &status,
                None,
                Some(SceneGuidWords { data: [1, 2, 3, 4] }),
                SceneDefinitionPlanLimits::default(),
            )
            .unwrap(),
        ];
        let mut definitions = WorkspaceSceneDefinitions::from_plans(
            plans,
            8,
            WorkspaceSceneDefinitionLimits::default(),
        )
        .unwrap();

        assert_eq!(definitions.scene_index_for_collection(1), Some(0));
        assert_eq!(definitions.scene_index_for_collection(4), Some(1));
        assert_eq!(definitions.scene_index_for_collection(7), None);
        assert_eq!(definitions.definitions()[0].collection_file_indices(), [1]);
        definitions.add_collection(0, 3).unwrap();
        assert_eq!(
            definitions.definitions()[0].collection_file_indices(),
            [1, 3]
        );
        assert_eq!(definitions.scene_index_for_collection(3), Some(0));
        assert_eq!(
            definitions.add_collection(1, 3),
            Err(WorkspaceSceneDefinitionError::CollectionAlreadyAssigned {
                collection_file_index: 3,
                scene_index: 0,
            })
        );
        assert_eq!(
            definitions.add_collection(0, 8),
            Err(
                WorkspaceSceneDefinitionError::CollectionFileIndexOutOfBounds {
                    index: 8,
                    count: 8,
                }
            )
        );
        assert_eq!(
            definitions.add_collection(2, 6),
            Err(WorkspaceSceneDefinitionError::SceneIndexOutOfBounds { index: 2, count: 2 })
        );
    }

    #[test]
    fn scene_definition_plan_bounds_names_and_paths() {
        let status = SceneCollectionPathStatus::Resolved {
            scene_index: 0,
            path: "Assets/Scenes/TooLong".to_owned(),
            duplicate_path: false,
        };
        let path_limits = SceneDefinitionPlanLimits {
            max_collection_name_bytes: 32,
            max_scene_name_bytes: 32,
            max_scene_path_bytes: 8,
        };
        assert_eq!(
            plan_scene_definition(0, "level0", &status, None, None, path_limits),
            Err(SceneDefinitionPlanError::ScenePathLimitExceeded {
                length: "Assets/Scenes/TooLong".len(),
                limit: 8,
            })
        );

        let name_limits = SceneDefinitionPlanLimits {
            max_collection_name_bytes: 32,
            max_scene_name_bytes: 3,
            max_scene_path_bytes: 64,
        };
        assert_eq!(
            plan_scene_definition(
                0,
                "x",
                &SceneCollectionPathStatus::BuildSettingsMissing,
                Some("Assets/Scenes/Main"),
                None,
                name_limits,
            ),
            Err(SceneDefinitionPlanError::SceneNameLimitExceeded {
                length: "Main".len(),
                limit: 3,
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

    #[test]
    fn asset_bundle_container_decoder_preserves_asset_pointer_file_scope() {
        let entry = |path: &str, file_id, path_id| {
            SerializedValue::Struct(vec![
                field(
                    "first",
                    SerializedValue::StringBytes(path.as_bytes().to_vec()),
                ),
                field(
                    "second",
                    SerializedValue::Struct(vec![
                        field("preloadIndex", SerializedValue::I32(0)),
                        field("preloadSize", SerializedValue::I32(0)),
                        field("asset", SerializedValue::PPtr { file_id, path_id }),
                    ]),
                ),
            ])
        };
        let root = SerializedValue::Struct(vec![field(
            "Base",
            SerializedValue::Struct(vec![field(
                "m_Container",
                SerializedValue::Array(vec![
                    entry("Assets/Local.prefab", 0, 17),
                    entry("Assets/Shared.mat", 2, 81),
                    entry("Assets/Null.asset", 0, 0),
                ]),
            )]),
        )]);

        assert_eq!(
            extract_asset_bundle_container_references(&root, SceneBundleMappingLimits::default(),)
                .unwrap(),
            [
                AssetBundleContainerReference {
                    path: "Assets/Local.prefab".to_owned(),
                    file_id: 0,
                    path_id: 17,
                },
                AssetBundleContainerReference {
                    path: "Assets/Shared.mat".to_owned(),
                    file_id: 2,
                    path_id: 81,
                },
                AssetBundleContainerReference {
                    path: "Assets/Null.asset".to_owned(),
                    file_id: 0,
                    path_id: 0,
                },
            ]
        );
    }

    #[test]
    fn asset_bundle_container_decoder_reports_missing_and_invalid_asset_pointers() {
        assert_eq!(
            extract_asset_bundle_container_references(
                &SerializedValue::Struct(Vec::new()),
                SceneBundleMappingLimits::default(),
            ),
            Err(AssetBundleContainerDecodeError::MissingContainerField)
        );

        let root = SerializedValue::Struct(vec![field(
            "m_Container",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field(
                    "first",
                    SerializedValue::StringBytes(b"Assets/Missing.asset".to_vec()),
                ),
                field("second", SerializedValue::Struct(Vec::new())),
            ])]),
        )]);
        assert_eq!(
            extract_asset_bundle_container_references(&root, SceneBundleMappingLimits::default(),),
            Err(AssetBundleContainerDecodeError::MissingAssetPointer { entry_index: 0 })
        );

        let root = SerializedValue::Struct(vec![field(
            "Container",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field(
                    "first",
                    SerializedValue::StringBytes(b"Assets/Wrong.asset".to_vec()),
                ),
                field(
                    "second",
                    SerializedValue::Struct(vec![field("asset", SerializedValue::I32(7))]),
                ),
            ])]),
        )]);
        assert!(matches!(
            extract_asset_bundle_container_references(
                &root,
                SceneBundleMappingLimits::default(),
            ),
            Err(AssetBundleContainerDecodeError::ContainerMetadata(
                StreamedSceneBundleDecodeError::WrongType {
                    field_path,
                    expected: "PPtr",
                    actual: "i32",
                }
            )) if field_path == "m_Container[0].second.asset"
        ));
    }

    #[test]
    fn asset_bundle_container_decoder_obeys_entry_and_path_limits() {
        let root = SerializedValue::Struct(vec![field(
            "m_Container",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("first", SerializedValue::StringBytes(b"a".to_vec())),
                field(
                    "second",
                    SerializedValue::Struct(vec![field(
                        "asset",
                        SerializedValue::PPtr {
                            file_id: 0,
                            path_id: 1,
                        },
                    )]),
                ),
            ])]),
        )]);

        assert!(matches!(
            extract_asset_bundle_container_references(
                &root,
                SceneBundleMappingLimits {
                    max_entries: 0,
                    max_total_name_and_path_bytes: usize::MAX,
                },
            ),
            Err(AssetBundleContainerDecodeError::ContainerMetadata(
                StreamedSceneBundleDecodeError::EntryCountLimitExceeded {
                    field_path,
                    count: 1,
                    limit: 0,
                }
            )) if field_path == "m_Container"
        ));
        assert!(matches!(
            extract_asset_bundle_container_references(
                &root,
                SceneBundleMappingLimits {
                    max_entries: 1,
                    max_total_name_and_path_bytes: 0,
                },
            ),
            Err(AssetBundleContainerDecodeError::ContainerMetadata(
                StreamedSceneBundleDecodeError::ByteLimitExceeded {
                    requested: 1,
                    limit: 0,
                }
            ))
        ));
    }

    #[test]
    fn resource_manager_container_decoder_preserves_order_and_pointer_scope() {
        let entry = |path: &str, file_id, path_id| {
            SerializedValue::Struct(vec![
                field(
                    "first",
                    SerializedValue::StringBytes(path.as_bytes().to_vec()),
                ),
                field("second", SerializedValue::PPtr { file_id, path_id }),
            ])
        };
        let root = SerializedValue::Struct(vec![field(
            "Base",
            SerializedValue::Struct(vec![field(
                "Container",
                SerializedValue::Array(vec![
                    entry("Textures/inner/wood", 0, 17),
                    entry("wood", 2, 81),
                    entry("Null", 0, 0),
                ]),
            )]),
        )]);

        assert_eq!(
            extract_resource_manager_container_references(
                &root,
                SceneBundleMappingLimits::default(),
            )
            .unwrap(),
            [
                ResourceManagerContainerReference {
                    path: "Textures/inner/wood".to_owned(),
                    file_id: 0,
                    path_id: 17,
                },
                ResourceManagerContainerReference {
                    path: "wood".to_owned(),
                    file_id: 2,
                    path_id: 81,
                },
                ResourceManagerContainerReference {
                    path: "Null".to_owned(),
                    file_id: 0,
                    path_id: 0,
                },
            ]
        );
    }

    #[test]
    fn resource_manager_container_decoder_reports_missing_and_invalid_values() {
        assert_eq!(
            extract_resource_manager_container_references(
                &SerializedValue::Struct(Vec::new()),
                SceneBundleMappingLimits::default(),
            ),
            Err(ResourceManagerContainerDecodeError::MissingContainerField)
        );

        let root = SerializedValue::Struct(vec![field(
            "m_Container",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("first", SerializedValue::StringBytes(b"bad".to_vec())),
                field("second", SerializedValue::I32(7)),
            ])]),
        )]);
        assert!(matches!(
            extract_resource_manager_container_references(
                &root,
                SceneBundleMappingLimits::default(),
            ),
            Err(ResourceManagerContainerDecodeError::Metadata(
                StreamedSceneBundleDecodeError::WrongType {
                    field_path,
                    expected: "PPtr",
                    actual: "i32",
                }
            )) if field_path == "m_Container[0].second"
        ));
    }

    #[test]
    fn streamed_scene_bundle_mapping_selects_scene_hashes_then_container_fallback() {
        let collection_names = [
            "CAB-Scene".to_owned(),
            "not-a-scene".to_owned(),
            "cab-scene".to_owned(),
        ];
        let collection_is_scene = [true, false, true];
        let scene_hashes_metadata = StreamedSceneBundleMetadata {
            is_streamed_scene_asset_bundle: Some(true),
            scene_hashes: Some(vec![(
                "Scenes/Main.unity".to_owned(),
                "CAB-Scene".to_owned(),
            )]),
            container_paths: Some(vec!["Ignored/Container.unity".to_owned()]),
        };
        assert_eq!(
            map_streamed_scene_bundle_paths(
                &scene_hashes_metadata,
                &collection_names,
                &collection_is_scene,
                SceneBundleMappingLimits::default(),
            )
            .unwrap(),
            Some(StreamedSceneBundlePathMappings {
                source: SceneBundlePathMappingSource::SceneHashes,
                mappings: vec![SceneBundlePathMapping {
                    collection_index: 0,
                    path: "Assets/Scenes/Main".to_owned(),
                }],
            })
        );

        let container_metadata = StreamedSceneBundleMetadata {
            is_streamed_scene_asset_bundle: Some(true),
            scene_hashes: Some(Vec::new()),
            container_paths: Some(vec![
                "Scenes/First.unity".to_owned(),
                "Scenes/Second.unity".to_owned(),
            ]),
        };
        assert_eq!(
            map_streamed_scene_bundle_paths(
                &container_metadata,
                &collection_names,
                &collection_is_scene,
                SceneBundleMappingLimits::default(),
            )
            .unwrap(),
            Some(StreamedSceneBundlePathMappings {
                source: SceneBundlePathMappingSource::Container,
                mappings: vec![
                    SceneBundlePathMapping {
                        collection_index: 0,
                        path: "Assets/Scenes/First".to_owned(),
                    },
                    SceneBundlePathMapping {
                        collection_index: 2,
                        path: "Assets/Scenes/Second".to_owned(),
                    },
                ],
            })
        );
    }

    #[test]
    fn streamed_scene_bundle_mapping_keeps_unselected_and_missing_states_explicit() {
        let not_streamed = StreamedSceneBundleMetadata {
            is_streamed_scene_asset_bundle: Some(false),
            scene_hashes: None,
            container_paths: None,
        };
        assert_eq!(
            map_streamed_scene_bundle_paths(
                &not_streamed,
                &[],
                &[],
                SceneBundleMappingLimits::default(),
            )
            .unwrap(),
            None
        );

        let streamed_without_paths = StreamedSceneBundleMetadata {
            is_streamed_scene_asset_bundle: Some(true),
            scene_hashes: None,
            container_paths: None,
        };
        assert_eq!(
            map_streamed_scene_bundle_paths(
                &streamed_without_paths,
                &[],
                &[],
                SceneBundleMappingLimits::default(),
            ),
            Err(SceneBundleMappingError::MissingContainerScenePaths)
        );

        let empty_container = StreamedSceneBundleMetadata {
            is_streamed_scene_asset_bundle: Some(true),
            scene_hashes: Some(Vec::new()),
            container_paths: Some(Vec::new()),
        };
        assert_eq!(
            map_streamed_scene_bundle_paths(
                &empty_container,
                &["collection".to_owned()],
                &[],
                SceneBundleMappingLimits::default(),
            ),
            Err(SceneBundleMappingError::CollectionMetadataCountMismatch {
                collection_name_count: 1,
                scene_flag_count: 0,
            })
        );
    }

    #[test]
    fn extracts_streamed_scene_bundle_fields_from_type_tree_values() {
        let root = SerializedValue::Struct(vec![
            field(
                "Base",
                SerializedValue::Struct(vec![field(
                    "m_IsStreamedSceneAssetBundle",
                    SerializedValue::Bool(true),
                )]),
            ),
            field(
                "m_SceneHashes",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    field(
                        "first",
                        SerializedValue::StringBytes(b"Scenes/Main.unity".to_vec()),
                    ),
                    field(
                        "second",
                        SerializedValue::StringBytes(b"CAB-Scene".to_vec()),
                    ),
                ])]),
            ),
            field(
                "m_Container",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    field(
                        "first",
                        SerializedValue::StringBytes(b"Scenes/Other.unity".to_vec()),
                    ),
                    field(
                        "second",
                        SerializedValue::Struct(vec![field(
                            "asset",
                            SerializedValue::PPtr {
                                file_id: 0,
                                path_id: 0,
                            },
                        )]),
                    ),
                ])]),
            ),
        ]);

        assert_eq!(
            extract_streamed_scene_bundle_metadata(&root, SceneBundleMappingLimits::default())
                .unwrap(),
            StreamedSceneBundleMetadata {
                is_streamed_scene_asset_bundle: Some(true),
                scene_hashes: Some(vec![(
                    "Scenes/Main.unity".to_owned(),
                    "CAB-Scene".to_owned()
                )]),
                container_paths: Some(vec!["Scenes/Other.unity".to_owned()]),
            }
        );
    }

    #[test]
    fn streamed_scene_bundle_metadata_preserves_absence_and_false_flag() {
        let absent = SerializedValue::Struct(Vec::new());
        assert_eq!(
            extract_streamed_scene_bundle_metadata(&absent, SceneBundleMappingLimits::default())
                .unwrap(),
            StreamedSceneBundleMetadata {
                is_streamed_scene_asset_bundle: None,
                scene_hashes: None,
                container_paths: None,
            }
        );

        let false_flag = SerializedValue::Struct(vec![field(
            "IsStreamedSceneAssetBundle",
            SerializedValue::Bool(false),
        )]);
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &false_flag,
                SceneBundleMappingLimits::default()
            )
            .unwrap()
            .is_streamed_scene_asset_bundle,
            Some(false)
        );
    }

    #[test]
    fn streamed_scene_bundle_metadata_rejects_ambiguous_and_unbounded_data() {
        let duplicate_flag = SerializedValue::Struct(vec![
            field("m_IsStreamedSceneAssetBundle", SerializedValue::Bool(true)),
            field("IsStreamedSceneAssetBundle", SerializedValue::Bool(true)),
        ]);
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &duplicate_flag,
                SceneBundleMappingLimits::default()
            ),
            Err(StreamedSceneBundleDecodeError::DuplicateAliases {
                primary: "m_IsStreamedSceneAssetBundle",
                alternate: "IsStreamedSceneAssetBundle",
            })
        );

        let too_many = SerializedValue::Struct(vec![field(
            "m_Container",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![])]),
        )]);
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &too_many,
                SceneBundleMappingLimits {
                    max_entries: 0,
                    max_total_name_and_path_bytes: usize::MAX,
                }
            ),
            Err(StreamedSceneBundleDecodeError::EntryCountLimitExceeded {
                field_path: "m_Container".to_owned(),
                count: 1,
                limit: 0,
            })
        );

        let combined_entries = SerializedValue::Struct(vec![
            field(
                "m_SceneHashes",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    field("first", SerializedValue::StringBytes(b"a".to_vec())),
                    field("second", SerializedValue::StringBytes(b"b".to_vec())),
                ])]),
            ),
            field(
                "m_Container",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    field("first", SerializedValue::StringBytes(b"c".to_vec())),
                    field("second", SerializedValue::Struct(Vec::new())),
                ])]),
            ),
        ]);
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &combined_entries,
                SceneBundleMappingLimits {
                    max_entries: 1,
                    max_total_name_and_path_bytes: usize::MAX,
                }
            ),
            Err(StreamedSceneBundleDecodeError::EntryCountLimitExceeded {
                field_path: "m_Container".to_owned(),
                count: 2,
                limit: 1,
            })
        );

        let invalid_utf8 = SerializedValue::Struct(vec![field(
            "m_SceneHashes",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("first", SerializedValue::StringBytes(vec![0xff])),
                field("second", SerializedValue::StringBytes(Vec::new())),
            ])]),
        )]);
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &invalid_utf8,
                SceneBundleMappingLimits::default()
            ),
            Err(StreamedSceneBundleDecodeError::InvalidUtf8 {
                field_path: "m_SceneHashes[0].first".to_owned(),
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
