use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::header::{HeaderError, SerializedFileHeader};
use super::metadata::{
    ExternalFileTableLayout, ObjectRecordTableLayout, SerializedFileMetadata,
    SerializedFileMetadataError,
};
use super::object::{ObjectDecodeError, ObjectDecodeLimits, ParsedObjectPayload};

mod writer;
pub use writer::{
    SerializedExternalFileInsertion, SerializedFileRewriteError, SerializedFileRewriteErrorKind,
    SerializedFileRewriteLimits, SerializedObjectInsertion,
};

/// A read-only view of one complete Unity SerializedFile.
///
/// The original input remains borrowed and unchanged. Header, metadata, and TypeTree-backed
/// object payloads are parsed from this same byte slice, preserving their file scope and offsets.
#[derive(Debug)]
pub struct SerializedFile<'a> {
    input: &'a [u8],
    object_record_layout: ObjectRecordTableLayout,
    external_file_table_layout: ExternalFileTableLayout,
    pub header: SerializedFileHeader,
    pub metadata: SerializedFileMetadata,
}

impl<'a> SerializedFile<'a> {
    /// Parse the header and all metadata tables from one complete SerializedFile input.
    pub fn parse(input: &'a [u8]) -> Result<Self, SerializedFileError> {
        let header =
            SerializedFileHeader::parse(input).map_err(SerializedFileError::from_header)?;
        let (metadata, rewrite_layouts) =
            SerializedFileMetadata::parse_file_with_rewrite_layouts(input, &header)
                .map_err(SerializedFileError::from_metadata)?;
        Ok(Self {
            input,
            object_record_layout: rewrite_layouts.object_records,
            external_file_table_layout: rewrite_layouts.external_files,
            header,
            metadata,
        })
    }

    /// Return the borrowed original bytes. Parsing never modifies the caller's input.
    pub fn input(&self) -> &'a [u8] {
        self.input
    }

    /// Decode one object payload using the default per-object allocation limits.
    pub fn decode_object_payload(
        &self,
        object_index: usize,
    ) -> Result<ParsedObjectPayload, SerializedFileError> {
        self.metadata
            .decode_object_payload(self.input, &self.header, object_index)
            .map_err(SerializedFileError::from_object)
    }

    /// Decode one object payload with caller-selected allocation and nesting limits.
    pub fn decode_object_payload_with_limits(
        &self,
        object_index: usize,
        limits: &ObjectDecodeLimits,
    ) -> Result<ParsedObjectPayload, SerializedFileError> {
        self.metadata
            .decode_object_payload_with_limits(self.input, &self.header, object_index, limits)
            .map_err(SerializedFileError::from_object)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedFileError {
    offset: usize,
    kind: SerializedFileErrorKind,
}

impl SerializedFileError {
    fn from_header(error: HeaderError) -> Self {
        let offset = error.offset();
        Self {
            offset,
            kind: SerializedFileErrorKind::Header(error),
        }
    }

    fn from_metadata(error: SerializedFileMetadataError) -> Self {
        let offset = error.offset();
        Self {
            offset,
            kind: SerializedFileErrorKind::Metadata(error),
        }
    }

    fn from_object(error: ObjectDecodeError) -> Self {
        let offset = error.offset();
        Self {
            offset,
            kind: SerializedFileErrorKind::Object(error),
        }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn kind(&self) -> &SerializedFileErrorKind {
        &self.kind
    }
}

impl Display for SerializedFileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.kind {
            SerializedFileErrorKind::Header(error) => Display::fmt(error, f),
            SerializedFileErrorKind::Metadata(error) => Display::fmt(error, f),
            SerializedFileErrorKind::Object(error) => Display::fmt(error, f),
        }
    }
}

impl Error for SerializedFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            SerializedFileErrorKind::Header(error) => Some(error),
            SerializedFileErrorKind::Metadata(error) => Some(error),
            SerializedFileErrorKind::Object(error) => Some(error),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedFileErrorKind {
    Header(HeaderError),
    Metadata(SerializedFileMetadataError),
    Object(ObjectDecodeError),
}

