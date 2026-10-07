use super::format_version::FormatVersion;
use super::header::SerializedFileHeader;
use super::metadata::{
    ObjectInfo, SerializedFileMetadata, SerializedType, SerializedTypeReference, TypeTree,
    TypeTreeNode,
};
use super::metadata_prefix::SerializedFileMetadataPrefix;
use crate::io::binary::Endian;

pub(crate) struct ManagedReferenceFixture {
    pub file: Vec<u8>,
    pub header: SerializedFileHeader,
    pub metadata: SerializedFileMetadata,
}

pub(crate) fn managed_reference_fixture() -> ManagedReferenceFixture {
    managed_reference_fixture_with_format(false)
}

pub(crate) fn terminated_managed_reference_fixture() -> ManagedReferenceFixture {
    managed_reference_fixture_with_format(true)
}

/// A source-runtime-authored payload with two registries that deliberately use the same ID.
pub(crate) fn nested_scope_managed_reference_fixture() -> ManagedReferenceFixture {
    nested_scope_managed_reference_fixture_with_format(false)
}

pub(crate) fn nested_scope_terminated_managed_reference_fixture() -> ManagedReferenceFixture {
    nested_scope_managed_reference_fixture_with_format(true)
}

fn nested_scope_managed_reference_fixture_with_format(indexed: bool) -> ManagedReferenceFixture {
    let fixture = if indexed {
        include_str!("managed_reference_nested_legacy_fixture.txt")
    } else {
        include_str!("managed_reference_nested_fixture.txt")
    };
    let payload_record = fixture
        .lines()
        .find(|line| line.starts_with("PAYLOAD|"))
        .expect("nested source fixture has a PAYLOAD record");
    let mut record = payload_record.split('|');
    assert_eq!(record.next(), Some("PAYLOAD"));
    let expected_length = record
        .next()
        .expect("nested source fixture length")
        .parse::<usize>()
        .expect("nested source fixture length is a number");
    let hex = record.next().expect("nested source fixture hex");
    assert!(
        record.next().is_none(),
        "nested source fixture has extra fields"
    );
    assert_eq!(
        hex.len() % 2,
        0,
        "nested source fixture has even hex length"
    );
    let payload = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("fixture hex is ASCII");
            u8::from_str_radix(text, 16).expect("fixture hex digits are valid")
        })
        .collect::<Vec<_>>();
    assert_eq!(payload.len(), expected_length);

    let (id_type, id_name, id_size) = if indexed {
        ("SInt32", "id", 4)
    } else {
        ("SInt64", "rid", 8)
    };
    let root_tree = TypeTree {
        nodes: vec![
            node(0, "Root", "Root", -1),
            node(1, "m_Root", "managedReference", id_size),
            node(2, id_name, id_type, id_size),
            node(1, "references", "ManagedReferencesRegistry", -1),
            node(2, "version", "int", 4),
            node(2, "RefIds", "vector", -1),
            node(3, "Array", "Array", -1),
            node(4, "size", "int", 4),
            node(4, "data", "ReferencedObjectData", -1),
        ],
        string_buffer: Vec::new(),
    };
    let node_a = reference_type(
        "NodeA",
        false,
        vec![
            node(0, "NodeA", "NodeA", -1),
            node(1, "m_Link", "managedReference", id_size),
            node(2, id_name, id_type, id_size),
            node(1, "m_Nested", "NestedState", -1),
            node(2, "m_NestedLink", "managedReference", id_size),
            node(3, id_name, id_type, id_size),
            node(2, "references", "ManagedReferencesRegistry", -1),
            node(3, "version", "int", 4),
            node(3, "RefIds", "vector", -1),
            node(4, "Array", "Array", -1),
            node(5, "size", "int", 4),
            node(5, "data", "ReferencedObjectData", -1),
        ],
    );
    let node_b = reference_type(
        "NodeB",
        false,
        vec![
            node(0, "NodeB", "NodeB", -1),
            node(1, "m_Link", "managedReference", id_size),
            node(2, id_name, id_type, id_size),
        ],
    );

    let mut file_metadata = Vec::new();
    file_metadata.extend_from_slice(b"2022.3.1f1\0");
    file_metadata.extend_from_slice(&0u32.to_le_bytes());
    file_metadata.push(1);
    let metadata_offset = 20usize;
    let data_offset = metadata_offset + file_metadata.len();
    let file_size = data_offset + payload.len();
    let mut file = vec![0; 20];
    file.extend_from_slice(&file_metadata);
    file.extend_from_slice(&payload);
    let header = SerializedFileHeader {
        metadata_size: file_metadata.len() as u64,
        file_size: file_size as u64,
        format_version: FormatVersion::Unknown9,
        data_offset: data_offset as u64,
        data_endian: Some(Endian::Little),
        reserved_v22: None,
        header_size: 20,
    };
    let metadata = SerializedFileMetadata {
        prefix: SerializedFileMetadataPrefix {
            metadata_offset,
            metadata_size: file_metadata.len(),
            body_endian: Endian::Little,
            unity_version_text: Some("2022.3.1f1".to_owned()),
            target_platform: Some(0),
            has_type_tree: true,
            consumed_bytes: 16,
        },
        types: vec![SerializedType {
            raw_type_id: 1,
            type_id_for_hash: 1,
            is_stripped_type: false,
            script_type_index: -1,
            script_id: None,
            old_type_hash: None,
            type_tree: Some(root_tree),
            type_dependencies: Vec::new(),
        }],
        long_file_id: None,
        objects: vec![ObjectInfo {
            path_id: 501,
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
        reference_types: vec![node_a, node_b],
        user_information: None,
        trailing_metadata: Vec::new(),
        consumed_bytes: 16,
    };
    ManagedReferenceFixture {
        file,
        header,
        metadata,
    }
}

fn managed_reference_fixture_with_format(terminated: bool) -> ManagedReferenceFixture {
    let mut payload = Vec::new();
    if terminated {
        append_i32(&mut payload, 0);
        append_i32(&mut payload, 0);
        append_i32(&mut payload, 1);
        append_i32(&mut payload, 1); // terminated registry format
    } else {
        append_i64(&mut payload, 7);
        append_i64(&mut payload, 7);
        append_i64(&mut payload, 9);
        append_i32(&mut payload, 2); // counted registry format
        append_i32(&mut payload, 2); // two entries
        append_i64(&mut payload, 7);
    }
    append_registry_string(&mut payload, "NodeA");
    append_registry_string(&mut payload, "Example");
    append_registry_string(&mut payload, "Assembly-CSharp");
    append_i32(&mut payload, 100);
    if terminated {
        append_i32(&mut payload, 1);
    } else {
        append_i64(&mut payload, 9);
    }
    append_i32(&mut payload, 1);
    append_i32(&mut payload, 42);

    if !terminated {
        append_i64(&mut payload, 9);
    }
    append_registry_string(&mut payload, "NodeB");
    append_registry_string(&mut payload, "Example");
    append_registry_string(&mut payload, "Assembly-CSharp");
    append_i32(&mut payload, 200);
    if terminated {
        append_i32(&mut payload, 0);
    } else {
        append_i64(&mut payload, 7);
    }
    append_i32(&mut payload, 0);
    append_i32(&mut payload, 300);
    if terminated {
        append_registry_string(&mut payload, "Terminus");
        append_registry_string(&mut payload, "UnityEngine.DMAT");
        append_registry_string(&mut payload, "FAKE_ASM");
    }
    append_i32(&mut payload, 0x1234_5678);

    let (outer_id_type, outer_id_name, outer_id_size) = if terminated {
        ("SInt32", "id", 4)
    } else {
        ("SInt64", "rid", 8)
    };

    let root_tree = TypeTree {
        nodes: vec![
            node(0, "Root", "Root", -1),
            node(1, "m_First", "managedReference", outer_id_size),
            node(2, outer_id_name, outer_id_type, outer_id_size),
            node(1, "m_Shared", "managedReference", outer_id_size),
            node(2, outer_id_name, outer_id_type, outer_id_size),
            node(1, "m_Second", "managedReference", outer_id_size),
            node(2, outer_id_name, outer_id_type, outer_id_size),
            node(1, "references", "ManagedReferencesRegistry", -1),
            node(2, "version", "int", 4),
            node(2, "RefIds", "vector", -1),
            node(3, "Array", "Array", -1),
            node(4, "size", "int", 4),
            node(4, "data", "ReferencedObjectData", -1),
            node(1, "m_Tail", "int", 4),
        ],
        string_buffer: Vec::new(),
    };
    let node_a = reference_type(
        "NodeA",
        terminated,
        vec![
            node(0, "NodeA", "NodeA", -1),
            node(1, "m_Value", "int", 4),
            node(1, "m_Link", "managedReference", 8),
            node(2, "rid", "SInt64", 8),
            node(1, "m_Asset", "PPtr<$Object>", 8),
            node(2, "m_FileID", "SInt32", 4),
            node(2, "m_PathID", "SInt32", 4),
        ],
    );
    let node_b = reference_type(
        "NodeB",
        terminated,
        vec![
            node(0, "NodeB", "NodeB", -1),
            node(1, "m_Value", "int", 4),
            node(1, "m_Link", "managedReference", 8),
            node(2, "rid", "SInt64", 8),
            node(1, "m_Asset", "PPtr<$Object>", 8),
            node(2, "m_FileID", "SInt32", 4),
            node(2, "m_PathID", "SInt32", 4),
        ],
    );

    let mut file_metadata = Vec::new();
    file_metadata.extend_from_slice(b"2022.3.1f1\0");
    file_metadata.extend_from_slice(&0u32.to_le_bytes());
    file_metadata.push(1);
    let metadata_offset = 20usize;
    let data_offset = metadata_offset + file_metadata.len();
    let file_size = data_offset + payload.len();
    let mut file = vec![0; 20];
    file.extend_from_slice(&file_metadata);
    file.extend_from_slice(&payload);
    let header = SerializedFileHeader {
        metadata_size: file_metadata.len() as u64,
        file_size: file_size as u64,
        format_version: FormatVersion::Unknown9,
        data_offset: data_offset as u64,
        data_endian: Some(Endian::Little),
        reserved_v22: None,
        header_size: 20,
    };
    let metadata = SerializedFileMetadata {
        prefix: SerializedFileMetadataPrefix {
            metadata_offset,
            metadata_size: file_metadata.len(),
            body_endian: Endian::Little,
            unity_version_text: Some("2022.3.1f1".to_owned()),
            target_platform: Some(0),
            has_type_tree: true,
            consumed_bytes: 16,
        },
        types: vec![SerializedType {
            raw_type_id: 1,
            type_id_for_hash: 1,
            is_stripped_type: false,
            script_type_index: -1,
            script_id: None,
            old_type_hash: None,
            type_tree: Some(root_tree),
            type_dependencies: Vec::new(),
        }],
        long_file_id: None,
        objects: vec![ObjectInfo {
            path_id: 500,
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
        reference_types: vec![node_a, node_b],
        user_information: None,
        trailing_metadata: Vec::new(),
        consumed_bytes: 16,
    };
    ManagedReferenceFixture {
        file,
        header,
        metadata,
    }
}

fn reference_type(
    class_name: &str,
    indexed: bool,
    mut nodes: Vec<TypeTreeNode>,
) -> SerializedTypeReference {
    if indexed {
        nodes[2].byte_size = 4;
        nodes[3].name = Some("id".to_owned());
        nodes[3].type_name = Some("SInt32".to_owned());
        nodes[3].byte_size = 4;
    }
    SerializedTypeReference {
        raw_type_id: 114,
        type_id_for_hash: 114,
        is_stripped_type: false,
        script_type_index: -1,
        script_id: None,
        old_type_hash: None,
        type_tree: Some(TypeTree {
            nodes,
            string_buffer: Vec::new(),
        }),
        class_name: Some(class_name.to_owned()),
        namespace: Some("Example".to_owned()),
        assembly_name: Some("Assembly-CSharp".to_owned()),
    }
}

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

fn append_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn append_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn append_registry_string(bytes: &mut Vec<u8>, value: &str) {
    append_i32(bytes, value.len() as i32);
    bytes.extend_from_slice(value.as_bytes());
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
}
