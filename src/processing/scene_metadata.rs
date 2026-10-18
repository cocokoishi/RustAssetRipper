//! Typed extraction of scene-related metadata from serialized Unity values.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str;

use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::header::SerializedFileHeader;
use crate::serialized_file::metadata::{ObjectInfo, SerializedFileMetadata};
use crate::serialized_file::object::{
    ObjectDecodeLimits, SerializedValue, SerializedValueLookupError,
};

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
/// Current TypeTrees use `m_Scenes`; older layouts used `m_Levels`. Generated BuildSettings
/// classes expose the corresponding normalized `scenes`/`levels` property names. Entries may be
/// serialized strings or structs with a `path` field.
pub fn extract_build_settings_scene_paths(
    root: &SerializedValue,
    limits: BuildSettingsSceneLimits,
) -> Result<Vec<String>, BuildSettingsSceneError> {
    let mut scenes_field = None;
    for name in ["m_Scenes", "scenes"] {
        let value = root.unique_field_in_base_chain(name).map_err(|source| {
            BuildSettingsSceneError::FieldLookup {
                field_name: "m_Scenes",
                source,
            }
        })?;
        if let Some(value) = value {
            if scenes_field.replace(value).is_some() {
                return Err(BuildSettingsSceneError::BothSceneArrayFieldsPresent);
            }
        }
    }
    let mut levels_field = None;
    for name in ["m_Levels", "levels"] {
        let value = root.unique_field_in_base_chain(name).map_err(|source| {
            BuildSettingsSceneError::FieldLookup {
                field_name: "m_Levels",
                source,
            }
        })?;
        if let Some(value) = value {
            if levels_field.replace(value).is_some() {
                return Err(BuildSettingsSceneError::BothSceneArrayFieldsPresent);
            }
        }
    }
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

/// Decode one BuildSettings object from a parsed SerializedFile and extract its ordered paths.
pub fn decode_build_settings_scene_paths(
    file: &SerializedFile<'_>,
    object_index: usize,
    scene_limits: BuildSettingsSceneLimits,
    object_limits: &ObjectDecodeLimits,
) -> Result<Vec<String>, BuildSettingsSceneError> {
    decode_build_settings_scene_paths_from_parts(
        &file.metadata,
        &file.header,
        file.input(),
        object_index,
        scene_limits,
        object_limits,
    )
}

/// Decode a BuildSettings object when metadata and original bytes are held separately by a workspace.
pub fn decode_build_settings_scene_paths_from_parts(
    metadata: &SerializedFileMetadata,
    header: &SerializedFileHeader,
    file_bytes: &[u8],
    object_index: usize,
    scene_limits: BuildSettingsSceneLimits,
    object_limits: &ObjectDecodeLimits,
) -> Result<Vec<String>, BuildSettingsSceneError> {
    let object = metadata.objects.get(object_index).ok_or(
        BuildSettingsSceneError::ObjectIndexOutOfBounds {
            object_index,
            object_count: metadata.objects.len(),
        },
    )?;
    let class_id = object_scene_class_id(object);
    if class_id != Some(BUILD_SETTINGS_CLASS_ID) {
        return Err(BuildSettingsSceneError::UnexpectedObjectClass {
            object_index,
            actual_class_id: class_id,
        });
    }
    let payload = metadata
        .decode_object_payload_with_limits(file_bytes, header, object_index, object_limits)
        .map_err(|error| BuildSettingsSceneError::ObjectDecode {
            object_index,
            message: error.to_string(),
        })?;
    extract_build_settings_scene_paths(&payload.root, scene_limits)
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

const BUILD_SETTINGS_CLASS_ID: i32 = 141;

fn object_scene_class_id(object: &ObjectInfo) -> Option<i32> {
    object
        .class_id
        .map(i32::from)
        .or_else(|| object.resolved_type_index.map(|_| object.raw_type_id))
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildSettingsSceneError {
    ObjectIndexOutOfBounds {
        object_index: usize,
        object_count: usize,
    },
    UnexpectedObjectClass {
        object_index: usize,
        actual_class_id: Option<i32>,
    },
    ObjectDecode {
        object_index: usize,
        message: String,
    },
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
            Self::ObjectIndexOutOfBounds {
                object_index,
                object_count,
            } => write!(
                f,
                "BuildSettings object index {object_index} is outside the {object_count}-object table"
            ),
            Self::UnexpectedObjectClass {
                object_index,
                actual_class_id,
            } => write!(
                f,
                "object {object_index} has class ID {actual_class_id:?}; expected BuildSettings class ID {BUILD_SETTINGS_CLASS_ID}"
            ),
            Self::ObjectDecode {
                object_index,
                message,
            } => write!(f, "could not decode BuildSettings object {object_index}: {message}"),
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
pub(crate) mod tests {
    use super::{
        decode_build_settings_scene_paths, extract_build_settings_scene_paths,
        scene_path_from_build_settings_value, BuildSettingsSceneError, BuildSettingsSceneLimits,
    };
    use crate::processing::scene_collections::{
        extract_asset_bundle_container_references, extract_streamed_scene_bundle_metadata,
        AssetBundleContainerReference, SceneBundleMappingLimits, StreamedSceneBundleMetadata,
    };
    use crate::processing::scene_naming::{scene_path_for_collection, SceneUnityVersion};
    use crate::serialized_file::file::SerializedFile;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::object::{
        ObjectDecodeLimits, SerializedFieldValue, SerializedValue,
    };

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

    fn put_u16_le(output: &mut Vec<u8>, value: u16) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_i32_le(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32_le(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn push_tree_string(strings: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(strings.len()).unwrap();
        strings.extend_from_slice(value.as_bytes());
        strings.push(0);
        offset
    }

    fn put_tree_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        put_u16_le(output, 1);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, 0);
    }

    fn add_tree_node(
        strings: &mut Vec<u8>,
        nodes: &mut Vec<(u8, u32, u32, i32, i32)>,
        level: u8,
        type_name: &str,
        field_name: &str,
        byte_size: i32,
    ) {
        let type_offset = push_tree_string(strings, type_name);
        let name_offset = push_tree_string(strings, field_name);
        let index = i32::try_from(nodes.len()).unwrap();
        nodes.push((level, type_offset, name_offset, byte_size, index));
    }

    fn put_string_payload(output: &mut Vec<u8>, value: &str) {
        let bytes = value.as_bytes();
        put_i32_le(output, i32::try_from(bytes.len()).unwrap());
        output.extend_from_slice(bytes);
        let padding = (4 - output.len() % 4) % 4;
        output.resize(output.len() + padding, 0);
    }

    fn make_asset_bundle_file() -> Vec<u8> {
        let mut strings = Vec::new();
        let mut nodes = Vec::new();
        add_tree_node(&mut strings, &mut nodes, 0, "AssetBundle", "", -1);

        add_tree_node(&mut strings, &mut nodes, 1, "map", "m_Container", -1);
        add_tree_node(&mut strings, &mut nodes, 2, "Array", "Array", -1);
        add_tree_node(&mut strings, &mut nodes, 3, "int", "size", 4);
        add_tree_node(&mut strings, &mut nodes, 3, "pair", "data", -1);
        add_tree_node(&mut strings, &mut nodes, 4, "string", "first", -1);
        add_tree_node(&mut strings, &mut nodes, 5, "Array", "Array", -1);
        add_tree_node(&mut strings, &mut nodes, 6, "int", "size", 4);
        add_tree_node(&mut strings, &mut nodes, 6, "char", "data", 1);
        add_tree_node(&mut strings, &mut nodes, 4, "AssetInfo", "second", -1);
        add_tree_node(&mut strings, &mut nodes, 5, "int", "preloadIndex", 4);
        add_tree_node(&mut strings, &mut nodes, 5, "int", "preloadSize", 4);
        add_tree_node(&mut strings, &mut nodes, 5, "PPtr<Object>", "asset", 12);
        add_tree_node(&mut strings, &mut nodes, 6, "int", "m_FileID", 4);
        add_tree_node(&mut strings, &mut nodes, 6, "SInt64", "m_PathID", 8);

        add_tree_node(
            &mut strings,
            &mut nodes,
            1,
            "bool",
            "m_IsStreamedSceneAssetBundle",
            1,
        );
        add_tree_node(&mut strings, &mut nodes, 1, "map", "m_SceneHashes", -1);
        add_tree_node(&mut strings, &mut nodes, 2, "Array", "Array", -1);
        add_tree_node(&mut strings, &mut nodes, 3, "int", "size", 4);
        add_tree_node(&mut strings, &mut nodes, 3, "pair", "data", -1);
        for member in ["first", "second"] {
            add_tree_node(&mut strings, &mut nodes, 4, "string", member, -1);
            add_tree_node(&mut strings, &mut nodes, 5, "Array", "Array", -1);
            add_tree_node(&mut strings, &mut nodes, 6, "int", "size", 4);
            add_tree_node(&mut strings, &mut nodes, 6, "char", "data", 1);
        }

        let node_count = i32::try_from(nodes.len()).unwrap();
        let mut tree = Vec::new();
        for (level, type_offset, name_offset, byte_size, index) in nodes {
            put_tree_node(&mut tree, level, type_offset, name_offset, byte_size, index);
        }

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 142);
        put_i32_le(&mut metadata, node_count);
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        metadata.extend_from_slice(&tree);
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, 1);
        put_string_payload(&mut payload, "Assets/Scenes/Main.unity");
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        payload.extend_from_slice(&0_i64.to_le_bytes());
        payload.push(1);
        put_i32_le(&mut payload, 1);
        put_string_payload(&mut payload, "Assets/Scenes/Main.unity");
        put_string_payload(&mut payload, "CAB-Scene");

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 142);
        put_u16_le(&mut metadata, 142);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&FormatVersion::Unknown10.raw().to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_build_settings_file() -> Vec<u8> {
        let mut strings = Vec::new();
        let build_settings_type = push_tree_string(&mut strings, "BuildSettings");
        let empty = push_tree_string(&mut strings, "");
        let vector_type = push_tree_string(&mut strings, "vector");
        let scenes_field = push_tree_string(&mut strings, "m_Scenes");
        let array_type = push_tree_string(&mut strings, "Array");
        let int_type = push_tree_string(&mut strings, "int");
        let size_field = push_tree_string(&mut strings, "size");
        let string_type = push_tree_string(&mut strings, "string");
        let data_field = push_tree_string(&mut strings, "data");
        let char_type = push_tree_string(&mut strings, "char");

        let mut tree = Vec::new();
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, build_settings_type, empty, -1, 0),
            (1, vector_type, scenes_field, -1, 1),
            (2, array_type, array_type, -1, 2),
            (3, int_type, size_field, 4, 3),
            (3, string_type, data_field, -1, 4),
            (4, array_type, array_type, -1, 5),
            (5, int_type, size_field, 4, 6),
            (5, char_type, data_field, 1, 7),
        ] {
            put_tree_node(&mut tree, level, type_offset, name_offset, byte_size, index);
        }

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 141);
        put_i32_le(&mut metadata, 8);
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        metadata.extend_from_slice(&tree);
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let scene_paths = [
            b"Assets/Scenes/Main.unity".as_slice(),
            b"Assets/Scenes/Next.unity",
        ];
        let mut payload = Vec::new();
        put_i32_le(&mut payload, i32::try_from(scene_paths.len()).unwrap());
        for path in scene_paths {
            put_i32_le(&mut payload, i32::try_from(path.len()).unwrap());
            payload.extend_from_slice(path);
            let padding = (4 - path.len() % 4) % 4;
            payload.resize(payload.len() + padding, 0);
        }

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 141);
        put_u16_le(&mut metadata, 141);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&FormatVersion::Unknown10.raw().to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn put_tree_node_with_flags(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
        flags: u32,
    ) {
        put_u16_le(output, 1);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, flags);
    }

    fn add_string_vector_nodes(
        strings: &mut Vec<u8>,
        nodes: &mut Vec<(u8, u32, u32, i32, i32, u32)>,
        level: u8,
        field_name: &str,
    ) {
        let vector_type = push_tree_string(strings, "vector");
        let field = push_tree_string(strings, field_name);
        let array_type = push_tree_string(strings, "Array");
        let int_type = push_tree_string(strings, "int");
        let size_field = push_tree_string(strings, "size");
        let string_type = push_tree_string(strings, "string");
        let data_field = push_tree_string(strings, "data");
        let char_type = push_tree_string(strings, "char");
        let mut index = i32::try_from(nodes.len()).unwrap();
        let mut push = |nodes: &mut Vec<(u8, u32, u32, i32, i32, u32)>,
                        level: u8,
                        type_offset: u32,
                        name_offset: u32,
                        byte_size: i32| {
            nodes.push((level, type_offset, name_offset, byte_size, index, 0));
            index += 1;
        };
        push(nodes, level, vector_type, field, -1);
        push(nodes, level + 1, array_type, array_type, -1);
        push(nodes, level + 2, int_type, size_field, 4);
        push(nodes, level + 2, string_type, data_field, -1);
        push(nodes, level + 3, array_type, array_type, -1);
        push(nodes, level + 4, int_type, size_field, 4);
        push(nodes, level + 4, char_type, data_field, 1);
    }

    /// Full-layout class-141 fixture following the reference TPK release trees:
    /// every field of the real versioned layout is present in TPK order with
    /// canonical Unity alignment, so the shipped fixed-layout reader consumes
    /// the payload byte-exactly. Only the scene-vector name/values vary.
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    fn make_full_layout_build_settings_file(
        version_text: &[u8],
        scenes_field: &str,
        scene_paths: &[&str],
        extra_vectors: &[&str],
        bools: &[&str],
        bool_values: &[u8],
        has_guid: bool,
        map_names: &[&str],
        modern_maps: bool,
        has_graphics_apis: bool,
        version_string: &str,
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let mut nodes: Vec<(u8, u32, u32, i32, i32, u32)> = Vec::new();
        let root_type = push_tree_string(&mut strings, "BuildSettings");
        let empty = push_tree_string(&mut strings, "");
        nodes.push((0, root_type, empty, -1, 0, 0));
        add_string_vector_nodes(&mut strings, &mut nodes, 1, scenes_field);
        for extra in extra_vectors {
            add_string_vector_nodes(&mut strings, &mut nodes, 1, extra);
        }
        if has_guid {
            let guid_type = push_tree_string(&mut strings, "GUID");
            let guid_field = push_tree_string(&mut strings, "m_BuildGUID");
            let uint_type = push_tree_string(&mut strings, "unsigned int");
            let mut index = i32::try_from(nodes.len()).unwrap();
            nodes.push((1, guid_type, guid_field, 16, index, 0));
            index += 1;
            for entry in ["data[0]", "data[1]", "data[2]", "data[3]"] {
                let name = push_tree_string(&mut strings, entry);
                nodes.push((2, uint_type, name, 4, index, 0));
                index += 1;
            }
        }
        let bool_type = push_tree_string(&mut strings, "bool");
        for (position, name) in bools.iter().enumerate() {
            let field = push_tree_string(&mut strings, name);
            let index = i32::try_from(nodes.len()).unwrap();
            // The real trees carry AlignBytes (0x4000) on the last bool so the
            // following string starts on a four-byte boundary.
            let flags = if position + 1 == bools.len() {
                0x4000
            } else {
                0
            };
            nodes.push((1, bool_type, field, 1, index, flags));
        }
        for name in ["m_Version", "m_AuthToken"] {
            let string_type = push_tree_string(&mut strings, "string");
            let field = push_tree_string(&mut strings, name);
            let array_type = push_tree_string(&mut strings, "Array");
            let int_type = push_tree_string(&mut strings, "int");
            let size_field = push_tree_string(&mut strings, "size");
            let char_type = push_tree_string(&mut strings, "char");
            let data_field = push_tree_string(&mut strings, "data");
            let mut index = i32::try_from(nodes.len()).unwrap();
            let mut push = |level: u8, type_offset: u32, name_offset: u32, byte_size: i32| {
                nodes.push((level, type_offset, name_offset, byte_size, index, 0));
                index += 1;
            };
            push(1, string_type, field, -1);
            push(2, array_type, array_type, -1);
            push(3, int_type, size_field, 4);
            push(3, char_type, data_field, 1);
        }
        for map_name in map_names {
            let map_type = push_tree_string(&mut strings, "map");
            let field = push_tree_string(&mut strings, map_name);
            let array_type = push_tree_string(&mut strings, "Array");
            let int_type = push_tree_string(&mut strings, "int");
            let size_field = push_tree_string(&mut strings, "size");
            let pair_type = push_tree_string(&mut strings, "pair");
            let data_field = push_tree_string(&mut strings, "data");
            let mut index = i32::try_from(nodes.len()).unwrap();
            let mut push = |level: u8, type_offset: u32, name_offset: u32, byte_size: i32| {
                nodes.push((level, type_offset, name_offset, byte_size, index, 0));
                index += 1;
            };
            push(1, map_type, field, -1);
            push(2, array_type, array_type, -1);
            push(3, int_type, size_field, 4);
            push(3, pair_type, data_field, -1);
            // Pair children mirror the TPK shapes (never traversed for the
            // empty maps in these fixtures, but kept faithful).
            if modern_maps && map_name.ends_with("ScriptHashes") {
                let hash_type = push_tree_string(&mut strings, "Hash128");
                for key in ["first", "second"] {
                    let key_field = push_tree_string(&mut strings, key);
                    push(4, hash_type, key_field, 16);
                    let byte_type = push_tree_string(&mut strings, "UInt8");
                    for entry in 0..16 {
                        let name = push_tree_string(&mut strings, &format!("bytes[{entry}]"));
                        push(5, byte_type, name, 1);
                    }
                }
            } else if modern_maps {
                let type_ptr = push_tree_string(&mut strings, "Type*");
                let first_field = push_tree_string(&mut strings, "first");
                push(4, type_ptr, first_field, 4);
                let hash_type = push_tree_string(&mut strings, "Hash128");
                let second_field = push_tree_string(&mut strings, "second");
                push(4, hash_type, second_field, 16);
                let byte_type = push_tree_string(&mut strings, "UInt8");
                for entry in 0..16 {
                    let name = push_tree_string(&mut strings, &format!("bytes[{entry}]"));
                    push(5, byte_type, name, 1);
                }
            } else {
                let first_field = push_tree_string(&mut strings, "first");
                push(4, int_type, first_field, 4);
                let uint_type = push_tree_string(&mut strings, "unsigned int");
                let second_field = push_tree_string(&mut strings, "second");
                push(4, uint_type, second_field, 4);
            }
        }
        if has_graphics_apis {
            let vector_type = push_tree_string(&mut strings, "vector");
            let field = push_tree_string(&mut strings, "m_GraphicsAPIs");
            let array_type = push_tree_string(&mut strings, "Array");
            let int_type = push_tree_string(&mut strings, "int");
            let size_field = push_tree_string(&mut strings, "size");
            let data_field = push_tree_string(&mut strings, "data");
            let mut index = i32::try_from(nodes.len()).unwrap();
            let mut push = |level: u8, type_offset: u32, name_offset: u32, byte_size: i32| {
                nodes.push((level, type_offset, name_offset, byte_size, index, 0));
                index += 1;
            };
            push(1, vector_type, field, -1);
            push(2, array_type, array_type, -1);
            push(3, int_type, size_field, 4);
            push(3, int_type, data_field, 4);
        }

        let mut tree = Vec::new();
        for (level, type_offset, name_offset, byte_size, index, flags) in &nodes {
            put_tree_node_with_flags(
                &mut tree,
                *level,
                *type_offset,
                *name_offset,
                *byte_size,
                *index,
                *flags,
            );
        }

        let mut metadata = Vec::new();
        metadata.extend_from_slice(version_text);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 141);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        metadata.extend_from_slice(&tree);
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, i32::try_from(scene_paths.len()).unwrap());
        for path in scene_paths {
            put_string_payload(&mut payload, path);
        }
        for _ in extra_vectors {
            put_i32_le(&mut payload, 0);
        }
        if has_guid {
            for word in [0x0102_0304u32, 0x0506_0708, 0x090a_0b0c, 0x0d0e_0f10] {
                put_u32_le(&mut payload, word);
            }
        }
        for value in bool_values {
            payload.push(*value);
        }
        payload.resize(payload.len() + (4 - payload.len() % 4) % 4, 0);
        put_string_payload(&mut payload, version_string);
        put_string_payload(&mut payload, "");
        for _ in map_names {
            put_i32_le(&mut payload, 0);
        }
        if has_graphics_apis {
            put_i32_le(&mut payload, 0);
        }

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 141);
        put_u16_le(&mut metadata, 141);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&FormatVersion::Unknown10.raw().to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_modern_full_build_settings_file() -> Vec<u8> {
        make_reference_build_settings_file(&[
            "Assets/Scenes/Main.unity",
            "Assets/Scenes/Extra.unity",
        ])
    }

    /// Build a 2019.4 class-141 file that the frozen generated factory materializes as
    /// `BuildSettings`, including every real TPK field and alignment lane.
    pub(crate) fn make_reference_build_settings_file(scene_paths: &[&str]) -> Vec<u8> {
        make_full_layout_build_settings_file(
            b"2019.4.40f1\0",
            "m_Scenes",
            scene_paths,
            &["m_PreloadedPlugins", "m_EnabledVRDevices", "m_BuildTags"],
            &[
                "hasPROVersion",
                "isNoWatermarkBuild",
                "isPrototypingBuild",
                "isEducationalBuild",
                "isEmbedded",
                "isTrial",
                "hasPublishingRights",
                "hasShadows",
                "hasSoftShadows",
                "hasLocalLightShadows",
                "hasAdvancedVersion",
                "enableDynamicBatching",
                "isDebugBuild",
                "usesOnMouseEvents",
                "hasClusterRendering",
            ],
            &[0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0],
            true,
            &["m_RuntimeClassHashes", "m_ScriptHashes"],
            true,
            true,
            "2019.4.40f1",
        )
    }

    fn make_legacy_full_build_settings_file() -> Vec<u8> {
        make_full_layout_build_settings_file(
            b"5.0.0f1\0",
            "m_Levels",
            &["Assets/Legacy/First.unity", "Assets/Legacy/Second.unity"],
            &["m_PreloadedPlugins"],
            &[
                "hasRenderTexture",
                "hasPROVersion",
                "isNoWatermarkBuild",
                "isPrototypingBuild",
                "isEducationalBuild",
                "isEmbedded",
                "hasPublishingRights",
                "hasShadows",
                "hasSoftShadows",
                "hasLocalLightShadows",
                "hasAdvancedVersion",
                "enableDynamicBatching",
                "isDebugBuild",
                "usesOnMouseEvents",
                "enableMultipleDisplays",
                "hasClusterRendering",
            ],
            &[0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0],
            false,
            &["m_RuntimeClassHashes", "m_ScriptHashes"],
            false,
            false,
            "5.0.0f1",
        )
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

        let generated_current = SerializedValue::Struct(vec![field(
            "Base",
            SerializedValue::Struct(vec![field(
                "scenes",
                SerializedValue::Array(vec![scene_path(b"Assets/Scenes/Generated.unity")]),
            )]),
        )]);
        assert_eq!(
            extract_build_settings_scene_paths(
                &generated_current,
                BuildSettingsSceneLimits::default()
            )
            .unwrap(),
            ["Assets/Scenes/Generated.unity"]
        );

        let generated_legacy = SerializedValue::Struct(vec![field(
            "levels",
            SerializedValue::Array(vec![SerializedValue::StringBytes(
                b"Assets/Scenes/GeneratedOld.unity".to_vec(),
            )]),
        )]);
        assert_eq!(
            extract_build_settings_scene_paths(
                &generated_legacy,
                BuildSettingsSceneLimits::default()
            )
            .unwrap(),
            ["Assets/Scenes/GeneratedOld.unity"]
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
    fn decodes_build_settings_paths_from_a_complete_serialized_file() {
        let bytes = make_build_settings_file();
        let file = SerializedFile::parse(&bytes).unwrap();
        let paths = decode_build_settings_scene_paths(
            &file,
            0,
            BuildSettingsSceneLimits::default(),
            &ObjectDecodeLimits::default(),
        )
        .unwrap();
        let version =
            SceneUnityVersion::parse(file.metadata.prefix.unity_version_text.as_deref().unwrap())
                .unwrap();
        assert_eq!(
            scene_path_for_collection("level1", version, &paths).as_deref(),
            Some("Assets/Scenes/Next")
        );
    }

    #[test]
    fn decodes_full_layout_build_settings_through_the_production_path() {
        for (bytes, expected) in [
            (
                make_modern_full_build_settings_file(),
                vec![
                    "Assets/Scenes/Main.unity".to_owned(),
                    "Assets/Scenes/Extra.unity".to_owned(),
                ],
            ),
            (
                make_legacy_full_build_settings_file(),
                vec![
                    "Assets/Legacy/First.unity".to_owned(),
                    "Assets/Legacy/Second.unity".to_owned(),
                ],
            ),
        ] {
            let file = SerializedFile::parse(&bytes).unwrap();
            let object = file.decode_object_payload(0).unwrap();
            assert!(object.trailing_bytes.is_empty());
            let paths = decode_build_settings_scene_paths(
                &file,
                0,
                BuildSettingsSceneLimits::default(),
                &ObjectDecodeLimits::default(),
            )
            .unwrap();
            assert_eq!(paths, expected);
        }
        // Scratch copies for the isolated reference probe (`materialize`
        // mode); the probe output is committed as
        // `buildsettings_probe_fixture.txt` and replayed below.
        let scratch = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("buildsettings-probe");
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(
            scratch.join("modern.assets"),
            make_modern_full_build_settings_file(),
        )
        .unwrap();
        std::fs::write(
            scratch.join("legacy.assets"),
            make_legacy_full_build_settings_file(),
        )
        .unwrap();
    }

    struct ProbeBuildSettings {
        asset_type: String,
        scenes: Vec<String>,
        version: String,
        guid_words: Option<[u32; 4]>,
        bools: Vec<(String, bool)>,
    }

    fn parse_probe_fixture(section: &str) -> ProbeBuildSettings {
        let fixture = include_str!("buildsettings_probe_fixture.txt");
        let mut in_section = false;
        let mut settings = ProbeBuildSettings {
            asset_type: String::new(),
            scenes: Vec::new(),
            version: String::new(),
            guid_words: None,
            bools: Vec::new(),
        };
        for line in fixture.lines() {
            if line.starts_with("## ") {
                in_section = line == format!("## {section}");
                continue;
            }
            if !in_section || line.starts_with('#') || line.is_empty() {
                continue;
            }
            let (kind, rest) = line.split_once('|').unwrap();
            match kind {
                "ASSET" => {
                    let mut parts = rest.split('|');
                    parts.next();
                    parts.next();
                    settings.asset_type = parts
                        .next()
                        .unwrap()
                        .strip_prefix("type=")
                        .unwrap()
                        .to_owned();
                }
                "SCENE" => settings.scenes.push(rest.to_owned()),
                "VERSION" => settings.version = rest.to_owned(),
                "GUID" => {
                    if rest != "ABSENT" {
                        let words: Vec<u32> =
                            rest.split(',').map(|word| word.parse().unwrap()).collect();
                        settings.guid_words = Some([words[0], words[1], words[2], words[3]]);
                    }
                }
                "BOOL" => {
                    let (name, value) = rest.split_once('|').unwrap();
                    settings.bools.push((
                        name.to_owned(),
                        match value {
                            "True" => true,
                            "False" => false,
                            other => panic!("unexpected bool row {other}"),
                        },
                    ));
                }
                _ => {}
            }
        }
        assert!(!settings.asset_type.is_empty(), "{section}");
        settings
    }

    fn decoded_root(bytes: &[u8]) -> SerializedValue {
        let file = SerializedFile::parse(bytes).unwrap();
        let object = file.decode_object_payload(0).unwrap();
        assert!(object.trailing_bytes.is_empty());
        object.root
    }

    fn lookup_string(root: &SerializedValue, field: &str) -> String {
        match root.unique_field_in_base_chain(field).unwrap().unwrap() {
            SerializedValue::StringBytes(bytes) => String::from_utf8(bytes.clone()).unwrap(),
            other => panic!("{field} is not a string: {other:?}"),
        }
    }

    /// Every row of the reference-materialized fixture is replayed through the
    /// production decoder: scene paths, version string, GUID words, and flags
    /// must equal the shipped object-model values, and the reference asset
    /// type must be a real generated class (never `UnreadableObject`).
    #[test]
    fn full_layout_build_settings_match_the_reference_materialization() {
        for (section, bytes) in [
            ("modern.assets", make_modern_full_build_settings_file()),
            ("legacy.assets", make_legacy_full_build_settings_file()),
        ] {
            let expected = parse_probe_fixture(section);
            assert!(
                expected.asset_type.starts_with("BuildSettings_"),
                "{section}: {}",
                expected.asset_type
            );
            let file = SerializedFile::parse(&bytes).unwrap();
            let paths = decode_build_settings_scene_paths(
                &file,
                0,
                BuildSettingsSceneLimits::default(),
                &ObjectDecodeLimits::default(),
            )
            .unwrap();
            assert_eq!(paths, expected.scenes, "{section}");

            let root = decoded_root(&bytes);
            assert_eq!(lookup_string(&root, "m_Version"), expected.version);
            match expected.guid_words {
                Some(words) => {
                    let guid = root
                        .unique_field_in_base_chain("m_BuildGUID")
                        .unwrap()
                        .unwrap();
                    let fields = guid.struct_fields().unwrap();
                    let mut actual = [0u32; 4];
                    for (index, word) in actual.iter_mut().enumerate() {
                        let name = format!("data[{index}]");
                        let field = fields.iter().find(|field| field.name == name).unwrap();
                        match field.value {
                            SerializedValue::U32(value) => *word = value,
                            ref other => panic!("{name} is not u32: {other:?}"),
                        }
                    }
                    assert_eq!(actual, words, "{section}");
                }
                None => assert!(
                    root.unique_field_in_base_chain("m_BuildGUID")
                        .unwrap()
                        .is_none(),
                    "{section}"
                ),
            }
            for (name, value) in &expected.bools {
                // The shipped C# names are PascalCase while the serialized
                // tree names follow the TPK lowercase-first spelling.
                let mut tree_name = name.clone();
                tree_name.get_mut(0..1).unwrap().make_ascii_lowercase();
                match root
                    .unique_field_in_base_chain(&tree_name)
                    .unwrap()
                    .unwrap()
                {
                    SerializedValue::Bool(actual) => {
                        assert_eq!(actual, value, "{section} {name}")
                    }
                    other => panic!("{section} {name} is not a bool: {other:?}"),
                }
            }
        }
    }

    #[test]
    fn decodes_assetbundle_scene_fields_from_a_complete_serialized_file() {
        let bytes = make_asset_bundle_file();
        let file = SerializedFile::parse(&bytes).unwrap();
        assert_eq!(file.metadata.objects[0].class_id, Some(142));
        let object = file.decode_object_payload(0).unwrap();
        assert_eq!(object.type_name, "AssetBundle");
        assert!(object.trailing_bytes.is_empty());
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &object.root,
                SceneBundleMappingLimits::default()
            )
            .unwrap(),
            StreamedSceneBundleMetadata {
                is_streamed_scene_asset_bundle: Some(true),
                scene_hashes: Some(vec![(
                    "Assets/Scenes/Main.unity".to_owned(),
                    "CAB-Scene".to_owned()
                )]),
                container_paths: Some(vec!["Assets/Scenes/Main.unity".to_owned()]),
            }
        );
        assert_eq!(
            extract_asset_bundle_container_references(
                &object.root,
                SceneBundleMappingLimits::default(),
            )
            .unwrap(),
            [AssetBundleContainerReference {
                path: "Assets/Scenes/Main.unity".to_owned(),
                file_id: 0,
                path_id: 0,
            }]
        );

        let container = object.root.unique_field("m_Container").unwrap().unwrap();
        let SerializedValue::Array(entries) = container else {
            panic!("AssetBundle m_Container TypeTree did not decode as an array");
        };
        let pair = entries[0].struct_fields().unwrap();
        let asset_info = pair
            .iter()
            .find(|field| field.name == "second")
            .unwrap()
            .value
            .struct_fields()
            .unwrap();
        assert_eq!(
            asset_info
                .iter()
                .find(|field| field.name == "asset")
                .unwrap()
                .value,
            SerializedValue::PPtr {
                file_id: 0,
                path_id: 0
            }
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
