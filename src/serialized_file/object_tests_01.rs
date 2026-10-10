    use super::{
        ObjectDecodeErrorKind, ObjectEncodeErrorKind, ObjectEncodeLimits, SerializedFieldValue,
        SerializedValue, SerializedValueLookupError,
    };
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::header::SerializedFileHeader;
    use crate::serialized_file::metadata::{SerializedFileMetadata, TypeTree, TypeTreeNode};

    fn layout_node(level: u8, name: &str, type_name: &str) -> TypeTreeNode {
        TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size: 0,
            index: 0,
            meta_flags: 0,
            reference_type_hash: None,
        }
    }

    fn element_layout(nodes: Vec<TypeTreeNode>) -> super::TypeTreeLayout<'static> {
        // The layout borrows the tree; leak the box so the test can hold it.
        let tree: &'static TypeTree = Box::leak(Box::new(TypeTree {
            nodes,
            string_buffer: Vec::new(),
        }));
        super::TypeTreeLayout::new(tree, 0, 0).unwrap()
    }

    #[test]
    fn managed_reference_elements_require_the_full_source_predicate() {
        // `managedReference` holding one `rid` child matches.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "rid", "SInt64"),
        ]);
        assert!(super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // The legacy `id` child matches too.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedRefArrayItem"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // A wrong child name does not match.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "value", "SInt64"),
        ]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Two children do not match.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "rid", "SInt64"),
            layout_node(1, "extra", "SInt32"),
        ]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // No children do not match.
        let layout = element_layout(vec![layout_node(0, "item", "managedRefArrayItem")]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Other element types never match, even with a `rid` child.
        let layout = element_layout(vec![
            layout_node(0, "item", "Generic Mono"),
            layout_node(1, "rid", "SInt64"),
        ]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
    }

    #[test]
    fn indexed_managed_references_require_the_legacy_identifier() {
        // The legacy `id` child selects the indexed shape.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // The modern `rid` child does not.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "rid", "SInt64"),
        ]);
        assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // The array-item spelling participates as well.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedRefArrayItem"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Non-reference elements never select the indexed shape.
        let layout = element_layout(vec![
            layout_node(0, "item", "Generic Mono"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Two children do not match, even when the first is `id`.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "id", "SInt32"),
            layout_node(1, "extra", "SInt32"),
        ]);
        assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
    }

    #[test]
    fn managed_registry_and_data_nodes_follow_the_source_predicates() {
        // `IsManagedReferencesRegistry`: the name pair plus more than one subnode.
        assert!(super::is_managed_references_registry(
            "ManagedReferencesRegistry",
            "references",
            2
        ));
        assert!(!super::is_managed_references_registry(
            "ManagedReferencesRegistry",
            "references",
            1
        ));
        assert!(!super::is_managed_references_registry(
            "ManagedReferencesRegistry",
            "references",
            0
        ));
        assert!(!super::is_managed_references_registry(
            "ManagedReferencesRegistry",
            "other",
            2
        ));
        assert!(!super::is_managed_references_registry(
            "other",
            "references",
            2
        ));
        // `IsReferencedObjectData`: the dynamic holder carries no subnodes; a
        // same-named node with children is an ordinary struct, not dynamic data.
        assert!(super::is_referenced_object_data(
            "ReferencedObjectData",
            "data",
            0
        ));
        assert!(!super::is_referenced_object_data(
            "ReferencedObjectData",
            "data",
            1
        ));
        assert!(!super::is_referenced_object_data(
            "ReferencedObjectData",
            "other",
            0
        ));
        assert!(!super::is_referenced_object_data("other", "data", 0));
    }

    #[test]
    fn structured_values_support_unique_fields_arrays_and_pointer_identity() {
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "target".to_owned(),
            value: SerializedValue::PPtr {
                file_id: 2,
                path_id: 41,
            },
        }]);
        let target = value.unique_field("target").unwrap().unwrap();
        assert_eq!(target.pptr(), Some((2, 41)));
        assert_eq!(value.unique_field("missing").unwrap(), None);
        assert_eq!(value.array_elements(), None);
        assert_eq!(target.struct_fields(), None);

        let array = SerializedValue::Array(vec![SerializedValue::U8(7)]);
        assert_eq!(array.array_elements().unwrap(), &[SerializedValue::U8(7)]);
        assert_eq!(array.pptr(), None);
    }

    #[test]
    fn structured_field_lookup_rejects_duplicate_names_and_non_structs() {
        let duplicate = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "value".to_owned(),
                value: SerializedValue::I32(1),
            },
            SerializedFieldValue {
                name: "value".to_owned(),
                value: SerializedValue::I32(2),
            },
        ]);
        assert_eq!(
            duplicate.unique_field("value"),
            Err(SerializedValueLookupError::DuplicateField {
                name: "value".to_owned(),
                match_count: 2,
            })
        );
        assert_eq!(
            SerializedValue::Bool(true).unique_field("value"),
            Err(SerializedValueLookupError::NotStruct)
        );
    }

    #[test]
    fn inherited_field_lookup_follows_only_base_structs_and_detects_duplicates() {
        let inherited = SerializedValue::Struct(vec![super::SerializedFieldValue {
            name: "Base".to_owned(),
            value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"child".to_vec()),
            }]),
        }]);
        assert_eq!(
            inherited.unique_field_in_base_chain("m_Name").unwrap(),
            Some(&SerializedValue::StringBytes(b"child".to_vec()))
        );

        let duplicate = SerializedValue::Struct(vec![
            super::SerializedFieldValue {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"derived".to_vec()),
            },
            super::SerializedFieldValue {
                name: "Base".to_owned(),
                value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                    name: "m_Name".to_owned(),
                    value: SerializedValue::StringBytes(b"base".to_vec()),
                }]),
            },
        ]);
        assert_eq!(
            duplicate.unique_field_in_base_chain("m_Name"),
            Err(SerializedValueLookupError::DuplicateField {
                name: "m_Name".to_owned(),
                match_count: 2,
            })
        );
    }

    #[test]
    fn inherited_component_array_mutation_preserves_source_slots_and_checks_bounds() {
        let mut root = SerializedValue::Struct(vec![super::SerializedFieldValue {
            name: "Base".to_owned(),
            value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(vec![
                    SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 0,
                    },
                    SerializedValue::PPtr {
                        file_id: 2,
                        path_id: 41,
                    },
                ]),
            }]),
        }]);

        let components = root
            .unique_field_in_base_chain_mut("m_Component")
            .unwrap()
            .unwrap();
        components
            .push_array_element_bounded(
                SerializedValue::PPtr {
                    file_id: 3,
                    path_id: 99,
                },
                3,
            )
            .unwrap();
        assert!(matches!(
            components.push_array_element_bounded(
                SerializedValue::PPtr {
                    file_id: 4,
                    path_id: 100,
                },
                3
            ),
            Err(
                super::SerializedValueMutationError::ArrayElementLimitExceeded {
                    requested: 4,
                    limit: 3,
                }
            )
        ));

        let components = root
            .unique_field_in_base_chain("m_Component")
            .unwrap()
            .unwrap();
        assert_eq!(
            components.array_elements().unwrap(),
            &[
                SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                },
                SerializedValue::PPtr {
                    file_id: 2,
                    path_id: 41,
                },
                SerializedValue::PPtr {
                    file_id: 3,
                    path_id: 99,
                },
            ]
        );
    }

    #[test]
    fn inherited_field_mutation_rejects_duplicate_matches() {
        let mut root = SerializedValue::Struct(vec![
            super::SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(Vec::new()),
            },
            super::SerializedFieldValue {
                name: "Base".to_owned(),
                value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                    name: "m_Component".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                }]),
            },
        ]);

        assert!(matches!(
            root.unique_field_in_base_chain_mut("m_Component"),
            Err(super::SerializedValueMutationError::DuplicateField {
                name,
                match_count: 2,
            }) if name == "m_Component"
        ));
    }

    fn node(
        level: u8,
        name: &str,
        type_name: &str,
        byte_size: i32,
        meta_flags: u32,
    ) -> TypeTreeNode {
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
            meta_flags,
            reference_type_hash: None,
        }
    }

    fn make_file(payload: &[u8]) -> (Vec<u8>, SerializedFileHeader, SerializedFileMetadata) {
        let generation = FormatVersion::Unknown9;
        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2022.3.1f1\0");
        metadata.extend_from_slice(&0u32.to_le_bytes()); // target platform
        metadata.extend_from_slice(&[1]); // type tree enabled
        let metadata_offset = 20usize;
        let data_offset = metadata_offset + metadata.len();
        let object_bytes = payload.to_vec();
        let file_size = data_offset + object_bytes.len();
        let mut file = vec![0; 20];
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&object_bytes);
        let header = SerializedFileHeader {
            metadata_size: metadata.len() as u64,
            file_size: file_size as u64,
            format_version: generation,
            data_offset: data_offset as u64,
            data_endian: Some(Endian::Little),
            reserved_v22: None,
            header_size: 20,
        };
        let mut metadata = SerializedFileMetadata {
            prefix: super::super::metadata_prefix::SerializedFileMetadataPrefix {
                metadata_offset,
                metadata_size: metadata.len(),
                body_endian: Endian::Little,
                unity_version_text: Some("2022.3.1f1".to_owned()),
                target_platform: Some(0),
                has_type_tree: true,
                consumed_bytes: 16,
            },
            types: vec![super::super::metadata::SerializedType {
                raw_type_id: 1,
                type_id_for_hash: 1,
                is_stripped_type: false,
                script_type_index: -1,
                script_id: None,
                old_type_hash: None,
                type_tree: Some(TypeTree {
                    nodes: vec![
                        node(0, "Root", "Root", -1, 0),
                        node(1, "m_Number", "int", 4, 0),
                        node(1, "m_Flag", "bool", 1, 0),
                    ],
                    string_buffer: Vec::new(),
                }),
                type_dependencies: Vec::new(),
            }],
            long_file_id: None,
            objects: vec![super::super::metadata::ObjectInfo {
                path_id: 42,
                byte_start: 0,
                byte_size: payload.len() as u32,
                serialized_type_index: None,
                raw_type_id: 1,
                class_id: Some(1),
                script_type_index: None,
                is_destroyed: None,
                is_stripped: None,
                resolved_type_index: Some(0),
            }],
            script_types: Vec::new(),
            externals: Vec::new(),
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 16,
        };
        metadata.prefix.metadata_size = metadata.prefix.consumed_bytes;
        (file, header, metadata)
    }

    #[test]
    fn decodes_primitive_fields_and_preserves_trailing_payload_bytes() {
        let payload = [0x78, 0x56, 0x34, 0x12, 1, 0xaa];
        let (file, header, metadata) = make_file(&payload);
        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();

        assert_eq!(object.path_id, 42);
        assert_eq!(object.type_name, "Root");
        assert_eq!(
            object.root,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "m_Number".to_owned(),
                    value: SerializedValue::I32(0x1234_5678),
                },
                super::SerializedFieldValue {
                    name: "m_Flag".to_owned(),
                    value: SerializedValue::Bool(true),
                },
            ])
        );
        assert_eq!(object.consumed_bytes, 5);
        assert_eq!(object.trailing_bytes, [0xaa]);
    }

    #[test]
    fn encodes_typed_object_fields_and_retains_trailing_bytes_in_both_endian_modes() {
        let little_endian_payload = [0x78, 0x56, 0x34, 0x12, 1, 0xaa];
        let (file, header, metadata) = make_file(&little_endian_payload);
        let mut object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            little_endian_payload
        );

        *object
            .root
            .unique_field_in_base_chain_mut("m_Number")
            .unwrap()
            .unwrap() = SerializedValue::I32(0x0102_0304);
        let encoded = metadata
            .encode_object_payload(&object, ObjectEncodeLimits::default())
            .unwrap();
        assert_eq!(encoded, [0x04, 0x03, 0x02, 0x01, 1, 0xaa]);
        let data_range = metadata.objects[0].data_range(&header).unwrap();
        let data_start = usize::try_from(data_range.start).unwrap();
        let mut edited_file = file.clone();
        edited_file[data_start..data_start + encoded.len()].copy_from_slice(&encoded);
        let decoded = metadata
            .decode_object_payload(&edited_file, &header, 0)
            .unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "m_Number".to_owned(),
                    value: SerializedValue::I32(0x0102_0304),
                },
                super::SerializedFieldValue {
                    name: "m_Flag".to_owned(),
                    value: SerializedValue::Bool(true),
                },
            ])
        );
        assert_eq!(decoded.trailing_bytes, [0xaa]);
        let error = metadata
            .encode_object_payload(
                &object,
                ObjectEncodeLimits {
                    max_encoded_payload_bytes: 4,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::EncodedPayloadLimitExceeded { limit: 4, .. }
        ));

        let big_endian_payload = [0x12, 0x34, 0x56, 0x78, 1, 0xaa];
        let (file, header, mut metadata) = make_file(&big_endian_payload);
        metadata.prefix.body_endian = Endian::Big;
        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            big_endian_payload
        );
    }

    #[test]
    fn tree_usability_distinguishes_absent_and_empty_trees() {
        use super::TypeTreeUsability;
        assert_eq!(
            super::classify_type_tree_usability(None),
            TypeTreeUsability::Missing
        );
        let empty = TypeTree {
            nodes: Vec::new(),
            string_buffer: Vec::new(),
        };
        assert_eq!(
            super::classify_type_tree_usability(Some(&empty)),
            TypeTreeUsability::Empty
        );
        let present = TypeTree {
            nodes: vec![node(0, "Root", "Root", -1, 0)],
            string_buffer: Vec::new(),
        };
        assert_eq!(
            super::classify_type_tree_usability(Some(&present)),
            TypeTreeUsability::Present
        );
        // The decode prelude reports each fact with its own error.
        let (file, header, mut metadata) = make_file(&[0]);
        metadata.types[0].type_tree = Some(empty);
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::EmptyTypeTree { object_index: 0 }
        ));
        metadata.types[0].type_tree = None;
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::MissingTypeTree { object_index: 0 }
        ));
    }

    #[test]
    fn pre_3_5_payloads_are_refused_before_decoding() {
        assert!(!super::version_is_unsupported(3, 5));
        assert!(!super::version_is_unsupported(4, 0));
        assert!(!super::version_is_unsupported(2022, 3));
        assert!(super::version_is_unsupported(3, 4));
        assert!(super::version_is_unsupported(2, 6));
        assert!(super::version_is_unsupported(0, 0));
        // A positively identified pre-3.5 file is refused with its own error.
        let full = [0x78, 0x56, 0x34, 0x12, 1];
        let (file, header, mut metadata) = make_file(&full);
        metadata.prefix.unity_version_text = Some("3.4.2f3".to_owned());
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::UnreadableUnsupportedVersion { object_index: 0 }
        ));
        // The 3.5 boundary itself, missing text, and garbage text all proceed.
        metadata.prefix.unity_version_text = Some("3.5.0b1".to_owned());
        assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
        metadata.prefix.unity_version_text = None;
        assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
        metadata.prefix.unity_version_text = Some("not-a-version".to_owned());
        assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
    }

    #[test]
    fn recovery_class_names_follow_the_source_enum_rule() {
        // Defined IDs render the enum member name.
        assert_eq!(super::class_id_type_name(1), Some("GameObject"));
        assert_eq!(super::class_id_type_name(28), Some("Texture2D"));
        assert_eq!(super::class_id_type_name(48), Some("Shader"));
        assert_eq!(super::class_id_type_name(114), Some("MonoBehaviour"));
        assert_eq!(super::class_id_type_name(115), Some("MonoScript"));
        assert_eq!(super::class_id_type_name(194), Some("NavMeshData_194"));
        assert_eq!(
            super::class_id_type_name(2126867596),
            Some("TilemapEditorUserSettings")
        );
        // `PlayerSettings` (129) is not a member; gaps and unknown IDs read `None`.
        assert_eq!(super::class_id_type_name(129), None);
        assert_eq!(super::class_id_type_name(3), None);
        assert_eq!(super::class_id_type_name(99999), None);
        // `RawDataObject.ClassName` renders undefined values numerically.
        assert_eq!(super::raw_data_class_name(48).as_ref(), "Shader");
        assert_eq!(super::raw_data_class_name(3).as_ref(), "3");
        assert_eq!(super::raw_data_class_name(99999).as_ref(), "99999");
        assert_eq!(super::raw_data_class_name(-5).as_ref(), "-5");
        // The fact composes with the factory naming shape: `Unknown{ClassName}_{Hash:X}`.
        assert_eq!(
            format!("Unknown{}_1234ABCD", super::raw_data_class_name(28)),
            "UnknownTexture2D_1234ABCD"
        );
    }

    #[test]
    fn rejects_negative_array_counts_and_missing_type_trees() {
        let (file, header, mut metadata) = make_file(&[0xff, 0xff, 0xff, 0xff]);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Values", "Array", -1, 0),
                node(2, "size", "int", 4, 0),
                node(2, "data", "int", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::NegativeArrayLength { length: -1, .. }
        ));

        metadata.types[0].type_tree = None;
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::MissingTypeTree { object_index: 0 }
        ));
    }

    #[test]
    fn decodes_modern_and_legacy_managed_reference_ids() {
        let modern_id = 7i64;
        let (file, header, mut metadata) = make_file(&modern_id.to_le_bytes());
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Target", "managedReference", 8, 0),
                node(2, "rid", "SInt64", 8, 0),
            ],
            string_buffer: Vec::new(),
        });
        let decoded = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Target".to_owned(),
                value: SerializedValue::ManagedReferenceId {
                    reference_id: 7,
                    indexed: false,
                },
            }])
        );

        let legacy_id = 3i32;
        let (file, header, mut metadata) = make_file(&legacy_id.to_le_bytes());
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Target", "managedReference", 4, 0),
                node(2, "id", "SInt32", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let decoded = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Target".to_owned(),
                value: SerializedValue::ManagedReferenceId {
                    reference_id: 3,
                    indexed: true,
                },
            }])
        );
    }

    #[test]
    fn decodes_and_encodes_named_vectors_of_managed_reference_array_items() {
        let mut modern_payload = Vec::new();
        modern_payload.extend_from_slice(&2i32.to_le_bytes());
        modern_payload.extend_from_slice(&7i64.to_le_bytes());
        modern_payload.extend_from_slice(&9i64.to_le_bytes());
        let (file, header, mut metadata) = make_file(&modern_payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Items", "managedReference", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "managedRefArrayItem", -1, 0),
                node(4, "rid", "SInt64", 8, 0),
            ],
            string_buffer: Vec::new(),
        });
        let decoded = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            decoded.root.unique_field("m_Items").unwrap(),
            Some(&SerializedValue::Array(vec![
                SerializedValue::ManagedReferenceId {
                    reference_id: 7,
                    indexed: false,
                },
                SerializedValue::ManagedReferenceId {
                    reference_id: 9,
                    indexed: false,
                },
            ]))
        );
        assert_eq!(
            metadata
                .encode_object_payload(&decoded, ObjectEncodeLimits::default())
                .unwrap(),
            modern_payload
        );

        let mut legacy_payload = Vec::new();
        legacy_payload.extend_from_slice(&2i32.to_le_bytes());
        legacy_payload.extend_from_slice(&0i32.to_le_bytes());
        legacy_payload.extend_from_slice(&1i32.to_le_bytes());
        let (file, header, mut metadata) = make_file(&legacy_payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Items", "managedReference", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "managedRefArrayItem", -1, 0),
                node(4, "id", "SInt32", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let decoded = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            decoded.root.unique_field("m_Items").unwrap(),
            Some(&SerializedValue::Array(vec![
                SerializedValue::ManagedReferenceId {
                    reference_id: 0,
                    indexed: true,
                },
                SerializedValue::ManagedReferenceId {
                    reference_id: 1,
                    indexed: true,
                },
            ]))
        );
        assert_eq!(
            metadata
                .encode_object_payload(&decoded, ObjectEncodeLimits::default())
                .unwrap(),
            legacy_payload
        );
    }

    #[test]
    fn decodes_runtime_types_shared_ids_and_cyclic_managed_payloads() {
        let fixture =
            crate::serialized_file::managed_reference_test_support::managed_reference_fixture();
        let decoded = fixture
            .metadata
            .decode_object_payload(&fixture.file, &fixture.header, 0)
            .unwrap();
        assert!(decoded.trailing_bytes.is_empty());
        assert_eq!(decoded.path_id, 500);
        let SerializedValue::Struct(fields) = decoded.root else {
            panic!("expected object struct");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::ManagedReferenceId {
                reference_id: 7,
                indexed: false,
            }
        );
        assert_eq!(
            fields[1].value,
            SerializedValue::ManagedReferenceId {
                reference_id: 7,
                indexed: false,
            }
        );
        assert_eq!(
            fields[2].value,
            SerializedValue::ManagedReferenceId {
                reference_id: 9,
                indexed: false,
            }
        );
        let SerializedValue::ManagedReferenceRegistry(registry) = &fields[3].value else {
            panic!("expected decoded registry");
        };
        assert_eq!(registry.entries.len(), 2);
        assert_eq!(registry.entries[0].reference_id, 7);
        assert_eq!(registry.entries[0].class_name, "NodeA");
        assert_eq!(registry.entries[1].reference_id, 9);
        assert_eq!(registry.entries[1].class_name, "NodeB");
        let crate::serialized_file::managed_registry::ManagedRegistryEntryData::Content(node_a) =
            &registry.entries[0].data
        else {
            panic!("expected NodeA content");
        };
        let crate::serialized_file::managed_registry::ManagedRegistryEntryData::Content(node_b) =
            &registry.entries[1].data
        else {
            panic!("expected NodeB content");
        };
        assert_eq!(
            node_a.unique_field("m_Value").unwrap(),
            Some(&SerializedValue::I32(100))
        );
        assert_eq!(
            node_b.unique_field("m_Value").unwrap(),
            Some(&SerializedValue::I32(200))
        );
        assert_eq!(
            node_a.unique_field("m_Link").unwrap(),
            Some(&SerializedValue::ManagedReferenceId {
                reference_id: 9,
                indexed: false,
            })
        );
        assert_eq!(
            node_b.unique_field("m_Link").unwrap(),
            Some(&SerializedValue::ManagedReferenceId {
                reference_id: 7,
                indexed: false,
            })
        );
        assert_eq!(
            node_a.unique_field("m_Asset").unwrap(),
            Some(&SerializedValue::PPtr {
                file_id: 1,
                path_id: 42,
            })
        );
        assert_eq!(fields[4].value, SerializedValue::I32(0x1234_5678));

        let decoded = fixture
            .metadata
            .decode_object_payload(&fixture.file, &fixture.header, 0)
            .unwrap();
        let encoded = fixture
            .metadata
            .encode_object_payload(&decoded, ObjectEncodeLimits::default())
            .unwrap();
        let start = usize::try_from(fixture.header.data_offset).unwrap();
        assert_eq!(
            encoded.len(),
            fixture.metadata.objects[0].byte_size as usize
        );
        assert_eq!(encoded, fixture.file[start..start + encoded.len()]);
    }

    #[test]
    fn decodes_terminated_legacy_registry_and_reencodes_exactly() {
        let fixture = crate::serialized_file::managed_reference_test_support::
            terminated_managed_reference_fixture();
        let decoded = fixture
            .metadata
            .decode_object_payload(&fixture.file, &fixture.header, 0)
            .unwrap();
        let SerializedValue::Struct(fields) = &decoded.root else {
            panic!("expected object struct");
        };
        for (field, expected_id) in [("m_First", 0), ("m_Shared", 0), ("m_Second", 1)] {
            assert_eq!(
                fields
                    .iter()
                    .find(|value| value.name == field)
                    .map(|value| &value.value),
                Some(&SerializedValue::ManagedReferenceId {
                    reference_id: expected_id,
                    indexed: true,
                })
            );
        }
        let SerializedValue::ManagedReferenceRegistry(registry) = &fields[3].value else {
            panic!("expected terminated registry");
        };
        assert_eq!(registry.version, 1);
        assert_eq!(registry.entries.len(), 2);
        assert_eq!(registry.entries[0].reference_id, 0);
        assert_eq!(registry.entries[1].reference_id, 1);
        let encoded = fixture
            .metadata
            .encode_object_payload(&decoded, ObjectEncodeLimits::default())
            .unwrap();
        let start = usize::try_from(fixture.header.data_offset).unwrap();
        assert_eq!(
            encoded.len(),
            fixture.metadata.objects[0].byte_size as usize
        );
        assert_eq!(encoded, fixture.file[start..start + encoded.len()]);
    }

    #[test]
    fn registry_failures_keep_the_object_and_type_tree_path() {
        let fixture =
            crate::serialized_file::managed_reference_test_support::managed_reference_fixture();
        let mut malformed = fixture.file.clone();
        let registry_start = usize::try_from(fixture.header.data_offset).unwrap() + 24;
        malformed[registry_start..registry_start + 4].copy_from_slice(&9i32.to_le_bytes());
        let error = fixture
            .metadata
            .decode_object_payload(&malformed, &fixture.header, 0)
            .unwrap_err();
        assert!(
            matches!(
                error.kind(),
                ObjectDecodeErrorKind::ManagedRegistryDecode {
                    object_index: 0,
                    path,
                    message,
                } if path == "Root.references" && message.contains("format 9")
            ),
            "unexpected contextual decode error: {error}"
        );
    }

    #[test]
    fn decodes_byte_arrays_with_versioned_alignment_before_the_next_field() {
        let payload = [1, 0, 0, 0, 0x7f, 0, 0, 0, 1];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Bytes", "vector", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "UInt8", 1, 0),
                node(1, "m_Active", "bool", 1, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].name, "m_Bytes");
        assert_eq!(
            fields[0].value,
            SerializedValue::Array(vec![SerializedValue::U8(0x7f)])
        );
        assert_eq!(fields[1].name, "m_Active");
        assert_eq!(fields[1].value, SerializedValue::Bool(true));
        assert_eq!(object.consumed_bytes, payload.len());
        assert!(object.trailing_bytes.is_empty());

        let mut shortened = metadata.decode_object_payload(&file, &header, 0).unwrap();
        *shortened
            .root
            .unique_field_in_base_chain_mut("m_Bytes")
            .unwrap()
            .unwrap() = SerializedValue::Array(Vec::new());
        let encoded = metadata
            .encode_object_payload(
                &shortened,
                ObjectEncodeLimits {
                    max_encoded_payload_bytes: 5,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(encoded, [0, 0, 0, 0, 1]);
        let data_start =
            usize::try_from(metadata.objects[0].data_range(&header).unwrap().start).unwrap();
        let mut edited_file = file.clone();
        edited_file[data_start..data_start + encoded.len()].copy_from_slice(&encoded);
        let mut edited_metadata = metadata.clone();
        edited_metadata.objects[0].byte_size = encoded.len() as u32;
        let decoded = edited_metadata
            .decode_object_payload(&edited_file, &header, 0)
            .unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "m_Bytes".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                },
                super::SerializedFieldValue {
                    name: "m_Active".to_owned(),
                    value: SerializedValue::Bool(true),
                },
            ])
        );
    }

    #[test]
    fn decodes_aligned_utf8_byte_strings_before_later_fields() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&3i32.to_le_bytes());
        payload.extend_from_slice(b"abc");
        payload.push(0); // string alignment to a four-byte boundary
        payload.extend_from_slice(&0xaabb_ccddu32.to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Name", "string", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "char", 1, 0),
                node(1, "m_Flags", "uint", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            payload
        );
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::StringBytes(b"abc".to_vec())
        );
        assert_eq!(fields[1].value, SerializedValue::U32(0xaabb_ccdd));
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn decodes_and_reencodes_utf8string_type_tree_nodes() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&5i32.to_le_bytes());
        payload.extend_from_slice(b"track");
        payload.extend_from_slice(&[0; 3]); // string alignment to a four-byte boundary
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Name", "Utf8String", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "char", 1, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            object.root,
            SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"track".to_vec()),
            }])
        );
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            payload
        );
    }

    #[test]
    fn honors_field_align_bytes_and_keeps_unknown_sized_leaves() {
        let payload = [7, 1, 0, 0, 0, 0, 0, 0, 0xaa, 0xbb];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_First", "UInt8", 1, 0),
                node(1, "m_Number", "int", 4, 0x4000),
                node(1, "m_Unknown", "VendorThing", 2, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            payload
        );
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].value, SerializedValue::U8(7));
        assert_eq!(fields[1].value, SerializedValue::I32(1));
        assert_eq!(
            fields[2].value,
            SerializedValue::Raw {
                type_name: "VendorThing".to_owned(),
                bytes: vec![0xaa, 0xbb],
            }
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn decodes_typed_pptr_nodes_without_losing_file_or_path_identity() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&2i32.to_le_bytes());
        payload.extend_from_slice(&0x1234_5678_9abc_def0i64.to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Target", "PPtr<$GameObject>", 12, 0),
                node(2, "m_FileID", "SInt32", 4, 0),
                node(2, "m_PathID", "SInt64", 8, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            object.root,
            SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Target".to_owned(),
                value: SerializedValue::PPtr {
                    file_id: 2,
                    path_id: 0x1234_5678_9abc_def0,
                },
            }])
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn sign_extends_legacy_32_bit_pptr_path_ids() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&(-1i32).to_le_bytes());
        payload.extend_from_slice(&(-42i32).to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "PPtr<Object>", "PPtr<Object>", 8, 0),
                node(1, "m_FileID", "SInt32", 4, 0),
                node(1, "m_PathID", "SInt32", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            object.root,
            SerializedValue::PPtr {
                file_id: -1,
                path_id: -42,
            }
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn decodes_nested_vector_struct_fields() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1.0f32.to_le_bytes());
        payload.extend_from_slice(&(-2.5f32).to_le_bytes());
        payload.extend_from_slice(&3.25f32.to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Position", "Vector3f", -1, 0),
                node(2, "x", "float", 4, 0),
                node(2, "y", "float", 4, 0),
                node(2, "z", "float", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "x".to_owned(),
                    value: SerializedValue::F32(1.0),
                },
                super::SerializedFieldValue {
                    name: "y".to_owned(),
                    value: SerializedValue::F32(-2.5),
                },
                super::SerializedFieldValue {
                    name: "z".to_owned(),
                    value: SerializedValue::F32(3.25),
                },
            ])
        );
    }

    #[test]
    fn honors_the_type_tree_integer_as_character_flag() {
        let payload = 0x0041u16.to_le_bytes();
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Character", "UInt16", 2, 0x0200_0000),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].value, SerializedValue::Char(0x0041));
    }

    #[test]
    fn decodes_object_payload_scalars_using_metadata_endianness() {
        let payload = [0x12, 0x34, 0x56, 0x78];
        let (file, mut header, mut metadata) = make_file(&payload);
        header.data_endian = Some(Endian::Big);
        metadata.prefix.body_endian = Endian::Big;
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Value", "int", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].value, SerializedValue::I32(0x1234_5678));
    }

    #[test]
    fn caller_decode_limits_reject_excessive_array_storage() {
        let payload = [2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Values", "vector", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "int", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let limits = super::ObjectDecodeLimits {
            max_array_elements: 1,
            ..super::ObjectDecodeLimits::default()
        };

        let error = metadata
            .decode_object_payload_with_limits(&file, &header, 0, &limits)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::DecodeLimitExceeded {
                requested: 2,
                limit: 1,
                ..
            }
        ));
    }

    #[test]
    fn aligns_each_inner_complex_array_when_the_field_requests_alignment() {
        let payload = [
            2, 0, 0, 0, // outer array count
            1, 0, 0, 0, // first inner count
            0x11, 0, 0, 0, // first byte plus inner-array padding
            1, 0, 0, 0, // second inner count
            0x22, 0, 0, 0, // second byte plus inner-array padding
        ];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Nested", "vector", -1, 0x4000),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "vector", -1, 0),
                node(4, "Array", "Array", -1, 0),
                node(5, "size", "int", 4, 0),
                node(5, "data", "Custom", -1, 0),
                node(6, "m_Value", "UInt8", 1, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::Array(vec![
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    super::SerializedFieldValue {
                        name: "m_Value".to_owned(),
                        value: SerializedValue::U8(0x11),
                    },
                ])]),
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    super::SerializedFieldValue {
                        name: "m_Value".to_owned(),
                        value: SerializedValue::U8(0x22),
                    },
                ])]),
            ])
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn object_read_flags_select_release_for_ordinary_game_files() {
        use super::TransferInstructionFlags;
        use crate::serialized_file::format_version::FormatVersion;

        // An ordinary platformed file reads as a game release.
        let flags = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(7),
            "/game/level0",
            "level0",
            crate::io::binary::Endian::Little,
        );
        assert_eq!(flags, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
        assert!(flags.is_release());
        // A generation without a platform field never takes the scene branch,
        // even with a NoTarget stand-in and a `.unity` path.
        let flags = super::serialized_file_object_read_flags(
            FormatVersion::Unknown7,
            None,
            "/game/Level.unity",
            "Level",
            crate::io::binary::Endian::Little,
        );
        assert_eq!(flags, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
    }

    #[test]
    fn object_read_flags_select_editor_scene_for_unity_paths_without_target() {
        use super::TransferInstructionFlags;
        use crate::io::binary::Endian;
        use crate::serialized_file::format_version::FormatVersion;

        let scene = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.unity",
            "Main",
            Endian::Little,
        );
        assert_eq!(
            scene,
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE
        );
        assert!(scene.is_editor_scene());
        assert!(!scene.is_release());
        // A NoTarget platform without a `.unity` path carries bare flags.
        let bare = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "/game/sharedassets0.assets",
            "sharedassets0.assets",
            Endian::Little,
        );
        assert_eq!(bare, TransferInstructionFlags::NONE);
        // The suffix check is ordinal and case-sensitive, like the source.
        let upper = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.UNITY",
            "Main",
            Endian::Little,
        );
        assert_eq!(upper, TransferInstructionFlags::NONE);
        let backup = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.unity.backup",
            "Main",
            Endian::Little,
        );
        assert_eq!(backup, TransferInstructionFlags::NONE);
        // A real platform target always reads as a game release.
        let release = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(7),
            "Assets/Scenes/Main.unity",
            "Main",
            Endian::Little,
        );
        assert_eq!(release, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
    }

    #[test]
    fn object_read_flags_mark_builtin_resource_files() {
        use super::TransferInstructionFlags;
        use crate::io::binary::Endian;
        use crate::serialized_file::format_version::FormatVersion;

        // Engine resources set the builtin bit on any generation.
        for name in [
            super::DEFAULT_RESOURCE_NAME_1,
            super::DEFAULT_RESOURCE_NAME_2,
            super::EDITOR_RESOURCE_NAME,
        ] {
            let flags = super::serialized_file_object_read_flags(
                FormatVersion::LargeFilesSupport,
                Some(7),
                "/game/resources.assets",
                name,
                Endian::Little,
            );
            assert_eq!(
                flags,
                TransferInstructionFlags::SERIALIZE_GAME_RELEASE
                    .with(TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE)
            );
            assert!(flags.is_builtin_resources());
        }
        // Builtin-extra names only count below generation 10.
        for name in [super::BUILTIN_EXTRA_NAME_1, super::BUILTIN_EXTRA_NAME_2] {
            let old = super::serialized_file_object_read_flags(
                FormatVersion::Unknown9,
                Some(super::BUILD_TARGET_NO_TARGET),
                "/game/extra.assets",
                name,
                Endian::Little,
            );
            assert!(old.is_builtin_resources());
            let cutoff = super::serialized_file_object_read_flags(
                FormatVersion::Unknown10,
                Some(7),
                "/game/extra.assets",
                name,
                Endian::Little,
            );
            assert!(!cutoff.is_builtin_resources());
            assert_eq!(cutoff, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
        }
        // Ordinary names never set the builtin bit.
        let plain = super::serialized_file_object_read_flags(
            FormatVersion::Unknown9,
            None,
            "/game/sharedassets0.assets",
            "sharedassets0.assets",
            Endian::Little,
        );
        assert!(!plain.is_builtin_resources());
    }

    #[test]
    fn object_read_flags_swap_endianness_for_big_endian_bodies() {
        use super::TransferInstructionFlags;
        use crate::io::binary::Endian;
        use crate::serialized_file::format_version::FormatVersion;

        let big = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(7),
            "/game/data.assets",
            "data.assets",
            Endian::Big,
        );
        assert_eq!(
            big,
            TransferInstructionFlags::SERIALIZE_GAME_RELEASE
                .with(TransferInstructionFlags::SWAP_ENDIANESS)
        );
        // The endian bit composes with the scene and builtin bits.
        let scene_big = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.unity",
            super::DEFAULT_RESOURCE_NAME_2,
            Endian::Big,
        );
        assert!(scene_big.is_editor_scene());
        assert!(scene_big.is_builtin_resources());
        assert_eq!(
            scene_big.0,
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE.0
                | TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE.0
                | TransferInstructionFlags::SWAP_ENDIANESS.0
        );
    }

    #[test]
    fn special_file_name_predicates_match_exact_names_only() {
        assert!(super::is_engine_resource_name("unity default resources"));
        assert!(super::is_engine_resource_name("unity_default_resources"));
        assert!(super::is_engine_resource_name("unity editor resources"));
        assert!(!super::is_engine_resource_name("unity builtin extra"));
        // Ordinal comparison: case and normalization matter.
        assert!(!super::is_engine_resource_name("Unity Default Resources"));
        assert!(!super::is_engine_resource_name(
            "library/unity default resources"
        ));
        assert!(!super::is_engine_resource_name(""));
        assert!(super::is_builtin_extra_name("unity builtin extra"));
        assert!(super::is_builtin_extra_name("unity_builtin_extra"));
        assert!(!super::is_builtin_extra_name("Unity Builtin Extra"));
        assert!(!super::is_builtin_extra_name("unity editor resources"));
    }

    fn object_field_fixture(case: &str) -> (Vec<u8>, String) {
        fn hex_to_bytes(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "odd hex length for {hex}");
            let mut out = Vec::with_capacity(hex.len() / 2);
            let bytes = hex.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let hi = match bytes[i] {
                    b'0'..=b'9' => bytes[i] - b'0',
                    b'a'..=b'f' => bytes[i] - b'a' + 10,
                    b'A'..=b'F' => bytes[i] - b'A' + 10,
                    b => panic!("invalid hex byte {b}"),
                };
                let lo = match bytes[i + 1] {
                    b'0'..=b'9' => bytes[i + 1] - b'0',
                    b'a'..=b'f' => bytes[i + 1] - b'a' + 10,
                    b'A'..=b'F' => bytes[i + 1] - b'A' + 10,
                    b => panic!("invalid hex byte {b}"),
                };
                out.push((hi << 4) | lo);
                i += 2;
            }
            out
        }

        // Probe Temp/opencode/objfieldprobe (SerializableTreeType.FromRootNode
        // plus SerializableStructure.Read over C#-constructed gen12 files with
        // ITypeResolver.Null, SDK 10.0.401). Nothing Rust-generated.
        let fixture = include_str!("object_field_fixture.txt");
        let mut file = None;
        let mut dump = None;
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("OBJ_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad OBJ_HEX {line}");
                if parts[1] == case {
                    file = Some(hex_to_bytes(parts[2]));
                }
            } else if line.starts_with("OBJ_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad OBJ_READ {line}");
                if parts[1] == case {
                    dump = Some(parts[2].to_string());
                }
            }
        }
        (
            file.unwrap_or_else(|| panic!("missing {case} hex")),
            dump.unwrap_or_else(|| panic!("missing {case} dump")),
        )
    }

    fn decode_fixture_object(case: &str, expected_dump: &str) -> super::ParsedObjectPayload {
        let (bytes, dump) = object_field_fixture(case);
        assert!(dump.contains(expected_dump), "{case} oracle {dump}");
        let header =
            SerializedFileHeader::parse(&bytes).unwrap_or_else(|e| panic!("{case} header: {e:?}"));
        let metadata = SerializedFileMetadata::parse_file(&bytes, &header)
            .unwrap_or_else(|e| panic!("{case} metadata: {e:?}"));
        metadata
            .decode_object_payload(&bytes, &header, 0)
            .unwrap_or_else(|e| panic!("{case} decode: {e:?}"))
    }

    fn require_field<'a>(root: &'a SerializedValue, name: &str) -> &'a SerializedValue {
        root.unique_field(name)
            .unwrap_or_else(|e| panic!("{name} lookup: {e:?}"))
            .unwrap_or_else(|| panic!("missing field {name}"))
    }

    #[test]
    fn object_scalar_string_nested_fields_match_reference() {
        let payload = decode_fixture_object(
            "F1",
            "INT=305419896|FLT=1.5|STR=hi|NEST=-5,2.5|INTS=10,20,30|BYTES=1,2,3,4,5",
        );
        assert_eq!(payload.path_id, 7001);
        let root = &payload.root;
        assert_eq!(
            require_field(root, "m_Int"),
            &SerializedValue::I32(0x12345678)
        );
        assert_eq!(
            require_field(root, "m_Float"),
            &SerializedValue::F32(1.5f32)
        );
        assert_eq!(
            require_field(root, "m_String"),
            &SerializedValue::StringBytes(b"hi".to_vec())
        );
        let nested = require_field(root, "m_Nested");
        let nested_fields = nested.struct_fields().expect("m_Nested is a struct");
        assert_eq!(nested_fields.len(), 2);
        assert_eq!(nested_fields[0].name, "a");
        assert_eq!(nested_fields[0].value, SerializedValue::I32(-5));
        assert_eq!(nested_fields[1].name, "b");
        assert_eq!(nested_fields[1].value, SerializedValue::F32(2.5f32));
        assert_eq!(
            require_field(root, "m_Ints"),
            &SerializedValue::Array(vec![
                SerializedValue::I32(10),
                SerializedValue::I32(20),
                SerializedValue::I32(30),
            ])
        );
        assert_eq!(
            require_field(root, "m_Bytes"),
            &SerializedValue::Array(vec![
                SerializedValue::U8(1),
                SerializedValue::U8(2),
                SerializedValue::U8(3),
                SerializedValue::U8(4),
                SerializedValue::U8(5),
            ])
        );
        // The byte vector carries AlignBytes (0x4000) with pad on both
        // sides, so no bytes remain after the last field either.
        assert!(payload.trailing_bytes.is_empty());
    }

    #[test]
    fn object_string_nested_empty_arrays_match_reference() {
        let payload = decode_fixture_object("F2", "STRS=a,bb,ccc|MAT=1,2;3|EMPTY=0");
        assert_eq!(payload.path_id, 7001);
        let root = &payload.root;
        assert_eq!(
            require_field(root, "m_Strings"),
            &SerializedValue::Array(vec![
                SerializedValue::StringBytes(b"a".to_vec()),
                SerializedValue::StringBytes(b"bb".to_vec()),
                SerializedValue::StringBytes(b"ccc".to_vec()),
            ])
        );
        assert_eq!(
            require_field(root, "m_Matrix"),
            &SerializedValue::Array(vec![
                SerializedValue::Array(vec![SerializedValue::I32(1), SerializedValue::I32(2),]),
                SerializedValue::Array(vec![SerializedValue::I32(3)]),
            ])
        );
        assert_eq!(
            require_field(root, "m_Empty"),
            &SerializedValue::Array(vec![])
        );
        assert!(payload.trailing_bytes.is_empty());
    }

    #[test]
    fn object_encode_reproduces_reference_authored_payload() {
        // The expected bytes below are the C#-constructed F1 payload from
        // object_field_fixture.txt (reference-readable, never Rust-encoded).
        // Re-encoding the decoded values must reproduce them exactly.
        let (bytes, _) = object_field_fixture("F1");
        let header = SerializedFileHeader::parse(&bytes).expect("F1 header");
        let metadata = SerializedFileMetadata::parse_file(&bytes, &header).expect("F1 metadata");
        let object = &metadata.objects[0];
        let range = object.data_range(&header).expect("F1 range");
        let original: Vec<u8> = bytes[range.start as usize..range.end as usize].to_vec();
        assert_eq!(original.len(), 52);

        let payload = metadata
            .decode_object_payload(&bytes, &header, 0)
            .expect("F1 decode");
        let encoded = metadata
            .encode_object_payload(&payload, ObjectEncodeLimits::default())
            .expect("F1 encode");
        assert_eq!(encoded, original);

        // Mutating scalar and variable-length fields re-encodes with the
        // reference layout (little-endian int, length-prefixed aligned
        // string) while the untouched tail stays byte-identical.
        let mut mutated = payload;
        *mutated
            .root
            .unique_field_in_base_chain_mut("m_Int")
            .expect("m_Int lookup")
            .expect("m_Int present") = SerializedValue::I32(0x0102_0304);
        *mutated
            .root
            .unique_field_in_base_chain_mut("m_String")
            .expect("m_String lookup")
            .expect("m_String present") = SerializedValue::StringBytes(b"hello!".to_vec());
        let recoded = metadata
            .encode_object_payload(&mutated, ObjectEncodeLimits::default())
            .expect("F1 recode");
        // "hi" section (8 bytes) becomes "hello!" (12 bytes): +4 overall.
        // Layout is int, float, string, nested, vectors: the float sits
        // between the mutated int and the grown string.
        assert_eq!(recoded.len(), 56);
        assert_eq!(recoded[0..4], [0x04, 0x03, 0x02, 0x01]);
        assert_eq!(recoded[4..8], [0x00, 0x00, 0xC0, 0x3F]);
        assert_eq!(recoded[8..12], [0x06, 0x00, 0x00, 0x00]);
        assert_eq!(recoded[12..18], *b"hello!");
        assert_eq!(recoded[18..20], [0x00, 0x00]);
        // Nested struct onwards shifts by 4 but is otherwise unchanged.
        assert_eq!(recoded[20..], original[16..]);
    }
