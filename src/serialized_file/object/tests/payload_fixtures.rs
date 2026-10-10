use super::test_support::{decode_fixture_object, object_field_fixture, require_field};
use super::{ObjectEncodeLimits, SerializedValue};
use crate::serialized_file::header::SerializedFileHeader;
use crate::serialized_file::metadata::SerializedFileMetadata;
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
