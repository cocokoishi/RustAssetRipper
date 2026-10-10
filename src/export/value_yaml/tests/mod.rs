use super::conversion::{
    format_dictionary_float32_key, format_dictionary_float64_key,
    STRIPPED_MONO_BEHAVIOUR_ALLOWED_FIELDS,
};
use super::layout::{TRANSFER_IGNORE_IN_META_FILES, TRANSFER_USING_FLOW_MAPPING_STYLE};
use super::*;
use std::collections::HashMap;

use crate::export::pointer_map::{
    ExportPointerGuid, ProjectPointerResolution, UnityMetaPointer, UnityProjectAssetType,
    UnresolvedProjectPointer,
};
use crate::export::scene_collection::{ExportAssetCollectionIdentity, ExportAssetIdentity};
use crate::export::unity_yaml::{
    UnityYamlDocument, UnityYamlNode, UnityYamlNodeKind, UnityYamlScalar, UnityYamlSequenceStyle,
};
use crate::processing::scene_naming::SceneUnityVersion;
use crate::serialized_file::metadata::{TypeTree, TypeTreeNode};
use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
fn source() -> ExportAssetIdentity {
    source_with_class_id(1)
}

fn source_with_class_id(class_id: i32) -> ExportAssetIdentity {
    ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 2 },
        object_index: 7,
        path_id: 90,
        class_id,
    }
}

fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
    SerializedFieldValue {
        name: name.to_owned(),
        value,
    }
}

fn type_tree_node(level: u8, type_name: &str, name: &str, meta_flags: u32) -> TypeTreeNode {
    TypeTreeNode {
        level,
        version: 1,
        type_flags: 0,
        type_string_offset: None,
        name_string_offset: None,
        type_name: Some(type_name.to_owned()),
        name: Some(name.to_owned()),
        byte_size: -1,
        index: 0,
        meta_flags,
        reference_type_hash: None,
    }
}

fn hash128_type_tree(serialized_version: i32) -> TypeTree {
    let mut nodes = vec![
        type_tree_node(0, "GameObject", "Base", 0),
        type_tree_node(1, "Hash128", "m_Hash", 0),
    ];
    nodes[1].version = serialized_version;
    for index in 0..16 {
        let name = if index < 10 {
            format!("m_Bytes__{index}")
        } else {
            format!("m_Bytes_{index}")
        };
        nodes.push(type_tree_node(2, "UInt8", &name, 0));
    }
    TypeTree {
        nodes,
        string_buffer: Vec::new(),
    }
}

fn hash128_value() -> SerializedValue {
    let fields = (0..16)
        .map(|index| {
            let name = if index < 10 {
                format!("m_Bytes__{index}")
            } else {
                format!("m_Bytes_{index}")
            };
            field(&name, SerializedValue::U8(index as u8))
        })
        .collect();
    SerializedValue::Struct(vec![field("m_Hash", SerializedValue::Struct(fields))])
}

fn guid_type_tree(array_element: bool) -> TypeTree {
    let mut nodes = if array_element {
        vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "vector", "m_Guids", 0),
            type_tree_node(2, "Array", "Array", 0),
            type_tree_node(3, "SInt32", "size", 0),
            type_tree_node(3, "GUID", "data", 0),
        ]
    } else {
        vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "GUID", "m_Guid", 0),
        ]
    };
    let field_level = if array_element { 4 } else { 2 };
    for index in 0..4 {
        nodes.push(type_tree_node(
            field_level,
            "UInt32",
            &format!("m_Data_{index}_"),
            0,
        ));
    }
    TypeTree {
        nodes,
        string_buffer: Vec::new(),
    }
}

fn property_name_type_tree() -> TypeTree {
    TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            // Raw serialized TypeTrees encode PropertyName as a string wrapper with one
            // integer or Utf8String child; the baseline generator names it PropertyName.
            type_tree_node(1, "string", "m_IntegerId", 0),
            type_tree_node(2, "SInt32", "Id_Int32", 0),
            type_tree_node(1, "string", "m_StringId", 0),
            type_tree_node(2, "Utf8String", "Id_Utf8String", 0),
            type_tree_node(3, "Array", "Array", 0),
            type_tree_node(4, "SInt32", "size", 0),
            type_tree_node(4, "char", "data", 0),
        ],
        string_buffer: Vec::new(),
    }
}

