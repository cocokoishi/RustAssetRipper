use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::format_version::FormatVersion;
use super::header::{HeaderError, SerializedFileHeader};
use super::metadata::{
    ExternalFileTableLayout, ObjectRecordTableLayout, SerializedFileMetadata,
    SerializedFileMetadataError,
};
use super::object::{
    serialized_file_object_read_flags, ObjectDecodeError, ObjectDecodeLimits, ParsedObjectPayload,
};
use super::transfer_flags::TransferInstructionFlags;

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
    #[cfg(test)]
    pub(crate) fn from_parts_for_test(
        input: &'a [u8],
        header: SerializedFileHeader,
        metadata: SerializedFileMetadata,
    ) -> Self {
        Self {
            input,
            object_record_layout: ObjectRecordTableLayout {
                count_field_offset: 0,
                records_start_offset: 0,
                records_end_offset: 0,
                records: Vec::new(),
            },
            external_file_table_layout: ExternalFileTableLayout {
                count_field_offset: 0,
                records_start_offset: 0,
                records_end_offset: 0,
            },
            header,
            metadata,
        }
    }

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

    /// Baseline `SerializedFile.Flags` for this parsed file, over caller path facts.
    ///
    /// Feeds the parsed generation, target platform, and body endianness with the raw
    /// `FileBase.FilePath`/`Name` facts into the shared `serialized_file_object_read_flags`
    /// kernel: `NoTarget` platforms on platform-carrying generations read editor scenes
    /// (`.unity` paths) as `SerializeEditorMinimalScene`, engine resources (and, below
    /// generation 10, builtin-extra files) add `IsBuiltinResourcesFile`, big-endian
    /// bodies add `SwapEndianess`, and everything else reads as `SerializeGameRelease`.
    /// Path entry naming (`NameFixed`) and the collections carrying the word stay
    /// caller-side; this entry point only derives the flag word from one parsed file.
    pub fn object_read_flags(&self, file_path: &str, file_name: &str) -> TransferInstructionFlags {
        serialized_file_object_read_flags(
            self.header.format_version,
            self.metadata.prefix.target_platform,
            file_path,
            file_name,
            self.metadata.prefix.body_endian,
        )
    }
}

/// The minimum header bytes the sniff reads (`SerializedFileHeader.HeaderMinSize`).
pub const SERIALIZED_FILE_SNIFF_MIN_SIZE: usize = 16;
/// The minimum metadata bytes a sniffed file must declare
/// (`SerializedFileHeader.MetadataMinSize`).
pub const SERIALIZED_FILE_SNIFF_MIN_METADATA_SIZE: i64 = 13;

