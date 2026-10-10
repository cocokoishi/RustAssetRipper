use super::test_support::{element_layout, layout_node, make_file, node};
use super::{ObjectDecodeErrorKind, ObjectEncodeErrorKind, ObjectEncodeLimits, SerializedValue};
use crate::io::binary::Endian;
use crate::serialized_file::metadata::TypeTree;
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
fn large_payload_measurements_justify_decode_defaults() {
    use crate::io::binary::BinaryReader;
    use std::time::Instant;

    // Representative large-file shapes must decode inside the product
    // defaults (256 MB allocation, 16 M array elements, 64 MB strings,
    // depth 256). Timings are reported for the record; success (not
    // speed) is asserted.
    let limits = super::ObjectDecodeLimits::default();

    // 2M-element int vector (8 MB payload).
    {
        let layout = element_layout(vec![
            layout_node(0, "m_Large", "vector"),
            layout_node(1, "Array", "Array"),
            layout_node(2, "size", "int"),
            layout_node(2, "data", "int"),
        ]);
        let count = 2_000_000usize;
        let mut payload = Vec::with_capacity(4 + count * 4);
        payload.extend_from_slice(&(count as i32).to_le_bytes());
        payload.extend(std::iter::repeat_n(0u8, count * 4));
        let mut budget = super::DecodeBudget {
            limits: &limits,
            allocated_bytes: 0,
            managed_type_references: &[],
            managed_reference_type_resolver: None,
            stream_offset: 0,
            managed_registry_depth: 0,
        };
        let mut reader = BinaryReader::new(&payload, Endian::Little);
        let start = Instant::now();
        let field = super::decode_field(&mut reader, &layout, 0, false, 0, "", 0, &mut budget)
            .expect("2M int vector decodes");
        let elapsed = start.elapsed();
        match field.value {
            SerializedValue::Array(elements) => assert_eq!(elements.len(), count),
            value => panic!("expected array, got {value:?}"),
        }
        println!(
            "LIMITS_MEASURE int-vector-2M elapsed_ms={}",
            elapsed.as_millis()
        );
    }

    // 8 MB string (length-prefixed, 4-aligned).
    {
        let layout = element_layout(vec![
            layout_node(0, "m_Big", "string"),
            layout_node(1, "Array", "Array"),
            layout_node(2, "size", "int"),
            layout_node(2, "data", "char"),
        ]);
        let length = 8_000_000usize;
        let mut payload = Vec::with_capacity(4 + length + 4);
        payload.extend_from_slice(&(length as i32).to_le_bytes());
        payload.extend(std::iter::repeat_n(b'x', length));
        while payload.len() % 4 != 0 {
            payload.push(0);
        }
        let mut budget = super::DecodeBudget {
            limits: &limits,
            allocated_bytes: 0,
            managed_type_references: &[],
            managed_reference_type_resolver: None,
            stream_offset: 0,
            managed_registry_depth: 0,
        };
        let mut reader = BinaryReader::new(&payload, Endian::Little);
        let start = Instant::now();
        let field = super::decode_field(&mut reader, &layout, 0, false, 0, "", 0, &mut budget)
            .expect("8MB string decodes");
        let elapsed = start.elapsed();
        match field.value {
            SerializedValue::StringBytes(bytes) => assert_eq!(bytes.len(), length),
            value => panic!("expected string, got {value:?}"),
        }
        println!(
            "LIMITS_MEASURE string-8M elapsed_ms={}",
            elapsed.as_millis()
        );
    }

    // 65-level struct chain (root plus 64 nested, well under the 256 depth cap).
    {
        let mut nodes = vec![layout_node(0, "m_Root", "S0")];
        for depth in 1..=64 {
            nodes.push(layout_node(
                depth as u8,
                Box::leak(format!("m_D{depth}").into_boxed_str()),
                Box::leak(format!("S{depth}").into_boxed_str()),
            ));
        }
        nodes.push(layout_node(65, "m_Value", "int"));
        let layout = element_layout(nodes);
        let payload = 12345i32.to_le_bytes().to_vec();
        let mut budget = super::DecodeBudget {
            limits: &limits,
            allocated_bytes: 0,
            managed_type_references: &[],
            managed_reference_type_resolver: None,
            stream_offset: 0,
            managed_registry_depth: 0,
        };
        let mut reader = BinaryReader::new(&payload, Endian::Little);
        let start = Instant::now();
        let field = super::decode_field(&mut reader, &layout, 0, false, 0, "", 0, &mut budget)
            .expect("64-deep nesting decodes");
        let elapsed = start.elapsed();
        let mut scope = &field.value;
        for _ in 0..65 {
            let fields = scope.struct_fields().expect("nested struct");
            assert_eq!(fields.len(), 1);
            scope = &fields[0].value;
        }
        assert_eq!(scope, &SerializedValue::I32(12345));
        println!(
            "LIMITS_MEASURE nesting-64 elapsed_ms={}",
            elapsed.as_millis()
        );
    }
}
