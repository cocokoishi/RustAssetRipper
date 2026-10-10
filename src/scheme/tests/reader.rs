use super::support::*;
use super::*;

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
    let fixture = include_str!("../../io/io_differential_stream_fixture.txt");
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
#[test]
fn recognizes_a_source_generation_22_serialized_file_header() {
    let mut bytes = [0; 61];
    bytes[8..12].copy_from_slice(&22i32.to_be_bytes());
    bytes[20..24].copy_from_slice(&13i32.to_be_bytes());
    bytes[24..32].copy_from_slice(&61u64.to_be_bytes());
    assert_eq!(
        SchemeReader::read(&bytes).unwrap().kind(),
        FileKind::SerializedFile
    );
}

#[test]
fn raw_header_probe_classifies_without_reading_the_file_body() {
    let mut bytes = [0; 61];
    bytes[8..12].copy_from_slice(&22i32.to_be_bytes());
    bytes[20..24].copy_from_slice(&13i32.to_be_bytes());
    bytes[24..32].copy_from_slice(&61u64.to_be_bytes());

    assert_eq!(
        SchemeReader::probe_raw_kind(&bytes[..32], bytes.len() as u64),
        FileKind::SerializedFile
    );
    assert_eq!(
        SchemeReader::probe_raw_kind(&bytes[..32], 62),
        FileKind::Resource
    );

    let mut bundle_header = vec![0; 32];
    bundle_header[..7].copy_from_slice(b"UnityFS");
    assert_eq!(
        SchemeReader::probe_raw_kind(&bundle_header, 64),
        FileKind::UnityFs
    );
}

#[test]
fn detection_edges_follow_the_source_minimums_and_readability() {
    // Empty input is an unknown resource, not a scheme.
    let empty = SchemeReader::read(b"").unwrap();
    assert_eq!(empty.kind(), FileKind::Resource);
    assert!(!empty.is_readable());
    assert!(!SchemeReader::is_readable(b""));
    // A bundle magic below the 0x20-byte source minimum does not dispatch.
    let short = b"UnityFS\0".to_vec();
    let short_file = SchemeReader::read(&short).unwrap();
    assert_eq!(short_file.kind(), FileKind::Resource);
    assert!(!SchemeReader::is_readable(&short));
    assert_eq!(
        SchemeReader::probe_raw_kind(b"short", 100),
        FileKind::Resource
    );
    // A wrapped resource counts as scheme-readable; the bare form does not.
    let wrapped = compress_gzip(b"payload", 1024).unwrap();
    let wrapped_file = SchemeReader::read(&wrapped).unwrap();
    assert_eq!(wrapped_file.kind(), FileKind::Resource);
    assert!(wrapped_file.is_readable());
    assert!(SchemeReader::is_readable(&wrapped));
}

/// The source `BundleHeader.IsBundleHeader` minimum applies to the whole stream
/// length, not the buffered prefix: a short prefix of a long stream still detects
/// when it holds the magic and its terminator.
#[test]
fn bundle_signature_probes_use_the_total_stream_length() {
    let mut archive_prefix = vec![0; 13];
    archive_prefix[..12].copy_from_slice(b"UnityArchive");
    assert_eq!(
        SchemeReader::probe_raw_kind(&archive_prefix, 100),
        FileKind::UnityArchive
    );
    let mut fs_prefix = vec![0; 8];
    fs_prefix[..7].copy_from_slice(b"UnityFS");
    assert_eq!(
        SchemeReader::probe_raw_kind(&fs_prefix, 64),
        FileKind::UnityFs
    );
    // Below the total-length minimum nothing dispatches, even with a full magic.
    assert_eq!(
        SchemeReader::probe_raw_kind(&archive_prefix, 20),
        FileKind::Resource
    );
    // A prefix cut inside the magic never matches.
    assert_eq!(
        SchemeReader::probe_raw_kind(b"Unity", 100),
        FileKind::Resource
    );
}

