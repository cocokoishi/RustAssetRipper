    use super::{
        is_compressed_data_bundle_name, is_primary_engine_file, plan_mono_assembly_collection,
        platform_supports_mono_backend, register_assembly, select_scripting_backend,
        MainAssemblyCollection, PlatformFileInventoryErrorKind, PlatformFileInventoryLimits,
        PlatformFileInventoryOptions, ScriptingBackend,
    };
    use crate::io::filesystem::VirtualFileSystem;
    use crate::io::platform_structure::{PlatformKind, PlatformStructure};

    fn structure(
        platform: PlatformKind,
        root: &str,
        data_paths: &[&str],
        game_data_path: Option<&str>,
        name: Option<&str>,
    ) -> PlatformStructure {
        PlatformStructure {
            platform,
            selected_path: root.to_owned(),
            consumed_paths: vec![root.to_owned()],
            root_path: root.to_owned(),
            name: name.map(str::to_owned),
            game_data_path: game_data_path.map(str::to_owned),
            data_paths: data_paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    fn make_directory(filesystem: &mut VirtualFileSystem, path: &str) {
        filesystem.create_directory(Some(path)).unwrap();
    }

    fn write_file(filesystem: &mut VirtualFileSystem, path: &str, bytes: &[u8]) {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        let mut stream = filesystem.create_file(path).unwrap();
        use std::io::Write;
        stream.write_all(bytes).unwrap();
    }

    fn bundle_header() -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[..7].copy_from_slice(b"UnityFS");
        bytes
    }

    #[test]
    fn collects_compressed_and_primary_files_in_data_paths() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/game/Game_Data");
        for name in [
            "data.unity3d",
            "datapack.unity3d",
            "mainData",
            "globalgamemanagers",
            "globalgamemanagers.assets",
            "resources.assets",
            "level2.split0",
            "sharedassets10.assets.split0",
            "unrelated.assets",
        ] {
            write_file(&mut filesystem, &format!("/game/Game_Data/{name}"), b"x");
        }
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Game_Data"],
            Some("/game/Game_Data"),
            Some("Game"),
        );

        let inventory = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                    disable_script_import: false,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        let entries: Vec<_> = inventory
            .registered_files
            .iter()
            .map(|entry| (entry.file_name.as_str(), entry.virtual_path.as_str()))
            .collect();
        assert_eq!(entries.len(), 8);
        assert!(entries.contains(&("data.unity3d", "/game/Game_Data/data.unity3d")));
        assert!(entries.contains(&("level2", "/game/Game_Data/level2")));
        assert!(entries.contains(&(
            "sharedassets10.assets",
            "/game/Game_Data/sharedassets10.assets"
        )));
        assert_eq!(inventory.primary_file_inputs.len(), 6);
        assert!(!inventory
            .primary_file_inputs
            .iter()
            .any(|entry| entry.file_name == "data.unity3d"));
        assert!(!entries.iter().any(|(name, _)| *name == "unrelated.assets"));
    }

    #[test]
    fn split_data_bundle_parts_register_under_their_logical_bundle_name() {
        use super::split_data_bundle_name;
        // The kernel only fires for split parts of the two data bundles.
        assert_eq!(
            split_data_bundle_name("/game/Game_Data/data.unity3d.split0").as_deref(),
            Some("data.unity3d")
        );
        assert_eq!(
            split_data_bundle_name("/game/Game_Data/datapack.unity3d.split12").as_deref(),
            Some("datapack.unity3d")
        );
        assert_eq!(split_data_bundle_name("/game/Game_Data/data.unity3d"), None);
        assert_eq!(
            split_data_bundle_name("/game/Game_Data/datapack.unity3d"),
            None
        );
        assert_eq!(
            split_data_bundle_name("/game/Game_Data/level2.split0"),
            None
        );
        assert_eq!(
            split_data_bundle_name("/game/Game_Data/unrelated.assets"),
            None
        );
        // Non-numeric split suffixes are not split parts on either side.
        assert_eq!(
            split_data_bundle_name("/game/Game_Data/data.unity3d.split"),
            None
        );
        assert_eq!(
            split_data_bundle_name("/game/Game_Data/data.unity3d.splitX"),
            None
        );

        // Split-only data bundles register exactly like literal bundle files.
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/game/Game_Data");
        for name in [
            "data.unity3d.split0",
            "data.unity3d.split1",
            "datapack.unity3d.split0",
            "level2.split0",
            "data.unity3d.splitX",
            "unrelated.assets",
        ] {
            write_file(&mut filesystem, &format!("/game/Game_Data/{name}"), b"x");
        }
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Game_Data"],
            Some("/game/Game_Data"),
            Some("Game"),
        );
        let inventory = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                    disable_script_import: false,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        let entries: Vec<_> = inventory
            .registered_files
            .iter()
            .map(|entry| (entry.file_name.as_str(), entry.virtual_path.as_str()))
            .collect();
        assert!(entries.contains(&("data.unity3d", "/game/Game_Data/data.unity3d")));
        assert!(entries.contains(&("datapack.unity3d", "/game/Game_Data/datapack.unity3d")));
        assert!(entries.contains(&("level2", "/game/Game_Data/level2")));
        assert!(!entries.iter().any(|(name, _)| *name == "unrelated.assets"));
        assert!(!entries.iter().any(|(name, _)| name.contains("split")));
    }

    #[test]
    fn collects_streaming_bundles_and_respects_skip_option() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/game/Game_Data/StreamingAssets/Nested");
        write_file(
            &mut filesystem,
            "/game/Game_Data/StreamingAssets/root.bundle",
            &bundle_header(),
        );
        write_file(
            &mut filesystem,
            "/game/Game_Data/StreamingAssets/Nested/Avatar.unity3d",
            &bundle_header(),
        );
        write_file(
            &mut filesystem,
            "/game/Game_Data/StreamingAssets/readme.txt",
            b"not a bundle",
        );
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Game_Data"],
            Some("/game/Game_Data"),
            Some("Game"),
        );

        let included = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert!(included
            .registered_files
            .iter()
            .any(|entry| entry.file_name == "root"));
        assert!(included
            .registered_files
            .iter()
            .any(|entry| entry.file_name == "avatar"));
        assert_eq!(included.report.inspected_files, 3);

        let skipped = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                    disable_script_import: false,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert!(skipped.registered_files.is_empty());
        assert_eq!(skipped.report.inspected_files, 0);
    }

    #[test]
    fn android_asset_collection_skips_bin_subtree() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/apk/assets/bin/Data");
        make_directory(&mut filesystem, "/apk/assets/aa/sub");
        write_file(
            &mut filesystem,
            "/apk/assets/direct.bundle",
            &bundle_header(),
        );
        write_file(
            &mut filesystem,
            "/apk/assets/aa/sub/content.unity3d",
            &bundle_header(),
        );
        write_file(
            &mut filesystem,
            "/apk/assets/bin/Data/not-collected.bundle",
            &bundle_header(),
        );
        let structure = structure(
            PlatformKind::Android,
            "/apk",
            &["/apk/assets/bin/Data"],
            Some("/apk/assets/bin/Data"),
            None,
        );

        let inventory = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                    disable_script_import: false,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        let names: Vec<_> = inventory
            .registered_files
            .iter()
            .map(|entry| entry.file_name.as_str())
            .collect();
        assert_eq!(names, ["content", "direct"]);
    }

    #[test]
    fn missing_platform_data_path_is_reported() {
        let filesystem = VirtualFileSystem::new();
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Game_Data"],
            Some("/game/Game_Data"),
            Some("Game"),
        );
        let error = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            PlatformFileInventoryErrorKind::DataPathNotFound
        ));
        assert_eq!(error.virtual_path.as_deref(), Some("/game/Game_Data"));
    }

    #[test]
    fn workspace_load_reports_when_the_structure_has_no_primary_serialized_files() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/player");
        write_file(&mut filesystem, "/player/game.unity3d", b"bundle");
        let structure = structure(
            PlatformKind::WebPlayer,
            "/player",
            &["/player"],
            None,
            Some("game"),
        );

        let error = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                super::PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            super::PlatformWorkspaceLoadErrorKind::NoSerializedFileInputs
        ));
    }

    #[test]
    fn duplicate_assembly_names_keep_the_first_path_like_try_add() {
        use std::collections::BTreeMap;
        let mut assemblies = BTreeMap::new();
        let mut duplicates = Vec::new();
        register_assembly(
            &mut assemblies,
            &mut duplicates,
            "A.dll".to_owned(),
            "/first/A.dll".to_owned(),
        );
        register_assembly(
            &mut assemblies,
            &mut duplicates,
            "B.dll".to_owned(),
            "/first/B.dll".to_owned(),
        );
        // Later paths for a repeated name are dropped; the name is reported in scan
        // order while the first path stays registered.
        register_assembly(
            &mut assemblies,
            &mut duplicates,
            "A.dll".to_owned(),
            "/lib/A.dll".to_owned(),
        );
        register_assembly(
            &mut assemblies,
            &mut duplicates,
            "A.dll".to_owned(),
            "/other/A.dll".to_owned(),
        );
        register_assembly(
            &mut assemblies,
            &mut duplicates,
            "B.dll".to_owned(),
            "/lib/B.dll".to_owned(),
        );
        assert_eq!(assemblies.len(), 2);
        assert_eq!(assemblies["A.dll"], "/first/A.dll");
        assert_eq!(assemblies["B.dll"], "/first/B.dll");
        assert_eq!(duplicates, vec!["A.dll", "A.dll", "B.dll"]);
    }

    #[test]
    fn mono_backend_collects_direct_managed_assemblies_only() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/game/Game_Data/Managed/Nested");
        write_file(
            &mut filesystem,
            "/game/Game_Data/Managed/Assembly-CSharp.dll",
            b"assembly",
        );
        write_file(
            &mut filesystem,
            "/game/Game_Data/Managed/Nested/NotCollected.dll",
            b"nested assembly",
        );
        write_file(
            &mut filesystem,
            "/game/Game_Data/Managed/notice.txt",
            b"text",
        );
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Game_Data"],
            Some("/game/Game_Data"),
            Some("Game"),
        );

        let inventory = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                    disable_script_import: false,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert_eq!(inventory.scripting_backend, ScriptingBackend::Mono);
        assert_eq!(inventory.assemblies.len(), 1);
        assert_eq!(inventory.assemblies[0].file_name, "Assembly-CSharp.dll");
        assert_eq!(
            inventory.assemblies[0].virtual_path,
            "/game/Game_Data/Managed/Assembly-CSharp.dll"
        );

        let error = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                    disable_script_import: false,
                },
                PlatformFileInventoryLimits {
                    max_assemblies: 0,
                    ..PlatformFileInventoryLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            PlatformFileInventoryErrorKind::AssemblyCountLimitExceeded { count: 1, limit: 0 }
        ));
    }

    #[test]
    fn disabled_script_import_reports_unknown_and_collects_no_assemblies() {
        let mut filesystem = VirtualFileSystem::new();
        write_file(
            &mut filesystem,
            "/game/Game_Data/Managed/Assembly-CSharp.dll",
            b"managed assembly",
        );
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Game_Data"],
            Some("/game/Game_Data"),
            Some("Game"),
        );

        let inventory = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                    disable_script_import: true,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert_eq!(inventory.scripting_backend, ScriptingBackend::Unknown);
        assert!(inventory.assemblies.is_empty());
    }

    #[test]
    fn backend_falls_back_to_mono_when_either_il2cpp_marker_is_missing() {
        for (assembly_marker, metadata_marker) in [(true, false), (false, true)] {
            let mut filesystem = VirtualFileSystem::new();
            if assembly_marker {
                write_file(&mut filesystem, "/game/GameAssembly.dll", b"player marker");
            }
            if metadata_marker {
                write_file(
                    &mut filesystem,
                    "/game/Game_Data/il2cpp_data/Metadata/global-metadata.dat",
                    b"metadata marker",
                );
            }
            write_file(
                &mut filesystem,
                "/game/Game_Data/Managed/Assembly-CSharp.dll",
                b"managed assembly",
            );
            let structure = structure(
                PlatformKind::Windows,
                "/game",
                &["/game/Game_Data"],
                Some("/game/Game_Data"),
                Some("Game"),
            );

            let inventory = structure
                .collect_game_files(
                    &filesystem,
                    PlatformFileInventoryOptions {
                        skip_streaming_assets: true,
                        disable_script_import: false,
                    },
                    PlatformFileInventoryLimits::default(),
                )
                .unwrap();
            assert_eq!(inventory.scripting_backend, ScriptingBackend::Mono);
            assert_eq!(inventory.assemblies.len(), 1);
        }
    }

    #[test]
    fn switch_never_reports_mono_even_with_managed_assemblies() {
        assert!(platform_supports_mono_backend(PlatformKind::Windows));
        assert!(!platform_supports_mono_backend(PlatformKind::Switch));
        // Any incomplete marker set leaves Switch Unknown: the source ternary has no Mono arm.
        for (assembly_marker, metadata_marker) in [(true, false), (false, true), (false, false)] {
            let mut filesystem = VirtualFileSystem::new();
            if assembly_marker {
                write_file(&mut filesystem, "/switch/exefs/main", b"player marker");
            }
            if metadata_marker {
                write_file(
                    &mut filesystem,
                    "/switch/romfs/Data/Managed/Metadata/global-metadata.dat",
                    b"metadata marker",
                );
            }
            write_file(
                &mut filesystem,
                "/switch/romfs/Data/Managed/Assembly-CSharp.dll",
                b"managed assembly",
            );
            let structure = structure(
                PlatformKind::Switch,
                "/switch",
                &["/switch/romfs/Data"],
                Some("/switch/romfs/Data"),
                Some("switch"),
            );

            let inventory = structure
                .collect_game_files(
                    &filesystem,
                    PlatformFileInventoryOptions {
                        skip_streaming_assets: true,
                        disable_script_import: false,
                    },
                    PlatformFileInventoryLimits::default(),
                )
                .unwrap();
            assert_eq!(inventory.scripting_backend, ScriptingBackend::Unknown);
            assert!(inventory.assemblies.is_empty());
        }
    }

    #[test]
    fn il2cpp_layout_markers_take_precedence_over_managed_dlls() {
        let cases = [
            (
                structure(
                    PlatformKind::Windows,
                    "/win",
                    &["/win/Game_Data"],
                    Some("/win/Game_Data"),
                    Some("Game"),
                ),
                "/win/GameAssembly.dll",
                "/win/Game_Data/il2cpp_data/Metadata/global-metadata.dat",
                "/win/Game_Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Linux,
                    "/linux",
                    &["/linux/Game_Data"],
                    Some("/linux/Game_Data"),
                    Some("Game"),
                ),
                "/linux/GameAssembly.so",
                "/linux/Game_Data/il2cpp_data/Metadata/global-metadata.dat",
                "/linux/Game_Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Mac,
                    "/Game.app",
                    &[
                        "/Game.app/Contents/Resources/Data",
                        "/Game.app/Contents/Resources",
                    ],
                    Some("/Game.app/Contents/Resources/Data"),
                    Some("Game"),
                ),
                "/Game.app/Contents/Frameworks/GameAssembly.dylib",
                "/Game.app/Contents/Resources/Data/il2cpp_data/Metadata/global-metadata.dat",
                "/Game.app/Contents/Resources/Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Android,
                    "/apk",
                    &["/apk/assets/bin/Data"],
                    Some("/apk/assets/bin/Data"),
                    None,
                ),
                "/apk/lib/arm64-v8a/libil2cpp.so",
                "/apk/assets/bin/Data/Managed/Metadata/global-metadata.dat",
                "/apk/assets/bin/Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Ios,
                    "/ipa",
                    &["/ipa/Payload/Game.app/Data"],
                    Some("/ipa/Payload/Game.app/Data"),
                    Some("Game"),
                ),
                "/ipa/Payload/Game.app/Game",
                "/ipa/Payload/Game.app/Data/Managed/Metadata/global-metadata.dat",
                "/ipa/Payload/Game.app/Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Switch,
                    "/switch",
                    &["/switch/romfs/Data"],
                    Some("/switch/romfs/Data"),
                    Some("switch"),
                ),
                "/switch/exefs/main",
                "/switch/romfs/Data/Managed/Metadata/global-metadata.dat",
                "/switch/romfs/Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Ps4,
                    "/ps4",
                    &["/ps4/Media"],
                    Some("/ps4/Media"),
                    Some("ps4"),
                ),
                "/ps4/Media/Modules/Il2CppUserAssemblies.prx",
                "/ps4/Media/Metadata/global-metadata.dat",
                "/ps4/Media/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::WindowsPhone,
                    "/wp",
                    &["/wp/Data"],
                    Some("/wp/Data"),
                    Some("wp"),
                ),
                "/wp/GameAssembly.dll",
                "/wp/Data/il2cpp_data/Metadata/global-metadata.dat",
                "/wp/Assembly-CSharp.dll",
            ),
        ];

        for (platform_structure, game_assembly, metadata, managed_assembly) in cases {
            let mut filesystem = VirtualFileSystem::new();
            write_file(&mut filesystem, game_assembly, b"native player marker");
            write_file(&mut filesystem, metadata, b"metadata marker");
            write_file(&mut filesystem, managed_assembly, b"managed assembly");
            let inventory = platform_structure
                .collect_game_files(
                    &filesystem,
                    PlatformFileInventoryOptions {
                        skip_streaming_assets: true,
                        disable_script_import: false,
                    },
                    PlatformFileInventoryLimits::default(),
                )
                .unwrap();
            assert_eq!(inventory.scripting_backend, ScriptingBackend::Il2Cpp);
            assert!(inventory.assemblies.is_empty());
        }
    }

    #[test]
    fn wiiu_remains_mono_even_when_other_platform_il2cpp_markers_exist() {
        let mut filesystem = VirtualFileSystem::new();
        write_file(
            &mut filesystem,
            "/wii/exefs/main",
            b"not a supported backend marker",
        );
        write_file(
            &mut filesystem,
            "/wii/content/Data/Metadata/global-metadata.dat",
            b"metadata",
        );
        write_file(
            &mut filesystem,
            "/wii/content/Data/Managed/Assembly-CSharp.dll",
            b"assembly",
        );
        let structure = structure(
            PlatformKind::WiiU,
            "/wii",
            &["/wii/content/Data"],
            Some("/wii/content/Data"),
            Some("wii"),
        );

        let inventory = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                    disable_script_import: false,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert_eq!(inventory.scripting_backend, ScriptingBackend::Mono);
        assert_eq!(inventory.assemblies.len(), 1);
    }

    #[test]
    fn webgl_data_and_webplayer_bundle_are_registered_by_platform_rule() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/web/Build");
        write_file(&mut filesystem, "/web/index.html", b"html");
        write_file(&mut filesystem, "/web/Build/site.data.unityweb", b"data");
        let webgl = structure(
            PlatformKind::WebGl,
            "/web",
            &["/web", "/web/Build"],
            Some("/web"),
            Some("web"),
        );
        let inventory = webgl
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                    disable_script_import: false,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert!(inventory.registered_files.iter().any(|entry| {
            entry.file_name == "site" && entry.virtual_path == "/web/Build/site.data.unityweb"
        }));

        make_directory(&mut filesystem, "/player");
        write_file(&mut filesystem, "/player/game.html", b"html");
        write_file(
            &mut filesystem,
            "/player/game.unity3d",
            b"not checked by source",
        );
        let web_player = structure(
            PlatformKind::WebPlayer,
            "/player",
            &["/player"],
            None,
            Some("game"),
        );
        let player_inventory = web_player
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert!(player_inventory.registered_files.iter().any(|entry| {
            entry.file_name == "game" && entry.virtual_path == "/player/game.unity3d"
        }));
    }

    #[test]
    fn bundle_probe_obeys_scan_and_traversal_limits() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/game/Data/StreamingAssets/Nested");
        write_file(
            &mut filesystem,
            "/game/Data/StreamingAssets/a.bundle",
            &bundle_header(),
        );
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Data"],
            Some("/game/Data"),
            Some("Game"),
        );
        let error = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits {
                    max_inspected_files: 0,
                    ..PlatformFileInventoryLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            PlatformFileInventoryErrorKind::InspectedFileCountLimitExceeded { count: 1, limit: 0 }
        ));

        let error = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits {
                    max_directory_depth: 0,
                    ..PlatformFileInventoryLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            PlatformFileInventoryErrorKind::DirectoryDepthLimitExceeded { depth: 1, limit: 0 }
        ));
    }

    #[test]
    fn primary_file_name_matching_follows_source_patterns() {
        for name in [
            "mainData",
            "globalgamemanagers",
            "globalgamemanagers.assets",
            "resources.assets",
            "level0",
            "level42.split0",
            "sharedassets2.assets",
            "sharedassets2.assets.split0",
        ] {
            assert!(is_primary_engine_file(name), "{name}");
        }
        for name in [
            "MainData",
            "level",
            "levelx",
            "level1.split1",
            "sharedassets.assets",
            "sharedassets1.assets.split1",
        ] {
            assert!(!is_primary_engine_file(name), "{name}");
        }
    }

    #[test]
    fn compressed_data_bundle_names_match_the_source_literals() {
        assert!(is_compressed_data_bundle_name("data.unity3d"));
        assert!(is_compressed_data_bundle_name("datapack.unity3d"));
        // Ordinal: case, longer names, and split parts stay with the other branches.
        for name in [
            "Data.unity3d",
            "data.unity3d.bak",
            "data.unity3d.split0",
            "level0",
        ] {
            assert!(!is_compressed_data_bundle_name(name), "{name}");
        }
    }

    #[test]
    fn scripting_backend_selection_follows_the_disable_and_structure_branches() {
        use ScriptingBackend::{Il2Cpp, Mono, Unknown};
        // Disabled script import always reports Unknown, like the source installing the
        // no-op assembly manager.
        for platform in [None, Some(Unknown), Some(Mono), Some(Il2Cpp)] {
            for mixed in [None, Some(Unknown), Some(Mono)] {
                assert_eq!(
                    select_scripting_backend(true, platform, mixed),
                    Unknown,
                    "{platform:?}/{mixed:?}"
                );
            }
        }
        // The platform backend wins unless it is unknown or absent.
        assert_eq!(
            select_scripting_backend(false, Some(Mono), Some(Il2Cpp)),
            Mono
        );
        assert_eq!(select_scripting_backend(false, Some(Il2Cpp), None), Il2Cpp);
        // An unknown or absent platform backend falls through to mixed.
        assert_eq!(
            select_scripting_backend(false, Some(Unknown), Some(Mono)),
            Mono
        );
        assert_eq!(select_scripting_backend(false, None, Some(Mono)), Mono);
        // Nothing known anywhere reports Unknown.
        assert_eq!(
            select_scripting_backend(false, Some(Unknown), Some(Unknown)),
            Unknown
        );
        assert_eq!(select_scripting_backend(false, None, None), Unknown);
    }

    #[test]
    fn mono_assembly_location_follows_the_collect_main_assemblies_branches() {
        // An existing managed directory wins on its own, even with a lib fallback ready.
        assert_eq!(
            plan_mono_assembly_collection(
                Some("/game/Data/Managed"),
                true,
                Some("/game/Data"),
                Some("/game/Data/lib"),
                true,
            ),
            MainAssemblyCollection::Managed("/game/Data/Managed")
        );
        // Blank managed paths behave as absent, like `IsNullOrWhiteSpace`.
        for blank in [None, Some(""), Some("   ")] {
            assert_eq!(
                plan_mono_assembly_collection(
                    blank,
                    true,
                    Some("/game/Data"),
                    Some("/game/Data/lib"),
                    true,
                ),
                MainAssemblyCollection::GameDataAndLib("/game/Data", "/game/Data/lib"),
                "{blank:?}"
            );
        }
        // A missing managed directory falls through to the game-data pair.
        assert_eq!(
            plan_mono_assembly_collection(
                Some("/game/Data/Managed"),
                false,
                Some("/game/Data"),
                Some("/game/Data/lib"),
                true,
            ),
            MainAssemblyCollection::GameDataAndLib("/game/Data", "/game/Data/lib")
        );
        // Without a game-data directory, or without its lib child, nothing is collected.
        assert_eq!(
            plan_mono_assembly_collection(Some("/game/Data/Managed"), false, None, None, false),
            MainAssemblyCollection::None
        );
        assert_eq!(
            plan_mono_assembly_collection(
                Some("/game/Data/Managed"),
                false,
                Some("/game/Data"),
                Some("/game/Data/lib"),
                false,
            ),
            MainAssemblyCollection::None
        );
        // An empty game-data path behaves as absent, like `IsNullOrEmpty`.
        assert_eq!(
            plan_mono_assembly_collection(Some("/game/Data/Managed"), false, Some(""), None, false),
            MainAssemblyCollection::None
        );
        // A missing lib path (not just a missing directory) also collects nothing.
        assert_eq!(
            plan_mono_assembly_collection(
                Some("/game/Data/Managed"),
                false,
                Some("/game/Data"),
                None,
                true,
            ),
            MainAssemblyCollection::None
        );
    }
