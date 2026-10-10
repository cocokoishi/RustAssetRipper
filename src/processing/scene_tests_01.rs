    use super::{
        classify_scene_hierarchy_asset, reference_field_path_matches,
        scene_hierarchy_dependency_fields, MultiFileReferenceGraph, SceneGraph,
        SceneGraphIssueKind, SceneGraphLimits, SceneHierarchyAssetKind, SceneHierarchyAuditLimits,
        SceneHierarchyIssueKind, SceneHierarchySlot, SceneQuaternion, SceneRectTransformValues,
        SceneRelationshipKind, SceneRelationshipTarget, SceneRootDiscoveryIssueKind,
        SceneRootDiscoveryLimits, SceneRootReason, SceneTransformValues, SceneVector2,
        SceneVector3,
    };
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

    #[test]
    fn reads_prefab_instance_root_game_object_pointer() {
        let metadata = metadata(&[(100, 1001), (200, 1)]);
        let roots = vec![
            (
                0,
                Value::Struct(vec![field("m_RootGameObject", pptr(0, 200))]),
            ),
            (1, Value::Struct(game_object_fields("Root", Vec::new()))),
        ];
        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        assert!(graph.is_complete(), "{:?}", graph.issues());
        assert_eq!(graph.relationships().len(), 1);
        let relationship = &graph.relationships()[0];
        assert_eq!(
            relationship.kind,
            SceneRelationshipKind::PrefabInstanceRootGameObject
        );
        assert_eq!(relationship.field_path, "m_RootGameObject");
        assert_eq!(relationship.file_id, 0);
        assert_eq!(relationship.path_id, 200);
        assert_eq!(
            relationship.target,
            SceneRelationshipTarget::LocalObject { object_index: 1 }
        );
    }

    #[test]
    fn distinguishes_null_and_missing_prefab_root_pointer_fields() {
        let metadata = metadata(&[(100, 1001)]);
        let null_graph = SceneGraph::from_decoded_roots(
            &metadata,
            &[(
                0,
                Value::Struct(vec![field("m_RootGameObject", pptr(0, 0))]),
            )],
            SceneGraphLimits::default(),
        )
        .unwrap();
        assert!(null_graph.is_complete());
        assert_eq!(null_graph.relationships().len(), 1);
        assert_eq!(
            null_graph.relationships()[0].target,
            SceneRelationshipTarget::Null
        );

        let missing_graph = SceneGraph::from_decoded_roots(
            &metadata,
            &[(0, Value::Struct(Vec::new()))],
            SceneGraphLimits::default(),
        )
        .unwrap();
        assert!(!missing_graph.is_complete());
        assert!(missing_graph.relationships().is_empty());
        assert!(missing_graph.issues().iter().any(|issue| matches!(
            &issue.kind,
            SceneGraphIssueKind::MissingField { field_name }
                if field_name == "m_RootGameObject"
        )));
    }

    #[test]
    fn resolves_serialized_prefab_root_pointer_across_loaded_files() {
        let prefab_bytes = make_prefab_instance_file(100, 1, 42, Some("shared.assets"));
        let root_bytes = make_game_object_file("Root", 42, None, None);
        let prefab_file = SerializedFile::parse(&prefab_bytes).unwrap();
        let root_file = SerializedFile::parse(&root_bytes).unwrap();
        let prefab_reference_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&prefab_file)
                .unwrap();
        let root_reference_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&root_file)
                .unwrap();
        let files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &prefab_file.metadata,
                graph: &prefab_reference_graph,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &root_file.metadata,
                graph: &root_reference_graph,
            },
        ];
        let references = MultiFileReferenceGraph::build(&files).unwrap();
        let mut scene = SceneGraph::build(&prefab_file, SceneGraphLimits::default()).unwrap();
        assert_eq!(
            scene.relationships().len(),
            1,
            "class={:?}; issues={:?}; decoded={:?}",
            prefab_file.metadata.objects[0].class_id,
            scene.issues(),
            prefab_file.decode_object_payload(0).unwrap().root
        );
        assert_eq!(
            scene.relationships()[0].target,
            SceneRelationshipTarget::ExternalObject {
                file_id: 1,
                external_index: 0,
                path_id: 42,
            }
        );

        scene
            .resolve_with_multi_file_graph(0, "main.assets", &references)
            .unwrap();
        assert_eq!(
            scene.relationships()[0].target,
            SceneRelationshipTarget::WorkspaceObject {
                file_index: 1,
                object_index: 0,
            }
        );
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

    #[test]
    fn reconstructs_game_object_component_and_transform_edges_without_merging_order() {
        let metadata = metadata(&[(10, 1), (20, 4), (30, 4), (40, 23)]);
        let roots = vec![
            (
                0,
                Value::Struct(vec![
                    field(
                        "Base",
                        Value::Struct(vec![
                            field("m_Name", Value::StringBytes(b"Avatar".to_vec())),
                            field("m_IsActive", Value::Bool(true)),
                        ]),
                    ),
                    field(
                        "m_Component",
                        Value::Array(vec![
                            Value::Struct(vec![field("component", pptr(0, 20))]),
                            Value::Struct(vec![field("component", pptr(0, 40))]),
                        ]),
                    ),
                ]),
            ),
            (
                1,
                Value::Struct(vec![field(
                    "Base",
                    Value::Struct(transform_fields_with_links(
                        true,
                        pptr(0, 0),
                        vec![pptr(0, 30)],
                    )),
                )]),
            ),
            (
                2,
                Value::Struct(vec![field(
                    "Base",
                    Value::Struct(transform_fields_with_links(true, pptr(0, 20), Vec::new())),
                )]),
            ),
        ];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();
        assert_eq!(graph.objects().len(), 4);
        assert_eq!(
            graph.objects()[0].name_bytes.as_deref(),
            Some(&b"Avatar"[..])
        );
        assert_eq!(graph.objects()[0].is_active, Some(true));
        assert_eq!(graph.relationships().len(), 7);
        assert_eq!(
            graph
                .relationships()
                .iter()
                .map(|relationship| relationship.kind)
                .collect::<Vec<_>>(),
            [
                SceneRelationshipKind::GameObjectComponent,
                SceneRelationshipKind::GameObjectComponent,
                SceneRelationshipKind::ComponentGameObject,
                SceneRelationshipKind::TransformParent,
                SceneRelationshipKind::TransformChild,
                SceneRelationshipKind::ComponentGameObject,
                SceneRelationshipKind::TransformParent,
            ]
        );
        assert_eq!(
            graph.relationships()[0].target,
            SceneRelationshipTarget::LocalObject { object_index: 1 }
        );
        assert_eq!(
            graph.relationships()[3].target,
            SceneRelationshipTarget::Null
        );
        assert_eq!(
            graph.relationships()[6].target,
            SceneRelationshipTarget::LocalObject { object_index: 1 }
        );
        assert!(graph.issues().is_empty());
        assert!(graph.audit_local_transform_links().unwrap().is_empty());
        assert!(graph.is_complete());
    }

    #[test]
    fn audits_missing_reciprocal_transform_child_links() {
        let metadata = metadata(&[(10, 4), (20, 4)]);
        let mut child_fields = transform_fields(false);
        child_fields
            .iter_mut()
            .find(|field| field.name == "m_Father")
            .unwrap()
            .value = pptr(0, 20);
        let roots = vec![
            (0, Value::Struct(child_fields)),
            (
                1,
                Value::Struct(transform_fields_with_links(false, pptr(0, 0), Vec::new())),
            ),
        ];
        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        let issues = graph.audit_local_transform_links().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0].kind,
            SceneHierarchyIssueKind::MissingReciprocal {
                counterpart_kind: SceneRelationshipKind::TransformChild,
            }
        );
        assert_eq!(issues[0].source_object_index, 0);
        assert_eq!(issues[0].target_object_index, 1);
    }

    #[test]
    fn audits_mismatched_and_duplicate_reciprocal_transform_links() {
        let metadata = metadata(&[(10, 4), (20, 4)]);
        let mismatched_roots = vec![
            (
                0,
                Value::Struct(transform_fields_with_links(
                    false,
                    pptr(0, 0),
                    vec![pptr(0, 20)],
                )),
            ),
            (1, Value::Struct(transform_fields(false))),
        ];
        let mismatched = SceneGraph::from_decoded_roots(
            &metadata,
            &mismatched_roots,
            SceneGraphLimits::default(),
        )
        .unwrap();
        let mismatch_issues = mismatched.audit_local_transform_links().unwrap();
        assert_eq!(mismatch_issues.len(), 1);
        assert_eq!(
            mismatch_issues[0].kind,
            SceneHierarchyIssueKind::MismatchedReciprocalTarget {
                counterpart_kind: SceneRelationshipKind::TransformParent,
                actual_target: SceneRelationshipTarget::Null,
            }
        );

        let duplicate_roots = vec![
            (
                0,
                Value::Struct(transform_fields_with_links(
                    false,
                    pptr(0, 0),
                    vec![pptr(0, 20), pptr(0, 20)],
                )),
            ),
            (
                1,
                Value::Struct(transform_fields_with_links(false, pptr(0, 10), Vec::new())),
            ),
        ];
        let duplicate = SceneGraph::from_decoded_roots(
            &metadata,
            &duplicate_roots,
            SceneGraphLimits::default(),
        )
        .unwrap();
        let duplicate_issues = duplicate.audit_local_transform_links().unwrap();
        assert_eq!(duplicate_issues.len(), 1);
        assert_eq!(
            duplicate_issues[0].kind,
            SceneHierarchyIssueKind::DuplicateReciprocal {
                counterpart_kind: SceneRelationshipKind::TransformChild,
                match_count: 2,
            }
        );
    }

    #[test]
    fn audits_local_parent_pointers_that_target_non_transforms() {
        let metadata = metadata(&[(10, 4), (20, 23)]);
        let mut fields = transform_fields(false);
        fields
            .iter_mut()
            .find(|field| field.name == "m_Father")
            .unwrap()
            .value = pptr(0, 20);
        let roots = vec![(0, Value::Struct(fields))];
        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        let issues = graph.audit_local_transform_links().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0].kind,
            SceneHierarchyIssueKind::TargetIsNotTransform { class_id: 23 }
        );
    }

    #[test]
    fn discovers_transform_roots_and_game_objects_without_transforms() {
        let metadata = metadata(&[(10, 1), (20, 4), (30, 1), (40, 4), (50, 1)]);
        let mut child_transform_fields =
            transform_fields_with_links(false, pptr(0, 20), Vec::new());
        child_transform_fields
            .iter_mut()
            .find(|field| field.name == "m_GameObject")
            .unwrap()
            .value = pptr(0, 30);
        let roots = vec![
            (
                0,
                Value::Struct(game_object_fields("Root", vec![pptr(0, 20)])),
            ),
            (
                1,
                Value::Struct(transform_fields_with_links(
                    false,
                    pptr(0, 0),
                    vec![pptr(0, 40)],
                )),
            ),
            (
                2,
                Value::Struct(game_object_fields("Child", vec![pptr(0, 40)])),
            ),
            (3, Value::Struct(child_transform_fields)),
            (
                4,
                Value::Struct(game_object_fields("NoTransform", Vec::new())),
            ),
        ];
        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        let discovery = graph
            .discover_local_roots(SceneRootDiscoveryLimits::default())
            .unwrap();
        assert_eq!(
            discovery.roots(),
            [
                super::SceneRootCandidate {
                    game_object_index: 0,
                    transform_index: Some(1),
                    reason: SceneRootReason::NullFather,
                },
                super::SceneRootCandidate {
                    game_object_index: 4,
                    transform_index: None,
                    reason: SceneRootReason::NoTransformComponent,
                },
            ]
        );
        assert!(discovery.issues().is_empty());
        assert!(discovery.is_complete());
        assert!(matches!(
            graph.discover_local_roots(SceneRootDiscoveryLimits {
                max_roots: 1,
                ..SceneRootDiscoveryLimits::default()
            }),
            Err(super::SceneRootDiscoveryError::RootCountLimitExceeded {
                requested: 2,
                limit: 1,
            })
        ));
    }

    #[test]
    fn root_discovery_keeps_unresolved_component_and_parent_states_visible() {
        let component_metadata = metadata(&[(10, 1), (20, 4)]);
        let component_roots = vec![
            (
                0,
                Value::Struct(game_object_fields(
                    "Unknown",
                    vec![pptr(0, 99), pptr(0, 20)],
                )),
            ),
            (1, Value::Struct(transform_fields(false))),
        ];
        let component_graph = SceneGraph::from_decoded_roots(
            &component_metadata,
            &component_roots,
            SceneGraphLimits::default(),
        )
        .unwrap();
        let component_discovery = component_graph
            .discover_local_roots(SceneRootDiscoveryLimits::default())
            .unwrap();
        assert!(component_discovery.roots().is_empty());
        assert!(matches!(
            component_discovery.issues()[0].kind,
            SceneRootDiscoveryIssueKind::UnresolvedComponent {
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 99 }
            }
        ));
        assert!(matches!(
            component_graph.discover_local_roots(SceneRootDiscoveryLimits {
                max_issues: 0,
                ..SceneRootDiscoveryLimits::default()
            }),
            Err(super::SceneRootDiscoveryError::IssueCountLimitExceeded {
                requested: 1,
                limit: 0,
            })
        ));

        let parent_metadata = metadata(&[(10, 1), (20, 4)]);
        let mut transform = transform_fields(false);
        transform
            .iter_mut()
            .find(|field| field.name == "m_Father")
            .unwrap()
            .value = pptr(0, 99);
        let parent_roots = vec![
            (
                0,
                Value::Struct(game_object_fields("UnknownParent", vec![pptr(0, 20)])),
            ),
            (1, Value::Struct(transform)),
        ];
        let parent_graph = SceneGraph::from_decoded_roots(
            &parent_metadata,
            &parent_roots,
            SceneGraphLimits::default(),
        )
        .unwrap();
        let parent_discovery = parent_graph
            .discover_local_roots(SceneRootDiscoveryLimits::default())
            .unwrap();
        assert!(parent_discovery.roots().is_empty());
        assert!(parent_discovery.issues().iter().any(|issue| matches!(
            issue.kind,
            SceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 99 }
            }
        )));
    }

    #[test]
    fn decodes_transform_and_rect_transform_values_from_type_tree_fields() {
        let metadata = metadata(&[(10, 4), (20, 224)]);
        let mut rect_fields = vec![field("Base", Value::Struct(transform_fields(true)))];
        rect_fields.extend([
            field("m_AnchorMin", vector2(0.1, 0.2)),
            field("m_AnchorMax", vector2(0.8, 0.9)),
            field("m_AnchoredPosition", vector2(12.0, -6.0)),
            field("m_SizeDelta", vector2(320.0, 180.0)),
            field("m_Pivot", vector2(0.5, 1.0)),
        ]);
        let roots = vec![
            (0, Value::Struct(transform_fields(false))),
            (1, Value::Struct(rect_fields)),
        ];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        assert_eq!(
            graph.objects()[0].transform,
            Some(SceneTransformValues {
                local_position: Some(SceneVector3 {
                    x: 1.25,
                    y: -2.0,
                    z: 3.5,
                }),
                local_rotation: Some(SceneQuaternion {
                    x: 0.1,
                    y: 0.2,
                    z: 0.3,
                    w: 0.9,
                }),
                local_scale: Some(SceneVector3 {
                    x: 2.0,
                    y: 3.0,
                    z: 4.0,
                }),
                root_order: None,
                local_euler_angles_hint: None,
                rect_transform: None,
            })
        );
        assert_eq!(
            graph.objects()[1]
                .transform
                .as_ref()
                .unwrap()
                .rect_transform,
            Some(SceneRectTransformValues {
                anchor_min: Some(SceneVector2 { x: 0.1, y: 0.2 }),
                anchor_max: Some(SceneVector2 { x: 0.8, y: 0.9 }),
                anchored_position: Some(SceneVector2 { x: 12.0, y: -6.0 }),
                size_delta: Some(SceneVector2 { x: 320.0, y: 180.0 }),
                pivot: Some(SceneVector2 { x: 0.5, y: 1.0 }),
            })
        );
        assert_eq!(
            graph.objects()[1].transform.as_ref().unwrap().root_order,
            Some(7)
        );
        assert_eq!(
            graph.objects()[1]
                .transform
                .as_ref()
                .unwrap()
                .local_euler_angles_hint,
            Some(SceneVector3 {
                x: 10.0,
                y: 20.0,
                z: 30.0,
            })
        );
        assert!(graph.issues().is_empty());
    }

    #[test]
    fn reports_malformed_transform_values_without_filling_defaults() {
        let metadata = metadata(&[(10, 4)]);
        let mut fields = transform_fields(false);
        fields
            .iter_mut()
            .find(|field| field.name == "m_LocalPosition")
            .unwrap()
            .value = Value::Bool(false);
        let roots = vec![(0, Value::Struct(fields))];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

        assert_eq!(
            graph.objects()[0]
                .transform
                .as_ref()
                .unwrap()
                .local_position,
            None
        );
        assert!(graph.issues().iter().any(|issue| matches!(
            issue.kind,
            SceneGraphIssueKind::UnexpectedValueType {
                expected: "Vector3",
                actual: "bool",
            }
        )));
        assert!(!graph.is_complete());
    }

    #[test]
    fn builds_a_scene_graph_from_a_complete_serialized_file() {
        let bytes = make_game_object_file("Root", 42, Some((0, 42)), None);
        let file = SerializedFile::parse(&bytes).unwrap();
        let mut graph = SceneGraph::build(&file, SceneGraphLimits::default()).unwrap();

        assert_eq!(graph.objects().len(), 1);
        assert_eq!(graph.objects()[0].path_id, 42);
        assert_eq!(graph.objects()[0].class_id, Some(1));
        assert_eq!(graph.objects()[0].name_bytes.as_deref(), Some(&b"Root"[..]));
        assert_eq!(graph.objects()[0].is_active, Some(true));
        assert_eq!(graph.relationships().len(), 1);
        assert_eq!(graph.relationships()[0].field_path, "m_Component[0]");
        assert_eq!(
            graph.relationships()[0].target,
            SceneRelationshipTarget::LocalObject { object_index: 0 }
        );
        assert!(graph.issues().is_empty());
        assert!(graph.is_complete());

        let per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&file).unwrap();
        let named_file = crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "GameObject.assets",
            metadata: &file.metadata,
            graph: &per_file_graph,
        };
        let multi_file_graph = MultiFileReferenceGraph::build(&[named_file]).unwrap();
        let target_before_failed_resolution = graph.relationships()[0].target.clone();
        assert!(matches!(
            graph.resolve_with_multi_file_graph(0, "wrong.assets", &multi_file_graph),
            Err(super::SceneGraphResolveError::SourceFileNameMismatch { .. })
        ));
        assert_eq!(
            graph.relationships()[0].target,
            target_before_failed_resolution
        );
        graph
            .resolve_with_multi_file_graph(0, "GAMEOBJECT.ASSETS", &multi_file_graph)
            .unwrap();
        assert_eq!(
            graph.relationships()[0].target,
            SceneRelationshipTarget::LocalObject { object_index: 0 }
        );
    }

    #[test]
    fn builds_transform_values_from_a_complete_2019_serialized_file() {
        let bytes = make_transform_file();
        let file = SerializedFile::parse(&bytes).unwrap();
        let graph = SceneGraph::build(&file, SceneGraphLimits::default()).unwrap();

        assert_eq!(graph.objects().len(), 1);
        assert_eq!(graph.objects()[0].class_id, Some(4));
        assert_eq!(
            graph.objects()[0].transform,
            Some(SceneTransformValues {
                local_position: Some(SceneVector3 {
                    x: 1.25,
                    y: -2.0,
                    z: 3.5,
                }),
                local_rotation: Some(SceneQuaternion {
                    x: 0.1,
                    y: 0.2,
                    z: 0.3,
                    w: 0.9,
                }),
                local_scale: Some(SceneVector3 {
                    x: 2.0,
                    y: 3.0,
                    z: 4.0,
                }),
                root_order: Some(7),
                local_euler_angles_hint: Some(SceneVector3 {
                    x: 10.0,
                    y: 20.0,
                    z: 30.0,
                }),
                rect_transform: None,
            })
        );
        assert_eq!(graph.relationships().len(), 2);
        assert!(graph.issues().is_empty());
    }

    #[test]
    fn resolves_scene_component_links_across_loaded_serialized_files() {
        let source_bytes =
            make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
        let target_bytes = make_game_object_file("Target", 99, None, None);
        let source_file = SerializedFile::parse(&source_bytes).unwrap();
        let target_file = SerializedFile::parse(&target_bytes).unwrap();
        let source_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&source_file)
                .unwrap();
        let target_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&target_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_file.metadata,
                graph: &source_per_file_graph,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &target_file.metadata,
                graph: &target_per_file_graph,
            },
        ];
        let reference_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut scene = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
        assert!(matches!(
            scene.relationships()[0].target,
            SceneRelationshipTarget::ExternalObject {
                file_id: 1,
                external_index: 0,
                path_id: 99,
            }
        ));

        scene
            .resolve_with_multi_file_graph(0, "main.assets", &reference_graph)
            .unwrap();
        assert_eq!(
            scene.relationships()[0].target,
            SceneRelationshipTarget::WorkspaceObject {
                file_index: 1,
                object_index: 0,
            }
        );
        assert!(scene.is_complete());

        let source_only = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_file.metadata,
                graph: &source_per_file_graph,
            },
        ];
        let missing_reference_graph = MultiFileReferenceGraph::build(&source_only).unwrap();
        let mut missing_scene =
            SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
        missing_scene
            .resolve_with_multi_file_graph(0, "main.assets", &missing_reference_graph)
            .unwrap();
        assert_eq!(
            missing_scene.relationships()[0].target,
            SceneRelationshipTarget::MissingDependency {
                external_index: 0,
                path_id: 99,
            }
        );
        assert!(!missing_scene.is_complete());

        let no_edge_bytes = make_game_object_file("Source", 42, None, Some("shared.assets"));
        let no_edge_file = SerializedFile::parse(&no_edge_bytes).unwrap();
        let no_edge_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&no_edge_file)
                .unwrap();
        let no_edge_named = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &no_edge_file.metadata,
                graph: &no_edge_per_file_graph,
            },
        ];
        let no_edge_graph = MultiFileReferenceGraph::build(&no_edge_named).unwrap();
        let mut unmatched_scene =
            SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
        let original_target = unmatched_scene.relationships()[0].target.clone();
        assert!(matches!(
            unmatched_scene.resolve_with_multi_file_graph(0, "main.assets", &no_edge_graph),
            Err(super::SceneGraphResolveError::ReferenceFieldNotFound { .. })
        ));
        assert_eq!(unmatched_scene.relationships()[0].target, original_target);
    }

    #[test]
    fn refuses_scene_reference_join_when_same_identity_has_a_different_pptr_value() {
        let scene_bytes = make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
        let graph_bytes = make_game_object_file("Source", 42, Some((1, 98)), Some("shared.assets"));
        let scene_file = SerializedFile::parse(&scene_bytes).unwrap();
        let graph_file = SerializedFile::parse(&graph_bytes).unwrap();
        let per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&graph_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &graph_file.metadata,
                graph: &per_file_graph,
            },
        ];
        let reference_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut scene = SceneGraph::build(&scene_file, SceneGraphLimits::default()).unwrap();
        let original_target = scene.relationships()[0].target.clone();

        assert!(matches!(
            scene.resolve_with_multi_file_graph(0, "main.assets", &reference_graph),
            Err(super::SceneGraphResolveError::ReferenceValueMismatch {
                scene_file_id: 1,
                scene_path_id: 99,
                graph_file_id: 1,
                graph_path_id: 98,
                ..
            })
        ));
        assert_eq!(scene.relationships()[0].target, original_target);
    }

    #[test]
    fn discovers_workspace_roots_when_transform_component_is_external() {
        let source_bytes =
            make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
        let target_bytes = make_transform_file_with_links(99, 0, 0);
        let source_file = SerializedFile::parse(&source_bytes).unwrap();
        let target_file = SerializedFile::parse(&target_bytes).unwrap();
        let source_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&source_file)
                .unwrap();
        let target_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&target_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_file.metadata,
                graph: &source_per_file_graph,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &target_file.metadata,
                graph: &target_per_file_graph,
            },
        ];
        let reference_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut scene = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();

        let roots = scene
            .discover_workspace_roots(
                0,
                "main.assets",
                &reference_graph,
                SceneRootDiscoveryLimits::default(),
            )
            .unwrap();
        assert_eq!(
            roots.roots(),
            &[super::WorkspaceSceneRootCandidate {
                source_file_index: 0,
                game_object_index: 0,
                transform_file_index: Some(1),
                transform_index: Some(0),
                reason: SceneRootReason::NullFather,
            }]
        );
        assert!(roots.issues().is_empty());
        assert!(roots.is_complete());
    }

    #[test]
    fn workspace_root_discovery_does_not_hide_missing_external_transform_parent() {
        let source_bytes =
            make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
        let target_bytes = make_transform_file_with_links(99, 0, 777);
        let source_file = SerializedFile::parse(&source_bytes).unwrap();
        let target_file = SerializedFile::parse(&target_bytes).unwrap();
        let source_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&source_file)
                .unwrap();
        let target_per_file_graph =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&target_file)
                .unwrap();
        let named_files = [
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_file.metadata,
                graph: &source_per_file_graph,
            },
            crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &target_file.metadata,
                graph: &target_per_file_graph,
            },
        ];
        let reference_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut scene = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();

        let roots = scene
            .discover_workspace_roots(
                0,
                "main.assets",
                &reference_graph,
                SceneRootDiscoveryLimits::default(),
            )
            .unwrap();
        assert!(roots.roots().is_empty());
        assert_eq!(
            roots.issues(),
            &[super::WorkspaceSceneRootDiscoveryIssue {
                source_file_index: 0,
                game_object_index: 0,
                transform_file_index: Some(1),
                transform_index: Some(0),
                kind: super::WorkspaceSceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                    target: SceneRelationshipTarget::MissingLocalObject { path_id: 777 },
                },
            }]
        );
        assert!(!roots.is_complete());
    }

    #[test]
    fn scene_definition_owns_compacted_cross_file_hierarchy_data() {
        use crate::serialized_file::reference_graph::{
            MultiFileReferenceGraph, NamedObjectReferenceGraph, ObjectReferenceGraph,
        };

        let source_bytes = make_game_object_file_with_components(
            "SceneObject",
            42,
            &[(1, 99), (0, 0)],
            Some("shared.assets"),
        );
        let target_bytes =
            make_transform_file_with_hierarchy(99, 0, 0, &[], &["main.assets"], false);
        let source_file = SerializedFile::parse(&source_bytes).unwrap();
        let target_file = SerializedFile::parse(&target_bytes).unwrap();
        let source_references = ObjectReferenceGraph::build(&source_file).unwrap();
        let target_references = ObjectReferenceGraph::build(&target_file).unwrap();
        let named_files = [
            NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_file.metadata,
                graph: &source_references,
            },
            NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &target_file.metadata,
                graph: &target_references,
            },
        ];
        let multi_file_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
        let mut scene_graph = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
        let roots = scene_graph
            .discover_workspace_roots(
                0,
                "main.assets",
                &multi_file_graph,
                SceneRootDiscoveryLimits::default(),
            )
            .unwrap();
        let audit = scene_graph
            .audit_workspace_transform_links(
                0,
                "main.assets",
                &multi_file_graph,
                super::SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        let plan = crate::processing::scene_collections::plan_scene_definition(
            0,
            "main.assets",
            &crate::processing::scene_collections::SceneCollectionPathStatus::BuildSettingsMissing,
            None,
            None,
            crate::processing::scene_collections::SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        let definitions =
            crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
                vec![plan],
                2,
                crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
            )
            .unwrap();

        let limited = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
            max_scene_objects: 0,
            ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
        };
        let mut bounded =
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(2, limited)
                .unwrap();
        assert!(matches!(
            bounded.add_collection_graph(
                0,
                &scene_graph,
                roots.clone(),
                audit.clone(),
                &multi_file_graph,
            ),
            Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
                resource: "scene objects",
                requested: 1,
                limit: 0,
            })
        ));
        assert!(!bounded.contains_collection_graph(0));

        let graph_limited = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
            max_workspace_collection_graphs: 0,
            ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
        };
        let mut bounded_graphs =
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
                2,
                graph_limited,
            )
            .unwrap();
        assert!(matches!(
            bounded_graphs.add_collection_graph(
                0,
                &scene_graph,
                roots.clone(),
                audit.clone(),
                &multi_file_graph,
            ),
            Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
                resource: "workspace hierarchy collection graphs",
                requested: 1,
                limit: 0,
            })
        ));
        assert!(!bounded_graphs.contains_collection_graph(0));

        let asset_limited = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
            max_scene_asset_records: 1,
            ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
        };
        let mut bounded_assets =
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
                2,
                asset_limited,
            )
            .unwrap();
        assert!(matches!(
            bounded_assets.add_collection_graph(
                0,
                &scene_graph,
                roots.clone(),
                audit.clone(),
                &multi_file_graph,
            ),
            Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
                resource: "scene asset and generated-transform records",
                requested: 2,
                limit: 1,
            })
        ));
        assert!(!bounded_assets.contains_collection_graph(0));

        let non_scene_definitions =
            crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
                Vec::new(),
                2,
                crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
            )
            .unwrap();
        let mut non_scene_builder =
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
                2,
                crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
            )
            .unwrap();
        non_scene_builder
            .add_collection_graph(
                0,
                &scene_graph,
                roots.clone(),
                audit.clone(),
                &multi_file_graph,
            )
            .unwrap();
        let non_scene_hierarchies = non_scene_builder.finish(&non_scene_definitions).unwrap();
        assert!(non_scene_hierarchies.scenes().is_empty());
        assert_eq!(non_scene_hierarchies.non_scene_collections().len(), 1);
        assert_eq!(
            non_scene_hierarchies.non_scene_collections()[0].collection_file_index(),
            0
        );
        assert_eq!(
            non_scene_hierarchies.non_scene_collections()[0].game_object_count(),
            1
        );

        let mut builder = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
            2,
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
        )
        .unwrap();
        builder
            .add_collection_graph(0, &scene_graph, roots, audit, &multi_file_graph)
            .unwrap();
        let hierarchy = builder.finish(&definitions).unwrap();
        assert_eq!(hierarchy.scenes().len(), 1);
        assert_eq!(hierarchy.scenes()[0].plan.collection_file_index, 0);
        let collections = hierarchy.scenes()[0].collections();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].game_object_count(), 1);
        assert_eq!(collections[0].transform_count(), 0);
        assert_eq!(collections[0].scene_assets().len(), 1);
        assert_eq!(
            collections[0].scene_assets()[0],
            crate::processing::scene_hierarchy::WorkspaceSceneAssetIdentity {
                identity: crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
                    file_index: 0,
                    object_index: 0,
                    path_id: 42,
                    class_id: Some(1),
                },
                category:
                    crate::processing::scene_hierarchy::WorkspaceSceneAssetCategory::GameObject,
            }
        );
        assert_eq!(collections[0].component_candidates().len(), 1);
        assert_eq!(
            collections[0].component_candidates()[0],
            crate::processing::scene_hierarchy::WorkspaceSceneComponentCandidate {
                identity: crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
                    file_index: 1,
                    object_index: 0,
                    path_id: 99,
                    class_id: Some(4),
                },
                evidence: crate::processing::scene_hierarchy::WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            }
        );
        assert_eq!(collections[0].relationships().len(), 2);
        assert_eq!(collections[0].game_object_hierarchies().len(), 1);
        assert_eq!(
            collections[0].game_object_hierarchies()[0].game_object(),
            crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
                file_index: 0,
                object_index: 0,
                path_id: 42,
                class_id: Some(1),
            }
        );
        assert_eq!(
            collections[0].game_object_hierarchies()[0].component_relationship_count(),
            2
        );
        let ordered_components: Vec<_> = collections[0]
            .game_object_component_relationships(0)
            .unwrap()
            .collect();
        assert_eq!(ordered_components.len(), 2);
        assert_eq!(ordered_components[0].field_path, "m_Component[0]");
        assert_eq!(ordered_components[0].file_id, 1);
        assert_eq!(ordered_components[0].path_id, 99);
        assert_eq!(
            ordered_components[0].target,
            SceneRelationshipTarget::WorkspaceObject {
                file_index: 1,
                object_index: 0,
            }
        );
        assert_eq!(ordered_components[1].field_path, "m_Component[1]");
        assert_eq!(ordered_components[1].target, SceneRelationshipTarget::Null);
        assert!(matches!(
            collections[0].game_object_component_relationships(1),
            Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::GameObjectHierarchyIndexOutOfBounds {
                hierarchy_index: 1,
                hierarchy_count: 1,
            })
        ));
        assert_eq!(collections[0].roots().roots().len(), 1);
        assert_eq!(
            collections[0].roots().roots()[0].transform_file_index,
            Some(1)
        );
        assert!(collections[0].is_complete());
    }
