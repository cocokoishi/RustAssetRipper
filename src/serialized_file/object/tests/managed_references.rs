use super::test_support::{make_file, node};
use super::{ObjectDecodeErrorKind, ObjectEncodeLimits, SerializedValue};
use crate::serialized_file::metadata::TypeTree;
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