#[test]
fn archive_detection_and_unsupported_read_match_reference() {
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

    let fixture = include_str!("../../container/bundle_header_fixture.txt");
    let mut detects: std::collections::HashMap<String, (Vec<u8>, bool, bool)> =
        std::collections::HashMap::new();
    let mut throws: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in fixture.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if line.starts_with("ARCHIVE_DETECT|") {
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts.len(), 5, "bad ARCHIVE_DETECT {line}");
            detects.insert(
                parts[1].to_string(),
                (
                    hex_to_bytes(parts[2]),
                    parts[3] == "True",
                    parts[4] == "True",
                ),
            );
        } else if line.starts_with("ARCHIVE_THROW|") {
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts.len(), 3, "bad ARCHIVE_THROW {line}");
            throws.insert(parts[1].to_string(), parts[2].to_string());
        }
    }

    // The baseline throws NotSupportedException for both Read and Write;
    // Rust fails loudly with ArchiveReadUnsupported instead of succeeding.
    assert!(throws
        .get("Read")
        .unwrap()
        .contains("NotSupportedException"));
    assert!(throws
        .get("Write")
        .unwrap()
        .contains("NotSupportedException"));

    for (case, (bytes, want_archive, want_unityfs)) in &detects {
        // Total-length minimum: the reference checks BaseStream.Length >= 0x20.
        let total = bytes.len() as u64;
        let kind = super::SchemeReader::probe_raw_kind(bytes, total);
        if *want_archive {
            assert_eq!(
                kind,
                super::FileKind::UnityArchive,
                "case {case} archive dispatch"
            );
            assert!(super::SchemeReader::is_readable(bytes));
            let file = super::SchemeReader::read(bytes).unwrap();
            assert_eq!(file.kind(), super::FileKind::UnityArchive);
            assert!(matches!(
                file.parse().unwrap_err().kind(),
                super::SchemeErrorKind::ArchiveReadUnsupported
            ));
        } else if *want_unityfs {
            assert_eq!(
                kind,
                super::FileKind::UnityFs,
                "case {case} unityfs dispatch"
            );
        } else {
            assert_eq!(
                kind,
                super::FileKind::Resource,
                "case {case} stays a resource"
            );
        }
        // The short and empty vectors prove the 0x20 total-length minimum:
        // the same 13-byte UnityArchive prefix dispatches only with a long
        // total length, which the dedicated length-threaded test pins.
        if case == "short" || case == "empty" {
            assert!(!want_archive && !want_unityfs);
        }
    }
}

