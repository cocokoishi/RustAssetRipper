    use super::{SerializedFileMetadata, SerializedFileMetadataErrorKind};
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::header::SerializedFileHeader;

    fn write_u16(bytes: &mut Vec<u8>, value: u16, endian: Endian) {
        match endian {
            Endian::Little => bytes.extend_from_slice(&value.to_le_bytes()),
            Endian::Big => bytes.extend_from_slice(&value.to_be_bytes()),
        }
    }

    fn write_i16(bytes: &mut Vec<u8>, value: i16, endian: Endian) {
        write_u16(bytes, value as u16, endian);
    }

    fn write_u32(bytes: &mut Vec<u8>, value: u32, endian: Endian) {
        match endian {
            Endian::Little => bytes.extend_from_slice(&value.to_le_bytes()),
            Endian::Big => bytes.extend_from_slice(&value.to_be_bytes()),
        }
    }

    fn write_i32(bytes: &mut Vec<u8>, value: i32, endian: Endian) {
        write_u32(bytes, value as u32, endian);
    }

    fn write_i64(bytes: &mut Vec<u8>, value: i64, endian: Endian) {
        match endian {
            Endian::Little => bytes.extend_from_slice(&value.to_le_bytes()),
            Endian::Big => bytes.extend_from_slice(&value.to_be_bytes()),
        }
    }

    fn write_string(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
    }

    fn make_file(
        generation: FormatVersion,
        body_endian: Endian,
        metadata_body: &[u8],
        payload: &[u8],
    ) -> (Vec<u8>, SerializedFileHeader) {
        make_file_with_version(
            generation,
            body_endian,
            metadata_body,
            payload,
            "2022.3.1f1",
        )
    }

    fn make_file_with_version(
        generation: FormatVersion,
        body_endian: Endian,
        metadata_body: &[u8],
        payload: &[u8],
        unity_version: &str,
    ) -> (Vec<u8>, SerializedFileHeader) {
        let header_size = if generation.has_large_file_support() {
            48
        } else if generation.has_endianness() {
            20
        } else {
            16
        };
        let mut metadata = Vec::new();
        if generation.metadata_at_end() {
            metadata.push(u8::from(body_endian == Endian::Big));
        }
        if generation.has_signature() {
            write_string(&mut metadata, unity_version);
        }
        if generation.has_platform() {
            write_u32(&mut metadata, 7, body_endian);
        }
        if generation.has_enable_type_tree() {
            metadata.push(1);
        }
        metadata.extend_from_slice(metadata_body);
        let metadata_size = metadata.len();
        let data_offset = if generation.metadata_at_end() {
            0
        } else {
            (header_size + metadata_size) as u64
        };
        let file_size = header_size + metadata_size + payload.len();
        let mut file = vec![0; header_size];
        file.extend_from_slice(&metadata);
        file.extend_from_slice(payload);
        let header = SerializedFileHeader {
            metadata_size: metadata_size as u64,
            file_size: file_size as u64,
            format_version: generation,
            data_offset,
            data_endian: generation.has_endianness().then_some(body_endian),
            reserved_v22: generation.has_large_file_support().then_some(0),
            header_size,
        };
        (file, header)
    }

    fn push_empty_modern_tree(bytes: &mut Vec<u8>, generation: FormatVersion, endian: Endian) {
        write_i32(bytes, 0, endian); // node count
        write_i32(bytes, 0, endian); // string buffer size
        assert!(generation.raw() == 10 || generation.raw() >= 12);
    }

    fn write_modern_node(
        bytes: &mut Vec<u8>,
        generation: FormatVersion,
        endian: Endian,
        version: u16,
        level: u8,
        type_offset: u32,
        name_offset: u32,
    ) {
        write_u16(bytes, version, endian);
        bytes.push(level);
        bytes.push(0);
        write_u32(bytes, type_offset, endian);
        write_u32(bytes, name_offset, endian);
        write_i32(bytes, -1, endian);
        write_i32(bytes, 0, endian);
        write_u32(bytes, 0, endian);
        if generation.raw() >= 19 {
            let value = 0x1122_3344_5566_7788u64;
            match endian {
                Endian::Little => bytes.extend_from_slice(&value.to_le_bytes()),
                Endian::Big => bytes.extend_from_slice(&value.to_be_bytes()),
            }
        }
    }

    fn align_table_position(generation: FormatVersion, tables: &mut Vec<u8>, header_size: usize) {
        if generation.raw() < 14 {
            return;
        }
        let metadata_prefix_size = usize::from(generation.metadata_at_end())
            + if generation.has_signature() { 11 } else { 0 }
            + if generation.has_platform() { 4 } else { 0 }
            + usize::from(generation.has_enable_type_tree());
        let absolute_offset = header_size + metadata_prefix_size + tables.len();
        let padding = (4 - absolute_offset % 4) % 4;
        tables.resize(tables.len() + padding, 0);
    }

    #[test]
    fn parses_generation_8_tables_types_objects_and_external_file_ids() {
        let generation = FormatVersion::Unknown8;
        let endian = Endian::Big;
        let mut tables = Vec::new();
        write_i32(&mut tables, 1, endian); // type count
        write_i32(&mut tables, 28, endian); // type ID
        write_string(&mut tables, "Base");
        write_string(&mut tables, "m_Name");
        write_i32(&mut tables, 4, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, 1, endian); // TypeTree node version
        write_u32(&mut tables, 0, endian); // TypeTree transfer flags
        write_i32(&mut tables, 0, endian); // TypeTree child count
        write_i32(&mut tables, 1, endian); // long FileID flag
        write_i32(&mut tables, 1, endian); // object count
        write_i32(&mut tables, 9, endian); // PathID remains 32-bit at generation 8
        write_u32(&mut tables, 0, endian); // payload offset relative to header data offset
        write_i32(&mut tables, 0, endian); // payload length
        write_i32(&mut tables, 28, endian);
        write_u16(&mut tables, 28, endian);
        write_u16(&mut tables, 0, endian); // destroyed
        write_i32(&mut tables, 1, endian); // external count
        write_string(&mut tables, ""); // generation 8 asset path
        for word in [1, 2, 3, 4] {
            write_u32(&mut tables, word, endian);
        }
        write_i32(&mut tables, 2, endian); // asset type
        write_string(&mut tables, "shared.assets");
        write_string(&mut tables, ""); // user information

        let (file, header) = make_file(generation, endian, &tables, &[]);
        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        assert_eq!(metadata.types.len(), 1);
        assert_eq!(metadata.types[0].raw_type_id, 28);
        assert_eq!(metadata.long_file_id, Some(1));
        assert_eq!(metadata.objects.len(), 1);
        assert_eq!(metadata.objects[0].path_id, 9);
        assert_eq!(metadata.objects[0].resolved_type_index, Some(0));
        assert_eq!(metadata.externals[0].asset_path.as_deref(), Some(""));
        assert_eq!(metadata.externals[0].guid, Some([1, 2, 3, 4]));
        assert_eq!(metadata.externals[0].path_name_origin, "shared.assets");
        assert!(metadata.trailing_metadata.is_empty());
    }

    #[test]
    fn parses_generation_12_script_table_and_little_endian_tables() {
        let generation = FormatVersion::Unknown12;
        let endian = Endian::Little;
        let mut tables = Vec::new();
        write_i32(&mut tables, 1, endian); // types
        write_i32(&mut tables, 28, endian);
        push_empty_modern_tree(&mut tables, generation, endian);
        write_u32(&mut tables, 0, endian); // generation 7-13 long FileID field
        write_i32(&mut tables, 1, endian); // objects
        write_i32(&mut tables, 45, endian);
        write_u32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, 28, endian);
        write_u16(&mut tables, 28, endian);
        write_i16(&mut tables, 0, endian); // script type index
        write_i32(&mut tables, 1, endian); // script identifiers
        write_i32(&mut tables, 2, endian);
        write_i32(&mut tables, 1234, endian);
        write_i32(&mut tables, 0, endian); // externals
        write_string(&mut tables, "metadata tail");

        let (file, header) = make_file(generation, endian, &tables, &[]);
        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        assert_eq!(metadata.objects[0].path_id, 45);
        assert_eq!(metadata.script_types[0].local_serialized_file_index, 2);
        assert_eq!(metadata.script_types[0].local_identifier_in_file, 1234);
        assert_eq!(metadata.user_information.as_deref(), Some("metadata tail"));
    }

    #[test]
    fn parses_generation_13_hashes_and_generation_14_aligned_path_ids() {
        for generation in [FormatVersion::HasTypeTreeHashes, FormatVersion::Unknown14] {
            let endian = Endian::Little;
            let mut tables = Vec::new();
            write_i32(&mut tables, 1, endian); // types
            write_i32(&mut tables, 28, endian);
            if generation.raw() >= 16 {
                tables.push(0);
            }
            if generation.raw() >= 13 {
                for word in [10, 11, 12, 13] {
                    write_u32(&mut tables, word, endian);
                }
            }
            push_empty_modern_tree(&mut tables, generation, endian);
            if generation.has_long_file_id() {
                write_u32(&mut tables, 0, endian);
            }
            write_i32(&mut tables, 1, endian); // objects
            if generation.raw() >= 14 {
                let header_size = if generation.has_large_file_support() {
                    48
                } else {
                    20
                };
                align_table_position(generation, &mut tables, header_size);
                write_i64(&mut tables, 0x1_0000_0001, endian);
            } else {
                write_i32(&mut tables, 1, endian);
            }
            write_u32(&mut tables, 0, endian);
            write_i32(&mut tables, 0, endian);
            write_i32(&mut tables, 28, endian);
            if generation.raw() < 16 {
                write_u16(&mut tables, 28, endian);
            } else {
                tables.push(0); // stripped type flag for the type record
            }
            if (11..17).contains(&generation.raw()) {
                write_i16(&mut tables, -1, endian);
            }
            if (15..17).contains(&generation.raw()) {
                tables.push(0);
            }
            if generation.has_script_types() {
                if generation.raw() == 14 {
                    write_i32(&mut tables, 1, endian);
                    write_i32(&mut tables, 3, endian);
                    align_table_position(generation, &mut tables, 20);
                    write_i64(&mut tables, 0x1_0000_0002, endian);
                } else {
                    write_i32(&mut tables, 0, endian);
                }
            }
            if generation.raw() >= 14 {
                write_i32(&mut tables, 0, endian); // script count
            }
            write_i32(&mut tables, 0, endian); // external count
            write_string(&mut tables, ""); // user information

            let (file, header) = make_file(generation, endian, &tables, &[]);
            let result = SerializedFileMetadata::parse_file(&file, &header);
            assert!(
                result.is_ok(),
                "generation {}: {result:?}",
                generation.raw()
            );
        }
    }

    #[test]
    fn parses_generation_19_modern_type_tree_nodes_and_reference_hashes() {
        let generation = FormatVersion::TypeTreeNodeWithTypeFlags;
        let endian = Endian::Big;
        let mut tables = Vec::new();
        write_i32(&mut tables, 1, endian); // type count
        write_i32(&mut tables, 28, endian);
        tables.push(0); // generation 16+ stripped type flag
        write_i16(&mut tables, -1, endian); // generation 17+ type script index
        for word in [1, 2, 3, 4] {
            write_u32(&mut tables, word, endian);
        }
        write_i32(&mut tables, 1, endian); // nodes
        write_i32(&mut tables, 5, endian); // string buffer bytes
        write_modern_node(&mut tables, generation, endian, 2, 0, 0x8000_0000 | 222, 0);
        tables.extend_from_slice(b"name\0");
        write_i32(&mut tables, 1, endian); // object count
        align_table_position(generation, &mut tables, 20);
        write_i64(&mut tables, 1, endian); // PathID
                                           // Generation 19 object data fields
        write_u32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian); // type index
        write_i32(&mut tables, 0, endian); // scripts
        write_i32(&mut tables, 0, endian); // externals
        write_string(&mut tables, "");
        let (file, header) = make_file(generation, endian, &tables, &[]);
        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        let node = &metadata.types[0].type_tree.as_ref().unwrap().nodes[0];
        assert_eq!(node.version, 2);
        assert_eq!(node.type_string_offset, Some(0x8000_0000 | 222));
        assert_eq!(node.name_string_offset, Some(0));
        assert_eq!(node.type_name.as_deref(), Some("int"));
        assert_eq!(node.name.as_deref(), Some("name"));
        assert_eq!(node.reference_type_hash, Some(0x1122_3344_5566_7788));
        assert_eq!(metadata.objects[0].path_id, 1);
    }

    #[test]
    fn parses_the_generation_10_and_generation_11_type_tree_layouts() {
        let mut modern_tables = Vec::new();
        write_i32(&mut modern_tables, 1, Endian::Little); // type count
        write_i32(&mut modern_tables, 28, Endian::Little);
        write_i32(&mut modern_tables, 1, Endian::Little); // node count
        write_i32(&mut modern_tables, 6, Endian::Little); // string buffer size
        write_modern_node(
            &mut modern_tables,
            FormatVersion::Unknown10,
            Endian::Little,
            2,
            0,
            0,
            4,
        );
        modern_tables.extend_from_slice(b"int\0x\0");
        write_i32(&mut modern_tables, 0, Endian::Little); // long FileID field
        write_i32(&mut modern_tables, 0, Endian::Little); // object count
        write_i32(&mut modern_tables, 0, Endian::Little); // external count
        write_string(&mut modern_tables, ""); // user information
        let (modern_file, modern_header) = make_file(
            FormatVersion::Unknown10,
            Endian::Little,
            &modern_tables,
            &[],
        );
        let modern = SerializedFileMetadata::parse_file(&modern_file, &modern_header).unwrap();
        let modern_node = &modern.types[0].type_tree.as_ref().unwrap().nodes[0];
        assert_eq!(modern_node.type_name.as_deref(), Some("int"));
        assert_eq!(modern_node.name.as_deref(), Some("x"));

        let mut legacy_tables = Vec::new();
        write_i32(&mut legacy_tables, 1, Endian::Big); // type count
        write_i32(&mut legacy_tables, 28, Endian::Big);
        write_string(&mut legacy_tables, "int");
        write_string(&mut legacy_tables, "value");
        write_i32(&mut legacy_tables, 4, Endian::Big);
        write_i32(&mut legacy_tables, 0, Endian::Big);
        write_i32(&mut legacy_tables, 0, Endian::Big);
        write_i32(&mut legacy_tables, 1, Endian::Big);
        write_u32(&mut legacy_tables, 0, Endian::Big);
        write_i32(&mut legacy_tables, 0, Endian::Big); // leaf node
        write_u32(&mut legacy_tables, 0, Endian::Big); // long FileID field
        write_i32(&mut legacy_tables, 0, Endian::Big); // object count
        write_i32(&mut legacy_tables, 0, Endian::Big); // script types
        write_i32(&mut legacy_tables, 0, Endian::Big); // external count
        write_string(&mut legacy_tables, ""); // user information
        let (legacy_file, legacy_header) = make_file(
            FormatVersion::HasScriptTypeIndex,
            Endian::Big,
            &legacy_tables,
            &[],
        );
        let legacy = SerializedFileMetadata::parse_file(&legacy_file, &legacy_header).unwrap();
        let legacy_node = &legacy.types[0].type_tree.as_ref().unwrap().nodes[0];
        assert_eq!(legacy_node.type_name.as_deref(), Some("int"));
        assert_eq!(legacy_node.name.as_deref(), Some("value"));
    }

    #[test]
    fn serialized_type_script_id_hash_presence_tracks_unity_version_fallback() {
        for (version, expect_script_id) in [
            ("2017.4.40f1", false),
            ("2018.3.0a1", true),
            ("unparseable", true),
        ] {
            let mut tables = Vec::new();
            write_i32(&mut tables, 1, Endian::Little); // type count
            write_i32(&mut tables, 28, Endian::Little);
            tables.push(0); // stripped flag
            write_i16(&mut tables, 1, Endian::Little); // script type index
            if expect_script_id {
                for word in [1, 2, 3, 4] {
                    write_u32(&mut tables, word, Endian::Little);
                }
            }
            for word in [5, 6, 7, 8] {
                write_u32(&mut tables, word, Endian::Little); // old type hash
            }
            push_empty_modern_tree(&mut tables, FormatVersion::RefactorTypeData, Endian::Little);
            write_i32(&mut tables, 1, Endian::Little); // objects
            align_table_position(FormatVersion::RefactorTypeData, &mut tables, 20);
            write_i64(
                &mut tables,
                100 + i64::from(expect_script_id),
                Endian::Little,
            );
            write_u32(&mut tables, 0, Endian::Little);
            write_i32(&mut tables, 0, Endian::Little);
            write_i32(&mut tables, 0, Endian::Little); // serialized type index
            write_i32(&mut tables, 0, Endian::Little); // scripts
            write_i32(&mut tables, 0, Endian::Little); // externals
            write_string(&mut tables, ""); // user information

            let (file, header) = make_file_with_version(
                FormatVersion::RefactorTypeData,
                Endian::Little,
                &tables,
                &[],
                version,
            );
            let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
            assert_eq!(
                metadata.types[0].script_id.is_some(),
                expect_script_id,
                "Unity version {version}"
            );
            assert_eq!(metadata.objects[0].resolved_type_index, Some(0));
        }
    }

    #[test]
    fn type_script_id_presence_matches_the_source_read_gate() {
        use super::should_read_type_script_id;
        // MonoBehaviour markers (`-1`, the clamped pre-16 negative-ID form) and
        // `MonoScript` (`114`) always carry the hash, whatever the script index,
        // flavor, or Unity version.
        for type_id in [-1, 114] {
            for (script, reference, version) in [
                (-1, false, None),
                (0, true, Some("5.4.0f1")),
                (-1, false, Some("2022.1.0f1")),
            ] {
                assert!(
                    should_read_type_script_id(type_id, script, reference, version),
                    "type {type_id}, script {script}, reference {reference}, version {version:?}"
                );
            }
        }
        // Otherwise a non-negative script index decides, except old Unity versions
        // ignore script types for plain types while reference types honor them.
        assert!(should_read_type_script_id(28, 0, false, Some("2020.3.0f1")));
        assert!(!should_read_type_script_id(
            28,
            0,
            false,
            Some("2017.4.0f1")
        ));
        assert!(should_read_type_script_id(28, 0, true, Some("2017.4.0f1")));
        // An unparseable version falls back to reading, like the source 0.0.0 default.
        assert!(should_read_type_script_id(
            28,
            0,
            false,
            Some("not-a-version")
        ));
        // Negative script indexes never carry the hash for ordinary classes.
        assert!(!should_read_type_script_id(
            28,
            -1,
            false,
            Some("2020.3.0f1")
        ));
        assert!(!should_read_type_script_id(28, -1, true, None));
    }

    #[test]
    fn generation_22_reads_64_bit_path_ids_offsets_and_checked_payload_ranges() {
        let generation = FormatVersion::LargeFilesSupport;
        let mut tables = Vec::new();
        write_i32(&mut tables, 1, Endian::Little); // types
        write_i32(&mut tables, 28, Endian::Little);
        tables.push(0); // stripped type
        write_i16(&mut tables, -1, Endian::Little); // script type index
        for word in [1, 2, 3, 4] {
            write_u32(&mut tables, word, Endian::Little); // old type hash
        }
        push_empty_modern_tree(&mut tables, generation, Endian::Little);
        write_i32(&mut tables, 0, Endian::Little); // type dependencies
        write_i32(&mut tables, 1, Endian::Little); // object count
        align_table_position(generation, &mut tables, 48);
        write_i64(&mut tables, 0x1234_5678_9abc_def0, Endian::Little);
        write_i64(&mut tables, 0, Endian::Little); // large payload offset
        write_i32(&mut tables, 1, Endian::Little); // payload size
        write_i32(&mut tables, 0, Endian::Little); // serialized type index
        write_i32(&mut tables, 0, Endian::Little); // script types
        write_i32(&mut tables, 0, Endian::Little); // externals
        write_i32(&mut tables, 0, Endian::Little); // reference types
        write_string(&mut tables, ""); // user information
        let (file, header) = make_file(generation, Endian::Little, &tables, &[0xa5]);

        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        assert_eq!(metadata.objects[0].path_id, 0x1234_5678_9abc_def0);
        assert_eq!(metadata.objects[0].byte_start, 0);
        assert_eq!(metadata.objects[0].byte_size, 1);
        assert_eq!(
            metadata.objects[0].data_range(&header),
            Some(header.data_offset..header.file_size)
        );
        assert_eq!(file[header.data_offset as usize], 0xa5);
    }

    #[test]
    fn parses_generation_20_reference_types_and_generation_21_dependencies() {
        let generation = FormatVersion::StoresTypeDependencies;
        let endian = Endian::Little;
        let mut tables = Vec::new();
        write_i32(&mut tables, 1, endian); // normal types
        write_i32(&mut tables, 114, endian);
        tables.push(0); // generation 16+ stripped type flag
        write_i16(&mut tables, -1, endian); // generation 17+ type script index
        for word in [1, 2, 3, 4, 5, 6, 7, 8] {
            write_u32(&mut tables, word, endian);
        }
        push_empty_modern_tree(&mut tables, generation, endian);
        write_i32(&mut tables, 2, endian);
        write_i32(&mut tables, 8, endian);
        write_i32(&mut tables, -1, endian);
        write_i32(&mut tables, 0, endian); // object count
        write_i32(&mut tables, 0, endian); // script type count
        write_i32(&mut tables, 0, endian); // external count
        write_i32(&mut tables, 1, endian); // one reference type
        write_i32(&mut tables, 114, endian);
        tables.push(0); // generation 16+ stripped type flag
        write_i16(&mut tables, -1, endian); // generation 17+ type script index
        for word in [11, 12, 13, 14, 15, 16, 17, 18] {
            write_u32(&mut tables, word, endian);
        }
        push_empty_modern_tree(&mut tables, generation, endian);
        write_string(&mut tables, "Example");
        write_string(&mut tables, "Example.Namespace");
        write_string(&mut tables, "Example.Assembly");
        write_string(&mut tables, ""); // user information

        let (file, header) = make_file(generation, endian, &tables, &[]);
        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        assert_eq!(metadata.types[0].type_dependencies, [8, -1]);
        assert_eq!(metadata.reference_types.len(), 1);
        assert_eq!(
            metadata.reference_types[0].class_name.as_deref(),
            Some("Example")
        );
        assert_eq!(
            metadata.reference_types[0].namespace.as_deref(),
            Some("Example.Namespace")
        );
        assert_eq!(
            metadata.reference_types[0].assembly_name.as_deref(),
            Some("Example.Assembly")
        );
    }

    #[test]
    fn malformed_count_and_object_range_return_contextual_errors() {
        let generation = FormatVersion::Unknown9;
        let endian = Endian::Little;
        let mut negative_count_tables = Vec::new();
        write_i32(&mut negative_count_tables, -1, endian);
        let (file, header) = make_file(generation, endian, &negative_count_tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::NegativeCount { .. }
        ));

        let mut out_of_bounds_tables = Vec::new();
        write_i32(&mut out_of_bounds_tables, 0, endian); // types
        write_i32(&mut out_of_bounds_tables, 1, endian); // objects
        write_i32(&mut out_of_bounds_tables, 1, endian); // PathID
        write_u32(&mut out_of_bounds_tables, 999, endian); // data offset outside file
        write_i32(&mut out_of_bounds_tables, 1, endian);
        write_i32(&mut out_of_bounds_tables, 0, endian); // type ID
        write_u16(&mut out_of_bounds_tables, 0, endian); // class ID
        write_i32(&mut out_of_bounds_tables, 0, endian); // externals
        write_string(&mut out_of_bounds_tables, ""); // user info
        let (file, header) = make_file(generation, endian, &out_of_bounds_tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::ObjectDataOutOfBounds { .. }
        ));
    }

    #[test]
    fn type_table_index_out_of_bounds_is_not_silently_ignored() {
        let generation = FormatVersion::RefactorTypeData;
        let endian = Endian::Little;
        let mut tables = Vec::new();
        write_i32(&mut tables, 0, endian); // types
        write_i32(&mut tables, 1, endian); // objects
        write_i64(&mut tables, 77, endian); // aligned PathID; initial metadata absolute offset is aligned here
        write_u32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, 4, endian); // invalid type table index
        write_i32(&mut tables, 0, endian); // scripts
        write_i32(&mut tables, 0, endian); // externals
        write_string(&mut tables, "");
        let (file, header) = make_file(generation, endian, &tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::InvalidTypeIndex { type_index: 4, .. }
        ));
    }

    #[test]
    fn modern_object_records_resolve_types_and_reject_bad_references() {
        fn modern_tables(type_index: i32) -> Vec<u8> {
            let generation = FormatVersion::TypeTreeNodeWithTypeFlags;
            let endian = Endian::Little;
            let mut tables = Vec::new();
            write_i32(&mut tables, 1, endian); // type count
            write_i32(&mut tables, 28, endian);
            tables.push(1); // stripped type flag
            write_i16(&mut tables, -1, endian); // type script index
            for word in [1, 2, 3, 4] {
                write_u32(&mut tables, word, endian);
            }
            push_empty_modern_tree(&mut tables, generation, endian);
            write_i32(&mut tables, 1, endian); // object count
            align_table_position(generation, &mut tables, 20);
            write_i64(&mut tables, 5, endian); // PathID
            write_u32(&mut tables, 0, endian);
            write_i32(&mut tables, 0, endian);
            write_i32(&mut tables, type_index, endian);
            write_i32(&mut tables, 0, endian); // scripts
            write_i32(&mut tables, 0, endian); // externals
            write_string(&mut tables, "");
            tables
        }

        // A resolved modern object backfills the raw ID and the script/stripped
        // slots from its type while the absent pre-17 fields stay empty, matching
        // the source defaults.
        let generation = FormatVersion::TypeTreeNodeWithTypeFlags;
        let endian = Endian::Little;
        let (file, header) = make_file(generation, endian, &modern_tables(0), &[]);
        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        let object = &metadata.objects[0];
        assert_eq!(object.path_id, 5);
        assert_eq!(object.serialized_type_index, Some(0));
        assert_eq!(object.raw_type_id, 28);
        assert_eq!(object.class_id, None);
        assert_eq!(object.script_type_index, Some(-1));
        assert_eq!(object.is_destroyed, None);
        assert_eq!(object.is_stripped, Some(true));
        assert_eq!(object.resolved_type_index, Some(0));

        // A negative serialized type index scans for TypeID 0 instead of
        // indexing, so a table without it is an explicit error.
        let (file, header) = make_file(generation, endian, &modern_tables(-1), &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::ObjectTypeNotFound { raw_type_id: 0, .. }
        ));

        // Duplicate TypeIDs on the scan path stay ambiguous instead of taking
        // the first match.
        let generation = FormatVersion::RefactorTypeData;
        let mut tables = Vec::new();
        write_i32(&mut tables, 2, endian); // type count
        for base in [1, 5] {
            write_i32(&mut tables, 0, endian);
            tables.push(0);
            write_i16(&mut tables, -1, endian);
            for word in base..base + 4 {
                write_u32(&mut tables, word, endian);
            }
            push_empty_modern_tree(&mut tables, generation, endian);
        }
        write_i32(&mut tables, 1, endian); // object count
        align_table_position(generation, &mut tables, 20);
        write_i64(&mut tables, 3, endian); // PathID
        write_u32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, -1, endian); // negative index scans for TypeID 0
        let (file, header) = make_file(generation, endian, &tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::AmbiguousObjectType { raw_type_id: 0, .. }
        ));

        // A negative large-file payload offset is rejected, not wrapped.
        let generation = FormatVersion::LargeFilesSupport;
        let mut tables = Vec::new();
        write_i32(&mut tables, 0, endian); // types
        write_i32(&mut tables, 1, endian); // objects
        align_table_position(generation, &mut tables, 48);
        write_i64(&mut tables, 9, endian); // PathID
        write_i64(&mut tables, -1, endian); // negative payload offset
        let (file, header) = make_file(generation, endian, &tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::NegativeObjectOffset(-1)
        ));

        // A negative payload size is rejected on the legacy integer path too.
        let generation = FormatVersion::Unknown9;
        let mut tables = Vec::new();
        write_i32(&mut tables, 0, endian); // types
        write_u32(&mut tables, 0, endian); // generation 7-13 long FileID field
        write_i32(&mut tables, 1, endian); // objects
        write_i32(&mut tables, 9, endian); // PathID
        write_u32(&mut tables, 0, endian);
        write_i32(&mut tables, -5, endian); // negative payload size
        let (file, header) = make_file(generation, endian, &tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::NegativeObjectSize(-5)
        ));
    }

    #[test]
    fn typetree_reader_matches_reference_runtime() {
        use super::{has_ref_type_hash, is_format5, read_type_tree};
        use crate::io::binary::BinaryReader;
        use crate::serialized_file::common_strings;

        fn hex_to_bytes(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "odd hex length for {hex}");
            let mut out = Vec::with_capacity(hex.len() / 2);
            let bytes = hex.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let hi = hex_val(bytes[i]);
                let lo = hex_val(bytes[i + 1]);
                out.push((hi << 4) | lo);
                i += 2;
            }
            out
        }

        fn hex_val(b: u8) -> u8 {
            match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => panic!("invalid hex byte {b}"),
            }
        }

        fn parse_kv<'a>(chunk: &'a str, key: &str) -> &'a str {
            let prefix = format!("{key}=");
            for part in chunk.split(' ') {
                if let Some(v) = part.strip_prefix(&prefix) {
                    return v;
                }
            }
            panic!("missing {key} in {chunk}");
        }

        let fixture = include_str!("typetree_reader_fixture.txt");
        let mut preds: Vec<(i32, bool, bool)> = Vec::new();
        let mut commons: Vec<(u32, String)> = Vec::new();
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("PRED|") {
                let parts: Vec<&str> = line.split('|').collect();
                assert_eq!(parts.len(), 4, "bad PRED {line}");
                preds.push((
                    parts[1].parse::<i32>().unwrap(),
                    parts[2] == "True",
                    parts[3] == "True",
                ));
            } else if line.starts_with("COMMON|") {
                let parts: Vec<&str> = line.splitn(4, '|').collect();
                assert_eq!(parts.len(), 4, "bad COMMON {line}");
                commons.push((parts[1].parse::<u32>().unwrap(), parts[3].to_string()));
            } else if line.contains("_HEX|") {
                let (name, hex) = line.split_once('|').unwrap();
                let name = name.strip_suffix("_HEX").unwrap().to_string();
                hexes.insert(name, hex_to_bytes(hex));
            } else if line.contains("_READ|") {
                let (name, dump) = line.split_once('|').unwrap();
                let name = name.strip_suffix("_READ").unwrap().to_string();
                reads.insert(name, dump.to_string());
            }
        }

        // Generation predicates cover every supported generation from the probe.
        assert!(preds.len() >= 21, "expected PRED rows, got {}", preds.len());
        for (raw, want_format5, want_hash) in preds {
            let generation = FormatVersion::try_from(raw).unwrap();
            assert_eq!(is_format5(generation), want_format5, "IsFormat5 gen {raw}");
            assert_eq!(
                has_ref_type_hash(generation),
                want_hash,
                "HasRefTypeHash gen {raw}"
            );
        }
        // The legacy/modern split is exactly generation 10 or 12+.
        for raw in [1, 2, 3, 5, 6, 7, 8, 9, 11] {
            assert!(
                !is_format5(FormatVersion::try_from(raw).unwrap()),
                "gen {raw}"
            );
        }
        for raw in [10, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22] {
            assert!(
                is_format5(FormatVersion::try_from(raw).unwrap()),
                "gen {raw}"
            );
        }

        // Full common-string table: every reference entry resolves identically.
        assert_eq!(commons.len(), 113, "expected 113 COMMON rows");
        for (offset, plain) in &commons {
            assert_eq!(
                common_strings::get(*offset).map(str::to_string),
                Some(plain.clone()),
                "common offset {offset}"
            );
        }
        for gap in [1u32, 2, 223, 9999, 5000] {
            assert_eq!(common_strings::get(gap), None, "gap offset {gap}");
        }

        // Nested and string-buffer payloads are C#-constructed in the probe.
        let cases: &[(&str, FormatVersion, Endian, bool)] = &[
            ("LEGACY_BE8", FormatVersion::Unknown8, Endian::Big, false),
            (
                "LEGACY_LE11",
                FormatVersion::HasScriptTypeIndex,
                Endian::Little,
                false,
            ),
            (
                "MODERN_LE10",
                FormatVersion::Unknown10,
                Endian::Little,
                false,
            ),
            (
                "MODERN_BE19",
                FormatVersion::TypeTreeNodeWithTypeFlags,
                Endian::Big,
                true,
            ),
            (
                "MODERN_LE22",
                FormatVersion::LargeFilesSupport,
                Endian::Little,
                true,
            ),
        ];
        for (name, generation, endian, want_hash) in cases {
            let bytes = hexes
                .get(*name)
                .unwrap_or_else(|| panic!("missing {name}_HEX"));
            let dump = reads
                .get(*name)
                .unwrap_or_else(|| panic!("missing {name}_READ"));
            assert!(!dump.starts_with("THROW"), "{name} unexpectedly threw");
            let mut reader = BinaryReader::new(bytes, *endian);
            let tree = read_type_tree(&mut reader, *generation, "fixture")
                .unwrap_or_else(|e| panic!("{name} Rust read failed: {e:?}"));
            assert_eq!(reader.remaining(), 0, "{name} trailing bytes");

            let (head, nodes_part) = dump.split_once(';').unwrap();
            let want_nodes: usize = parse_kv(head, "NODES").parse().unwrap();
            let want_buf_len: usize = parse_kv(head, "BUF").parse().unwrap();
            let want_buf_hex = parse_kv(head, "HEXBUF");
            assert_eq!(tree.nodes.len(), want_nodes, "{name} node count");
            assert_eq!(tree.string_buffer.len(), want_buf_len, "{name} buf len");
            assert_eq!(
                hex_to_bytes(want_buf_hex),
                tree.string_buffer,
                "{name} string buffer"
            );
            // Split `[i ...]` blocks without external crates.
            let mut blocks: Vec<String> = Vec::new();
            let mut cur = String::new();
            let mut open = false;
            for ch in nodes_part.chars() {
                if ch == '[' {
                    open = true;
                    cur.clear();
                } else if ch == ']' {
                    open = false;
                    blocks.push(std::mem::take(&mut cur));
                } else if open {
                    cur.push(ch);
                }
            }
            assert_eq!(blocks.len(), want_nodes, "{name} dump blocks");
            for (i, block) in blocks.iter().enumerate() {
                let node = &tree.nodes[i];
                let want_level: u8 = parse_kv(block, "L").parse().unwrap();
                let want_version: i32 = parse_kv(block, "V").parse().unwrap();
                let want_flags: i32 = parse_kv(block, "F").parse().unwrap();
                let want_type = parse_kv(block, "T");
                let want_name = parse_kv(block, "N");
                let want_size: i32 = parse_kv(block, "S").parse().unwrap();
                let want_index: i32 = parse_kv(block, "I").parse().unwrap();
                let want_meta: u32 = parse_kv(block, "M").parse().unwrap();
                let want_hash_hex = parse_kv(block, "H");
                let want_to_hex = parse_kv(block, "TO");
                let want_no_hex = parse_kv(block, "NO");
                assert_eq!(node.level, want_level, "{name} node {i} level");
                assert_eq!(node.version, want_version, "{name} node {i} version");
                assert_eq!(node.type_flags, want_flags, "{name} node {i} flags");
                assert_eq!(
                    node.type_name.as_deref(),
                    Some(want_type),
                    "{name} node {i} type"
                );
                assert_eq!(
                    node.name.as_deref(),
                    Some(want_name),
                    "{name} node {i} name"
                );
                assert_eq!(node.byte_size, want_size, "{name} node {i} size");
                assert_eq!(node.index, want_index, "{name} node {i} index");
                assert_eq!(node.meta_flags, want_meta, "{name} node {i} meta");
                if name.starts_with("LEGACY") {
                    assert_eq!(node.type_string_offset, None, "{name} node {i} TO");
                    assert_eq!(node.name_string_offset, None, "{name} node {i} NO");
                    assert_eq!(node.reference_type_hash, None, "{name} node {i} hash");
                } else {
                    let want_to = u32::from_str_radix(want_to_hex, 16).unwrap();
                    let want_no = u32::from_str_radix(want_no_hex, 16).unwrap();
                    assert_eq!(node.type_string_offset, Some(want_to), "{name} node {i} TO");
                    assert_eq!(node.name_string_offset, Some(want_no), "{name} node {i} NO");
                    if *want_hash {
                        let want_h = u64::from_str_radix(want_hash_hex, 16).unwrap();
                        assert_eq!(
                            node.reference_type_hash,
                            Some(want_h),
                            "{name} node {i} hash"
                        );
                    } else {
                        assert_eq!(node.reference_type_hash, None, "{name} node {i} hash");
                    }
                }
            }
        }

        // Negative node counts fail loudly on both sides.
        {
            let bytes = hexes.get("MODERN_NEG").expect("missing MODERN_NEG_HEX");
            let dump = reads.get("MODERN_NEG").expect("missing MODERN_NEG_READ");
            assert!(dump.starts_with("THROW:"), "reference should throw");
            let mut reader = BinaryReader::new(bytes, Endian::Little);
            let err = read_type_tree(&mut reader, FormatVersion::Unknown10, "fixture")
                .expect_err("negative count must fail");
            assert!(format!("{err:?}").len() > 8, "error carries context");
        }
    }

    #[test]
    fn scalar_fields_match_reference_constructed_files() {
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

        fn field<'a>(parts: &[&'a str], key: &str) -> &'a str {
            let prefix = format!("{key}=");
            for part in parts {
                if let Some(v) = part.strip_prefix(&prefix) {
                    return v;
                }
            }
            panic!("missing {key} in {parts:?}");
        }

        // Probe Temp/opencode/sermetascalarprobe (SerializedFileMetadata.Read
        // over C#-constructed full files, SDK 10.0.401): S1 gen8 parseable,
        // S2 gen8 unparseable (reference normalizes to 0.0.0a0 while Rust
        // preserves the raw text; both signal stripped), S3 gen14 without
        // the LongFileID field. Nothing here was produced by Rust.
        let fixture = include_str!("metadata_scalar_fixture.txt");
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("SCALAR_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad SCALAR_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("SCALAR_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad SCALAR_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        // S1: gen8 parseable signature, LongFileID, user info, platform.
        {
            let bytes = hexes.get("S1").expect("S1 hex");
            let dump = reads.get("S1").expect("S1 dump");
            let parts: Vec<&str> = dump.split('|').collect();
            assert_eq!(field(&parts, "V"), "8");
            assert_eq!(field(&parts, "UV"), "2019.4.1f1");
            assert_eq!(field(&parts, "Plat"), "19");
            assert_eq!(field(&parts, "Long"), "1");
            assert_eq!(field(&parts, "User"), "test user info");
            let header = SerializedFileHeader::parse(bytes).expect("S1 header");
            assert_eq!(header.format_version, FormatVersion::Unknown8);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("S1 metadata");
            assert_eq!(
                metadata.prefix.unity_version_text.as_deref(),
                Some("2019.4.1f1")
            );
            assert_eq!(metadata.prefix.target_platform, Some(19));
            assert_eq!(metadata.long_file_id, Some(1));
            assert_eq!(metadata.user_information.as_deref(), Some("test user info"));
            assert!(metadata.types.is_empty());
            assert!(metadata.objects.is_empty());
            assert!(metadata.externals.is_empty());
        }

        // S2: unparseable signatures fall back on both sides.
        {
            let bytes = hexes.get("S2").expect("S2 hex");
            let dump = reads.get("S2").expect("S2 dump");
            let parts: Vec<&str> = dump.split('|').collect();
            assert_eq!(field(&parts, "UV"), "0.0.0a0");
            let header = SerializedFileHeader::parse(bytes).expect("S2 header");
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("S2 metadata");
            assert_eq!(
                metadata.prefix.unity_version_text.as_deref(),
                Some("custom-private")
            );
            assert_eq!(metadata.long_file_id, Some(1));
            assert_eq!(metadata.user_information.as_deref(), Some(""));
        }

        // S3: gen14 has no LongFileID field and carries script types.
        {
            let bytes = hexes.get("S3").expect("S3 hex");
            let dump = reads.get("S3").expect("S3 dump");
            let parts: Vec<&str> = dump.split('|').collect();
            assert_eq!(field(&parts, "V"), "14");
            assert_eq!(field(&parts, "UV"), "2021.2.0f1");
            assert_eq!(field(&parts, "Long"), "0");
            assert_eq!(field(&parts, "User"), "gen14 user");
            let header = SerializedFileHeader::parse(bytes).expect("S3 header");
            assert_eq!(header.format_version, FormatVersion::Unknown14);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("S3 metadata");
            assert_eq!(
                metadata.prefix.unity_version_text.as_deref(),
                Some("2021.2.0f1")
            );
            assert_eq!(metadata.prefix.target_platform, Some(19));
            assert_eq!(metadata.long_file_id, None);
            assert!(metadata.script_types.is_empty());
            assert_eq!(metadata.user_information.as_deref(), Some("gen14 user"));
            assert!(metadata.types.is_empty());
            assert!(metadata.objects.is_empty());
            assert!(metadata.externals.is_empty());
        }
    }

    #[test]
    fn table_records_match_reference_constructed_files() {
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

        // Probe Temp/opencode/sermetatableprobe (SerializedFileMetadata.Read
        // over C#-constructed full files, SDK 10.0.401, zero types so no
        // type matching): T1 gen8, T2 gen12 with scripts, T3 gen20 with
        // 64-bit IDs and an empty ref-types table. Nothing Rust-generated.
        let fixture = include_str!("metadata_table_fixture.txt");
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("TABLE_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad TABLE_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("TABLE_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad TABLE_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        // T1: gen8 32-bit PathID with ClassID and IsDestroyed.
        {
            let bytes = hexes.get("T1").expect("T1 hex");
            let dump = reads.get("T1").expect("T1 dump");
            assert!(dump.contains("OBJ:1001,1,1,4"), "T1 object {dump}");
            assert!(dump.contains("EXT:library/extra.bin,0"), "T1 ext {dump}");
            let header = SerializedFileHeader::parse(bytes).expect("T1 header");
            assert_eq!(header.format_version, FormatVersion::Unknown8);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("T1 metadata");
            assert_eq!(metadata.objects.len(), 1);
            let object = &metadata.objects[0];
            assert_eq!(object.path_id, 1001);
            assert_eq!(object.byte_size, 4);
            assert_eq!(object.raw_type_id, 1);
            assert_eq!(object.class_id, Some(1));
            assert_eq!(object.is_destroyed, Some(0));
            assert_eq!(metadata.externals.len(), 1);
            let external = &metadata.externals[0];
            assert_eq!(external.path_name_origin, "library/extra.bin");
            assert_eq!(external.guid, Some([0, 0, 0, 0]));
            assert!(metadata.script_types.is_empty());
        }

        // T2: gen12 adds ScriptTypeIndex plus a script-identifier table.
        {
            let bytes = hexes.get("T2").expect("T2 hex");
            let dump = reads.get("T2").expect("T2 dump");
            assert!(dump.contains("OBJ:1001,1,1,0"), "T2 object {dump}");
            assert!(dump.contains("SCR:0,3003"), "T2 script {dump}");
            let header = SerializedFileHeader::parse(bytes).expect("T2 header");
            assert_eq!(header.format_version.raw(), 12);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("T2 metadata");
            assert_eq!(metadata.objects.len(), 1);
            let object = &metadata.objects[0];
            assert_eq!(object.path_id, 1001);
            assert_eq!(object.script_type_index, Some(0));
            assert_eq!(metadata.script_types.len(), 1);
            assert_eq!(metadata.script_types[0].local_serialized_file_index, 0);
            assert_eq!(metadata.script_types[0].local_identifier_in_file, 3003);
            assert_eq!(metadata.externals.len(), 1);
            assert_eq!(metadata.externals[0].path_name_origin, "library/extra.bin");
        }

        // T3: gen20 uses 64-bit IDs, a type-table index, and ref types.
        {
            let bytes = hexes.get("T3").expect("T3 hex");
            let dump = reads.get("T3").expect("T3 dump");
            assert!(dump.contains("OBJ:2002,0,0,0"), "T3 object {dump}");
            assert!(dump.contains("SCR:0,4004"), "T3 script {dump}");
            let header = SerializedFileHeader::parse(bytes).expect("T3 header");
            assert_eq!(header.format_version.raw(), 20);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("T3 metadata");
            assert_eq!(metadata.objects.len(), 1);
            let object = &metadata.objects[0];
            assert_eq!(object.path_id, 2002);
            assert_eq!(object.serialized_type_index, Some(-1));
            assert_eq!(object.byte_size, 0);
            assert_eq!(metadata.script_types.len(), 1);
            assert_eq!(metadata.script_types[0].local_identifier_in_file, 4004);
            assert!(metadata.reference_types.is_empty());
            assert_eq!(metadata.externals.len(), 1);
        }
    }

    #[test]
    fn type_records_match_reference_constructed_files() {
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

        // Probe Temp/opencode/sermetatypeprobe (SerializedFileMetadata.Read
        // over C#-constructed full files, SDK 10.0.401, zero objects so no
        // type-index linkage): Y1 gen8 legacy 1-node tree, Y2 gen12 modern
        // empty tree without hash, Y3 gen20 MonoBehaviour ScriptID plus hash,
        // Y4 gen21 hash-only plus deps [0], R1 gen21 managed ref with
        // class/namespace/assembly. Hash bytes are construction facts (AA/ BB/
        // CC/DD); the reference structural dump proves layout alignment and
        // Rust asserts the values. Nothing Rust-generated.
        let fixture = include_str!("metadata_type_fixture.txt");
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("TYPE_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad TYPE_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("TYPE_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad TYPE_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        // Y1: gen8 legacy 1-node TypeTree, no hash/stripped/script-index lanes.
        {
            let bytes = hexes.get("Y1").expect("Y1 hex");
            let dump = reads.get("Y1").expect("Y1 dump");
            assert!(dump.contains("TYP:1,False,-1,1,0"), "Y1 type {dump}");
            let header = SerializedFileHeader::parse(bytes).expect("Y1 header");
            assert_eq!(header.format_version.raw(), 8);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("Y1 metadata");
            assert_eq!(metadata.types.len(), 1);
            let ty = &metadata.types[0];
            assert_eq!(ty.raw_type_id, 1);
            assert!(!ty.is_stripped_type);
            assert_eq!(ty.script_type_index, -1);
            assert!(ty.script_id.is_none());
            assert!(ty.old_type_hash.is_none());
            let tree = ty.type_tree.as_ref().expect("Y1 tree");
            assert_eq!(tree.nodes.len(), 1);
            assert!(ty.type_dependencies.is_empty());
            assert!(metadata.objects.is_empty());
        }

        // Y2: gen12 modern empty tree, still no hash lane.
        {
            let bytes = hexes.get("Y2").expect("Y2 hex");
            let dump = reads.get("Y2").expect("Y2 dump");
            assert!(dump.contains("TYP:1,False,-1,0,0"), "Y2 type {dump}");
            let header = SerializedFileHeader::parse(bytes).expect("Y2 header");
            assert_eq!(header.format_version.raw(), 12);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("Y2 metadata");
            assert_eq!(metadata.types.len(), 1);
            let ty = &metadata.types[0];
            assert_eq!(ty.raw_type_id, 1);
            assert!(ty.script_id.is_none());
            assert!(ty.old_type_hash.is_none());
            let tree = ty.type_tree.as_ref().expect("Y2 tree");
            assert!(tree.nodes.is_empty());
            assert!(ty.type_dependencies.is_empty());
        }

        // Y3: gen20 MonoBehaviour reads ScriptID plus OldTypeHash.
        {
            let bytes = hexes.get("Y3").expect("Y3 hex");
            let dump = reads.get("Y3").expect("Y3 dump");
            assert!(dump.contains("TYP:114,False,0,0,0"), "Y3 type {dump}");
            let header = SerializedFileHeader::parse(bytes).expect("Y3 header");
            assert_eq!(header.format_version.raw(), 20);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("Y3 metadata");
            assert_eq!(metadata.types.len(), 1);
            let ty = &metadata.types[0];
            assert_eq!(ty.raw_type_id, 114);
            assert!(!ty.is_stripped_type);
            assert_eq!(ty.script_type_index, 0);
            assert_eq!(ty.script_id, Some([0xAAAAAAAA; 4]));
            assert_eq!(ty.old_type_hash, Some([0xBBBBBBBB; 4]));
            let tree = ty.type_tree.as_ref().expect("Y3 tree");
            assert!(tree.nodes.is_empty());
            assert!(ty.type_dependencies.is_empty());
        }

        // Y4: gen21 hash-only plus type dependencies [0].
        {
            let bytes = hexes.get("Y4").expect("Y4 hex");
            let dump = reads.get("Y4").expect("Y4 dump");
            assert!(dump.contains("TYP:1,False,-1,0,1"), "Y4 type {dump}");
            let header = SerializedFileHeader::parse(bytes).expect("Y4 header");
            assert_eq!(header.format_version.raw(), 21);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("Y4 metadata");
            assert_eq!(metadata.types.len(), 1);
            let ty = &metadata.types[0];
            assert_eq!(ty.raw_type_id, 1);
            assert_eq!(ty.script_type_index, -1);
            assert!(ty.script_id.is_none());
            assert_eq!(ty.old_type_hash, Some([0xBBBBBBBB; 4]));
            assert_eq!(ty.type_dependencies, vec![0]);
        }

        // R1: gen21 managed reference with class/namespace/assembly.
        {
            let bytes = hexes.get("R1").expect("R1 hex");
            let dump = reads.get("R1").expect("R1 dump");
            assert!(
                dump.contains("REF:-1,0,MyClass,MyNamespace,MyAssembly,0"),
                "R1 ref {dump}"
            );
            let header = SerializedFileHeader::parse(bytes).expect("R1 header");
            assert_eq!(header.format_version.raw(), 21);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("R1 metadata");
            assert!(metadata.types.is_empty());
            assert_eq!(metadata.reference_types.len(), 1);
            let rf = &metadata.reference_types[0];
            assert_eq!(rf.raw_type_id, -1);
            assert_eq!(rf.script_type_index, 0);
            assert_eq!(rf.script_id, Some([0xCCCCCCCC; 4]));
            assert_eq!(rf.old_type_hash, Some([0xDDDDDDDD; 4]));
            assert_eq!(rf.class_name.as_deref(), Some("MyClass"));
            assert_eq!(rf.namespace.as_deref(), Some("MyNamespace"));
            assert_eq!(rf.assembly_name.as_deref(), Some("MyAssembly"));
            let tree = rf.type_tree.as_ref().expect("R1 tree");
            assert!(tree.nodes.is_empty());
        }
    }

    #[test]
    fn metadata_envelopes_match_reference_constructed_files() {
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

        // Probe Temp/opencode/sermetaenvprobe (SerializedFileMetadata.Read
        // over C#-constructed minimal files, SDK 10.0.401): G8-G22 envelope
        // presence gates plus E1 gen10 full file with a 16-byte object
        // payload. Nothing Rust-generated.
        let fixture = include_str!("metadata_envelope_fixture.txt");
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("ENV_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad ENV_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("ENV_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad ENV_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        // Envelope gates reproduce across all covered generations: empty
        // tables with presence-correct sections on both sides.
        for gen in [8u8, 10, 11, 12, 13, 14, 17, 19, 20, 21, 22] {
            let case = format!("G{gen}");
            let bytes = hexes.get(&case).unwrap_or_else(|| panic!("missing {case}"));
            let dump = reads.get(&case).unwrap_or_else(|| panic!("missing {case}"));
            assert!(
                dump.contains(&format!("V={gen}|Types=0|Objs=0|Scripts=0|Ext=0|Refs=0")),
                "{case} envelope {dump}"
            );
            let header = SerializedFileHeader::parse(bytes)
                .unwrap_or_else(|e| panic!("{case} header: {e:?}"));
            assert_eq!(header.format_version.raw(), gen as i32, "{case} version");
            let metadata = SerializedFileMetadata::parse_file(bytes, &header)
                .unwrap_or_else(|e| panic!("{case} metadata: {e:?}"));
            assert!(metadata.types.is_empty(), "{case} types");
            assert!(metadata.objects.is_empty(), "{case} objects");
            assert!(metadata.script_types.is_empty(), "{case} scripts");
            assert!(metadata.externals.is_empty(), "{case} externals");
            assert!(metadata.reference_types.is_empty(), "{case} reftypes");
        }

        // E1: gen10 file entry carries the 16-byte object payload.
        {
            let bytes = hexes.get("E1").expect("E1 hex");
            let dump = reads.get("E1").expect("E1 dump");
            assert!(
                dump.contains("OBJ:5001,1,1,16=000102030405060708090A0B0C0D0E0F"),
                "E1 object {dump}"
            );
            let header = SerializedFileHeader::parse(bytes).expect("E1 header");
            assert_eq!(header.format_version.raw(), 10);
            let metadata = SerializedFileMetadata::parse_file(bytes, &header).expect("E1 metadata");
            assert_eq!(metadata.types.len(), 1);
            assert_eq!(metadata.types[0].raw_type_id, 1);
            assert_eq!(metadata.objects.len(), 1);
            let object = &metadata.objects[0];
            assert_eq!(object.path_id, 5001);
            assert_eq!(object.byte_size, 16);
            let range = object.data_range(&header).expect("E1 range");
            let data = bytes
                .get(range.start as usize..range.end as usize)
                .expect("E1 data");
            assert_eq!(data.len(), 16);
            assert_eq!(data, (0..16).collect::<Vec<u8>>());
            assert_eq!(metadata.externals.len(), 1);
        }
    }

    /// `UnityGuid.ToString()` rendering, replayed against the reference runtime.
    ///
    /// `FileIdentifier.GetFilePath` returns this rendering for an `AssetType.Meta` identifier, so it
    /// reaches every `.meta` guid line. The rows come from `tools/probes/guid-render`, which
    /// constructs a `UnityGuid` from the same four words and records `ToString()`, so the
    /// expectation does not depend on this module. The sign-bit and all-ones rows are the ones that
    /// would expose a shift or signedness mistake.
    #[test]
    fn file_identifier_meta_guid_rendering_matches_the_reference_runtime() {
        const META: i32 = 3;
        let fixture = include_str!("unity_guid_render_fixture.txt");
        let mut rows = 0usize;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let rest = line.strip_prefix("GUID|").expect("GUID row");
            let (words_text, expected) = rest.split_once('|').expect("words and text");
            let mut words = [0u32; 4];
            for (slot, word) in words.iter_mut().zip(words_text.split(',')) {
                *slot = u32::from_str_radix(word, 16).expect("word hex");
            }
            let identifier = super::FileIdentifier {
                asset_path: None,
                guid: Some(words),
                file_type: Some(META),
                path_name_origin: String::new(),
            };
            assert_eq!(
                identifier.get_file_path().as_deref(),
                Some(expected),
                "words {words_text}"
            );
            rows += 1;
        }
        assert_eq!(rows, 6, "every recorded row is replayed");

        // A Meta identifier without a GUID is malformed and must not invent one, and a non-Meta
        // identifier uses the normalized path instead of a GUID.
        let no_guid = super::FileIdentifier {
            asset_path: None,
            guid: None,
            file_type: Some(META),
            path_name_origin: "archive:/CAB-1".to_owned(),
        };
        assert_eq!(no_guid.get_file_path(), None);
        let not_meta = super::FileIdentifier {
            asset_path: None,
            guid: Some([0x0102_0304, 0, 0, 0]),
            file_type: Some(0),
            path_name_origin: "archive:/CAB-1".to_owned(),
        };
        assert_eq!(
            not_meta.get_file_path().as_deref(),
            Some(not_meta.fixed_path_name().as_str()),
            "a non-Meta identifier uses the normalized path"
        );
    }
