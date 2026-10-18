use super::support::{decode_hex_fixture, unityraw_fixture, webdata_fixture};
use super::*;
use crate::io::stream::SmartStream;
use crate::scheme::container_mount::mount_stream_container_tree;

#[test]
fn webdata_routes_frozen_nested_containers_through_the_common_stream_mount() {
    let fixture = include_str!("../../io/io_differential_fixture.txt");
    for case in [
        "raw_nested_unityfs",
        "web_nested_webdata",
        "ufs_nested_webdata",
    ] {
        let prefix = format!("SCHEME|{case}|");
        let row = fixture
            .lines()
            .find(|row| row.starts_with(&prefix))
            .unwrap();
        let child = row
            .split('|')
            .find_map(|field| field.strip_prefix("in="))
            .map(decode_hex_fixture)
            .unwrap();
        let scene_bytes = if case == "web_nested_webdata" {
            u64::from(
                crate::container::raw_web::RawWebBundle::parse(&child)
                    .unwrap()
                    .header
                    .scenes
                    .last()
                    .unwrap()
                    .decompressed_size,
            )
        } else {
            0
        };
        // Reuse independent frozen child bytes; only the enclosing WebData envelope is built
        // here. Raw/Web/UnityFS all previously took WebData's whole-child slice fallback.
        let input = webdata_fixture(&[("nested", &child)]);
        let mut legacy_filesystem = VirtualFileSystem::new();
        let expected = SchemeReader::read(&input)
            .unwrap()
            .mount_container_tree_with_source_name(
                &mut legacy_filesystem,
                Some("/tree"),
                Some("root.webdata"),
                ContainerMountLimits::default(),
            )
            .unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let source = SmartStream::from_reader(Box::new(Cursor::new(input))).unwrap();
        let actual = mount_stream_container_tree(
            &source,
            FileKind::UnityWebData,
            Some("root.webdata"),
            &mut filesystem,
            Some("/tree"),
            SchemeReaderLimits::default(),
            ContainerMountLimits {
                // All nested input ranges are shared. Only UnityWeb's one decoded scene
                // allocation needs a budget; its nested WebData ranges add zero bytes.
                max_active_nested_bytes: scene_bytes,
                ..ContainerMountLimits::default()
            },
        )
        .unwrap();
        assert_eq!(actual, expected, "source fixture {case} metadata/order");
        assert_eq!(
            actual.container_bundles.len(),
            3,
            "{case} container hierarchy"
        );
        drop(source);
        for resource in &actual.resource_files {
            assert_eq!(
                filesystem
                    .read_all_bytes(&resource.virtual_path, 4096)
                    .unwrap(),
                legacy_filesystem
                    .read_all_bytes(&resource.virtual_path, 4096)
                    .unwrap(),
                "{case}: {} after dropping the input stream",
                resource.name,
            );
        }
    }
}

#[test]
fn shared_nested_ranges_use_zero_budget_but_wrapper_backing_is_charged() {
    let leaf = webdata_fixture(&[("leaf.bin", b"leaf")]);
    let nested = webdata_fixture(&[("inner.webdata", &leaf)]);
    let input = webdata_fixture(&[("outer.webdata", &nested)]);
    let limits = ContainerMountLimits {
        max_active_nested_bytes: 0,
        ..ContainerMountLimits::default()
    };
    let mut filesystem = VirtualFileSystem::new();
    let source = SmartStream::from_reader(Box::new(Cursor::new(input))).unwrap();
    let report = mount_stream_container_tree(
        &source,
        FileKind::UnityWebData,
        None,
        &mut filesystem,
        Some("/tree"),
        SchemeReaderLimits::default(),
        limits,
    )
    .unwrap();
    assert_eq!(report.container_bundles.len(), 3);
    assert_eq!(report.mounted_entries, 1);
    drop(source);
    assert_eq!(
        filesystem
            .read_all_bytes("/tree/outer.webdata/inner.webdata/leaf.bin", 4)
            .unwrap(),
        b"leaf"
    );

    let wrapped = compress_gzip(&nested, 4096).unwrap();
    let input = webdata_fixture(&[("wrapped.gz", &wrapped)]);
    let source = SmartStream::from_reader(Box::new(Cursor::new(input))).unwrap();
    let mut filesystem = VirtualFileSystem::new();
    let error = mount_stream_container_tree(
        &source,
        FileKind::UnityWebData,
        None,
        &mut filesystem,
        Some("/tree"),
        SchemeReaderLimits::default(),
        limits,
    )
    .unwrap_err();
    assert!(matches!(error.kind,
        ContainerMountErrorKind::ActiveNestedContainerLimitExceeded {
            requested_total, limit: 0, entry_index: 0,
        } if requested_total == nested.len() as u64
    ));
    assert_eq!(error.partial_report.mounted_entries, 0);
}

#[test]
fn unityraw_routes_nested_webdata_through_the_common_stream_mount() {
    let payload = b"nested resource bytes";
    let nested = webdata_fixture(&[("leaf.resS", payload)]);
    let input = unityraw_fixture("nested.webdata", &nested);

    let mut legacy_filesystem = VirtualFileSystem::new();
    let expected = SchemeReader::read(&input)
        .unwrap()
        .mount_container_tree_with_source_name(
            &mut legacy_filesystem,
            Some("/tree"),
            Some("root.raw"),
            ContainerMountLimits::default(),
        )
        .unwrap();

    let mut filesystem = VirtualFileSystem::new();
    let source = SmartStream::from_reader(Box::new(Cursor::new(input))).unwrap();
    let actual = mount_stream_container_tree(
        &source,
        FileKind::UnityRaw,
        Some("root.raw"),
        &mut filesystem,
        Some("/tree"),
        SchemeReaderLimits::default(),
        ContainerMountLimits {
            max_active_nested_bytes: 0,
            ..ContainerMountLimits::default()
        },
    )
    .unwrap();

    assert_eq!(actual, expected);
    assert_eq!(actual.container_bundles.len(), 2);
    assert_eq!(actual.mounted_entries, 1);
    drop(source);
    assert_eq!(
        filesystem
            .read_all_bytes("/tree/nested.webdata/leaf.resS", payload.len())
            .unwrap(),
        payload,
    );
    assert_eq!(
        filesystem
            .read_all_bytes("/tree/nested.webdata/leaf.resS", payload.len())
            .unwrap(),
        legacy_filesystem
            .read_all_bytes("/tree/nested.webdata/leaf.resS", payload.len())
            .unwrap(),
    );
}