#[cfg(test)]
mod tests {
    use super::{
        SerializedExternalFileInsertion, SerializedFile, SerializedFileErrorKind,
        SerializedFileRewriteErrorKind, SerializedFileRewriteLimits, SerializedObjectInsertion,
    };
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::metadata::FileIdentifier;
    use crate::serialized_file::object::SerializedValue;

    fn put_u16_le(output: &mut Vec<u8>, value: u16) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_i32_le(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32_le(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn put_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        put_u16_le(output, 1); // node version
        output.push(level);
        output.push(0); // type flags
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, 0); // transfer metadata flags
    }

    fn put_legacy_node(
        output: &mut Vec<u8>,
        type_name: &str,
        name: &str,
        byte_size: i32,
        index: i32,
        child_count: i32,
    ) {
        output.extend_from_slice(type_name.as_bytes());
        output.push(0);
        output.extend_from_slice(name.as_bytes());
        output.push(0);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_i32_le(output, 0); // type flags
        put_i32_le(output, 1); // node version
        put_u32_le(output, 0); // transfer metadata flags
        put_i32_le(output, child_count);
    }

    fn make_generation_8_file() -> Vec<u8> {
        let mut metadata = vec![0]; // little-endian metadata marker
        metadata.extend_from_slice(b"2018.4.0f1\0");
        put_u32_le(&mut metadata, 0); // target platform
        put_i32_le(&mut metadata, 1); // serialized type count
        put_i32_le(&mut metadata, 1); // raw type ID
        put_legacy_node(&mut metadata, "Root", "", -1, 0, 1);
        put_legacy_node(&mut metadata, "int", "m_Value", 4, 1, 0);
        put_u32_le(&mut metadata, 0); // generation-8 LongFileID value
        put_i32_le(&mut metadata, 1); // object count
        put_i32_le(&mut metadata, 42); // 32-bit PathID
        put_u32_le(&mut metadata, 0); // data offset relative to the file data offset
        put_i32_le(&mut metadata, 4); // data size
        put_i32_le(&mut metadata, 1); // raw type ID
        put_u16_le(&mut metadata, 1); // class ID
        put_u16_le(&mut metadata, 0); // destroyed flag
        put_i32_le(&mut metadata, 0); // external count
        metadata.push(0); // empty user-information string

        let header_size = 16usize;
        let data_offset = header_size;
        let mut payload = Vec::from(0x1234_5678_i32.to_le_bytes());
        payload.extend_from_slice(&[0; 4]); // object data alignment to eight bytes
        let metadata_offset = data_offset + payload.len();
        let file_size = metadata_offset + metadata.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as i32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&FormatVersion::Unknown8.raw().to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.extend_from_slice(&payload);
        file.extend_from_slice(&metadata);
        file
    }

    fn make_generation_10_file() -> Vec<u8> {
        make_generation_10_file_with_object_count(1)
    }

