use super::test_support::*;
use super::*;
use crate::processing::asset_bundle::{
    map_asset_bundle_container, resolve_asset_bundle_container_reference,
    AssetBundleContainerTarget,
};
use crate::processing::scene_collections::{
    AssetBundleContainerReference, SceneBundleMappingLimits,
};
use crate::serialized_file::metadata::{
    SerializedFileMetadata, SerializedType, TypeTree, TypeTreeNode,
};
use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
use crate::serialized_file::references::PPtrResolution;
#[test]
fn external_file_ids_address_table_slots_like_try_get_dependency() {
    use super::external_table_index;
    // File 1 addresses slot 0; higher files shift by one like Dependencies.
    assert_eq!(external_table_index(1, 2), Some(0));
    assert_eq!(external_table_index(2, 2), Some(1));
    // File 0 is the local file (handled before the gate); negative,
    // shifted, and past-the-table IDs have no slot.
    assert_eq!(external_table_index(0, 2), None);
    assert_eq!(external_table_index(-1, 2), None);
    assert_eq!(external_table_index(i32::MIN, 2), None);
    assert_eq!(external_table_index(3, 2), None);
    assert_eq!(external_table_index(1, 0), None);
    assert_eq!(external_table_index(i32::MAX, 0), None);
}

#[test]
fn asset_bundle_container_pptrs_resolve_in_the_owning_file_scope() {
    let owner_metadata = metadata(&[10, 20]);
    let graph = empty_graph(&[10, 20]);
    let reference = |file_id, path_id| AssetBundleContainerReference {
        path: "Assets/entry.asset".to_owned(),
        file_id,
        path_id,
    };
    assert_eq!(
        resolve_asset_bundle_container_reference(&reference(0, 20), &owner_metadata, &graph),
        AssetBundleContainerTarget::LocalObject { object_index: 1 }
    );
    assert_eq!(
        resolve_asset_bundle_container_reference(&reference(0, 0), &owner_metadata, &graph),
        AssetBundleContainerTarget::Null
    );
    assert_eq!(
        resolve_asset_bundle_container_reference(&reference(2, 99), &owner_metadata, &graph),
        AssetBundleContainerTarget::SharedFile {
            file_id: 2,
            external_index: 1,
            path_id: 99,
        }
    );
    assert_eq!(
        resolve_asset_bundle_container_reference(&reference(0, 99), &owner_metadata, &graph),
        AssetBundleContainerTarget::MissingLocalObject { path_id: 99 }
    );
    assert_eq!(
        resolve_asset_bundle_container_reference(
            &reference(0, 10),
            &metadata(&[10, 10]),
            &empty_graph(&[10, 10]),
        ),
        AssetBundleContainerTarget::AmbiguousLocalObject {
            path_id: 10,
            match_count: 2,
        }
    );
    assert_eq!(
        resolve_asset_bundle_container_reference(&reference(3, 99), &owner_metadata, &graph),
        AssetBundleContainerTarget::InvalidFileId {
            file_id: 3,
            external_count: 2,
        }
    );
}

#[test]
fn asset_bundle_container_map_applies_name_rules_and_preserves_target_kinds() {
    let metadata = metadata(&[10, 20]);
    let graph = empty_graph(&[10, 20]);
    let container_entry = |path: &str, file_id, path_id| {
        SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "first".to_owned(),
                value: SerializedValue::StringBytes(path.as_bytes().to_vec()),
            },
            SerializedFieldValue {
                name: "second".to_owned(),
                value: SerializedValue::Struct(vec![SerializedFieldValue {
                    name: "asset".to_owned(),
                    value: SerializedValue::PPtr { file_id, path_id },
                }]),
            },
        ])
    };
    let root = SerializedValue::Struct(vec![
        SerializedFieldValue {
            name: "m_AssetBundleName_R".to_owned(),
            value: SerializedValue::StringBytes(b"character.bundle".to_vec()),
        },
        SerializedFieldValue {
            name: "m_Container".to_owned(),
            value: SerializedValue::Array(vec![
                container_entry("Assets/local.asset", 0, 20),
                container_entry("Assets/shared.asset", 2, 99),
                container_entry("Assets/null.asset", 0, 0),
            ]),
        },
    ]);
    let mapping = map_asset_bundle_container(
        &root,
        "source.bundle",
        &metadata,
        &graph,
        SceneBundleMappingLimits::default(),
    )
    .unwrap();

    assert_eq!(mapping.source_name, "character.bundle");
    assert_eq!(mapping.asset_name, "character");
    assert_eq!(
        mapping
            .references
            .iter()
            .map(|entry| entry.target.clone())
            .collect::<Vec<_>>(),
        [
            AssetBundleContainerTarget::LocalObject { object_index: 1 },
            AssetBundleContainerTarget::SharedFile {
                file_id: 2,
                external_index: 1,
                path_id: 99,
            },
            AssetBundleContainerTarget::Null,
        ]
    );
}

fn graph_with_reference(
    path_ids: &[i64],
    source_path_id: i64,
    file_id: i32,
    path_id: i64,
) -> ObjectReferenceGraph {
    let mut graph = empty_graph(path_ids);
    graph.references.push(SerializedObjectReference {
        source_object_index: 0,
        source_path_id,
        field_path: "m_Target".to_owned(),
        file_id,
        path_id,
    });
    graph
}

#[test]
fn multi_file_reference_graph_exposes_an_objects_resolved_type_tree() {
    let type_tree = TypeTree {
        nodes: vec![TypeTreeNode {
            level: 0,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some("Transform".to_owned()),
            name: Some(String::new()),
            byte_size: -1,
            index: 0,
            meta_flags: 0,
            reference_type_hash: None,
        }],
        string_buffer: Vec::new(),
    };
    let mut source_metadata = metadata(&[10]);
    source_metadata.types.push(SerializedType {
        raw_type_id: 4,
        type_id_for_hash: 4,
        is_stripped_type: false,
        script_type_index: -1,
        script_id: None,
        old_type_hash: None,
        type_tree: Some(type_tree.clone()),
        type_dependencies: Vec::new(),
    });
    source_metadata.objects[0].resolved_type_index = Some(0);
    let source_graph = empty_graph(&[10]);
    let files = [NamedObjectReferenceGraph {
        file_name: "scene.assets",
        metadata: &source_metadata,
        graph: &source_graph,
    }];
    let references = MultiFileReferenceGraph::build(&files).unwrap();

    assert_eq!(references.object_type_tree(0, 0), Some(&type_tree));
    assert_eq!(references.object_type_tree(0, 1), None);
    assert_eq!(references.object_type_tree(1, 0), None);
}

#[test]
fn indexes_duplicate_ids_and_classifies_local_external_and_null_edges() {
    let metadata = metadata(&[101, 202, 101]);
    let graph = empty_graph(&[101, 202, 101]);
    assert_eq!(
        graph.object_indices_for_path_id(101).collect::<Vec<_>>(),
        [0, 2]
    );
    assert_eq!(
        graph.object_indices_for_path_id(202).collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(graph.object_indices_for_path_id(303).count(), 0);
    assert_eq!(
        graph.resolve_pointer(&metadata, 0, 202),
        PPtrResolution::LocalObject { object_index: 1 }
    );
    assert_eq!(
        graph.resolve_pointer(&metadata, 0, 101),
        PPtrResolution::AmbiguousLocalObject {
            path_id: 101,
            match_count: 2
        }
    );
    assert_eq!(
        graph.resolve_pointer(&metadata, 0, 303),
        PPtrResolution::MissingLocalObject { path_id: 303 }
    );
    assert_eq!(graph.resolve_pointer(&metadata, 2, 0), PPtrResolution::Null);
    assert_eq!(
        graph.resolve_pointer(&metadata, 2, 404),
        PPtrResolution::ExternalDependency {
            file_id: 2,
            external_index: 1,
            path_id: 404,
            identifier: &metadata.externals[1],
        }
    );
    assert_eq!(
        graph.resolve_pointer(&metadata, -1, 404),
        PPtrResolution::InvalidFileId {
            file_id: -1,
            external_count: 2,
        }
    );
}

#[test]
fn joins_unique_external_file_names_and_resolves_target_path_ids() {
    let source_metadata = metadata(&[11]);
    let source_graph = graph_with_reference(&[11], 11, 1, 22);
    let target_metadata = metadata(&[22]);
    let target_graph = empty_graph(&[22]);
    let files = [
        NamedObjectReferenceGraph {
            file_name: "root/main.assets",
            metadata: &source_metadata,
            graph: &source_graph,
        },
        NamedObjectReferenceGraph {
            file_name: "SHARED.ASSETS",
            metadata: &target_metadata,
            graph: &target_graph,
        },
    ];

    let graph = MultiFileReferenceGraph::build(&files).unwrap();
    assert_eq!(graph.file_count(), 2);
    assert_eq!(graph.normalized_file_names()[1], "shared.assets");
    assert_eq!(
        source_graph
            .references_from_object(0)
            .map(|reference| reference.field_path.as_str())
            .collect::<Vec<_>>(),
        ["m_Target"]
    );
    assert_eq!(source_graph.references_from_object(1).count(), 0);
    assert_eq!(
        graph
            .candidate_file_indices("library/Shared.Assets")
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(
        graph.references(),
        [MultiFileObjectReference {
            source_file_index: 0,
            source_object_index: 0,
            source_path_id: 11,
            field_path: "m_Target",
            file_id: 1,
            path_id: 22,
            resolution: MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            },
        }]
    );
    assert!(graph.is_complete());
}

#[test]
fn exposes_file_scoped_object_identity_and_incoming_outgoing_edges() {
    let source_metadata = metadata(&[11, 12, 13]);
    let mut source_graph = empty_graph(&[11, 12, 13]);
    source_graph.references = vec![
        SerializedObjectReference {
            source_object_index: 0,
            source_path_id: 11,
            field_path: "m_External".to_owned(),
            file_id: 1,
            path_id: 22,
        },
        SerializedObjectReference {
            source_object_index: 1,
            source_path_id: 12,
            field_path: "m_Local".to_owned(),
            file_id: 0,
            path_id: 13,
        },
        SerializedObjectReference {
            source_object_index: 2,
            source_path_id: 13,
            field_path: "m_External".to_owned(),
            file_id: 1,
            path_id: 22,
        },
    ];
    let target_metadata = metadata(&[22]);
    let target_graph = empty_graph(&[22]);
    let files = [
        NamedObjectReferenceGraph {
            file_name: "root/main.assets",
            metadata: &source_metadata,
            graph: &source_graph,
        },
        NamedObjectReferenceGraph {
            file_name: "SHARED.ASSETS",
            metadata: &target_metadata,
            graph: &target_graph,
        },
    ];

    let graph = MultiFileReferenceGraph::build(&files).unwrap();
    assert_eq!(
        graph.object_identity(1, 0),
        Some(MultiFileObjectIdentity {
            file_index: 1,
            normalized_file_name: "shared.assets",
            object_index: 0,
            path_id: 22,
        })
    );
    assert_eq!(graph.object_identity(1, 1), None);
    assert_eq!(
        graph
            .references_from_object(0, 1)
            .map(|reference| reference.field_path)
            .collect::<Vec<_>>(),
        ["m_Local"]
    );
    assert_eq!(
        graph
            .references_to_object(1, 0)
            .map(|reference| (reference.source_file_index, reference.source_object_index))
            .collect::<Vec<_>>(),
        [(0, 0), (0, 2)]
    );
    assert_eq!(graph.references_from_object(4, 0).count(), 0);
}

#[test]
fn missing_dependencies_report_each_unresolved_identifier_once() {
    let source_metadata = metadata(&[11]);
    let mut source_graph = graph_with_reference(&[11], 11, 1, 22);
    // A second PPtr shares external slot 0; a third uses slot 1.
    source_graph.references.push(SerializedObjectReference {
        source_object_index: 0,
        source_path_id: 11,
        field_path: "m_Other".to_owned(),
        file_id: 1,
        path_id: 23,
    });
    source_graph.references.push(SerializedObjectReference {
        source_object_index: 0,
        source_path_id: 11,
        field_path: "m_Third".to_owned(),
        file_id: 2,
        path_id: 24,
    });
    let unrelated_metadata = metadata(&[22]);
    let unrelated_graph = empty_graph(&[22]);
    let files = [
        NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &source_metadata,
            graph: &source_graph,
        },
        NamedObjectReferenceGraph {
            file_name: "different.assets",
            metadata: &unrelated_metadata,
            graph: &unrelated_graph,
        },
    ];
    let graph = MultiFileReferenceGraph::build(&files).unwrap();
    // Slot 0 is shared by two references but reported once, in identifier order.
    assert_eq!(graph.missing_dependency_indexes(0), [0, 1]);
    assert_eq!(
        graph.dependency_identifier(0, 0).unwrap().fixed_path_name(),
        "shared.assets"
    );
    assert_eq!(
        graph.dependency_identifier(0, 1).unwrap().fixed_path_name(),
        "scene.assets"
    );
    // Files without missing dependencies report nothing; neither do unknown files.
    assert_eq!(graph.missing_dependency_indexes(1), Vec::<usize>::new());
    assert_eq!(graph.missing_dependency_indexes(7), Vec::<usize>::new());
}

