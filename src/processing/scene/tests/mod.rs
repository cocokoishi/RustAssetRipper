mod graph_construction;
mod hierarchy_audit;
mod hierarchy_contract;
mod prefab_instance;
mod reference_resolution;
mod root_discovery;

use super::reference_resolution::{
    reference_field_path_matches, reference_field_path_matches_array_element,
    scene_target_from_multi_file_resolution,
};
use super::*;
use crate::io::binary::Endian;
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::format_version::FormatVersion;
use crate::serialized_file::metadata::{FileIdentifier, ObjectInfo, SerializedFileMetadata};
use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
use crate::serialized_file::object::{SerializedFieldValue as Field, SerializedValue as Value};

fn metadata(path_ids: &[(i64, i16)]) -> SerializedFileMetadata {
    SerializedFileMetadata {
        prefix: SerializedFileMetadataPrefix {
            metadata_offset: 0,
            metadata_size: 0,
            body_endian: Endian::Little,
            unity_version_text: None,
            target_platform: None,
            has_type_tree: false,
            consumed_bytes: 0,
        },
        types: Vec::new(),
        long_file_id: None,
        objects: path_ids
            .iter()
            .map(|(path_id, class_id)| ObjectInfo {
                path_id: *path_id,
                byte_start: 0,
                byte_size: 0,
                serialized_type_index: None,
                raw_type_id: i32::from(*class_id),
                class_id: Some(*class_id),
                script_type_index: None,
                is_destroyed: None,
                is_stripped: None,
                resolved_type_index: None,
            })
            .collect(),
        script_types: Vec::new(),
        externals: vec![FileIdentifier {
            asset_path: Some("shared.assets".to_owned()),
            guid: None,
            file_type: None,
            path_name_origin: "shared.assets".to_owned(),
        }],
        reference_types: Vec::new(),
        user_information: None,
        trailing_metadata: Vec::new(),
        consumed_bytes: 0,
    }
}

fn field(name: &str, value: Value) -> Field {
    Field {
        name: name.to_owned(),
        value,
    }
}

fn pptr(file_id: i32, path_id: i64) -> Value {
    Value::PPtr { file_id, path_id }
}

fn vector2(x: f32, y: f32) -> Value {
    Value::Struct(vec![field("x", Value::F32(x)), field("y", Value::F32(y))])
}

fn vector3(x: f32, y: f32, z: f32) -> Value {
    Value::Struct(vec![
        field("x", Value::F32(x)),
        field("y", Value::F32(y)),
        field("z", Value::F32(z)),
    ])
}

fn quaternion(x: f32, y: f32, z: f32, w: f32) -> Value {
    Value::Struct(vec![
        field("x", Value::F32(x)),
        field("y", Value::F32(y)),
        field("z", Value::F32(z)),
        field("w", Value::F32(w)),
    ])
}

fn transform_fields(include_optional_version_fields: bool) -> Vec<Field> {
    transform_fields_with_links(include_optional_version_fields, pptr(0, 0), Vec::new())
}

fn transform_fields_with_links(
    include_optional_version_fields: bool,
    father: Value,
    children: Vec<Value>,
) -> Vec<Field> {
    let mut fields = vec![
        field("m_GameObject", pptr(0, 10)),
        field("m_LocalRotation", quaternion(0.1, 0.2, 0.3, 0.9)),
        field("m_LocalPosition", vector3(1.25, -2.0, 3.5)),
        field("m_LocalScale", vector3(2.0, 3.0, 4.0)),
        field("m_Children", Value::Array(children)),
        field("m_Father", father),
    ];
    if include_optional_version_fields {
        fields.push(field("m_RootOrder", Value::I32(7)));
        fields.push(field("m_LocalEulerAnglesHint", vector3(10.0, 20.0, 30.0)));
    }
    fields
}

fn game_object_fields(name: &str, components: Vec<Value>) -> Vec<Field> {
    vec![
        field("m_Name", Value::StringBytes(name.as_bytes().to_vec())),
        field("m_IsActive", Value::Bool(true)),
        field(
            "m_Component",
            Value::Array(
                components
                    .into_iter()
                    .map(|component| Value::Struct(vec![field("component", component)]))
                    .collect(),
            ),
        ),
    ]
}