/// `SerializedFileHeader.IsSerializedFileHeader` over a whole input slice.
///
/// Reads the big-endian size/generation prefix (generation is always at offset 8);
/// generations with large-file support re-read the sizes from offsets `0x14`/`0x18`.
/// The generation must be a defined `FormatVersion`, the declared metadata size must
/// reach 13 bytes, the declared file size must reach 29 bytes, and the declared file
/// size must equal the actual input length.
///
/// One deliberate hardening difference: the source repositions its reader past the
/// 16-byte prefix for large-file generations without re-checking bounds, so a
/// 16-to-31-byte input naming generation 22 throws there; this kernel returns `false`
/// for any input too short to hold the fields it reads.
pub fn is_serialized_file(input: &[u8]) -> bool {
    if input.len() < SERIALIZED_FILE_SNIFF_MIN_SIZE {
        return false;
    }
    let mut metadata_size = i32::from_be_bytes([input[0], input[1], input[2], input[3]]) as i64;
    let mut header_file_size = u32::from_be_bytes([input[4], input[5], input[6], input[7]]) as u64;
    let generation = i32::from_be_bytes([input[8], input[9], input[10], input[11]]);
    let Ok(version) = FormatVersion::try_from(generation) else {
        return false;
    };
    if version.has_large_file_support() {
        const EXTENDED_PREFIX_END: usize = 0x20;
        if input.len() < EXTENDED_PREFIX_END {
            return false;
        }
        metadata_size =
            i32::from_be_bytes([input[0x14], input[0x15], input[0x16], input[0x17]]) as i64;
        header_file_size = u64::from_be_bytes([
            input[0x18],
            input[0x19],
            input[0x1a],
            input[0x1b],
            input[0x1c],
            input[0x1d],
            input[0x1e],
            input[0x1f],
        ]);
    }
    metadata_size >= SERIALIZED_FILE_SNIFF_MIN_METADATA_SIZE
        && header_file_size
            >= (SERIALIZED_FILE_SNIFF_MIN_SIZE as u64
                + SERIALIZED_FILE_SNIFF_MIN_METADATA_SIZE as u64)
        && header_file_size == input.len() as u64
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
        is_serialized_file, SerializedExternalFileInsertion, SerializedFile,
        SerializedFileErrorKind, SerializedFileRewriteErrorKind, SerializedFileRewriteLimits,
        SerializedObjectInsertion,
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
    fn header_sniff_matches_the_source_checks() {
        // A well-formed file passes.
        let file = make_generation_10_file();
        assert!(is_serialized_file(&file));
        // Too short for even the legacy prefix.
        assert!(!is_serialized_file(&file[..15]));
        assert!(!is_serialized_file(&[]));
        // The declared size must equal the actual length.
        let mut short_size = file.clone();
        short_size[4..8].copy_from_slice(&((file.len() as u32) - 1).to_be_bytes());
        assert!(!is_serialized_file(&short_size));
        // Undefined generations fail (4 and 23 are not defined).
        for generation in [0_i32, 4, 23, 100, -1] {
            let mut bad_gen = file.clone();
            bad_gen[8..12].copy_from_slice(&generation.to_be_bytes());
            assert!(!is_serialized_file(&bad_gen), "generation {generation}");
        }
        // A declared metadata size below the 13-byte minimum fails.
        let mut small_meta = file.clone();
        small_meta[0..4].copy_from_slice(&12_i32.to_be_bytes());
        assert!(!is_serialized_file(&small_meta));
    }

    #[test]
    fn header_sniff_reads_large_file_sizes_from_the_extended_prefix() {
        // Minimal 64-byte generation-22 input: legacy fields zeroed, extended sizes set.
        let mut file = vec![0u8; 64];
        file[8..12].copy_from_slice(&22_i32.to_be_bytes());
        file[0x14..0x18].copy_from_slice(&13_i32.to_be_bytes());
        file[0x18..0x20].copy_from_slice(&64_u64.to_be_bytes());
        assert!(is_serialized_file(&file));
        // A mismatched extended file size fails.
        file[0x18..0x20].copy_from_slice(&63_u64.to_be_bytes());
        assert!(!is_serialized_file(&file));
        // A generation-22 input too short for the extended prefix fails instead of
        // reading out of bounds (hardened relative to the source).
        let mut short = vec![0u8; 20];
        short[8..12].copy_from_slice(&22_i32.to_be_bytes());
        assert!(!is_serialized_file(&short));
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

    #[test]
    fn read_entry_derives_baseline_transfer_flags_from_parsed_facts() {
        use crate::serialized_file::transfer_flags::TransferInstructionFlags;

        // The generation-10 fixture targets platform 0 with a little-endian body.
        let input = make_generation_10_file();
        let parsed = SerializedFile::parse(&input).unwrap();
        assert_eq!(
            parsed.object_read_flags("Assets/level0", "level0"),
            TransferInstructionFlags::SERIALIZE_GAME_RELEASE
        );
        // Engine resource names add the builtin bit on top of the release base.
        let builtin = parsed.object_read_flags("Assets/level0", "unity default resources");
        assert!(builtin.is_builtin_resources());
        assert_eq!(
            builtin,
            TransferInstructionFlags::SERIALIZE_GAME_RELEASE
                .with(TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE)
        );
        // A `.unity` path alone changes nothing while the platform is not `NoTarget`.
        assert_eq!(
            parsed.object_read_flags("Assets/scene.unity", "scene"),
            TransferInstructionFlags::SERIALIZE_GAME_RELEASE
        );

        // Patch the target platform to `NoTarget` (0xFFFFFFFE), which sits right
        // after the 11-byte version string in this 20-byte-header fixture: `.unity`
        // paths then read as editor-minimal scenes, anything else with bare flags,
        // and the ordinal suffix check stays case-sensitive.
        let mut no_target = input.clone();
        no_target[31..35].copy_from_slice(&0xFFFFFFFEu32.to_le_bytes());
        let parsed = SerializedFile::parse(&no_target).unwrap();
        assert_eq!(
            parsed.object_read_flags("Assets/scene.unity", "scene"),
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE
        );
        assert_eq!(
            parsed.object_read_flags("Assets/level0", "level0"),
            TransferInstructionFlags::NONE
        );
        assert_eq!(
            parsed.object_read_flags("Assets/SCENE.UNITY", "scene"),
            TransferInstructionFlags::NONE
        );
    }

    fn rewrite_fixture_bytes(case: &str) -> (Vec<u8>, String) {
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

        // Probe Temp/opencode/objrewriteprobe (SerializedFileMetadata.Read
        // plus SerializableStructure.Read over C#-constructed decodeable
        // files, SDK 10.0.401). Nothing Rust-generated.
        let fixture = include_str!("object_rewrite_fixture.txt");
        let mut file = None;
        let mut dump = None;
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("RW_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad RW_HEX {line}");
                if parts[1] == case {
                    file = Some(hex_to_bytes(parts[2]));
                }
            } else if line.starts_with("RW_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad RW_READ {line}");
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

    #[test]
    fn rewrites_reference_constructed_files_across_generations() {
        // (case, generation, header length, appended-data layout)
        for (case, generation) in [("R8", 8), ("R12", 12), ("R20", 20), ("R22", 22)] {
            let (bytes, dump) = rewrite_fixture_bytes(case);
            assert!(
                dump.contains("INT=11|STR=go|SIZE=12"),
                "{case} oracle {dump}"
            );
            let parsed =
                SerializedFile::parse(&bytes).unwrap_or_else(|e| panic!("{case} parse: {e:?}"));
            assert_eq!(parsed.header.format_version.raw(), generation);
            assert_eq!(parsed.header.file_size as usize, bytes.len());
            let payload = parsed
                .decode_object_payload(0)
                .unwrap_or_else(|e| panic!("{case} decode: {e:?}"));
            assert_eq!(parsed.metadata.objects[0].byte_size, 12);

            // Grow the variable-length string: "go" (8 encoded bytes)
            // becomes "hello!" (12 encoded bytes), so the payload grows
            // from 12 to 16 bytes on every generation.
            let mut replacement = payload;
            *replacement
                .root
                .unique_field_in_base_chain_mut("m_String")
                .expect("m_String lookup")
                .expect("m_String present") = SerializedValue::StringBytes(b"hello!".to_vec());
            let output = parsed
                .rewrite_object_payloads(
                    &[(0, replacement)],
                    SerializedFileRewriteLimits::default(),
                )
                .unwrap_or_else(|e| panic!("{case} rewrite: {e:?}"));
            // The payload grows by 4 ("go" to "hello!"); the rebuild may
            // add documented data/object alignment padding on top, so only
            // a lower bound is pinned here (exact pad layout is Rust policy;
            // the baseline never rewrites files).
            assert!(
                output.len() >= bytes.len() + 4,
                "{case} growth {}",
                output.len()
            );

            let rewritten =
                SerializedFile::parse(&output).unwrap_or_else(|e| panic!("{case} reparse: {e:?}"));
            assert_eq!(rewritten.header.format_version.raw(), generation);
            assert_eq!(rewritten.header.file_size as usize, output.len());
            assert_eq!(rewritten.header.data_offset % 16, 0);
            assert_eq!(rewritten.metadata.objects[0].path_id, 8001);
            assert_eq!(rewritten.metadata.objects[0].byte_start, 0);
            assert_eq!(rewritten.metadata.objects[0].byte_size, 16);
            // The single external survives the rebuild byte-for-byte.
            assert_eq!(rewritten.metadata.externals.len(), 1);
            assert_eq!(
                rewritten.metadata.externals[0],
                parsed.metadata.externals[0]
            );
            let decoded = rewritten
                .decode_object_payload(0)
                .unwrap_or_else(|e| panic!("{case} redecode: {e:?}"));
            let root = &decoded.root;
            let int = root
                .unique_field("m_Int")
                .expect("m_Int lookup")
                .expect("m_Int present");
            assert_eq!(int, &SerializedValue::I32(11));
            let string = root
                .unique_field("m_String")
                .expect("m_String lookup")
                .expect("m_String present");
            assert_eq!(string, &SerializedValue::StringBytes(b"hello!".to_vec()));
        }
    }
}
