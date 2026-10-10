    use super::{
        full_path, new_guid_string, LocalFileSystem, TextEncoding, TextEncodingFallbacks,
        TextFallback,
    };
    use crate::io::filesystem::SearchOption;
    use std::io::{self, Read};
    #[cfg(windows)]
    use std::io::{Seek, Write};
    use std::path::Path;

    #[cfg(windows)]
    fn is_hex_name(name: &str, length: usize) -> bool {
        name.len() == length && name.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    #[cfg(windows)]
    fn is_guid_name(name: &str) -> bool {
        let parts: Vec<&str> = name.split('-').collect();
        parts.len() == 5
            && [8, 4, 4, 4, 12]
                .into_iter()
                .zip(parts)
                .all(|(length, part)| is_hex_name(part, length))
    }

    #[cfg(windows)]
    fn bytes_as_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02X}")).collect()
    }

    #[test]
    fn local_text_io_handles_bom_detection_and_supported_encodings() {
        let root = crate::test_support::test_temp_root().join("local-fs-text");
        std::fs::create_dir_all(&root).unwrap();
        let filesystem = LocalFileSystem::with_roots(&root, root.join("system"));
        let path = root.join("text.txt");

        filesystem.write_all_text(&path, "café").unwrap();
        assert_eq!(filesystem.read_all_bytes(&path).unwrap(), b"caf\xC3\xA9");
        assert_eq!(filesystem.read_all_text(&path).unwrap(), "café");

        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Utf16Le,
            TextEncoding::Utf16Be,
            TextEncoding::Utf32Le,
            TextEncoding::Utf32Be,
        ] {
            filesystem
                .write_all_text_with_encoding(&path, "café🙂", encoding)
                .unwrap();
            assert_eq!(filesystem.read_all_text(&path).unwrap(), "café🙂");
            assert_eq!(
                filesystem
                    .read_all_text_with_encoding(&path, TextEncoding::Utf8)
                    .unwrap(),
                "café🙂",
                "a detected BOM takes precedence over the requested fallback encoding"
            );
        }

        filesystem.write_all_bytes(&path, b"bad\xFFtext").unwrap();
        assert_eq!(filesystem.read_all_text(&path).unwrap(), "bad\u{FFFD}text");
        filesystem
            .write_all_bytes(&path, b"\xFF\xFEa\x00\xFF")
            .unwrap();
        assert_eq!(filesystem.read_all_text(&path).unwrap(), "a\u{FFFD}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_and_virtual_text_encodings_match_the_source_runtime_fixture() {
        use crate::io::filesystem::VirtualFileSystem;
        use std::collections::HashMap;

        let fixture = include_str!("text_encoding_fixture.txt");
        let rows: HashMap<(String, String), String> = fixture
            .lines()
            .filter(|line| !line.starts_with('#'))
            .map(|line| {
                let mut fields = line.splitn(3, '|');
                let kind = fields.next().expect("fixture row has kind");
                let key = fields.next().expect("fixture row has key");
                let value = fields.next().expect("fixture row has value");
                ((kind.to_owned(), key.to_owned()), value.to_owned())
            })
            .collect();
        let expected = |kind: &str, key: &str| {
            rows.get(&(kind.to_owned(), key.to_owned()))
                .unwrap_or_else(|| panic!("missing fixture row {kind}|{key}"))
                .as_str()
        };
        let hex = |bytes: &[u8]| {
            bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        };

        let root = crate::test_support::test_temp_root().join("local-fs-encoding-fixture");
        std::fs::create_dir_all(&root).unwrap();
        let local = LocalFileSystem::with_roots(&root, root.join("system"));
        let mut virtual_fs = VirtualFileSystem::new();
        let replacement_fallbacks = TextEncodingFallbacks {
            encoder: TextFallback::Replacement("<encode>".to_owned()),
            decoder: TextFallback::Replacement("<decode>".to_owned()),
        };

        let code_pages = [1200, 1201, 12000, 12001, 20127, 28591, 65001];
        for code_page in code_pages {
            let key = code_page.to_string();
            let encoding = TextEncoding::from_code_page(code_page).unwrap();
            assert_eq!(encoding.code_page(), code_page);

            let local_path = root.join(format!("{code_page}-local.txt"));
            local
                .write_all_text_with_encoding(&local_path, "A", encoding)
                .unwrap();
            assert_eq!(
                hex(&local.read_all_bytes(&local_path).unwrap()),
                expected("LOCAL_WRITE", &key),
                "local preamble mismatch for code page {code_page}"
            );
            virtual_fs
                .write_all_text_with_encoding("/encoded.txt", "A", encoding)
                .unwrap();
            assert_eq!(
                hex(&virtual_fs.read_all_bytes("/encoded.txt", 64).unwrap()),
                expected("VFS_WRITE", &key),
                "virtual encoding bytes mismatch for code page {code_page}"
            );

            local.write_all_bytes(&local_path, &[0xff]).unwrap();
            assert_eq!(
                hex(local
                    .read_all_text_with_encoding(&local_path, encoding)
                    .unwrap()
                    .as_bytes()),
                expected("LOCAL_READ", &key),
                "local decoder default mismatch for code page {code_page}"
            );
            virtual_fs.write_all_bytes("/invalid.bin", &[0xff]).unwrap();
            assert_eq!(
                hex(virtual_fs
                    .read_all_text_with_encoding("/invalid.bin", 64, encoding)
                    .unwrap()
                    .as_bytes()),
                expected("VFS_READ", &key),
                "virtual decoder default mismatch for code page {code_page}"
            );

            assert_eq!(
                hex(local
                    .read_all_text_with_encoding_and_fallbacks(
                        &local_path,
                        encoding,
                        &replacement_fallbacks,
                    )
                    .unwrap()
                    .as_bytes()),
                expected("LOCAL_CUSTOM_READ", &key),
                "local custom decoder fallback mismatch for code page {code_page}"
            );
            assert_eq!(
                hex(virtual_fs
                    .read_all_text_with_encoding_and_fallbacks(
                        "/invalid.bin",
                        64,
                        encoding,
                        &replacement_fallbacks,
                    )
                    .unwrap()
                    .as_bytes()),
                expected("VFS_CUSTOM_READ", &key),
                "virtual custom decoder fallback mismatch for code page {code_page}"
            );

            local
                .write_all_text_with_encoding_and_fallbacks(
                    &local_path,
                    "Aé🙂",
                    encoding,
                    &replacement_fallbacks,
                )
                .unwrap();
            assert_eq!(
                hex(&local.read_all_bytes(&local_path).unwrap()),
                expected("LOCAL_CUSTOM_WRITE", &key),
                "local custom encoder fallback mismatch for code page {code_page}"
            );
            virtual_fs
                .write_all_text_with_encoding_and_fallbacks(
                    "/replacement.txt",
                    "Aé🙂",
                    encoding,
                    &replacement_fallbacks,
                )
                .unwrap();
            assert_eq!(
                hex(&virtual_fs.read_all_bytes("/replacement.txt", 128).unwrap()),
                expected("VFS_CUSTOM_WRITE", &key),
                "virtual custom encoder fallback mismatch for code page {code_page}"
            );
        }

        let malformed_cases: [(u32, &[u8]); 8] = [
            (1200, &[0x00, 0xd8, 0x41, 0x00, 0x42]),
            (1201, &[0xd8, 0x00, 0x00, 0x41, 0x42]),
            (12000, &[0x00, 0xd8, 0x00, 0x00, 0x00, 0x00, 0x11, 0x00]),
            (12001, &[0x00, 0x00, 0xd8, 0x00, 0x00, 0x11, 0x00, 0x00]),
            (20127, &[0x80, 0x41]),
            (28591, &[0x80, 0xff]),
            (65001, &[0xed, 0xa0, 0x80]),
            (65000, b"+2AA-"),
        ];
        for (code_page, bytes) in malformed_cases {
            let key = code_page.to_string();
            let encoding = TextEncoding::from_code_page(code_page).unwrap();
            let local_path = root.join(format!("{code_page}-malformed.bin"));
            local.write_all_bytes(&local_path, bytes).unwrap();
            assert_eq!(
                hex(local
                    .read_all_text_with_encoding(&local_path, encoding)
                    .unwrap()
                    .as_bytes()),
                expected("LOCAL_MALFORMED", &key),
                "local malformed decoder mismatch for code page {code_page}"
            );
            virtual_fs.write_all_bytes("/malformed.bin", bytes).unwrap();
            assert_eq!(
                hex(virtual_fs
                    .read_all_text_with_encoding("/malformed.bin", 64, encoding)
                    .unwrap()
                    .as_bytes()),
                expected("VFS_MALFORMED", &key),
                "virtual malformed decoder mismatch for code page {code_page}"
            );
        }

        let custom_utf8 = TextEncodingFallbacks {
            encoder: TextFallback::Replacement("<encode>".to_owned()),
            decoder: TextFallback::Replacement("<decode>".to_owned()),
        };
        local
            .write_all_bytes(root.join("custom-utf8-malformed.bin"), &[0xc3, 0x28, 0xff])
            .unwrap();
        assert_eq!(
            hex(local
                .read_all_text_with_encoding_and_fallbacks(
                    root.join("custom-utf8-malformed.bin"),
                    TextEncoding::Utf8,
                    &custom_utf8,
                )
                .unwrap()
                .as_bytes()),
            expected("CUSTOM_UTF8_MALFORMED", "-")
        );
        virtual_fs
            .write_all_bytes("/custom-utf8-malformed.bin", &[0xc3, 0x28, 0xff])
            .unwrap();
        assert_eq!(
            hex(virtual_fs
                .read_all_text_with_encoding_and_fallbacks(
                    "/custom-utf8-malformed.bin",
                    64,
                    TextEncoding::Utf8,
                    &custom_utf8,
                )
                .unwrap()
                .as_bytes()),
            expected("CUSTOM_UTF8_MALFORMED", "-")
        );
        let bom_path = root.join("utf8-bom.bin");
        local
            .write_all_bytes(&bom_path, b"\xEF\xBB\xBF\xC3(\xFF")
            .unwrap();
        assert_eq!(
            hex(local
                .read_all_text_with_encoding_and_fallbacks(
                    &bom_path,
                    TextEncoding::Utf8,
                    &custom_utf8,
                )
                .unwrap()
                .as_bytes()),
            expected("LOCAL_MATCHED_UTF8_BOM", "-")
        );
        let custom_ascii = TextEncodingFallbacks {
            encoder: TextFallback::Replacement("<encode>".to_owned()),
            decoder: TextFallback::Replacement("<decode>".to_owned()),
        };
        assert_eq!(
            hex(local
                .read_all_text_with_encoding_and_fallbacks(
                    &bom_path,
                    TextEncoding::Ascii,
                    &custom_ascii,
                )
                .unwrap()
                .as_bytes()),
            expected("LOCAL_MISMATCH_UTF8_BOM", "-")
        );
        virtual_fs
            .write_all_bytes("/bom.bin", b"\xEF\xBB\xBFA")
            .unwrap();
        assert_eq!(
            hex(virtual_fs
                .read_all_text_with_encoding("/bom.bin", 64, TextEncoding::Utf8)
                .unwrap()
                .as_bytes()),
            expected("VFS_BOM_TEXT", "-")
        );

        let ascii_fallbacks = TextEncodingFallbacks::for_encoding(TextEncoding::Ascii);
        assert_eq!(
            ascii_fallbacks.decoder,
            TextFallback::Replacement("?".to_owned())
        );
        local
            .write_all_text_with_encoding_and_fallbacks(
                &bom_path,
                "Aé🙂",
                TextEncoding::Ascii,
                &TextEncodingFallbacks::for_encoding(TextEncoding::Ascii),
            )
            .unwrap();
        assert_eq!(
            hex(&local.read_all_bytes(&bom_path).unwrap()),
            expected("DEFAULT_ASCII_WRITE", "-")
        );
        virtual_fs
            .write_all_text_with_encoding("/ascii.txt", "Aé🙂", TextEncoding::Ascii)
            .unwrap();
        assert_eq!(
            hex(&virtual_fs.read_all_bytes("/ascii.txt", 64).unwrap()),
            expected("DEFAULT_ASCII_WRITE", "-")
        );

        let mut throwing = TextEncodingFallbacks::for_encoding(TextEncoding::Ascii);
        throwing.encoder = TextFallback::Exception;
        virtual_fs.write_all_bytes("/preserve.txt", b"old").unwrap();
        let error = virtual_fs
            .write_all_text_with_encoding_and_fallbacks(
                "/preserve.txt",
                "Aé",
                TextEncoding::Ascii,
                &throwing,
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            hex(&virtual_fs.read_all_bytes("/preserve.txt", 64).unwrap()),
            expected("VFS_ENCODER_EXCEPTION_PRESERVES", "-")
        );
        assert_eq!(
            expected("VFS_ENCODER_EXCEPTION_KIND", "-"),
            "EncoderFallbackException"
        );
        let local_throwing_path = root.join("truncate-on-failure.txt");
        local.write_all_bytes(&local_throwing_path, b"old").unwrap();
        let error = local
            .write_all_text_with_encoding_and_fallbacks(
                &local_throwing_path,
                "Aé",
                TextEncoding::Ascii,
                &throwing,
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(local
            .read_all_bytes(&local_throwing_path)
            .unwrap()
            .is_empty());
        assert_eq!(expected("LOCAL_ENCODER_EXCEPTION_AFTER", "-"), "<empty>");
        assert_eq!(
            expected("LOCAL_ENCODER_EXCEPTION_KIND", "-"),
            "EncoderFallbackException"
        );

        let mut decoder_exception = TextEncodingFallbacks::for_encoding(TextEncoding::Utf8);
        decoder_exception.decoder = TextFallback::Exception;
        virtual_fs
            .write_all_bytes("/bad-utf8.bin", &[0xff])
            .unwrap();
        let error = virtual_fs
            .read_all_text_with_encoding_and_fallbacks(
                "/bad-utf8.bin",
                64,
                TextEncoding::Utf8,
                &decoder_exception,
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("code page 65001"));
        assert_eq!(
            expected("VFS_DECODER_EXCEPTION_KIND", "65001"),
            "DecoderFallbackException"
        );
        let local_bad_utf8_path = root.join("bad-utf8.bin");
        local
            .write_all_bytes(&local_bad_utf8_path, &[0xff])
            .unwrap();
        let error = local
            .read_all_text_with_encoding_and_fallbacks(
                &local_bad_utf8_path,
                TextEncoding::Utf8,
                &decoder_exception,
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            expected("LOCAL_DECODER_EXCEPTION_KIND", "65001"),
            "DecoderFallbackException"
        );

        assert_eq!(
            rows.keys().filter(|(kind, _)| kind == "ENCODING").count(),
            8
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn utf7_codec_matches_the_source_runtime_fixture() {
        use crate::io::filesystem::VirtualFileSystem;
        use std::collections::HashMap;

        let rows: HashMap<(String, String), String> = include_str!("text_encoding_fixture.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .map(|line| {
                let mut fields = line.splitn(3, '|');
                (
                    (
                        fields.next().expect("fixture row has kind").to_owned(),
                        fields.next().expect("fixture row has key").to_owned(),
                    ),
                    fields.next().expect("fixture row has value").to_owned(),
                )
            })
            .collect();
        let expected = |kind: &str, key: &str| {
            let value = rows
                .get(&(kind.to_owned(), key.to_owned()))
                .unwrap_or_else(|| panic!("missing fixture row {kind}|{key}"))
                .as_str();
            if value == "<empty-hex>" {
                ""
            } else {
                value
            }
        };
        let hex = |bytes: &[u8]| {
            bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        };

        let encoding = TextEncoding::from_code_page(65_000).unwrap();
        assert_eq!(encoding, TextEncoding::Utf7);
        let root = crate::test_support::test_temp_root().join("local-fs-utf7-fixture");
        std::fs::create_dir_all(&root).unwrap();
        let local = LocalFileSystem::with_roots(&root, root.join("system"));
        let mut virtual_fs = VirtualFileSystem::new();

        let local_path = root.join("utf7.txt");
        local
            .write_all_text_with_encoding(&local_path, "Aé🙂", encoding)
            .unwrap();
        assert_eq!(
            hex(&local.read_all_bytes(&local_path).unwrap()),
            expected("LOCAL_CUSTOM_WRITE", "65000")
        );
        virtual_fs
            .write_all_text_with_encoding("/utf7.txt", "Aé🙂", encoding)
            .unwrap();
        assert_eq!(
            hex(&virtual_fs.read_all_bytes("/utf7.txt", 64).unwrap()),
            expected("VFS_CUSTOM_WRITE", "65000")
        );

        let utf7_cases: [(&str, &[u8]); 10] = [
            ("plain", b"A~"),
            ("plus", b"+-"),
            ("shift-e-acute", b"+AOk-"),
            ("shift-emoji", b"+2D3eQg-"),
            ("unclosed-shift", b"+AOk"),
            ("dangling-plus", b"+"),
            ("invalid-base64", b"+!A-"),
            ("incomplete-unit", b"+A-"),
            ("invalid-surrogate", b"+2AA-"),
            ("high-byte", &[0xff]),
        ];
        for (name, bytes) in utf7_cases {
            virtual_fs
                .write_all_bytes("/utf7-input.bin", bytes)
                .unwrap();
            let actual = virtual_fs
                .read_all_text_with_encoding("/utf7-input.bin", 64, encoding)
                .unwrap();
            assert_eq!(
                hex(actual.as_bytes()),
                expected("UTF7_READ", name),
                "UTF-7 decode mismatch for {name}"
            );
        }

        let printable: String = (0x20..=0x7e).map(char::from).collect();
        let mut printable_vfs = VirtualFileSystem::new();
        printable_vfs
            .write_all_text_with_encoding("/printable.txt", &printable, encoding)
            .unwrap();
        assert_eq!(
            hex(&printable_vfs.read_all_bytes("/printable.txt", 256).unwrap()),
            expected("UTF7_PRINTABLE", "-")
        );
        for (name, text) in [
            ("plus", "+"),
            ("two-plus", "++"),
            ("direct-plus", "x+"),
            ("plus-direct", "+x"),
            ("optional-plus", "*+"),
            ("plus-optional", "+*"),
            ("plus-unicode", "+é"),
            ("unicode-plus", "é+"),
            ("unicode-plus-direct", "é+A"),
        ] {
            printable_vfs
                .write_all_text_with_encoding("/plus.txt", text, encoding)
                .unwrap();
            assert_eq!(
                hex(&printable_vfs.read_all_bytes("/plus.txt", 64).unwrap()),
                expected("UTF7_ENCODE", name),
                "UTF-7 encoder mismatch for {name}"
            );
        }

        let custom = TextEncodingFallbacks {
            encoder: TextFallback::Replacement("<encode>".to_owned()),
            decoder: TextFallback::Replacement("<decode>".to_owned()),
        };
        virtual_fs
            .write_all_bytes("/utf7-custom-input.bin", b"+!A-")
            .unwrap();
        assert_eq!(
            hex(virtual_fs
                .read_all_text_with_encoding_and_fallbacks(
                    "/utf7-custom-input.bin",
                    64,
                    encoding,
                    &custom,
                )
                .unwrap()
                .as_bytes()),
            expected("UTF7_CUSTOM_READ", "invalid-base64")
        );
        virtual_fs
            .write_all_bytes("/utf7-custom-input.bin", &[0xff])
            .unwrap();
        assert_eq!(
            hex(virtual_fs
                .read_all_text_with_encoding_and_fallbacks(
                    "/utf7-custom-input.bin",
                    64,
                    encoding,
                    &custom,
                )
                .unwrap()
                .as_bytes()),
            expected("UTF7_CUSTOM_READ", "high-byte")
        );
        for (name, bytes) in [
            ("dangling-plus", b"+".as_slice()),
            ("incomplete-unit", b"+A-".as_slice()),
            ("invalid-surrogate", b"+2AA-".as_slice()),
        ] {
            virtual_fs
                .write_all_bytes("/utf7-custom-input.bin", bytes)
                .unwrap();
            assert_eq!(
                hex(virtual_fs
                    .read_all_text_with_encoding_and_fallbacks(
                        "/utf7-custom-input.bin",
                        64,
                        encoding,
                        &custom,
                    )
                    .unwrap()
                    .as_bytes()),
                expected("UTF7_CUSTOM_READ", name),
                "custom UTF-7 decode mismatch for {name}"
            );
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_directory_enumeration_matches_search_options_and_patterns() {
        let root = crate::test_support::test_temp_root().join("local-fs-enumeration");
        let nested = root.join("nested");
        std::fs::create_dir_all(nested.join("deep")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        for path in [
            root.join("alpha.txt"),
            root.join("BETA.TXT"),
            root.join("no-extension"),
            nested.join("inner.txt"),
            nested.join("deep").join("deep.txt"),
        ] {
            std::fs::write(path, b"fixture").unwrap();
        }
        let filesystem = LocalFileSystem::with_roots(&root, root.join("system"));
        let files = filesystem
            .enumerate_files(&root, "*.txt", SearchOption::TopDirectoryOnly)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        #[cfg(windows)]
        assert_eq!(names, ["BETA.TXT", "alpha.txt"]);
        #[cfg(not(windows))]
        assert_eq!(names, ["alpha.txt"]);

        let all_files = filesystem
            .enumerate_files(&root, "*.txt", SearchOption::AllDirectories)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(all_files.len(), if cfg!(windows) { 4 } else { 3 });
        let directories = filesystem
            .enumerate_directories(&root, "*", SearchOption::AllDirectories)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(directories.len(), 3);
        assert!(filesystem
            .enumerate_files(&root, "", SearchOption::TopDirectoryOnly)
            .is_err());
        assert!(filesystem
            .enumerate_files(&root, "nested/*.txt", SearchOption::AllDirectories)
            .is_err());
        let missing = root.join("removed-after-enumerator-creation");
        assert!(matches!(
            filesystem.enumerate_files(&missing, "*", SearchOption::TopDirectoryOnly),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn directory_enumeration_error_timing_matches_reference_runtime_fixture() {
        let root = crate::test_support::test_temp_root().join("local-fs-enumeration-timing");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let filesystem = LocalFileSystem::with_roots(&root, root.join("system"));
        let missing = root.join("missing");
        let mut actual = Vec::new();

        fn error_name(error: &io::Error) -> &'static str {
            match error.kind() {
                io::ErrorKind::NotFound => "DirectoryNotFoundException",
                io::ErrorKind::InvalidInput => "ArgumentException",
                io::ErrorKind::NotADirectory => "IOException",
                _ => "other",
            }
        }

        fn call_result<T>(result: io::Result<T>) -> &'static str {
            match result {
                Ok(_) => "ok",
                Err(error) => error_name(&error),
            }
        }

        fn first_move(enumeration: &mut super::DirectoryEnumeration) -> &'static str {
            match enumeration.next() {
                Some(Ok(_)) => "true",
                Some(Err(error)) => error_name(&error),
                None => "false",
            }
        }

        actual.push(format!(
            "ENUM_TIMING|missing-files|call={}",
            call_result(filesystem.enumerate_files_in(&missing))
        ));
        actual.push(format!(
            "ENUM_TIMING|missing-get-files|call={}",
            call_result(filesystem.get_files(&missing))
        ));
        actual.push(format!(
            "ENUM_TIMING|missing-dirs|call={}",
            call_result(filesystem.enumerate_directories_in(&missing))
        ));
        actual.push(format!(
            "ENUM_TIMING|missing-and-invalid|call={}",
            call_result(filesystem.enumerate_files_matching(&missing, "bad\0pattern"))
        ));

        let file_root = root.join("file-root");
        std::fs::write(&file_root, b"file").unwrap();
        let mut file_root_enumeration = filesystem.enumerate_files_in(&file_root).unwrap();
        actual.push("ENUM_TIMING|file-as-root|call=ok".to_owned());
        actual.push(format!(
            "ENUM_TIMING|file-as-root|first-move={}",
            first_move(&mut file_root_enumeration)
        ));

        actual.push(format!(
            "ENUM_TIMING|empty-path|call={}",
            call_result(filesystem.enumerate_files_in(Path::new("")))
        ));
        actual.push(format!(
            "ENUM_TIMING|invalid-pattern|call={}",
            call_result(filesystem.enumerate_files_matching(&root, "bad\0pattern"))
        ));

        let removed_root = root.join("removed-root");
        std::fs::create_dir(&removed_root).unwrap();
        let mut removed_enumeration = filesystem.enumerate_files_in(&removed_root).unwrap();
        std::fs::remove_dir(&removed_root).unwrap();
        actual.push(format!(
            "ENUM_TIMING|removed-root|first-move={}",
            first_move(&mut removed_enumeration)
        ));

        let removed_directory_root = root.join("removed-directory-root");
        std::fs::create_dir(&removed_directory_root).unwrap();
        let mut removed_directory_enumeration = filesystem
            .enumerate_directories_in(&removed_directory_root)
            .unwrap();
        std::fs::remove_dir(&removed_directory_root).unwrap();
        actual.push(format!(
            "ENUM_TIMING|removed-directory-root|first-move={}",
            first_move(&mut removed_directory_enumeration)
        ));

        let late_file_root = root.join("late-file-root");
        std::fs::create_dir(&late_file_root).unwrap();
        let mut late_file_enumeration = filesystem
            .enumerate_files_matching(&late_file_root, "late.txt")
            .unwrap();
        std::fs::write(late_file_root.join("late.txt"), b"late").unwrap();
        actual.push(format!(
            "ENUM_TIMING|late-file|move-result={}",
            first_move(&mut late_file_enumeration)
        ));

        let late_directory_root = root.join("late-directory-root");
        std::fs::create_dir(&late_directory_root).unwrap();
        let mut late_directory_enumeration = filesystem
            .enumerate_directories_matching(&late_directory_root, "late")
            .unwrap();
        std::fs::create_dir(late_directory_root.join("late")).unwrap();
        actual.push(format!(
            "ENUM_TIMING|late-directory|move-result={}",
            first_move(&mut late_directory_enumeration)
        ));

        let expected: Vec<&str> = include_str!("enumeration_timing_fixture.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect();
        assert_eq!(
            actual.iter().map(String::as_str).collect::<Vec<_>>(),
            expected
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_path_helpers_match_path_join_and_component_contracts() {
        let root = crate::test_support::test_temp_root().join("local-fs-paths");
        let filesystem = LocalFileSystem::with_roots(&root, root.join("system"));
        assert_eq!(
            filesystem.join_paths(&["alpha", "beta", "gamma"]),
            ["alpha", "beta", "gamma"].join(std::path::MAIN_SEPARATOR_STR)
        );
        assert_eq!(filesystem.get_file_name("alpha"), "alpha",);
        assert_eq!(filesystem.get_extension(".hidden"), ".hidden");
        assert_eq!(filesystem.get_file_name_without_extension("x.txt"), "x");
        assert_eq!(filesystem.get_file_name("alpha/beta/"), "");
        assert!(filesystem.is_path_rooted(std::path::MAIN_SEPARATOR.to_string()));
        assert_eq!(
            filesystem
                .get_relative_path(root.join("base"), root.join("base").join("a").join("b"))
                .unwrap(),
            Path::new("a").join("b")
        );
        assert!(filesystem.is_path_rooted("C:relative") == cfg!(windows));
    }

    #[test]
    fn local_file_methods_match_the_expected_sharing_and_delete_contracts() {
        let root = crate::test_support::test_temp_root().join("local-fs-methods");
        let filesystem = LocalFileSystem::with_roots(&root, root.join("system"));
        filesystem.create_directory(&root).unwrap();
        let path = root.join("input.bin");
        filesystem.write_all_bytes(&path, b"before").unwrap();
        {
            let mut reader = filesystem.open_read(&path).unwrap();
            let mut byte = [0];
            reader.read_exact(&mut byte).unwrap();
            assert_eq!(byte, [b'b']);
            let mut rest = Vec::new();
            reader.read_to_end(&mut rest).unwrap();
            assert_eq!(rest, b"efore");
        }
        filesystem.write_all_bytes(&path, b"after").unwrap();
        assert_eq!(filesystem.read_all_bytes(&path).unwrap(), b"after");
        filesystem.delete_file(&path).unwrap();
        filesystem.delete_file(&path).unwrap();
        assert!(!filesystem.file_exists(&path));
        filesystem.delete_directory(&root).unwrap();
    }

    #[test]
    fn local_temp_directory_prefers_executable_path_and_honors_setter_rules() {
        let root = crate::test_support::test_temp_root().join("local-fs-temp");
        let executing = root.join("application");
        let system = root.join("system");
        std::fs::create_dir_all(&executing).unwrap();
        let filesystem = LocalFileSystem::with_roots(&executing, &system);
        let selected = filesystem.temporary_directory().unwrap();
        assert!(selected.starts_with(executing.join("temp")));
        assert!(
            !selected.exists(),
            "the source probes then deletes this folder"
        );
        let file = filesystem.create_temporary_file().unwrap();
        assert!(file.starts_with(&selected));
        assert!(filesystem.file_exists(&file));
        let directory = filesystem.create_temporary_directory().unwrap();
        assert!(directory.starts_with(&selected));
        assert!(filesystem.directory_exists(&directory));

        filesystem
            .set_temporary_directory(root.join("custom"))
            .unwrap();
        let custom = full_path(&root.join("custom")).unwrap();
        assert_eq!(filesystem.temporary_directory().unwrap(), custom);
        filesystem.set_temporary_directory("   ").unwrap();
        assert_eq!(filesystem.temporary_directory().unwrap(), custom);
        filesystem.delete_temporary_directory().unwrap();
        assert!(!custom.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_temp_directory_falls_back_when_executable_path_is_not_a_directory() {
        let root = crate::test_support::test_temp_root().join("local-fs-fallback");
        std::fs::create_dir_all(&root).unwrap();
        let not_a_directory = root.join("not-a-directory");
        std::fs::write(&not_a_directory, b"file").unwrap();
        let system = root.join("system");
        let filesystem = LocalFileSystem::with_roots(&not_a_directory, &system);
        let selected = filesystem.temporary_directory().unwrap();
        assert!(selected.starts_with(system.join("AssetRipper")));
        let file = filesystem.create_temporary_file().unwrap();
        assert!(filesystem.file_exists(&file));
        filesystem.delete_temporary_directory().unwrap();
        assert!(!selected.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_file_path_helpers_normalize_components_and_guid_names() {
        let full = full_path(Path::new("alpha/../beta")).unwrap();
        assert!(full.ends_with("beta"));
        let first = new_guid_string();
        let second = new_guid_string();
        assert_eq!(first.len(), 36);
        assert_eq!(&first[14..15], "4");
        assert!(first != second);
    }

    #[cfg(windows)]
    #[test]
    fn local_filesystem_operations_match_the_frozen_runtime_fixture() {
        let expected: Vec<&str> = include_str!("local_filesystem_fixture.txt")
            .lines()
            .filter(|line| {
                !line.starts_with('#')
                    && [
                        "DEFAULT_TEMP|",
                        "SET_TEMP|",
                        "BLANK_SETTER|",
                        "TEMP_FILE|",
                        "TEMP_DIRECTORY|",
                        "OPEN_WRITE|",
                        "WRITE_WHILE_READ_OPEN|",
                        "DELETE_TEMP|",
                        "DELETE_MISSING_FILE|",
                        "PATH|",
                        "ENUM_FILES|",
                        "ENUMERATE_",
                        "ENUM_DIRS|",
                        "TEXT|",
                        "PATH_HELPERS|",
                    ]
                    .iter()
                    .any(|prefix| line.starts_with(prefix))
            })
            .collect();
        let root = crate::test_support::test_temp_root().join("local-fs-source-fixture");
        let executable = root.join("application");
        let system = root.join("system");
        std::fs::create_dir_all(&executable).unwrap();
        let filesystem = LocalFileSystem::with_roots(&executable, &system);
        let selected = filesystem.temporary_directory().unwrap();
        let local_name = selected.file_name().unwrap().to_string_lossy();
        let mut actual = vec![format!(
            "DEFAULT_TEMP|app-local={}|exists={}",
            selected.starts_with(executable.join("temp")) && is_hex_name(&local_name, 4),
            selected.exists()
        )];

        let configured = root.join("custom").join("..").join("temp");
        filesystem.set_temporary_directory(&configured).unwrap();
        let normalized = filesystem.temporary_directory().unwrap();
        let normalized_string = normalized.to_string_lossy();
        actual.push(format!(
            "SET_TEMP|full={}|normalized={}",
            normalized == full_path(&configured).unwrap(),
            !normalized_string.contains("..")
        ));
        filesystem.set_temporary_directory("   ").unwrap();
        actual.push(format!(
            "BLANK_SETTER|unchanged={}",
            filesystem.temporary_directory().unwrap() == normalized
        ));

        let file = filesystem.create_temporary_file().unwrap();
        actual.push(format!(
            "TEMP_FILE|inside={}|exists={}|length={}|guid={}",
            file.parent() == Some(normalized.as_path()),
            filesystem.file_exists(&file),
            std::fs::metadata(&file).unwrap().len(),
            is_guid_name(&file.file_name().unwrap().to_string_lossy())
        ));

        let child = filesystem.create_temporary_directory().unwrap();
        actual.push(format!(
            "TEMP_DIRECTORY|inside={}|guid8={}|exists={}",
            child.parent() == Some(normalized.as_path()),
            is_hex_name(&child.file_name().unwrap().to_string_lossy(), 8),
            filesystem.directory_exists(&child)
        ));

        let open_write = root.join("openwrite.bin");
        filesystem.write_all_bytes(&open_write, b"abcdef").unwrap();
        let mut writer = filesystem.open_write(&open_write).unwrap();
        let initial_position = writer.stream_position().unwrap();
        writer.write_all(b"XY").unwrap();
        drop(writer);
        actual.push(format!("OPEN_WRITE|initial-position={initial_position}"));
        let open_write_bytes = filesystem.read_all_bytes(&open_write).unwrap();
        actual.push(format!(
            "OPEN_WRITE|bytes={}",
            open_write_bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        ));

        let retained = root.join("retained.bin");
        filesystem.write_all_bytes(&retained, b"before").unwrap();
        let reader = filesystem.open_read(&retained).unwrap();
        let write_rejected = filesystem.write_all_bytes(&retained, b"after").is_err();
        drop(reader);
        actual.push(format!(
            "WRITE_WHILE_READ_OPEN|{}",
            if write_rejected {
                "REJECTED"
            } else {
                "ALLOWED"
            }
        ));

        let nested_file = child.join("nested").join("file.bin");
        filesystem
            .create_directory(nested_file.parent().unwrap())
            .unwrap();
        filesystem.write_all_bytes(&nested_file, b"temp").unwrap();
        filesystem.delete_temporary_directory().unwrap();
        actual.push(format!(
            "DELETE_TEMP|removed={}|repeat={}",
            !normalized.exists(),
            filesystem.delete_temporary_directory().is_ok()
        ));

        let missing = root.join("missing");
        let missing_delete =
            filesystem.delete_file(&missing).is_ok() && !filesystem.file_exists(&missing);
        actual.push(format!("DELETE_MISSING_FILE|no-op={missing_delete}"));
        actual.push(format!(
            "PATH|rooted-dot={}|rooted-root={}|full-relative={}",
            filesystem.is_path_rooted("."),
            filesystem.is_path_rooted(std::path::MAIN_SEPARATOR.to_string()),
            filesystem.get_full_path(".").unwrap().is_absolute()
        ));

        let enumeration_root = root.join("enumeration");
        let nested = enumeration_root.join("nested");
        let deep = nested.join("deep");
        let other = enumeration_root.join("other");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        for (path, contents) in [
            (enumeration_root.join("alpha.txt"), "alpha"),
            (enumeration_root.join("BETA.TXT"), "beta"),
            (enumeration_root.join("no-extension"), "none"),
            (enumeration_root.join(".hidden"), "hidden"),
            (nested.join("inner.txt"), "inner"),
            (deep.join("deep.txt"), "deep"),
            (other.join("other.bin"), "other"),
        ] {
            std::fs::write(path, contents).unwrap();
        }
        let relative_list = |paths: Vec<std::path::PathBuf>| {
            let mut paths: Vec<String> = paths
                .into_iter()
                .map(|path| {
                    path.strip_prefix(&enumeration_root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            paths.sort();
            paths.join(",")
        };
        actual.push(format!(
            "ENUM_FILES|all={}",
            relative_list(filesystem.get_files(&enumeration_root).unwrap())
        ));
        actual.push(format!(
            "ENUM_FILES|txt={}",
            relative_list(
                filesystem
                    .get_files_matching(&enumeration_root, "*.txt")
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUM_FILES|star-dot-star={}",
            relative_list(
                filesystem
                    .get_files_matching(&enumeration_root, "*.*")
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUM_FILES|recursive={}",
            relative_list(
                filesystem
                    .get_files_with_options(
                        &enumeration_root,
                        "*.txt",
                        SearchOption::AllDirectories
                    )
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_FILES|all={}",
            relative_list(
                filesystem
                    .enumerate_files_in(&enumeration_root)
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_FILES|txt={}",
            relative_list(
                filesystem
                    .enumerate_files_matching(&enumeration_root, "*.txt")
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_FILES|recursive={}",
            relative_list(
                filesystem
                    .enumerate_files(&enumeration_root, "*.txt", SearchOption::AllDirectories)
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUM_DIRS|top={}",
            relative_list(filesystem.get_directories(&enumeration_root).unwrap())
        ));
        actual.push(format!(
            "ENUM_DIRS|recursive={}",
            relative_list(
                filesystem
                    .get_directories_with_options(
                        &enumeration_root,
                        "*",
                        SearchOption::AllDirectories
                    )
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_DIRS|top={}",
            relative_list(
                filesystem
                    .enumerate_directories_in(&enumeration_root)
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_DIRS|pattern={}",
            relative_list(
                filesystem
                    .enumerate_directories_matching(&enumeration_root, "*")
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_DIRS|recursive={}",
            relative_list(
                filesystem
                    .enumerate_directories(&enumeration_root, "*", SearchOption::AllDirectories)
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));

        let text_path = root.join("text.txt");
        filesystem.write_all_text(&text_path, "café").unwrap();
        actual.push(format!(
            "TEXT|default-write={}",
            bytes_as_hex(&filesystem.read_all_bytes(&text_path).unwrap())
        ));
        filesystem
            .write_all_bytes(&text_path, b"\xEF\xBB\xBFcaf\xC3\xA9")
            .unwrap();
        actual.push(format!(
            "TEXT|utf8-bom-read={}",
            bytes_as_hex(filesystem.read_all_text(&text_path).unwrap().as_bytes())
        ));
        filesystem
            .write_all_bytes(&text_path, b"\xFF\xFEc\x00\xE9\x00")
            .unwrap();
        actual.push(format!(
            "TEXT|utf16-bom-read={}",
            bytes_as_hex(filesystem.read_all_text(&text_path).unwrap().as_bytes())
        ));
        let explicit_encoding_path = root.join("explicit-encoding.txt");
        filesystem
            .write_all_text_with_encoding(&explicit_encoding_path, "café", TextEncoding::Utf16Le)
            .unwrap();
        actual.push(format!(
            "TEXT|utf16-write={}",
            bytes_as_hex(&filesystem.read_all_bytes(&explicit_encoding_path).unwrap())
        ));

        actual.push(format!(
            "PATH_HELPERS|join={}",
            filesystem.join_paths(&["alpha", "beta", "gamma"])
        ));
        actual.push(format!(
            "PATH_HELPERS|join-rooted={}",
            filesystem.join_paths(&["alpha", "\\beta"])
        ));
        actual.push(format!(
            "PATH_HELPERS|directory={}",
            filesystem
                .get_directory_name("alpha\\beta.ext")
                .unwrap()
                .display()
        ));
        actual.push(format!(
            "PATH_HELPERS|extension={}",
            filesystem.get_extension("alpha\\beta.ext")
        ));
        actual.push(format!(
            "PATH_HELPERS|filename={}",
            filesystem.get_file_name("alpha\\beta.ext")
        ));
        actual.push(format!(
            "PATH_HELPERS|filename-no-extension={}",
            filesystem.get_file_name_without_extension("alpha\\beta.ext")
        ));
        actual.push(format!(
            "PATH_HELPERS|dotfile-extension={}",
            filesystem.get_extension(".hidden")
        ));
        actual.push(format!(
            "PATH_HELPERS|trailing-filename={}",
            filesystem.get_file_name("alpha\\beta\\")
        ));
        actual.push(format!(
            "PATH_HELPERS|rooted-drive-relative={}",
            filesystem.is_path_rooted("C:relative")
        ));
        let relative = filesystem
            .get_relative_path(root.join("base"), root.join("base").join("a").join("b.txt"))
            .unwrap()
            .to_string_lossy()
            .into_owned();
        actual.push(format!("PATH_HELPERS|relative={relative}"));

        let actual: Vec<String> = actual
            .into_iter()
            .map(|line| line.replace("=true", "=True").replace("=false", "=False"))
            .collect();
        assert_eq!(
            actual.iter().map(String::as_str).collect::<Vec<_>>(),
            expected
        );
        let _ = std::fs::remove_dir_all(root);
    }