    fn make_generation_10_file_with_object_count(object_count: usize) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, "Root");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let value_name = push_type_tree_string(&mut string_buffer, "m_Value");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let target_name = push_type_tree_string(&mut string_buffer, "m_Target");
        let pptr_type = push_type_tree_string(&mut string_buffer, "PPtr<GameObject>");
        let file_id_name = push_type_tree_string(&mut string_buffer, "m_FileID");
        let int32_type = push_type_tree_string(&mut string_buffer, "SInt32");
        let path_id_name = push_type_tree_string(&mut string_buffer, "m_PathID");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0); // target platform

        put_i32_le(&mut metadata, 1); // serialized type count
        put_i32_le(&mut metadata, 1); // raw type ID
        put_i32_le(&mut metadata, 5); // TypeTree node count
        put_i32_le(&mut metadata, string_buffer.len() as i32);
        put_node(&mut metadata, 0, root_type, empty_name, -1, 0);
        put_node(&mut metadata, 1, int_type, value_name, 4, 1);
        put_node(&mut metadata, 1, pptr_type, target_name, 8, 2);
        put_node(&mut metadata, 2, int32_type, file_id_name, 4, 3);
        put_node(&mut metadata, 2, int32_type, path_id_name, 4, 4);
        metadata.extend_from_slice(&string_buffer);

        put_u32_le(&mut metadata, 0); // generation-10 LongFileID value
        put_i32_le(&mut metadata, i32::try_from(object_count).unwrap()); // object count
        for index in 0..object_count {
            let path_id = 42 + i32::try_from(index).unwrap() * 42;
            put_i32_le(&mut metadata, path_id); // 32-bit PathID
            put_u32_le(&mut metadata, u32::try_from(index * 16).unwrap()); // data offset relative to the file data offset
            put_i32_le(&mut metadata, 12); // data size
            put_i32_le(&mut metadata, 1); // raw type ID
            put_u16_le(&mut metadata, 1); // class ID
            put_u16_le(&mut metadata, 0); // destroyed flag
        }
        put_i32_le(&mut metadata, 0); // external count
        metadata.push(0); // empty user-information string

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let mut payload = Vec::new();
        for index in 0..object_count {
            let path_id = 42 + i32::try_from(index).unwrap() * 42;
            put_i32_le(&mut payload, 0x1234_5678 + i32::try_from(index).unwrap());
            put_i32_le(&mut payload, 0); // local PPtr fileID
            put_i32_le(&mut payload, path_id); // local PPtr PathID
            while payload.len() % 8 != 0 {
                payload.push(0);
            }
        }
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as i32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&FormatVersion::Unknown10.raw().to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.push(0); // little-endian serialized data
        file.extend_from_slice(&[0; 3]); // generation-10 header alignment
        debug_assert_eq!(file.len(), header_size);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    #[test]
    fn parses_header_metadata_and_object_payload_from_one_complete_file() {
        let input = make_generation_10_file();
        let parsed = SerializedFile::parse(&input).unwrap();

        assert_eq!(parsed.header.format_version, FormatVersion::Unknown10);
        assert_eq!(parsed.metadata.types.len(), 1);
        assert_eq!(parsed.metadata.objects[0].path_id, 42);
        assert_eq!(parsed.object_record_layout.records.len(), 1);
        let table_layout = &parsed.object_record_layout;
        let record_layout = table_layout.records[0];
        let metadata_start = parsed.metadata.prefix.metadata_offset;
        let count_position = metadata_start + table_layout.count_field_offset;
        let records_start = metadata_start + table_layout.records_start_offset;
        let records_end = metadata_start + table_layout.records_end_offset;
        assert_eq!(
            i32::from_le_bytes(
                input[count_position..count_position + 4]
                    .try_into()
                    .unwrap()
            ),
            1
        );
        assert!(records_start < records_end);
        let data_offset_position = metadata_start + record_layout.data_offset_field_offset;
        let data_size_position = metadata_start + record_layout.data_size_field_offset;
        assert_eq!(
            u32::from_le_bytes(
                input[data_offset_position..data_offset_position + 4]
                    .try_into()
                    .unwrap()
            ),
            parsed.metadata.objects[0].byte_start as u32
        );
        assert_eq!(
            i32::from_le_bytes(
                input[data_size_position..data_size_position + 4]
                    .try_into()
                    .unwrap()
            ),
            parsed.metadata.objects[0].byte_size as i32
        );
        assert_eq!(parsed.input(), input.as_slice());

        let object = parsed.decode_object_payload(0).unwrap();
        assert_eq!(object.type_name, "Root");
        assert_eq!(
            object.root,
            SerializedValue::Struct(vec![
                super::super::object::SerializedFieldValue {
                    name: "m_Value".to_owned(),
                    value: SerializedValue::I32(0x1234_5678),
                },
                super::super::object::SerializedFieldValue {
                    name: "m_Target".to_owned(),
                    value: SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 42,
                    },
                },
            ])
        );
        assert_eq!(object.consumed_bytes, 12);
        assert!(object.trailing_bytes.is_empty());

        let graph = super::super::reference_graph::ObjectReferenceGraph::build(&parsed).unwrap();
        assert!(graph.is_complete());
        assert_eq!(graph.object_count(), 1);
        assert_eq!(
            graph.references(),
            &[super::super::reference_graph::SerializedObjectReference {
                source_object_index: 0,
                source_path_id: 42,
                field_path: "m_Target".to_owned(),
                file_id: 0,
                path_id: 42,
            }]
        );
        assert_eq!(
            graph.resolve_pointer(&parsed.metadata, 0, 42),
            super::super::references::PPtrResolution::LocalObject { object_index: 0 }
        );

        let edge_limited = super::super::reference_graph::ObjectReferenceGraph::build_with_limits(
            &parsed,
            super::super::reference_graph::ReferenceGraphLimits {
                max_edges: 0,
                ..super::super::reference_graph::ReferenceGraphLimits::default()
            },
        )
        .unwrap();
        assert!(!edge_limited.is_complete());
        assert!(matches!(
            edge_limited.issues(),
            [super::super::reference_graph::ReferenceGraphIssue::EdgeLimitExceeded { .. }]
        ));

        assert!(matches!(
            super::super::reference_graph::ObjectReferenceGraph::build_with_limits(
                &parsed,
                super::super::reference_graph::ReferenceGraphLimits {
                    max_objects: 0,
                    ..super::super::reference_graph::ReferenceGraphLimits::default()
                }
            ),
            Err(
                super::super::reference_graph::ReferenceGraphBuildError::ObjectCountLimitExceeded {
                    object_count: 1,
                    limit: 0
                }
            )
        ));
        assert!(matches!(
            super::super::reference_graph::ObjectReferenceGraph::build_with_limits(
                &parsed,
                super::super::reference_graph::ReferenceGraphLimits {
                    max_edges: 0,
                    max_issues: 0,
                    ..super::super::reference_graph::ReferenceGraphLimits::default()
                }
            ),
            Err(
                super::super::reference_graph::ReferenceGraphBuildError::IssueLimitExceeded {
                    object_index: 0,
                    path_id: 42,
                    limit: 0
                }
            )
        ));
    }

    #[test]
    fn high_level_parse_reports_header_truncation_with_its_original_offset() {
        let error = SerializedFile::parse(&[0; 8]).unwrap_err();
        assert!(matches!(error.kind(), SerializedFileErrorKind::Header(_)));
        assert_eq!(error.offset(), 8);
    }

    #[test]
    fn rewrites_payload_sizes_offsets_and_header_then_reparses_semantically() {
        let input = make_generation_10_file_with_object_count(2);
        let parsed = SerializedFile::parse(&input).unwrap();
        let unchanged = parsed
            .rewrite_object_payloads(&[], SerializedFileRewriteLimits::default())
            .unwrap();
        assert_eq!(unchanged, input);

        let mut replacement = parsed.decode_object_payload(0).unwrap();
        replacement.trailing_bytes = vec![0xa1, 0xb2, 0xc3, 0xd4, 0xe5];
        let output = parsed
            .rewrite_object_payloads(&[(0, replacement)], SerializedFileRewriteLimits::default())
            .unwrap();

        let rewritten = SerializedFile::parse(&output).unwrap();
        assert_eq!(rewritten.header.file_size as usize, output.len());
        assert_eq!(rewritten.header.data_offset % 16, 0);
        assert_eq!(rewritten.metadata.objects[0].path_id, 42);
        assert_eq!(rewritten.metadata.objects[0].byte_start, 0);
        assert_eq!(rewritten.metadata.objects[0].byte_size, 17);
        assert_eq!(rewritten.metadata.objects[1].path_id, 84);
        assert_eq!(rewritten.metadata.objects[1].byte_start, 24);
        assert_eq!(rewritten.metadata.objects[1].byte_size, 12);
        let first = rewritten.decode_object_payload(0).unwrap();
        assert_eq!(first.trailing_bytes, [0xa1, 0xb2, 0xc3, 0xd4, 0xe5]);
        let second = rewritten.decode_object_payload(1).unwrap();
        assert_eq!(
            second.root,
            SerializedValue::Struct(vec![
                super::super::object::SerializedFieldValue {
                    name: "m_Value".to_owned(),
                    value: SerializedValue::I32(0x1234_5679),
                },
                super::super::object::SerializedFieldValue {
                    name: "m_Target".to_owned(),
                    value: SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 84,
                    },
                },
            ])
        );
        assert!(matches!(
            parsed.rewrite_object_payloads(
                &[(0, parsed.decode_object_payload(0).unwrap())],
                SerializedFileRewriteLimits {
                    max_output_file_bytes: 64,
                    ..Default::default()
                }
            ),
            Err(error) if matches!(
                error.kind(),
                SerializedFileRewriteErrorKind::OutputLimitExceeded { limit: 64, .. }
            )
        ));
    }

    #[test]
    fn rewrites_pre_generation_9_metadata_at_the_end() {
        let input = make_generation_8_file();
        let parsed = SerializedFile::parse(&input).unwrap();
        assert!(parsed.header.format_version.metadata_at_end());
        assert_eq!(parsed.metadata.prefix.metadata_offset, 24);

        let mut replacement = parsed.decode_object_payload(0).unwrap();
        *replacement
            .root
            .unique_field_in_base_chain_mut("m_Value")
            .unwrap()
            .unwrap() = SerializedValue::I32(0x0102_0304);
        replacement.trailing_bytes = vec![0xa1, 0xb2, 0xc3, 0xd4, 0xe5];
        let output = parsed
            .rewrite_object_payloads(&[(0, replacement)], SerializedFileRewriteLimits::default())
            .unwrap();

        let rewritten = SerializedFile::parse(&output).unwrap();
        assert_eq!(rewritten.header.format_version, FormatVersion::Unknown8);
        assert_eq!(rewritten.header.file_size as usize, output.len());
        assert_eq!(rewritten.header.data_offset, 16);
        assert_eq!(rewritten.metadata.prefix.metadata_offset, 32);
        assert_eq!(rewritten.metadata.objects[0].byte_start, 0);
        assert_eq!(rewritten.metadata.objects[0].byte_size, 9);
        let decoded = rewritten.decode_object_payload(0).unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![super::super::object::SerializedFieldValue {
                name: "m_Value".to_owned(),
                value: SerializedValue::I32(0x0102_0304),
            }])
        );
        assert_eq!(decoded.trailing_bytes, [0xa1, 0xb2, 0xc3, 0xd4, 0xe5]);
    }

    #[test]
    fn inserts_object_into_pre_generation_9_file_with_metadata_at_end() {
        let input = make_generation_8_file();
        let parsed = SerializedFile::parse(&input).unwrap();
        let mut object = parsed.metadata.objects[0].clone();
        object.path_id = 84;
        let insertion = SerializedObjectInsertion {
            object,
            root: SerializedValue::Struct(vec![super::super::object::SerializedFieldValue {
                name: "m_Value".to_owned(),
                value: SerializedValue::I32(0x0102_0304),
            }]),
            trailing_bytes: Vec::new(),
        };

        let output = parsed
            .rewrite_with_insertions(&[], &[insertion], SerializedFileRewriteLimits::default())
            .unwrap();
        let rewritten = SerializedFile::parse(&output).unwrap();
        assert_eq!(rewritten.header.format_version, FormatVersion::Unknown8);
        assert_eq!(rewritten.metadata.objects.len(), 2);
        assert_eq!(rewritten.metadata.objects[1].path_id, 84);
        assert_eq!(rewritten.metadata.objects[1].byte_start, 8);
        assert_eq!(rewritten.metadata.objects[1].byte_size, 4);
        assert_eq!(rewritten.metadata.prefix.metadata_offset, 32);
        let decoded = rewritten.decode_object_payload(1).unwrap();
        assert_eq!(
            decoded
                .root
                .unique_field_in_base_chain("m_Value")
                .unwrap()
                .unwrap(),
            &SerializedValue::I32(0x0102_0304)
        );
    }

    #[test]
    fn inserts_object_record_payload_and_preserves_following_metadata() {
        let input = make_generation_10_file();
        let parsed = SerializedFile::parse(&input).unwrap();
        let source_metadata_start = parsed.metadata.prefix.metadata_offset;
        let source_metadata_end = source_metadata_start + parsed.header.metadata_size as usize;
        let source_metadata = &input[source_metadata_start..source_metadata_end];
        let insertion_offset = parsed.object_record_layout.records_end_offset;
        let mut object = parsed.metadata.objects[0].clone();
        object.path_id = 126;
        object.byte_start = 999; // The writer computes both data fields.
        object.byte_size = 1;
        let insertion = SerializedObjectInsertion {
            object,
            root: SerializedValue::Struct(vec![
                super::super::object::SerializedFieldValue {
                    name: "m_Value".to_owned(),
                    value: SerializedValue::I32(0x2468_ace0),
                },
                super::super::object::SerializedFieldValue {
                    name: "m_Target".to_owned(),
                    value: SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 126,
                    },
                },
            ]),
            trailing_bytes: vec![0xde, 0xad],
        };

        let output = parsed
            .rewrite_with_insertions(&[], &[insertion], SerializedFileRewriteLimits::default())
            .unwrap();
        let rewritten = SerializedFile::parse(&output).unwrap();
        assert_eq!(rewritten.metadata.objects.len(), 2);
        assert_eq!(rewritten.metadata.objects[1].path_id, 126);
        assert_eq!(rewritten.metadata.objects[1].byte_start, 16);
        assert_eq!(rewritten.metadata.objects[1].byte_size, 14);
        let decoded = rewritten.decode_object_payload(1).unwrap();
        assert_eq!(decoded.object_index, 1);
        assert_eq!(decoded.path_id, 126);
        assert_eq!(decoded.type_name, "Root");
        assert_eq!(decoded.consumed_bytes, 12);
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![
                super::super::object::SerializedFieldValue {
                    name: "m_Value".to_owned(),
                    value: SerializedValue::I32(0x2468_ace0),
                },
                super::super::object::SerializedFieldValue {
                    name: "m_Target".to_owned(),
                    value: SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 126,
                    },
                },
            ])
        );
        assert_eq!(decoded.trailing_bytes, [0xde, 0xad]);
        let rewritten_metadata_start = rewritten.metadata.prefix.metadata_offset;
        let rewritten_metadata_end =
            rewritten_metadata_start + rewritten.header.metadata_size as usize;
        let rewritten_metadata = &output[rewritten_metadata_start..rewritten_metadata_end];
        assert_eq!(
            i32::from_le_bytes(
                rewritten_metadata[rewritten.object_record_layout.count_field_offset
                    ..rewritten.object_record_layout.count_field_offset + 4]
                    .try_into()
                    .unwrap()
            ),
            2
        );
        let inserted_record_size = rewritten.object_record_layout.records_end_offset
            - parsed.object_record_layout.records_end_offset;
        assert_eq!(inserted_record_size, 20);
        assert_eq!(
            &rewritten_metadata[rewritten.object_record_layout.records_end_offset..],
            &source_metadata[insertion_offset..]
        );
        assert_eq!(
            &output
                [rewritten.header.data_offset as usize..rewritten.header.data_offset as usize + 12],
            &input[parsed.header.data_offset as usize..parsed.header.data_offset as usize + 12]
        );
    }

    #[test]
    fn rejects_invalid_object_insertions_before_rewriting() {
        let input = make_generation_10_file();
        let parsed = SerializedFile::parse(&input).unwrap();
        let make_insertion = |path_id| {
            let mut object = parsed.metadata.objects[0].clone();
            object.path_id = path_id;
            SerializedObjectInsertion {
                object,
                root: parsed.decode_object_payload(0).unwrap().root,
                trailing_bytes: Vec::new(),
            }
        };

        let duplicate = parsed
            .rewrite_with_insertions(
                &[],
                &[make_insertion(42)],
                SerializedFileRewriteLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            duplicate.kind(),
            SerializedFileRewriteErrorKind::DuplicateObjectPathId { path_id: 42 }
        ));

        let limited = parsed
            .rewrite_with_insertions(
                &[],
                &[make_insertion(126)],
                SerializedFileRewriteLimits {
                    max_insertions: 0,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            limited.kind(),
            SerializedFileRewriteErrorKind::InsertionLimitExceeded {
                requested: 1,
                limit: 0
            }
        ));

        let mut mismatched_type = make_insertion(126);
        mismatched_type.object.raw_type_id = 2;
        let mismatch = parsed
            .rewrite_with_insertions(
                &[],
                &[mismatched_type],
                SerializedFileRewriteLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            mismatch.kind(),
            SerializedFileRewriteErrorKind::InsertionTypeMismatch {
                field: "raw type ID"
            }
        ));

        let unrepresentable = parsed
            .rewrite_with_insertions(
                &[],
                &[make_insertion(i64::from(i32::MAX) + 1)],
                SerializedFileRewriteLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            unrepresentable.kind(),
            SerializedFileRewriteErrorKind::InsertionPathIdUnrepresentable { width: 4, .. }
        ));
    }

    #[test]
    fn appends_external_file_records_alone_and_with_object_records() {
        let input = make_generation_10_file();
        let parsed = SerializedFile::parse(&input).unwrap();
        let identifier = FileIdentifier {
            asset_path: Some("Assets/Generated.asset".to_owned()),
            guid: Some([0x0102_0304, 0x1112_1314, 0x2122_2324, 0x3132_3334]),
            file_type: Some(7),
            path_name_origin: "Library/Generated.asset".to_owned(),
        };
        let external_insertion = SerializedExternalFileInsertion {
            file_identifier: identifier.clone(),
        };

        let external_only = parsed
            .rewrite_with_changes(
                &[],
                &[],
                std::slice::from_ref(&external_insertion),
                SerializedFileRewriteLimits::default(),
            )
            .unwrap();
        let external_only_file = SerializedFile::parse(&external_only).unwrap();
        assert_eq!(
            external_only_file.metadata.externals.as_slice(),
            std::slice::from_ref(&identifier)
        );
        assert_eq!(
            external_only_file.metadata.user_information.as_deref(),
            Some("")
        );
        assert_eq!(
            external_only_file.header.file_size as usize,
            external_only.len()
        );

        let mut object = parsed.metadata.objects[0].clone();
        object.path_id = 126;
        let object_insertion = SerializedObjectInsertion {
            object,
            root: parsed.decode_object_payload(0).unwrap().root,
            trailing_bytes: Vec::new(),
        };
        let combined = parsed
            .rewrite_with_changes(
                &[],
                &[object_insertion],
                std::slice::from_ref(&external_insertion),
                SerializedFileRewriteLimits::default(),
            )
            .unwrap();
        let combined_file = SerializedFile::parse(&combined).unwrap();
        assert_eq!(combined_file.metadata.objects.len(), 2);
        assert_eq!(combined_file.metadata.objects[1].path_id, 126);
        assert_eq!(combined_file.metadata.externals, [identifier]);
        assert_eq!(combined_file.metadata.user_information.as_deref(), Some(""));
        assert_eq!(combined_file.header.file_size as usize, combined.len());
    }

    #[test]
    fn rejects_external_file_schema_nuls_and_insertion_limits() {
        let input = make_generation_10_file();
        let parsed = SerializedFile::parse(&input).unwrap();
        let valid = FileIdentifier {
            asset_path: Some("Assets/Generated.asset".to_owned()),
            guid: Some([0; 4]),
            file_type: Some(0),
            path_name_origin: "Library/Generated.asset".to_owned(),
        };
        let insertion = |file_identifier| SerializedExternalFileInsertion { file_identifier };

        let limited = parsed
            .rewrite_with_changes(
                &[],
                &[],
                &[insertion(valid.clone())],
                SerializedFileRewriteLimits {
                    max_external_file_insertions: 0,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            limited.kind(),
            SerializedFileRewriteErrorKind::ExternalFileInsertionLimitExceeded {
                requested: 1,
                limit: 0
            }
        ));

        let mut wrong_schema = valid.clone();
        wrong_schema.asset_path = None;
        let schema_error = parsed
            .rewrite_with_changes(
                &[],
                &[],
                &[insertion(wrong_schema)],
                SerializedFileRewriteLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            schema_error.kind(),
            SerializedFileRewriteErrorKind::ExternalFileFieldMismatch {
                field: "asset path presence"
            }
        ));

        let mut embedded_nul = valid;
        embedded_nul.path_name_origin.push('\0');
        let nul_error = parsed
            .rewrite_with_changes(
                &[],
                &[],
                &[insertion(embedded_nul)],
                SerializedFileRewriteLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            nul_error.kind(),
            SerializedFileRewriteErrorKind::ExternalFileStringContainsNul {
                field: "path name origin"
            }
        ));
    }
}