/// Reference-runtime differential for the container and compression dispatch rows.
///
/// Fixture: `src/io/io_differential_fixture.txt`, produced by
/// `tools/probes/io-differential` against the shipped reference `AssetRipper.IO.Files.dll`
/// from the frozen build. Every row replays independently built container bytes through the
/// production `SchemeReader` and compares the recursive inventory with the reference's:
/// the selected scheme, UnityFS metadata codecs (None/LZ4/LZ4HC/LZMA/source-rejected
/// LZHAM/unknown), storage codecs (None/LZMA/LZ4/LZ4HC/Zstandard and source-rejected
/// LZHAM with valid-sibling continuation), multi-block entries crossing a boundary,
/// combined and uncombined directory metadata, front/end placement and padding alignment,
/// Raw/Web headers for every bundle version 1 through 8, Web's last-scene selection, nested
/// bundle entries, UnityArchive rejection, plain-resource fallback, and GZip recursion. The
/// v7 at-end cases also combine compressed metadata with padded and multi-block entry data.
#[test]
fn scheme_dispatch_matches_the_reference_runtime_fixture() {
    fn wrapper_name(kind: WrapperKind) -> &'static str {
        match kind {
            WrapperKind::Gzip => "GZipFile",
            WrapperKind::Brotli => "BrotliFile",
        }
    }

    fn kind_name(kind: FileKind) -> &'static str {
        match kind {
            FileKind::UnityFs => "FileStreamBundleFile",
            FileKind::UnityRaw => "RawBundleFile",
            FileKind::UnityWebBundle => "WebBundleFile",
            FileKind::UnityArchive => "ArchiveBundleFile",
            FileKind::UnityWebData => "WebFile",
            FileKind::SerializedFile => "SerializedFile",
            FileKind::Resource => "ResourceFile",
        }
    }

    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            write!(text, "{byte:02X}").expect("string write");
        }
        text
    }

    fn walk(dispatched: &super::DispatchedFile<'_>, name: &str, out: &mut Vec<String>) {
        for wrapper in dispatched.wrappers() {
            out.push(format!("CODEC:{}", wrapper_name(wrapper.kind)));
        }
        match dispatched.kind() {
            FileKind::Resource => out.push(format!("RES:{name}:{}", hex(dispatched.bytes()))),
            FileKind::UnityFs => {
                let Ok(super::ParsedFile::UnityFs(bundle)) = dispatched.parse() else {
                    out.push(format!("PARSE_FAIL:{name}"));
                    return;
                };
                let header_index = out.len();
                out.push(String::new());
                let mut resources = 0usize;
                let mut failed_names = Vec::new();
                for (index, node) in bundle.nodes.iter().enumerate() {
                    let Ok(entry) = bundle.read_entry(index) else {
                        failed_names.push(node.path.as_str());
                        continue;
                    };
                    let Ok(child) = SchemeReader::read_with_limits(
                        entry.as_ref(),
                        SchemeReaderLimits::default(),
                    ) else {
                        failed_names.push(node.path.as_str());
                        continue;
                    };
                    if child.kind() == FileKind::Resource && child.wrappers().is_empty() {
                        resources += 1;
                    }
                    walk(&child, &node.path, out);
                }
                for failed_name in &failed_names {
                    out.push(format!("FAIL:{failed_name}"));
                }
                out[header_index] = format!(
                    "CONTAINER:{}:{name}:res={resources}:failed={}",
                    kind_name(FileKind::UnityFs),
                    failed_names.len()
                );
            }
            FileKind::UnityRaw | FileKind::UnityWebBundle => {
                let kind = dispatched.kind();
                let Ok(super::ParsedFile::RawWeb(bundle)) = dispatched.parse() else {
                    out.push(format!("PARSE_FAIL:{name}"));
                    return;
                };
                let header_index = out.len();
                out.push(String::new());
                let mut resources = 0usize;
                let mut failures = 0usize;
                for (index, node) in bundle.entries.iter().enumerate() {
                    let Ok(entry) = bundle.read_entry(index) else {
                        failures += 1;
                        out.push(format!("FAIL:{}", node.path));
                        continue;
                    };
                    let Ok(child) =
                        SchemeReader::read_with_limits(entry, SchemeReaderLimits::default())
                    else {
                        failures += 1;
                        out.push(format!("FAIL:{}", node.path));
                        continue;
                    };
                    if child.kind() == FileKind::Resource && child.wrappers().is_empty() {
                        resources += 1;
                    }
                    walk(&child, &node.path, out);
                }
                out[header_index] = format!(
                    "CONTAINER:{}:{name}:res={resources}:failed={failures}",
                    kind_name(kind)
                );
            }
            FileKind::UnityWebData => {
                let Ok(super::ParsedFile::WebData(web_file)) = dispatched.parse() else {
                    out.push(format!("PARSE_FAIL:{name}"));
                    return;
                };
                let header_index = out.len();
                out.push(String::new());
                let mut resources = 0usize;
                let mut failures = 0usize;
                let entries: Vec<String> = web_file
                    .entries
                    .iter()
                    .map(|entry| entry.name.clone())
                    .collect();
                for (index, entry_name) in entries.iter().enumerate() {
                    let Some(entry) = web_file.entry_data(index) else {
                        failures += 1;
                        out.push(format!("FAIL:{entry_name}"));
                        continue;
                    };
                    let Ok(child) =
                        SchemeReader::read_with_limits(entry, SchemeReaderLimits::default())
                    else {
                        failures += 1;
                        out.push(format!("FAIL:{entry_name}"));
                        continue;
                    };
                    if child.kind() == FileKind::Resource && child.wrappers().is_empty() {
                        resources += 1;
                    }
                    walk(&child, entry_name, out);
                }
                out[header_index] = format!(
                    "CONTAINER:{}:{name}:res={resources}:failed={failures}",
                    kind_name(FileKind::UnityWebData)
                );
            }
            FileKind::SerializedFile => out.push(format!("SER:{name}")),
            FileKind::UnityArchive => out.push(format!("PARSE_FAIL:{name}")),
        }
    }

    let mut checked = 0usize;
    for line in include_str!("../../io/io_differential_fixture.txt").lines() {
        let Some(rest) = line.strip_prefix("SCHEME|") else {
            continue;
        };
        let fields: Vec<&str> = rest.split('|').collect();
        let case = fields[0];
        let input = decode_hex_fixture(
            fields
                .iter()
                .find_map(|field| field.strip_prefix("in="))
                .unwrap_or_else(|| panic!("{case}: missing in=")),
        );
        let dispatched = SchemeReader::read_with_limits(&input, SchemeReaderLimits::default())
            .unwrap_or_else(|error| panic!("{case}: dispatch failed: {error}"));

        if let Some(throw) = fields.iter().find_map(|field| field.strip_prefix("THROW:")) {
            let error = dispatched
                .parse()
                .err()
                .unwrap_or_else(|| panic!("{case}: reference threw {throw}, Rust parsed"));
            match case {
                "archive" => assert!(
                    matches!(error.kind(), SchemeErrorKind::ArchiveReadUnsupported),
                    "{case}: {error}"
                ),
                "ufs_v6_unknown_meta" | "ufs_v6_lzham_meta" => assert!(
                    matches!(error.kind(), SchemeErrorKind::UnityFs(_)),
                    "{case}: {error}"
                ),
                other => panic!("unhandled reference throw row {other}"),
            }
            checked += 1;
            continue;
        }

        let expected_type = fields
            .iter()
            .find_map(|field| field.strip_prefix("type="))
            .unwrap_or_else(|| panic!("{case}: missing type="));
        let expected_tree = fields
            .iter()
            .find_map(|field| field.strip_prefix("tree="))
            .unwrap_or_else(|| panic!("{case}: missing tree="));
        let detected = dispatched.wrappers().first().map_or_else(
            || kind_name(dispatched.kind()),
            |layer| wrapper_name(layer.kind),
        );
        assert_eq!(detected, expected_type, "{case}: selected scheme");

        let mut inventory = Vec::new();
        walk(&dispatched, &format!("{case}.bin"), &mut inventory);
        assert_eq!(
            inventory.join(","),
            expected_tree,
            "{case}: recursive inventory"
        );

        if matches!(
            dispatched.kind(),
            FileKind::UnityFs
                | FileKind::UnityRaw
                | FileKind::UnityWebBundle
                | FileKind::UnityWebData
        ) {
            let expected_resources: Vec<(&str, &str)> = expected_tree
                .split(',')
                .filter_map(|part| part.strip_prefix("RES:")?.split_once(':'))
                .collect();
            let expected_failures: Vec<&str> = expected_tree
                .split(',')
                .filter_map(|part| part.strip_prefix("FAIL:"))
                .collect();
            let mut filesystem = crate::io::filesystem::VirtualFileSystem::new();
            let mounted = dispatched
                .mount_container_tree_with_source_name(
                    &mut filesystem,
                    Some("/io-differential"),
                    Some(&format!("{case}.bin")),
                    super::ContainerMountLimits::default(),
                )
                .unwrap_or_else(|error| panic!("{case}: recursive mount failed: {error}"));
            assert_eq!(
                mounted.failed_entries.len(),
                expected_failures.len(),
                "{case}: source FailedFile count"
            );
            for (failure, expected_name) in mounted
                .failed_entries
                .iter()
                .zip(expected_failures.iter().copied())
            {
                assert_eq!(
                    failure.source_name, expected_name,
                    "{case}: failed entry name"
                );
                assert_eq!(
                    failure.container_bundle_index,
                    Some(0),
                    "{case}: failure owner"
                );
                assert!(
                    matches!(
                        failure.error.kind(),
                        crate::container::unityfs::UnityFsErrorKind::UnsupportedCompression {
                            compression: 4,
                            ..
                        }
                    ),
                    "{case}: {failure:?}"
                );
            }
            assert_eq!(
                mounted.resource_files.len(),
                expected_resources.len(),
                "{case}: mounted resource count"
            );
            assert_eq!(
                mounted.mounted_entries,
                expected_resources.len(),
                "{case}: mounted leaf count"
            );
            for (resource, (expected_name, expected_hex)) in mounted
                .resource_files
                .iter()
                .zip(expected_resources.iter().copied())
            {
                assert_eq!(resource.name, expected_name, "{case}: source entry name");
                assert_eq!(
                    hex(&filesystem
                        .read_all_bytes(&resource.virtual_path, 1024)
                        .unwrap_or_else(|error| {
                            panic!("{case}: read mounted {}: {error}", resource.virtual_path)
                        })),
                    expected_hex,
                    "{case}: mounted bytes for {}",
                    resource.virtual_path
                );
            }
            if !expected_failures.is_empty() {
                assert_eq!(expected_failures, ["unsupported.bin"]);
                assert_eq!(
                    filesystem
                        .read_all_bytes("/io-differential/after.bin", 16)
                        .expect("valid sibling after the failed entry remains mounted"),
                    b"LATER",
                    "{case}: traversal continues after the failed entry"
                );

                let mut direct_filesystem = crate::io::filesystem::VirtualFileSystem::new();
                let direct = dispatched
                    .mount_container_entries(
                        &mut direct_filesystem,
                        Some("/direct"),
                        super::ContainerMountLimits::default(),
                    )
                    .unwrap_or_else(|error| panic!("{case}: direct mount failed: {error}"));
                assert_eq!(direct.failed_entries.len(), 1, "{case}: direct FailedFile");
                assert_eq!(direct.failed_entries[0].container_bundle_index, None);
                assert_eq!(
                    direct_filesystem
                        .read_all_bytes("/direct/after.bin", 16)
                        .expect("direct mount continues after the failed entry"),
                    b"LATER"
                );

                let mut limited_filesystem = crate::io::filesystem::VirtualFileSystem::new();
                let limited = dispatched
                    .mount_container_tree(
                        &mut limited_filesystem,
                        Some("/limited"),
                        super::ContainerMountLimits {
                            max_failed_entries: 0,
                            ..super::ContainerMountLimits::default()
                        },
                    )
                    .expect_err("a disabled failure-record budget must not silently succeed");
                assert!(matches!(
                    limited.kind,
                    super::ContainerMountErrorKind::FailedEntryCountLimitExceeded {
                        count: 1,
                        limit: 0
                    }
                ));
                assert_eq!(limited.partial_report.mounted_entries, 1);
            }
        }

        // The same independently produced bytes must dispatch identically through the
        // bounded seekable streaming path, which never materializes a compressed layer.
        let path = crate::test_support::unique_test_path("io-stream", "bin");
        std::fs::write(&path, &input).expect("test scratch file");
        let streamed = SchemeReader::read_path_streaming(&path, SchemeReaderLimits::default())
            .unwrap_or_else(|error| panic!("{case}: streaming dispatch failed: {error}"));
        std::fs::remove_file(&path).expect("remove scratch file");
        assert_eq!(streamed.kind(), dispatched.kind(), "{case}: streaming kind");
        assert_eq!(
            streamed.wrappers(),
            dispatched.wrappers(),
            "{case}: streaming wrapper layers"
        );
        let mut streamed_inventory = Vec::new();
        walk(&streamed, &format!("{case}.bin"), &mut streamed_inventory);
        assert_eq!(
            streamed_inventory.join(","),
            expected_tree,
            "{case}: streamed recursive inventory"
        );
        checked += 1;
    }
    assert_eq!(checked, 43, "reference fixture row count");
}

