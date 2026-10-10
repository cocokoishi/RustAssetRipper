    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

    use super::{
        extract_asset_bundle_container_references, extract_occlusion_scene_guid,
        extract_resource_manager_container_references, extract_streamed_scene_bundle_metadata,
        map_container_paths_to_scene_collections, map_scene_hashes_to_collections,
        map_streamed_scene_bundle_paths, plan_scene_definition, resolve_scene_collection_path,
        scene_manager_kind, streamed_scene_project_path, AssetBundleContainerDecodeError,
        AssetBundleContainerReference, BuildSettingsScenePathSet,
        ResourceManagerContainerDecodeError, ResourceManagerContainerReference,
        SceneBundleMappingError, SceneBundleMappingLimits, SceneBundlePathMapping,
        SceneBundlePathMappingSource, SceneCollectionPathStatus, SceneDefinitionGuidPlan,
        SceneDefinitionPathSource, SceneDefinitionPlanError, SceneDefinitionPlanLimits,
        SceneGuidError, SceneGuidGenerationReason, SceneGuidWords, SceneManagerKind,
        StreamedSceneBundleDecodeError, StreamedSceneBundleMetadata,
        StreamedSceneBundlePathMappings, WorkspaceSceneDefinitionError,
        WorkspaceSceneDefinitionLimits, WorkspaceSceneDefinitions,
    };

    #[test]
    fn identifies_the_baseline_level_game_manager_class_ids() {
        for (class_id, expected) in [
            (29, SceneManagerKind::OcclusionCullingSettings),
            (104, SceneManagerKind::RenderSettings),
            (157, SceneManagerKind::LightmapSettings),
            (196, SceneManagerKind::NavMeshSettings),
        ] {
            assert_eq!(scene_manager_kind(Some(class_id)), Some(expected));
            assert_eq!(expected.class_id(), class_id);
        }
        for class_id in [None, Some(1), Some(4), Some(141), Some(238), Some(1045)] {
            assert_eq!(scene_manager_kind(class_id), None);
        }
    }

    #[test]
    fn extracts_scene_guid_words_through_inherited_fields() {
        let root = SerializedValue::Struct(vec![field(
            "Base",
            SerializedValue::Struct(vec![field(
                "m_SceneGUID",
                SerializedValue::Struct(vec![
                    field("m_Data_0_", SerializedValue::U32(0x0123_4567)),
                    field("m_Data_1_", SerializedValue::U32(0x89ab_cdef)),
                    field("m_Data_2_", SerializedValue::U32(0x1020_3040)),
                    field("m_Data_3_", SerializedValue::U32(0x5060_7080)),
                ]),
            )]),
        )]);
        assert_eq!(
            extract_occlusion_scene_guid(&root).unwrap(),
            Some(SceneGuidWords {
                data: [0x0123_4567, 0x89ab_cdef, 0x1020_3040, 0x5060_7080]
            })
        );
    }

    #[test]
    fn scene_guid_extraction_preserves_absence_and_rejects_malformed_shapes() {
        assert_eq!(
            extract_occlusion_scene_guid(&SerializedValue::Struct(Vec::new())).unwrap(),
            None
        );

        let duplicate_aliases = SerializedValue::Struct(vec![
            field("SceneGUID", SerializedValue::Struct(Vec::new())),
            field("m_SceneGUID", SerializedValue::Struct(Vec::new())),
        ]);
        assert_eq!(
            extract_occlusion_scene_guid(&duplicate_aliases),
            Err(SceneGuidError::DuplicateAliases {
                primary: "SceneGUID",
                alternate: "m_SceneGUID"
            })
        );

        let missing_word = SerializedValue::Struct(vec![field(
            "SceneGUID",
            SerializedValue::Struct(vec![field("m_Data_0_", SerializedValue::U32(1))]),
        )]);
        assert_eq!(
            extract_occlusion_scene_guid(&missing_word),
            Err(SceneGuidError::MissingGuidWord { word_index: 1 })
        );

        let wrong_word_type = SerializedValue::Struct(vec![field(
            "SceneGUID",
            SerializedValue::Struct(vec![
                field("m_Data_0_", SerializedValue::U32(1)),
                field("m_Data_1_", SerializedValue::I32(2)),
                field("m_Data_2_", SerializedValue::U32(3)),
                field("m_Data_3_", SerializedValue::U32(4)),
            ]),
        )]);
        assert_eq!(
            extract_occlusion_scene_guid(&wrong_word_type),
            Err(SceneGuidError::GuidWordHasWrongType {
                word_index: 1,
                actual: "i32"
            })
        );
    }

    #[test]
    fn scene_definition_plan_applies_streamed_build_and_name_precedence() {
        let build_path = SceneCollectionPathStatus::Resolved {
            scene_index: 3,
            path: "Assets/Build/BuildName".to_owned(),
            duplicate_path: false,
        };
        let streamed = plan_scene_definition(
            17,
            "LIBRARY/Level0.assets",
            &build_path,
            Some("Assets/Streamed/BundleScene"),
            None,
            SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(streamed.collection_file_index, 17);
        assert_eq!(streamed.collection_name, "level0.assets");
        assert_eq!(streamed.name, "BundleScene");
        assert_eq!(streamed.path, "Assets/Streamed/BundleScene");
        assert_eq!(
            streamed.path_source,
            SceneDefinitionPathSource::StreamedSceneBundle
        );
        assert_eq!(
            streamed.guid,
            SceneDefinitionGuidPlan::GenerateAtExport(SceneGuidGenerationReason::MissingFromInput)
        );

        let build_settings = plan_scene_definition(
            17,
            "level0.assets",
            &build_path,
            None,
            None,
            SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(build_settings.name, "BuildName");
        assert_eq!(build_settings.path, "Assets/Build/BuildName");
        assert_eq!(
            build_settings.path_source,
            SceneDefinitionPathSource::BuildSettings { scene_index: 3 }
        );

        let windows_separator = plan_scene_definition(
            17,
            "level0.assets",
            &SceneCollectionPathStatus::Resolved {
                scene_index: 4,
                path: r"Assets\Build\WindowsName".to_owned(),
                duplicate_path: false,
            },
            None,
            None,
            SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(windows_separator.name, "WindowsName");

        let name_fallback = plan_scene_definition(
            17,
            "LIBRARY/Level0.assets",
            &SceneCollectionPathStatus::BuildSettingsMissing,
            None,
            None,
            SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(name_fallback.name, "level0.assets");
        assert_eq!(name_fallback.path, "Assets/Scenes/level0.assets");
        assert_eq!(
            name_fallback.path_source,
            SceneDefinitionPathSource::CollectionNameFallback
        );
    }

    #[test]
    fn scene_definition_plan_preserves_nonzero_guid_and_defers_zero_guid_generation() {
        let status = SceneCollectionPathStatus::BuildSettingsMissing;
        let limits = SceneDefinitionPlanLimits::default();
        let nonzero = SceneGuidWords { data: [1, 2, 0, 4] };
        let preserved =
            plan_scene_definition(0, "CAB-Scene", &status, None, Some(nonzero), limits).unwrap();
        assert_eq!(preserved.guid, SceneDefinitionGuidPlan::Preserve(nonzero));

        let zero = plan_scene_definition(
            0,
            "CAB-Scene",
            &status,
            None,
            Some(SceneGuidWords { data: [0; 4] }),
            limits,
        )
        .unwrap();
        assert_eq!(
            zero.guid,
            SceneDefinitionGuidPlan::GenerateAtExport(SceneGuidGenerationReason::ZeroInInput)
        );
        assert!(SceneGuidWords { data: [0; 4] }.is_zero());
    }

    #[test]
    fn workspace_scene_definitions_preserve_collection_ownership() {
        let status = SceneCollectionPathStatus::BuildSettingsMissing;
        let plans = vec![
            plan_scene_definition(
                1,
                "level0",
                &status,
                None,
                None,
                SceneDefinitionPlanLimits::default(),
            )
            .unwrap(),
            plan_scene_definition(
                4,
                "level1",
                &status,
                None,
                Some(SceneGuidWords { data: [1, 2, 3, 4] }),
                SceneDefinitionPlanLimits::default(),
            )
            .unwrap(),
        ];
        let mut definitions = WorkspaceSceneDefinitions::from_plans(
            plans,
            8,
            WorkspaceSceneDefinitionLimits::default(),
        )
        .unwrap();

        assert_eq!(definitions.scene_index_for_collection(1), Some(0));
        assert_eq!(definitions.scene_index_for_collection(4), Some(1));
        assert_eq!(definitions.scene_index_for_collection(7), None);
        assert_eq!(definitions.definitions()[0].collection_file_indices(), [1]);
        definitions.add_collection(0, 3).unwrap();
        assert_eq!(
            definitions.definitions()[0].collection_file_indices(),
            [1, 3]
        );
        assert_eq!(definitions.scene_index_for_collection(3), Some(0));
        assert_eq!(
            definitions.add_collection(1, 3),
            Err(WorkspaceSceneDefinitionError::CollectionAlreadyAssigned {
                collection_file_index: 3,
                scene_index: 0,
            })
        );
        assert_eq!(
            definitions.add_collection(0, 8),
            Err(
                WorkspaceSceneDefinitionError::CollectionFileIndexOutOfBounds {
                    index: 8,
                    count: 8,
                }
            )
        );
        assert_eq!(
            definitions.add_collection(2, 6),
            Err(WorkspaceSceneDefinitionError::SceneIndexOutOfBounds { index: 2, count: 2 })
        );
    }

    #[test]
    fn scene_definition_plan_bounds_names_and_paths() {
        let status = SceneCollectionPathStatus::Resolved {
            scene_index: 0,
            path: "Assets/Scenes/TooLong".to_owned(),
            duplicate_path: false,
        };
        let path_limits = SceneDefinitionPlanLimits {
            max_collection_name_bytes: 32,
            max_scene_name_bytes: 32,
            max_scene_path_bytes: 8,
        };
        assert_eq!(
            plan_scene_definition(0, "level0", &status, None, None, path_limits),
            Err(SceneDefinitionPlanError::ScenePathLimitExceeded {
                length: "Assets/Scenes/TooLong".len(),
                limit: 8,
            })
        );

        let name_limits = SceneDefinitionPlanLimits {
            max_collection_name_bytes: 32,
            max_scene_name_bytes: 3,
            max_scene_path_bytes: 64,
        };
        assert_eq!(
            plan_scene_definition(
                0,
                "x",
                &SceneCollectionPathStatus::BuildSettingsMissing,
                Some("Assets/Scenes/Main"),
                None,
                name_limits,
            ),
            Err(SceneDefinitionPlanError::SceneNameLimitExceeded {
                length: "Main".len(),
                limit: 3,
            })
        );
    }

    #[test]
    fn resolves_scene_collection_names_to_build_settings_paths() {
        let scenes = [
            "Assets/Scenes/Main.unity".to_owned(),
            "Assets/Scenes/Next.unity".to_owned(),
        ];
        assert_eq!(
            resolve_scene_collection_path(
                "MAINDATA",
                Some("5.2.4f1"),
                BuildSettingsScenePathSet::Unique(&scenes),
            ),
            SceneCollectionPathStatus::Resolved {
                scene_index: 0,
                path: "Assets/Scenes/Main".to_owned(),
                duplicate_path: false,
            }
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                Some("5.3.0f1"),
                BuildSettingsScenePathSet::Unique(&scenes),
            ),
            SceneCollectionPathStatus::Resolved {
                scene_index: 0,
                path: "Assets/Scenes/Main".to_owned(),
                duplicate_path: false,
            }
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                Some("5.2.4f1"),
                BuildSettingsScenePathSet::Unique(&scenes),
            ),
            SceneCollectionPathStatus::Resolved {
                scene_index: 1,
                path: "Assets/Scenes/Next".to_owned(),
                duplicate_path: false,
            }
        );
    }

    #[test]
    fn keeps_missing_ambiguous_stale_and_unusable_scene_paths_distinct() {
        let paths = ["".to_owned()];
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                Some("2022.3.1f1"),
                BuildSettingsScenePathSet::Missing
            ),
            SceneCollectionPathStatus::BuildSettingsMissing
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                Some("2022.3.1f1"),
                BuildSettingsScenePathSet::Ambiguous
            ),
            SceneCollectionPathStatus::BuildSettingsAmbiguous
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                None,
                BuildSettingsScenePathSet::Unique(&paths)
            ),
            SceneCollectionPathStatus::UnityVersionUnavailable
        );
        assert_eq!(
            resolve_scene_collection_path(
                "other",
                Some("2022.3.1f1"),
                BuildSettingsScenePathSet::Unique(&paths)
            ),
            SceneCollectionPathStatus::CollectionNameNotMapped
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level1",
                Some("2022.3.1f1"),
                BuildSettingsScenePathSet::Unique(&paths)
            ),
            SceneCollectionPathStatus::StaleBuildSettingsIndex {
                scene_index: 1,
                scene_count: 1
            }
        );
        assert_eq!(
            resolve_scene_collection_path(
                "level0",
                Some("2022.3.1f1"),
                BuildSettingsScenePathSet::Unique(&paths)
            ),
            SceneCollectionPathStatus::BuildSettingsPathUnusable { scene_index: 0 }
        );
    }

    #[test]
    fn maps_scene_hashes_to_first_normalized_bundle_collection() {
        let collection_names = ["other".to_owned(), "Level0".to_owned(), "LEVEL0".to_owned()];
        let scene_hashes = [
            (
                "/build/project/Assets/Scenes/Main.unity".to_owned(),
                "archive:/bundle/LEVEL0".to_owned(),
            ),
            ("assets/Scenes/Next.unity".to_owned(), "level0".to_owned()),
        ];
        assert_eq!(
            map_scene_hashes_to_collections(
                &scene_hashes,
                &collection_names,
                SceneBundleMappingLimits::default()
            )
            .unwrap(),
            [
                SceneBundlePathMapping {
                    collection_index: 1,
                    path: "Assets/Scenes/Main".to_owned(),
                },
                SceneBundlePathMapping {
                    collection_index: 1,
                    path: "Assets/Scenes/Next".to_owned(),
                }
            ]
        );
        assert_eq!(
            streamed_scene_project_path("/project/outside/Scene.unity"),
            "Assets/"
        );
    }

    #[test]
    fn maps_container_paths_in_order_to_the_next_known_scene_collection() {
        let container_paths = [
            "Assets/Scenes/First.unity".to_owned(),
            "Second.unity".to_owned(),
        ];
        let collection_is_scene = [false, true, false, true];
        assert_eq!(
            map_container_paths_to_scene_collections(
                &container_paths,
                &collection_is_scene,
                SceneBundleMappingLimits::default()
            )
            .unwrap(),
            [
                SceneBundlePathMapping {
                    collection_index: 1,
                    path: "Assets/Scenes/First".to_owned(),
                },
                SceneBundlePathMapping {
                    collection_index: 3,
                    path: "Assets/Second".to_owned(),
                }
            ]
        );
        assert_eq!(
            map_container_paths_to_scene_collections(
                &["Scene.unity".to_owned()],
                &[false],
                SceneBundleMappingLimits::default()
            ),
            Err(
                SceneBundleMappingError::ContainerEntryHasNoSceneCollection {
                    entry_index: 0,
                    starting_collection_index: 0,
                }
            )
        );
    }

    #[test]
    fn streamed_scene_mapping_reports_missing_collections_and_limits() {
        let scene_hashes = [("Scene.unity".to_owned(), "missing".to_owned())];
        assert_eq!(
            map_scene_hashes_to_collections(
                &scene_hashes,
                &["present".to_owned()],
                SceneBundleMappingLimits::default()
            ),
            Err(SceneBundleMappingError::SceneHashCollectionNotFound {
                entry_index: 0,
                collection_name: "missing".to_owned(),
            })
        );
        assert_eq!(
            map_scene_hashes_to_collections(
                &scene_hashes,
                &["present".to_owned()],
                SceneBundleMappingLimits {
                    max_entries: 0,
                    max_total_name_and_path_bytes: usize::MAX,
                }
            ),
            Err(SceneBundleMappingError::EntryCountLimitExceeded {
                resource: "SceneHashes",
                count: 1,
                limit: 0,
            })
        );
    }

    #[test]
    fn asset_bundle_container_decoder_preserves_asset_pointer_file_scope() {
        let entry = |path: &str, file_id, path_id| {
            SerializedValue::Struct(vec![
                field(
                    "first",
                    SerializedValue::StringBytes(path.as_bytes().to_vec()),
                ),
                field(
                    "second",
                    SerializedValue::Struct(vec![
                        field("preloadIndex", SerializedValue::I32(0)),
                        field("preloadSize", SerializedValue::I32(0)),
                        field("asset", SerializedValue::PPtr { file_id, path_id }),
                    ]),
                ),
            ])
        };
        let root = SerializedValue::Struct(vec![field(
            "Base",
            SerializedValue::Struct(vec![field(
                "m_Container",
                SerializedValue::Array(vec![
                    entry("Assets/Local.prefab", 0, 17),
                    entry("Assets/Shared.mat", 2, 81),
                    entry("Assets/Null.asset", 0, 0),
                ]),
            )]),
        )]);

        assert_eq!(
            extract_asset_bundle_container_references(&root, SceneBundleMappingLimits::default(),)
                .unwrap(),
            [
                AssetBundleContainerReference {
                    path: "Assets/Local.prefab".to_owned(),
                    file_id: 0,
                    path_id: 17,
                },
                AssetBundleContainerReference {
                    path: "Assets/Shared.mat".to_owned(),
                    file_id: 2,
                    path_id: 81,
                },
                AssetBundleContainerReference {
                    path: "Assets/Null.asset".to_owned(),
                    file_id: 0,
                    path_id: 0,
                },
            ]
        );
    }

    #[test]
    fn asset_bundle_container_decoder_reports_missing_and_invalid_asset_pointers() {
        assert_eq!(
            extract_asset_bundle_container_references(
                &SerializedValue::Struct(Vec::new()),
                SceneBundleMappingLimits::default(),
            ),
            Err(AssetBundleContainerDecodeError::MissingContainerField)
        );

        let root = SerializedValue::Struct(vec![field(
            "m_Container",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field(
                    "first",
                    SerializedValue::StringBytes(b"Assets/Missing.asset".to_vec()),
                ),
                field("second", SerializedValue::Struct(Vec::new())),
            ])]),
        )]);
        assert_eq!(
            extract_asset_bundle_container_references(&root, SceneBundleMappingLimits::default(),),
            Err(AssetBundleContainerDecodeError::MissingAssetPointer { entry_index: 0 })
        );

        let root = SerializedValue::Struct(vec![field(
            "Container",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field(
                    "first",
                    SerializedValue::StringBytes(b"Assets/Wrong.asset".to_vec()),
                ),
                field(
                    "second",
                    SerializedValue::Struct(vec![field("asset", SerializedValue::I32(7))]),
                ),
            ])]),
        )]);
        assert!(matches!(
            extract_asset_bundle_container_references(
                &root,
                SceneBundleMappingLimits::default(),
            ),
            Err(AssetBundleContainerDecodeError::ContainerMetadata(
                StreamedSceneBundleDecodeError::WrongType {
                    field_path,
                    expected: "PPtr",
                    actual: "i32",
                }
            )) if field_path == "m_Container[0].second.asset"
        ));
    }

    #[test]
    fn asset_bundle_container_decoder_obeys_entry_and_path_limits() {
        let root = SerializedValue::Struct(vec![field(
            "m_Container",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("first", SerializedValue::StringBytes(b"a".to_vec())),
                field(
                    "second",
                    SerializedValue::Struct(vec![field(
                        "asset",
                        SerializedValue::PPtr {
                            file_id: 0,
                            path_id: 1,
                        },
                    )]),
                ),
            ])]),
        )]);

        assert!(matches!(
            extract_asset_bundle_container_references(
                &root,
                SceneBundleMappingLimits {
                    max_entries: 0,
                    max_total_name_and_path_bytes: usize::MAX,
                },
            ),
            Err(AssetBundleContainerDecodeError::ContainerMetadata(
                StreamedSceneBundleDecodeError::EntryCountLimitExceeded {
                    field_path,
                    count: 1,
                    limit: 0,
                }
            )) if field_path == "m_Container"
        ));
        assert!(matches!(
            extract_asset_bundle_container_references(
                &root,
                SceneBundleMappingLimits {
                    max_entries: 1,
                    max_total_name_and_path_bytes: 0,
                },
            ),
            Err(AssetBundleContainerDecodeError::ContainerMetadata(
                StreamedSceneBundleDecodeError::ByteLimitExceeded {
                    requested: 1,
                    limit: 0,
                }
            ))
        ));
    }

    #[test]
    fn resource_manager_container_decoder_preserves_order_and_pointer_scope() {
        let entry = |path: &str, file_id, path_id| {
            SerializedValue::Struct(vec![
                field(
                    "first",
                    SerializedValue::StringBytes(path.as_bytes().to_vec()),
                ),
                field("second", SerializedValue::PPtr { file_id, path_id }),
            ])
        };
        let root = SerializedValue::Struct(vec![field(
            "Base",
            SerializedValue::Struct(vec![field(
                "Container",
                SerializedValue::Array(vec![
                    entry("Textures/inner/wood", 0, 17),
                    entry("wood", 2, 81),
                    entry("Null", 0, 0),
                ]),
            )]),
        )]);

        assert_eq!(
            extract_resource_manager_container_references(
                &root,
                SceneBundleMappingLimits::default(),
            )
            .unwrap(),
            [
                ResourceManagerContainerReference {
                    path: "Textures/inner/wood".to_owned(),
                    file_id: 0,
                    path_id: 17,
                },
                ResourceManagerContainerReference {
                    path: "wood".to_owned(),
                    file_id: 2,
                    path_id: 81,
                },
                ResourceManagerContainerReference {
                    path: "Null".to_owned(),
                    file_id: 0,
                    path_id: 0,
                },
            ]
        );
    }

    #[test]
    fn resource_manager_container_decoder_reports_missing_and_invalid_values() {
        assert_eq!(
            extract_resource_manager_container_references(
                &SerializedValue::Struct(Vec::new()),
                SceneBundleMappingLimits::default(),
            ),
            Err(ResourceManagerContainerDecodeError::MissingContainerField)
        );

        let root = SerializedValue::Struct(vec![field(
            "m_Container",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("first", SerializedValue::StringBytes(b"bad".to_vec())),
                field("second", SerializedValue::I32(7)),
            ])]),
        )]);
        assert!(matches!(
            extract_resource_manager_container_references(
                &root,
                SceneBundleMappingLimits::default(),
            ),
            Err(ResourceManagerContainerDecodeError::Metadata(
                StreamedSceneBundleDecodeError::WrongType {
                    field_path,
                    expected: "PPtr",
                    actual: "i32",
                }
            )) if field_path == "m_Container[0].second"
        ));
    }

    #[test]
    fn streamed_scene_bundle_mapping_selects_scene_hashes_then_container_fallback() {
        let collection_names = [
            "CAB-Scene".to_owned(),
            "not-a-scene".to_owned(),
            "cab-scene".to_owned(),
        ];
        let collection_is_scene = [true, false, true];
        let scene_hashes_metadata = StreamedSceneBundleMetadata {
            is_streamed_scene_asset_bundle: Some(true),
            scene_hashes: Some(vec![(
                "Scenes/Main.unity".to_owned(),
                "CAB-Scene".to_owned(),
            )]),
            container_paths: Some(vec!["Ignored/Container.unity".to_owned()]),
        };
        assert_eq!(
            map_streamed_scene_bundle_paths(
                &scene_hashes_metadata,
                &collection_names,
                &collection_is_scene,
                SceneBundleMappingLimits::default(),
            )
            .unwrap(),
            Some(StreamedSceneBundlePathMappings {
                source: SceneBundlePathMappingSource::SceneHashes,
                mappings: vec![SceneBundlePathMapping {
                    collection_index: 0,
                    path: "Assets/Scenes/Main".to_owned(),
                }],
            })
        );

        let container_metadata = StreamedSceneBundleMetadata {
            is_streamed_scene_asset_bundle: Some(true),
            scene_hashes: Some(Vec::new()),
            container_paths: Some(vec![
                "Scenes/First.unity".to_owned(),
                "Scenes/Second.unity".to_owned(),
            ]),
        };
        assert_eq!(
            map_streamed_scene_bundle_paths(
                &container_metadata,
                &collection_names,
                &collection_is_scene,
                SceneBundleMappingLimits::default(),
            )
            .unwrap(),
            Some(StreamedSceneBundlePathMappings {
                source: SceneBundlePathMappingSource::Container,
                mappings: vec![
                    SceneBundlePathMapping {
                        collection_index: 0,
                        path: "Assets/Scenes/First".to_owned(),
                    },
                    SceneBundlePathMapping {
                        collection_index: 2,
                        path: "Assets/Scenes/Second".to_owned(),
                    },
                ],
            })
        );
    }

    #[test]
    fn streamed_scene_bundle_mapping_keeps_unselected_and_missing_states_explicit() {
        let not_streamed = StreamedSceneBundleMetadata {
            is_streamed_scene_asset_bundle: Some(false),
            scene_hashes: None,
            container_paths: None,
        };
        assert_eq!(
            map_streamed_scene_bundle_paths(
                &not_streamed,
                &[],
                &[],
                SceneBundleMappingLimits::default(),
            )
            .unwrap(),
            None
        );

        let streamed_without_paths = StreamedSceneBundleMetadata {
            is_streamed_scene_asset_bundle: Some(true),
            scene_hashes: None,
            container_paths: None,
        };
        assert_eq!(
            map_streamed_scene_bundle_paths(
                &streamed_without_paths,
                &[],
                &[],
                SceneBundleMappingLimits::default(),
            ),
            Err(SceneBundleMappingError::MissingContainerScenePaths)
        );

        let empty_container = StreamedSceneBundleMetadata {
            is_streamed_scene_asset_bundle: Some(true),
            scene_hashes: Some(Vec::new()),
            container_paths: Some(Vec::new()),
        };
        assert_eq!(
            map_streamed_scene_bundle_paths(
                &empty_container,
                &["collection".to_owned()],
                &[],
                SceneBundleMappingLimits::default(),
            ),
            Err(SceneBundleMappingError::CollectionMetadataCountMismatch {
                collection_name_count: 1,
                scene_flag_count: 0,
            })
        );
    }

    #[test]
    fn extracts_streamed_scene_bundle_fields_from_type_tree_values() {
        let root = SerializedValue::Struct(vec![
            field(
                "Base",
                SerializedValue::Struct(vec![field(
                    "m_IsStreamedSceneAssetBundle",
                    SerializedValue::Bool(true),
                )]),
            ),
            field(
                "m_SceneHashes",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    field(
                        "first",
                        SerializedValue::StringBytes(b"Scenes/Main.unity".to_vec()),
                    ),
                    field(
                        "second",
                        SerializedValue::StringBytes(b"CAB-Scene".to_vec()),
                    ),
                ])]),
            ),
            field(
                "m_Container",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    field(
                        "first",
                        SerializedValue::StringBytes(b"Scenes/Other.unity".to_vec()),
                    ),
                    field(
                        "second",
                        SerializedValue::Struct(vec![field(
                            "asset",
                            SerializedValue::PPtr {
                                file_id: 0,
                                path_id: 0,
                            },
                        )]),
                    ),
                ])]),
            ),
        ]);

        assert_eq!(
            extract_streamed_scene_bundle_metadata(&root, SceneBundleMappingLimits::default())
                .unwrap(),
            StreamedSceneBundleMetadata {
                is_streamed_scene_asset_bundle: Some(true),
                scene_hashes: Some(vec![(
                    "Scenes/Main.unity".to_owned(),
                    "CAB-Scene".to_owned()
                )]),
                container_paths: Some(vec!["Scenes/Other.unity".to_owned()]),
            }
        );
    }

    #[test]
    fn streamed_scene_bundle_metadata_preserves_absence_and_false_flag() {
        let absent = SerializedValue::Struct(Vec::new());
        assert_eq!(
            extract_streamed_scene_bundle_metadata(&absent, SceneBundleMappingLimits::default())
                .unwrap(),
            StreamedSceneBundleMetadata {
                is_streamed_scene_asset_bundle: None,
                scene_hashes: None,
                container_paths: None,
            }
        );

        let false_flag = SerializedValue::Struct(vec![field(
            "IsStreamedSceneAssetBundle",
            SerializedValue::Bool(false),
        )]);
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &false_flag,
                SceneBundleMappingLimits::default()
            )
            .unwrap()
            .is_streamed_scene_asset_bundle,
            Some(false)
        );
    }

    #[test]
    fn streamed_scene_bundle_metadata_rejects_ambiguous_and_unbounded_data() {
        let duplicate_flag = SerializedValue::Struct(vec![
            field("m_IsStreamedSceneAssetBundle", SerializedValue::Bool(true)),
            field("IsStreamedSceneAssetBundle", SerializedValue::Bool(true)),
        ]);
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &duplicate_flag,
                SceneBundleMappingLimits::default()
            ),
            Err(StreamedSceneBundleDecodeError::DuplicateAliases {
                primary: "m_IsStreamedSceneAssetBundle",
                alternate: "IsStreamedSceneAssetBundle",
            })
        );

        let too_many = SerializedValue::Struct(vec![field(
            "m_Container",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![])]),
        )]);
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &too_many,
                SceneBundleMappingLimits {
                    max_entries: 0,
                    max_total_name_and_path_bytes: usize::MAX,
                }
            ),
            Err(StreamedSceneBundleDecodeError::EntryCountLimitExceeded {
                field_path: "m_Container".to_owned(),
                count: 1,
                limit: 0,
            })
        );

        let combined_entries = SerializedValue::Struct(vec![
            field(
                "m_SceneHashes",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    field("first", SerializedValue::StringBytes(b"a".to_vec())),
                    field("second", SerializedValue::StringBytes(b"b".to_vec())),
                ])]),
            ),
            field(
                "m_Container",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    field("first", SerializedValue::StringBytes(b"c".to_vec())),
                    field("second", SerializedValue::Struct(Vec::new())),
                ])]),
            ),
        ]);
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &combined_entries,
                SceneBundleMappingLimits {
                    max_entries: 1,
                    max_total_name_and_path_bytes: usize::MAX,
                }
            ),
            Err(StreamedSceneBundleDecodeError::EntryCountLimitExceeded {
                field_path: "m_Container".to_owned(),
                count: 2,
                limit: 1,
            })
        );

        let invalid_utf8 = SerializedValue::Struct(vec![field(
            "m_SceneHashes",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("first", SerializedValue::StringBytes(vec![0xff])),
                field("second", SerializedValue::StringBytes(Vec::new())),
            ])]),
        )]);
        assert_eq!(
            extract_streamed_scene_bundle_metadata(
                &invalid_utf8,
                SceneBundleMappingLimits::default()
            ),
            Err(StreamedSceneBundleDecodeError::InvalidUtf8 {
                field_path: "m_SceneHashes[0].first".to_owned(),
            })
        );
    }

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }
