    #[test]
    fn workspace_retains_source_failed_entries_with_their_container_owner() {
        use crate::scheme::{ContainerMountLimits, SchemeReader};

        let row = include_str!("../io/io_differential_fixture.txt")
            .lines()
            .find(|line| line.starts_with("SCHEME|ufs_v6_lzham_block|"))
            .expect("reference fixture contains the failed-entry continuation case");
        let input_hex = row
            .split('|')
            .find_map(|field| field.strip_prefix("in="))
            .expect("fixture row contains input bytes");
        let bytes = input_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let dispatched =
            SchemeReader::read_with_limits(&bytes, crate::scheme::SchemeReaderLimits::default())
                .unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let mount = dispatched
            .mount_container_tree_with_source_name(
                &mut filesystem,
                Some("/workspace-ufs"),
                Some("sample.bundle"),
                ContainerMountLimits::default(),
            )
            .unwrap();
        let mut workspace = SerializedFileWorkspace::default();
        append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &mount.container_scopes,
                mounted_bundles: &mount.container_bundles,
                serialized_files: &mount.serialized_files,
                resource_files: &mount.resource_files,
                failed_entries: &mount.failed_entries,
                source_container_path: "/source/sample.bundle",
                loaded_paths: &std::collections::HashMap::new(),
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 4,
            },
            &mut 0,
            &mut 0,
        )
        .unwrap();

        let failures = workspace.failed_container_entries();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].source_container_path, "/source/sample.bundle");
        assert_eq!(failures[0].entry.source_name, "unsupported.bin");
        assert_eq!(failures[0].entry.container_bundle_index, Some(0));
        assert_eq!(failures[0].entry.entry_index, 1);
        assert_eq!(
            filesystem
                .read_all_bytes("/workspace-ufs/after.bin", 16)
                .unwrap(),
            b"LATER"
        );
    }

    #[test]
    fn workspace_container_bundle_path_and_count_limits_are_explicit() {
        let scopes = ["/root".to_owned()];
        let bundles = [MountedContainerBundle {
            scope_index: Some(0),
            parent_bundle_index: None,
            parent_entry_index: None,
            name_fixed: None,
        }];
        let mounted_files = [];
        let mounted_resources = [MountedResourceFile {
            virtual_path: "/root/stream.resS".to_owned(),
            name: "stream.resS".to_owned(),
            container_scope_index: Some(0),
            container_bundle_index: 0,
            entry_index: 0,
            resource_index: 0,
        }];
        let loaded_paths = std::collections::HashMap::new();
        let mut workspace = SerializedFileWorkspace::default();
        let mut total_path_bytes = 0usize;
        let mut total_resource_path_bytes = 0usize;
        let path_limited = SerializedFileWorkspaceLimits {
            max_total_bundle_node_path_bytes: 4,
            ..SerializedFileWorkspaceLimits::default()
        };
        let error = append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &mounted_files,
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &loaded_paths,
                limits: path_limited,
                bundle_count_limit: 1,
            },
            &mut total_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalBundleNodePathBytesLimitExceeded {
                byte_count: 5,
                limit: 4
            }
        ));
        assert!(workspace.container_bundles().is_empty());

        let resource_path_limited = SerializedFileWorkspaceLimits {
            max_total_bundle_resource_file_path_bytes: 4,
            ..SerializedFileWorkspaceLimits::default()
        };
        let error = append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &mounted_files,
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &loaded_paths,
                limits: resource_path_limited,
                bundle_count_limit: 1,
            },
            &mut total_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalBundleResourceFilePathBytesLimitExceeded {
                byte_count: 17,
                limit: 4
            }
        ));
        assert!(workspace.container_bundles().is_empty());
        assert!(workspace.resource_files().is_empty());

        let gapped_resources = [
            MountedResourceFile {
                resource_index: 0,
                ..mounted_resources[0].clone()
            },
            MountedResourceFile {
                virtual_path: "/root/second.resS".to_owned(),
                entry_index: 1,
                resource_index: 2,
                ..mounted_resources[0].clone()
            },
        ];
        let error = append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &mounted_files,
                resource_files: &gapped_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &loaded_paths,
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 1,
            },
            &mut total_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::InvalidContainerResourceSequence {
                expected: 1,
                actual: 2
            }
        ));
        assert!(workspace.container_bundles().is_empty());
        assert!(workspace.resource_files().is_empty());

        let error = append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &mounted_files,
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &loaded_paths,
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 0,
            },
            &mut total_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::ContainerBundleCountLimitExceeded {
                count: 1,
                limit: 0
            }
        ));
        assert!(workspace.container_bundles().is_empty());
    }

    #[test]
    fn workspace_retains_fixed_bundle_names_and_bounds_their_aggregate_bytes() {
        let scopes = ["/bundle".to_owned()];
        let bundles = [MountedContainerBundle {
            scope_index: Some(0),
            parent_bundle_index: None,
            parent_entry_index: None,
            name_fixed: Some("assets.bundle".to_owned()),
        }];
        let mut workspace = SerializedFileWorkspace::default();
        append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &[],
                resource_files: &[],
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &std::collections::HashMap::new(),
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 1,
            },
            &mut 0,
            &mut 0,
        )
        .unwrap();
        assert_eq!(
            workspace.container_bundles()[0].name_fixed.as_deref(),
            Some("assets.bundle")
        );
        assert_eq!(workspace.report().bundle_name_bytes, 13);

        let mut limited_workspace = SerializedFileWorkspace::default();
        let error = append_mounted_container_bundles(
            &mut limited_workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &[],
                resource_files: &[],
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &std::collections::HashMap::new(),
                limits: SerializedFileWorkspaceLimits {
                    max_total_bundle_name_bytes: 4,
                    ..SerializedFileWorkspaceLimits::default()
                },
                bundle_count_limit: 1,
            },
            &mut 0,
            &mut 0,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalBundleNameBytesLimitExceeded {
                byte_count: 13,
                limit: 4
            }
        ));
        assert!(limited_workspace.container_bundles().is_empty());
        assert_eq!(limited_workspace.report().bundle_name_bytes, 0);
    }

    #[test]
    fn workspace_resource_source_names_obey_their_aggregate_bound() {
        let scopes = ["/root".to_owned()];
        let bundles = [MountedContainerBundle {
            scope_index: Some(0),
            parent_bundle_index: None,
            parent_entry_index: None,
            name_fixed: None,
        }];
        let mounted_resources = [MountedResourceFile {
            virtual_path: "/root/stream.resS".to_owned(),
            name: "stream.resS".to_owned(),
            container_scope_index: Some(0),
            container_bundle_index: 0,
            entry_index: 0,
            resource_index: 0,
        }];
        let mut workspace = SerializedFileWorkspace::default();
        let error = append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &[],
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &std::collections::HashMap::new(),
                limits: SerializedFileWorkspaceLimits {
                    max_total_bundle_resource_file_name_bytes: 4,
                    ..SerializedFileWorkspaceLimits::default()
                },
                bundle_count_limit: 1,
            },
            &mut 0,
            &mut 0,
        )
        .unwrap_err();

        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalBundleResourceFileNameBytesLimitExceeded {
                byte_count: 11,
                limit: 4
            }
        ));
        assert_eq!(error.report.bundle_resource_file_name_bytes, 0);
    }

    #[test]
    fn pathless_container_root_keeps_its_direct_collection_membership() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/first.asset", &make_generation_10_file());
        insert_file(&mut filesystem, "/resource.resS", b"stream data");
        let inputs = [input("/first.asset")];
        let mut workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let scopes = [];
        let bundles = [MountedContainerBundle {
            scope_index: None,
            parent_bundle_index: None,
            parent_entry_index: None,
            name_fixed: None,
        }];
        let mounted_files = [crate::scheme::MountedSerializedFile {
            virtual_path: "/first.asset".to_owned(),
            container_scope_index: None,
            container_bundle_index: Some(0),
            entry_index: 0,
            collection_index: Some(0),
        }];
        let mounted_resources = [MountedResourceFile {
            virtual_path: "/resource.resS".to_owned(),
            name: "resource.resS".to_owned(),
            container_scope_index: None,
            container_bundle_index: 0,
            entry_index: 1,
            resource_index: 0,
        }];
        let mut loaded_paths = std::collections::HashMap::new();
        loaded_paths.insert("/first.asset".to_owned(), 0);
        let mut total_node_path_bytes = 0;
        let mut total_resource_path_bytes = 0;
        append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &mounted_files,
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &loaded_paths,
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 1,
            },
            &mut total_node_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap();
        let order = super::build_mounted_file_order_index(
            std::iter::once((scopes.as_slice(), mounted_files.as_slice())),
            1,
            workspace.report,
        )
        .unwrap();
        super::attach_container_file_order(&mut workspace.files[0], &order, workspace.report)
            .unwrap();

        assert_eq!(workspace.container_bundles().len(), 1);
        assert_eq!(workspace.container_bundles()[0].scope_path, None);
        assert_eq!(
            workspace.container_bundles()[0].direct_collection_file_indices,
            [0]
        );
        assert_eq!(
            workspace.container_bundles()[0].direct_resource_file_indices,
            [0]
        );
        assert_eq!(workspace.files()[0].container_bundle_index, Some(0));
        assert_eq!(workspace.files()[0].container_collection_index, Some(0));
        assert_eq!(workspace.resource_files().len(), 1);
        assert_eq!(workspace.resource_files()[0].virtual_path, "/resource.resS");
        assert_eq!(workspace.resource_files()[0].container_bundle_index, 0);
    }

    /// Mounted resources cache their fixed name at registration (like `FileBase.NameFixed`),
    /// so hierarchy lookups compare the cached form instead of renormalizing per lookup.
    #[test]
    fn mounted_resources_cache_their_fixed_names_for_lookups() {
        let scopes = [];
        let bundles = [MountedContainerBundle {
            scope_index: None,
            parent_bundle_index: None,
            parent_entry_index: None,
            name_fixed: None,
        }];
        let mounted_resources = [MountedResourceFile {
            virtual_path: "/archive:/textures/Shared.resS".to_owned(),
            name: "archive:/textures/Shared.resS".to_owned(),
            container_scope_index: None,
            container_bundle_index: 0,
            entry_index: 0,
            resource_index: 0,
        }];
        let mut workspace = SerializedFileWorkspace::default();
        let mut total_node_path_bytes = 0;
        let mut total_resource_path_bytes = 0;
        super::append_mounted_container_bundles(
            &mut workspace,
            super::MountedContainerBundleInput {
                container_scopes: &scopes,
                mounted_bundles: &bundles,
                serialized_files: &[],
                resource_files: &mounted_resources,
                failed_entries: &[],
                source_container_path: "/test",
                loaded_paths: &std::collections::HashMap::new(),
                limits: SerializedFileWorkspaceLimits::default(),
                bundle_count_limit: 1,
            },
            &mut total_node_path_bytes,
            &mut total_resource_path_bytes,
        )
        .unwrap();

        assert_eq!(workspace.resource_files().len(), 1);
        assert_eq!(
            workspace.resource_files()[0].name_fixed,
            "shared.ress",
            "the fixed name is cached at registration, not per lookup"
        );
        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(0), "ARCHIVE:/request/SHARED.ress")
                .unwrap(),
            Some(0),
            "lookups match through the cached fixed name"
        );
        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(0), "archive:/missing/nope.resS")
                .unwrap(),
            None
        );
    }

    #[test]
    fn platform_inventory_enforces_initial_serialized_file_limit() {
        let mut filesystem = VirtualFileSystem::new();
        let serialized_leaf = make_generation_10_file();
        let webdata = make_webdata_fixture(&[("shared.assets", &serialized_leaf)]);
        insert_file(
            &mut filesystem,
            "/game/Game_Data/globalgamemanagers",
            &make_generation_10_file(),
        );
        insert_file(&mut filesystem, "/game/Game_Data/data.unity3d", &webdata);
        let structure = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: Some("/game/Game_Data".to_owned()),
            data_paths: vec!["/game/Game_Data".to_owned()],
        };

        let error = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions {
                    workspace_limits: SerializedFileWorkspaceLimits {
                        max_files: 1,
                        ..SerializedFileWorkspaceLimits::default()
                    },
                    ..PlatformWorkspaceLoadOptions::default()
                },
            )
            .unwrap_err();

        assert!(matches!(
            error.kind,
            PlatformWorkspaceLoadErrorKind::InitialSerializedFileCountLimitExceeded {
                count: 2,
                limit: 1
            }
        ));
    }

    #[test]
    fn recursively_loads_external_serialized_files_and_resolves_file_scoped_pptrs() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/Data/main.assets",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(
            &mut filesystem,
            "/game/Data/shared.assets",
            &make_generation_10_file_with(&["third.assets"], 1, 42, 42),
        );
        insert_file(
            &mut filesystem,
            "/game/Data/third.assets",
            &make_generation_10_file_with(&["main.assets"], 0, 42, 42),
        );
        let inputs = [input("/game/Data/main.assets")];
        let data_paths = ["/game/Data".to_owned()];

        let (workspace, dependency_report) =
            SerializedFileWorkspace::load_from_vfs_with_dependencies_and_layout_source(
                &mut filesystem,
                &inputs,
                &[],
                SerializedFileWorkspaceLoadOptions {
                    data_paths: &data_paths,
                    limits: SerializedFileWorkspaceLimits::default(),
                    index_limits: DependencyIndexLimits::default(),
                    container_limits: ContainerMountLimits::default(),
                    preexpanded_containers: &[],
                    layouts: &FileTypeTreeTestLayouts,
                },
            )
            .unwrap();

        assert_eq!(workspace.files().len(), 3);
        assert_eq!(dependency_report.loaded_serialized_files, 2);
        assert!(dependency_report.is_complete());
        assert_eq!(
            dependency_report.requests[0].outcome,
            SerializedFileDependencyOutcome::LoadedSerializedFile {
                file_index: 1,
                virtual_path: "/game/Data/shared.assets".to_owned(),
            }
        );
        assert_eq!(
            dependency_report.requests[1].outcome,
            SerializedFileDependencyOutcome::LoadedSerializedFile {
                file_index: 2,
                virtual_path: "/game/Data/third.assets".to_owned(),
            }
        );
        assert_eq!(
            dependency_report.requests[2].outcome,
            SerializedFileDependencyOutcome::AlreadyLoaded { file_index: 0 }
        );

        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(graph.references().len(), 3);
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
        assert_eq!(
            graph.references()[1].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 2,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn dependency_collections_resolve_each_external_identifier_to_its_loaded_file() {
        // The per-identifier collection lookup behind `AssetCollection.Dependencies[1..]` runs
        // the same source-scoped `Bundle.ResolveCollection` walk the joined reference graph
        // installs as its resolver, so a resolved slot names the file a reference row would
        // name, a name no loaded file carries leaves its slot empty, and an identifier naming
        // the source's own scope still reaches that file.
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/Data/main.assets",
            &make_generation_10_file_with(&["shared.assets", "absent.assets"], 1, 10, 42),
        );
        insert_file(
            &mut filesystem,
            "/game/Data/shared.assets",
            &make_generation_10_file_with(&["third.assets"], 1, 42, 42),
        );
        insert_file(
            &mut filesystem,
            "/game/Data/third.assets",
            &make_generation_10_file_with(&["main.assets"], 0, 42, 42),
        );
        let inputs = [input("/game/Data/main.assets")];
        let data_paths = ["/game/Data".to_owned()];

        let (workspace, _dependency_report) =
            SerializedFileWorkspace::load_from_vfs_with_dependencies(
                &mut filesystem,
                &inputs,
                &[],
                &data_paths,
                SerializedFileWorkspaceLimits::default(),
                DependencyIndexLimits::default(),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(
            workspace.resolve_dependency_collections().unwrap(),
            [vec![Some(1), None], vec![Some(2)], vec![Some(0)]]
        );
    }

    #[test]
    fn dependency_container_resource_leaf_attaches_to_its_workspace_bundle() {
        let mut filesystem = VirtualFileSystem::new();
        let serialized_leaf = make_generation_10_file();
        let container = make_webdata_fixture(&[
            ("shared.assets", &serialized_leaf),
            ("stream.resS", b"dependency stream"),
        ]);
        insert_file(
            &mut filesystem,
            "/game/main.assets",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(&mut filesystem, "/game/shared.assets", &container);
        let inputs = [input("/game/main.assets")];
        let data_paths = ["/game".to_owned()];

        let (workspace, dependency_report) =
            SerializedFileWorkspace::load_from_vfs_with_dependencies(
                &mut filesystem,
                &inputs,
                &[],
                &data_paths,
                SerializedFileWorkspaceLimits::default(),
                DependencyIndexLimits::default(),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert!(dependency_report.is_complete());
        assert_eq!(workspace.files().len(), 2);
        assert_eq!(workspace.container_bundles().len(), 1);
        assert_eq!(
            workspace.container_bundles()[0].direct_collection_file_indices,
            [1]
        );
        assert_eq!(
            workspace.container_bundles()[0].direct_resource_file_indices,
            [0]
        );
        assert_eq!(
            workspace.resource_files(),
            [super::WorkspaceResourceFile {
                virtual_path: "/__assetripper_dependency_mount_0/stream.resS".to_owned(),
                name: "stream.resS".to_owned(),
                name_fixed: "stream.ress".to_owned(),
                container_bundle_index: 0,
                container_entry_index: 1,
                container_resource_index: 0,
            }]
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/__assetripper_dependency_mount_0/stream.resS", 64)
                .unwrap(),
            b"dependency stream"
        );
    }

    #[test]
    fn keeps_missing_ambiguous_and_resource_dependency_results_explicit() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/Data/main.assets",
            &make_generation_10_file_with(
                &[
                    "texture.resS",
                    "dup.assets",
                    "missing.assets",
                    "archive.webdata",
                ],
                0,
                42,
                42,
            ),
        );
        insert_file(&mut filesystem, "/game/Data/texture.resS", &[1, 2, 3]);
        insert_file(&mut filesystem, "/bundles/a/dup.assets", &[1, 2, 3]);
        insert_file(&mut filesystem, "/bundles/b/dup.assets", &[4, 5, 6]);
        let mut webdata_container = b"UnityWebData1.0\0".to_vec();
        webdata_container.extend_from_slice(&20i32.to_le_bytes());
        insert_file(
            &mut filesystem,
            "/game/Data/archive.webdata",
            &webdata_container,
        );
        let inputs = [input("/game/Data/main.assets")];
        let data_paths = ["/game/Data".to_owned()];

        let (workspace, dependency_report) =
            SerializedFileWorkspace::load_from_vfs_with_dependencies(
                &mut filesystem,
                &inputs,
                &[],
                &data_paths,
                SerializedFileWorkspaceLimits::default(),
                DependencyIndexLimits::default(),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(workspace.files().len(), 1);
        assert!(!dependency_report.is_complete());
        assert!(matches!(
            dependency_report.requests[0].outcome,
            SerializedFileDependencyOutcome::ResourceCandidate { .. }
        ));
        assert!(matches!(
            &dependency_report.requests[1].outcome,
            SerializedFileDependencyOutcome::Ambiguous {
                candidate_count: 2,
                candidate_paths,
            } if candidate_paths == &["/bundles/a/dup.assets", "/bundles/b/dup.assets"]
        ));
        assert_eq!(
            dependency_report.requests[2].outcome,
            SerializedFileDependencyOutcome::Missing
        );
        assert_eq!(
            dependency_report.requests[3].outcome,
            SerializedFileDependencyOutcome::ContainerExpanded {
                virtual_path: "/game/Data/archive.webdata".to_owned(),
                kind: FileKind::UnityWebData,
                mounted_entries: 0,
                mounted_bytes: 0,
                serialized_files_loaded: 0,
                matching_serialized_files: 0,
            }
        );
    }

    #[test]
    fn expands_dependency_containers_and_loads_their_serialized_file_leaves() {
        let mut filesystem = VirtualFileSystem::new();
        let serialized_leaf = make_generation_10_file();
        let container = make_webdata_fixture(&[("shared.assets", &serialized_leaf)]);
        insert_file(
            &mut filesystem,
            "/game/main.assets",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(&mut filesystem, "/game/shared.assets", &container);
        let inputs = [input("/game/main.assets")];
        let data_paths = ["/game".to_owned()];

        let (workspace, dependency_report) =
            SerializedFileWorkspace::load_from_vfs_with_dependencies_and_layout_source(
                &mut filesystem,
                &inputs,
                &[],
                SerializedFileWorkspaceLoadOptions {
                    data_paths: &data_paths,
                    limits: SerializedFileWorkspaceLimits::default(),
                    index_limits: DependencyIndexLimits::default(),
                    container_limits: ContainerMountLimits::default(),
                    preexpanded_containers: &[],
                    layouts: &FileTypeTreeTestLayouts,
                },
            )
            .unwrap();

        assert_eq!(workspace.files().len(), 2);
        assert_eq!(workspace.files()[1].file_name, "shared.assets");
        assert_eq!(
            workspace.files()[1].container_scope.as_deref(),
            Some("/__assetripper_dependency_mount_0")
        );
        assert_eq!(workspace.files()[1].container_entry_index, Some(0));
        assert_eq!(workspace.files()[1].container_collection_index, Some(0));
        assert_eq!(workspace.files()[1].container_bundle_index, Some(0));
        assert_eq!(
            workspace
                .ordered_collection_file_indices("/__assetripper_dependency_mount_0")
                .unwrap(),
            [1]
        );
        assert_eq!(workspace.container_bundles().len(), 1);
        assert_eq!(
            workspace.container_bundles()[0].scope_path.as_deref(),
            Some("/__assetripper_dependency_mount_0")
        );
        assert_eq!(
            workspace.container_bundles()[0].direct_collection_file_indices,
            [1]
        );
        assert!(dependency_report.is_complete());
        assert_eq!(
            dependency_report.requests[0].outcome,
            SerializedFileDependencyOutcome::ContainerExpanded {
                virtual_path: "/game/shared.assets".to_owned(),
                kind: FileKind::UnityWebData,
                mounted_entries: 1,
                mounted_bytes: u64::try_from(serialized_leaf.len()).unwrap(),
                serialized_files_loaded: 1,
                matching_serialized_files: 1,
            }
        );
        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(graph.references().len(), 2);
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn enforces_aggregate_dependency_container_input_entry_and_output_limits() {
        let inputs = [input("/game/main.assets")];
        let data_paths = ["/game".to_owned()];

        let mut filesystem = dependency_container_filesystem();
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &mut filesystem,
            &inputs,
            &[],
            &data_paths,
            SerializedFileWorkspaceLimits {
                max_total_dependency_container_input_bytes: 1,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalDependencyContainerInputBytesLimitExceeded {
                limit: 1,
                ..
            }
        ));
        assert!(!filesystem
            .directory_exists(Some("/__assetripper_dependency_mount_0"))
            .unwrap());

        let mut filesystem = dependency_container_filesystem();
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &mut filesystem,
            &inputs,
            &[],
            &data_paths,
            SerializedFileWorkspaceLimits {
                max_total_dependency_container_entries: 0,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::ContainerMount(ref mount_error)
                if matches!(
                    mount_error.kind,
                    ContainerMountErrorKind::EntryCountLimitExceeded { limit: 0, .. }
                )
        ));

        let mut filesystem = dependency_container_filesystem();
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &mut filesystem,
            &inputs,
            &[],
            &data_paths,
            SerializedFileWorkspaceLimits {
                max_total_dependency_container_output_bytes: 1,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::ContainerMount(ref mount_error)
                if matches!(
                    mount_error.kind,
                    ContainerMountErrorKind::TotalOutputLimitExceeded { limit: 1, .. }
                )
        ));
    }

    #[test]
    fn bounds_dependency_request_walk() {
        let bytes = make_generation_10_file_with(&["shared.assets"], 1, 10, 42);
        let mut filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &mut filesystem,
            &inputs,
            &[],
            &[],
            SerializedFileWorkspaceLimits {
                max_dependency_requests: 0,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();
        assert_eq!(error.report.completed_files, 1);
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::DependencyRequestLimitExceeded { count: 1, limit: 0 }
        ));
    }

    #[test]
    fn applies_aggregate_object_limits_to_recursively_loaded_dependencies() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/main.assets",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(
            &mut filesystem,
            "/game/shared.assets",
            &make_generation_10_file(),
        );
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs_with_dependencies(
            &mut filesystem,
            &inputs,
            &[],
            &[],
            SerializedFileWorkspaceLimits {
                max_total_objects: 1,
                ..SerializedFileWorkspaceLimits::default()
            },
            DependencyIndexLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap_err();

        assert_eq!(error.report.completed_files, 1);
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalObjectLimitExceeded {
                requested_total: 1,
                limit: 0
            }
        ));
    }

    #[test]
    fn preflights_aggregate_input_before_loading_any_file() {
        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits {
                max_total_input_bytes: 1,
                ..SerializedFileWorkspaceLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.report.completed_files, 0);
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalInputLimitExceeded { limit: 1, .. }
        ));
    }

    #[test]
    fn rejects_nonserialized_files_and_object_count_overruns_with_context() {
        let resource_filesystem = virtual_file_system("/game/resource.resS", &[1, 2, 3]);
        let resource_inputs = [input("/game/resource.resS")];
        let error = SerializedFileWorkspace::load_from_vfs(
            &resource_filesystem,
            &resource_inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap_err();
        assert_eq!(error.input_path.as_deref(), Some("/game/resource.resS"));
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::NotSerializedFile {
                detected_kind: FileKind::Resource
            }
        ));

        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits {
                max_total_objects: 0,
                ..SerializedFileWorkspaceLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.report.completed_files, 0);
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalObjectLimitExceeded {
                requested_total: 1,
                limit: 0
            }
        ));
    }