#[test]
fn wrapper_scheme_runtime_fixture_covers_brotli_detection_and_webdata_gzip() {
    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            write!(text, "{byte:02X}").unwrap();
        }
        text
    }

    let mut source_brotli_input = None;
    let mut source_brotli_type = None;
    let mut source_brotli_tree = None;
    let mut direct_brotli_output = None;
    let mut source_webdata_input = None;
    let mut source_webdata_tree = None;

    for line in include_str!("../../compression/wrapper_scheme_runtime_fixture.txt").lines() {
        let fields: Vec<&str> = line.split('|').collect();
        match fields.first().copied() {
            Some("SCHEME") if fields.get(1) == Some(&"brotli_ufs") => {
                source_brotli_input = fields
                    .iter()
                    .find_map(|field| field.strip_prefix("in="))
                    .map(decode_hex_fixture);
                source_brotli_type = fields.iter().find_map(|field| field.strip_prefix("type="));
                source_brotli_tree = fields.iter().find_map(|field| field.strip_prefix("tree="));
            }
            Some("BROTLI") if fields.get(1) == Some(&"brotli_ufs") => {
                assert!(fields.contains(&"type=ResourceFile"));
                direct_brotli_output = fields
                    .iter()
                    .find_map(|field| field.strip_prefix("decoded="))
                    .map(decode_hex_fixture);
            }
            Some("SCHEME") if fields.get(1) == Some(&"webdata_gzip_entry") => {
                source_webdata_input = fields
                    .iter()
                    .find_map(|field| field.strip_prefix("in="))
                    .map(decode_hex_fixture);
                source_webdata_tree = fields.iter().find_map(|field| field.strip_prefix("tree="));
            }
            _ => {}
        }
    }

    let source_brotli_input = source_brotli_input.expect("source SchemeReader Brotli row");
    assert_eq!(source_brotli_type, Some("ResourceFile"));
    assert_eq!(
        source_brotli_tree,
        Some(format!("RES:brotli_ufs.bin:{}", hex(&source_brotli_input)).as_str()),
        "reference SchemeReader currently fails to recognize this valid marked stream"
    );
    let source_direct_output =
        direct_brotli_output.expect("direct reference BrotliFile.Read result");
    assert!(source_direct_output.starts_with(b"UnityFS\0"));
    assert_eq!(
        crate::compression::wrappers::decompress_brotli(&source_brotli_input, 1024 * 1024).unwrap(),
        source_direct_output,
        "the reference BrotliFile decoder accepts the same bytes that its scheme detector misses"
    );

    let dispatched =
        SchemeReader::read_with_limits(&source_brotli_input, SchemeReaderLimits::default())
            .expect("RFC-valid marked Brotli stream dispatches in Rust");
    assert_eq!(dispatched.kind(), FileKind::UnityFs);
    assert_eq!(
        dispatched.wrappers().first().map(|layer| layer.kind),
        Some(WrapperKind::Brotli)
    );
    let super::ParsedFile::UnityFs(bundle) = dispatched.parse().unwrap() else {
        panic!("Brotli wrapper must dispatch its decoded UnityFS payload");
    };
    assert_eq!(bundle.nodes.len(), 1);
    assert_eq!(bundle.read_entry(0).unwrap().as_ref(), b"0123456789");

    let path = crate::test_support::unique_test_path("brotli-scheme-runtime", "bin");
    std::fs::write(&path, &source_brotli_input).expect("write reference wrapper bytes");
    let streamed = SchemeReader::read_path_streaming(&path, SchemeReaderLimits::default())
        .expect("file-backed Brotli wrapper dispatch");
    assert_eq!(streamed.kind(), FileKind::UnityFs);
    assert_eq!(streamed.wrappers(), dispatched.wrappers());

    let file_backed = SchemeReader::dispatch_stream_file_backed(
        crate::io::stream::SmartStream::open_read(&path).unwrap(),
        SchemeReaderLimits::default(),
    )
    .expect("file-backed Brotli wrapper stays seekable");
    assert_eq!(file_backed.kind, FileKind::UnityFs);
    assert_eq!(
        file_backed.wrappers(),
        &[crate::scheme::WrapperLayer {
            kind: WrapperKind::Brotli,
            input_size: source_brotli_input.len(),
            output_size: source_direct_output.len(),
        }]
    );
    assert_eq!(
        file_backed.stream.stream_type(),
        crate::io::stream::SmartStreamType::File,
        "decoded UnityFS is backed by a temporary stream"
    );

    let mut path_mount_files = crate::io::filesystem::VirtualFileSystem::new();
    let path_mount = SchemeReader::mount_path(
        &path,
        &mut path_mount_files,
        Some("/brotli-mount-path"),
        SchemeReaderLimits::default(),
        super::ContainerMountLimits::default(),
    )
    .expect("mount_path streams wrapped UnityFS from its temporary backing stream");
    assert_eq!(path_mount.mounted_entries, 1);
    assert_eq!(
        path_mount_files
            .read_all_bytes("/brotli-mount-path/data/file.bin", 32)
            .unwrap(),
        b"0123456789"
    );

    let mut batch_mount_files = crate::io::filesystem::VirtualFileSystem::new();
    let batch_input = [super::PathMountInput {
        path: path.clone(),
        destination_root: Some("/brotli-batch-path".to_owned()),
    }];
    let batch_mount = SchemeReader::mount_paths(
        &batch_input,
        &mut batch_mount_files,
        SchemeReaderLimits::default(),
        super::ContainerMountLimits::default(),
        super::BatchMountLimits::default(),
    )
    .expect("mount_paths streams wrapped UnityFS from its temporary backing stream");
    assert_eq!(batch_mount.completed_inputs, 1);
    assert_eq!(batch_mount.mounted_entries, 1);
    assert_eq!(
        batch_mount_files
            .read_all_bytes("/brotli-batch-path/data/file.bin", 32)
            .unwrap(),
        b"0123456789"
    );
    std::fs::remove_file(&path).expect("remove reference wrapper bytes");

    let mut filesystem = crate::io::filesystem::VirtualFileSystem::new();
    let mounted = streamed
        .mount_container_tree(
            &mut filesystem,
            Some("/brotli-wrapper"),
            super::ContainerMountLimits::default(),
        )
        .unwrap();
    assert_eq!(mounted.mounted_entries, 1);
    assert_eq!(
        filesystem
            .read_all_bytes("/brotli-wrapper/data/file.bin", 32)
            .unwrap(),
        b"0123456789"
    );

    let source_webdata_input = source_webdata_input.expect("source WebData GZip entry row");
    assert_eq!(
            source_webdata_tree,
            Some("CONTAINER:WebFile:webdata_gzip_entry.bin:res=1:failed=0,RES:nested.gz:7765626461746120677A6970207061796C6F6164")
        );
    let webdata = SchemeReader::read(&source_webdata_input).unwrap();
    assert_eq!(webdata.kind(), FileKind::UnityWebData);
    let entry = webdata.dispatch_entry(0).unwrap();
    assert_eq!(
        entry.wrappers().first().map(|layer| layer.kind),
        Some(WrapperKind::Gzip)
    );
    assert_eq!(entry.bytes(), b"webdata gzip payload");
    let path = crate::test_support::unique_test_path("webdata-gzip-streaming", "bin");
    std::fs::write(&path, &source_webdata_input).expect("write source WebData bytes");
    let streamed = SchemeReader::read_path_streaming(&path, SchemeReaderLimits::default())
        .expect("file-backed WebData dispatch");
    std::fs::remove_file(&path).expect("remove source WebData bytes");
    assert_eq!(streamed.kind(), webdata.kind());
    assert_eq!(streamed.wrappers(), webdata.wrappers());
    let streamed_entry = streamed.dispatch_entry(0).unwrap();
    assert_eq!(streamed_entry.wrappers(), entry.wrappers());
    assert_eq!(streamed_entry.bytes(), entry.bytes());
    let mut filesystem = crate::io::filesystem::VirtualFileSystem::new();
    let mounted = streamed
        .mount_container_tree(
            &mut filesystem,
            Some("/webdata-gzip"),
            super::ContainerMountLimits::default(),
        )
        .unwrap();
    assert_eq!(mounted.mounted_entries, 1);
    assert_eq!(mounted.resource_files[0].name, "nested.gz");
    assert_eq!(
        filesystem
            .read_all_bytes(&mounted.resource_files[0].virtual_path, 128)
            .unwrap(),
        b"webdata gzip payload"
    );
}

