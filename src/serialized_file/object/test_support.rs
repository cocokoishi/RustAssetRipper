use super::SerializedValue;
use crate::io::binary::Endian;
use crate::serialized_file::format_version::FormatVersion;
use crate::serialized_file::header::SerializedFileHeader;
use crate::serialized_file::metadata::{SerializedFileMetadata, TypeTree, TypeTreeNode};
pub(super) fn layout_node(level: u8, name: &str, type_name: &str) -> TypeTreeNode {
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

pub(super) fn element_layout(nodes: Vec<TypeTreeNode>) -> super::TypeTreeLayout<'static> {
    // The layout borrows the tree; leak the box so the test can hold it.
    let tree: &'static TypeTree = Box::leak(Box::new(TypeTree {
        nodes,
        string_buffer: Vec::new(),
    }));
    super::TypeTreeLayout::new(tree, 0, 0).unwrap()
}

pub(super) fn node(
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

pub(super) fn make_file(payload: &[u8]) -> (Vec<u8>, SerializedFileHeader, SerializedFileMetadata) {
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

pub(super) fn object_field_fixture(case: &str) -> (Vec<u8>, String) {
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
    let fixture = include_str!("../object_field_fixture.txt");
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

pub(super) fn decode_fixture_object(case: &str, expected_dump: &str) -> super::ParsedObjectPayload {
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

pub(super) fn require_field<'a>(root: &'a SerializedValue, name: &str) -> &'a SerializedValue {
    root.unique_field(name)
        .unwrap_or_else(|e| panic!("{name} lookup: {e:?}"))
        .unwrap_or_else(|| panic!("missing field {name}"))
}