#[test]
fn missing_and_ambiguous_external_files_remain_explicit() {
    let source_metadata = metadata(&[11]);
    let source_graph = graph_with_reference(&[11], 11, 1, 22);
    let unrelated_metadata = metadata(&[22]);
    let unrelated_graph = empty_graph(&[22]);
    let missing_files = [
        NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &source_metadata,
            graph: &source_graph,
        },
        NamedObjectReferenceGraph {
            file_name: "different.assets",
            metadata: &unrelated_metadata,
            graph: &unrelated_graph,
        },
    ];
    let missing = MultiFileReferenceGraph::build(&missing_files).unwrap();
    assert_eq!(
        missing.references()[0].resolution,
        MultiFilePPtrResolution::MissingDependency {
            external_index: 0,
            path_id: 22,
        }
    );
    assert_eq!(
        missing
            .dependency_identifier(0, 0)
            .unwrap()
            .fixed_path_name(),
        "shared.assets"
    );
    assert!(!missing.is_complete());

    let duplicate_files = [
        NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &source_metadata,
            graph: &source_graph,
        },
        NamedObjectReferenceGraph {
            file_name: "shared.assets",
            metadata: &unrelated_metadata,
            graph: &unrelated_graph,
        },
        NamedObjectReferenceGraph {
            file_name: "LIBRARY/SHARED.ASSETS",
            metadata: &unrelated_metadata,
            graph: &unrelated_graph,
        },
    ];
    let ambiguous = MultiFileReferenceGraph::build(&duplicate_files).unwrap();
    assert_eq!(
        ambiguous.references()[0].resolution,
        MultiFilePPtrResolution::AmbiguousDependency {
            external_index: 0,
            path_id: 22,
            candidate_count: 2,
        }
    );
    assert_eq!(
        ambiguous
            .candidate_file_indices("shared.assets")
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(!ambiguous.is_complete());
}

fn metadata_with_external(path_ids: &[i64], external_path: &str) -> SerializedFileMetadata {
    let mut source_metadata = metadata(path_ids);
    source_metadata.externals = vec![external(external_path)];
    source_metadata
}

fn named_file<'a>(
    file_name: &'a str,
    metadata: &'a SerializedFileMetadata,
    graph: &'a ObjectReferenceGraph,
) -> NamedObjectReferenceGraph<'a> {
    NamedObjectReferenceGraph {
        file_name,
        metadata,
        graph,
    }
}