#[test]
fn source_standalone_serialized_file_dispatches_through_the_bounded_path_reader() {
    let row = include_str!("../../io/serialized_file_scheme_fixture.txt")
        .lines()
        .find(|line| line.starts_with("SCHEME|serialized_file_direct|"))
        .expect("source standalone SerializedFile SchemeReader row");
    let fields: Vec<&str> = row.split('|').collect();
    let input = fields
        .iter()
        .find_map(|field| field.strip_prefix("in="))
        .map(decode_hex_fixture)
        .expect("source SerializedFile input bytes");
    assert!(fields.contains(&"type=SerializedFile"));
    assert!(fields.contains(&"fixed=serialized_file_direct.bin"));
    assert!(fields.contains(&"tree=OTHER:SerializedFile:serialized_file_direct.bin"));

    let directory = crate::test_support::unique_test_path("serialized-file-scheme", "dir");
    std::fs::create_dir(&directory).expect("create serialized-file scratch directory");
    let path = directory.join("serialized_file_direct.bin");
    std::fs::write(&path, &input).expect("write source SerializedFile bytes");
    let streamed = SchemeReader::read_path_streaming(&path, SchemeReaderLimits::default())
        .expect("file-backed SerializedFile dispatch");
    assert_eq!(streamed.kind(), FileKind::SerializedFile);
    assert!(streamed.wrappers().is_empty());
    assert!(matches!(
        streamed.parse(),
        Ok(super::ParsedFile::SerializedFile(_))
    ));

    let mut filesystem = crate::io::filesystem::VirtualFileSystem::new();
    filesystem
        .create_directory(Some("/serialized"))
        .expect("create destination root");
    let mounted = SchemeReader::mount_path(
        &path,
        &mut filesystem,
        Some("/serialized"),
        SchemeReaderLimits::default(),
        super::ContainerMountLimits::default(),
    )
    .unwrap();
    assert_eq!(mounted.mounted_entries, 1);
    assert_eq!(mounted.mounted_bytes, input.len() as u64);
    assert_eq!(
        filesystem
            .read_all_bytes("/serialized/serialized_file_direct.bin", input.len())
            .unwrap(),
        input
    );
    std::fs::remove_file(&path).expect("remove source SerializedFile bytes");
    std::fs::remove_dir(&directory).expect("remove serialized-file scratch directory");
}

