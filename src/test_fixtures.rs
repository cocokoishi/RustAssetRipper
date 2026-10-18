// Shared synthetic SerializedFile fixtures for the library and binary test targets.

fn write_type_tree_with_meta_flags(
    output: &mut Vec<u8>,
    class_id: i32,
    nodes: &[(u8, i32, &str, &str, i32, u32)],
) {
    let mut string_buffer = Vec::new();
    let mut offsets = Vec::new();
    for (_, _, type_name, name, _, _) in nodes {
        let type_offset = push_type_tree_string(&mut string_buffer, type_name);
        let name_offset = push_type_tree_string(&mut string_buffer, name);
        offsets.push((type_offset, name_offset));
    }
    output.extend_from_slice(&class_id.to_le_bytes());
    output.extend_from_slice(&(nodes.len() as i32).to_le_bytes());
    output.extend_from_slice(&(string_buffer.len() as i32).to_le_bytes());
    for ((level, version, _, _, byte_size, meta_flags), (type_offset, name_offset)) in
        nodes.iter().zip(offsets)
    {
        output.extend_from_slice(&(*version as u16).to_le_bytes());
        output.push(*level);
        output.push(0);
        output.extend_from_slice(&type_offset.to_le_bytes());
        output.extend_from_slice(&name_offset.to_le_bytes());
        output.extend_from_slice(&byte_size.to_le_bytes());
        output.extend_from_slice(&0i32.to_le_bytes());
        output.extend_from_slice(&meta_flags.to_le_bytes());
    }
    output.extend_from_slice(&string_buffer);
}

fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
    let offset = u32::try_from(buffer.len()).unwrap();
    buffer.extend_from_slice(value.as_bytes());
    buffer.push(0);
    offset
}

