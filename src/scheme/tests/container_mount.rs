use super::support::*;
use super::*;

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
fn archive_inputs_take_the_container_mount_branch() {
    let mut bytes = vec![0; 32];
    bytes[..12].copy_from_slice(b"UnityArchive");
    let file = SchemeReader::read(&bytes).unwrap();
    assert_eq!(file.kind(), FileKind::UnityArchive);
    let mut filesystem = VirtualFileSystem::new();
    let error = file
        .mount_container_tree(
            &mut filesystem,
            Some("/tree"),
            ContainerMountLimits::default(),
        )
        .unwrap_err();
    // The container branch parses the bundle: only the unsupported-reader error
    // surfaces, never a leaf mount or a not-a-container rejection.
    assert!(matches!(
        error.kind,
        ContainerMountErrorKind::Scheme(error)
        if matches!(error.kind(), SchemeErrorKind::ArchiveReadUnsupported)
    ));
    assert_eq!(filesystem.count(), 1);
}

#[test]
fn reports_the_baseline_archive_reader_as_explicitly_unsupported() {
    let mut bytes = vec![0; 32];
    bytes[..12].copy_from_slice(b"UnityArchive");
    let file = SchemeReader::read(&bytes).unwrap();
    assert_eq!(file.kind(), FileKind::UnityArchive);
    assert!(matches!(
        file.parse().unwrap_err().kind(),
        SchemeErrorKind::ArchiveReadUnsupported
    ));
}

#[test]
fn source_unknown_non_zstd_unityfs_block_is_retained_as_a_failed_entry() {
    let row = include_str!("../../compression/wrapper_scheme_runtime_fixture.txt")
        .lines()
        .find(|line| line.starts_with("SCHEME|ufs_v6_unknown_non_zstd_block|"))
        .expect("source SchemeReader unknown-code fallback row");
    let fields: Vec<&str> = row.split('|').collect();
    assert_eq!(fields.first(), Some(&"SCHEME"));
    assert!(fields.get(2).is_some_and(|field| field.starts_with("in=")));
    let input = fields
        .iter()
        .find_map(|field| field.strip_prefix("in="))
        .map(decode_hex_fixture)
        .expect("source input bytes");
    assert_eq!(
        fields.iter().find_map(|field| field.strip_prefix("type=")),
        Some("FileStreamBundleFile")
    );
    let expected_tree = fields
        .iter()
        .find_map(|field| field.strip_prefix("tree="))
        .expect("source inventory");
    assert_eq!(
            expected_tree,
            "CONTAINER:FileStreamBundleFile:ufs_v6_unknown_non_zstd_block.bin:res=0:failed=1,FAIL:data/file.bin"
        );

    let dispatched = SchemeReader::read(&input).expect("UnityFS input dispatch");
    assert_eq!(dispatched.kind(), FileKind::UnityFs);
    let super::ParsedFile::UnityFs(bundle) = dispatched.parse().unwrap() else {
        panic!("unknown-code fixture must parse as UnityFS");
    };
    let read_error = bundle.read_entry(0).expect_err("unknown non-Zstd codec");
    assert!(matches!(
        read_error.kind(),
        crate::container::unityfs::UnityFsErrorKind::UnsupportedCompression {
            compression: 6,
            context: "storage block"
        }
    ));

    let path = crate::test_support::unique_test_path("unknown-code-streaming", "bin");
    std::fs::write(&path, &input).expect("write source UnityFS bytes");
    let streamed = SchemeReader::read_path_streaming(&path, SchemeReaderLimits::default())
        .expect("file-backed unknown-code UnityFS dispatch");
    std::fs::remove_file(&path).expect("remove source UnityFS bytes");
    assert_eq!(streamed.kind(), FileKind::UnityFs);
    assert_eq!(streamed.wrappers(), dispatched.wrappers());
    let super::ParsedFile::UnityFs(streamed_bundle) = streamed.parse().unwrap() else {
        panic!("streamed unknown-code fixture must parse as UnityFS");
    };
    let streamed_error = streamed_bundle
        .read_entry(0)
        .expect_err("streamed unknown non-Zstd codec");
    assert!(matches!(
        streamed_error.kind(),
        crate::container::unityfs::UnityFsErrorKind::UnsupportedCompression {
            compression: 6,
            context: "storage block"
        }
    ));

    let mut filesystem = crate::io::filesystem::VirtualFileSystem::new();
    let mounted = streamed
        .mount_container_tree_with_source_name(
            &mut filesystem,
            Some("/unknown-codec"),
            Some("ufs_v6_unknown_non_zstd_block.bin"),
            super::ContainerMountLimits::default(),
        )
        .expect("unsupported entry is retained as a diagnostic");
    assert_eq!(mounted.mounted_entries, 0);
    assert_eq!(mounted.failed_entries.len(), 1);
    assert_eq!(mounted.failed_entries[0].source_name, "data/file.bin");
    assert_eq!(mounted.failed_entries[0].container_bundle_index, Some(0));
    assert!(matches!(
        mounted.failed_entries[0].error.kind(),
        crate::container::unityfs::UnityFsErrorKind::UnsupportedCompression {
            compression: 6,
            context: "storage block"
        }
    ));
    assert!(filesystem
        .read_all_bytes("/unknown-codec/data/file.bin", 32)
        .is_err());
}