#[test]
fn file_backed_wrapper_path_spools_a_source_serialized_file_to_ranges() {
    let row = include_str!("../../io/serialized_file_scheme_fixture.txt")
        .lines()
        .find(|line| line.starts_with("SCHEME|serialized_file_direct|"))
        .expect("source standalone SerializedFile SchemeReader row");
    let input = row
        .split('|')
        .find_map(|field| field.strip_prefix("in="))
        .map(decode_hex_fixture)
        .expect("source SerializedFile input bytes");
    let expected = crate::serialized_file::file::SerializedFile::parse(&input).unwrap();
    let compressed = compress_gzip(&input, input.len() + 4096).unwrap();
    let input_stream = crate::io::stream::SmartStream::from_memory(compressed);

    let unwrapped =
        SchemeReader::unwrap_serialized_file_stream(input_stream, SchemeReaderLimits::default())
            .unwrap()
            .expect("GZip wrapper contains a SerializedFile");
    assert_eq!(
        unwrapped.stream_type(),
        crate::io::stream::SmartStreamType::File,
        "decoded bytes are spooled to a temporary file-backed stream"
    );

    let streamed =
        crate::serialized_file::file::StreamSerializedFile::try_parse(unwrapped, input.len())
            .unwrap()
            .expect("spooled source fixture parses as a SerializedFile");
    assert_eq!(streamed.header, expected.header);
    assert_eq!(streamed.metadata, expected.metadata);
    for (object_index, object) in expected.metadata.objects.iter().enumerate() {
        let range = object.data_range(&expected.header).unwrap();
        let start = usize::try_from(range.start).unwrap();
        let end = usize::try_from(range.end).unwrap();
        assert_eq!(
            streamed.read_object_payload_bytes(object_index).unwrap(),
            input[start..end],
            "source fixture object {object_index} range"
        );
    }
}

