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
mod tests {
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
    fn decodes_assetbundle_scene_fields_from_a_complete_serialized_file() {
        let bytes = make_asset_bundle_file();
        let file = SerializedFile::parse(&bytes).unwrap();
        assert_eq!(file.metadata.objects[0].class_id, Some(142));
        let object = file.decode_object_payload(0).unwrap();
        assert_eq!(object.type_name, "AssetBundle");
        assert_eq!(object.trailing_bytes, []);
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
