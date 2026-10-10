    use std::io::Read;

    use super::{
        append_mounted_container_bundles, collection_lookup_candidates,
        engine_resource_collection_alias, fix_file_identifier, SerializedFileDependencyOutcome,
        SerializedFileWorkspace, SerializedFileWorkspaceErrorKind, SerializedFileWorkspaceLimits,
        SerializedFileWorkspaceLoadOptions, SerializedFileWorkspaceReport,
        WorkspaceSerializedBundle,
    };
    use crate::io::dependency::{DependencyFilePath, DependencyIndexLimits};
    use crate::io::filesystem::VirtualFileSystem;
    use crate::io::platform_file_inventory::{
        PlatformWorkspaceLoadErrorKind, PlatformWorkspaceLoadOptions,
    };
    use crate::io::platform_structure::{PlatformKind, PlatformStructure};
    use crate::processing::asset_factory::FileTypeTreeTestLayouts;
    use crate::processing::managed_assembly::{ManagedAssemblyCatalog, ManagedAssemblyLimits};
    use crate::processing::managed_assembly_schema::ManagedSchemaLimits;
    use crate::scheme::{
        ContainerMountErrorKind, ContainerMountLimits, FileKind, MountedContainerBundle,
        MountedResourceFile,
    };
    use crate::serialized_file::collection_workspace::{
        WorkspaceAssetCollections, WorkspaceCollectionErrorKind, WorkspaceCollectionLimits,
    };
    use crate::serialized_file::file::SerializedFile;
    use crate::serialized_file::object::SerializedValue;
    use crate::serialized_file::object_workspace::{
        ManagedBehaviourRecoveryOutcome, ObjectWorkspace, ObjectWorkspaceErrorKind,
        ObjectWorkspaceLimits,
    };
    use crate::serialized_file::reference_graph::{
        MultiFilePPtrResolution, MultiFileReferenceGraphLimits, ObjectReferenceGraph,
    };

    #[test]
    fn collection_lookup_candidates_follow_the_resolve_collection_fallback() {
        use crate::io::special_names::{
            BUILTIN_EXTRA_NAME_1, BUILTIN_EXTRA_NAME_2, DEFAULT_RESOURCE_NAME_1,
            DEFAULT_RESOURCE_NAME_2,
        };
        // Normalized non-engine names try only themselves; the source would repeat the same
        // walk, so the fixed stage is skipped.
        assert_eq!(
            collection_lookup_candidates("shared.assets", "shared.assets"),
            [Some("shared.assets"), None, None]
        );
        // Non-normalized names try the raw request before the fixed form.
        assert_eq!(
            collection_lookup_candidates("LIBRARY/Shared.Assets", "shared.assets"),
            [Some("LIBRARY/Shared.Assets"), Some("shared.assets"), None]
        );
        // Engine names add the space/underscore twin in both directions.
        assert_eq!(
            collection_lookup_candidates(DEFAULT_RESOURCE_NAME_1, DEFAULT_RESOURCE_NAME_1),
            [
                Some(DEFAULT_RESOURCE_NAME_1),
                None,
                Some(DEFAULT_RESOURCE_NAME_2)
            ]
        );
        assert_eq!(
            collection_lookup_candidates(DEFAULT_RESOURCE_NAME_2, DEFAULT_RESOURCE_NAME_2),
            [
                Some(DEFAULT_RESOURCE_NAME_2),
                None,
                Some(DEFAULT_RESOURCE_NAME_1)
            ]
        );
        assert_eq!(
            collection_lookup_candidates("library/unity builtin extra", "unity builtin extra"),
            [
                Some("library/unity builtin extra"),
                Some("unity builtin extra"),
                Some(BUILTIN_EXTRA_NAME_2)
            ]
        );
        assert_eq!(
            collection_lookup_candidates(BUILTIN_EXTRA_NAME_1, BUILTIN_EXTRA_NAME_1),
            [Some(BUILTIN_EXTRA_NAME_1), None, Some(BUILTIN_EXTRA_NAME_2)]
        );
        // The editor resources name and ordinary names have no alias.
        assert_eq!(
            engine_resource_collection_alias("unity editor resources"),
            None
        );
        assert_eq!(engine_resource_collection_alias("shared.assets"), None);
        // The fixed form feeds the alias even when the request carried a folder prefix.
        let fixed = fix_file_identifier("resources/unity_default_resources");
        assert_eq!(fixed, DEFAULT_RESOURCE_NAME_2);
        assert_eq!(
            collection_lookup_candidates("resources/unity_default_resources", &fixed),
            [
                Some("resources/unity_default_resources"),
                Some(DEFAULT_RESOURCE_NAME_2),
                Some(DEFAULT_RESOURCE_NAME_1)
            ]
        );
    }

    #[test]
    fn bundled_resource_resolution_matches_bundle_scope_order_and_name_rules() {
        use super::WorkspaceResourceFile;

        let workspace = SerializedFileWorkspace {
            container_bundles: vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/root".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![0],
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/root/first".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(1),
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![1],
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/root/second".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(2),
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![2, 4],
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/root/first/nested".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(1),
                    parent_entry_index: Some(4),
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![3],
                },
            ],
            resource_files: vec![
                WorkspaceResourceFile {
                    virtual_path: "/root/from-parent.resS".to_owned(),
                    name: "from-parent.resS".to_owned(),
                    name_fixed: "from-parent.ress".to_owned(),
                    container_bundle_index: 0,
                    container_entry_index: 0,
                    container_resource_index: 0,
                },
                WorkspaceResourceFile {
                    virtual_path: "/root/first/archive:/textures/Shared.resS".to_owned(),
                    name: "archive:/textures/Shared.resS".to_owned(),
                    name_fixed: "shared.ress".to_owned(),
                    container_bundle_index: 1,
                    container_entry_index: 0,
                    container_resource_index: 0,
                },
                WorkspaceResourceFile {
                    virtual_path: "/root/second/from-parent.resS".to_owned(),
                    name: "from-parent.resS".to_owned(),
                    name_fixed: "from-parent.ress".to_owned(),
                    container_bundle_index: 2,
                    container_entry_index: 0,
                    container_resource_index: 0,
                },
                WorkspaceResourceFile {
                    virtual_path: "/root/first/nested/archive:/other/Shared.resS".to_owned(),
                    name: "archive:/other/Shared.resS".to_owned(),
                    name_fixed: "shared.ress".to_owned(),
                    container_bundle_index: 3,
                    container_entry_index: 0,
                    container_resource_index: 0,
                },
                WorkspaceResourceFile {
                    virtual_path: "/root/second/sibling.resS".to_owned(),
                    name: "sibling.resS".to_owned(),
                    name_fixed: "sibling.ress".to_owned(),
                    container_bundle_index: 2,
                    container_entry_index: 1,
                    container_resource_index: 1,
                },
            ],
            ..SerializedFileWorkspace::default()
        };

        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(1), "ARCHIVE:/request/SHARED.ress")
                .unwrap(),
            Some(1),
            "the current bundle's direct resource precedes its child bundle"
        );
        let resolved_resource = workspace
            .resolve_bundled_resource_file(Some(1), "ARCHIVE:/request/SHARED.ress")
            .unwrap()
            .unwrap();
        let mut filesystem = VirtualFileSystem::new();
        filesystem
            .create_directory(Some("/root/first/archive:/textures"))
            .unwrap();
        filesystem
            .write_all_bytes(
                &workspace.resource_files[resolved_resource].virtual_path,
                b"012345",
            )
            .unwrap();
        assert_eq!(
            crate::resource::read_streamed_resource_bytes(
                &filesystem,
                &workspace.resource_files[resolved_resource].virtual_path,
                1,
                3,
                crate::resource::StreamedResourceLimits::default(),
            )
            .unwrap(),
            Some(b"123".to_vec())
        );
        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(1), "from-parent.resS")
                .unwrap(),
            Some(0),
            "the parent bundle's direct resource precedes a sibling child"
        );
        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(1), "sibling.resS")
                .unwrap(),
            Some(4),
            "the parent's other direct child is searched after its own resources"
        );
        assert_eq!(
            workspace
                .resolve_bundled_resource_file(Some(1), "archive:/missing/nope.resS")
                .unwrap(),
            None
        );
    }

    #[test]
    fn resource_hierarchy_walk_reports_parent_cycles() {
        use super::WorkspaceResourceLookupError;

        // One step per level plus the root-scope step always suffices.
        assert_eq!(super::bundle_walk_step_limit(0), 1);
        assert_eq!(super::bundle_walk_step_limit(2), 3);
        // Two bundles parenting each other never reach the root scope: the walk
        // reports the cycle instead of hanging (the source would loop forever).
        let workspace = SerializedFileWorkspace {
            container_bundles: vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/a".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(1),
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: Vec::new(),
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/b".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: Vec::new(),
                },
            ],
            resource_files: Vec::new(),
            ..SerializedFileWorkspace::default()
        };
        assert!(matches!(
            workspace.resolve_bundled_resource_file(Some(0), "missing.ress"),
            Err(WorkspaceResourceLookupError::BundleParentCycle)
        ));
    }

    #[test]
    fn unified_resource_resolution_prefers_bundle_then_platform_then_mixed_provider() {
        use super::WorkspaceResourceFile;
        use crate::io::platform_structure::{PlatformResourceProvider, PlatformResourceSource};
        use crate::resource::{resolve_resource_file, ResolvedResourceFile};

        let workspace = SerializedFileWorkspace {
            container_bundles: vec![WorkspaceSerializedBundle {
                scope_path: Some("/bundle".to_owned()),
                name_fixed: None,
                parent_bundle_index: None,
                parent_entry_index: None,
                direct_collection_file_indices: Vec::new(),
                direct_resource_file_indices: vec![0],
            }],
            resource_files: vec![WorkspaceResourceFile {
                virtual_path: "/bundle/shared.resS".to_owned(),
                name: "shared.resS".to_owned(),
                name_fixed: "shared.ress".to_owned(),
                container_bundle_index: 0,
                container_entry_index: 0,
                container_resource_index: 0,
            }],
            ..SerializedFileWorkspace::default()
        };
        let mut filesystem = VirtualFileSystem::new();
        for (path, bytes) in [
            ("/bundle/shared.resS", b"bundle".as_slice()),
            ("/platform/shared.resS", b"platform".as_slice()),
            ("/mixed/shared.resS", b"mixed".as_slice()),
            ("/platform/external.resS", b"external".as_slice()),
            ("/mixed/external.resS", b"mixed-external".as_slice()),
            ("/mixed/mixed-only.resS", b"mixed-only".as_slice()),
        ] {
            let parent = path.rsplit_once('/').unwrap().0;
            filesystem
                .create_directory(Some(parent))
                .unwrap_or_else(|error| {
                    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
                });
            filesystem.write_all_bytes(path, bytes).unwrap();
        }
        let platform = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/platform".to_owned(),
            consumed_paths: vec!["/platform".to_owned()],
            root_path: "/platform".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: None,
            data_paths: vec!["/platform/missing".to_owned(), "/platform".to_owned()],
        };
        let mixed = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/mixed".to_owned(),
            consumed_paths: vec!["/mixed".to_owned()],
            root_path: "/mixed".to_owned(),
            name: Some("Mixed".to_owned()),
            game_data_path: None,
            data_paths: vec!["/mixed".to_owned()],
        };
        let provider = PlatformResourceProvider::new(Some(&platform), Some(&mixed));

        assert_eq!(
            resolve_resource_file(
                &filesystem,
                &workspace,
                None,
                "shared.resS",
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::Bundled {
                resource_index: 0,
                virtual_path: "/bundle/shared.resS",
            },
            "mounted bundle resources precede both external providers"
        );
        assert_eq!(
            resolve_resource_file(
                &filesystem,
                &workspace,
                None,
                "external.resS",
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::External {
                virtual_path: "/platform/external.resS".to_owned(),
                source: PlatformResourceSource::Platform,
                data_path_index: 1,
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            },
            "platform provider precedes Mixed when both contain the resource"
        );
        assert_eq!(
            resolve_resource_file(
                &filesystem,
                &workspace,
                None,
                "mixed-only.resS",
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::External {
                virtual_path: "/mixed/mixed-only.resS".to_owned(),
                source: PlatformResourceSource::Mixed,
                data_path_index: 0,
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            },
            "Mixed is searched only after platform DataPaths"
        );
        assert_eq!(
            resolve_resource_file(
                &filesystem,
                &workspace,
                None,
                "absent.resS",
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::Missing {
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            }
        );
    }

    #[test]
    fn explicit_root_resource_precedes_root_sibling_bundle_and_external_provider() {
        use super::{WorkspaceResourceFile, WorkspaceRootResourceFile};
        use crate::io::platform_structure::PlatformResourceProvider;
        use crate::resource::{
            resolve_resource_file, resolve_resource_file_with_root_catalog,
            ExplicitRootResourceInput, ResolvedResourceFile, RootResourceCatalogLimits,
            RootResourceFileCatalog,
        };
        use crate::scheme::{SchemeReader, SchemeReaderLimits};

        let workspace = SerializedFileWorkspace {
            container_bundles: vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/source".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: Vec::new(),
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/sibling".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![0],
                },
            ],
            resource_files: vec![WorkspaceResourceFile {
                virtual_path: "/sibling/shared.resS".to_owned(),
                name: "shared.resS".to_owned(),
                name_fixed: "shared.ress".to_owned(),
                container_bundle_index: 1,
                container_entry_index: 0,
                container_resource_index: 0,
            }],
            ..SerializedFileWorkspace::default()
        };
        let mut filesystem = VirtualFileSystem::new();
        for path in [
            "/sibling/shared.resS",
            "/explicit/shared.resS",
            "/platform/shared.resS",
        ] {
            let parent = path.rsplit_once('/').unwrap().0;
            filesystem
                .create_directory(Some(parent))
                .unwrap_or_else(|error| {
                    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
                });
            filesystem.write_all_bytes(path, b"resource bytes").unwrap();
        }
        let dispatched =
            SchemeReader::read_with_limits(b"explicit resource", SchemeReaderLimits::default())
                .unwrap();
        let root_inputs = [ExplicitRootResourceInput {
            file_name: "SHARED.resS",
            virtual_path: "/explicit/shared.resS",
            dispatched_file: &dispatched,
        }];
        let root_catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &root_inputs,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(
            root_catalog.files(),
            [WorkspaceRootResourceFile {
                name: "SHARED.resS".to_owned(),
                virtual_path: "/explicit/shared.resS".to_owned(),
            }]
        );
        let platform = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/platform".to_owned(),
            consumed_paths: vec!["/platform".to_owned()],
            root_path: "/platform".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: None,
            data_paths: vec!["/platform".to_owned()],
        };
        let provider = PlatformResourceProvider::new(Some(&platform), None);

        assert_eq!(
            resolve_resource_file(
                &filesystem,
                &workspace,
                Some(0),
                "shared.resS",
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::Bundled {
                resource_index: 0,
                virtual_path: "/sibling/shared.resS",
            },
            "the legacy bundle-only lookup reaches the sibling after the owner branch"
        );
        assert_eq!(
            resolve_resource_file_with_root_catalog(
                &filesystem,
                &workspace,
                Some(0),
                "shared.resS",
                &root_catalog,
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::ExplicitRoot {
                resource_index: 0,
                virtual_path: "/explicit/shared.resS",
            },
            "GameBundle root resources precede sibling Bundles and the external provider"
        );
        assert_eq!(
            resolve_resource_file_with_root_catalog(
                &filesystem,
                &workspace,
                Some(0),
                "other.resS",
                &root_catalog,
                &provider,
                crate::io::platform_structure::PlatformStructureLimits::default(),
            )
            .unwrap(),
            ResolvedResourceFile::Missing {
                unavailable_data_paths: Vec::new(),
            }
        );
    }

    fn put_u16_le(output: &mut Vec<u8>, value: u16) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_i32_le(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32_le(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn put_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        put_u16_le(output, 1);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, 0);
    }

    fn make_generation_10_file() -> Vec<u8> {
        make_generation_10_file_with(&[], 0, 42, 42)
    }

    fn make_generation_10_file_with(
        external_names: &[&str],
        pointer_file_id: i32,
        object_path_id: i32,
        pointer_path_id: i32,
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, "Root");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let value_name = push_type_tree_string(&mut string_buffer, "m_Value");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let target_name = push_type_tree_string(&mut string_buffer, "m_Target");
        let pptr_type = push_type_tree_string(&mut string_buffer, "PPtr<GameObject>");
        let file_id_name = push_type_tree_string(&mut string_buffer, "m_FileID");
        let int32_type = push_type_tree_string(&mut string_buffer, "SInt32");
        let path_id_name = push_type_tree_string(&mut string_buffer, "m_PathID");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 5);
        put_i32_le(&mut metadata, string_buffer.len() as i32);
        put_node(&mut metadata, 0, root_type, empty_name, -1, 0);
        put_node(&mut metadata, 1, int_type, value_name, 4, 1);
        put_node(&mut metadata, 1, pptr_type, target_name, 8, 2);
        put_node(&mut metadata, 2, int32_type, file_id_name, 4, 3);
        put_node(&mut metadata, 2, int32_type, path_id_name, 4, 4);
        metadata.extend_from_slice(&string_buffer);

        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, object_path_id);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 12);
        put_i32_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
        for external_name in external_names {
            metadata.push(0); // empty AssetPath
            for word in [0, 0, 0, 0] {
                put_u32_le(&mut metadata, word);
            }
            put_i32_le(&mut metadata, 0); // serialized AssetType
            metadata.extend_from_slice(external_name.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let mut payload = Vec::new();
        put_i32_le(&mut payload, 0x1234_5678);
        put_i32_le(&mut payload, pointer_file_id);
        put_i32_le(&mut payload, pointer_path_id);
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as i32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_webdata_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let header_size = 20usize
            + entries
                .iter()
                .map(|(name, _)| 12usize + name.len())
                .sum::<usize>();
        let mut container = b"UnityWebData1.0\0".to_vec();
        container.extend_from_slice(&i32::try_from(header_size).unwrap().to_le_bytes());
        let mut data_offset = header_size;
        for (name, bytes) in entries {
            assert!(name.len() < 1024 * 1024);
            container.extend_from_slice(&i32::try_from(data_offset).unwrap().to_le_bytes());
            container.extend_from_slice(&i32::try_from(bytes.len()).unwrap().to_le_bytes());
            container.extend_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
            container.extend_from_slice(name.as_bytes());
            data_offset += bytes.len();
        }
        for (_, bytes) in entries {
            container.extend_from_slice(bytes);
        }
        container
    }

    fn virtual_file_system(path: &str, bytes: &[u8]) -> VirtualFileSystem {
        let mut filesystem = VirtualFileSystem::new();
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, bytes).unwrap();
        filesystem
    }

    fn insert_file(filesystem: &mut VirtualFileSystem, path: &str, bytes: &[u8]) {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, bytes).unwrap();
    }

    fn dependency_container_filesystem() -> VirtualFileSystem {
        let mut filesystem = VirtualFileSystem::new();
        let leaf = make_generation_10_file();
        let container = make_webdata_fixture(&[("shared.assets", &leaf)]);
        insert_file(
            &mut filesystem,
            "/game/main.assets",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(&mut filesystem, "/game/shared.assets", &container);
        filesystem
    }

    fn input(path: &str) -> DependencyFilePath {
        DependencyFilePath {
            file_name: path.rsplit('/').next().unwrap().to_owned(),
            virtual_path: path.to_owned(),
        }
    }

    #[derive(Debug)]
    struct SourceFactoryWorkspaceExpectedObject {
        class_id: i32,
        path_id: i64,
        source_type: String,
        source_name: String,
        source_structure: String,
        payload_bytes: usize,
    }

    #[derive(Debug)]
    struct SourceFactoryWorkspaceExpectedFile {
        file_name: String,
        unity_version: String,
        editor_mode: bool,
        bytes: Vec<u8>,
        objects: Vec<SourceFactoryWorkspaceExpectedObject>,
    }

    fn read_factory_fixture_u32(cursor: &mut std::io::Cursor<&[u8]>) -> u32 {
        let mut bytes = [0; 4];
        cursor
            .read_exact(&mut bytes)
            .expect("source-factory fixture contains a u32");
        u32::from_le_bytes(bytes)
    }

    fn read_factory_fixture_i32(cursor: &mut std::io::Cursor<&[u8]>) -> i32 {
        let mut bytes = [0; 4];
        cursor
            .read_exact(&mut bytes)
            .expect("source-factory fixture contains an i32");
        i32::from_le_bytes(bytes)
    }

    fn read_factory_fixture_i64(cursor: &mut std::io::Cursor<&[u8]>) -> i64 {
        let mut bytes = [0; 8];
        cursor
            .read_exact(&mut bytes)
            .expect("source-factory fixture contains an i64");
        i64::from_le_bytes(bytes)
    }

    fn read_factory_fixture_string(cursor: &mut std::io::Cursor<&[u8]>) -> String {
        let length = usize::try_from(read_factory_fixture_u32(cursor))
            .expect("fixture string length fits usize");
        assert!(length <= 1024 * 1024, "fixture string is bounded");
        let mut bytes = vec![0; length];
        cursor
            .read_exact(&mut bytes)
            .expect("source-factory fixture contains a complete string");
        String::from_utf8(bytes).expect("source-factory fixture strings are UTF-8")
    }

    fn parse_source_factory_workspace_fixture(
        bytes: &[u8],
    ) -> Vec<SourceFactoryWorkspaceExpectedFile> {
        let decoder = flate2::read::GzDecoder::new(bytes);
        let mut decoded = Vec::new();
        decoder
            .take(64 * 1024 * 1024)
            .read_to_end(&mut decoded)
            .expect("source-factory workspace fixture decompresses within its bound");
        let mut cursor = std::io::Cursor::new(decoded.as_slice());
        let mut magic = [0; 8];
        cursor
            .read_exact(&mut magic)
            .expect("source-factory workspace fixture has a header");
        assert_eq!(&magic, b"ARWFAC01");
        let group_count = usize::try_from(read_factory_fixture_u32(&mut cursor))
            .expect("fixture group count fits usize");
        assert!(group_count <= 100_000, "fixture group count is bounded");
        let mut files = Vec::with_capacity(group_count);
        for _ in 0..group_count {
            let file_name = read_factory_fixture_string(&mut cursor);
            let unity_version = read_factory_fixture_string(&mut cursor);
            let mut editor = [0];
            cursor
                .read_exact(&mut editor)
                .expect("source-factory fixture contains its serialization mode");
            assert!(editor[0] <= 1, "fixture mode is release or editor");
            let file_length = usize::try_from(read_factory_fixture_u32(&mut cursor))
                .expect("serialized file length fits usize");
            assert!(file_length <= 16 * 1024 * 1024, "fixture file is bounded");
            let mut file_bytes = vec![0; file_length];
            cursor
                .read_exact(&mut file_bytes)
                .expect("source-factory fixture contains a complete SerializedFile");
            let object_count = usize::try_from(read_factory_fixture_u32(&mut cursor))
                .expect("fixture object count fits usize");
            assert!(object_count <= 100_000, "fixture object count is bounded");
            let mut objects = Vec::with_capacity(object_count);
            for _ in 0..object_count {
                let class_id = read_factory_fixture_i32(&mut cursor);
                let path_id = read_factory_fixture_i64(&mut cursor);
                let source_type = read_factory_fixture_string(&mut cursor);
                let source_name = read_factory_fixture_string(&mut cursor);
                let source_structure = read_factory_fixture_string(&mut cursor);
                let payload_bytes = usize::try_from(read_factory_fixture_u32(&mut cursor))
                    .expect("fixture payload length fits usize");
                objects.push(SourceFactoryWorkspaceExpectedObject {
                    class_id,
                    path_id,
                    source_type,
                    source_name,
                    source_structure,
                    payload_bytes,
                });
            }
            files.push(SourceFactoryWorkspaceExpectedFile {
                file_name,
                unity_version,
                editor_mode: editor[0] == 1,
                bytes: file_bytes,
                objects,
            });
        }
        assert_eq!(
            cursor.position(),
            u64::try_from(decoded.len()).unwrap(),
            "source-factory fixture has no unaccounted trailing bytes"
        );
        files
    }

    #[test]
    fn source_collection_file_order_matches_root_and_nested_bundle_walk() {
        let mut filesystem = VirtualFileSystem::new();
        for index in 0..6 {
            let path = format!("/game/file_{index}.assets");
            insert_file(
                &mut filesystem,
                &path,
                &make_generation_10_file_with(&[], 0, index + 1, index + 1),
            );
        }
        let inputs = (0..6)
            .map(|index| input(&format!("/game/file_{index}.assets")))
            .collect::<Vec<_>>();
        let mut workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();

        for (file_index, bundle_index) in [(1, 0), (2, 1), (3, 2), (4, 3)] {
            workspace.files[file_index].container_bundle_index = Some(bundle_index);
            workspace.files[file_index].container_collection_index = Some(0);
        }
        workspace.container_bundles = vec![
            WorkspaceSerializedBundle {
                scope_path: None,
                name_fixed: None,
                parent_bundle_index: None,
                parent_entry_index: None,
                direct_collection_file_indices: vec![1],
                direct_resource_file_indices: Vec::new(),
            },
            WorkspaceSerializedBundle {
                scope_path: None,
                name_fixed: None,
                parent_bundle_index: Some(0),
                parent_entry_index: Some(9),
                direct_collection_file_indices: vec![2],
                direct_resource_file_indices: Vec::new(),
            },
            WorkspaceSerializedBundle {
                scope_path: None,
                name_fixed: None,
                parent_bundle_index: Some(0),
                parent_entry_index: Some(2),
                direct_collection_file_indices: vec![3],
                direct_resource_file_indices: Vec::new(),
            },
            WorkspaceSerializedBundle {
                scope_path: None,
                name_fixed: None,
                parent_bundle_index: None,
                parent_entry_index: None,
                direct_collection_file_indices: vec![4],
                direct_resource_file_indices: Vec::new(),
            },
        ];

        assert_eq!(
            workspace.source_collection_file_indices().unwrap(),
            [5, 0, 4, 1, 3, 2]
        );
    }

    #[test]
    fn object_workspace_retains_factory_failure_and_raw_fallback_ranges() {
        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/main.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let decoded = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();

        assert!(!decoded.report().is_complete());
        assert_eq!(decoded.report().object_count, 1);
        assert_eq!(decoded.report().decoded_objects, 0);
        assert_eq!(decoded.report().undecoded_objects, 1);
        let object = &decoded.objects()[0];
        let source_object = &workspace.files()[0].metadata.objects[0];
        assert_eq!(object.file_index, 0);
        assert_eq!(object.object_index, 0);
        assert_eq!(object.path_id, 42);
        assert_eq!(
            object.serialized_type_index,
            source_object.serialized_type_index
        );
        assert_eq!(object.raw_type_id, source_object.raw_type_id);
        assert_eq!(
            object.resolved_type_index,
            source_object.resolved_type_index
        );
        assert_eq!(object.class_id, Some(1));
        assert_eq!(object.script_type_index, source_object.script_type_index);
        assert_eq!(object.is_destroyed, source_object.is_destroyed);
        assert_eq!(object.is_stripped, source_object.is_stripped);
        assert_eq!(
            object.factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Unreadable
        );
        assert!(object.payload.is_err());

        // This deliberately malformed class-1 fixture still exercises the generic reader
        // directly; the product ObjectWorkspace must honor GameAssetFactory's generated
        // GameObject layout instead of trusting the incompatible custom TypeTree.
        let parsed = SerializedFile::parse(&bytes).unwrap();
        let payload = parsed.decode_object_payload(0).unwrap();
        assert_eq!(payload.path_id, 42);
        assert_eq!(
            payload.root,
            SerializedValue::Struct(vec![
                crate::serialized_file::object::SerializedFieldValue {
                    name: "m_Value".to_owned(),
                    value: SerializedValue::I32(0x1234_5678),
                },
                crate::serialized_file::object::SerializedFieldValue {
                    name: "m_Target".to_owned(),
                    value: SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 42,
                    },
                },
            ])
        );

        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        let raw_hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|textasset_notree|"))
            .expect("the source-factory fixture includes a valid no-TypeTree TextAsset");
        let raw_bytes = raw_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let raw_filesystem = virtual_file_system("/game/textasset.assets", &raw_bytes);
        let raw_workspace = SerializedFileWorkspace::load_from_vfs(
            &raw_filesystem,
            &[input("/game/textasset.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let raw_limited = ObjectWorkspace::load_from_vfs(
            &raw_filesystem,
            &raw_workspace,
            ObjectWorkspaceLimits {
                max_total_decoded_allocation_bytes: 0,
                ..ObjectWorkspaceLimits::default()
            },
        )
        .unwrap();
        assert!(!raw_limited.report().is_complete());
        assert_eq!(raw_limited.report().decoded_objects, 0);
        assert_eq!(raw_limited.report().undecoded_objects, 1);
        let raw_length = raw_limited.objects()[0].raw_payload_range.end
            - raw_limited.objects()[0].raw_payload_range.start;
        assert_eq!(
            raw_limited.report().total_undecoded_raw_bytes,
            usize::try_from(raw_length).unwrap()
        );
        assert!(matches!(
            raw_limited.objects()[0].factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Unreadable
        ));
        assert!(
            raw_limited.objects()[0]
                .factory
                .log_message
                .as_deref()
                .is_some_and(|message| message.contains("configured limit 0")),
            "factory diagnostic: {:?}",
            raw_limited.objects()[0].factory.log_message
        );
        let start = usize::try_from(raw_limited.objects()[0].raw_payload_range.start).unwrap();
        let end = usize::try_from(raw_limited.objects()[0].raw_payload_range.end).unwrap();
        let parsed_raw = SerializedFile::parse(&raw_bytes).unwrap();
        let source_range = parsed_raw.metadata.objects[0]
            .data_range(&parsed_raw.header)
            .unwrap();
        assert_eq!(raw_limited.objects()[0].raw_payload_range, source_range);
        let source_start = usize::try_from(source_range.start).unwrap();
        let source_end = usize::try_from(source_range.end).unwrap();
        assert_eq!(&raw_bytes[start..end], &raw_bytes[source_start..source_end]);
    }

    #[test]
    fn object_workspace_replays_source_factory_defaults_at_every_tpk_boundary() {
        let expected_files = parse_source_factory_workspace_fixture(include_bytes!(
            "../processing/source_factory_workspace_fixture.bin.gz"
        ));
        assert_eq!(expected_files.len(), 1168, "reference group count changed");
        let expected_object_count = expected_files
            .iter()
            .map(|file| file.objects.len())
            .sum::<usize>();
        assert_eq!(expected_object_count, 8_922);

        let mut source_payloads = std::collections::HashMap::new();
        let mut source_skip_count = 0usize;
        for (line_index, line) in include_str!("../processing/source_factory_payload_fixture.txt")
            .lines()
            .enumerate()
        {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<_> = line.split('|').collect();
            match fields.first().copied() {
                Some("SKIP") => {
                    assert_eq!(
                        fields.len(),
                        4,
                        "source payload fixture line {}",
                        line_index + 1
                    );
                    source_skip_count += 1;
                }
                Some("PAYLOAD") => {
                    assert_eq!(
                        fields.len(),
                        7,
                        "source payload fixture line {}",
                        line_index + 1
                    );
                    let class_id = fields[1].parse::<i32>().expect("source class ID");
                    let mode = match fields[4] {
                        "release" => false,
                        "editor" => true,
                        other => panic!("unexpected source payload mode `{other}`"),
                    };
                    let byte_count = fields[5]
                        .parse::<usize>()
                        .expect("source payload byte count");
                    let hex = fields[6].as_bytes();
                    assert_eq!(hex.len(), byte_count * 2, "source payload hex length");
                    let bytes = hex
                        .chunks_exact(2)
                        .map(|pair| {
                            let digits =
                                std::str::from_utf8(pair).expect("source payload hex is ASCII");
                            u8::from_str_radix(digits, 16).expect("source payload hex digit")
                        })
                        .collect::<Vec<_>>();
                    assert!(
                        source_payloads
                            .insert(
                                (class_id, fields[2].to_owned(), mode),
                                (fields[3].to_owned(), bytes)
                            )
                            .is_none(),
                        "source payload fixture has a unique class/version/mode key"
                    );
                }
                other => panic!("unexpected source payload fixture row {other:?}"),
            }
        }
        assert_eq!(source_payloads.len(), 8_922);
        assert_eq!(source_skip_count, 539);

        let mut filesystem = VirtualFileSystem::new();
        let mut inputs = Vec::with_capacity(expected_files.len());
        for file in &expected_files {
            let path = format!("/factory-matrix/{}", file.file_name);
            insert_file(&mut filesystem, &path, &file.bytes);
            inputs.push(input(&path));
        }

        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("every source-authored matrix file parses and scans");
        assert_eq!(workspace.files().len(), expected_files.len());
        assert_eq!(workspace.report().total_objects, expected_object_count);
        for (file_index, (file, expected)) in
            workspace.files().iter().zip(&expected_files).enumerate()
        {
            assert_eq!(file.file_name, expected.file_name, "file {file_index}");
            assert_eq!(
                file.metadata.prefix.unity_version_text.as_deref(),
                Some(expected.unity_version.as_str()),
                "file {} version",
                expected.file_name
            );
            assert_eq!(
                file.metadata.prefix.target_platform,
                Some(if expected.editor_mode {
                    crate::serialized_file::object::BUILD_TARGET_NO_TARGET
                } else {
                    19
                }),
                "file {} platform/mode",
                expected.file_name
            );
            assert_eq!(file.metadata.objects.len(), expected.objects.len());
            assert!(file.metadata.types.iter().all(|serialized_type| {
                serialized_type
                    .type_tree
                    .as_ref()
                    .is_none_or(|tree| tree.nodes.is_empty())
            }));
            assert_eq!(
                filesystem
                    .read_all_bytes(&file.virtual_path, usize::MAX)
                    .expect("fixture file remains in the source VFS"),
                expected.bytes,
                "{} source bytes are retained",
                expected.file_name
            );
        }

        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .expect("ObjectWorkspace materializes every source-factory matrix object");
        assert_eq!(objects.report().object_count, expected_object_count);
        assert_eq!(objects.objects().len(), expected_object_count);
        assert_eq!(
            objects.report().decoded_objects + objects.report().undecoded_objects,
            expected_object_count,
            "every object has an explicit materialization outcome"
        );

        let mut expected_full_count = 0usize;
        let mut expected_object_index = 0usize;
        for (file_index, file) in expected_files.iter().enumerate() {
            for (object_index, expected) in file.objects.iter().enumerate() {
                let actual = &objects.objects()[expected_object_index];
                expected_object_index += 1;
                assert_eq!(
                    actual.file_index, file_index,
                    "{} file index",
                    file.file_name
                );
                assert_eq!(
                    actual.object_index, object_index,
                    "{} object index",
                    file.file_name
                );
                assert_eq!(
                    actual.path_id, expected.path_id,
                    "{} PathID",
                    file.file_name
                );
                assert_eq!(
                    actual.factory.class_id, expected.class_id,
                    "{} class",
                    file.file_name
                );
                let actual_payload_bytes =
                    usize::try_from(actual.raw_payload_range.end - actual.raw_payload_range.start)
                        .expect("object payload range fits usize");
                assert_eq!(
                    actual_payload_bytes, expected.payload_bytes,
                    "{} payload size",
                    file.file_name
                );
                let (source_type, source_payload) = source_payloads
                    .remove(&(
                        expected.class_id,
                        file.unity_version.clone(),
                        file.editor_mode,
                    ))
                    .expect("every workspace object has a matching source payload fixture row");
                assert_eq!(
                    source_type, expected.source_type,
                    "{} source type",
                    file.file_name
                );
                let payload_start = usize::try_from(actual.raw_payload_range.start)
                    .expect("object payload start fits usize");
                let payload_end = usize::try_from(actual.raw_payload_range.end)
                    .expect("object payload end fits usize");
                assert_eq!(
                    file.bytes.get(payload_start..payload_end),
                    Some(source_payload.as_slice()),
                    "{} PathID {} preserves the byte-exact source release/editor payload",
                    file.file_name,
                    expected.path_id
                );

                match expected.source_type.as_str() {
                    "UnknownObject" | "RawDataObject" => assert!(
                        matches!(
                            actual.factory.kind,
                            crate::processing::asset_factory::FactoryAssetKind::Unknown
                        ),
                        "{} PathID {}: source type {} vs Rust {:?}",
                        file.file_name,
                        expected.path_id,
                        expected.source_type,
                        actual.factory.kind
                    ),
                    name if name.starts_with("Unreadable") => assert!(
                        matches!(
                            actual.factory.kind,
                            crate::processing::asset_factory::FactoryAssetKind::Unreadable
                        ),
                        "{} PathID {}: source type {} vs Rust {:?}",
                        file.file_name,
                        expected.path_id,
                        expected.source_type,
                        actual.factory.kind
                    ),
                    name if name.contains("TypeTreeObject") => assert!(
                        matches!(
                            actual.factory.kind,
                            crate::processing::asset_factory::FactoryAssetKind::TypeTreeObject
                        ),
                        "{} PathID {}: source type {} vs Rust {:?}",
                        file.file_name,
                        expected.path_id,
                        expected.source_type,
                        actual.factory.kind
                    ),
                    source_type => assert_eq!(
                        actual.factory.kind,
                        crate::processing::asset_factory::FactoryAssetKind::Generated(
                            source_type.to_owned()
                        ),
                        "{} PathID {} source generated type",
                        file.file_name,
                        expected.path_id
                    ),
                }

                if expected.source_name != "NULL" {
                    assert_eq!(
                        actual.factory.name, expected.source_name,
                        "{} PathID {} source name",
                        file.file_name, expected.path_id
                    );
                }

                if !expected.source_structure.is_empty() {
                    let actual_structure = actual.factory.structure.as_ref().map(|structure| {
                        match structure.outcome {
                            crate::processing::asset_factory::MonoBehaviourStructureOutcome::UseParsedStructure => {
                                "SerializableStructure"
                            }
                            crate::processing::asset_factory::MonoBehaviourStructureOutcome::ClearStructure => {
                                "NULL"
                            }
                            crate::processing::asset_factory::MonoBehaviourStructureOutcome::UseUnloadedRemainder => {
                                "UnloadedStructure"
                            }
                            crate::processing::asset_factory::MonoBehaviourStructureOutcome::KeepExisting => {
                                "KeepExisting"
                            }
                        }
                    });
                    assert_eq!(
                        actual_structure,
                        Some(expected.source_structure.as_str()),
                        "{} PathID {} source MonoBehaviour structure",
                        file.file_name,
                        expected.path_id
                    );
                }

                let source_is_incomplete = expected.source_type == "UnknownObject"
                    || expected.source_type == "RawDataObject"
                    || expected.source_type.starts_with("Unreadable")
                    || expected.source_structure == "UnloadedStructure";
                if !source_is_incomplete {
                    expected_full_count += 1;
                    assert!(
                        actual.is_fully_materialized(),
                        "{} PathID {} is complete in the source but incomplete in Rust: {:?}",
                        file.file_name,
                        expected.path_id,
                        actual.factory
                    );
                } else if expected.source_structure == "UnloadedStructure" {
                    assert!(
                        !actual.is_fully_materialized(),
                        "{} PathID {} source UnloadedStructure must remain explicit",
                        file.file_name,
                        expected.path_id
                    );
                }

                match actual.payload.as_ref() {
                    Ok(payload) => {
                        assert_eq!(payload.path_id, expected.path_id);
                        assert_eq!(
                            payload.consumed_bytes + payload.trailing_bytes.len(),
                            expected.payload_bytes,
                            "{} PathID {} decoded range",
                            file.file_name,
                            expected.path_id
                        );
                        if !matches!(
                            actual.factory.kind,
                            crate::processing::asset_factory::FactoryAssetKind::Unreadable
                                | crate::processing::asset_factory::FactoryAssetKind::Unknown
                        ) {
                            let tree = actual
                                .recovered_type_tree
                                .as_ref()
                                .expect("no-TypeTree generated objects retain their selected TPK tree");
                            assert_eq!(
                                tree.nodes.first().and_then(|node| node.type_name.as_deref()),
                                Some(payload.type_name.as_str()),
                                "{} PathID {} selected tree root",
                                file.file_name,
                                expected.path_id
                            );
                        }
                    }
                    Err(error) => assert!(
                        source_is_incomplete,
                        "{} PathID {} has a Rust decode error without a matching source fallback: {error}",
                        file.file_name,
                        expected.path_id
                    ),
                }
            }

            for issue in workspace.files()[file_index].reference_graph.issues() {
                let source_object_index = match issue {
                    crate::serialized_file::reference_graph::ReferenceGraphIssue::ObjectDecodeFailed {
                        object_index,
                        ..
                    }
                    | crate::serialized_file::reference_graph::ReferenceGraphIssue::UnparsedTrailingPayload {
                        object_index,
                        ..
                    }
                    | crate::serialized_file::reference_graph::ReferenceGraphIssue::UnparsedRawValue {
                        object_index,
                        ..
                    }
                    | crate::serialized_file::reference_graph::ReferenceGraphIssue::FieldPathLimitExceeded {
                        object_index,
                        ..
                    }
                    | crate::serialized_file::reference_graph::ReferenceGraphIssue::EdgeLimitExceeded {
                        object_index,
                        ..
                    }
                    | crate::serialized_file::reference_graph::ReferenceGraphIssue::ManagedObjectLimitExceeded {
                        object_index,
                        ..
                    }
                    | crate::serialized_file::reference_graph::ReferenceGraphIssue::ManagedReferenceLimitExceeded {
                        object_index,
                        ..
                    }
                    | crate::serialized_file::reference_graph::ReferenceGraphIssue::ManagedRegistryScopeLimitExceeded {
                        object_index,
                        ..
                    } => *object_index,
                };
                let source_object = &file.objects[source_object_index];
                assert!(
                    source_object.source_type == "UnknownObject"
                        || source_object.source_type == "RawDataObject"
                        || source_object.source_type.starts_with("Unreadable")
                        || source_object.source_structure == "UnloadedStructure",
                    "{} PathID {} has a Rust reference-scan issue without a matching source partial/raw outcome: {issue:?}",
                    file.file_name,
                    source_object.path_id
                );
            }
        }
        assert_eq!(
            objects.report().decoded_objects,
            expected_full_count,
            "ObjectWorkspace completion count matches the source factory outcomes"
        );
        assert!(
            source_payloads.is_empty(),
            "all source payload rows were consumed"
        );
    }