#[test]
fn compressed_source_serialized_file_matches_both_workspace_loaders() {
    let row = include_str!("../../io/serialized_file_scheme_fixture.txt")
        .lines()
        .find(|line| line.starts_with("SCHEME|serialized_file_direct|"))
        .expect("source standalone SerializedFile SchemeReader row");
    let input = row
        .split('|')
        .find_map(|field| field.strip_prefix("in="))
        .map(decode_hex_fixture)
        .expect("source SerializedFile input bytes");
    let compressed = compress_gzip(&input, input.len() + 4096).unwrap();

    fn load(
        bytes: &[u8],
    ) -> (
        crate::serialized_file::workspace::SerializedFileWorkspace,
        crate::serialized_file::object_workspace::ObjectWorkspace,
    ) {
        let mut filesystem = crate::io::filesystem::VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem
            .write_all_bytes("/game/source.assets", bytes)
            .unwrap();
        let inputs = [crate::io::dependency::DependencyFilePath {
            file_name: "source.assets".to_owned(),
            virtual_path: "/game/source.assets".to_owned(),
        }];
        let workspace = crate::serialized_file::workspace::SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            crate::serialized_file::workspace::SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let objects = crate::serialized_file::object_workspace::ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            crate::serialized_file::object_workspace::ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        (workspace, objects)
    }

    let (plain_workspace, plain_objects) = load(&input);
    let (wrapped_workspace, wrapped_objects) = load(&compressed);
    assert_eq!(plain_workspace.files().len(), 1);
    assert_eq!(wrapped_workspace.files().len(), 1);
    assert_eq!(
        wrapped_workspace.files()[0].header,
        plain_workspace.files()[0].header
    );
    assert_eq!(
        wrapped_workspace.files()[0].metadata,
        plain_workspace.files()[0].metadata
    );
    assert_eq!(
        wrapped_workspace.files()[0].reference_graph,
        plain_workspace.files()[0].reference_graph
    );
    assert_eq!(wrapped_objects.report(), plain_objects.report());
    assert_eq!(wrapped_objects.objects(), plain_objects.objects());
}

#[test]
fn file_backed_wrapper_spooling_enforces_decoded_input_limits() {
    let input = vec![b'a'; 4096];
    let compressed = compress_gzip(&input, input.len() + 4096).unwrap();
    let limits = SchemeReaderLimits {
        max_input_bytes: 1024,
        ..SchemeReaderLimits::default()
    };
    let result = SchemeReader::unwrap_serialized_file_stream(
        crate::io::stream::SmartStream::from_memory(compressed),
        limits,
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("decoded output beyond max_input_bytes must fail"),
    };
    assert!(matches!(
        error.kind(),
        SchemeErrorKind::InputSizeLimitExceeded { limit, .. } if *limit == 1024
    ));
}

