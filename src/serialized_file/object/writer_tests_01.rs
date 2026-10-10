    use super::*;
    use crate::io::binary::Endian;
    use crate::serialized_file::metadata::{
        SerializedFileMetadata, SerializedType, TypeTree, TypeTreeNode,
    };
    use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
    use crate::serialized_file::object::SerializedFieldValue;

    fn node(level: u8, name: &str, type_name: &str, byte_size: i32) -> TypeTreeNode {
        TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size,
            index: 0,
            meta_flags: 0,
            reference_type_hash: None,
        }
    }

    fn metadata_with_tree(tree: TypeTree) -> SerializedFileMetadata {
        SerializedFileMetadata {
            prefix: SerializedFileMetadataPrefix {
                metadata_offset: 0,
                metadata_size: 0,
                body_endian: Endian::Little,
                unity_version_text: Some("2022.3.1f1".to_owned()),
                target_platform: Some(0),
                has_type_tree: true,
                consumed_bytes: 0,
            },
            types: vec![SerializedType {
                raw_type_id: 4,
                type_id_for_hash: 4,
                is_stripped_type: false,
                script_type_index: -1,
                script_id: None,
                old_type_hash: None,
                type_tree: Some(tree),
                type_dependencies: Vec::new(),
            }],
            long_file_id: None,
            objects: Vec::new(),
            script_types: Vec::new(),
            externals: Vec::new(),
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        }
    }

    fn encoder<'layout, 'tree>(
        layout: &'layout TypeTreeLayout<'tree>,
        arrays_align: bool,
    ) -> PayloadEncoder<'layout, 'tree, 'static> {
        PayloadEncoder {
            type_tree: layout,
            endian: Endian::Little,
            arrays_align,
            reference_types: &[],
            allow_managed_registry: false,
            object_index: 7,
            limits: ObjectEncodeLimits::default(),
            output: Vec::new(),
        }
    }

    #[test]
    fn encodes_pptr_file_and_path_ids_using_the_declared_integer_width() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Target", "PPtr<GameObject>", 8),
                node(2, "m_FileID", "SInt32", 4),
                node(2, "m_PathID", "SInt32", 4),
            ],
            string_buffer: Vec::new(),
        };
        let layout = TypeTreeLayout::new(&tree, 7, 0).unwrap();
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Target".to_owned(),
            value: SerializedValue::PPtr {
                file_id: 2,
                path_id: -3,
            },
        }]);
        let mut encoder = encoder(&layout, false);

        encoder.encode_root(&value, "Root").unwrap();

        assert_eq!(encoder.output, [2, 0, 0, 0, 0xfd, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn encodes_64_bit_pptr_path_ids() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Target", "PPtr<GameObject>", 12),
                node(2, "m_FileID", "SInt32", 4),
                node(2, "m_PathID", "SInt64", 8),
            ],
            string_buffer: Vec::new(),
        };
        let layout = TypeTreeLayout::new(&tree, 7, 0).unwrap();
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Target".to_owned(),
            value: SerializedValue::PPtr {
                file_id: 2,
                path_id: -3,
            },
        }]);
        let mut encoder = encoder(&layout, false);

        encoder.encode_root(&value, "Root").unwrap();

        assert_eq!(
            encoder.output,
            [2, 0, 0, 0, 0xfd, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
    }

    #[test]
    fn encodes_version_aligned_primitive_arrays_with_zero_padding() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Values", "vector", -1),
                node(2, "Array", "Array", -1),
                node(3, "size", "int", 4),
                node(3, "data", "UInt8", 1),
            ],
            string_buffer: Vec::new(),
        };
        let layout = TypeTreeLayout::new(&tree, 7, 0).unwrap();
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Values".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::U8(7)]),
        }]);
        let mut encoder = encoder(&layout, true);

        encoder.encode_root(&value, "Root").unwrap();

        assert_eq!(encoder.output, [1, 0, 0, 0, 7, 0, 0, 0]);
    }

    #[test]
    fn rejects_nonempty_zero_width_arrays() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Values", "vector", -1),
                node(2, "Array", "Array", -1),
                node(3, "size", "int", 4),
                node(3, "data", "ZeroWidth", 0),
            ],
            string_buffer: Vec::new(),
        };
        let layout = TypeTreeLayout::new(&tree, 7, 0).unwrap();
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Values".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::Raw {
                type_name: "ZeroWidth".to_owned(),
                bytes: Vec::new(),
            }]),
        }]);
        let mut encoder = encoder(&layout, false);

        let error = encoder.encode_root(&value, "Root").unwrap_err();

        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::ZeroWidthArrayElement { count: 1 }
        ));
    }

    #[test]
    fn writes_unknown_fixed_size_leaves_and_aligns_the_following_field() {
        let mut aligned = node(1, "m_Value", "UInt8", 1);
        aligned.meta_flags = 0x4000;
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Raw", "Opaque", 2),
                aligned,
            ],
            string_buffer: Vec::new(),
        };
        let layout = TypeTreeLayout::new(&tree, 7, 0).unwrap();
        let value = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "m_Raw".to_owned(),
                value: SerializedValue::Raw {
                    type_name: "Opaque".to_owned(),
                    bytes: vec![0xaa, 0xbb],
                },
            },
            SerializedFieldValue {
                name: "m_Value".to_owned(),
                value: SerializedValue::U8(7),
            },
        ]);
        let mut encoder = encoder(&layout, false);

        encoder.encode_root(&value, "Root").unwrap();

        assert_eq!(encoder.output, [0xaa, 0xbb, 7, 0]);
    }

    #[test]
    fn constructs_bounded_type_tree_defaults_and_encodes_them() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Transform", -1),
                node(1, "m_GameObject", "PPtr<GameObject>", 12),
                node(2, "m_FileID", "SInt32", 4),
                node(2, "m_PathID", "SInt64", 8),
                node(1, "m_LocalRotation", "Quaternion", -1),
                node(2, "x", "float", 4),
                node(2, "y", "float", 4),
                node(2, "z", "float", 4),
                node(2, "w", "float", 4),
                node(1, "m_Name", "string", -1),
                node(2, "Array", "Array", -1),
                node(3, "size", "int", 4),
                node(3, "data", "char", 1),
                node(1, "m_Bytes", "TypelessData", -1),
                node(2, "size", "int", 4),
                node(2, "data", "UInt8", 1),
                node(1, "m_Children", "vector", -1),
                node(2, "Array", "Array", -1),
                node(3, "size", "int", 4),
                node(3, "data", "PPtr<Transform>", 12),
                node(4, "m_FileID", "SInt32", 4),
                node(4, "m_PathID", "SInt64", 8),
                node(1, "m_Opaque", "OpaqueRecord", 3),
            ],
            string_buffer: Vec::new(),
        };
        let metadata = metadata_with_tree(tree);

        let value = metadata
            .default_object_value_for_type(0, 0, ObjectDefaultValueLimits::default())
            .unwrap();

        assert_eq!(
            value.unique_field("m_GameObject").unwrap(),
            Some(&SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            })
        );
        assert_eq!(
            value.unique_field("m_Name").unwrap(),
            Some(&SerializedValue::StringBytes(Vec::new()))
        );
        assert_eq!(
            value.unique_field("m_Bytes").unwrap(),
            Some(&SerializedValue::Bytes(Vec::new()))
        );
        assert_eq!(
            value.unique_field("m_Children").unwrap(),
            Some(&SerializedValue::Array(Vec::new()))
        );
        assert_eq!(
            value.unique_field("m_Opaque").unwrap(),
            Some(&SerializedValue::Raw {
                type_name: "OpaqueRecord".to_owned(),
                bytes: vec![0; 3],
            })
        );
        let payload = metadata
            .encode_object_payload_for_type(0, 0, &value, &[], ObjectEncodeLimits::default())
            .unwrap();
        assert_eq!(payload.len(), 12 + 16 + 4 + 4 + 4 + 3);
    }

    #[test]
    fn constructs_array_element_template_from_its_type_tree_subtree() {
        let metadata = metadata_with_tree(TypeTree {
            nodes: vec![
                node(0, "", "GameObject", -1),
                node(1, "m_Component", "vector", -1),
                node(2, "Array", "Array", -1),
                node(3, "size", "int", 4),
                node(3, "data", "ComponentPair", -1),
                node(4, "component", "PPtr<Component>", 12),
                node(5, "m_FileID", "SInt32", 4),
                node(5, "m_PathID", "SInt64", 8),
            ],
            string_buffer: Vec::new(),
        });

        let template = metadata
            .default_value_for_type_tree_node(0, 0, 4, ObjectDefaultValueLimits::default())
            .unwrap();

        assert_eq!(
            template.unique_field("component").unwrap(),
            Some(&SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            })
        );
    }

    #[test]
    fn default_value_builder_enforces_node_and_allocation_limits() {
        let metadata = metadata_with_tree(TypeTree {
            nodes: vec![node(0, "", "OpaqueRecord", 16)],
            string_buffer: Vec::new(),
        });

        let type_tree_error = metadata
            .default_object_value_for_type(
                0,
                0,
                ObjectDefaultValueLimits {
                    max_type_tree_nodes: 0,
                    ..ObjectDefaultValueLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            type_tree_error.kind(),
            ObjectEncodeErrorKind::DefaultTypeTreeNodeLimitExceeded {
                requested: 1,
                limit: 0
            }
        ));

        let node_error = metadata
            .default_object_value_for_type(
                0,
                0,
                ObjectDefaultValueLimits {
                    max_value_nodes: 0,
                    ..ObjectDefaultValueLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            node_error.kind(),
            ObjectEncodeErrorKind::DefaultValueNodeLimitExceeded {
                requested: 1,
                limit: 0
            }
        ));

        let allocation_error = metadata
            .default_object_value_for_type(
                0,
                0,
                ObjectDefaultValueLimits {
                    max_allocation_bytes: 1,
                    ..ObjectDefaultValueLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            allocation_error.kind(),
            ObjectEncodeErrorKind::DefaultValueAllocationLimitExceeded {
                requested,
                limit: 1
            } if *requested > 1
        ));
    }

    #[test]
    fn default_value_builder_rejects_malformed_pptr_and_managed_reference_shapes() {
        let malformed_pptr = metadata_with_tree(TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Target", "PPtr<GameObject>", 4),
            ],
            string_buffer: Vec::new(),
        });
        let error = malformed_pptr
            .default_object_value_for_type(0, 0, ObjectDefaultValueLimits::default())
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::InvalidTypeTree { message }
                if message.contains("PPtr node has an invalid component shape")
        ));

        let malformed_pptr_integer = metadata_with_tree(TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Target", "PPtr<GameObject>", 8),
                node(2, "m_FileID", "float", 4),
                node(2, "m_PathID", "SInt32", 4),
            ],
            string_buffer: Vec::new(),
        });
        let error = malformed_pptr_integer
            .default_object_value_for_type(0, 0, ObjectDefaultValueLimits::default())
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::InvalidTypeTree { message }
                if message.contains("PPtr file ID node is not 32-bit")
        ));

        let managed_reference = metadata_with_tree(TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Reference", "managedReference", 8),
                node(2, "rid", "SInt64", 8),
            ],
            string_buffer: Vec::new(),
        });
        let error = managed_reference
            .default_object_value_for_type(0, 0, ObjectDefaultValueLimits::default())
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::UnsupportedManagedReferences {
                type_name,
                field_name,
            } if type_name == "managedReference" && field_name == "m_Reference"
        ));
    }