fn write_object_record(
    output: &mut Vec<u8>,
    path_id: i64,
    data_start: usize,
    data_size: usize,
    raw_type_id: i32,
    class_id: i16,
) {
    output.extend_from_slice(&i32::try_from(path_id).unwrap().to_le_bytes());
    output.extend_from_slice(&u32::try_from(data_start).unwrap().to_le_bytes());
    output.extend_from_slice(&i32::try_from(data_size).unwrap().to_le_bytes());
    output.extend_from_slice(&raw_type_id.to_le_bytes());
    output.extend_from_slice(&class_id.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
}

pub(crate) fn make_existing_prefab_instance_file() -> Vec<u8> {
    const GAME_OBJECT_PATH_ID: i64 = 10;
    const TRANSFORM_PATH_ID: i64 = 11;
    const PREFAB_INSTANCE_PATH_ID: i64 = 12;
    let game_object_tree = [
        (0, 6, "GameObject", "Base", -1, 32768),
        (1, 1, "unsigned int", "m_ObjectHideFlags", 4, 1),
        (
            1,
            1,
            "PPtr<EditorExtension>",
            "m_CorrespondingSourceObject",
            12,
            1,
        ),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "PPtr<PrefabInstance>", "m_PrefabInstance", 12, 1),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "PPtr<Prefab>", "m_PrefabAsset", 12, 1),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "vector", "m_Component", -1, 268468289),
        (2, 1, "Array", "Array", -1, 268451905),
        (3, 1, "int", "size", 4, 268435521),
        (3, 1, "ComponentPair", "data", -1, 268435521),
        (4, 1, "PPtr<Component>", "component", 12, 268435521),
        (5, 1, "int", "m_FileID", 4, 268435521),
        (5, 1, "SInt64", "m_PathID", 8, 268435521),
        (1, 1, "unsigned int", "m_Layer", 4, 0),
        (1, 1, "string", "m_Name", -1, 32768),
        (2, 1, "Array", "Array", -1, 16385),
        (3, 1, "int", "size", 4, 1),
        (3, 1, "char", "data", 1, 1),
        (1, 1, "string", "m_TagString", -1, 32768),
        (2, 1, "Array", "Array", -1, 16385),
        (3, 1, "int", "size", 4, 1),
        (3, 1, "char", "data", 1, 1),
        (1, 1, "PPtr<Texture2D>", "m_Icon", 12, 0),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "unsigned int", "m_NavMeshLayer", 4, 1),
        (1, 1, "unsigned int", "m_StaticEditorFlags", 4, 4194304),
        (1, 1, "bool", "m_IsActive", 1, 1),
    ];
    let transform_tree = [
        (0, 1, "Transform", "Base", -1, 32768),
        (1, 1, "unsigned int", "m_ObjectHideFlags", 4, 1),
        (
            1,
            1,
            "PPtr<EditorExtension>",
            "m_CorrespondingSourceObject",
            12,
            1,
        ),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "PPtr<PrefabInstance>", "m_PrefabInstance", 12, 1),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "PPtr<Prefab>", "m_PrefabAsset", 12, 1),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "PPtr<GameObject>", "m_GameObject", 12, 65),
        (2, 1, "int", "m_FileID", 4, 65),
        (2, 1, "SInt64", "m_PathID", 8, 65),
        (1, 1, "Quaternionf", "m_LocalRotation", -1, 2097152),
        (2, 1, "float", "x", 4, 2097152),
        (2, 1, "float", "y", 4, 2097152),
        (2, 1, "float", "z", 4, 2097152),
        (2, 1, "float", "w", 4, 2097152),
        (1, 1, "Vector3f", "m_LocalPosition", -1, 2097152),
        (2, 1, "float", "x", 4, 2097152),
        (2, 1, "float", "y", 4, 2097152),
        (2, 1, "float", "z", 4, 2097152),
        (1, 1, "Vector3f", "m_LocalScale", -1, 2097152),
        (2, 1, "float", "x", 4, 2097152),
        (2, 1, "float", "y", 4, 2097152),
        (2, 1, "float", "z", 4, 2097152),
        (1, 1, "vector", "m_Children", -1, 32833),
        (2, 1, "Array", "Array", -1, 16449),
        (3, 1, "int", "size", 4, 65),
        (3, 1, "PPtr<Transform>", "data", 12, 65),
        (4, 1, "int", "m_FileID", 4, 65),
        (4, 1, "SInt64", "m_PathID", 8, 65),
        (1, 1, "PPtr<Transform>", "m_Father", 12, 1),
        (2, 1, "int", "m_FileID", 4, 1),
        (2, 1, "SInt64", "m_PathID", 8, 1),
        (1, 1, "int", "m_RootOrder", 4, 8388609),
        (1, 1, "Vector3f", "m_LocalEulerAnglesHint", -1, 10485761),
        (2, 1, "float", "x", 4, 10485761),
        (2, 1, "float", "y", 4, 10485761),
        (2, 1, "float", "z", 4, 10485761),
    ];
    let prefab_instance_tree = [
        (0, 2, "PrefabInstance", "Base", -1, 32768),
        (1, 1, "unsigned int", "m_ObjectHideFlags", 4, 1),
        (1, 1, "PPtr<GameObject>", "m_RootGameObject", 12, 1),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "PrefabModification", "m_Modification", -1, 32769),
        (2, 1, "PPtr<Transform>", "m_TransformParent", 12, 1),
        (3, 1, "int", "m_FileID", 4, 8388609),
        (3, 1, "SInt64", "m_PathID", 8, 8388609),
        (2, 1, "vector", "m_Modifications", -1, 32769),
        (3, 1, "Array", "Array", -1, 49153),
        (4, 1, "int", "size", 4, 1),
        (4, 1, "PropertyModification", "data", -1, 32769),
        (5, 1, "PPtr<Object>", "target", 12, 1),
        (6, 1, "int", "m_FileID", 4, 8388609),
        (6, 1, "SInt64", "m_PathID", 8, 8388609),
        (5, 1, "string", "propertyPath", -1, 32769),
        (6, 1, "Array", "Array", -1, 16385),
        (7, 1, "int", "size", 4, 1),
        (7, 1, "char", "data", 1, 1),
        (5, 1, "string", "value", -1, 32769),
        (6, 1, "Array", "Array", -1, 16385),
        (7, 1, "int", "size", 4, 1),
        (7, 1, "char", "data", 1, 1),
        (5, 1, "PPtr<Object>", "objectReference", 12, 1),
        (6, 1, "int", "m_FileID", 4, 8388609),
        (6, 1, "SInt64", "m_PathID", 8, 8388609),
        (2, 1, "vector", "m_RemovedComponents", -1, 32769),
        (3, 1, "Array", "Array", -1, 16385),
        (4, 1, "int", "size", 4, 1),
        (4, 1, "PPtr<Component>", "data", 12, 1),
        (5, 1, "int", "m_FileID", 4, 8388609),
        (5, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "PPtr<Prefab>", "m_SourcePrefab", 12, 1),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
    ];

    let mut metadata = b"2019.4.40f1\0".to_vec();
    metadata.extend_from_slice(&(u32::MAX - 1).to_le_bytes()); // BuildTarget.NoTarget
    metadata.extend_from_slice(&3_i32.to_le_bytes()); // type count
    write_type_tree_with_meta_flags(&mut metadata, 1, &game_object_tree);
    write_type_tree_with_meta_flags(&mut metadata, 4, &transform_tree);
    write_type_tree_with_meta_flags(&mut metadata, 1001, &prefab_instance_tree);
    metadata.extend_from_slice(&0_u32.to_le_bytes()); // generation-10 long FileID flag
    metadata.extend_from_slice(&3_i32.to_le_bytes()); // object count

    let mut payload = Vec::new();
    let game_object_start = payload.len();
    payload.extend_from_slice(&0_u32.to_le_bytes()); // object hide flags
    for _ in 0..3 {
        payload.extend_from_slice(&0_i32.to_le_bytes());
        payload.extend_from_slice(&0_i64.to_le_bytes()); // source, PrefabInstance, Prefab
    }
    payload.extend_from_slice(&1_i32.to_le_bytes()); // one Transform component
    payload.extend_from_slice(&0_i32.to_le_bytes());
    payload.extend_from_slice(&TRANSFORM_PATH_ID.to_le_bytes());
    payload.extend_from_slice(&0_u32.to_le_bytes()); // layer
    payload.extend_from_slice(&4_i32.to_le_bytes());
    payload.extend_from_slice(b"Root");
    payload.extend_from_slice(&0_i32.to_le_bytes()); // empty tag string
    payload.extend_from_slice(&0_i32.to_le_bytes());
    payload.extend_from_slice(&0_i64.to_le_bytes()); // icon
    payload.extend_from_slice(&0_u32.to_le_bytes()); // navigation layer
    payload.extend_from_slice(&0_u32.to_le_bytes()); // static editor flags
    payload.push(1); // active GameObject
    let game_object_size = payload.len() - game_object_start;
    while payload.len() % 4 != 0 {
        payload.push(0);
    }

    let transform_start = payload.len();
    payload.extend_from_slice(&0_u32.to_le_bytes()); // object hide flags
    for _ in 0..3 {
        payload.extend_from_slice(&0_i32.to_le_bytes());
        payload.extend_from_slice(&0_i64.to_le_bytes()); // source, PrefabInstance, Prefab
    }
    payload.extend_from_slice(&0_i32.to_le_bytes());
    payload.extend_from_slice(&GAME_OBJECT_PATH_ID.to_le_bytes());
    for value in [0.0_f32, 0.0, 0.0, 1.0] {
        payload.extend_from_slice(&value.to_le_bytes()); // local rotation
    }
    for value in [0.0_f32, 0.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes()); // local position
    }
    for value in [1.0_f32, 1.0, 1.0] {
        payload.extend_from_slice(&value.to_le_bytes()); // local scale
    }
    payload.extend_from_slice(&0_i32.to_le_bytes()); // no children
    payload.extend_from_slice(&0_i32.to_le_bytes()); // no father
    payload.extend_from_slice(&0_i64.to_le_bytes());
    payload.extend_from_slice(&0_i32.to_le_bytes()); // root order
    for value in [0.0_f32, 0.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes()); // local Euler hint
    }
    let transform_size = payload.len() - transform_start;
    while payload.len() % 4 != 0 {
        payload.push(0);
    }

    let prefab_instance_start = payload.len();
    payload.extend_from_slice(&0_u32.to_le_bytes()); // object hide flags
    payload.extend_from_slice(&0_i32.to_le_bytes());
    payload.extend_from_slice(&GAME_OBJECT_PATH_ID.to_le_bytes());
    payload.extend_from_slice(&0_i32.to_le_bytes());
    payload.extend_from_slice(&0_i64.to_le_bytes()); // modification transform parent
    payload.extend_from_slice(&0_i32.to_le_bytes()); // no modifications
    payload.extend_from_slice(&0_i32.to_le_bytes()); // no removed components
    payload.extend_from_slice(&0_i32.to_le_bytes());
    payload.extend_from_slice(&0_i64.to_le_bytes()); // source Prefab
    let prefab_instance_size = payload.len() - prefab_instance_start;

    write_object_record(
        &mut metadata,
        GAME_OBJECT_PATH_ID,
        game_object_start,
        game_object_size,
        1,
        1,
    );
    write_object_record(
        &mut metadata,
        TRANSFORM_PATH_ID,
        transform_start,
        transform_size,
        4,
        4,
    );
    write_object_record(
        &mut metadata,
        PREFAB_INSTANCE_PATH_ID,
        prefab_instance_start,
        prefab_instance_size,
        1001,
        1001,
    );
    metadata.extend_from_slice(&0_i32.to_le_bytes()); // external files
    metadata.push(0); // user information string

    let data_offset = 20 + metadata.len();
    let file_size = data_offset + payload.len();
    let mut file = Vec::with_capacity(file_size);
    file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    file.extend_from_slice(&(file_size as u32).to_be_bytes());
    file.extend_from_slice(&10_i32.to_be_bytes());
    file.extend_from_slice(&(data_offset as u32).to_be_bytes());
    file.push(0);
    file.extend_from_slice(&[0; 3]);
    file.extend_from_slice(&metadata);
    file.extend_from_slice(&payload);
    file
}