#[test]
fn file_backed_wrapper_spooling_enforces_depth_layer_and_cumulative_limits() {
    fn expect_error(
        result: Result<Option<crate::io::stream::SmartStream>, crate::scheme::SchemeError>,
    ) -> crate::scheme::SchemeError {
        match result {
            Err(error) => error,
            Ok(_) => panic!("configured wrapper bound must fail closed"),
        }
    }

    let row = include_str!("../../io/serialized_file_scheme_fixture.txt")
        .lines()
        .find(|line| line.starts_with("SCHEME|serialized_file_direct|"))
        .expect("source standalone SerializedFile SchemeReader row");
    let input = row
        .split('|')
        .find_map(|field| field.strip_prefix("in="))
        .map(decode_hex_fixture)
        .expect("source SerializedFile input bytes");
    let inner = compress_gzip(&input, input.len() + 4096).unwrap();
    let outer = compress_gzip(&inner, inner.len() + 4096).unwrap();

    let depth_error = expect_error(SchemeReader::unwrap_serialized_file_stream(
        crate::io::stream::SmartStream::from_memory(outer.clone()),
        SchemeReaderLimits {
            max_wrapper_depth: 1,
            ..SchemeReaderLimits::default()
        },
    ));
    assert!(matches!(
        depth_error.kind(),
        SchemeErrorKind::WrapperDepthExceeded { limit: 1 }
    ));

    let layer_error = expect_error(SchemeReader::unwrap_serialized_file_stream(
        crate::io::stream::SmartStream::from_memory(outer.clone()),
        SchemeReaderLimits {
            max_decompressed_bytes_per_layer: inner.len() - 1,
            ..SchemeReaderLimits::default()
        },
    ));
    assert!(matches!(
        layer_error.kind(),
        SchemeErrorKind::WrapperDecode {
            source,
            ..
        } if matches!(
            source.kind(),
            crate::compression::wrappers::WrapperCodecErrorKind::OutputLimitExceeded { .. }
        )
    ));

    let cumulative_error = expect_error(SchemeReader::unwrap_serialized_file_stream(
        crate::io::stream::SmartStream::from_memory(outer),
        SchemeReaderLimits {
            max_total_decompressed_bytes: inner.len() + input.len() - 1,
            ..SchemeReaderLimits::default()
        },
    ));
    assert!(matches!(
        cumulative_error.kind(),
        SchemeErrorKind::TotalDecompressionLimitExceeded { limit, .. }
            if *limit == inner.len() + input.len() - 1
    ));
}

#[test]
fn brotli_stream_sink_matches_the_frozen_source_decode() {
    let fixture = include_str!("../../compression/wrapper_scheme_runtime_fixture.txt");
    let input_hex = fixture
        .lines()
        .find(|line| line.starts_with("SCHEME|brotli_ufs|"))
        .and_then(|line| line.split('|').find_map(|field| field.strip_prefix("in=")))
        .expect("source Brotli scheme input");
    let expected_hex = fixture
        .lines()
        .find(|line| line.starts_with("BROTLI|brotli_ufs|"))
        .and_then(|line| {
            line.split('|')
                .find_map(|field| field.strip_prefix("decoded="))
        })
        .expect("source Brotli direct-decode output");
    let input = decode_hex_fixture(input_hex);
    let expected = decode_hex_fixture(expected_hex);
    let mut output = crate::io::stream::SmartStream::create_temp().unwrap();
    let output_length = crate::compression::wrappers::decompress_brotli_stream_to(
        std::io::Cursor::new(input),
        &mut output,
        expected.len(),
    )
    .unwrap();
    assert_eq!(output_length, expected.len());
    assert_eq!(output.to_vec(expected.len()).unwrap(), expected);
}

/// The streaming path enforces the same wrapper bounds before materializing a layer.
#[test]
fn streaming_dispatch_enforces_wrapper_and_input_limits() {
    let inner = b"streaming limit payload".to_vec();
    let gzip = compress_gzip(&inner, 4096).unwrap();
    let path = crate::test_support::unique_test_path("io-stream-limit", "bin");
    std::fs::write(&path, &gzip).unwrap();

    // Zero wrapper depth refuses the wrapper layer outright.
    assert!(matches!(
        SchemeReader::read_path_streaming(
            &path,
            SchemeReaderLimits {
                max_wrapper_depth: 0,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err()
        .kind(),
        SchemeErrorKind::WrapperDepthExceeded { limit: 0 }
    ));

    // A length above the input bound is refused before any layer is read.
    assert!(matches!(
        SchemeReader::read_path_streaming(
            &path,
            SchemeReaderLimits {
                max_input_bytes: 4,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err()
        .kind(),
        SchemeErrorKind::InputSizeLimitExceeded { limit: 4, .. }
    ));

    // A per-layer bound that the decoded output exceeds is reported as a wrapper decode.
    assert!(matches!(
        SchemeReader::read_path_streaming(
            &path,
            SchemeReaderLimits {
                max_decompressed_bytes_per_layer: 4,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err()
        .kind(),
        SchemeErrorKind::WrapperDecode {
            kind: WrapperKind::Gzip,
            ..
        }
    ));

    // The aggregate decoded-byte budget is separate from the per-layer budget.
    assert!(matches!(
        SchemeReader::read_path_streaming(
            &path,
            SchemeReaderLimits {
                max_total_decompressed_bytes: 4,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err()
        .kind(),
        SchemeErrorKind::TotalDecompressionLimitExceeded { limit: 4, .. }
    ));

    std::fs::remove_file(&path).unwrap();
}
