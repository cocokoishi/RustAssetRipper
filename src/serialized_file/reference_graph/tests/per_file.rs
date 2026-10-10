use super::walkers::{collect_managed_reference_graph, collect_references, WalkIssue};
use super::*;
use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
#[test]
fn walks_nested_structures_and_arrays_with_stable_field_paths() {
    let pointer = |path_id| SerializedValue::PPtr {
        file_id: 0,
        path_id,
    };
    let value = SerializedValue::Struct(vec![SerializedFieldValue {
        name: "m_Items".to_owned(),
        value: SerializedValue::Array(vec![SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Target".to_owned(),
            value: pointer(42),
        }])]),
    }]);
    let mut references = Vec::new();
    let mut issues = Vec::new();
    collect_references(
        &value,
        &mut String::new(),
        3,
        88,
        &mut references,
        &mut issues,
        ReferenceGraphLimits::default(),
    )
    .unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].source_object_index, 3);
    assert_eq!(references[0].source_path_id, 88);
    assert_eq!(references[0].field_path, "m_Items[0].m_Target");
    assert_eq!(references[0].file_id, 0);
    assert_eq!(references[0].path_id, 42);
    assert!(issues.is_empty());
}

#[test]
fn scans_managed_registry_cycles_and_nested_pptrs_from_decoded_object() {
    let fixture =
        crate::serialized_file::managed_reference_test_support::managed_reference_fixture();
    let payload = fixture
        .metadata
        .decode_object_payload(&fixture.file, &fixture.header, 0)
        .unwrap();

    let mut pptrs = Vec::new();
    let mut issues = Vec::new();
    collect_references(
        &payload.root,
        &mut String::new(),
        0,
        payload.path_id,
        &mut pptrs,
        &mut issues,
        ReferenceGraphLimits::default(),
    )
    .unwrap();
    assert!(issues.is_empty());
    assert_eq!(
        pptrs
            .iter()
            .map(|reference| (
                reference.field_path.as_str(),
                reference.file_id,
                reference.path_id,
            ))
            .collect::<Vec<_>>(),
        [
            ("references.RefIds[0].data.m_Asset", 1, 42),
            ("references.RefIds[1].data.m_Asset", 0, 300),
        ]
    );

    let managed = collect_managed_reference_graph(
        &payload.root,
        &mut String::new(),
        0,
        payload.path_id,
        ReferenceGraphLimits::default(),
    )
    .unwrap();
    assert_eq!(
        managed
            .objects
            .iter()
            .map(|object| (object.reference_id, object.class_name.as_str()))
            .collect::<Vec<_>>(),
        [(7, "NodeA"), (9, "NodeB")]
    );
    assert_eq!(
        managed
            .references
            .iter()
            .map(|reference| (
                reference.field_path.as_str(),
                reference.reference_id,
                reference.resolution.clone(),
            ))
            .collect::<Vec<_>>(),
        [
            (
                "m_First",
                7,
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 0,
                    registry_entry_index: 0,
                },
            ),
            (
                "m_Shared",
                7,
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 0,
                    registry_entry_index: 0,
                },
            ),
            (
                "m_Second",
                9,
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 1,
                    registry_entry_index: 1,
                },
            ),
            (
                "references.RefIds[0].data.m_Link",
                9,
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 1,
                    registry_entry_index: 1,
                },
            ),
            (
                "references.RefIds[1].data.m_Link",
                7,
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 0,
                    registry_entry_index: 0,
                },
            ),
        ]
    );

    let serialized_file = crate::serialized_file::file::SerializedFile::from_parts_for_test(
        &fixture.file,
        fixture.header.clone(),
        fixture.metadata.clone(),
    );
    let integrated = ObjectReferenceGraph::build(&serialized_file).unwrap();
    assert!(
        integrated.is_complete(),
        "issues: {:?}",
        integrated.issues()
    );
    assert_eq!(integrated.managed_objects().len(), 2);
    assert_eq!(integrated.managed_references().len(), 5);
    assert_eq!(integrated.references().len(), 2);
    assert_eq!(
        integrated
            .references()
            .iter()
            .map(|reference| reference.field_path.as_str())
            .collect::<Vec<_>>(),
        [
            "references.RefIds[0].data.m_Asset",
            "references.RefIds[1].data.m_Asset",
        ]
    );

    let legacy_fixture = crate::serialized_file::managed_reference_test_support::
            terminated_managed_reference_fixture();
    let legacy_payload = legacy_fixture
        .metadata
        .decode_object_payload(&legacy_fixture.file, &legacy_fixture.header, 0)
        .unwrap();
    let legacy = collect_managed_reference_graph(
        &legacy_payload.root,
        &mut String::new(),
        0,
        legacy_payload.path_id,
        ReferenceGraphLimits::default(),
    )
    .unwrap();
    assert_eq!(
        legacy
            .objects
            .iter()
            .map(|object| object.reference_id)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(
        legacy
            .references
            .iter()
            .map(|reference| (
                reference.reference_id,
                reference.indexed,
                reference.resolution.clone(),
            ))
            .collect::<Vec<_>>(),
        [
            (
                0,
                true,
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 0,
                    registry_entry_index: 0,
                },
            ),
            (
                0,
                true,
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 0,
                    registry_entry_index: 0,
                },
            ),
            (
                1,
                true,
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 1,
                    registry_entry_index: 1,
                },
            ),
            (
                1,
                true,
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 1,
                    registry_entry_index: 1,
                },
            ),
            (
                0,
                true,
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 0,
                    registry_entry_index: 0,
                },
            ),
        ]
    );
}