fn put_u16_le(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_i32_le(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_i64_le(output: &mut Vec<u8>, value: i64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_f32_le(output: &mut Vec<u8>, value: f32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u32_le(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
    let offset = u32::try_from(buffer.len()).unwrap();
    buffer.extend_from_slice(value.as_bytes());
    buffer.push(0);
    offset
}

#[allow(clippy::too_many_arguments)]
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

fn make_game_object_file(
    name: &str,
    path_id: i32,
    component: Option<(i32, i32)>,
    external_path: Option<&str>,
) -> Vec<u8> {
    match component {
        Some(component) => {
            make_game_object_file_with_components(name, path_id, &[component], external_path)
        }
        None => make_game_object_file_with_components(name, path_id, &[], external_path),
    }
}

fn make_game_object_file_with_components(
    name: &str,
    path_id: i32,
    components: &[(i32, i32)],
    external_path: Option<&str>,
) -> Vec<u8> {
    let mut strings = Vec::new();
    let root_type = push_tree_string(&mut strings, "GameObject");
    let empty = push_tree_string(&mut strings, "");
    let name_field = push_tree_string(&mut strings, "m_Name");
    let string_type = push_tree_string(&mut strings, "string");
    let array = push_tree_string(&mut strings, "Array");
    let size = push_tree_string(&mut strings, "size");
    let int_type = push_tree_string(&mut strings, "int");
    let data = push_tree_string(&mut strings, "data");
    let char_type = push_tree_string(&mut strings, "char");
    let active_field = push_tree_string(&mut strings, "m_IsActive");
    let bool_type = push_tree_string(&mut strings, "bool");
    let component_field = push_tree_string(&mut strings, "m_Component");
    let vector_type = push_tree_string(&mut strings, "vector");
    let pptr_type = push_tree_string(&mut strings, "PPtr<Component>");
    let file_id_field = push_tree_string(&mut strings, "m_FileID");
    let path_id_field = push_tree_string(&mut strings, "m_PathID");
    let int32_type = push_tree_string(&mut strings, "SInt32");

    let mut metadata = Vec::new();
    metadata.extend_from_slice(b"2019.4.0f1\0");
    put_u32_le(&mut metadata, 0);
    put_i32_le(&mut metadata, 1);
    put_i32_le(&mut metadata, 1);
    put_i32_le(&mut metadata, 12);
    put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
    for (level, type_offset, name_offset, byte_size, index) in [
        (0, root_type, empty, -1, 0),
        (1, string_type, name_field, -1, 1),
        (2, array, array, -1, 2),
        (3, int_type, size, 4, 3),
        (3, char_type, data, 1, 4),
        (1, bool_type, active_field, 1, 5),
        (1, vector_type, component_field, -1, 6),
        (2, array, array, -1, 7),
        (3, int_type, size, 4, 8),
        (3, pptr_type, data, -1, 9),
        (4, int32_type, file_id_field, 4, 10),
        (4, int32_type, path_id_field, 4, 11),
    ] {
        put_tree_node(
            &mut metadata,
            level,
            type_offset,
            name_offset,
            byte_size,
            index,
        );
    }
    metadata.extend_from_slice(&strings);
    put_u32_le(&mut metadata, 0);
    put_i32_le(&mut metadata, 1);
    put_i32_le(&mut metadata, path_id);
    put_u32_le(&mut metadata, 0);

    let mut payload = Vec::new();
    put_i32_le(&mut payload, i32::try_from(name.len()).unwrap());
    payload.extend_from_slice(name.as_bytes());
    let name_padding = (4 - name.len() % 4) % 4;
    payload.resize(payload.len() + name_padding, 0);
    payload.push(1);
    put_i32_le(&mut payload, i32::try_from(components.len()).unwrap());
    for &(file_id, target_path_id) in components {
        put_i32_le(&mut payload, file_id);
        put_i32_le(&mut payload, target_path_id);
    }
    put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
    put_i32_le(&mut metadata, 1);
    put_u16_le(&mut metadata, 1);
    put_u16_le(&mut metadata, 0);
    put_i32_le(&mut metadata, i32::from(external_path.is_some()));
    if let Some(external_path) = external_path {
        metadata.push(0); // empty asset path
        metadata.extend_from_slice(&[0; 16]); // GUID
        put_i32_le(&mut metadata, 0); // asset type
        metadata.extend_from_slice(external_path.as_bytes());
        metadata.push(0);
    }
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

fn make_prefab_instance_file(
    prefab_path_id: i32,
    root_file_id: i32,
    root_path_id: i64,
    external_path: Option<&str>,
) -> Vec<u8> {
    let mut strings = Vec::new();
    let root_type = push_tree_string(&mut strings, "PrefabInstance");
    let empty = push_tree_string(&mut strings, "");
    let root_game_object = push_tree_string(&mut strings, "m_RootGameObject");
    let pptr_game_object = push_tree_string(&mut strings, "PPtr<GameObject>");
    let file_id_field = push_tree_string(&mut strings, "m_FileID");
    let file_id_type = push_tree_string(&mut strings, "SInt32");
    let path_id_field = push_tree_string(&mut strings, "m_PathID");
    let path_id_type = push_tree_string(&mut strings, "SInt64");

    let mut metadata = Vec::new();
    metadata.extend_from_slice(b"2019.4.0f1\0");
    put_u32_le(&mut metadata, 0);
    put_i32_le(&mut metadata, 1);
    put_i32_le(&mut metadata, 1001);
    put_i32_le(&mut metadata, 4);
    put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
    for (level, type_offset, name_offset, byte_size, index) in [
        (0, root_type, empty, -1, 0),
        (1, pptr_game_object, root_game_object, -1, 1),
        (2, file_id_type, file_id_field, 4, 2),
        (2, path_id_type, path_id_field, 8, 3),
    ] {
        put_tree_node(
            &mut metadata,
            level,
            type_offset,
            name_offset,
            byte_size,
            index,
        );
    }
    metadata.extend_from_slice(&strings);
    put_u32_le(&mut metadata, 0);
    put_i32_le(&mut metadata, 1);
    put_i32_le(&mut metadata, prefab_path_id);
    put_u32_le(&mut metadata, 0);

    let mut payload = Vec::new();
    put_i32_le(&mut payload, root_file_id);
    put_i64_le(&mut payload, root_path_id);
    put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
    put_i32_le(&mut metadata, 1001);
    put_u16_le(&mut metadata, 1001);
    put_u16_le(&mut metadata, 0);
    put_i32_le(&mut metadata, i32::from(external_path.is_some()));
    if let Some(external_path) = external_path {
        metadata.push(0);
        metadata.extend_from_slice(&[0; 16]);
        put_i32_le(&mut metadata, 0);
        metadata.extend_from_slice(external_path.as_bytes());
        metadata.push(0);
    }
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

fn make_transform_file() -> Vec<u8> {
    make_transform_file_with_links(42, 0, 0)
}

fn make_transform_file_with_links(
    transform_path_id: i32,
    father_file_id: i32,
    father_path_id: i64,
) -> Vec<u8> {
    make_transform_file_with_hierarchy(
        transform_path_id,
        father_file_id,
        father_path_id,
        &[],
        &[],
        false,
    )
}

fn make_transform_file_with_hierarchy(
    transform_path_id: i32,
    father_file_id: i32,
    father_path_id: i64,
    children: &[(i32, i64)],
    external_names: &[&str],
    include_unparsed_raw_leaf: bool,
) -> Vec<u8> {
    let mut strings = Vec::new();
    let root_type = push_tree_string(&mut strings, "Transform");
    let empty = push_tree_string(&mut strings, "");
    let pptr_game_object = push_tree_string(&mut strings, "PPtr<GameObject>");
    let pptr_transform = push_tree_string(&mut strings, "PPtr<Transform>");
    let quaternion_type = push_tree_string(&mut strings, "Quaternion");
    let vector3_type = push_tree_string(&mut strings, "Vector3");
    let float_type = push_tree_string(&mut strings, "float");
    let int_type = push_tree_string(&mut strings, "int");
    let vector_type = push_tree_string(&mut strings, "vector");
    let array_type = push_tree_string(&mut strings, "Array");
    let file_id_field = push_tree_string(&mut strings, "m_FileID");
    let file_id_type = push_tree_string(&mut strings, "SInt32");
    let path_id_field = push_tree_string(&mut strings, "m_PathID");
    let path_id_type = push_tree_string(&mut strings, "SInt64");
    let game_object_field = push_tree_string(&mut strings, "m_GameObject");
    let local_rotation_field = push_tree_string(&mut strings, "m_LocalRotation");
    let local_position_field = push_tree_string(&mut strings, "m_LocalPosition");
    let local_scale_field = push_tree_string(&mut strings, "m_LocalScale");
    let children_field = push_tree_string(&mut strings, "m_Children");
    let father_field = push_tree_string(&mut strings, "m_Father");
    let root_order_field = push_tree_string(&mut strings, "m_RootOrder");
    let euler_hint_field = push_tree_string(&mut strings, "m_LocalEulerAnglesHint");
    let unknown_type = push_tree_string(&mut strings, "UnknownSerializedType");
    let unknown_field = push_tree_string(&mut strings, "m_UnparsedTail");
    let x_field = push_tree_string(&mut strings, "x");
    let y_field = push_tree_string(&mut strings, "y");
    let z_field = push_tree_string(&mut strings, "z");
    let w_field = push_tree_string(&mut strings, "w");
    let size_field = push_tree_string(&mut strings, "size");
    let data_field = push_tree_string(&mut strings, "data");

    let mut nodes = Vec::new();
    let mut add_node = |level, type_offset, name_offset, byte_size| {
        let index = i32::try_from(nodes.len()).unwrap();
        nodes.push((level, type_offset, name_offset, byte_size, index));
    };
    add_node(0, root_type, empty, -1);
    add_node(1, pptr_game_object, game_object_field, -1);
    add_node(2, file_id_type, file_id_field, 4);
    add_node(2, path_id_type, path_id_field, 8);
    add_node(1, quaternion_type, local_rotation_field, 16);
    add_node(2, float_type, x_field, 4);
    add_node(2, float_type, y_field, 4);
    add_node(2, float_type, z_field, 4);
    add_node(2, float_type, w_field, 4);
    add_node(1, vector3_type, local_position_field, 12);
    add_node(2, float_type, x_field, 4);
    add_node(2, float_type, y_field, 4);
    add_node(2, float_type, z_field, 4);
    add_node(1, vector3_type, local_scale_field, 12);
    add_node(2, float_type, x_field, 4);
    add_node(2, float_type, y_field, 4);
    add_node(2, float_type, z_field, 4);
    add_node(1, vector_type, children_field, -1);
    add_node(2, array_type, array_type, -1);
    add_node(3, int_type, size_field, 4);
    add_node(3, pptr_transform, data_field, -1);
    add_node(4, file_id_type, file_id_field, 4);
    add_node(4, path_id_type, path_id_field, 8);
    add_node(1, pptr_transform, father_field, -1);
    add_node(2, file_id_type, file_id_field, 4);
    add_node(2, path_id_type, path_id_field, 8);
    add_node(1, int_type, root_order_field, 4);
    add_node(1, vector3_type, euler_hint_field, 12);
    add_node(2, float_type, x_field, 4);
    add_node(2, float_type, y_field, 4);
    add_node(2, float_type, z_field, 4);
    if include_unparsed_raw_leaf {
        add_node(1, unknown_type, unknown_field, 1);
    }

    let mut metadata = Vec::new();
    metadata.extend_from_slice(b"2019.4.0f1\0");
    put_u32_le(&mut metadata, 0);
    put_i32_le(&mut metadata, 1);
    put_i32_le(&mut metadata, 4);
    put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
    put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
    for (level, type_offset, name_offset, byte_size, index) in nodes {
        put_tree_node(
            &mut metadata,
            level,
            type_offset,
            name_offset,
            byte_size,
            index,
        );
    }
    metadata.extend_from_slice(&strings);
    put_u32_le(&mut metadata, 0);
    put_i32_le(&mut metadata, 1);
    put_i32_le(&mut metadata, transform_path_id);
    put_u32_le(&mut metadata, 0);

    let mut payload = Vec::new();
    put_i32_le(&mut payload, 0);
    put_i64_le(&mut payload, 0);
    for value in [0.1, 0.2, 0.3, 0.9] {
        put_f32_le(&mut payload, value);
    }
    for value in [1.25, -2.0, 3.5, 2.0, 3.0, 4.0] {
        put_f32_le(&mut payload, value);
    }
    put_i32_le(&mut payload, i32::try_from(children.len()).unwrap());
    for &(file_id, path_id) in children {
        put_i32_le(&mut payload, file_id);
        put_i64_le(&mut payload, path_id);
    }
    put_i32_le(&mut payload, father_file_id); // m_Father file ID
    put_i64_le(&mut payload, father_path_id); // m_Father PathID
    put_i32_le(&mut payload, 7); // m_RootOrder
    for value in [10.0, 20.0, 30.0] {
        put_f32_le(&mut payload, value);
    }
    if include_unparsed_raw_leaf {
        payload.push(0x7f);
    }

    put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
    put_i32_le(&mut metadata, 4);
    put_u16_le(&mut metadata, 4);
    put_u16_le(&mut metadata, 0);
    put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
    for external_name in external_names {
        metadata.push(0); // asset path
        metadata.extend_from_slice(&[0; 16]); // GUID
        put_i32_le(&mut metadata, 0); // asset type
        metadata.extend_from_slice(external_name.as_bytes());
        metadata.push(0);
    }
    metadata.push(0); // reference type count

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
