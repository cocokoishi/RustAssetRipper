    use std::io::{Read, Seek, SeekFrom};

    use super::{
        check_streamed_resource_integrity, extract_streamed_resource_info,
        find_streamed_resource_infos, open_streamed_resource_range, read_streamed_resource_bytes,
        read_streamed_resource_info_bytes, resolve_and_read_streamed_resource_info,
        resolve_and_read_streamed_resource_range, resolve_streamed_resource_info,
        ExplicitRootResourceInput, InvalidStreamedResourceRange, LocatedStreamedResourceInfo,
        ResolvedResourceFile, RootResourceCatalogError, RootResourceCatalogLimits,
        RootResourceFileCatalog, StreamedResourceContentError, StreamedResourceContentLimits,
        StreamedResourceContentResolution, StreamedResourceError, StreamedResourceInfo,
        StreamedResourceInfoError, StreamedResourceLimits, StreamedResourceRange,
        StreamedResourceResolution, StreamedResourceScanError, StreamedResourceScanLimits,
    };
    use crate::io::filesystem::VirtualFileSystem;
    use crate::scheme::{FileKind, SchemeReader, SchemeReaderLimits};
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
    use crate::serialized_file::workspace::WorkspaceRootResourceFile;

    fn filesystem_with_resource() -> VirtualFileSystem {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/bundle")).unwrap();
        filesystem
            .write_all_bytes("/bundle/shared.resS", b"0123456789")
            .unwrap();
        filesystem
    }

    fn streaming_info_value(path: &[u8], offset: SerializedValue) -> SerializedValue {
        SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "path".to_owned(),
                value: SerializedValue::StringBytes(path.to_vec()),
            },
            SerializedFieldValue {
                name: "offset".to_owned(),
                value: offset,
            },
            SerializedFieldValue {
                name: "size".to_owned(),
                value: SerializedValue::U32(12),
            },
        ])
    }

    /// Explicit inputs are stored in reverse (file-stack pop) order so the last input
    /// wins duplicate names, matching `GameBundle.InitializeFromPaths`. Proven by a
    /// direct runtime differential: real `FromPaths([A/dup.ress, B/dup.ress])`
    /// resolves B's file.
    #[test]
    fn explicit_root_resource_catalog_applies_file_stack_pop_order() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/root")).unwrap();
        filesystem
            .write_all_bytes("/root/first.resS", b"first")
            .unwrap();
        filesystem
            .write_all_bytes("/root/second.resS", b"second")
            .unwrap();
        let first_dispatch =
            SchemeReader::read_with_limits(b"first resource", SchemeReaderLimits::default())
                .unwrap();
        let second_dispatch =
            SchemeReader::read_with_limits(b"second resource", SchemeReaderLimits::default())
                .unwrap();
        assert_eq!(first_dispatch.kind(), FileKind::Resource);

        let inputs = [
            ExplicitRootResourceInput {
                file_name: "SHARED.resS",
                virtual_path: "/root/first.resS",
                dispatched_file: &first_dispatch,
            },
            ExplicitRootResourceInput {
                file_name: "shared.ress",
                virtual_path: "/root/second.resS",
                dispatched_file: &second_dispatch,
            },
        ];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &inputs,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(
            catalog.files(),
            [
                WorkspaceRootResourceFile {
                    name: "shared.ress".to_owned(),
                    virtual_path: "/root/second.resS".to_owned(),
                },
                WorkspaceRootResourceFile {
                    name: "SHARED.resS".to_owned(),
                    virtual_path: "/root/first.resS".to_owned(),
                },
            ]
        );
        assert!(matches!(
            RootResourceFileCatalog::from_explicit_resource_inputs(
                &filesystem,
                &inputs,
                RootResourceCatalogLimits {
                    max_files: 1,
                    ..RootResourceCatalogLimits::default()
                },
            ),
            Err(RootResourceCatalogError::FileCountLimitExceeded { count: 2, limit: 1 })
        ));
        assert!(matches!(
            RootResourceFileCatalog::from_explicit_resource_inputs(
                &filesystem,
                &inputs,
                RootResourceCatalogLimits {
                    max_total_file_name_bytes: 21,
                    ..RootResourceCatalogLimits::default()
                },
            ),
            Err(RootResourceCatalogError::AggregateSizeLimitExceeded {
                resource: "explicit root ResourceFile names",
                byte_count: 22,
                limit: 21,
            })
        ));

        let mut bundle_bytes = vec![0; 32];
        bundle_bytes[..7].copy_from_slice(b"UnityFS");
        let bundle_dispatch =
            SchemeReader::read_with_limits(&bundle_bytes, SchemeReaderLimits::default()).unwrap();
        let wrong_kind = [ExplicitRootResourceInput {
            file_name: "wrong.resS",
            virtual_path: "/root/first.resS",
            dispatched_file: &bundle_dispatch,
        }];
        assert!(matches!(
            RootResourceFileCatalog::from_explicit_resource_inputs(
                &filesystem,
                &wrong_kind,
                RootResourceCatalogLimits::default(),
            ),
            Err(RootResourceCatalogError::NotResourceFile {
                input_index: 0,
                kind: FileKind::UnityFs,
            })
        ));
        let missing = [ExplicitRootResourceInput {
            file_name: "missing.resS",
            virtual_path: "/root/missing.resS",
            dispatched_file: &first_dispatch,
        }];
        assert!(matches!(
            RootResourceFileCatalog::from_explicit_resource_inputs(
                &filesystem,
                &missing,
                RootResourceCatalogLimits::default(),
            ),
            Err(RootResourceCatalogError::FileNotFound { input_index: 0, .. })
        ));
    }

    #[test]
    fn explicit_root_duplicates_resolve_last_input_first() {
        use super::resolve_resource_file_with_root_catalog;
        use crate::io::platform_structure::{PlatformResourceProvider, PlatformStructureLimits};
        use crate::serialized_file::workspace::SerializedFileWorkspace;

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/root")).unwrap();
        filesystem
            .write_all_bytes("/root/first.resS", b"first")
            .unwrap();
        filesystem
            .write_all_bytes("/root/second.resS", b"second")
            .unwrap();
        let first_dispatch =
            SchemeReader::read_with_limits(b"first resource", SchemeReaderLimits::default())
                .unwrap();
        let second_dispatch =
            SchemeReader::read_with_limits(b"second resource", SchemeReaderLimits::default())
                .unwrap();
        let inputs = [
            ExplicitRootResourceInput {
                file_name: "dup.resS",
                virtual_path: "/root/first.resS",
                dispatched_file: &first_dispatch,
            },
            ExplicitRootResourceInput {
                file_name: "DUP.ress",
                virtual_path: "/root/second.resS",
                dispatched_file: &second_dispatch,
            },
        ];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &inputs,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        let workspace = SerializedFileWorkspace::default();
        let provider = PlatformResourceProvider::new(None, None);
        assert_eq!(
            resolve_resource_file_with_root_catalog(
                &filesystem,
                &workspace,
                None,
                "dup.resS",
                &catalog,
                &provider,
                PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::ExplicitRoot {
                resource_index: 0,
                virtual_path: "/root/second.resS",
            },
            "last explicit input wins, like real FromPaths([first, second]) resolving second's file"
        );
    }

    #[test]
    fn streaming_info_extracts_both_baseline_offset_widths() {
        assert_eq!(
            extract_streamed_resource_info(&streaming_info_value(
                b"archive:/data/shared.resS",
                SerializedValue::U32(0xFEDC_BA98),
            ))
            .unwrap(),
            StreamedResourceInfo {
                path: "archive:/data/shared.resS".to_owned(),
                offset: 0xFEDC_BA98,
                size: 12,
            }
        );
        assert_eq!(
            extract_streamed_resource_info(&streaming_info_value(
                b"shared.resS",
                SerializedValue::U64(0x1_0000_0000),
            ))
            .unwrap(),
            StreamedResourceInfo {
                path: "shared.resS".to_owned(),
                offset: 0x1_0000_0000,
                size: 12,
            }
        );
        assert!(!extract_streamed_resource_info(&streaming_info_value(
            b"",
            SerializedValue::U32(0),
        ))
        .unwrap()
        .is_set());
    }

    #[test]
    fn nested_streaming_info_scan_preserves_field_paths_and_source_order() {
        let root = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "m_StreamData".to_owned(),
                value: streaming_info_value(b"first.resS", SerializedValue::U32(3)),
            },
            SerializedFieldValue {
                name: "m_Levels".to_owned(),
                value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    SerializedFieldValue {
                        name: "m_StreamData_C28".to_owned(),
                        value: streaming_info_value(b"second.resS", SerializedValue::U64(9)),
                    },
                ])]),
            },
        ]);

        let found =
            find_streamed_resource_infos(&root, StreamedResourceScanLimits::default()).unwrap();
        assert_eq!(
            found,
            [
                LocatedStreamedResourceInfo {
                    field_path: "m_StreamData".to_owned(),
                    info: StreamedResourceInfo {
                        path: "first.resS".to_owned(),
                        offset: 3,
                        size: 12,
                    },
                },
                LocatedStreamedResourceInfo {
                    field_path: "m_Levels[0].m_StreamData_C28".to_owned(),
                    info: StreamedResourceInfo {
                        path: "second.resS".to_owned(),
                        offset: 9,
                        size: 12,
                    },
                },
            ]
        );
    }

    #[test]
    fn nested_streaming_info_scan_reports_malformed_owner_shapes_and_limits() {
        let malformed = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_StreamData".to_owned(),
            value: SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "path".to_owned(),
                    value: SerializedValue::StringBytes(b"broken.resS".to_vec()),
                },
                SerializedFieldValue {
                    name: "offset".to_owned(),
                    value: SerializedValue::U32(0),
                },
            ]),
        }]);
        assert!(matches!(
            find_streamed_resource_infos(&malformed, StreamedResourceScanLimits::default()),
            Err(StreamedResourceScanError::InvalidInfo {
                field_path,
                source: StreamedResourceInfoError::MissingField { field: "size" },
            }) if field_path == "m_StreamData"
        ));
        let nested = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "nested".to_owned(),
            value: SerializedValue::Struct(Vec::new()),
        }]);
        assert!(matches!(
            find_streamed_resource_infos(
                &nested,
                StreamedResourceScanLimits {
                    max_value_nodes: 1,
                    ..StreamedResourceScanLimits::default()
                },
            ),
            Err(StreamedResourceScanError::ValueNodeLimitExceeded { count: 2, limit: 1 })
        ));
    }

    #[test]
    fn decoded_streaming_info_resolves_explicit_root_and_checks_its_range() {
        use crate::io::platform_structure::{PlatformResourceProvider, PlatformStructureLimits};
        use crate::serialized_file::workspace::SerializedFileWorkspace;

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/explicit")).unwrap();
        filesystem
            .write_all_bytes("/explicit/root.resS", b"0123456789ab")
            .unwrap();
        let dispatched =
            SchemeReader::read_with_limits(b"root resource", SchemeReaderLimits::default())
                .unwrap();
        let input = [ExplicitRootResourceInput {
            file_name: "root.resS",
            virtual_path: "/explicit/root.resS",
            dispatched_file: &dispatched,
        }];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &input,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        let info = StreamedResourceInfo {
            path: "root.resS".to_owned(),
            offset: 2,
            size: 4,
        };
        let workspace = SerializedFileWorkspace::default();
        let provider = PlatformResourceProvider::new(None, None);
        let resolution = resolve_streamed_resource_info(
            &filesystem,
            &workspace,
            None,
            &info,
            &catalog,
            &provider,
            PlatformStructureLimits::default(),
        )
        .unwrap();
        assert_eq!(
            resolution,
            StreamedResourceResolution::Resolved {
                resource: ResolvedResourceFile::ExplicitRoot {
                    resource_index: 0,
                    virtual_path: "/explicit/root.resS",
                },
                range_is_valid: true,
            }
        );
        assert_eq!(
            resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                None,
                &info,
                &catalog,
                &provider,
                StreamedResourceContentLimits::default(),
            )
            .unwrap(),
            StreamedResourceContentResolution::Resolved {
                resource: ResolvedResourceFile::ExplicitRoot {
                    resource_index: 0,
                    virtual_path: "/explicit/root.resS",
                },
                bytes: b"2345".to_vec(),
            }
        );
        assert_eq!(
            resolve_and_read_streamed_resource_range(
                &filesystem,
                &workspace,
                None,
                StreamedResourceRange {
                    path: "root.resS",
                    offset: 2,
                    size: 4,
                },
                &catalog,
                &provider,
                StreamedResourceContentLimits::default(),
            )
            .unwrap(),
            StreamedResourceContentResolution::Resolved {
                resource: ResolvedResourceFile::ExplicitRoot {
                    resource_index: 0,
                    virtual_path: "/explicit/root.resS",
                },
                bytes: b"2345".to_vec(),
            }
        );
    }

    #[test]
    fn streamed_resource_content_keeps_unset_missing_and_read_errors_distinct() {
        use crate::io::platform_structure::PlatformResourceProvider;
        use crate::serialized_file::workspace::SerializedFileWorkspace;

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/explicit")).unwrap();
        filesystem
            .write_all_bytes("/explicit/root.resS", b"0123456789ab")
            .unwrap();
        let dispatched =
            SchemeReader::read_with_limits(b"root resource", SchemeReaderLimits::default())
                .unwrap();
        let input = [ExplicitRootResourceInput {
            file_name: "root.resS",
            virtual_path: "/explicit/root.resS",
            dispatched_file: &dispatched,
        }];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &input,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        let workspace = SerializedFileWorkspace::default();
        let provider = PlatformResourceProvider::new(None, None);
        let content_limits = StreamedResourceContentLimits::default();

        let unset = StreamedResourceInfo {
            path: String::new(),
            offset: 0,
            size: 0,
        };
        assert_eq!(
            resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                None,
                &unset,
                &catalog,
                &provider,
                content_limits,
            )
            .unwrap(),
            StreamedResourceContentResolution::Unset
        );

        let missing = StreamedResourceInfo {
            path: "not-selected.resS".to_owned(),
            offset: 0,
            size: 4,
        };
        assert_eq!(
            resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                None,
                &missing,
                &catalog,
                &provider,
                content_limits,
            )
            .unwrap(),
            StreamedResourceContentResolution::Missing {
                unavailable_data_paths: Vec::new(),
            }
        );

        let outside = StreamedResourceInfo {
            path: "root.resS".to_owned(),
            offset: 10,
            size: 4,
        };
        assert!(matches!(
            resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                None,
                &outside,
                &catalog,
                &provider,
                content_limits,
            ),
            Err(StreamedResourceContentError::Read(
                StreamedResourceError::RangeOutsideResource {
                    offset: 10,
                    size: 4,
                    resource_size: 12,
                    ..
                }
            ))
        ));

        let wide_size = u64::from(u32::MAX) + 1;
        assert!(matches!(
            resolve_and_read_streamed_resource_range(
                &filesystem,
                &workspace,
                None,
                StreamedResourceRange {
                    path: "root.resS",
                    offset: 0,
                    size: wide_size,
                },
                &catalog,
                &provider,
                content_limits,
            ),
            Err(StreamedResourceContentError::Read(
                StreamedResourceError::MaterializationLimitExceeded { size, .. }
            )) if size == wide_size
        ));

        let over_limit = StreamedResourceInfo {
            path: "root.resS".to_owned(),
            offset: 0,
            size: 4,
        };
        assert!(matches!(
            resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                None,
                &over_limit,
                &catalog,
                &provider,
                StreamedResourceContentLimits {
                    range_read: StreamedResourceLimits {
                        max_materialized_bytes: 3,
                        ..content_limits.range_read
                    },
                    ..content_limits
                },
            ),
            Err(StreamedResourceContentError::Read(
                StreamedResourceError::MaterializationLimitExceeded {
                    size: 4,
                    limit: 3,
                    ..
                }
            ))
        ));
    }

    #[test]
    fn streaming_info_rejects_missing_duplicate_invalid_and_wrongly_typed_fields() {
        assert_eq!(
            extract_streamed_resource_info(&SerializedValue::Struct(vec![])).unwrap_err(),
            StreamedResourceInfoError::MissingField { field: "path" }
        );
        assert!(matches!(
            extract_streamed_resource_info(
                &streaming_info_value(&[0xFF], SerializedValue::U32(0),)
            ),
            Err(StreamedResourceInfoError::InvalidUtf8Path { .. })
        ));
        assert!(matches!(
            extract_streamed_resource_info(&streaming_info_value(
                b"resource.resS",
                SerializedValue::I64(5),
            )),
            Err(StreamedResourceInfoError::WrongFieldType {
                field: "offset",
                expected: "UInt32 or UInt64",
                actual: "Int64",
            })
        ));
        let duplicate_path = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "path".to_owned(),
                value: SerializedValue::StringBytes(b"one".to_vec()),
            },
            SerializedFieldValue {
                name: "path".to_owned(),
                value: SerializedValue::StringBytes(b"two".to_vec()),
            },
        ]);
        assert!(matches!(
            extract_streamed_resource_info(&duplicate_path),
            Err(StreamedResourceInfoError::FieldLookup { field: "path", .. })
        ));
        let mut wrong_size = streaming_info_value(b"resource.resS", SerializedValue::U32(0));
        let SerializedValue::Struct(fields) = &mut wrong_size else {
            unreachable!();
        };
        fields
            .iter_mut()
            .find(|field| field.name == "size")
            .unwrap()
            .value = SerializedValue::U64(12);
        assert!(matches!(
            extract_streamed_resource_info(&wrong_size),
            Err(StreamedResourceInfoError::WrongFieldType {
                field: "size",
                expected: "UInt32",
                actual: "UInt64",
            })
        ));
    }

    #[test]
    fn decoded_streaming_info_reads_its_range_from_the_resolved_resource_path() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/resolved")).unwrap();
        filesystem
            .write_all_bytes("/resolved/shared.resS", b"0123456789abcdefg")
            .unwrap();
        let info = extract_streamed_resource_info(&streaming_info_value(
            b"archive:/bundle/shared.resS",
            SerializedValue::U64(3),
        ))
        .unwrap();

        assert_eq!(
            read_streamed_resource_info_bytes(
                &filesystem,
                "/resolved/shared.resS",
                &info,
                StreamedResourceLimits::default(),
            )
            .unwrap(),
            Some(b"3456789abcde".to_vec())
        );
        let empty_info =
            extract_streamed_resource_info(&streaming_info_value(b"", SerializedValue::U32(0)))
                .unwrap();
        assert_eq!(
            read_streamed_resource_info_bytes(
                &filesystem,
                "/resolved/missing.resS",
                &empty_info,
                StreamedResourceLimits::default(),
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn reads_only_the_requested_streamed_resource_range() {
        let filesystem = filesystem_with_resource();
        let mut source_stream = filesystem.open_file("/bundle/shared.resS").unwrap();
        source_stream.seek(SeekFrom::Start(7)).unwrap();

        let bytes = read_streamed_resource_bytes(
            &filesystem,
            "/bundle/shared.resS",
            3,
            4,
            StreamedResourceLimits::default(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(bytes, b"3456");
        assert_eq!(source_stream.stream_position().unwrap(), 7);
        assert!(
            check_streamed_resource_integrity(&filesystem, "/bundle/shared.resS", 3, 4).unwrap()
        );
    }

    #[test]
    fn reads_streamed_resources_from_contiguous_split_files() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/bundle")).unwrap();
        filesystem
            .write_all_bytes("/bundle/chunk.resS.split0", b"012")
            .unwrap();
        filesystem
            .write_all_bytes("/bundle/chunk.resS.split1", b"345")
            .unwrap();

        assert_eq!(
            read_streamed_resource_bytes(
                &filesystem,
                "/bundle/chunk.resS",
                2,
                3,
                StreamedResourceLimits::default(),
            )
            .unwrap(),
            Some(b"234".to_vec())
        );
        assert!(
            check_streamed_resource_integrity(&filesystem, "/bundle/chunk.resS", 2, 3).unwrap()
        );
    }

    #[test]
    fn opens_an_independently_seekable_resource_range() {
        let filesystem = filesystem_with_resource();
        let mut range = open_streamed_resource_range(
            &filesystem,
            "/bundle/shared.resS",
            2,
            5,
            StreamedResourceLimits::default(),
        )
        .unwrap()
        .unwrap();
        range.seek(SeekFrom::Start(1)).unwrap();
        let mut bytes = [0; 2];
        range.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"34");
    }

    #[test]
    fn empty_resource_path_is_absent_and_integrity_compatible() {
        let filesystem = VirtualFileSystem::new();
        assert!(open_streamed_resource_range(
            &filesystem,
            "",
            0,
            0,
            StreamedResourceLimits::default()
        )
        .unwrap()
        .is_none());
        assert!(read_streamed_resource_bytes(
            &filesystem,
            "",
            0,
            0,
            StreamedResourceLimits::default()
        )
        .unwrap()
        .is_none());
        assert!(check_streamed_resource_integrity(&filesystem, "", u64::MAX, 0).unwrap());
    }

    #[test]
    fn rejects_zero_length_overflow_missing_and_out_of_bounds_ranges() {
        let filesystem = filesystem_with_resource();
        assert!(matches!(
            open_streamed_resource_range(
                &filesystem,
                "/bundle/shared.resS",
                0,
                0,
                StreamedResourceLimits::default()
            ),
            Err(StreamedResourceError::InvalidRange {
                reason: InvalidStreamedResourceRange::ZeroLength,
                ..
            })
        ));
        assert!(matches!(
            open_streamed_resource_range(
                &filesystem,
                "/bundle/shared.resS",
                u64::MAX,
                2,
                StreamedResourceLimits::default()
            ),
            Err(StreamedResourceError::InvalidRange { .. })
        ));
        assert!(matches!(
            open_streamed_resource_range(
                &filesystem,
                "/bundle/missing.resS",
                0,
                1,
                StreamedResourceLimits::default()
            ),
            Err(StreamedResourceError::ResourceNotFound { .. })
        ));
        assert!(matches!(
            open_streamed_resource_range(
                &filesystem,
                "/bundle/shared.resS",
                9,
                2,
                StreamedResourceLimits::default()
            ),
            Err(StreamedResourceError::RangeOutsideResource {
                resource_size: 10,
                ..
            })
        ));
        assert!(
            !check_streamed_resource_integrity(&filesystem, "/bundle/missing.resS", 0, 1).unwrap()
        );
        assert!(
            !check_streamed_resource_integrity(&filesystem, "/bundle/shared.resS", 0, 0).unwrap()
        );
    }

    #[test]
    fn check_integrity_rejects_short_and_huge_ranges_like_the_baseline() {
        // Edge outcomes from a direct runtime differential over the real
        // `StreamingInfo` extensions against a 10-byte resource: a range past the
        // end fails, and an offset outside the signed stream range fails. Where the
        // overflow-checked baseline throws `OverflowException`, this port returns
        // `InvalidRange`; both reject loudly instead of yielding a short buffer.
        let filesystem = filesystem_with_resource();
        assert!(
            !check_streamed_resource_integrity(&filesystem, "/bundle/shared.resS", 8, 4).unwrap(),
            "range past the end fails, like baseline `GetContent` returning null"
        );
        assert!(
            !check_streamed_resource_integrity(
                &filesystem,
                "/bundle/shared.resS",
                (i64::MAX as u64) + 1,
                4,
            )
            .unwrap(),
            "offset outside the signed range fails, like the baseline bounds check"
        );
    }

    #[test]
    fn enforces_memory_copy_and_materialized_output_limits() {
        let filesystem = filesystem_with_resource();
        let limits = StreamedResourceLimits {
            max_range_copy_bytes: 3,
            max_materialized_bytes: 4,
        };
        assert!(matches!(
            open_streamed_resource_range(&filesystem, "/bundle/shared.resS", 0, 4, limits),
            Err(StreamedResourceError::RangeCopyLimitExceeded { limit: 3, .. })
        ));
        assert!(matches!(
            read_streamed_resource_bytes(&filesystem, "/bundle/shared.resS", 0, 5, limits),
            Err(StreamedResourceError::MaterializationLimitExceeded { limit: 4, .. })
        ));
    }

    #[test]
    fn default_resource_files_match_the_source_extensions() {
        use super::is_default_resource_file;
        assert!(is_default_resource_file("shared.ress"));
        assert!(is_default_resource_file("shared.resource"));
        assert!(is_default_resource_file("SHARED.RESS"));
        assert!(is_default_resource_file("archive:/textures/shared.ResS"));
        assert!(!is_default_resource_file("shared.assets"));
        assert!(!is_default_resource_file("shared"));
        assert!(!is_default_resource_file("shared.ress.txt"));
        assert!(!is_default_resource_file(""));
    }

    /// The export-time value operations (`StreamingInfoExtensions`): snapshot the values,
    /// clear them for the inline export, then restore them exactly, as
    /// `YamlStreamedAssetExportCollection` does around `ExportInner`.
    #[test]
    fn streaming_info_values_clear_and_restore_around_exports() {
        use super::StreamedResourceInfo;
        let mut info = StreamedResourceInfo {
            path: String::from("archive:/level0"),
            offset: 128,
            size: 64,
        };
        assert!(info.is_set());
        let snapshot = (info.path.clone(), info.offset, info.size);

        info.clear_values();
        assert!(!info.is_set());
        assert_eq!(info.offset, 0);
        assert_eq!(info.size, 0);

        info.set_values(snapshot.0.clone(), snapshot.1, snapshot.2);
        assert!(info.is_set());
        assert_eq!(info.path, snapshot.0);
        assert_eq!(info.offset, snapshot.1);
        assert_eq!(info.size, snapshot.2);
    }