fn scene_object_identifier_type_tree(array_element: bool) -> TypeTree {
    let mut nodes = if array_element {
        vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "vector", "m_Identifiers", 0),
            type_tree_node(2, "Array", "Array", 0),
            type_tree_node(3, "SInt32", "size", 0),
            type_tree_node(3, "SceneObjectIdentifier", "data", 0),
        ]
    } else {
        vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "SceneObjectIdentifier", "m_Identifier", 0),
        ]
    };
    let field_level = if array_element { 4 } else { 2 };
    nodes.push(type_tree_node(field_level, "SInt64", "TargetObject", 0));
    nodes.push(type_tree_node(field_level, "SInt64", "TargetPrefab", 0));
    TypeTree {
        nodes,
        string_buffer: Vec::new(),
    }
}

fn scene_object_identifier_value(target_object: i64, target_prefab: i64) -> SerializedValue {
    SerializedValue::Struct(vec![field(
        "m_Identifier",
        SerializedValue::Struct(vec![
            field("TargetObject", SerializedValue::I64(target_object)),
            field("TargetPrefab", SerializedValue::I64(target_prefab)),
        ]),
    )])
}

fn guid_value_struct() -> SerializedValue {
    let words = [0x0102_0304, 0x1112_1314, 0x2122_2324, 0x3132_3334];
    SerializedValue::Struct(
        words
            .into_iter()
            .enumerate()
            .map(|(index, word)| field(&format!("m_Data_{index}_"), SerializedValue::U32(word)))
            .collect(),
    )
}

fn guid_value(array_element: bool) -> SerializedValue {
    if array_element {
        SerializedValue::Struct(vec![field(
            "m_Guids",
            SerializedValue::Array(vec![guid_value_struct()]),
        )])
    } else {
        SerializedValue::Struct(vec![field("m_Guid", guid_value_struct())])
    }
}

fn object_field<'a>(document: &'a UnityYamlDocument, field_name: &str) -> &'a UnityYamlNode {
    let UnityYamlNodeKind::Mapping {
        entries: root_entries,
        ..
    } = &document.root.kind
    else {
        unreachable!("object root is a mapping")
    };
    let (_, body) = root_entries.first().expect("object body exists");
    let UnityYamlNodeKind::Mapping { entries, .. } = &body.kind else {
        unreachable!("object body is a mapping")
    };
    entries
        .iter()
        .find_map(|(key, value)| match &key.kind {
            UnityYamlNodeKind::Scalar(
                UnityYamlScalar::PlainString(name) | UnityYamlScalar::String(name),
            ) if name == field_name => Some(value),
            _ => None,
        })
        .expect("object field exists")
}

fn mapping_field<'a>(mapping: &'a UnityYamlNode, field_name: &str) -> &'a UnityYamlNode {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mapping.kind else {
        panic!("expected mapping for field {field_name}");
    };
    entries
        .iter()
        .find_map(|(key, value)| match &key.kind {
            UnityYamlNodeKind::Scalar(
                UnityYamlScalar::PlainString(name) | UnityYamlScalar::String(name),
            ) if name == field_name => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing mapping field {field_name}"))
}

fn version() -> SceneUnityVersion {
    SceneUnityVersion::parse("2022.3.1f1").unwrap()
}

fn convert<F>(
    root: &SerializedValue,
    hints: &HashMap<String, UnityYamlEmptyArrayKind>,
    resolver: F,
) -> Result<UnityYamlDocument, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    let hints = UnityYamlObjectConversionHints {
        empty_array_kinds: hints.clone(),
        ..UnityYamlObjectConversionHints::default()
    };
    convert_with_object_hints(root, &hints, resolver)
}

fn convert_with_object_hints<F>(
    root: &SerializedValue,
    hints: &UnityYamlObjectConversionHints,
    resolver: F,
) -> Result<UnityYamlDocument, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    convert_with_object_field_mode(
        root,
        hints,
        UnityYamlObjectFieldMode::ProjectAsset,
        resolver,
    )
}

fn convert_with_object_field_mode<F>(
    root: &SerializedValue,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    resolver: F,
) -> Result<UnityYamlDocument, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    convert_object_with_options(source(), root, hints, field_mode, false, resolver)
}

fn convert_object_with_options<F>(
    source: ExportAssetIdentity,
    root: &SerializedValue,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    stripped: bool,
    resolver: F,
) -> Result<UnityYamlDocument, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    let class_name = match source.class_id {
        74 => "AnimationClip",
        114 => "MonoBehaviour",
        _ => "GameObject",
    };
    serialized_object_value_to_unity_yaml_document(
        source,
        class_name,
        6,
        1_000_001,
        stripped,
        root,
        hints,
        field_mode,
        version(),
        None,
        UnityYamlObjectConversionLimits::default(),
        resolver,
    )
}

fn null_pptr() -> SerializedValue {
    SerializedValue::PPtr {
        file_id: 0,
        path_id: 0,
    }
}

mod field_aliases;
mod value_conversion;
