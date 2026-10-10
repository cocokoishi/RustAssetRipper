    use super::{
        collect_references, MultiFileObjectIdentity, MultiFileObjectReference,
        MultiFilePPtrResolution, MultiFileReferenceGraph, MultiFileReferenceGraphBuildError,
        MultiFileReferenceGraphLimits, NamedObjectReferenceGraph, ObjectReferenceGraph,
        ReferenceGraphLimits, SerializedObjectReference, WalkIssue,
    };
    use crate::io::binary::Endian;
    use crate::processing::asset_bundle::{
        map_asset_bundle_container, resolve_asset_bundle_container_reference,
        AssetBundleContainerTarget,
    };
    use crate::processing::scene_collections::{
        AssetBundleContainerReference, SceneBundleMappingLimits,
    };
    use crate::serialized_file::metadata::{
        FileIdentifier, ObjectInfo, SerializedFileMetadata, SerializedType, TypeTree, TypeTreeNode,
    };
    use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
    use crate::serialized_file::references::PPtrResolution;

    fn metadata(path_ids: &[i64]) -> SerializedFileMetadata {
        SerializedFileMetadata {
            prefix: SerializedFileMetadataPrefix {
                metadata_offset: 0,
                metadata_size: 0,
                body_endian: Endian::Little,
                unity_version_text: None,
                target_platform: None,
                has_type_tree: false,
                consumed_bytes: 0,
            },
            types: Vec::new(),
            long_file_id: None,
            objects: path_ids.iter().map(|path_id| object(*path_id)).collect(),
            script_types: Vec::new(),
            externals: vec![external("shared.assets"), external("scene.assets")],
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        }
    }

    fn object(path_id: i64) -> ObjectInfo {
        ObjectInfo {
            path_id,
            byte_start: 0,
            byte_size: 0,
            serialized_type_index: None,
            raw_type_id: 0,
            class_id: None,
            script_type_index: None,
            is_destroyed: None,
            is_stripped: None,
            resolved_type_index: None,
        }
    }

    fn external(path: &str) -> FileIdentifier {
        FileIdentifier {
            asset_path: Some(path.to_owned()),
            guid: None,
            file_type: None,
            path_name_origin: path.to_owned(),
        }
    }

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

    fn empty_graph(path_ids: &[i64]) -> ObjectReferenceGraph {
        let mut path_id_index: Vec<_> = path_ids
            .iter()
            .enumerate()
            .map(|(index, path_id)| (*path_id, index))
            .collect();
        path_id_index.sort_unstable();
        ObjectReferenceGraph {
            object_count: path_ids.len(),
            attempted_objects: path_ids.len(),
            path_id_index,
            references: Vec::new(),
            managed_objects: Vec::new(),
            managed_references: Vec::new(),
            issues: Vec::new(),
        }
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
            super::engine_resource_alias("unity default resources"),
            Some("unity_default_resources")
        );
        assert_eq!(
            super::engine_resource_alias("unity_default_resources"),
            Some("unity default resources")
        );
        assert_eq!(
            super::engine_resource_alias("unity builtin extra"),
            Some("unity_builtin_extra")
        );
        assert_eq!(
            super::engine_resource_alias("unity_builtin_extra"),
            Some("unity builtin extra")
        );
        assert_eq!(super::engine_resource_alias("shared.assets"), None);

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

    #[test]
    fn walks_nested_structures_and_arrays_with_stable_field_paths() {
        let pointer = |path_id| SerializedValue::PPtr {
            file_id: 0,
            path_id,
        };
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Items".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "m_Target".to_owned(),
                    value: pointer(42),
                },
            ])]),
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

        let managed = super::collect_managed_reference_graph(
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
        let legacy = super::collect_managed_reference_graph(
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

        let managed = super::collect_managed_reference_graph(
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

        let legacy_graph = super::collect_managed_reference_graph(
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

        let scope_limited = super::collect_managed_reference_graph(
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
            Err(super::WalkIssue::ManagedRegistryScopeLimitExceeded { .. })
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

        let fixture = include_str!("pptr_probe_fixture.txt");
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
