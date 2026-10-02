use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::header::{HeaderError, SerializedFileHeader};
use super::metadata::{SerializedFileMetadata, SerializedFileMetadataError};
use super::object::{ObjectDecodeError, ObjectDecodeLimits, ParsedObjectPayload};

/// A read-only view of one complete Unity SerializedFile.
///
/// The original input remains borrowed and unchanged. Header, metadata, and TypeTree-backed
/// object payloads are parsed from this same byte slice, preserving their file scope and offsets.
#[derive(Debug)]
pub struct SerializedFile<'a> {
    input: &'a [u8],
    pub header: SerializedFileHeader,
    pub metadata: SerializedFileMetadata,
}

impl<'a> SerializedFile<'a> {
    /// Parse the header and all metadata tables from one complete SerializedFile input.
    pub fn parse(input: &'a [u8]) -> Result<Self, SerializedFileError> {
        let header =
            SerializedFileHeader::parse(input).map_err(SerializedFileError::from_header)?;
        let metadata = SerializedFileMetadata::parse_file(input, &header)
            .map_err(SerializedFileError::from_metadata)?;
        Ok(Self {
            input,
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
    use super::{SerializedFile, SerializedFileErrorKind};
    use crate::serialized_file::format_version::FormatVersion;
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

    fn make_generation_10_file() -> Vec<u8> {
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
        put_i32_le(&mut metadata, 1); // object count
        put_i32_le(&mut metadata, 42); // 32-bit PathID
        put_u32_le(&mut metadata, 0); // data offset relative to the file data offset
        put_i32_le(&mut metadata, 12); // data size
        put_i32_le(&mut metadata, 1); // raw type ID
        put_u16_le(&mut metadata, 1); // class ID
        put_u16_le(&mut metadata, 0); // destroyed flag
        put_i32_le(&mut metadata, 0); // external count
        metadata.push(0); // empty user-information string

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let mut payload = Vec::new();
        put_i32_le(&mut payload, 0x1234_5678);
        put_i32_le(&mut payload, 0); // local PPtr fileID
        put_i32_le(&mut payload, 42); // local PPtr PathID
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
}