#[test]
fn nested_registry_decodes_writes_and_resolves_equal_ids_in_distinct_scopes() {
    let fixture = crate::serialized_file::managed_reference_test_support::
            nested_scope_managed_reference_fixture();
    let payload = fixture
        .metadata
        .decode_object_payload(&fixture.file, &fixture.header, 0)
        .expect("source-runtime-authored nested registry fixture decodes");
    assert_eq!(payload.consumed_bytes, 152);
    assert!(payload.trailing_bytes.is_empty());

    let encoded = fixture
        .metadata
        .encode_object_payload(
            &payload,
            super::super::object::ObjectEncodeLimits::default(),
        )
        .expect("nested registries encode recursively");
    let original = &fixture.file[fixture.header.data_offset as usize
        ..fixture.header.data_offset as usize + payload.consumed_bytes];
    assert_eq!(encoded, original, "nested managed payload bytes round-trip");

    let managed = collect_managed_reference_graph(
        &payload.root,
        &mut String::new(),
        0,
        payload.path_id,
        ReferenceGraphLimits::default(),
    )
    .expect("nested registry graph resolves");
    assert_eq!(
        managed
            .objects
            .iter()
            .map(|object| (
                object.registry_scope_id,
                object.registry_scope_path.as_deref(),
                object.registry_field_path.as_str(),
                object.registry_entry_index,
                object.reference_id,
                object.class_name.as_str(),
            ))
            .collect::<Vec<_>>(),
        [
            (Some(0), Some("references"), "references", 0, 100, "NodeA"),
            (
                Some(1),
                Some("references.RefIds[0].data.m_Nested.references"),
                "references.RefIds[0].data.m_Nested.references",
                0,
                100,
                "NodeB",
            ),
        ]
    );
    assert_eq!(
        managed
            .references
            .iter()
            .map(|reference| (
                reference.field_path.as_str(),
                reference.registry_scope_id,
                reference.resolution.clone(),
            ))
            .collect::<Vec<_>>(),
        [
            (
                "m_Root",
                Some(0),
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 0,
                    registry_entry_index: 0,
                },
            ),
            (
                "references.RefIds[0].data.m_Link",
                Some(0),
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 0,
                    registry_entry_index: 0,
                },
            ),
            (
                "references.RefIds[0].data.m_Nested.m_NestedLink",
                Some(1),
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 1,
                    managed_object_index: 1,
                    registry_entry_index: 0,
                },
            ),
            (
                "references.RefIds[0].data.m_Nested.references.RefIds[0].data.m_Link",
                Some(1),
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 1,
                    managed_object_index: 1,
                    registry_entry_index: 0,
                },
            ),
        ]
    );

    let depth_limited = fixture
        .metadata
        .decode_object_payload_with_limits(
            &fixture.file,
            &fixture.header,
            0,
            &crate::serialized_file::object::ObjectDecodeLimits {
                max_managed_registry_depth: 1,
                ..Default::default()
            },
        )
        .expect_err("the nested level exceeds a configured depth of one");
    let depth_message = depth_limited.to_string();
    assert!(
        depth_message.contains("depth 2") && depth_message.contains("limit 1"),
        "unexpected depth-bound diagnostic: {depth_message}"
    );

    let legacy_fixture = crate::serialized_file::managed_reference_test_support::
            nested_scope_terminated_managed_reference_fixture();
    let legacy_payload = legacy_fixture
        .metadata
        .decode_object_payload(&legacy_fixture.file, &legacy_fixture.header, 0)
        .expect("source-runtime-authored terminated nested registry fixture decodes");
    assert_eq!(legacy_payload.consumed_bytes, 200);
    let legacy_encoded = legacy_fixture
        .metadata
        .encode_object_payload(
            &legacy_payload,
            super::super::object::ObjectEncodeLimits::default(),
        )
        .expect("terminated nested registries encode recursively");
    let legacy_original = &legacy_fixture.file[legacy_fixture.header.data_offset as usize
        ..legacy_fixture.header.data_offset as usize + legacy_payload.consumed_bytes];
    assert_eq!(legacy_encoded, legacy_original);

    let legacy_graph = collect_managed_reference_graph(
        &legacy_payload.root,
        &mut String::new(),
        0,
        legacy_payload.path_id,
        ReferenceGraphLimits::default(),
    )
    .expect("legacy indexed IDs resolve in their nearest registry scopes");
    assert_eq!(
        legacy_graph
            .references
            .iter()
            .map(|reference| (
                reference.reference_id,
                reference.indexed,
                reference.registry_scope_id,
                reference.resolution.clone(),
            ))
            .collect::<Vec<_>>(),
        [
            (
                0,
                true,
                Some(0),
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 0,
                    registry_entry_index: 0,
                },
            ),
            (
                0,
                true,
                Some(0),
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 0,
                    managed_object_index: 0,
                    registry_entry_index: 0,
                },
            ),
            (
                0,
                true,
                Some(1),
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 1,
                    managed_object_index: 1,
                    registry_entry_index: 0,
                },
            ),
            (
                0,
                true,
                Some(1),
                super::SerializedManagedReferenceResolution::Object {
                    registry_scope_id: 1,
                    managed_object_index: 1,
                    registry_entry_index: 0,
                },
            ),
        ]
    );

    let scope_limited = collect_managed_reference_graph(
        &payload.root,
        &mut String::new(),
        0,
        payload.path_id,
        ReferenceGraphLimits {
            max_managed_registry_scopes: 1,
            ..ReferenceGraphLimits::default()
        },
    );
    assert!(matches!(
        scope_limited,
        Err(WalkIssue::ManagedRegistryScopeLimitExceeded { .. })
    ));
}