#[test]
fn engine_resource_alias_fallback_matches_the_bundle_resolver() {
    // The alias table itself mirrors `Bundle.ResolveCollection` exactly.
    assert_eq!(
        super::multi_file::engine_resource_alias("unity default resources"),
        Some("unity_default_resources")
    );
    assert_eq!(
        super::multi_file::engine_resource_alias("unity_default_resources"),
        Some("unity default resources")
    );
    assert_eq!(
        super::multi_file::engine_resource_alias("unity builtin extra"),
        Some("unity_builtin_extra")
    );
    assert_eq!(
        super::multi_file::engine_resource_alias("unity_builtin_extra"),
        Some("unity builtin extra")
    );
    assert_eq!(
        super::multi_file::engine_resource_alias("shared.assets"),
        None
    );

    // `library/unity default resources` resolves into a loaded
    // `unity_default_resources` file the exact-name join would miss.
    let source_metadata = metadata_with_external(&[11], "library/unity default resources");
    let source_graph = graph_with_reference(&[11], 11, 1, 22);
    let target_metadata = metadata(&[22]);
    let target_graph = empty_graph(&[22]);
    let files = [
        named_file("main.assets", &source_metadata, &source_graph),
        named_file("unity_default_resources", &target_metadata, &target_graph),
    ];
    let graph = MultiFileReferenceGraph::build(&files).unwrap();
    assert_eq!(
        graph.references()[0].resolution,
        MultiFilePPtrResolution::Object {
            target_file_index: 1,
            target_object_index: 0,
        }
    );
    assert!(graph.is_complete());

    // The reverse spelling and the builtin-extra pair behave the same way.
    let source_metadata = metadata_with_external(&[11], "resources/unity builtin extra");
    let source_graph = graph_with_reference(&[11], 11, 1, 22);
    let files = [
        named_file("main.assets", &source_metadata, &source_graph),
        named_file(
            "Library/unity_builtin_extra",
            &target_metadata,
            &target_graph,
        ),
    ];
    let graph = MultiFileReferenceGraph::build(&files).unwrap();
    assert_eq!(
        graph.references()[0].resolution,
        MultiFilePPtrResolution::Object {
            target_file_index: 1,
            target_object_index: 0,
        }
    );

    // An exact-name hit wins over the alias: the alias file is not consulted.
    let exact_metadata = metadata(&[99]);
    let exact_graph = empty_graph(&[99]);
    let source_metadata = metadata_with_external(&[11], "library/unity default resources");
    let source_graph = graph_with_reference(&[11], 11, 1, 22);
    let files = [
        named_file("main.assets", &source_metadata, &source_graph),
        named_file("unity default resources", &exact_metadata, &exact_graph),
        named_file("unity_default_resources", &target_metadata, &target_graph),
    ];
    let graph = MultiFileReferenceGraph::build(&files).unwrap();
    assert_eq!(
        graph.references()[0].resolution,
        MultiFilePPtrResolution::MissingExternalObject {
            target_file_index: 1,
            path_id: 22,
        }
    );

    // Duplicate alias spellings stay ambiguous instead of merging.
    let files = [
        named_file("main.assets", &source_metadata, &source_graph),
        named_file("unity_default_resources", &target_metadata, &target_graph),
        named_file(
            "ARCHIVE:/UNITY_DEFAULT_RESOURCES",
            &target_metadata,
            &target_graph,
        ),
    ];
    let graph = MultiFileReferenceGraph::build(&files).unwrap();
    assert_eq!(
        graph.references()[0].resolution,
        MultiFilePPtrResolution::AmbiguousDependency {
            external_index: 0,
            path_id: 22,
            candidate_count: 2,
        }
    );
    assert!(!graph.is_complete());
}

