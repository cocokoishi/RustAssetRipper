    use super::{
        compression_type_name, UnityFsBundle, UnityFsErrorKind, UnityFsLimits, UnityFsNode,
        UnityFsStreamBundle, BUNDLE_VERSION_LARGE_FILES_SUPPORT, NODE_SERIALIZED_FILE_FLAG,
    };
    use crate::compression::wrappers::compress_gzip;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::io::stream::{SmartStream, SmartStreamType};
    use crate::scheme::{
        BatchMountLimits, ContainerMountErrorKind, ContainerMountLimits, FileKind, ParsedFile,
        SchemeReader, SchemeReaderLimits, WrapperKind,
    };

    fn push_i32(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn push_i64(output: &mut Vec<u8>, value: i64) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn push_u32(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn push_cstring(output: &mut Vec<u8>, value: &str) {
        output.extend_from_slice(value.as_bytes());
        output.push(0);
    }

    fn lz4_literal_block(bytes: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        result.push((bytes.len().min(15) as u8) << 4);
        if bytes.len() >= 15 {
            let mut remainder = bytes.len() - 15;
            while remainder >= 255 {
                result.push(255);
                remainder -= 255;
            }
            result.push(remainder as u8);
        }
        result.extend_from_slice(bytes);
        result
    }

    fn lzma_raw_block(bytes: &[u8]) -> Vec<u8> {
        let mut framed = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::Cursor::new(bytes), &mut framed).unwrap();
        assert!(framed.len() >= 13);
        let mut raw = framed[..5].to_vec();
        raw.extend_from_slice(&framed[13..]);
        raw
    }

    fn zstd_raw_block(bytes: &[u8]) -> Vec<u8> {
        assert!(bytes.len() <= 255);
        let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0x20, bytes.len() as u8];
        let block_header = ((bytes.len() as u32) << 3) | 1;
        frame.extend_from_slice(&block_header.to_le_bytes()[..3]);
        frame.extend_from_slice(bytes);
        frame
    }

    fn zstd_rle_block(bytes: &[u8]) -> Vec<u8> {
        assert!(!bytes.is_empty() && bytes.len() <= 255);
        assert!(bytes.iter().all(|byte| *byte == bytes[0]));
        let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0x20, bytes.len() as u8];
        let block_header = ((bytes.len() as u32) << 3) | 3;
        frame.extend_from_slice(&block_header.to_le_bytes()[..3]);
        frame.push(bytes[0]);
        frame
    }

    fn make_bundle(
        entry_bytes: &[u8],
        metadata_compression: u8,
        split_data_block: bool,
        flags_extra: u32,
    ) -> Vec<u8> {
        make_bundle_variant(
            entry_bytes,
            metadata_compression,
            split_data_block,
            6,
            flags_extra,
            false,
        )
    }

    fn make_bundle_variant(
        entry_bytes: &[u8],
        metadata_compression: u8,
        split_data_block: bool,
        version: i32,
        flags_extra: u32,
        metadata_at_end: bool,
    ) -> Vec<u8> {
        make_bundle_with_block_compression(
            entry_bytes,
            metadata_compression,
            0,
            split_data_block,
            version,
            flags_extra,
            metadata_at_end,
        )
    }

    fn make_bundle_with_block_compression(
        entry_bytes: &[u8],
        metadata_compression: u8,
        block_compression: u8,
        split_data_block: bool,
        version: i32,
        flags_extra: u32,
        metadata_at_end: bool,
    ) -> Vec<u8> {
        let (first_data, second_data) = if split_data_block {
            (&entry_bytes[..2], &entry_bytes[2..])
        } else {
            (entry_bytes, &[][..])
        };
        let first_compressed = match block_compression {
            0 => first_data.to_vec(),
            1 => lzma_raw_block(first_data),
            2 | 3 => lz4_literal_block(first_data),
            4 | 5 => zstd_raw_block(first_data),
            6 => zstd_rle_block(first_data),
            other => panic!("test helper does not encode compression type {other}"),
        };
        let second_compressed = if split_data_block {
            lz4_literal_block(second_data)
        } else {
            Vec::new()
        };

        let mut metadata = vec![0x5a; 16];
        push_i32(&mut metadata, if split_data_block { 2 } else { 1 });
        metadata.extend_from_slice(&(first_data.len() as u32).to_be_bytes());
        metadata.extend_from_slice(&(first_compressed.len() as u32).to_be_bytes());
        metadata.extend_from_slice(&(block_compression as u16).to_be_bytes());
        if split_data_block {
            metadata.extend_from_slice(&(second_data.len() as u32).to_be_bytes());
            metadata.extend_from_slice(&(second_compressed.len() as u32).to_be_bytes());
            metadata.extend_from_slice(&2u16.to_be_bytes());
        }
        push_i32(&mut metadata, 1);
        push_i64(&mut metadata, 0);
        push_i64(&mut metadata, entry_bytes.len() as i64);
        push_i32(&mut metadata, 4);
        push_cstring(&mut metadata, "data/file.bin");

        let encoded_metadata = match metadata_compression {
            0 => metadata.clone(),
            1 => lzma_raw_block(&metadata),
            2 | 3 => lz4_literal_block(&metadata),
            4 | 5 => zstd_raw_block(&metadata),
            other => panic!("test helper does not encode compression type {other}"),
        };
        let mut header = Vec::new();
        push_cstring(&mut header, "UnityFS");
        push_i32(&mut header, version);
        push_cstring(&mut header, "2022.3.1f1");
        push_cstring(&mut header, "2022.3.1f1");
        let data_start = header.len() + 8 + 4 + 4 + 4;
        let mut body = Vec::new();
        if version >= BUNDLE_VERSION_LARGE_FILES_SUPPORT {
            let padding = (16 - (data_start & 15)) & 15;
            body.resize(padding, 0);
        }
        if metadata_at_end {
            body.extend_from_slice(&first_compressed);
            body.extend_from_slice(&second_compressed);
            body.extend_from_slice(&encoded_metadata);
        } else {
            body.extend_from_slice(&encoded_metadata);
            let after_metadata = data_start + body.len();
            if flags_extra & 0x200 != 0 {
                let padding = (16 - (after_metadata & 15)) & 15;
                body.resize(body.len() + padding, 0);
            }
            body.extend_from_slice(&first_compressed);
            body.extend_from_slice(&second_compressed);
        }
        let total_size = data_start + body.len();
        push_i64(&mut header, total_size as i64);
        push_i32(&mut header, encoded_metadata.len() as i32);
        push_i32(&mut header, metadata.len() as i32);
        let at_end_flag = if metadata_at_end { 0x80 } else { 0 };
        push_u32(
            &mut header,
            (metadata_compression as u32) | 0x40 | flags_extra | at_end_flag,
        );
        header.extend_from_slice(&body);
        header
    }

    /// A metadata-at-start bundle whose flags omit the combined-directory bit: the
    /// baseline `ReadMetadata` then reads blocks only, and the directory stays empty.
    fn make_bare_bundle(entry_bytes: &[u8]) -> Vec<u8> {
        let mut metadata = vec![0x5a; 16];
        push_i32(&mut metadata, 1);
        metadata.extend_from_slice(&(entry_bytes.len() as u32).to_be_bytes());
        metadata.extend_from_slice(&(entry_bytes.len() as u32).to_be_bytes());
        metadata.extend_from_slice(&0u16.to_be_bytes());
        let mut header = Vec::new();
        push_cstring(&mut header, "UnityFS");
        push_i32(&mut header, 6);
        push_cstring(&mut header, "2022.3.1f1");
        push_cstring(&mut header, "2022.3.1f1");
        let data_start = header.len() + 8 + 4 + 4 + 4;
        push_i64(
            &mut header,
            (data_start + metadata.len() + entry_bytes.len()) as i64,
        );
        push_i32(&mut header, metadata.len() as i32);
        push_i32(&mut header, metadata.len() as i32);
        push_u32(&mut header, 0);
        header.extend_from_slice(&metadata);
        header.extend_from_slice(entry_bytes);
        header
    }

    /// The metadata-placement branches the other fixtures never take: a directory-less
    /// bundle parses to an empty node table, and a pre-alignment version with
    /// blocks-info-at-end skips the 16-byte data alignment.
    #[test]
    fn parses_bare_metadata_and_unaligned_end_placement() {
        let input = make_bare_bundle(b"bare entry");
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.uncompressed_data_hash, [0x5a; 16]);
        assert_eq!(bundle.storage_blocks.len(), 1);
        assert!(bundle.nodes.is_empty());
        assert_eq!(bundle.uncompressed_data_size, 10);
        let error = bundle.read_entry(0).unwrap_err();
        assert!(matches!(
            error.kind(),
            UnityFsErrorKind::InvalidEntryIndex {
                entry_index: 0,
                entry_count: 0
            }
        ));

        let input = make_bundle_variant(b"entry data", 0, false, 6, 0, true);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.header.version, 6);
        assert_eq!(bundle.data_offset, bundle.header.header_size);
        assert_eq!(
            bundle.metadata_offset + bundle.header.compressed_blocks_info_size as usize,
            input.len()
        );
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), b"entry data");
    }

    #[test]
    fn parses_uncompressed_unityfs_metadata_and_borrows_single_block_entry() {
        let input = make_bundle(b"sample asset bytes", 0, false, 0);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.header.version, 6);
        assert_eq!(bundle.header.unity_version, "2022.3.1f1");
        assert_eq!(bundle.storage_blocks.len(), 1);
        assert_eq!(bundle.nodes[0].path, "data/file.bin");
        assert_eq!(
            bundle.read_entry(0).unwrap().as_ref(),
            b"sample asset bytes"
        );
        assert!(matches!(
            bundle.read_entry(0).unwrap(),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn default_entry_bound_covers_large_files_and_explicit_lower_bounds_fail_closed() {
        let default_limits = UnityFsLimits::default();
        assert_eq!(default_limits.max_entry_bytes, 1024 * 1024 * 1024);
        assert_eq!(default_limits.max_block_uncompressed_bytes, 512 * 1024 * 1024);

        let input = make_bundle(b"entry data", 0, false, 0);
        let limited_bundle = UnityFsBundle::parse_with_limits(
            &input,
            UnityFsLimits {
                max_entry_bytes: 4,
                ..default_limits
            },
        )
        .unwrap();
        let error = limited_bundle.read_entry(0).unwrap_err();
        assert!(matches!(
            error.kind(),
            UnityFsErrorKind::LimitExceeded {
                resource: "bundle entry",
                requested: 10,
                limit: 4,
            }
        ));
    }

    #[test]
    fn streamed_unityfs_reader_keeps_single_uncompressed_entry_file_backed() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);
        let input = make_bundle(b"source-backed entry bytes", 0, false, 0);
        let path = crate::test_support::test_temp_root().join(format!(
            "unityfs-stream-entry-{}-{}.bundle",
            std::process::id(),
            NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, &input).unwrap();

        let source = SmartStream::open_read(&path).unwrap();
        let bundle =
            UnityFsStreamBundle::parse(&source, UnityFsLimits::default(), 1024 * 1024).unwrap();
        assert_eq!(bundle.nodes[0].path, "data/file.bin");
        let entry = bundle.read_entry(0).unwrap();
        assert_eq!(entry.stream_type(), SmartStreamType::File);
        assert_eq!(entry.to_vec(1024).unwrap(), b"source-backed entry bytes");

        drop(entry);
        drop(bundle);
        drop(source);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn production_mount_paths_retain_file_backing_for_uncompressed_unityfs_leaves() {
        let row = include_str!("unityfs_stream_entry_source_fixture.txt")
            .lines()
            .find(|line| line.starts_with("UNITYFS_ENTRY_STREAM|case=uncompressed|"))
            .expect("source UnityFS stream row");
        let field = |name: &str| {
            row.split('|')
                .find_map(|part| part.strip_prefix(name))
                .unwrap_or_else(|| panic!("missing source row field {name}"))
        };
        let decode_hex = |hex: &str| {
            assert_eq!(hex.len() % 2, 0, "odd source hex length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };
        let input = decode_hex(field("input="));
        let payload = decode_hex(field("entry="));
        assert_eq!(input.len().to_string(), field("input_length="));
        assert_eq!(payload.len().to_string(), field("entry_length="));
        assert_eq!(field("resource_name="), "data/file.bin");
        assert_eq!(field("backing_stream="), "RandomAccessStream");
        assert_eq!(field("stream_type="), "THROW:InvalidOperationException");
        assert_eq!(field("resources="), "1");
        assert_eq!(field("failed="), "0");

        let directory = crate::test_support::unique_test_directory("unityfs-stream-mount");
        let path = directory.join("input.bundle");
        std::fs::write(&path, &input).unwrap();

        let mut path_filesystem = VirtualFileSystem::new();
        let path_report = SchemeReader::mount_path(
            &path,
            &mut path_filesystem,
            Some("/path-mount"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();
        assert_eq!(path_report.mounted_entries, 1);
        assert_eq!(path_report.mounted_bytes, payload.len() as u64);
        assert_eq!(path_report.resource_files.len(), 1);
        let path_leaf = "/path-mount/data/file.bin";
        let path_stream = path_filesystem.open_file(path_leaf).unwrap();
        assert_eq!(path_stream.stream_type(), SmartStreamType::File);
        assert_eq!(path_stream.to_vec(1024).unwrap(), payload.as_slice());

        let mut directory_filesystem = VirtualFileSystem::new();
        let directory_report = SchemeReader::mount_directory(
            &directory,
            &mut directory_filesystem,
            Some("/directory-mount"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap();
        assert_eq!(directory_report.discovered_inputs, 1);
        assert_eq!(directory_report.completed_inputs, 1);
        assert_eq!(directory_report.mounted_entries, 1);
        assert_eq!(directory_report.mounted_bytes, payload.len() as u64);
        assert_eq!(directory_report.container_trees.len(), 1);
        assert_eq!(directory_report.container_trees[0].resource_files.len(), 1);
        let directory_leaf = "/directory-mount/input.bundle/data/file.bin";
        let directory_stream = directory_filesystem.open_file(directory_leaf).unwrap();
        assert_eq!(directory_stream.stream_type(), SmartStreamType::File);
        assert_eq!(directory_stream.to_vec(1024).unwrap(), payload.as_slice());

        let empty_row = include_str!("unityfs_stream_entry_source_fixture.txt")
            .lines()
            .find(|line| line.starts_with("UNITYFS_ENTRY_STREAM|case=empty|"))
            .expect("source UnityFS empty-entry row");
        let empty_field = |name: &str| {
            empty_row
                .split('|')
                .find_map(|part| part.strip_prefix(name))
                .unwrap_or_else(|| panic!("missing empty-entry source row field {name}"))
        };
        let empty_input = decode_hex(empty_field("input="));
        assert_eq!(empty_field("entry_length="), "0");
        assert_eq!(empty_field("backing_stream="), "RandomAccessStream");
        assert_eq!(
            empty_field("stream_type="),
            "THROW:InvalidOperationException"
        );
        let empty_path = directory.join("empty.bundle");
        std::fs::write(&empty_path, empty_input).unwrap();
        let mut empty_filesystem = VirtualFileSystem::new();
        let empty_report = SchemeReader::mount_path(
            &empty_path,
            &mut empty_filesystem,
            Some("/empty-mount"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();
        assert_eq!(empty_report.mounted_entries, 1);
        assert_eq!(empty_report.mounted_bytes, 0);
        let empty_stream = empty_filesystem
            .open_file("/empty-mount/data/file.bin")
            .unwrap();
        assert_eq!(empty_stream.stream_type(), SmartStreamType::File);
        assert!(empty_stream.to_vec(0).unwrap().is_empty());

        drop(path_stream);
        drop(path_filesystem);
        drop(directory_stream);
        drop(directory_filesystem);
        drop(empty_stream);
        drop(empty_filesystem);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(empty_path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn production_stream_mount_decodes_source_lz4_and_multiblock_entries() {
        let fixture = include_str!("unityfs_stream_entry_source_fixture.txt");
        let decode_hex = |hex: &str| {
            assert_eq!(hex.len() % 2, 0, "odd source hex length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };

        for case_name in ["lz4", "multiblock"] {
            let row = fixture
                .lines()
                .find(|line| line.starts_with(&format!("UNITYFS_ENTRY_STREAM|case={case_name}|")))
                .unwrap_or_else(|| panic!("missing source UnityFS {case_name} row"));
            let field = |name: &str| {
                row.split('|')
                    .find_map(|part| part.strip_prefix(name))
                    .unwrap_or_else(|| panic!("missing {case_name} source row field {name}"))
            };
            let input = decode_hex(field("input="));
            let expected = decode_hex(field("entry="));
            assert_eq!(input.len().to_string(), field("input_length="));
            assert_eq!(expected.len().to_string(), field("entry_length="));
            assert_eq!(field("resource_name="), "data/file.bin");
            assert_eq!(field("backing_stream="), "MemoryStream");
            assert_eq!(field("stream_type="), "Memory");
            assert_eq!(field("resources="), "1");
            assert_eq!(field("failed="), "0");

            let path = crate::test_support::unique_test_path(
                &format!("unityfs-stream-{case_name}"),
                "bundle",
            );
            std::fs::write(&path, &input).unwrap();
            let mut filesystem = VirtualFileSystem::new();
            let destination = format!("/source-{case_name}");
            let report = SchemeReader::mount_path(
                &path,
                &mut filesystem,
                Some(&destination),
                SchemeReaderLimits::default(),
                ContainerMountLimits::default(),
            )
            .unwrap();
            assert_eq!(report.mounted_entries, 1, "{case_name} mounted entries");
            assert_eq!(
                report.mounted_bytes,
                expected.len() as u64,
                "{case_name} mounted bytes"
            );
            let leaf_path = format!("{destination}/data/file.bin");
            let leaf = filesystem.open_file(&leaf_path).unwrap();
            assert_eq!(
                leaf.stream_type(),
                SmartStreamType::Memory,
                "{case_name} stream"
            );
            assert_eq!(leaf.to_vec(1024).unwrap(), expected, "{case_name} payload");

            drop(leaf);
            drop(filesystem);
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn streamed_mount_applies_nested_owned_byte_limit_to_materialized_children() {
        let payload = b"nested payload";
        let inner = make_bundle(payload, 0, false, 0);
        let direct_outer = make_bundle(&inner, 0, false, 0);
        let compressed_outer = make_bundle_with_block_compression(&inner, 0, 2, false, 6, 0, false);
        let directory = crate::test_support::unique_test_directory("unityfs-stream-nested-limit");
        let direct_path = directory.join("direct.bundle");
        let compressed_path = directory.join("compressed.bundle");
        std::fs::write(&direct_path, &direct_outer).unwrap();
        std::fs::write(&compressed_path, &compressed_outer).unwrap();
        let limits = ContainerMountLimits {
            max_active_nested_bytes: 0,
            ..ContainerMountLimits::default()
        };

        let mut direct_filesystem = VirtualFileSystem::new();
        let direct_report = SchemeReader::mount_path(
            &direct_path,
            &mut direct_filesystem,
            Some("/direct"),
            SchemeReaderLimits::default(),
            limits,
        )
        .unwrap();
        assert_eq!(direct_report.mounted_entries, 1);
        let direct_leaf = direct_filesystem
            .read_all_bytes("/direct/data/file.bin/data/file.bin", 1024)
            .unwrap();
        assert_eq!(direct_leaf, payload);
        drop(direct_filesystem);

        let mut compressed_filesystem = VirtualFileSystem::new();
        let error = SchemeReader::mount_path(
            &compressed_path,
            &mut compressed_filesystem,
            Some("/compressed"),
            SchemeReaderLimits::default(),
            limits,
        )
        .unwrap_err();
        assert!(matches!(
            &error.kind,
            ContainerMountErrorKind::ActiveNestedContainerLimitExceeded {
                requested_total,
                limit: 0,
                ..
            } if *requested_total == inner.len() as u64
        ));
        drop(compressed_filesystem);

        std::fs::remove_file(direct_path).unwrap();
        std::fs::remove_file(compressed_path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn streamed_mount_preserves_source_nested_container_topology() {
        let input =
            include_bytes!("../export/managed_reference_project_fixture/nested_reference.bundle");
        let directory = crate::test_support::unique_test_directory("unityfs-stream-topology");
        let path = directory.join("workspace.bundle");
        std::fs::write(&path, input).unwrap();

        let dispatched = SchemeReader::read(input).unwrap();
        let mut slice_filesystem = VirtualFileSystem::new();
        let slice_report = dispatched
            .mount_container_tree_with_source_name(
                &mut slice_filesystem,
                Some("/source"),
                Some("workspace.bundle"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        let mut stream_filesystem = VirtualFileSystem::new();
        let stream_report = SchemeReader::mount_path(
            &path,
            &mut stream_filesystem,
            Some("/source"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();
        assert_eq!(
            stream_report.container_scopes,
            slice_report.container_scopes
        );
        assert_eq!(
            stream_report.container_bundles,
            slice_report.container_bundles
        );
        assert_eq!(
            stream_report.serialized_files,
            slice_report.serialized_files
        );
        assert_eq!(stream_report.resource_files, slice_report.resource_files);
        assert_eq!(stream_report.failed_entries, slice_report.failed_entries);
        assert_eq!(stream_report.mounted_entries, slice_report.mounted_entries);
        assert_eq!(stream_report.mounted_bytes, slice_report.mounted_bytes);

        drop(stream_filesystem);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn streamed_unityfs_reader_decodes_compressed_blocks_across_boundaries() {
        for compression in [1, 2, 5] {
            let input = make_bundle_with_block_compression(
                b"abcdefghijklmnop",
                2,
                compression,
                true,
                6,
                0,
                false,
            );
            let source = SmartStream::from_memory(input);
            let bundle =
                UnityFsStreamBundle::parse(&source, UnityFsLimits::default(), 1024 * 1024).unwrap();
            let entry = bundle.read_entry(0).unwrap();
            assert_eq!(entry.to_vec(1024).unwrap(), b"abcdefghijklmnop");
        }
    }

    /// Uncompressed metadata with tolerated 4-byte-alignment slack (the source
    /// `#1470` case): the slack pad is accepted but is not entry data, so block
    /// data still starts after the declared size and the entry reads intact.
    /// Continuing after the consumed bytes instead (as the source fall-through
    /// does) would shift the entry by the slack.
    #[test]
    fn uncompressed_metadata_slack_starts_data_after_declared_size() {
        let entry = b"slack entry payload";
        let mut slack = make_bundle_variant(entry, 0, false, 7, 0, false);
        // The v7 fixture header is 54 bytes plus 10 alignment bytes, so metadata
        // starts at 64; its struct is 68 bytes ending with the `data/file.bin`
        // NUL. Growing the path by one byte makes the struct 69 bytes; three pad
        // bytes bring the region to the declared 72, which the alignment
        // tolerance accepts: (69 + 3) & !3 == 72.
        const METADATA_OFFSET: usize = 64;
        slack.insert(METADATA_OFFSET + 67, b'1');
        slack.splice(METADATA_OFFSET + 69..METADATA_OFFSET + 69, [0, 0, 0]);
        let total_size = i64::from_be_bytes(slack[34..42].try_into().unwrap()) + 4;
        slack[34..42].copy_from_slice(&total_size.to_be_bytes());
        let compressed = i32::from_be_bytes(slack[42..46].try_into().unwrap()) + 4;
        slack[42..46].copy_from_slice(&compressed.to_be_bytes());
        let uncompressed = i32::from_be_bytes(slack[46..50].try_into().unwrap()) + 4;
        slack[46..50].copy_from_slice(&uncompressed.to_be_bytes());

        let bundle = UnityFsBundle::parse(&slack).unwrap();
        assert_eq!(bundle.metadata_offset, METADATA_OFFSET);
        assert_eq!(bundle.data_offset, METADATA_OFFSET + 72);
        assert_eq!(bundle.nodes[0].path, "data/file.bin1");
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), entry);
    }

    /// The source accepts the four-byte metadata slack but resumes at the number of
    /// bytes consumed by its metadata structs, before the padding. This replay uses a
    /// separately built source-probe input and verifies the production dispatcher
    /// honors the declared metadata region and returns the intact resource bytes.
    #[test]
    fn production_dispatch_skips_reference_metadata_slack_before_entry_data() {
        let fixture = include_str!("unityfs_alignment_slack_fixture.txt");
        let field = |prefix: &str| {
            fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("missing fixture field {prefix}"))
        };
        let decode_hex = |hex: &str| {
            assert_eq!(hex.len() % 2, 0);
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };

        let input = decode_hex(field("INPUT_HEX|"));
        let source_tree = field("SOURCE_TREE|");
        let source_bytes = decode_hex(field("SOURCE_ENTRY_HEX|"));
        let expected_bytes = decode_hex(field("RUST_EXPECTED_ENTRY_HEX|"));
        assert!(source_tree.ends_with(&format!(
            "RES:data/file.bin1:{}",
            field("SOURCE_ENTRY_HEX|")
        )));
        assert_eq!(source_bytes, b"\0\0\0slack entry payl");
        assert_eq!(expected_bytes, b"slack entry payload");

        let parsed = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(parsed.header.version, 7);
        assert_eq!(parsed.metadata_offset, 64);
        assert_eq!(parsed.header.uncompressed_blocks_info_size, 72);
        assert_eq!(parsed.data_offset, 136);
        assert_eq!(parsed.read_entry(0).unwrap().as_ref(), expected_bytes);

        let dispatched = SchemeReader::read(&input).unwrap();
        assert_eq!(dispatched.kind(), FileKind::UnityFs);
        let entry = dispatched.dispatch_entry(0).unwrap();
        assert_eq!(entry.kind(), FileKind::Resource);
        assert_eq!(entry.bytes(), expected_bytes);
        assert_ne!(entry.bytes(), source_bytes);
    }

    #[test]
    fn mounts_unityfs_entries_using_the_parsed_node_paths() {
        let payload = b"UnityFS direct mount payload";
        let input = make_bundle(payload, 0, false, 0);
        let dispatched = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = dispatched
            .mount_container_entries(
                &mut filesystem,
                Some("/bundles/unityfs"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(report.mounted_bytes, payload.len() as u64);
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/unityfs/data/file.bin", 128)
                .unwrap(),
            payload
        );
    }

    #[test]
    fn recursively_mounts_nested_unityfs_entries() {
        let inner = make_bundle(b"nested UnityFS leaf", 0, false, 0);
        let outer = make_bundle(&inner, 0, false, 0);
        let dispatched = SchemeReader::read(&outer).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = dispatched
            .mount_container_tree(
                &mut filesystem,
                Some("/bundles/tree"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(report.mounted_bytes, b"nested UnityFS leaf".len() as u64);
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/tree/data/file.bin/data/file.bin", 128,)
                .unwrap(),
            b"nested UnityFS leaf"
        );
    }

    #[test]
    fn recursively_dispatches_unityfs_entries() {
        let nested = compress_gzip(b"nested UnityFS entry", 1024).unwrap();
        let input = make_bundle(&nested, 0, false, 0);
        let bundle = SchemeReader::read(&input).unwrap();
        assert_eq!(bundle.kind(), FileKind::UnityFs);

        let entry = bundle.dispatch_entry(0).unwrap();
        assert_eq!(entry.kind(), FileKind::Resource);
        assert_eq!(entry.bytes(), b"nested UnityFS entry");
        assert_eq!(entry.wrappers()[0].kind, WrapperKind::Gzip);
        assert!(matches!(entry.parse().unwrap(), ParsedFile::Resource(_)));
    }

    #[test]
    fn decodes_lz4_metadata_and_reassembles_entries_across_storage_blocks() {
        let input = make_bundle(b"abcdefghijklmnop", 2, true, 0);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.storage_blocks.len(), 2);
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), b"abcdefghijklmnop");
    }

    #[test]
    fn decodes_lzma_metadata_and_storage_blocks() {
        let input =
            make_bundle_with_block_compression(b"lzma entry payload", 1, 1, false, 6, 0, false);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(
            bundle.read_entry(0).unwrap().as_ref(),
            b"lzma entry payload"
        );
    }

    #[test]
    fn decodes_zstd_storage_blocks_by_frame_signature() {
        let entry_bytes = vec![b'Z'; 64];
        let input = make_bundle_with_block_compression(&entry_bytes, 0, 6, false, 6, 0, false);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), entry_bytes);
    }

    #[test]
    fn zstd_signature_does_not_override_metadata_or_lzham_dispatch() {
        let metadata = make_bundle(b"entry", 5, false, 0);
        let metadata_error = UnityFsBundle::parse(&metadata).unwrap_err();
        assert!(matches!(
            metadata_error.kind(),
            UnityFsErrorKind::UnsupportedCompression {
                compression: 5,
                context: "blocks metadata"
            }
        ));

        let lzham = make_bundle_with_block_compression(b"entry", 0, 4, false, 6, 0, false);
        let bundle = UnityFsBundle::parse(&lzham).unwrap();
        let lzham_error = bundle.read_entry(0).unwrap_err();
        assert!(matches!(
            lzham_error.kind(),
            UnityFsErrorKind::UnsupportedCompression {
                compression: 4,
                context: "storage block"
            }
        ));
    }

    #[test]
    fn handles_generation_seven_alignment_and_blocks_info_at_end() {
        let input = make_bundle_variant(b"entry data", 2, false, 7, 0, true);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.header.version, 7);
        assert_eq!(bundle.data_offset % 16, 0);
        assert_eq!(
            bundle.metadata_offset + bundle.header.compressed_blocks_info_size as usize,
            input.len()
        );
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), b"entry data");
    }

    #[test]
    fn honors_padding_flag_before_block_data() {
        let input = make_bundle_variant(b"entry data", 0, false, 6, 0x200, false);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.data_offset % 16, 0);
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), b"entry data");
    }

    #[test]
    fn validates_directory_ranges_and_configured_metadata_bounds() {
        let input = make_bundle(b"short", 0, false, 0);
        let mut malformed = input.clone();
        let parsed = UnityFsBundle::parse(&input).unwrap();
        let entry_offset_field = parsed.metadata_offset
            + parsed.header.compressed_blocks_info_size as usize
            - 8
            - 4
            - "data/file.bin".len()
            - 1;
        malformed[entry_offset_field..entry_offset_field + 8]
            .copy_from_slice(&100i64.to_be_bytes());
        let error = UnityFsBundle::parse(&malformed).unwrap_err();
        assert!(matches!(
            error.kind(),
            UnityFsErrorKind::NodeOutsideUncompressedData { .. }
        ));

        let limits = UnityFsLimits {
            max_metadata_uncompressed_bytes: 1,
            ..UnityFsLimits::default()
        };
        let error = UnityFsBundle::parse_with_limits(&input, limits).unwrap_err();
        assert!(matches!(
            error.kind(),
            UnityFsErrorKind::LimitExceeded {
                resource: "uncompressed blocks metadata",
                ..
            }
        ));
    }

    #[test]
    fn bundle_flag_and_compression_names_match_the_source_enums() {
        assert_eq!(NODE_SERIALIZED_FILE_FLAG, 4);
        // The named compression discriminants resolve; anything else reads unknown,
        // mirroring the source `default` branch.
        assert_eq!(compression_type_name(0), Some("None"));
        assert_eq!(compression_type_name(1), Some("Lzma"));
        assert_eq!(compression_type_name(2), Some("Lz4"));
        assert_eq!(compression_type_name(3), Some("Lz4HC"));
        assert_eq!(compression_type_name(4), Some("Lzham"));
        assert_eq!(compression_type_name(5), None);
        assert_eq!(compression_type_name(255), None);
    }

    #[test]
    fn directory_node_serialized_file_bit_follows_the_flag_word() {
        let node = UnityFsNode {
            offset: 0,
            size: 8,
            flags: NODE_SERIALIZED_FILE_FLAG,
            path: "level0".to_owned(),
        };
        assert!(node.is_serialized_file());
        // Other flag bits alone do not mark a SerializedFile.
        let other = UnityFsNode {
            flags: 0x40,
            ..node.clone()
        };
        assert!(!other.is_serialized_file());
        let combined = UnityFsNode {
            flags: 0x40 | NODE_SERIALIZED_FILE_FLAG,
            ..node.clone()
        };
        assert!(combined.is_serialized_file());
        let bare = UnityFsNode { flags: 0, ..node };
        assert!(!bare.is_serialized_file());
    }

    #[test]
    fn entry_block_windows_cover_exactly_the_overlap() {
        // Disjoint and merely adjacent ranges contribute nothing.
        assert_eq!(super::entry_block_window(10, 20, 30, 40), None);
        assert_eq!(super::entry_block_window(10, 20, 0, 10), None);
        assert_eq!(super::entry_block_window(10, 20, 20, 30), None);
        // Partial overlaps clip to the shared window.
        assert_eq!(super::entry_block_window(10, 20, 0, 15), Some((10, 15)));
        assert_eq!(super::entry_block_window(10, 20, 15, 30), Some((15, 20)));
        // Containment runs either way.
        assert_eq!(super::entry_block_window(10, 20, 0, 30), Some((10, 20)));
        assert_eq!(super::entry_block_window(10, 20, 12, 18), Some((12, 18)));
        // An empty entry has no window anywhere.
        assert_eq!(super::entry_block_window(10, 10, 0, 30), None);
    }

    #[test]
    fn metadata_size_rule_accepts_exact_and_aligned_slack() {
        // An exact byte count always passes, whatever the alignment.
        assert!(super::metadata_size_accepts(20, 20, 1));
        assert!(super::metadata_size_accepts(20, 20, 4));
        // Up to three alignment bytes pass when both sides are 4-aligned.
        assert!(super::metadata_size_accepts(17, 20, 4));
        assert!(super::metadata_size_accepts(18, 20, 0));
        assert!(super::metadata_size_accepts(19, 20, 8));
        // Anything else mismatches: unaligned base, unaligned declared size,
        // four slack bytes, or an overrun.
        assert!(!super::metadata_size_accepts(17, 20, 1));
        assert!(!super::metadata_size_accepts(17, 21, 4));
        assert!(!super::metadata_size_accepts(16, 20, 4));
        assert!(!super::metadata_size_accepts(21, 20, 4));
    }

    #[test]
    fn bundle_headers_match_reference_generated_bundles() {
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

        let fixture = include_str!("bundle_header_fixture.txt");
        let mut versions: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("BUNDLEVERSION|") {
                let parts: Vec<&str> = line.split('|').collect();
                assert_eq!(parts.len(), 3, "bad BUNDLEVERSION {line}");
                versions.insert(parts[1].to_string(), parts[2].parse().unwrap());
            } else if line.starts_with("BUNDLE_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad BUNDLE_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("BUNDLE_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad BUNDLE_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        assert_eq!(
            versions.get("BF_LargeFilesSupport"),
            Some(&BUNDLE_VERSION_LARGE_FILES_SUPPORT)
        );
        assert_eq!(versions.get("BF_520_x"), Some(&6));
        assert_eq!(versions.get("BF_2022_2"), Some(&8));

        for ver in ["5", "6", "7", "8"] {
            let bytes = hexes
                .get(ver)
                .unwrap_or_else(|| panic!("missing BUNDLE_HEX {ver}"));
            let dump = reads
                .get(ver)
                .unwrap_or_else(|| panic!("missing BUNDLE_READ {ver}"));
            // Dump form: ver|V=..|Size=..|Comp=..|Uncomp=..|Flags=..|Eng=..|Rev=..|Blocks=..|...
            let head = dump.strip_prefix(&format!("{ver}|")).unwrap_or(dump);
            let parts: Vec<&str> = head.split('|').collect();
            let want_version: i32 = field(&parts, "V").parse().unwrap();
            let want_size: u64 = field(&parts, "Size").parse().unwrap();
            let want_comp: u32 = field(&parts, "Comp").parse().unwrap();
            let want_uncomp: u32 = field(&parts, "Uncomp").parse().unwrap();
            let want_flags: u32 = field(&parts, "Flags").parse().unwrap();
            let want_eng = field(&parts, "Eng");
            let want_rev = field(&parts, "Rev");
            let want_blocks: usize = field(&parts, "Blocks").parse().unwrap();
            let want_nodes: usize = field(&parts, "Nodes").parse().unwrap();
            let want_total: usize = field(&parts, "Total").parse().unwrap();
            let want_head_len: usize = field(&parts, "HeadLen").parse().unwrap();
            let want_pad: usize = field(&parts, "Pad").parse().unwrap();

            assert_eq!(bytes.len(), want_total, "v{ver} total length");
            let bundle = UnityFsBundle::parse(bytes)
                .unwrap_or_else(|e| panic!("v{ver} Rust parse failed: {e:?}"));
            assert_eq!(bundle.header.version, want_version, "v{ver} version");
            assert_eq!(bundle.header.declared_size, want_size, "v{ver} size");
            assert_eq!(
                bundle.header.compressed_blocks_info_size, want_comp,
                "v{ver} comp"
            );
            assert_eq!(
                bundle.header.uncompressed_blocks_info_size, want_uncomp,
                "v{ver} uncomp"
            );
            assert_eq!(bundle.header.flags, want_flags, "v{ver} flags");
            assert_eq!(bundle.header.unity_version, want_eng, "v{ver} engine");
            assert_eq!(bundle.header.unity_revision, want_rev, "v{ver} revision");
            assert_eq!(bundle.header.header_size, want_head_len, "v{ver} head len");
            assert_eq!(bundle.storage_blocks.len(), want_blocks, "v{ver} blocks");
            assert_eq!(bundle.nodes.len(), want_nodes, "v{ver} nodes");
            assert_eq!(bundle.uncompressed_data_hash, [0; 16], "v{ver} hash");
            assert_eq!(
                bundle.metadata_offset,
                want_head_len + want_pad,
                "v{ver} metadata offset"
            );
            assert_eq!(bundle.data_offset, want_total, "v{ver} data offset");
            if want_version >= BUNDLE_VERSION_LARGE_FILES_SUPPORT {
                assert_eq!(
                    bundle.metadata_offset % 16,
                    0,
                    "v{ver} v7+ metadata alignment"
                );
                // Versions 7 and 8 share the large-files alignment rule.
                assert!(want_pad < 16, "v{ver} pad range");
            } else {
                assert_eq!(want_pad, 0, "v{ver} pre-7 has no pad");
                assert_eq!(
                    bundle.metadata_offset, bundle.header.header_size,
                    "v{ver} pre-7 metadata follows header"
                );
            }
        }
    }

    #[test]
    fn metadata_tables_match_reference_constructed_bundles() {
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

        // Probe Temp/opencode/unityfsmetaprobe (FileStreamBundleFile.Read over
        // C#-constructed full bundles, SDK 10.0.401; LZ4 via K4os, LZMA via
        // LzmaCompression.CompressLzmaStream): A None combined 1x1 start v6,
        // B None separate 1x0 start v6, C LZ4 combined, D LZMA combined,
        // E LZHAM unsupported, F at-end v6, G at-end v7, H truncated.
        // Nothing here was produced by the Rust writer.
        let fixture = include_str!("unityfs_meta_fixture.txt");
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("META_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad META_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("META_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad META_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        // Baseline refuses LZHAM metadata; Rust must fail loud, never succeed.
        let e_dump = reads.get("E").expect("E dump");
        assert!(
            e_dump.contains("UnsupportedBundleDecompression"),
            "E refusal {e_dump}"
        );
        let e_bytes = hexes.get("E").expect("E hex");
        assert!(UnityFsBundle::parse(e_bytes).is_err(), "E Rust fails loud");

        // Truncated metadata fails loud on both sides (different messages,
        // both loud, no silent success).
        let h_dump = reads.get("H").expect("H dump");
        assert!(h_dump.starts_with("THROW:"), "H refusal {h_dump}");
        let h_bytes = hexes.get("H").expect("H hex");
        assert!(UnityFsBundle::parse(h_bytes).is_err(), "H Rust fails loud");

        // All decodable lanes reproduce blocks, nodes, header, and offset.
        for case in ["A", "B", "C", "D", "F", "G"] {
            let bytes = hexes.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let dump = reads.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let parts: Vec<&str> = dump.split('|').collect();
            let want_version: i32 = field(&parts, "V").parse().unwrap();
            let want_size: u64 = field(&parts, "Size").parse().unwrap();
            let want_comp: u32 = field(&parts, "Comp").parse().unwrap();
            let want_uncomp: u32 = field(&parts, "Uncomp").parse().unwrap();
            let want_flags: u32 = field(&parts, "Flags").parse().unwrap();
            let want_blocks: usize = field(&parts, "Blocks").parse().unwrap();
            let want_nodes: usize = field(&parts, "Nodes").parse().unwrap();
            let want_total: usize = field(&parts, "Total").parse().unwrap();

            assert_eq!(bytes.len(), want_total, "{case} total length");
            let bundle = UnityFsBundle::parse(bytes)
                .unwrap_or_else(|e| panic!("{case} Rust parse failed: {e:?}"));
            assert_eq!(bundle.header.version, want_version, "{case} version");
            assert_eq!(bundle.header.declared_size, want_size, "{case} size");
            assert_eq!(
                bundle.header.compressed_blocks_info_size, want_comp,
                "{case} comp"
            );
            assert_eq!(
                bundle.header.uncompressed_blocks_info_size, want_uncomp,
                "{case} uncomp"
            );
            assert_eq!(bundle.header.flags, want_flags, "{case} flags");
            assert_eq!(bundle.storage_blocks.len(), want_blocks, "{case} blocks");
            assert_eq!(bundle.nodes.len(), want_nodes, "{case} nodes");
            assert_eq!(want_blocks, 1, "{case} one block");
            let block = &bundle.storage_blocks[0];
            assert_eq!(block.uncompressed_size, 10, "{case} block uncomp");
            assert_eq!(block.compressed_size, 10, "{case} block comp");
            assert_eq!(block.compression_type(), 0, "{case} block None");
            if case == "B" {
                assert_eq!(want_nodes, 0, "B separate has no directory");
            } else {
                assert_eq!(want_nodes, 1, "{case} one node");
                let node = &bundle.nodes[0];
                assert_eq!(node.offset, 0, "{case} node offset");
                assert_eq!(node.size, 10, "{case} node size");
                assert_eq!(node.path, "data/file.bin", "{case} node path");
            }
            // Placement: start cases follow the header; end cases sit at
            // Size minus the compressed metadata length.
            if case == "F" || case == "G" {
                assert_eq!(
                    bundle.metadata_offset as u64,
                    want_size - u64::from(want_comp),
                    "{case} metadata at end"
                );
            } else {
                assert_eq!(
                    bundle.metadata_offset, bundle.header.header_size,
                    "{case} metadata follows header"
                );
            }
        }
    }

    #[test]
    fn entry_bytes_match_reference_constructed_bundles() {
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

        // Probe Temp/opencode/unityfsentryprobe (FileStreamBundleFile.Read with
        // ResourceFiles/FailedFiles over C#-constructed bundles, SDK 10.0.401;
        // LZ4 via K4os, LZMA via LzmaCompression, Zstd via ZstdSharp with
        // Huffman/FSE for D): A None, B LZ4, C LZMA, D Zstd, E reassembly,
        // F non-combined empty, G LZHAM unsupported (Failed, loud).
        let fixture = include_str!("unityfs_entry_fixture.txt");
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("ENTRY_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad ENTRY_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("ENTRY_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad ENTRY_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        // Decodable lanes reproduce entry bytes on both sides.
        for case in ["A", "B", "C", "D", "E"] {
            let bytes = hexes.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let dump = reads.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let file_hex = dump
                .split('|')
                .find_map(|p| p.strip_prefix("FILE:data/file.bin="))
                .unwrap_or_else(|| panic!("{case} no FILE row in {dump}"));
            let expected = hex_to_bytes(file_hex);
            let bundle = UnityFsBundle::parse(bytes)
                .unwrap_or_else(|e| panic!("{case} Rust parse failed: {e:?}"));
            assert_eq!(bundle.nodes.len(), 1, "{case} one node");
            let entry = bundle
                .read_entry(0)
                .unwrap_or_else(|e| panic!("{case} Rust entry failed: {e:?}"));
            assert_eq!(&entry[..], &expected[..], "{case} entry bytes");
        }

        // Non-combined directories carry zero entries on both sides.
        {
            let bytes = hexes.get("F").expect("F hex");
            let dump = reads.get("F").expect("F dump");
            assert!(dump.contains("Nodes=0"), "F no nodes {dump}");
            assert!(dump.contains("Res=0"), "F no resources {dump}");
            let bundle = UnityFsBundle::parse(bytes).expect("F Rust parse");
            assert!(bundle.nodes.is_empty(), "F Rust no nodes");
        }

        // LZHAM entries stay loud failures on both sides, never silent.
        {
            let bytes = hexes.get("G").expect("G hex");
            let dump = reads.get("G").expect("G dump");
            assert!(dump.contains("Failed=1"), "G reference failed {dump}");
            let bundle = UnityFsBundle::parse(bytes).expect("G Rust parse");
            assert_eq!(bundle.nodes.len(), 1, "G one node");
            assert!(bundle.read_entry(0).is_err(), "G Rust fails loud");
        }
    }