#[test]
fn edge_and_field_path_limits_are_explicit() {
    let pointer = SerializedValue::PPtr {
        file_id: 0,
        path_id: 42,
    };
    let mut references = Vec::new();
    let mut issues = Vec::new();
    assert!(matches!(
        collect_references(
            &pointer,
            &mut String::new(),
            0,
            1,
            &mut references,
            &mut issues,
            ReferenceGraphLimits {
                max_edges: 0,
                ..ReferenceGraphLimits::default()
            }
        ),
        Err(WalkIssue::EdgeLimitExceeded { .. })
    ));

    let named_pointer = SerializedValue::Struct(vec![SerializedFieldValue {
        name: "target".to_owned(),
        value: pointer,
    }]);
    assert!(matches!(
        collect_references(
            &named_pointer,
            &mut String::new(),
            0,
            1,
            &mut references,
            &mut issues,
            ReferenceGraphLimits {
                max_field_path_bytes: 3,
                ..ReferenceGraphLimits::default()
            }
        ),
        Err(WalkIssue::FieldPathLimitExceeded { .. })
    ));
}

#[test]
fn raw_type_tree_leaves_are_reported_instead_of_claimed_as_scanned() {
    let value = SerializedValue::Struct(vec![SerializedFieldValue {
        name: "m_Unknown".to_owned(),
        value: SerializedValue::Raw {
            type_name: "UnknownPointerShape".to_owned(),
            bytes: vec![1, 2, 3, 4],
        },
    }]);
    let mut references = Vec::new();
    let mut issues = Vec::new();
    collect_references(
        &value,
        &mut String::new(),
        2,
        77,
        &mut references,
        &mut issues,
        ReferenceGraphLimits::default(),
    )
    .unwrap();
    assert!(references.is_empty());
    assert_eq!(
        issues,
        [super::ReferenceGraphIssue::UnparsedRawValue {
            object_index: 2,
            path_id: 77,
            field_path: "m_Unknown".to_owned(),
            byte_count: 4,
        }]
    );
}

#[test]
fn empty_raw_type_tree_leaves_do_not_report_unscanned_reference_bytes() {
    let value = SerializedValue::Struct(vec![SerializedFieldValue {
        name: "m_Unknown".to_owned(),
        value: SerializedValue::Raw {
            type_name: "UnknownPointerShape".to_owned(),
            bytes: Vec::new(),
        },
    }]);
    let mut references = Vec::new();
    let mut issues = Vec::new();
    collect_references(
        &value,
        &mut String::new(),
        2,
        77,
        &mut references,
        &mut issues,
        ReferenceGraphLimits::default(),
    )
    .unwrap();
    assert!(references.is_empty());
    assert!(issues.is_empty());
}