#[test]
fn absent_2022_3_default_resources_resolve_only_known_tpk_path_ids() {
    let source_graph = graph_with_reference(&[11], 11, 1, 10202);
    for version in [
        "2022.3.0f1",
        "2022.3.6f1",
        "2022.3.10f1",
        "2022.3.11f1",
        "2022.3.22f1",
    ] {
        let mut source_metadata = metadata_with_external(&[11], "Library/unity default resources");
        source_metadata.prefix.unity_version_text = Some(version.to_owned());
        let files = [named_file("main.assets", &source_metadata, &source_graph)];
        let graph = MultiFileReferenceGraph::build(&files).unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::PredefinedEngineAsset {
                external_index: 0,
                resource: PredefinedEngineResourceKind::DefaultResources,
                path_id: 10202,
                class_id: 43,
            },
            "engine resource PathID should resolve using the frozen TPK row for Unity {version}"
        );
        assert!(graph.is_complete());
    }

    let mut source_metadata = metadata_with_external(&[11], "Library/unity default resources");
    source_metadata.prefix.unity_version_text = Some("2022.2.22f1".to_owned());
    let files = [named_file("main.assets", &source_metadata, &source_graph)];
    let graph = MultiFileReferenceGraph::build(&files).unwrap();
    assert_eq!(
        graph.references()[0].resolution,
        MultiFilePPtrResolution::MissingDependency {
            external_index: 0,
            path_id: 10202,
        }
    );
    assert!(!graph.is_complete());

    let mut source_metadata = metadata_with_external(&[11], "Library/unity default resources");
    source_metadata.prefix.unity_version_text = Some("2022.3.22f1".to_owned());
    let unknown_graph = graph_with_reference(&[11], 11, 1, 10201);
    let files = [named_file("main.assets", &source_metadata, &unknown_graph)];
    let graph = MultiFileReferenceGraph::build(&files).unwrap();
    assert_eq!(
        graph.references()[0].resolution,
        MultiFilePPtrResolution::MissingDependency {
            external_index: 0,
            path_id: 10201,
        }
    );
    assert!(!graph.is_complete());
}

#[test]
fn cross_file_graph_distinguishes_missing_target_objects_and_limits() {
    let source_metadata = metadata(&[11]);
    let source_graph = graph_with_reference(&[11], 11, 1, 404);
    let target_metadata = metadata(&[22]);
    let target_graph = empty_graph(&[22]);
    let files = [
        NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &source_metadata,
            graph: &source_graph,
        },
        NamedObjectReferenceGraph {
            file_name: "shared.assets",
            metadata: &target_metadata,
            graph: &target_graph,
        },
    ];

    let graph = MultiFileReferenceGraph::build(&files).unwrap();
    assert_eq!(
        graph.references()[0].resolution,
        MultiFilePPtrResolution::MissingExternalObject {
            target_file_index: 1,
            path_id: 404,
        }
    );
    assert!(matches!(
        MultiFileReferenceGraph::build_with_limits(
            &files,
            MultiFileReferenceGraphLimits {
                max_files: 1,
                ..MultiFileReferenceGraphLimits::default()
            }
        ),
        Err(MultiFileReferenceGraphBuildError::FileCountLimitExceeded {
            file_count: 2,
            limit: 1
        })
    ));
    assert!(matches!(
        MultiFileReferenceGraph::build_with_limits(
            &files,
            MultiFileReferenceGraphLimits {
                max_references: 0,
                ..MultiFileReferenceGraphLimits::default()
            }
        ),
        Err(
            MultiFileReferenceGraphBuildError::ReferenceCountLimitExceeded {
                reference_count: 1,
                limit: 0
            }
        )
    ));
    assert!(matches!(
        MultiFileReferenceGraph::build_with_limits(
            &files,
            MultiFileReferenceGraphLimits {
                max_total_file_name_bytes: 10,
                ..MultiFileReferenceGraphLimits::default()
            }
        ),
        Err(
            MultiFileReferenceGraphBuildError::TotalFileNameBytesLimitExceeded {
                byte_count: 11,
                limit: 10
            }
        )
    ));
}
