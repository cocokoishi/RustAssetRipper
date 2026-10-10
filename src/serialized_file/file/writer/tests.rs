use super::bytes::{patch_header, patch_object_records, serialized_header_size};
use super::metadata::build_metadata_with_insertions;
use super::model::{
    MetadataInsertionContext, MetadataInsertions, SerializedExternalFileInsertion,
    SerializedObjectInsertion,
};
use crate::io::binary::Endian;
use crate::serialized_file::format_version::FormatVersion;
use crate::serialized_file::header::SerializedFileHeader;
use crate::serialized_file::metadata::{
    ExternalFileTableLayout, FileIdentifier, ObjectInfo, ObjectRecordLayout,
    ObjectRecordTableLayout, SerializedFileMetadata,
};
use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
use crate::serialized_file::object::SerializedValue;

fn make_insertion(version: FormatVersion, path_id: i64) -> SerializedObjectInsertion {
    SerializedObjectInsertion {
        object: ObjectInfo {
            path_id,
            byte_start: 123,
            byte_size: 456,
            serialized_type_index: (version.raw() >= 17).then_some(0),
            raw_type_id: 1,
            class_id: (version.raw() < 16).then_some(1),
            script_type_index: (11..17).contains(&version.raw()).then_some(0),
            is_destroyed: (version.raw() < 11).then_some(0),
            is_stripped: (15..17).contains(&version.raw()).then_some(false),
            resolved_type_index: Some(0),
        },
        root: SerializedValue::I32(7),
        trailing_bytes: Vec::new(),
    }
}

#[test]
fn writes_extended_generation_22_header_and_64_bit_object_offsets() {
    let version = FormatVersion::LargeFilesSupport;
    let header_size = serialized_header_size(version);
    assert_eq!(header_size, 48);
    let mut header_bytes = [0; 48];
    patch_header(
        &mut header_bytes,
        version,
        Endian::Big,
        123,
        456,
        64,
        header_size,
    )
    .unwrap();
    let header = SerializedFileHeader::parse(&header_bytes).unwrap();
    assert_eq!(header.metadata_size, 123);
    assert_eq!(header.file_size, 456);
    assert_eq!(header.data_offset, 64);
    assert_eq!(header.data_endian, Some(Endian::Big));
    assert_eq!(header.reserved_v22, Some(0));

    let layout = [ObjectRecordLayout {
        path_id: 99,
        data_offset_field_offset: 0,
        data_size_field_offset: 8,
    }];
    let offset = u64::from(u32::MAX) + 1;
    let mut metadata_bytes = [0; 12];
    let metadata_size = metadata_bytes.len();
    patch_object_records(
        &mut metadata_bytes,
        0,
        metadata_size,
        &layout,
        &[(offset, 13)],
        version,
        Endian::Big,
    )
    .unwrap();
    assert_eq!(
        i64::from_be_bytes(metadata_bytes[..8].try_into().unwrap()),
        i64::try_from(offset).unwrap()
    );
    assert_eq!(
        i32::from_be_bytes(metadata_bytes[8..12].try_into().unwrap()),
        13
    );
}

#[test]
fn writes_inserted_record_fields_for_aligned_and_extended_generations() {
    for (version, expected_record_size, offset_width, expected_type_field) in [
        (FormatVersion::Unknown14, 24, 4, 1),
        (FormatVersion::SupportsStrippedObject, 25, 4, 1),
        (FormatVersion::RefactorTypeData, 20, 4, 0),
        (FormatVersion::LargeFilesSupport, 24, 8, 0),
    ] {
        let source = [0, 0, 0, 0, 0, 0, 0, 0, 0xca, 0xfe];
        let table = ObjectRecordTableLayout {
            count_field_offset: 0,
            records_start_offset: 4,
            records_end_offset: 4,
            records: Vec::new(),
        };
        let external_table = ExternalFileTableLayout {
            count_field_offset: 4,
            records_start_offset: 8,
            records_end_offset: 8,
        };
        let identifier = FileIdentifier {
            asset_path: Some("Assets/Generated.asset".to_owned()),
            guid: Some([0x0102_0304, 0x1112_1314, 0x2122_2324, 0x3132_3334]),
            file_type: Some(7),
            path_name_origin: "Library/Generated.asset".to_owned(),
        };
        let mut expected_external_record =
            identifier.asset_path.as_ref().unwrap().as_bytes().to_vec();
        expected_external_record.push(0);
        for word in identifier.guid.unwrap() {
            expected_external_record.extend_from_slice(&word.to_be_bytes());
        }
        expected_external_record.extend_from_slice(&7_i32.to_be_bytes());
        expected_external_record.extend_from_slice(identifier.path_name_origin.as_bytes());
        expected_external_record.push(0);
        let metadata = SerializedFileMetadata {
            prefix: SerializedFileMetadataPrefix {
                metadata_offset: 0,
                metadata_size: source.len(),
                body_endian: Endian::Big,
                unity_version_text: None,
                target_platform: None,
                has_type_tree: true,
                consumed_bytes: 0,
            },
            types: Vec::new(),
            long_file_id: None,
            objects: Vec::new(),
            script_types: Vec::new(),
            externals: Vec::new(),
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        };
        let insertion = make_insertion(version, 0x0102_0304_0506_0708);
        let (metadata, layouts) = build_metadata_with_insertions(
            &source,
            &table,
            &external_table,
            &metadata,
            MetadataInsertions {
                objects: &[insertion],
                external_files: &[SerializedExternalFileInsertion {
                    file_identifier: identifier,
                }],
            },
            MetadataInsertionContext {
                version,
                endian: Endian::Big,
                metadata_file_offset: 20,
                output_limit: 1024,
            },
        )
        .unwrap();

        assert_eq!(&metadata[..4], &1_i32.to_be_bytes());
        let external_count_field = 4 + expected_record_size;
        assert_eq!(
            i32::from_be_bytes(
                metadata[external_count_field..external_count_field + 4]
                    .try_into()
                    .unwrap()
            ),
            1
        );
        let external_record_start = external_count_field + 4;
        let external_record_end = external_record_start + expected_external_record.len();
        assert_eq!(
            &metadata[external_record_start..external_record_end],
            expected_external_record
        );
        assert_eq!(
            metadata.len(),
            source.len() + expected_record_size + expected_external_record.len()
        );
        assert_eq!(&metadata[external_record_end..], &[0xca, 0xfe]);
        assert_eq!(layouts.len(), 1);
        let layout = layouts[0];
        assert_eq!(layout.path_id, 0x0102_0304_0506_0708);
        assert_eq!(layout.data_offset_field_offset, 12);
        assert_eq!(layout.data_size_field_offset, 12 + offset_width);
        assert_eq!(
            &metadata
                [layout.data_offset_field_offset..layout.data_offset_field_offset + offset_width],
            &[0; 8][..offset_width]
        );
        assert_eq!(
            i32::from_be_bytes(
                metadata[layout.data_size_field_offset..layout.data_size_field_offset + 4]
                    .try_into()
                    .unwrap()
            ),
            0
        );
        let type_field_offset = layout.data_size_field_offset + 4;
        let actual_type_field = i32::from_be_bytes(
            metadata[type_field_offset..type_field_offset + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(actual_type_field, expected_type_field);
    }
}
