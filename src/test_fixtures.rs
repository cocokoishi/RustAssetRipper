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

fn make_prefab_hierarchy_file() -> Vec<u8> {
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

pub(crate) fn make_existing_prefab_instance_file() -> Vec<u8> {
    make_prefab_mesh_instance_file(None, None)
}

/// Combine the Prefab hierarchy fixture with a production-layout class-43 Mesh object.
///
/// `mesh_file` is the existing 2019.4 single-Mesh fixture. By default its TypeTree record and
/// payload are copied into the hierarchy file; the external-mesh variant leaves the object in its
/// own SerializedFile and writes a source-table PPtr plus external-file record. Both variants use
/// the same production Mesh payload and minimal MeshFilter/MeshRenderer objects.
pub(crate) fn make_prefab_mesh_instance_file(
    mesh_file: Option<&[u8]>,
    external_mesh_file_name: Option<&str>,
) -> Vec<u8> {
    const GAME_OBJECT_PATH_ID: i64 = 10;
    const TRANSFORM_PATH_ID: i64 = 11;
    const MESH_FILTER_PATH_ID: i64 = 13;
    const MESH_RENDERER_PATH_ID: i64 = 14;
    const MESH_PATH_ID: i64 = 42;

    #[derive(Clone, Copy)]
    struct ObjectRecord {
        path_id: i32,
        byte_start: u32,
        byte_size: i32,
        raw_type_id: i32,
        class_id: i16,
        destroyed: u16,
    }

    struct FileLayout {
        metadata_end: usize,
        data_offset: usize,
        type_count_offset: usize,
        type_ranges: Vec<std::ops::Range<usize>>,
        type_records_end: usize,
        object_count_offset: usize,
        object_records_end: usize,
        objects: Vec<ObjectRecord>,
    }

    fn read_i32_le(bytes: &[u8], offset: usize) -> i32 {
        i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn parse_layout(bytes: &[u8]) -> FileLayout {
        assert!(
            bytes.len() >= 20,
            "the SerializedFile has a complete header"
        );
        let metadata_size = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let metadata_end = 20 + metadata_size;
        let data_offset = u32::from_be_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert!(metadata_end <= bytes.len(), "the metadata is in bounds");
        assert!(data_offset >= metadata_end, "the data follows the metadata");
        let metadata = &bytes[20..metadata_end];
        let version_end = metadata
            .iter()
            .position(|byte| *byte == 0)
            .expect("the Unity version is terminated")
            + 1;
        let type_count_offset = version_end + 4;
        let type_count = read_i32_le(metadata, type_count_offset);
        let mut type_cursor = type_count_offset + 4;
        let mut type_ranges = Vec::with_capacity(usize::try_from(type_count).unwrap());
        for _ in 0..type_count {
            let node_count = usize::try_from(read_i32_le(metadata, type_cursor + 4)).unwrap();
            let string_size = usize::try_from(read_i32_le(metadata, type_cursor + 8)).unwrap();
            let node_bytes = node_count.checked_mul(24).expect("TypeTree size fits");
            let type_end = type_cursor
                .checked_add(12 + node_bytes + string_size)
                .expect("TypeTree range fits");
            assert!(type_end <= metadata.len(), "the TypeTree is in bounds");
            type_ranges.push(type_cursor..type_end);
            type_cursor = type_end;
        }
        let type_records_end = type_cursor;
        // Unity SerializedFile generation 10 stores the long-FileID flag after the type table.
        let object_count_offset = type_records_end + 4;
        let object_count = usize::try_from(read_i32_le(metadata, object_count_offset)).unwrap();
        let object_records_start = object_count_offset + 4;
        let object_records_end = object_records_start
            .checked_add(
                object_count
                    .checked_mul(20)
                    .expect("object table size fits"),
            )
            .expect("object table range fits");
        assert!(
            object_records_end <= metadata.len(),
            "the object table is in bounds"
        );
        let objects = (0..object_count)
            .map(|index| {
                let start = object_records_start + index * 20;
                ObjectRecord {
                    path_id: read_i32_le(metadata, start),
                    byte_start: u32::from_le_bytes(
                        metadata[start + 4..start + 8].try_into().unwrap(),
                    ),
                    byte_size: read_i32_le(metadata, start + 8),
                    raw_type_id: read_i32_le(metadata, start + 12),
                    class_id: i16::from_le_bytes(
                        metadata[start + 16..start + 18].try_into().unwrap(),
                    ),
                    destroyed: u16::from_le_bytes(
                        metadata[start + 18..start + 20].try_into().unwrap(),
                    ),
                }
            })
            .collect();
        FileLayout {
            metadata_end,
            data_offset,
            type_count_offset,
            type_ranges,
            type_records_end,
            object_count_offset,
            object_records_end,
            objects,
        }
    }

    fn payload_for<'a>(bytes: &'a [u8], layout: &FileLayout, object: ObjectRecord) -> &'a [u8] {
        let start = layout.data_offset + usize::try_from(object.byte_start).unwrap();
        let end = start + usize::try_from(object.byte_size).unwrap();
        bytes
            .get(start..end)
            .expect("the object payload is in bounds")
    }

    fn append_component_pointer(output: &mut Vec<u8>, path_id: i64) {
        output.extend_from_slice(&0_i32.to_le_bytes());
        output.extend_from_slice(&path_id.to_le_bytes());
    }

    fn append_aligned_string(output: &mut Vec<u8>, value: &str) {
        output.extend_from_slice(&i32::try_from(value.len()).unwrap().to_le_bytes());
        output.extend_from_slice(value.as_bytes());
        while !output.len().is_multiple_of(4) {
            output.push(0);
        }
    }

    fn append_metadata_string(output: &mut Vec<u8>, value: &str) {
        output.extend_from_slice(value.as_bytes());
        output.push(0);
    }

    let prefab_file = make_prefab_hierarchy_file();
    let Some(mesh_file) = mesh_file else {
        return prefab_file;
    };
    let prefab_layout = parse_layout(&prefab_file);
    let mesh_layout = parse_layout(mesh_file);
    assert_eq!(prefab_file[8..12], 10_u32.to_be_bytes());
    assert_eq!(mesh_file[8..12], 10_u32.to_be_bytes());
    assert_eq!(prefab_layout.type_ranges.len(), 3);
    assert_eq!(mesh_layout.type_ranges.len(), 1);
    assert!(
        prefab_layout
            .objects
            .iter()
            .all(|object| object.destroyed == 0),
        "the base hierarchy fixture has no destroyed objects"
    );

    let prefab_metadata = &prefab_file[20..prefab_layout.metadata_end];
    let mesh_metadata = &mesh_file[20..mesh_layout.metadata_end];
    let mesh_type_range = &mesh_layout.type_ranges[0];
    assert_eq!(read_i32_le(mesh_metadata, mesh_type_range.start), 43);
    let mesh_record = mesh_layout
        .objects
        .iter()
        .copied()
        .find(|object| object.class_id == 43)
        .expect("the production fixture contains a Mesh");
    assert_eq!(mesh_record.path_id, MESH_PATH_ID as i32);

    let mesh_filter_tree = [
        (0, 1, "MeshFilter", "Base", -1, 32768),
        (1, 1, "Component", "Base", -1, 0),
        (2, 1, "PPtr<GameObject>", "m_GameObject", 12, 0),
        (3, 1, "int", "m_FileID", 4, 0),
        (3, 1, "SInt64", "m_PathID", 8, 0),
        (2, 1, "PPtr<Mesh>", "m_Mesh", 12, 0),
        (3, 1, "int", "m_FileID", 4, 0),
        (3, 1, "SInt64", "m_PathID", 8, 0),
    ];
    let mesh_renderer_tree = [
        (0, 1, "MeshRenderer", "Base", -1, 32768),
        (1, 1, "Renderer", "Base", -1, 0),
        (2, 1, "Component", "Base", -1, 0),
        (3, 1, "PPtr<GameObject>", "m_GameObject", 12, 0),
        (4, 1, "int", "m_FileID", 4, 0),
        (4, 1, "SInt64", "m_PathID", 8, 0),
        (2, 1, "StaticBatchInfo", "m_StaticBatchInfo", -1, 0),
        (3, 1, "unsigned int", "firstSubMesh", 4, 0),
        (3, 1, "unsigned int", "subMeshCount", 4, 0),
    ];
    let mut extra_types = Vec::new();
    write_type_tree_with_meta_flags(&mut extra_types, 33, &mesh_filter_tree);
    write_type_tree_with_meta_flags(&mut extra_types, 23, &mesh_renderer_tree);

    let mut game_object = Vec::new();
    game_object.extend_from_slice(&0_u32.to_le_bytes()); // m_ObjectHideFlags
    for _ in 0..3 {
        append_component_pointer(&mut game_object, 0); // source, PrefabInstance, Prefab
    }
    game_object.extend_from_slice(&3_i32.to_le_bytes()); // Transform, MeshFilter, MeshRenderer
    for path_id in [
        TRANSFORM_PATH_ID,
        MESH_FILTER_PATH_ID,
        MESH_RENDERER_PATH_ID,
    ] {
        append_component_pointer(&mut game_object, path_id);
    }
    game_object.extend_from_slice(&0_u32.to_le_bytes()); // m_Layer
    append_aligned_string(&mut game_object, "Meshed"); // m_Name
    append_aligned_string(&mut game_object, ""); // m_TagString
    append_component_pointer(&mut game_object, 0); // m_Icon
    game_object.extend_from_slice(&0_u32.to_le_bytes()); // m_NavMeshLayer
    game_object.extend_from_slice(&0_u32.to_le_bytes()); // m_StaticEditorFlags
    game_object.push(1); // m_IsActive

    let mut mesh_filter = Vec::with_capacity(24);
    append_component_pointer(&mut mesh_filter, GAME_OBJECT_PATH_ID);
    mesh_filter.extend_from_slice(
        &if external_mesh_file_name.is_some() {
            1_i32
        } else {
            0_i32
        }
        .to_le_bytes(),
    );
    mesh_filter.extend_from_slice(&MESH_PATH_ID.to_le_bytes());

    let mut mesh_renderer = Vec::with_capacity(20);
    append_component_pointer(&mut mesh_renderer, GAME_OBJECT_PATH_ID);
    mesh_renderer.extend_from_slice(&0_u32.to_le_bytes()); // firstSubMesh
    mesh_renderer.extend_from_slice(&0_u32.to_le_bytes()); // subMeshCount: dynamic mesh

    let mut output_objects = Vec::with_capacity(6);
    for object in &prefab_layout.objects {
        let payload = if object.path_id == GAME_OBJECT_PATH_ID as i32 {
            game_object.clone()
        } else {
            payload_for(&prefab_file, &prefab_layout, *object).to_vec()
        };
        output_objects.push((object.path_id, object.raw_type_id, object.class_id, payload));
    }
    output_objects.push((MESH_FILTER_PATH_ID as i32, 33, 33, mesh_filter));
    output_objects.push((MESH_RENDERER_PATH_ID as i32, 23, 23, mesh_renderer));
    if external_mesh_file_name.is_none() {
        output_objects.push((
            MESH_PATH_ID as i32,
            43,
            43,
            payload_for(mesh_file, &mesh_layout, mesh_record).to_vec(),
        ));
    }

    let mut data = Vec::new();
    let mut object_records = Vec::with_capacity(output_objects.len());
    for (path_id, raw_type_id, class_id, object_payload) in output_objects {
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
        let data_start = data.len();
        data.extend_from_slice(&object_payload);
        object_records.push((
            i64::from(path_id),
            u32::try_from(data_start).unwrap(),
            u32::try_from(object_payload.len()).unwrap(),
            raw_type_id,
            class_id,
        ));
    }

    let mut metadata = Vec::new();
    metadata.extend_from_slice(&prefab_metadata[..prefab_layout.type_count_offset]);
    metadata.extend_from_slice(
        &if external_mesh_file_name.is_some() {
            5_i32
        } else {
            6_i32
        }
        .to_le_bytes(),
    ); // three hierarchy classes, MeshFilter, MeshRenderer, and an optional local Mesh
    for range in &prefab_layout.type_ranges {
        metadata.extend_from_slice(&prefab_metadata[range.start..range.end]);
    }
    metadata.extend_from_slice(&extra_types);
    if external_mesh_file_name.is_none() {
        metadata.extend_from_slice(&mesh_metadata[mesh_type_range.start..mesh_type_range.end]);
    }
    // Preserve the generation-10 long-FileID flag before writing the external/user-info tail.
    metadata.extend_from_slice(
        &prefab_metadata[prefab_layout.type_records_end..prefab_layout.object_count_offset],
    );
    metadata.extend_from_slice(&i32::try_from(object_records.len()).unwrap().to_le_bytes());
    for (path_id, data_start, byte_size, raw_type_id, class_id) in object_records {
        write_object_record(
            &mut metadata,
            path_id,
            usize::try_from(data_start).unwrap(),
            usize::try_from(byte_size).unwrap(),
            raw_type_id,
            class_id,
        );
    }
    if let Some(dependency_name) = external_mesh_file_name {
        metadata.extend_from_slice(&1_i32.to_le_bytes()); // one external file
        append_metadata_string(&mut metadata, dependency_name); // asset path
        metadata.extend_from_slice(&[0; 16]); // GUID
        metadata.extend_from_slice(&0_i32.to_le_bytes()); // asset type
        append_metadata_string(&mut metadata, dependency_name); // path name
        metadata.push(0); // empty user information string
    } else {
        metadata.extend_from_slice(&prefab_metadata[prefab_layout.object_records_end..]);
    }

    let data_offset = 20 + metadata.len();
    let file_size = data_offset + data.len();
    let mut output = Vec::with_capacity(file_size);
    output.extend_from_slice(&u32::try_from(metadata.len()).unwrap().to_be_bytes());
    output.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
    output.extend_from_slice(&prefab_file[8..12]); // SerializedFile format version
    output.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
    output.extend_from_slice(&prefab_file[16..20]); // endian and reserved bytes
    output.extend_from_slice(&metadata);
    output.extend_from_slice(&data);
    output
}