/// File-level PPtr/external differential against the reference dump
/// committed in `pptr_probe_fixture.txt` (probe `Temp/opencode/pptrprobe`,
/// isolated reference bins, SDK 10.0.401). File A carries two objects
/// with external, local, local-back, and null PPtrs plus one external
/// table entry; file B is the external target. The per-file graphs must
/// expose the same non-null edges, the external table must match on GUID
/// words/type/path, and the multi-file join must resolve every edge
/// (null PPtrs are field values, not graph edges, on both sides).
#[test]
fn pptr_values_external_table_and_join_reproduce_the_reference_dump() {
    use crate::serialized_file::file::SerializedFile;

    fn decode_hex(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2), "odd hex length");
        hex.as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    let fixture = include_str!("../../pptr_probe_fixture.txt");
    let mut file_a = Vec::new();
    let mut file_b = Vec::new();
    let mut expected_edges: Vec<(i64, String, i32, i64)> = Vec::new();
    let mut expected_guid = String::new();
    for line in fixture.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(hex) = line.strip_prefix("SFAHEX|") {
            file_a = decode_hex(hex);
        } else if let Some(hex) = line.strip_prefix("SFBHEX|") {
            file_b = decode_hex(hex);
        } else if let Some(row) = line.strip_prefix("PPTR|") {
            let parts: Vec<&str> = row.split('|').collect();
            assert_eq!(parts.len(), 5, "bad PPTR {row}");
            let file_id: i32 = parts[3].parse().unwrap();
            let path_id: i64 = parts[4].parse().unwrap();
            expected_edges.push((
                parts[1].parse().unwrap(),
                parts[2].to_owned(),
                file_id,
                path_id,
            ));
        } else if let Some(row) = line.strip_prefix("PEXT|") {
            let parts: Vec<&str> = row.split('|').collect();
            assert_eq!(parts.len(), 5, "bad PEXT {row}");
            expected_guid = parts[2].to_owned();
            assert_eq!(parts[3], "Internal");
            assert_eq!(parts[4], "other.assets");
        } else {
            panic!("unknown fixture row {line}");
        }
    }
    assert!(!file_a.is_empty() && !file_b.is_empty());

    let parsed_a = SerializedFile::parse(&file_a).unwrap();
    let parsed_b = SerializedFile::parse(&file_b).unwrap();
    assert_eq!(parsed_a.metadata.objects.len(), 2);
    assert_eq!(parsed_b.metadata.objects.len(), 1);
    assert_eq!(parsed_b.metadata.objects[0].path_id, 100);

    let external = &parsed_a.metadata.externals;
    assert_eq!(external.len(), 1);
    assert_eq!(external[0].file_type, Some(0));
    assert_eq!(external[0].path_name_origin, "other.assets");
    let guid_words = external[0].guid.unwrap();
    let mut guid_bytes = Vec::new();
    for word in guid_words {
        guid_bytes.extend_from_slice(&word.to_le_bytes());
    }
    let guid_hex: String = guid_bytes.iter().map(|b| format!("{b:02X}")).collect();
    assert_eq!(guid_hex, expected_guid);

    let graph_a = ObjectReferenceGraph::build(&parsed_a).unwrap();
    let mut edges: Vec<(i64, String, i32, i64)> = graph_a
        .references()
        .iter()
        .map(|reference| {
            (
                reference.source_path_id,
                reference.field_path.clone(),
                reference.file_id,
                reference.path_id,
            )
        })
        .collect();
    edges.sort();
    expected_edges.sort();
    assert_eq!(edges, expected_edges);
    let graph_b = ObjectReferenceGraph::build(&parsed_b).unwrap();
    assert!(graph_b.references().is_empty());

    let files = [
        NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &parsed_a.metadata,
            graph: &graph_a,
        },
        NamedObjectReferenceGraph {
            file_name: "other.assets",
            metadata: &parsed_b.metadata,
            graph: &graph_b,
        },
    ];
    let joined = MultiFileReferenceGraph::build(&files).unwrap();
    assert_eq!(joined.file_count(), 2);
    let mut resolutions: Vec<(i64, String, i32, i64, usize, usize)> = Vec::new();
    let mut nulls = 0;
    for reference in joined.references() {
        match reference.resolution {
            MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index,
            } => resolutions.push((
                reference.source_path_id,
                reference.field_path.to_owned(),
                reference.file_id,
                reference.path_id,
                target_file_index,
                target_object_index,
            )),
            MultiFilePPtrResolution::Null => {
                assert_eq!(reference.file_id, 0);
                assert_eq!(reference.path_id, 0);
                nulls += 1;
            }
            ref other => panic!("unresolved {other:?}"),
        }
    }
    assert_eq!(nulls, 1);
    resolutions.sort();
    assert_eq!(
        resolutions,
        [
            (8001, "m_Friend".to_owned(), 0, 8002, 0, 1),
            (8001, "m_Target".to_owned(), 1, 100, 1, 0),
            (8002, "m_Friend".to_owned(), 0, 8001, 0, 0),
        ]
    );
    assert!(joined.is_complete());
}
