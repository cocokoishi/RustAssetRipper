use super::support::*;
use super::*;

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
            .open_file(&format!(
                "/inputs/{}",
                path.file_name().unwrap().to_string_lossy()
            ))
            .unwrap()
            .stream_type(),
        crate::io::stream::SmartStreamType::File
    );
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

#[test]
fn source_webdata_entries_mount_file_backed_through_both_path_apis() {
    let cases = [
        (
            "webdata",
            include_str!("../../io/io_differential_fixture.txt"),
            "SCHEME|webdata|",
            "CONTAINER:WebFile:webdata.bin:res=2:failed=0,RES:inner.bin:77656221,RES:sub/deep.bin:64656570",
            vec![("inner.bin", b"web!".as_slice()), ("sub/deep.bin", b"deep".as_slice())],
        ),
        (
            "webdata-gzip-entry",
            include_str!("../../io/io_differential_stream_fixture.txt"),
            "SCHEME|webdata_gzip_entry|",
            "CONTAINER:WebFile:webdata_gzip_entry.bin:res=1:failed=0,RES:nested.gz:7765626461746120677A6970207061796C6F6164",
            vec![("nested.gz", b"webdata gzip payload".as_slice())],
        ),
    ];

    for (case_name, fixture, row_prefix, expected_tree, expected_entries) in cases {
        let row = fixture
            .lines()
            .find(|line| line.starts_with(row_prefix))
            .unwrap_or_else(|| panic!("source fixture has no {row_prefix:?} row"));
        let fields: Vec<&str> = row.split('|').collect();
        let input = fields
            .iter()
            .find_map(|field| field.strip_prefix("in="))
            .map(decode_hex_fixture)
            .expect("source WebData row includes exact input bytes");
        assert!(fields.contains(&format!("tree={expected_tree}").as_str()));

        let path = unique_temp_file_path("webdata");
        std::fs::write(&path, input).expect("write frozen source WebData bytes");

        let mut path_filesystem = VirtualFileSystem::new();
        let path_report = SchemeReader::mount_path(
            &path,
            &mut path_filesystem,
            Some("/webdata-path"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_or_else(|error| panic!("mount source {case_name} WebData by path: {error}"));
        assert_eq!(path_report.mounted_entries, expected_entries.len());
        assert_eq!(path_report.resource_files.len(), expected_entries.len());
        let expected_names = expected_entries
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        assert_eq!(
            path_report
                .resource_files
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            expected_names,
            "source {case_name} resource order is retained"
        );
        for (name, expected) in &expected_entries {
            let virtual_path = format!("/webdata-path/{name}");
            assert_eq!(
                path_filesystem
                    .open_file(&virtual_path)
                    .unwrap()
                    .stream_type(),
                crate::io::stream::SmartStreamType::File,
                "source {case_name} entry remains file-backed"
            );
            assert_eq!(
                path_filesystem
                    .read_all_bytes(&virtual_path, expected.len())
                    .unwrap(),
                *expected,
                "source {case_name} entry bytes match the reference inventory"
            );
        }

        let mut batch_filesystem = VirtualFileSystem::new();
        let batch_inputs = [PathMountInput {
            path: path.clone(),
            destination_root: Some("/webdata-batch".to_owned()),
        }];
        let batch_report = SchemeReader::mount_paths(
            &batch_inputs,
            &mut batch_filesystem,
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap_or_else(|error| panic!("mount source {case_name} WebData by batch path: {error}"));
        assert_eq!(batch_report.completed_inputs, 1);
        assert_eq!(batch_report.mounted_entries, expected_entries.len());
        assert_eq!(batch_report.container_trees.len(), 1);
        assert_eq!(
            batch_report.container_trees[0]
                .resource_files
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            expected_names,
            "batch source {case_name} resource order is retained"
        );
        for (name, expected) in &expected_entries {
            let virtual_path = format!("/webdata-batch/{name}");
            assert_eq!(
                batch_filesystem
                    .open_file(&virtual_path)
                    .unwrap()
                    .stream_type(),
                crate::io::stream::SmartStreamType::File,
                "batch source {case_name} entry remains file-backed"
            );
            assert_eq!(
                batch_filesystem
                    .read_all_bytes(&virtual_path, expected.len())
                    .unwrap(),
                *expected,
                "batch source {case_name} entry bytes match the reference inventory"
            );
        }

        std::fs::remove_file(&path).expect("mounted WebData entries no longer hold source file");
    }
}

#[test]
fn source_unityfs_nested_webdata_mounts_file_backed_through_both_path_apis() {
    let row = include_str!("../../io/io_differential_fixture.txt")
        .lines()
        .find(|line| line.starts_with("SCHEME|ufs_nested_webdata|"))
        .expect("frozen source UnityFS-with-WebData row");
    let fields: Vec<&str> = row.split('|').collect();
    let input = fields
        .iter()
        .find_map(|field| field.strip_prefix("in="))
        .map(decode_hex_fixture)
        .expect("source UnityFS row includes exact input bytes");
    assert!(fields.contains(&"tree=CONTAINER:FileStreamBundleFile:ufs_nested_webdata.bin:res=0:failed=0,CONTAINER:WebFile:inner.webdata:res=2:failed=0,RES:inner.bin:77656221,RES:sub/deep.bin:64656570"));
    let expected_entries = [
        ("inner.bin", b"web!".as_slice()),
        ("sub/deep.bin", b"deep".as_slice()),
    ];

    let directory = unique_temp_file_path("dir");
    std::fs::create_dir(&directory).expect("create source bundle directory");
    let path = directory.join("ufs_nested_webdata.bin");
    std::fs::write(&path, input).expect("write frozen source UnityFS bytes");

    let mut path_filesystem = VirtualFileSystem::new();
    let path_report = SchemeReader::mount_path(
        &path,
        &mut path_filesystem,
        Some("/ufs-webdata-path"),
        SchemeReaderLimits::default(),
        ContainerMountLimits::default(),
    )
    .expect("mount source UnityFS with nested WebData by path");
    assert_eq!(path_report.mounted_entries, expected_entries.len());
    assert!(path_report.failed_entries.is_empty());
    assert_eq!(path_report.container_bundles.len(), 2);
    assert_eq!(
        path_report.container_scopes,
        ["/ufs-webdata-path", "/ufs-webdata-path/inner.webdata"]
    );
    assert_eq!(
        path_report.container_bundles[0].name_fixed.as_deref(),
        Some("ufs_nested_webdata.bin")
    );
    assert_eq!(
        path_report.container_bundles[1].parent_bundle_index,
        Some(0)
    );
    assert_eq!(path_report.container_bundles[1].parent_entry_index, Some(0));
    assert_eq!(
        path_report.container_bundles[1].name_fixed.as_deref(),
        Some("inner.webdata")
    );
    assert_eq!(path_report.resource_files.len(), expected_entries.len());
    for (index, (name, expected)) in expected_entries.iter().enumerate() {
        let resource = &path_report.resource_files[index];
        assert_eq!(resource.name, *name);
        assert_eq!(resource.container_bundle_index, 1);
        assert_eq!(resource.entry_index, index);
        assert_eq!(resource.resource_index, index);
        let virtual_path = format!("/ufs-webdata-path/inner.webdata/{name}");
        assert_eq!(resource.virtual_path, virtual_path);
        assert_eq!(
            path_filesystem
                .open_file(&virtual_path)
                .unwrap()
                .stream_type(),
            crate::io::stream::SmartStreamType::File
        );
        assert_eq!(
            path_filesystem
                .read_all_bytes(&virtual_path, expected.len())
                .unwrap(),
            *expected
        );
    }

    let mut batch_filesystem = VirtualFileSystem::new();
    let inputs = [PathMountInput {
        path: path.clone(),
        destination_root: Some("/ufs-webdata-batch".to_owned()),
    }];
    let batch_report = SchemeReader::mount_paths(
        &inputs,
        &mut batch_filesystem,
        SchemeReaderLimits::default(),
        ContainerMountLimits::default(),
        BatchMountLimits::default(),
    )
    .expect("mount source UnityFS with nested WebData by batch path");
    assert_eq!(batch_report.completed_inputs, 1);
    assert_eq!(batch_report.mounted_entries, expected_entries.len());
    assert_eq!(batch_report.container_trees.len(), 1);
    assert!(batch_report.container_trees[0].failed_entries.is_empty());
    assert_eq!(batch_report.container_trees[0].container_bundles.len(), 2);
    assert_eq!(
        batch_report.container_trees[0].container_scopes,
        ["/ufs-webdata-batch", "/ufs-webdata-batch/inner.webdata"]
    );
    assert_eq!(
        batch_report.container_trees[0].resource_files.len(),
        expected_entries.len()
    );
    for (index, (name, _)) in expected_entries.iter().enumerate() {
        let resource = &batch_report.container_trees[0].resource_files[index];
        assert_eq!(resource.name, *name);
        assert_eq!(resource.container_bundle_index, 1);
        assert_eq!(resource.resource_index, index);
    }
    for (name, expected) in expected_entries {
        let virtual_path = format!("/ufs-webdata-batch/inner.webdata/{name}");
        assert_eq!(
            batch_filesystem
                .open_file(&virtual_path)
                .unwrap()
                .stream_type(),
            crate::io::stream::SmartStreamType::File
        );
        assert_eq!(
            batch_filesystem
                .read_all_bytes(&virtual_path, expected.len())
                .unwrap(),
            expected
        );
    }

    std::fs::remove_file(&path).expect("mounted nested WebData no longer holds the input file");
    std::fs::remove_dir(&directory).expect("remove source bundle directory");
}

#[test]
fn source_unityweb_nested_webdata_mounts_file_backed_through_both_path_apis() {
    let row = include_str!("../../io/io_differential_fixture.txt")
        .lines()
        .find(|line| line.starts_with("SCHEME|web_nested_webdata|"))
        .expect("frozen source UnityWeb-with-WebData row");
    let fields: Vec<&str> = row.split('|').collect();
    let input = fields
        .iter()
        .find_map(|field| field.strip_prefix("in="))
        .map(decode_hex_fixture)
        .expect("source UnityWeb row includes exact input bytes");
    assert!(fields.contains(&"tree=CONTAINER:WebBundleFile:web_nested_webdata.bin:res=0:failed=0,CONTAINER:WebFile:nested.webdata:res=2:failed=0,RES:inner.bin:77656221,RES:sub/deep.bin:64656570"));
    let expected_entries = [
        ("inner.bin", b"web!".as_slice()),
        ("sub/deep.bin", b"deep".as_slice()),
    ];

    let directory = unique_temp_file_path("dir");
    std::fs::create_dir(&directory).expect("create source bundle directory");
    let path = directory.join("web_nested_webdata.bin");
    std::fs::write(&path, input).expect("write frozen source UnityWeb bytes");

    let mut path_filesystem = VirtualFileSystem::new();
    let path_report = SchemeReader::mount_path(
        &path,
        &mut path_filesystem,
        Some("/web-webdata-path"),
        SchemeReaderLimits::default(),
        ContainerMountLimits::default(),
    )
    .expect("mount source UnityWeb with nested WebData by path");
    assert_eq!(path_report.mounted_entries, expected_entries.len());
    assert!(path_report.failed_entries.is_empty());
    assert_eq!(path_report.container_bundles.len(), 2);
    assert_eq!(
        path_report.container_scopes,
        ["/web-webdata-path", "/web-webdata-path/nested.webdata"]
    );
    assert_eq!(
        path_report.container_bundles[0].name_fixed.as_deref(),
        Some("web_nested_webdata.bin")
    );
    assert_eq!(
        path_report.container_bundles[1].parent_bundle_index,
        Some(0)
    );
    assert_eq!(path_report.container_bundles[1].parent_entry_index, Some(0));
    assert_eq!(
        path_report.container_bundles[1].name_fixed.as_deref(),
        Some("nested.webdata")
    );
    assert_eq!(path_report.resource_files.len(), expected_entries.len());
    for (index, (name, expected)) in expected_entries.iter().enumerate() {
        let resource = &path_report.resource_files[index];
        assert_eq!(resource.name, *name);
        assert_eq!(resource.container_bundle_index, 1);
        assert_eq!(resource.entry_index, index);
        assert_eq!(resource.resource_index, index);
        let virtual_path = format!("/web-webdata-path/nested.webdata/{name}");
        assert_eq!(resource.virtual_path, virtual_path);
        assert_eq!(
            path_filesystem
                .open_file(&virtual_path)
                .unwrap()
                .stream_type(),
            crate::io::stream::SmartStreamType::File
        );
        assert_eq!(
            path_filesystem
                .read_all_bytes(&virtual_path, expected.len())
                .unwrap(),
            *expected
        );
    }

    let mut batch_filesystem = VirtualFileSystem::new();
    let inputs = [PathMountInput {
        path: path.clone(),
        destination_root: Some("/web-webdata-batch".to_owned()),
    }];
    let batch_report = SchemeReader::mount_paths(
        &inputs,
        &mut batch_filesystem,
        SchemeReaderLimits::default(),
        ContainerMountLimits::default(),
        BatchMountLimits::default(),
    )
    .expect("mount source UnityWeb with nested WebData by batch path");
    assert_eq!(batch_report.completed_inputs, 1);
    assert_eq!(batch_report.mounted_entries, expected_entries.len());
    assert_eq!(batch_report.container_trees.len(), 1);
    assert!(batch_report.container_trees[0].failed_entries.is_empty());
    assert_eq!(batch_report.container_trees[0].container_bundles.len(), 2);
    assert_eq!(
        batch_report.container_trees[0].container_scopes,
        ["/web-webdata-batch", "/web-webdata-batch/nested.webdata"]
    );
    assert_eq!(
        batch_report.container_trees[0].resource_files.len(),
        expected_entries.len()
    );
    for (index, (name, _)) in expected_entries.iter().enumerate() {
        let resource = &batch_report.container_trees[0].resource_files[index];
        assert_eq!(resource.name, *name);
        assert_eq!(resource.container_bundle_index, 1);
        assert_eq!(resource.resource_index, index);
    }
    for (name, expected) in expected_entries {
        let virtual_path = format!("/web-webdata-batch/nested.webdata/{name}");
        assert_eq!(
            batch_filesystem
                .open_file(&virtual_path)
                .unwrap()
                .stream_type(),
            crate::io::stream::SmartStreamType::File
        );
        assert_eq!(
            batch_filesystem
                .read_all_bytes(&virtual_path, expected.len())
                .unwrap(),
            expected
        );
    }

    std::fs::remove_file(&path).expect("mounted nested WebData no longer holds the input file");
    std::fs::remove_dir(&directory).expect("remove source bundle directory");
}

#[test]
fn source_unityraw_nested_unityfs_mounts_file_backed_through_both_path_apis() {
    let row = include_str!("../../io/io_differential_stream_fixture.txt")
        .lines()
        .find(|line| line.starts_with("SCHEME|raw_nested_unityfs|"))
        .expect("frozen source UnityRaw-with-UnityFS row");
    let fields: Vec<&str> = row.split('|').collect();
    let input = fields
        .iter()
        .find_map(|field| field.strip_prefix("in="))
        .map(decode_hex_fixture)
        .expect("source UnityRaw row includes exact input bytes");
    assert!(fields.contains(&"tree=CONTAINER:RawBundleFile:raw_nested_unityfs.bin:res=0:failed=0,CONTAINER:FileStreamBundleFile:nested.bundle:res=1:failed=0,RES:data/file.bin:30313233343536373839"));
    let expected_payload = b"0123456789";

    let directory = unique_temp_file_path("dir");
    std::fs::create_dir(&directory).expect("create source bundle directory");
    let path = directory.join("raw_nested_unityfs.bin");
    std::fs::write(&path, input).expect("write frozen source UnityRaw bytes");

    let mut path_filesystem = VirtualFileSystem::new();
    let path_report = SchemeReader::mount_path(
        &path,
        &mut path_filesystem,
        Some("/raw-unityfs-path"),
        SchemeReaderLimits::default(),
        ContainerMountLimits::default(),
    )
    .expect("mount source UnityRaw with nested UnityFS by path");
    assert_eq!(path_report.mounted_entries, 1);
    assert!(path_report.failed_entries.is_empty());
    assert_eq!(path_report.container_bundles.len(), 2);
    assert_eq!(
        path_report.container_scopes,
        ["/raw-unityfs-path", "/raw-unityfs-path/nested.bundle"]
    );
    assert_eq!(
        path_report.container_bundles[0].name_fixed.as_deref(),
        Some("raw_nested_unityfs.bin")
    );
    assert_eq!(
        path_report.container_bundles[1].parent_bundle_index,
        Some(0)
    );
    assert_eq!(path_report.container_bundles[1].parent_entry_index, Some(0));
    assert_eq!(
        path_report.container_bundles[1].name_fixed.as_deref(),
        Some("nested.bundle")
    );
    assert_eq!(path_report.resource_files.len(), 1);
    let path_resource = &path_report.resource_files[0];
    assert_eq!(path_resource.name, "data/file.bin");
    assert_eq!(path_resource.container_bundle_index, 1);
    assert_eq!(path_resource.entry_index, 0);
    assert_eq!(path_resource.resource_index, 0);
    let path_output = "/raw-unityfs-path/nested.bundle/data/file.bin";
    assert_eq!(path_resource.virtual_path, path_output);
    assert_eq!(
        path_filesystem
            .open_file(path_output)
            .unwrap()
            .stream_type(),
        crate::io::stream::SmartStreamType::File
    );
    assert_eq!(
        path_filesystem
            .read_all_bytes(path_output, expected_payload.len())
            .unwrap(),
        expected_payload
    );

    let mut batch_filesystem = VirtualFileSystem::new();
    let inputs = [PathMountInput {
        path: path.clone(),
        destination_root: Some("/raw-unityfs-batch".to_owned()),
    }];
    let batch_report = SchemeReader::mount_paths(
        &inputs,
        &mut batch_filesystem,
        SchemeReaderLimits::default(),
        ContainerMountLimits::default(),
        BatchMountLimits::default(),
    )
    .expect("mount source UnityRaw with nested UnityFS by batch path");
    assert_eq!(batch_report.completed_inputs, 1);
    assert_eq!(batch_report.mounted_entries, 1);
    assert_eq!(batch_report.container_trees.len(), 1);
    assert!(batch_report.container_trees[0].failed_entries.is_empty());
    assert_eq!(batch_report.container_trees[0].container_bundles.len(), 2);
    assert_eq!(
        batch_report.container_trees[0].container_scopes,
        ["/raw-unityfs-batch", "/raw-unityfs-batch/nested.bundle"]
    );
    assert_eq!(batch_report.container_trees[0].resource_files.len(), 1);
    let batch_resource = &batch_report.container_trees[0].resource_files[0];
    assert_eq!(batch_resource.name, "data/file.bin");
    assert_eq!(batch_resource.container_bundle_index, 1);
    assert_eq!(batch_resource.resource_index, 0);
    let batch_output = "/raw-unityfs-batch/nested.bundle/data/file.bin";
    assert_eq!(batch_resource.virtual_path, batch_output);
    assert_eq!(
        batch_filesystem
            .open_file(batch_output)
            .unwrap()
            .stream_type(),
        crate::io::stream::SmartStreamType::File
    );
    assert_eq!(
        batch_filesystem
            .read_all_bytes(batch_output, expected_payload.len())
            .unwrap(),
        expected_payload
    );

    std::fs::remove_file(&path).expect("mounted nested UnityFS no longer holds the input file");
    std::fs::remove_dir(&directory).expect("remove source bundle directory");
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
        include_str!("../singlefile_probe_fixture.txt")
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
    for (variant, entry_prefix) in [("aligned", "ALIGNED_ENTRY|"), ("noalign", "NOALIGN_ENTRY|")] {
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
            .open_file(&resource_virtual_path)
            .unwrap()
            .stream_type(),
        crate::io::stream::SmartStreamType::File
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
