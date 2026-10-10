    use super::{
        BatchMountErrorKind, BatchMountLimits, ContainerMountErrorKind, ContainerMountLimits,
        FileKind, MountedContainerBundle, MountedExplicitResourceFile, MountedResourceFile,
        MountedSerializedFile, PathMountInput, SchemeErrorKind, SchemeReader, SchemeReaderLimits,
        WrapperKind,
    };
    use crate::compression::wrappers::{
        compress_gzip, decompress_brotli, has_brotli_file_signature,
    };
    use crate::io::filesystem::VirtualFileSystem;
    use std::io::{Cursor, Read, Seek, SeekFrom};
    use std::path::PathBuf;

    fn unique_temp_file_path(extension: &str) -> PathBuf {
        crate::test_support::unique_test_path("mount", extension)
    }

    fn source_multifile_path_fields(prefix: &str) -> Vec<&'static str> {
        include_str!("io/multifile_path_fixture.txt")
            .lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing source MultiFile fixture row {prefix:?}"))
            .split('|')
            .collect()
    }

    fn decode_hex_fixture(value: &str) -> Vec<u8> {
        assert!(value.len().is_multiple_of(2));
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let high = (pair[0] as char).to_digit(16).unwrap();
                let low = (pair[1] as char).to_digit(16).unwrap();
                u8::try_from((high << 4) | low).unwrap()
            })
            .collect()
    }

    fn webdata_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
        assert!(entries.iter().all(|(name, _)| name.len() < 1024 * 1024));
        let header_size = 20
            + entries
                .iter()
                .map(|(name, _)| 12 + name.len())
                .sum::<usize>();
        let mut input = b"UnityWebData1.0\0".to_vec();
        input.extend_from_slice(&(header_size as i32).to_le_bytes());

        let mut data_offset = header_size;
        for (name, payload) in entries {
            input.extend_from_slice(&(data_offset as i32).to_le_bytes());
            input.extend_from_slice(&(payload.len() as i32).to_le_bytes());
            input.extend_from_slice(&(name.len() as u32).to_le_bytes());
            input.extend_from_slice(name.as_bytes());
            data_offset += payload.len();
        }
        for (_, payload) in entries {
            input.extend_from_slice(payload);
        }
        input
    }

    fn serialized_file_header_fixture() -> Vec<u8> {
        // Scheme detection only needs a valid legacy header envelope; this deliberately minimal
        // payload is not passed to the SerializedFile parser.
        let mut bytes = vec![0; 29];
        bytes[0..4].copy_from_slice(&13_i32.to_be_bytes());
        bytes[4..8].copy_from_slice(&29_u32.to_be_bytes());
        bytes[8..12].copy_from_slice(&6_i32.to_be_bytes());
        bytes
    }

    fn write_bits(bytes: &mut Vec<u8>, bit_position: &mut usize, value: u32, count: usize) {
        for bit in 0..count {
            let byte_index = *bit_position / 8;
            if bytes.len() <= byte_index {
                bytes.resize(byte_index + 1, 0);
            }
            if value & (1 << bit) != 0 {
                bytes[byte_index] |= 1 << (*bit_position % 8);
            }
            *bit_position += 1;
        }
    }

    fn unity_marked_brotli_fixture(output: &[u8], window_bits: u8) -> Vec<u8> {
        let marker = b"UnityWeb Compressed Content (brotli)";
        let mut bytes = Vec::new();
        let mut bit_position = 0;

        // RFC 7932 stream header, followed by an empty metadata meta-block.
        let (window_code, window_code_bits) = match window_bits {
            10 => (33, 7),
            11 => (49, 7),
            12 => (65, 7),
            13 => (81, 7),
            14 => (97, 7),
            15 => (113, 7),
            16 => (0, 1),
            17 => (1, 7),
            18 => (3, 4),
            19 => (5, 4),
            20 => (7, 4),
            21 => (9, 4),
            22 => (11, 4),
            23 => (13, 4),
            24 => (15, 4),
            _ => panic!("invalid fixture WBITS"),
        };
        write_bits(&mut bytes, &mut bit_position, window_code, window_code_bits);
        write_bits(&mut bytes, &mut bit_position, 0, 1); // ISLAST
        write_bits(&mut bytes, &mut bit_position, 3, 2); // MNIBBLES=0
        write_bits(&mut bytes, &mut bit_position, 0, 1); // reserved
        write_bits(&mut bytes, &mut bit_position, 1, 2); // MSKIPBYTES=1
        write_bits(&mut bytes, &mut bit_position, marker.len() as u32 - 1, 8);
        while bit_position & 7 != 0 {
            write_bits(&mut bytes, &mut bit_position, 0, 1);
        }
        bytes.extend_from_slice(marker);
        bit_position += marker.len() * 8;

        // Non-final uncompressed meta-block containing the requested payload.
        let length_minus_one = output.len().checked_sub(1).unwrap() as u32;
        write_bits(&mut bytes, &mut bit_position, 0, 1); // ISLAST=0
        write_bits(&mut bytes, &mut bit_position, 0, 2); // MNIBBLES=4
        write_bits(&mut bytes, &mut bit_position, length_minus_one, 16);
        write_bits(&mut bytes, &mut bit_position, 1, 1); // ISUNCOMPRESSED
        while bit_position & 7 != 0 {
            write_bits(&mut bytes, &mut bit_position, 0, 1);
        }
        bytes.extend_from_slice(output);
        bit_position += output.len() * 8;

        // Final empty meta-block.
        write_bits(&mut bytes, &mut bit_position, 1, 1); // ISLAST
        write_bits(&mut bytes, &mut bit_position, 1, 1); // ISLASTEMPTY
        while bit_position & 7 != 0 {
            write_bits(&mut bytes, &mut bit_position, 0, 1);
        }
        bytes
    }

    #[test]
    fn dispatches_nested_gzip_wrappers_to_the_inner_resource() {
        let once = compress_gzip(b"inner payload", 1024).unwrap();
        let twice = compress_gzip(&once, 1024).unwrap();
        let file = SchemeReader::read(&twice).unwrap();

        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"inner payload");
        assert_eq!(
            file.wrappers()
                .iter()
                .map(|layer| layer.kind)
                .collect::<Vec<_>>(),
            [WrapperKind::Gzip, WrapperKind::Gzip]
        );
        assert!(matches!(
            file.parse().unwrap(),
            super::ParsedFile::Resource(_)
        ));
    }

    #[test]
    fn dispatches_a_complete_seekable_stream_from_its_start() {
        let input = compress_gzip(b"stream payload", 1024).unwrap();
        let mut stream = Cursor::new(input);
        stream.seek(SeekFrom::End(0)).unwrap();

        let file = SchemeReader::read_stream(&mut stream, SchemeReaderLimits::default()).unwrap();
        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"stream payload");
        assert_eq!(file.wrappers()[0].kind, WrapperKind::Gzip);
    }

    #[test]
    fn seekable_stream_dispatch_matches_both_source_scheme_reader_routes() {
        let fixture = include_str!("io/io_differential_stream_fixture.txt");
        let mut path_rows = std::collections::BTreeMap::new();
        let mut stream_rows = Vec::new();
        for line in fixture.lines() {
            if let Some(rest) = line.strip_prefix("SCHEME|") {
                let mut fields = rest.split('|');
                let case = fields.next().expect("scheme case");
                let input = fields
                    .next()
                    .and_then(|field| field.strip_prefix("in="))
                    .unwrap_or_else(|| panic!("{case}: missing source input"));
                let outcome = fields.collect::<Vec<_>>().join("|");
                assert!(
                    path_rows
                        .insert(case.to_owned(), (decode_hex_fixture(input), outcome))
                        .is_none(),
                    "duplicate source path case {case}"
                );
            } else if let Some(rest) = line.strip_prefix("SCHEME_STREAM|") {
                stream_rows.push(rest);
            }
        }
        assert_eq!(path_rows.len(), 47, "source LoadFile cases");
        assert_eq!(
            stream_rows.len(),
            path_rows.len(),
            "source SmartStream cases"
        );

        fn expected_scheme_name(file: &super::DispatchedFile<'_>) -> &'static str {
            match file.wrappers().first().map(|wrapper| wrapper.kind) {
                Some(WrapperKind::Gzip) => "GZipFile",
                Some(WrapperKind::Brotli) => "BrotliFile",
                None => match file.kind() {
                    super::FileKind::UnityFs => "FileStreamBundleFile",
                    super::FileKind::UnityRaw => "RawBundleFile",
                    super::FileKind::UnityWebBundle => "WebBundleFile",
                    super::FileKind::UnityArchive => "ArchiveBundleFile",
                    super::FileKind::UnityWebData => "WebFile",
                    super::FileKind::SerializedFile => "SerializedFile",
                    super::FileKind::Resource => "ResourceFile",
                },
            }
        }

        for rest in stream_rows {
            let mut fields = rest.split('|');
            let case = fields.next().expect("stream case");
            let source_outcome = fields.collect::<Vec<_>>().join("|");
            let (input, path_outcome) = path_rows
                .get(case)
                .unwrap_or_else(|| panic!("{case}: no matching source LoadFile case"));
            assert_eq!(
                source_outcome, *path_outcome,
                "{case}: LoadFile and ReadFile(SmartStream) source outcomes"
            );

            let in_memory = SchemeReader::read_with_limits(input, SchemeReaderLimits::default())
                .unwrap_or_else(|error| panic!("{case}: in-memory dispatch failed: {error}"));
            let mut cursor = Cursor::new(input.clone());
            let streamed = SchemeReader::read_stream(&mut cursor, SchemeReaderLimits::default())
                .unwrap_or_else(|error| panic!("{case}: seekable-stream dispatch failed: {error}"));
            assert_eq!(streamed.kind(), in_memory.kind(), "{case}: dispatched kind");
            assert_eq!(
                streamed.wrappers(),
                in_memory.wrappers(),
                "{case}: wrappers"
            );
            assert_eq!(
                streamed.bytes(),
                in_memory.bytes(),
                "{case}: dispatched bytes"
            );

            if let Some(source_throw) = source_outcome.strip_prefix("THROW:") {
                assert!(
                    streamed.parse().is_err(),
                    "{case}: reference threw {source_throw}, Rust parsed"
                );
                continue;
            }

            let source_type = source_outcome
                .split('|')
                .find_map(|field| field.strip_prefix("type="))
                .unwrap_or_else(|| panic!("{case}: missing source type"));
            if source_type == "ResourceFile" && has_brotli_file_signature(input) {
                // The frozen source detector misses this RFC-valid marker; its direct Brotli
                // decoder accepts the same bytes. Rust keeps the source bug classified in
                // DIFF-BROTLI-METADATA-001 and recognizes the valid wrapper here.
                assert_eq!(
                    expected_scheme_name(&streamed),
                    "BrotliFile",
                    "{case}: scheme"
                );
                assert!(matches!(streamed.kind(), super::FileKind::UnityFs));
            } else {
                assert_eq!(
                    expected_scheme_name(&streamed),
                    source_type,
                    "{case}: scheme"
                );
            }
            let source_fixed = source_outcome
                .split('|')
                .find_map(|field| field.strip_prefix("fixed="))
                .unwrap_or_else(|| panic!("{case}: missing source fixed name"));
            assert_eq!(source_fixed, format!("{case}.bin"), "{case}: file name");
        }
    }

    #[test]
    fn seekable_stream_wrapper_decode_does_not_read_into_a_whole_input_buffer() {
        struct CountedCursor {
            cursor: Cursor<Vec<u8>>,
            largest_read_request: usize,
        }

        impl Read for CountedCursor {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.largest_read_request = self.largest_read_request.max(buffer.len());
                self.cursor.read(buffer)
            }
        }

        impl Seek for CountedCursor {
            fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
                self.cursor.seek(position)
            }
        }

        let mut state = 0x1234_5678u32;
        let payload_len = 1024 * 1024;
        let mut payload = Vec::with_capacity(payload_len);
        for _ in 0..payload_len {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            payload.push((state >> 24) as u8);
        }
        let compressed = compress_gzip(&payload, payload.len() + 64 * 1024).unwrap();
        assert!(compressed.len() > 256 * 1024);
        let compressed_len = compressed.len();
        let mut stream = CountedCursor {
            cursor: Cursor::new(compressed),
            largest_read_request: 0,
        };

        let dispatched = SchemeReader::read_stream(&mut stream, SchemeReaderLimits::default())
            .expect("bounded wrapper streaming dispatch");
        assert_eq!(dispatched.kind(), super::FileKind::Resource);
        assert_eq!(dispatched.bytes(), payload);
        assert!(
            stream.largest_read_request < compressed_len,
            "compressed input was requested as one whole buffer: largest read {}, input {compressed_len}",
            stream.largest_read_request
        );
    }

    #[test]
    fn enforces_the_input_limit_before_allocating_stream_contents() {
        let mut stream = Cursor::new(b"too many input bytes".to_vec());
        let error = SchemeReader::read_stream(
            &mut stream,
            SchemeReaderLimits {
                max_input_bytes: 4,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind(),
            SchemeErrorKind::InputSizeLimitExceeded { limit: 4, .. }
        ));
    }

    #[test]
    fn recursively_dispatches_webdata_entries() {
        let payload = compress_gzip(b"embedded asset", 1024).unwrap();
        let name = b"main.asset";
        let header_size = 16 + 4 + 4 + 4 + 4 + name.len();
        let mut input = b"UnityWebData1.0\0".to_vec();
        input.extend_from_slice(&(header_size as i32).to_le_bytes());
        input.extend_from_slice(&(header_size as i32).to_le_bytes());
        input.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        input.extend_from_slice(&(name.len() as u32).to_le_bytes());
        input.extend_from_slice(name);
        input.extend_from_slice(&payload);

        let web_file = SchemeReader::read(&input).unwrap();
        assert_eq!(web_file.kind(), FileKind::UnityWebData);
        let entry = web_file.dispatch_entry(0).unwrap();
        assert_eq!(entry.bytes(), b"embedded asset");
        assert_eq!(entry.wrappers()[0].kind, WrapperKind::Gzip);
        assert!(matches!(
            entry.parse().unwrap(),
            super::ParsedFile::Resource(_)
        ));
    }

    #[test]
    fn mounts_webdata_entries_with_parent_directories() {
        let input = webdata_fixture(&[
            ("nested/asset.bin", b"embedded asset"),
            ("other.bin", b"second"),
        ]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = file
            .mount_container_entries(
                &mut filesystem,
                Some("/bundles/sample"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.mounted_entries, 2);
        assert_eq!(report.mounted_bytes, 20);
        assert_eq!(filesystem.count(), 6);
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/sample/nested/asset.bin", 64)
                .unwrap(),
            b"embedded asset"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/sample/other.bin", 64)
                .unwrap(),
            b"second"
        );
    }

    #[test]
    fn container_mount_limits_fail_before_changing_the_virtual_filesystem() {
        let input = webdata_fixture(&[("first.bin", b"first"), ("second.bin", b"second")]);
        let file = SchemeReader::read(&input).unwrap();

        let mut count_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_entries(
                &mut count_limited,
                Some("/bundle"),
                ContainerMountLimits {
                    max_entries: 1,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::EntryCountLimitExceeded { count: 2, limit: 1 }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert_eq!(count_limited.count(), 1);

        let mut byte_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_entries(
                &mut byte_limited,
                Some("/bundle"),
                ContainerMountLimits {
                    max_total_output_bytes: 5,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::TotalOutputLimitExceeded { .. }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert_eq!(byte_limited.count(), 1);
    }

    #[test]
    fn container_mount_reports_partial_progress_on_path_collision() {
        let input = webdata_fixture(&[("new.bin", b"new"), ("existing.bin", b"incoming")]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/mount")).unwrap();
        filesystem
            .write_all_bytes("/mount/existing.bin", b"original")
            .unwrap();

        let error = file
            .mount_container_entries(
                &mut filesystem,
                Some("/mount"),
                ContainerMountLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::FileSystem { entry_index: 1, .. }
        ));
        assert_eq!(error.partial_report.mounted_entries, 1);
        assert_eq!(error.partial_report.mounted_bytes, 3);
        assert_eq!(
            filesystem.read_all_bytes("/mount/new.bin", 32).unwrap(),
            b"new"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/mount/existing.bin", 32)
                .unwrap(),
            b"original"
        );
    }

    #[test]
    fn container_mount_rejects_parent_directory_paths() {
        let input = webdata_fixture(&[("../escape.bin", b"data")]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_entries(
                &mut filesystem,
                Some("/bundle"),
                ContainerMountLimits::default(),
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::InvalidEntryPath { entry_index: 0, .. }
        ));
        assert_eq!(filesystem.count(), 1);
    }

    #[test]
    fn recursively_mounts_nested_and_wrapper_containers_under_their_entry_paths() {
        let nested = webdata_fixture(&[("leaf.bin", b"nested leaf")]);
        let wrapped_nested = compress_gzip(&nested, 4096).unwrap();
        let input = webdata_fixture(&[
            ("plain-container", &nested),
            ("wrapped-container.gz", &wrapped_nested),
        ]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.mounted_entries, 2);
        assert_eq!(report.mounted_bytes, 22);
        assert_eq!(
            report.container_scopes,
            [
                "/tree",
                "/tree/plain-container",
                "/tree/wrapped-container.gz"
            ]
        );
        assert_eq!(
            report.container_bundles,
            [
                MountedContainerBundle {
                    scope_index: Some(0),
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    name_fixed: None,
                },
                MountedContainerBundle {
                    scope_index: Some(1),
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(0),
                    name_fixed: Some("plain-container".to_owned()),
                },
                MountedContainerBundle {
                    scope_index: Some(2),
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(1),
                    name_fixed: Some("wrapped-container.gz".to_owned()),
                },
            ]
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/tree/plain-container/leaf.bin", 64)
                .unwrap(),
            b"nested leaf"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/tree/wrapped-container.gz/leaf.bin", 64)
                .unwrap(),
            b"nested leaf"
        );
    }

    #[test]
    fn recursively_mounted_serialized_file_records_preserve_each_container_entry_order() {
        let direct_first = serialized_file_header_fixture();
        let nested_leaf = serialized_file_header_fixture();
        let direct_second = serialized_file_header_fixture();
        let nested = webdata_fixture(&[("inside.asset", &nested_leaf)]);
        let input = webdata_fixture(&[
            ("z-first.asset", &direct_first),
            ("nested.bundle", &nested),
            ("a-second.asset", &direct_second),
        ]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.container_scopes, ["/tree", "/tree/nested.bundle"]);
        assert_eq!(
            report.container_bundles,
            [
                MountedContainerBundle {
                    scope_index: Some(0),
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    name_fixed: None,
                },
                MountedContainerBundle {
                    scope_index: Some(1),
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(1),
                    name_fixed: Some("nested.bundle".to_owned()),
                },
            ]
        );
        assert_eq!(
            report.serialized_files,
            [
                MountedSerializedFile {
                    virtual_path: "/tree/z-first.asset".to_owned(),
                    container_scope_index: Some(0),
                    container_bundle_index: Some(0),
                    entry_index: 0,
                    collection_index: Some(0),
                },
                MountedSerializedFile {
                    virtual_path: "/tree/nested.bundle/inside.asset".to_owned(),
                    container_scope_index: Some(1),
                    container_bundle_index: Some(1),
                    entry_index: 0,
                    collection_index: Some(0),
                },
                MountedSerializedFile {
                    virtual_path: "/tree/a-second.asset".to_owned(),
                    container_scope_index: Some(0),
                    container_bundle_index: Some(0),
                    entry_index: 2,
                    collection_index: Some(1),
                },
            ]
        );
        let root_collection_order: Vec<_> = report
            .serialized_files
            .iter()
            .filter(|file| file.container_scope_index == Some(0))
            .map(|file| file.virtual_path.as_str())
            .collect();
        assert_eq!(
            root_collection_order,
            ["/tree/z-first.asset", "/tree/a-second.asset"]
        );
    }

    #[test]
    fn recursive_container_mount_retains_fixed_source_bundle_names() {
        let nested = webdata_fixture(&[("leaf.asset", b"leaf")]);
        let input = webdata_fixture(&[("LIBRARY/Nested.Bundle", &nested)]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = file
            .mount_container_tree_with_source_name(
                &mut filesystem,
                Some("/mounted/root.bundle"),
                Some("Library/Root.Bundle"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(
            report
                .container_bundles
                .iter()
                .map(|bundle| bundle.name_fixed.as_deref())
                .collect::<Vec<_>>(),
            [Some("root.bundle"), Some("nested.bundle")]
        );
        assert_eq!(report.container_bundle_name_bytes, 24);
    }

    #[test]
    fn source_bundle_names_obey_their_aggregate_mount_bound() {
        let input = webdata_fixture(&[("leaf.asset", b"leaf")]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_tree_with_source_name(
                &mut filesystem,
                Some("/mounted"),
                Some("root.bundle"),
                ContainerMountLimits {
                    max_total_container_bundle_name_bytes: 4,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::ContainerBundleNameLimitExceeded {
                requested: 11,
                limit: 4
            }
        ));
        assert_eq!(error.partial_report.container_bundle_name_bytes, 0);
        assert_eq!(filesystem.count(), 1);
    }

    #[test]
    fn recursively_mounted_resource_records_preserve_each_container_entry_order() {
        let nested = webdata_fixture(&[("nested.resS", b"nested resource")]);
        let input = webdata_fixture(&[
            ("z-first.resource", b"first resource"),
            ("nested.bundle", &nested),
            ("a-second.ress", b"second resource"),
            ("archive:/textures/shared.ress", b"archive resource"),
        ]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(
            report.resource_files,
            [
                MountedResourceFile {
                    virtual_path: "/tree/z-first.resource".to_owned(),
                    name: "z-first.resource".to_owned(),
                    container_scope_index: Some(0),
                    container_bundle_index: 0,
                    entry_index: 0,
                    resource_index: 0,
                },
                MountedResourceFile {
                    virtual_path: "/tree/nested.bundle/nested.resS".to_owned(),
                    name: "nested.resS".to_owned(),
                    container_scope_index: Some(1),
                    container_bundle_index: 1,
                    entry_index: 0,
                    resource_index: 0,
                },
                MountedResourceFile {
                    virtual_path: "/tree/a-second.ress".to_owned(),
                    name: "a-second.ress".to_owned(),
                    container_scope_index: Some(0),
                    container_bundle_index: 0,
                    entry_index: 2,
                    resource_index: 1,
                },
                MountedResourceFile {
                    virtual_path: "/tree/archive:/textures/shared.ress".to_owned(),
                    name: "archive:/textures/shared.ress".to_owned(),
                    container_scope_index: Some(0),
                    container_bundle_index: 0,
                    entry_index: 3,
                    resource_index: 2,
                },
            ]
        );
        assert_eq!(
            report.resource_file_path_bytes,
            u64::try_from(
                "/tree/z-first.resource".len()
                    + "/tree/nested.bundle/nested.resS".len()
                    + "/tree/a-second.ress".len()
                    + "/tree/archive:/textures/shared.ress".len(),
            )
            .unwrap()
        );
        assert_eq!(
            report.resource_file_name_bytes,
            u64::try_from(
                "z-first.resource".len()
                    + "nested.resS".len()
                    + "a-second.ress".len()
                    + "archive:/textures/shared.ress".len(),
            )
            .unwrap()
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/tree/nested.bundle/nested.resS", 64)
                .unwrap(),
            b"nested resource"
        );
    }

    #[test]
    fn resource_file_order_metadata_obeys_aggregate_path_bound() {
        let input = webdata_fixture(&[("long.bin", b"resource")]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits {
                    max_total_resource_file_path_bytes: 4,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::ResourceFilePathLimitExceeded {
                requested: 14,
                limit: 4
            }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert!(error.partial_report.resource_files.is_empty());
        assert_eq!(error.partial_report.resource_file_path_bytes, 0);
    }

    #[test]
    fn original_resource_file_names_obey_their_aggregate_bound() {
        let input = webdata_fixture(&[("long.bin", b"resource")]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits {
                    max_total_resource_file_name_bytes: 4,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::ResourceFileNameLimitExceeded {
                requested: 8,
                limit: 4
            }
        ));
        assert!(error.partial_report.resource_files.is_empty());
        assert_eq!(error.partial_report.resource_file_name_bytes, 0);
    }

    #[test]
    fn serialized_file_order_metadata_obeys_aggregate_path_bound() {
        let serialized_file = serialized_file_header_fixture();
        let input = webdata_fixture(&[("leaf.asset", &serialized_file)]);
        let file = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut filesystem,
                Some("/tree"),
                ContainerMountLimits {
                    max_total_serialized_file_path_bytes: 4,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::SerializedFilePathLimitExceeded {
                requested: 16,
                limit: 4
            }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert!(error.partial_report.serialized_files.is_empty());
    }

    #[test]
    fn mount_path_imports_a_root_resource_under_its_logical_filename() {
        let path = unique_temp_file_path("bin");
        std::fs::write(&path, b"standalone resource").unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_path(
            &path,
            &mut filesystem,
            Some("/inputs"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(report.mounted_bytes, 19);
        assert_eq!(
            filesystem
                .read_all_bytes(
                    &format!("/inputs/{}", path.file_name().unwrap().to_string_lossy()),
                    64,
                )
                .unwrap(),
            b"standalone resource"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn mount_path_assembles_split_inputs_and_uses_the_logical_base_name() {
        let source_record = source_multifile_path_fields("SPLIT_PART|");
        assert_eq!(source_record.len(), 4);
        let expected_bytes = decode_hex_fixture(source_record[3]);
        let directory = unique_temp_file_path("dir");
        std::fs::create_dir(&directory).unwrap();
        let base_path = directory.join(source_record[1]);
        let first_part = PathBuf::from(format!("{}.split0", base_path.display()));
        let second_part = PathBuf::from(format!("{}.split1", base_path.display()));
        std::fs::write(&first_part, b"alpha:").unwrap();
        std::fs::write(&second_part, b"beta").unwrap();
        let supplied_part = PathBuf::from(format!("{}.split1", base_path.display()));
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_path(
            &supplied_part,
            &mut filesystem,
            Some("/inputs"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(
            source_record[1], source_record[2],
            "the reference FilePath basename and Name agree for this resource"
        );
        assert_eq!(
            filesystem
                .read_all_bytes(&format!("/inputs/{}", source_record[1]), 64,)
                .unwrap(),
            expected_bytes
        );
        std::fs::remove_file(first_part).unwrap();
        std::fs::remove_file(second_part).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn mount_path_rejects_zero_padded_parts_the_reference_cannot_open() {
        assert_eq!(
            source_multifile_path_fields("ZERO_PADDED_INDEX|")[1],
            "REJECTED"
        );
        let directory = unique_temp_file_path("dir");
        std::fs::create_dir(&directory).unwrap();
        let base_path = directory.join("padded.bundle");
        std::fs::write(format!("{}.split00", base_path.display()), b"zero").unwrap();
        std::fs::write(format!("{}.split1", base_path.display()), b"one").unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let error = SchemeReader::mount_path(
            &base_path,
            &mut filesystem,
            Some("/inputs"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("padded.bundle.split0"));
        assert!(filesystem
            .read_all_bytes("/inputs/padded.bundle", 32)
            .is_err());
        std::fs::remove_file(directory.join("padded.bundle.split00")).unwrap();
        std::fs::remove_file(directory.join("padded.bundle.split1")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn mount_path_connects_root_webdata_to_recursive_virtual_mounting() {
        let path = unique_temp_file_path("webdata");
        let input = webdata_fixture(&[("assets/leaf.bin", b"root WebData leaf")]);
        std::fs::write(&path, input).unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_path(
            &path,
            &mut filesystem,
            Some("/bundles/root"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/root/assets/leaf.bin", 64)
                .unwrap(),
            b"root WebData leaf"
        );
        std::fs::remove_file(path).unwrap();
    }

    /// Single-path import against reference-written bytes.
    ///
    /// The WebData containers below were written by the frozen
    /// `AssetRipper.IO.Files` runtime (`tools/probes/singlefile/Program.cs`;
    /// rows in `scheme/singlefile_probe_fixture.txt`), not by the Rust encoder:
    /// aligned and unaligned variants carrying `alpha.bin` (13 bytes) and
    /// `beta.bin` (5 bytes). The probe also pins the source outcomes this test
    /// replays: an 8-byte truncation falls through dispatch to a raw
    /// `ResourceFile` leaf (no error), split base/part paths resolve to the
    /// same entries, and a missing middle part is rejected.
    #[test]
    fn mount_path_matches_reference_written_webdata_and_fails_loud_on_gap() {
        fn singlefile_fields(prefix: &str) -> Vec<&'static str> {
            include_str!("scheme/singlefile_probe_fixture.txt")
                .lines()
                .find(|line| line.starts_with(prefix))
                .unwrap_or_else(|| panic!("missing single-file fixture row {prefix:?}"))
                .split('|')
                .collect()
        }

        fn assert_reference_entries(filesystem: &VirtualFileSystem, destination: &str) {
            for (entry_prefix, expected_name, expected_len, expected_hex) in [
                (
                    "ALIGNED_ENTRY|",
                    "alpha.bin",
                    13usize,
                    "616C7068612D7061796C6F6164",
                ),
                ("ALIGNED_ENTRY|", "beta.bin", 5usize, "6265746121"),
            ] {
                let _ = entry_prefix;
                let expected = decode_hex_fixture(expected_hex);
                assert_eq!(expected.len(), expected_len);
                assert_eq!(
                    filesystem
                        .read_all_bytes(&format!("{destination}/{expected_name}"), 64)
                        .unwrap(),
                    expected,
                    "reference entry {expected_name} mounts with exact bytes",
                );
            }
        }

        let aligned = decode_hex_fixture(singlefile_fields("FILE_HEX|aligned|")[2]);
        let noalign = decode_hex_fixture(singlefile_fields("FILE_HEX|noalign|")[2]);
        // Both alignment variants carry the same entries on the source side.
        for (variant, entry_prefix) in
            [("aligned", "ALIGNED_ENTRY|"), ("noalign", "NOALIGN_ENTRY|")]
        {
            for (entry_row, expected_hex) in [
                (
                    format!("{entry_prefix}alpha.bin|"),
                    "616C7068612D7061796C6F6164",
                ),
                (format!("{entry_prefix}beta.bin|"), "6265746121"),
            ] {
                let fields = singlefile_fields(&entry_row);
                assert_eq!(fields[3], expected_hex, "{variant} reference entry bytes");
            }
        }
        assert_eq!(
            singlefile_fields("TRUNCATED|")[1],
            "ACCEPTED",
            "the source falls through truncated input to a raw leaf"
        );
        assert_eq!(singlefile_fields("GAP|")[1], "REJECTED");

        // Regular-file imports of both reference-written variants.
        for bytes in [&aligned, &noalign] {
            let path = unique_temp_file_path("webdata");
            std::fs::write(&path, bytes).unwrap();
            let mut filesystem = VirtualFileSystem::new();
            let report = SchemeReader::mount_path(
                &path,
                &mut filesystem,
                Some("/inputs"),
                SchemeReaderLimits::default(),
                ContainerMountLimits::default(),
            )
            .unwrap();
            assert_eq!(report.mounted_entries, 2);
            assert_reference_entries(&filesystem, "/inputs");
            std::fs::remove_file(path).unwrap();
        }

        // Split base and split part paths resolve to the same entries.
        let directory = unique_temp_file_path("dir");
        std::fs::create_dir(&directory).unwrap();
        let base_path = directory.join("single.webdata");
        let half = aligned.len() / 2;
        std::fs::write(format!("{}.split0", base_path.display()), &aligned[..half]).unwrap();
        std::fs::write(format!("{}.split1", base_path.display()), &aligned[half..]).unwrap();
        for probe_row in ["SPLIT_BASE|", "SPLIT_PART|"] {
            assert_eq!(singlefile_fields(probe_row)[1], "OK");
        }
        for supplied in [
            base_path.clone(),
            PathBuf::from(format!("{}.split1", base_path.display())),
        ] {
            let mut filesystem = VirtualFileSystem::new();
            let report = SchemeReader::mount_path(
                &supplied,
                &mut filesystem,
                Some("/inputs"),
                SchemeReaderLimits::default(),
                ContainerMountLimits::default(),
            )
            .unwrap();
            assert_eq!(
                report.mounted_entries,
                2,
                "split import via {}",
                supplied.display()
            );
            assert_reference_entries(&filesystem, "/inputs");
        }

        // A missing middle part fails instead of mounting a partial stream,
        // and leaves no partial entries behind.
        std::fs::remove_file(format!("{}.split1", base_path.display())).unwrap();
        std::fs::write(format!("{}.split2", base_path.display()), &aligned[half..]).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let error = SchemeReader::mount_path(
            &base_path,
            &mut filesystem,
            Some("/inputs"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("single.webdata.split1"),
            "gap error names the missing part: {error}"
        );
        assert!(filesystem.read_all_bytes("/inputs/alpha.bin", 32).is_err());
        assert!(filesystem.read_all_bytes("/inputs/beta.bin", 32).is_err());
        std::fs::remove_file(format!("{}.split0", base_path.display())).unwrap();
        std::fs::remove_file(format!("{}.split2", base_path.display())).unwrap();
        std::fs::remove_dir(directory).unwrap();

        // Truncated input falls through to a raw leaf on both sides.
        let truncated = &aligned[..8];
        let path = unique_temp_file_path("webdata");
        std::fs::write(&path, truncated).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = SchemeReader::mount_path(
            &path,
            &mut filesystem,
            Some("/inputs"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();
        assert_eq!(report.mounted_entries, 1);
        assert_eq!(
            filesystem
                .read_all_bytes(
                    &format!("/inputs/{}", path.file_name().unwrap().to_string_lossy()),
                    64,
                )
                .unwrap(),
            truncated,
            "truncated bytes survive verbatim as a raw leaf, matching the source ResourceFile fallback",
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn mount_directory_preserves_paths_and_assembles_split_groups() {
        let root = unique_temp_file_path("directory");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir_all(root.join("parts")).unwrap();
        std::fs::write(root.join("a.bin"), b"alpha").unwrap();
        std::fs::write(root.join("nested/z.bin"), b"nested").unwrap();
        std::fs::write(root.join("parts/joined.split0"), b"split ").unwrap();
        std::fs::write(root.join("parts/joined.split1"), b"part").unwrap();
        let container = webdata_fixture(&[("inner/asset.dat", b"bundle leaf")]);
        std::fs::write(root.join("bundle.dat"), container).unwrap();
        let second_container = webdata_fixture(&[("inner/asset.dat", b"other bundle")]);
        std::fs::write(root.join("second.bundle"), second_container).unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_directory(
            &root,
            &mut filesystem,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.discovered_inputs, 5);
        assert_eq!(report.completed_inputs, 5);
        assert_eq!(report.mounted_entries, 5);
        assert_eq!(report.mounted_bytes, 5 + 6 + 10 + 11 + 12);
        assert_eq!(
            filesystem.read_all_bytes("/batch/a.bin", 32).unwrap(),
            b"alpha"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/batch/nested/z.bin", 32)
                .unwrap(),
            b"nested"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/batch/parts/joined", 32)
                .unwrap(),
            b"split part"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/batch/bundle.dat/inner/asset.dat", 32)
                .unwrap(),
            b"bundle leaf"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/batch/second.bundle/inner/asset.dat", 32)
                .unwrap(),
            b"other bundle"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mount_directory_preserving_containers_keeps_split_container_identity() {
        let root = unique_temp_file_path("preserve-container");
        std::fs::create_dir_all(&root).unwrap();
        let container = webdata_fixture(&[("inner/asset.dat", b"bundle leaf")]);
        let split = container.len() / 2;
        std::fs::write(root.join("data.unity3d.split0"), &container[..split]).unwrap();
        std::fs::write(root.join("data.unity3d.split1"), &container[split..]).unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_directory_preserving_containers(
            &root,
            &mut filesystem,
            Some("/platform/Game_Data"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.discovered_inputs, 1);
        assert_eq!(report.completed_inputs, 1);
        assert_eq!(report.mounted_entries, 1);
        assert_eq!(
            report.mounted_bytes,
            u64::try_from(container.len()).unwrap()
        );
        assert!(filesystem
            .file_exists(Some("/platform/Game_Data/data.unity3d"))
            .unwrap());
        assert!(!filesystem
            .directory_exists(Some("/platform/Game_Data/data.unity3d"))
            .unwrap());
        let preserved = filesystem
            .read_all_bytes("/platform/Game_Data/data.unity3d", container.len())
            .unwrap();
        assert_eq!(preserved, container);
        assert_eq!(
            SchemeReader::read(&preserved).unwrap().kind(),
            FileKind::UnityWebData
        );

        #[cfg(windows)]
        {
            assert!(std::fs::write(root.join("data.unity3d.split0"), b"replacement").is_err());
            assert!(std::fs::remove_file(root.join("data.unity3d.split1")).is_err());
        }

        drop(filesystem);
        std::fs::remove_file(root.join("data.unity3d.split0")).unwrap();
        std::fs::remove_file(root.join("data.unity3d.split1")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn mount_directory_enforces_aggregate_input_limit_before_mutation() {
        let root = unique_temp_file_path("input-limit");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("first.bin"), b"first").unwrap();
        std::fs::write(root.join("second.bin"), b"second").unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let error = SchemeReader::mount_directory(
            &root,
            &mut filesystem,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_total_input_bytes: 10,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();

        assert!(matches!(
            error.kind,
            BatchMountErrorKind::TotalInputLimitExceeded {
                requested_total: 11,
                limit: 10
            }
        ));
        assert_eq!(error.partial_report.completed_inputs, 0);
        assert_eq!(filesystem.count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mount_directory_applies_output_limit_across_inputs_and_reports_partial_progress() {
        let root = unique_temp_file_path("output-limit");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.bin"), b"1234").unwrap();
        std::fs::write(root.join("b.bin"), b"56").unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let error = SchemeReader::mount_directory(
            &root,
            &mut filesystem,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_total_output_bytes: 4,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();

        assert!(matches!(
            error.kind,
            BatchMountErrorKind::TotalOutputLimitExceeded {
                requested_total: 6,
                limit: 4
            }
        ));
        assert_eq!(error.partial_report.completed_inputs, 1);
        assert_eq!(error.partial_report.mounted_entries, 1);
        assert_eq!(error.partial_report.mounted_bytes, 4);
        assert_eq!(
            filesystem.read_all_bytes("/batch/a.bin", 8).unwrap(),
            b"1234"
        );
        assert!(!filesystem.file_exists(Some("/batch/b.bin")).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mount_directory_rejects_regular_and_split_inputs_with_same_logical_path() {
        let root = unique_temp_file_path("duplicate-input");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("same.bin"), b"regular").unwrap();
        std::fs::write(root.join("same.bin.split0"), b"split").unwrap();
        let mut filesystem = VirtualFileSystem::new();

        let error = SchemeReader::mount_directory(
            &root,
            &mut filesystem,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap_err();

        assert!(matches!(
            error.kind,
            BatchMountErrorKind::DirectoryDiscovery(_)
        ));
        assert_eq!(filesystem.count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mount_directory_enforces_input_count_and_depth_limits() {
        let root = unique_temp_file_path("directory-bounds");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("a.bin"), b"root").unwrap();
        std::fs::write(root.join("nested/b.bin"), b"nested").unwrap();

        let mut input_limited = VirtualFileSystem::new();
        let error = SchemeReader::mount_directory(
            &root,
            &mut input_limited,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_inputs: 1,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            BatchMountErrorKind::DirectoryDiscovery(_)
        ));
        assert_eq!(input_limited.count(), 1);

        let mut directory_entry_limited = VirtualFileSystem::new();
        let error = SchemeReader::mount_directory(
            &root,
            &mut directory_entry_limited,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_directory_entries: 1,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            BatchMountErrorKind::DirectoryDiscovery(_)
        ));
        assert_eq!(directory_entry_limited.count(), 1);

        let mut depth_limited = VirtualFileSystem::new();
        let error = SchemeReader::mount_directory(
            &root,
            &mut depth_limited,
            Some("/batch"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_directory_depth: 0,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            BatchMountErrorKind::DirectoryDiscovery(_)
        ));
        assert_eq!(depth_limited.count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mount_paths_imports_explicit_roots_and_honors_each_destination() {
        let resource_path = unique_temp_file_path("explicit-resource");
        let container_path = unique_temp_file_path("explicit-container");
        std::fs::write(&resource_path, b"first").unwrap();
        std::fs::write(&container_path, webdata_fixture(&[("leaf.bin", b"leaf")])).unwrap();
        let inputs = [
            PathMountInput {
                path: resource_path.clone(),
                destination_root: Some("/explicit".to_owned()),
            },
            PathMountInput {
                path: container_path.clone(),
                destination_root: Some("/explicit/container".to_owned()),
            },
        ];
        let mut filesystem = VirtualFileSystem::new();

        let report = SchemeReader::mount_paths(
            &inputs,
            &mut filesystem,
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap();

        assert_eq!(report.discovered_inputs, 2);
        assert_eq!(report.completed_inputs, 2);
        assert_eq!(report.mounted_entries, 2);
        assert_eq!(report.mounted_bytes, 9);
        let resource_virtual_path = format!(
            "/explicit/{}",
            resource_path.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(
            report.explicit_root_resource_files,
            Box::new(vec![MountedExplicitResourceFile {
                file_name: resource_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                virtual_path: resource_virtual_path.clone(),
                kind: FileKind::Resource,
            }])
        );
        assert_eq!(
            filesystem
                .read_all_bytes(&resource_virtual_path, 16)
                .unwrap(),
            b"first"
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/explicit/container/leaf.bin", 16)
                .unwrap(),
            b"leaf"
        );
        std::fs::remove_file(resource_path).unwrap();
        std::fs::remove_file(container_path).unwrap();
    }

    #[test]
    fn mount_paths_preflights_input_size_and_rejects_duplicate_split_groups() {
        let resource_path = unique_temp_file_path("explicit-limit");
        std::fs::write(&resource_path, b"resource").unwrap();
        let inputs = [PathMountInput {
            path: resource_path.clone(),
            destination_root: Some("/explicit".to_owned()),
        }];
        let mut filesystem = VirtualFileSystem::new();

        let error = SchemeReader::mount_paths(
            &inputs,
            &mut filesystem,
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits {
                max_total_input_bytes: 4,
                ..BatchMountLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            BatchMountErrorKind::TotalInputLimitExceeded {
                requested_total: 8,
                limit: 4
            }
        ));
        assert_eq!(filesystem.count(), 1);

        let split_base = unique_temp_file_path("explicit-split");
        let split0 = PathBuf::from(format!("{}.split0", split_base.display()));
        let split1 = PathBuf::from(format!("{}.split1", split_base.display()));
        std::fs::write(&split0, b"split ").unwrap();
        std::fs::write(&split1, b"group").unwrap();
        let split_inputs = [
            PathMountInput {
                path: split0.clone(),
                destination_root: Some("/explicit".to_owned()),
            },
            PathMountInput {
                path: split1.clone(),
                destination_root: Some("/explicit".to_owned()),
            },
        ];
        let error = SchemeReader::mount_paths(
            &split_inputs,
            &mut filesystem,
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            BatchMountErrorKind::DuplicateInputPath
        ));
        assert_eq!(filesystem.count(), 1);

        std::fs::remove_file(resource_path).unwrap();
        std::fs::remove_file(split0).unwrap();
        std::fs::remove_file(split1).unwrap();
    }

    #[test]
    fn recursive_mount_applies_global_count_depth_and_output_limits() {
        let nested = webdata_fixture(&[("first.bin", b"first"), ("second.bin", b"second")]);
        let input = webdata_fixture(&[("nested", &nested)]);
        let file = SchemeReader::read(&input).unwrap();

        let mut count_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut count_limited,
                Some("/tree"),
                ContainerMountLimits {
                    max_entries: 2,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::EntryCountLimitExceeded { count: 3, limit: 2 }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert_eq!(count_limited.count(), 1);

        let mut scope_path_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut scope_path_limited,
                Some("/tree"),
                ContainerMountLimits {
                    max_total_container_scope_path_bytes: 4,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::ContainerScopePathLimitExceeded {
                requested: 5,
                limit: 4
            }
        ));
        assert!(error.partial_report.container_scopes.is_empty());
        assert_eq!(scope_path_limited.count(), 1);

        let mut depth_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut depth_limited,
                Some("/tree"),
                ContainerMountLimits {
                    max_depth: 0,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::NestingDepthLimitExceeded { depth: 1, limit: 0 }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert_eq!(depth_limited.count(), 1);

        let wrapped_nested = compress_gzip(&nested, 4096).unwrap();
        let wrapped_input = webdata_fixture(&[("nested.gz", &wrapped_nested)]);
        let wrapped_file = SchemeReader::read(&wrapped_input).unwrap();
        let mut active_byte_limited = VirtualFileSystem::new();
        let error = wrapped_file
            .mount_container_tree(
                &mut active_byte_limited,
                Some("/tree"),
                ContainerMountLimits {
                    max_active_nested_bytes: 0,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::ActiveNestedContainerLimitExceeded {
                entry_index: 0,
                limit: 0,
                ..
            }
        ));
        assert_eq!(error.partial_report.mounted_entries, 0);
        assert_eq!(active_byte_limited.count(), 1);

        let mut output_limited = VirtualFileSystem::new();
        let error = file
            .mount_container_tree(
                &mut output_limited,
                Some("/tree"),
                ContainerMountLimits {
                    max_total_output_bytes: 5,
                    ..ContainerMountLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ContainerMountErrorKind::TotalOutputLimitExceeded { .. }
        ));
        assert_eq!(error.partial_report.mounted_entries, 1);
        assert_eq!(error.partial_report.mounted_bytes, 5);
        assert_eq!(
            output_limited
                .read_all_bytes("/tree/nested/first.bin", 32)
                .unwrap(),
            b"first"
        );
        assert!(!output_limited
            .file_exists(Some("/tree/nested/second.bin"))
            .unwrap());
    }

    #[test]
    fn dispatches_a_spec_encoded_unity_brotli_metadata_wrapper() {
        for window_bits in 10..=24 {
            let input = unity_marked_brotli_fixture(b"webdata payload", window_bits);
            assert!(has_brotli_file_signature(&input), "WBITS={window_bits}");
            assert_eq!(
                decompress_brotli(&input, 1024).unwrap(),
                b"webdata payload",
                "WBITS={window_bits}"
            );
        }

        let input = unity_marked_brotli_fixture(b"webdata payload", 18);
        let file = SchemeReader::read(&input).unwrap();
        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"webdata payload");
        assert_eq!(file.wrappers()[0].kind, WrapperKind::Brotli);
    }

    #[test]
    fn readability_checks_signatures_without_decoding() {
        assert!(SchemeReader::is_readable(b"\x1f\x8b"));
        assert!(!SchemeReader::is_readable(b"arbitrary resource"));
        let mut archive = vec![0; 32];
        archive[..12].copy_from_slice(b"UnityArchive");
        assert!(SchemeReader::is_readable(&archive));
    }

    #[test]
    fn enforces_recursive_and_cumulative_wrapper_limits() {
        let once = compress_gzip(b"data", 1024).unwrap();
        let twice = compress_gzip(&once, 1024).unwrap();

        let depth_error = SchemeReader::read_with_limits(
            &twice,
            SchemeReaderLimits {
                max_wrapper_depth: 1,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            depth_error.kind(),
            SchemeErrorKind::WrapperDepthExceeded { limit: 1 }
        ));

        let total_error = SchemeReader::read_with_limits(
            &once,
            SchemeReaderLimits {
                max_total_decompressed_bytes: 2,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            total_error.kind(),
            SchemeErrorKind::TotalDecompressionLimitExceeded { limit: 2, .. }
        ));
    }

    #[test]
    fn follows_the_source_bundle_and_webdata_detection_order() {
        for (magic, expected) in [
            (b"UnityFS".as_slice(), FileKind::UnityFs),
            (b"UnityRaw".as_slice(), FileKind::UnityRaw),
            (b"UnityWeb".as_slice(), FileKind::UnityWebBundle),
            (b"UnityArchive".as_slice(), FileKind::UnityArchive),
        ] {
            let mut bytes = vec![0; 32];
            bytes[..magic.len()].copy_from_slice(magic);
            assert_eq!(SchemeReader::read(&bytes).unwrap().kind(), expected);
        }

        let web_data = b"UnityWebData1.0\0\x14\0\0\0";
        assert_eq!(
            SchemeReader::read(web_data).unwrap().kind(),
            FileKind::UnityWebData
        );
    }

    #[test]
    fn container_kinds_follow_the_source_bundle_and_leaf_partition() {
        for kind in [
            FileKind::UnityFs,
            FileKind::UnityRaw,
            FileKind::UnityWebBundle,
            FileKind::UnityWebData,
            FileKind::UnityArchive,
        ] {
            assert!(
                super::is_container_kind(kind),
                "{kind:?} expands into mounted entries"
            );
        }
        for kind in [FileKind::SerializedFile, FileKind::Resource] {
            assert!(
                !super::is_container_kind(kind),
                "{kind:?} mounts as a single leaf"
            );
        }
    }
