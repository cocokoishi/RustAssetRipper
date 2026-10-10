    use super::{
        android_data_layout, android_root_role, desktop_data_fallback, detect_desktop_platform,
        detect_platform_structure, ios_app_data, ios_data_layout, ps4_data_layout,
        switch_data_layout, web_player_bundle_path, wiiu_data_layout, AndroidRootRole,
        DesktopPlatform, PlatformKind, PlatformResourceLookup, PlatformResourceProvider,
        PlatformResourceSource, PlatformStructure, PlatformStructureErrorKind,
        PlatformStructureLimits, SWITCH_EXEFS_DIR, SWITCH_MAIN_BINARY, SWITCH_ROMFS_DIR,
        WEBGL_BUILD_FLAVORS,
    };
    use crate::io::filesystem::VirtualFileSystem;

    fn insert_file(filesystem: &mut VirtualFileSystem, path: &str) {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, &[1]).unwrap();
    }

    fn insert_directory(filesystem: &mut VirtualFileSystem, path: &str) {
        filesystem.create_directory(Some(path)).unwrap();
    }

    fn resource_structure(paths: &[&str]) -> PlatformStructure {
        PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: None,
            data_paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    #[test]
    fn external_resource_lookup_uses_ordered_platform_then_mixed_paths_and_split_files() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/platform/second/tex.resS");
        insert_file(&mut filesystem, "/platform/second/same.resS");
        insert_file(&mut filesystem, "/mixed/split.resS.split0");
        insert_file(&mut filesystem, "/mixed/split.resS.split1");
        insert_file(&mut filesystem, "/mixed/same.resS");

        let platform = resource_structure(&["/platform/missing", "/platform/second"]);
        let mixed = resource_structure(&["/mixed"]);
        let provider = PlatformResourceProvider::new(Some(&platform), Some(&mixed));
        assert_eq!(
            provider
                .find_resource(
                    &filesystem,
                    "archive:/container/tex.resS",
                    PlatformStructureLimits::default(),
                )
                .unwrap(),
            PlatformResourceLookup::Found {
                virtual_path: "/platform/second/tex.resS".to_owned(),
                source: PlatformResourceSource::Platform,
                data_path_index: 1,
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            }
        );
        assert_eq!(
            provider
                .find_resource(&filesystem, "same.resS", PlatformStructureLimits::default(),)
                .unwrap(),
            PlatformResourceLookup::Found {
                virtual_path: "/platform/second/same.resS".to_owned(),
                source: PlatformResourceSource::Platform,
                data_path_index: 1,
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            }
        );
        assert_eq!(
            provider
                .find_resource(
                    &filesystem,
                    "split.resS",
                    PlatformStructureLimits::default(),
                )
                .unwrap(),
            PlatformResourceLookup::Found {
                virtual_path: "/mixed/split.resS".to_owned(),
                source: PlatformResourceSource::Mixed,
                data_path_index: 0,
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            }
        );
    }

    #[test]
    fn external_resource_lookup_accepts_mixed_game_structure_data_paths() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/mixed/resources/texture.resS");
        let mixed_data_paths = vec!["/mixed/missing".to_owned(), "/mixed/resources".to_owned()];
        let provider =
            PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed_data_paths));

        assert_eq!(
            provider
                .find_resource(
                    &filesystem,
                    "texture.resS",
                    PlatformStructureLimits::default(),
                )
                .unwrap(),
            PlatformResourceLookup::Found {
                virtual_path: "/mixed/resources/texture.resS".to_owned(),
                source: PlatformResourceSource::Mixed,
                data_path_index: 1,
                unavailable_data_paths: vec!["/mixed/missing".to_owned()],
            }
        );
    }

    #[test]
    fn external_resource_lookup_keeps_not_found_and_unavailable_paths_distinct() {
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/game/data");
        let platform = resource_structure(&["/missing", "/game/data"]);
        let provider = PlatformResourceProvider::new(Some(&platform), None);

        assert_eq!(
            provider
                .find_resource(
                    &filesystem,
                    "absent.resS",
                    PlatformStructureLimits::default(),
                )
                .unwrap(),
            PlatformResourceLookup::Missing {
                unavailable_data_paths: vec!["/missing".to_owned()],
            }
        );
    }

    #[test]
    fn detects_windows_roots_from_executable_and_data_directory() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/games/Nova.exe");
        insert_directory(&mut filesystem, "/games/Nova_Data");

        let from_executable = detect_desktop_platform(
            &filesystem,
            &["/games/Nova.exe".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(from_executable.platform, DesktopPlatform::Windows);
        assert_eq!(from_executable.root_path, "/games");
        assert_eq!(
            from_executable.game_data_path.as_deref(),
            Some("/games/Nova_Data")
        );

        let from_data = detect_desktop_platform(
            &filesystem,
            &["/games/Nova_Data".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(from_data.selected_path, "/games/Nova_Data");
        assert_eq!(from_data.root_path, "/games");
    }

    #[test]
    fn detects_linux_fallback_data_directory_and_mac_ordered_paths() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/linux/game.x86_64");
        insert_directory(&mut filesystem, "/linux/Data");
        let linux = detect_desktop_platform(
            &filesystem,
            &["/linux".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(linux.platform, DesktopPlatform::Linux);
        assert_eq!(linux.game_data_path.as_deref(), Some("/linux/Data"));

        insert_directory(&mut filesystem, "/App.app/Contents/Resources/Data");
        let mac = detect_desktop_platform(
            &filesystem,
            &["/App.app".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(mac.platform, DesktopPlatform::Mac);
        assert_eq!(
            mac.data_paths,
            [
                "/App.app/Contents/Resources/Data",
                "/App.app/Contents/Resources"
            ]
        );
    }

    #[test]
    fn desktop_detectors_cover_fallback_names_direct_selection_and_empty_shapes() {
        // Windows falls back to a bare `Data` directory: one executable keeps
        // its stem as the game name.
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/solo/only.exe");
        insert_directory(&mut filesystem, "/solo/Data");
        let solo = detect_desktop_platform(
            &filesystem,
            &["/solo".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(solo.platform, DesktopPlatform::Windows);
        assert_eq!(solo.name.as_deref(), Some("only"));
        assert_eq!(solo.game_data_path.as_deref(), Some("/solo/Data"));

        // Several executables without named pairs share the root folder name.
        insert_file(&mut filesystem, "/multi/b.exe");
        insert_file(&mut filesystem, "/multi/a.exe");
        insert_directory(&mut filesystem, "/multi/Data");
        let multi = detect_desktop_platform(
            &filesystem,
            &["/multi".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(multi.platform, DesktopPlatform::Windows);
        assert_eq!(multi.name.as_deref(), Some("multi"));
        assert_eq!(multi.game_data_path.as_deref(), Some("/multi/Data"));

        // A Linux executable selects its parent root directly, and a named
        // pair wins without any fallback.
        insert_file(&mut filesystem, "/lin/game.x86");
        insert_directory(&mut filesystem, "/lin/game_Data");
        let direct = detect_desktop_platform(
            &filesystem,
            &["/lin/game.x86".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(direct.platform, DesktopPlatform::Linux);
        assert_eq!(direct.selected_path, "/lin/game.x86");
        assert_eq!(direct.root_path, "/lin");
        assert_eq!(direct.game_data_path.as_deref(), Some("/lin/game_Data"));

        // A `.app` bundle without resources and an orphaned data directory
        // match no desktop shape.
        insert_directory(&mut filesystem, "/Empty.app");
        assert!(detect_desktop_platform(
            &filesystem,
            &["/Empty.app".to_owned()],
            PlatformStructureLimits::default()
        )
        .unwrap()
        .is_none());
        insert_directory(&mut filesystem, "/o/Orphan_Data");
        assert!(detect_desktop_platform(
            &filesystem,
            &["/o/Orphan_Data".to_owned()],
            PlatformStructureLimits::default()
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn follows_source_priority_and_reports_bad_selection_paths() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/both/Game.exe");
        insert_directory(&mut filesystem, "/both/Game_Data");
        insert_file(&mut filesystem, "/both/game.x86");
        insert_directory(&mut filesystem, "/both/game_Data");
        let selected = ["/both".to_owned()];
        assert_eq!(
            detect_desktop_platform(&filesystem, &selected, PlatformStructureLimits::default())
                .unwrap()
                .unwrap()
                .platform,
            DesktopPlatform::Windows
        );

        let bad_path = detect_desktop_platform(
            &filesystem,
            &["/both/../escape".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            bad_path.kind,
            PlatformStructureErrorKind::InvalidSelectedPath
        ));

        assert!(detect_desktop_platform(
            &filesystem,
            &["/not-a-game".to_owned()],
            PlatformStructureLimits::default()
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn detects_android_apk_and_obb_with_source_ordered_data_paths() {
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/apk/assets/bin/Data");
        insert_directory(&mut filesystem, "/apk/lib/arm64-v8a");
        insert_directory(&mut filesystem, "/apk/META-INF");
        insert_directory(&mut filesystem, "/obb/assets/bin/Data");

        let structure = detect_platform_structure(
            &filesystem,
            &["/obb".to_owned(), "/apk".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(structure.platform, PlatformKind::Android);
        assert_eq!(structure.selected_path, "/apk");
        assert_eq!(structure.consumed_paths, ["/apk", "/obb"]);
        assert_eq!(
            structure.data_paths,
            ["/apk/assets/bin/Data", "/obb/assets/bin/Data"]
        );

        insert_directory(&mut filesystem, "/apk2/assets/bin/Data");
        insert_directory(&mut filesystem, "/apk2/META-INF");
        let ambiguous = detect_platform_structure(
            &filesystem,
            &["/apk".to_owned(), "/apk2".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            ambiguous.kind,
            PlatformStructureErrorKind::AmbiguousAndroidRoots {
                role: "APK",
                count: 2
            }
        ));
    }

    #[test]
    fn android_directory_layout_matches_the_constructor_joins() {
        let layout = android_data_layout("/apk");
        assert_eq!(layout.game_data_path, "/apk/assets/bin/Data");
        assert_eq!(layout.resources_path, "/apk/assets/bin/Data/Resources");
        assert_eq!(layout.managed_path, "/apk/assets/bin/Data/Managed");
        assert_eq!(layout.lib_path, "/apk/lib");
        assert_eq!(
            layout.il2cpp_metadata_path,
            "/apk/assets/bin/Data/Managed/Metadata/global-metadata.dat"
        );
        // The same joins apply to an OBB root.
        let obb = android_data_layout("/obb");
        assert_eq!(obb.game_data_path, "/obb/assets/bin/Data");
        assert_eq!(
            obb.il2cpp_metadata_path,
            "/obb/assets/bin/Data/Managed/Metadata/global-metadata.dat"
        );
    }

    #[test]
    fn android_root_roles_follow_the_apk_obb_split() {
        // Assets plus `lib` and/or `META-INF` claim the APK slot ...
        assert_eq!(android_root_role(14, true), AndroidRootRole::Apk);
        assert_eq!(android_root_role(12, true), AndroidRootRole::Apk);
        assert_eq!(android_root_role(10, true), AndroidRootRole::Apk);
        // ... assets alone claim the OBB slot ...
        assert_eq!(android_root_role(8, true), AndroidRootRole::Obb);
        // ... and anything without the data directory stays out of the pairing.
        assert_eq!(android_root_role(14, false), AndroidRootRole::Neither);
        assert_eq!(android_root_role(8, false), AndroidRootRole::Neither);
        assert_eq!(android_root_role(6, true), AndroidRootRole::Neither);
        assert_eq!(android_root_role(0, true), AndroidRootRole::Neither);
    }

    #[test]
    fn android_obb_without_apk_is_no_structure_and_obb_roots_must_be_unique() {
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/obb/assets/bin/Data");

        // An OBB root alone is no structure, like the source `androidStructure != null` gate.
        assert!(detect_platform_structure(
            &filesystem,
            &["/obb".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .is_none());

        // A second OBB root errors, mirroring the source duplicate-OBB throw.
        insert_directory(&mut filesystem, "/obb2/assets/bin/Data");
        let ambiguous = detect_platform_structure(
            &filesystem,
            &["/obb".to_owned(), "/obb2".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            ambiguous.kind,
            PlatformStructureErrorKind::AmbiguousAndroidRoots {
                role: "OBB",
                count: 2
            }
        ));
    }

    #[test]
    fn ios_app_candidates_skip_dataless_bundles_and_non_app_names() {
        // Stemming follows the source suffix strip.
        assert_eq!(
            ios_app_data("/ios/Payload/Game.app", true),
            Some(("Game".to_owned(), "/ios/Payload/Game.app/Data".to_owned(),))
        );
        // Dataless bundles and non-`.app` names never match, like the source loop.
        assert_eq!(ios_app_data("/ios/Payload/Stale.app", false), None);
        assert_eq!(ios_app_data("/ios/Payload/notes", true), None);
        // The suffix match is ordinal, like the source check.
        assert_eq!(ios_app_data("/ios/Payload/Game.APP", true), None);

        // End-to-end: a dataless bundle does not shadow a later valid one.
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/ios/Payload/Aaa.app");
        insert_directory(&mut filesystem, "/ios/Payload/Game.app/Data");
        let ios = detect_platform_structure(
            &filesystem,
            &["/ios".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(ios.name.as_deref(), Some("Game"));
        assert_eq!(ios.data_paths, ["/ios/Payload/Game.app/Data"]);
    }

    #[test]
    fn ios_directory_layout_matches_the_constructor_joins() {
        let layout = ios_data_layout("/ios", "/ios/Payload/Example.app", "Example");
        assert_eq!(layout.game_data_path, "/ios/Payload/Example.app/Data");
        assert_eq!(layout.streaming_assets_path, "/ios/Raw");
        assert_eq!(
            layout.resources_path,
            "/ios/Payload/Example.app/Data/Resources"
        );
        assert_eq!(layout.managed_path, "/ios/Payload/Example.app/Data/Managed");
        // The IL2Cpp assembly is the app binary itself, not a lib file.
        assert_eq!(
            layout.il2cpp_game_assembly_path,
            "/ios/Payload/Example.app/Example"
        );
        assert_eq!(
            layout.il2cpp_metadata_path,
            "/ios/Payload/Example.app/Data/Managed/Metadata/global-metadata.dat"
        );
    }

    #[test]
    fn switch_directory_layout_matches_the_constructor_joins() {
        let layout = switch_data_layout("/switch");
        assert_eq!(layout.game_data_path, "/switch/romfs/Data");
        assert_eq!(
            layout.streaming_assets_path,
            "/switch/romfs/Data/StreamingAssets"
        );
        assert_eq!(layout.resources_path, "/switch/romfs/Data/Resources");
        assert_eq!(layout.managed_path, "/switch/romfs/Data/Managed");
        assert_eq!(layout.exefs_path, "/switch/exefs");
        // The IL2Cpp assembly is the `main` binary under `exefs`.
        assert_eq!(layout.il2cpp_game_assembly_path, "/switch/exefs/main");
        assert_eq!(
            layout.il2cpp_metadata_path,
            "/switch/romfs/Data/Managed/Metadata/global-metadata.dat"
        );
    }

    #[test]
    fn switch_directory_names_match_the_source_constants() {
        // `SwitchGameStructure` names its executable/data filesystems and IL2Cpp binary.
        assert_eq!(SWITCH_EXEFS_DIR, "exefs");
        assert_eq!(SWITCH_ROMFS_DIR, "romfs");
        assert_eq!(SWITCH_MAIN_BINARY, "main");

        // The names flow into detection: name and data path resolve off the root.
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/game/exefs");
        insert_directory(&mut filesystem, "/game/romfs/Data");
        let structure = detect_platform_structure(
            &filesystem,
            &["/game".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(structure.platform, PlatformKind::Switch);
        assert_eq!(structure.name.as_deref(), Some("game"));
        assert_eq!(
            structure.game_data_path.as_deref(),
            Some("/game/romfs/Data")
        );
    }

    #[test]
    fn ps4_directory_layout_matches_the_constructor_joins() {
        let layout = ps4_data_layout("/ps4");
        assert_eq!(layout.game_data_path, "/ps4/Media");
        assert_eq!(layout.resources_path, "/ps4/Media/Resources");
        assert_eq!(layout.managed_path, "/ps4/Media/Managed");
        assert_eq!(layout.modules_path, "/ps4/Media/Modules");
        // The IL2Cpp assembly is the PS4-only prx under `Modules`.
        assert_eq!(
            layout.il2cpp_game_assembly_path,
            "/ps4/Media/Modules/Il2CppUserAssemblies.prx"
        );
        // The metadata sits directly under `Data/Metadata`, not under `Managed`.
        assert_eq!(
            layout.il2cpp_metadata_path,
            "/ps4/Media/Metadata/global-metadata.dat"
        );
    }

    #[test]
    fn detects_ios_and_console_game_data_roots() {
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/ios/Payload/Example.app/Data");
        let ios = detect_platform_structure(
            &filesystem,
            &["/ios".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(ios.platform, PlatformKind::Ios);
        assert_eq!(ios.name.as_deref(), Some("Example"));
        assert_eq!(ios.data_paths, ["/ios/Payload/Example.app/Data"]);

        insert_file(&mut filesystem, "/ps4/eboot.bin");
        insert_directory(&mut filesystem, "/ps4/Media");
        let ps4 = detect_platform_structure(
            &filesystem,
            &["/ps4".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(ps4.platform, PlatformKind::Ps4);
        assert_eq!(ps4.game_data_path.as_deref(), Some("/ps4/Media"));

        insert_directory(&mut filesystem, "/switch/exefs");
        insert_directory(&mut filesystem, "/switch/romfs/Data");
        let switch = detect_platform_structure(
            &filesystem,
            &["/switch".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(switch.platform, PlatformKind::Switch);
        assert_eq!(switch.game_data_path.as_deref(), Some("/switch/romfs/Data"));

        insert_directory(&mut filesystem, "/wiiu/content/Data");
        let wiiu = detect_platform_structure(
            &filesystem,
            &["/wiiu".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(wiiu.platform, PlatformKind::WiiU);
        assert_eq!(wiiu.game_data_path.as_deref(), Some("/wiiu/content/Data"));
    }

    #[test]
    fn detects_webgl_layout_precedence_and_web_player_bundle_roots() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/web/index.html");
        insert_file(&mut filesystem, "/web/Build/player.data.unityweb");
        insert_directory(&mut filesystem, "/web/Development");
        insert_file(&mut filesystem, "/web/Development/player.data");
        let webgl = detect_platform_structure(
            &filesystem,
            &["/web".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(webgl.platform, PlatformKind::WebGl);
        assert_eq!(webgl.data_paths, ["/web", "/web/Build"]);

        insert_file(&mut filesystem, "/player/index.html");
        insert_file(&mut filesystem, "/player/index.unity3d");
        let web_player = detect_platform_structure(
            &filesystem,
            &["/player".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(web_player.platform, PlatformKind::WebPlayer);
        assert_eq!(web_player.name.as_deref(), Some("index"));
        assert_eq!(web_player.game_data_path, None);
        assert_eq!(web_player.data_paths, ["/player"]);
    }

    #[test]
    fn webgl_build_flavors_prefer_build_then_development_then_release() {
        assert_eq!(
            WEBGL_BUILD_FLAVORS,
            [
                ("Build", ".data.unityweb"),
                ("Development", ".data"),
                ("Release", ".datagz"),
            ]
        );
        // A Release-only tree still detects, with the release directory as data path.
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/rel/index.html");
        insert_file(&mut filesystem, "/rel/Release/game.datagz");
        let release = detect_platform_structure(
            &filesystem,
            &["/rel".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(release.platform, PlatformKind::WebGl);
        assert_eq!(release.data_paths, ["/rel", "/rel/Release"]);
    }

    #[test]
    fn web_player_pairing_follows_the_source_name_rule() {
        // An `.html` stem names the player beside its `.unity3d` bundle.
        assert_eq!(
            web_player_bundle_path("/player", "index.html"),
            Some(("index".to_owned(), "/player/index.unity3d".to_owned()))
        );
        // The extension match is ordinal, like the source switch.
        assert_eq!(web_player_bundle_path("/player", "INDEX.HTML"), None);
        assert_eq!(web_player_bundle_path("/player", "index.unity3d"), None);
        assert_eq!(web_player_bundle_path("/player", "notes.txt"), None);
    }

    #[test]
    fn desktop_data_fallback_names_single_and_multiple_executables() {
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/fb/Data");
        insert_file(&mut filesystem, "/fb/game.x86");
        // A lone executable without a matching data directory lends its stem.
        assert_eq!(
            desktop_data_fallback(&filesystem, "/fb", 1, "game").unwrap(),
            Some(("/fb/Data".to_owned(), "game".to_owned()))
        );
        // Several executables fall back to the root name.
        assert_eq!(
            desktop_data_fallback(&filesystem, "/fb", 2, "game").unwrap(),
            Some(("/fb/Data".to_owned(), "fb".to_owned()))
        );
        // No executables, or no `Data` directory: no fallback.
        assert_eq!(
            desktop_data_fallback(&filesystem, "/fb", 0, "game").unwrap(),
            None
        );
        assert_eq!(
            desktop_data_fallback(&filesystem, "/empty", 1, "game").unwrap(),
            None
        );
    }

    #[test]
    fn wiiu_directory_layout_matches_the_constructor_joins() {
        let layout = wiiu_data_layout("/wiiu");
        assert_eq!(layout.game_data_path, "/wiiu/content/Data");
        assert_eq!(
            layout.streaming_assets_path,
            "/wiiu/content/Data/StreamingAssets"
        );
        assert_eq!(layout.resources_path, "/wiiu/content/Data/Resources");
        assert_eq!(layout.managed_path, "/wiiu/content/Data/Managed");
    }

    #[test]
    fn preserves_windows_before_windows_phone_detection_priority() {
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/phone/Assets");
        insert_directory(&mut filesystem, "/phone/Data");
        insert_file(&mut filesystem, "/phone/Game.exe");

        // The source checker tests Windows before Windows Phone. Windows' generic `Data`
        // fallback therefore wins for this overlapping shape.
        let structure = detect_platform_structure(
            &filesystem,
            &["/phone".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(structure.platform, PlatformKind::Windows);
    }

    /// End-to-end platform differential against the reference `CheckPlatform`
    /// plus `CollectFiles` dump committed in `platform_probe_fixture.txt` (probe
    /// `Temp/opencode/platformprobe`, isolated reference bins, SDK 10.0.401).
    /// Every case rebuilds the reference directory tree in a scratch VFS (with
    /// the same readable version/container bytes) and requires the same
    /// platform kind, name, backend, DataPaths, registered files, and
    /// assemblies. The fixture `version=` column is recorded for the record
    /// only: the inventory carries no version field by row scope.
    #[test]
    fn detection_and_registration_reproduce_the_reference_platform_dump() {
        use crate::io::game_structure::{MixedGameStructure, MixedGameStructureLimits};
        use crate::io::platform_file_inventory::{
            PlatformFileInventoryLimits, PlatformFileInventoryOptions,
        };

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "odd hex length");
            hex.as_bytes()
                .chunks(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn platform_class(platform: PlatformKind) -> &'static str {
            match platform {
                PlatformKind::Windows => "WindowsGameStructure",
                PlatformKind::Linux => "LinuxGameStructure",
                PlatformKind::Mac => "MacGameStructure",
                PlatformKind::Android => "AndroidGameStructure",
                PlatformKind::Ios => "iOSGameStructure",
                PlatformKind::Switch => "SwitchGameStructure",
                PlatformKind::Ps4 => "PS4GameStructure",
                PlatformKind::WebGl => "WebGLGameStructure",
                PlatformKind::WebPlayer => "WebPlayerGameStructure",
                PlatformKind::WiiU => "WiiUGameStructure",
                PlatformKind::WindowsPhone => "WindowsPhoneGameStructure",
            }
        }

        let fixture = include_str!("platform_probe_fixture.txt");
        let mut sf_hex = String::new();
        let mut bundle_hex = String::new();
        let mut specs: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut extras: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut expectations: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            match parts[0] {
                "SFHEX" => sf_hex = parts[1].to_owned(),
                "BUNDLEHEX" => bundle_hex = parts[1].to_owned(),
                "PTREE" => {
                    specs.insert(
                        parts[1].to_owned(),
                        parts[2].split(';').map(str::to_owned).collect(),
                    );
                }
                "PTREE_EXTRA" => {
                    extras.insert(
                        parts[1].to_owned(),
                        parts[2].split(';').map(str::to_owned).collect(),
                    );
                }
                "PREF" => {
                    let mut fields = Vec::new();
                    for field in parts[2].split('|') {
                        let (key, value) = field.split_once('=').unwrap();
                        fields.push((key.to_owned(), value.to_owned()));
                    }
                    expectations.insert(parts[1].to_owned(), fields);
                }
                kind => panic!("unknown fixture row {kind}: {line}"),
            }
        }
        assert!(!sf_hex.is_empty() && !bundle_hex.is_empty());

        fn build_tree(
            filesystem: &mut VirtualFileSystem,
            root: &str,
            spec: &[String],
            sf_hex: &str,
            bundle_hex: &str,
        ) {
            for entry in spec {
                let entry_parts: Vec<&str> = entry.splitn(3, '|').collect();
                let path = format!("{root}/{}", entry_parts[1]);
                match entry_parts[0] {
                    "D" => filesystem.create_directory(Some(path.as_str())).unwrap(),
                    "F" => {
                        filesystem
                            .create_directory(Some(
                                path.rsplit_once('/').map(|(p, _)| p).unwrap_or(""),
                            ))
                            .unwrap();
                        filesystem.write_all_bytes(path.as_str(), &[]).unwrap();
                    }
                    "C" => {
                        let hex = match entry_parts[2] {
                            "{SF}" => sf_hex,
                            "{BUNDLE}" => bundle_hex,
                            token => panic!("unknown content token {token}"),
                        };
                        filesystem
                            .create_directory(Some(
                                path.rsplit_once('/').map(|(p, _)| p).unwrap_or(""),
                            ))
                            .unwrap();
                        filesystem
                            .write_all_bytes(path.as_str(), &decode_hex(hex))
                            .unwrap();
                    }
                    kind => panic!("unknown spec entry {kind}"),
                }
            }
        }

        let mut tags: Vec<String> = expectations.keys().cloned().collect();
        tags.sort();
        assert!(tags.len() >= 21, "the fixture pins every platform case");
        for tag in tags {
            let mut filesystem = VirtualFileSystem::new();
            let root = format!("/{tag}");
            let spec = specs.get(&tag).unwrap();
            build_tree(&mut filesystem, &root, spec, &sf_hex, &bundle_hex);
            let mut roots = vec![root.clone()];
            let mut obb_root = String::new();
            if let Some(extra) = extras.get(&tag) {
                obb_root = format!("/{tag}_obb");
                build_tree(&mut filesystem, &obb_root, extra, &sf_hex, &bundle_hex);
                roots.push(obb_root.clone());
            }
            // The Mac platform root is the `.app` bundle directory itself.
            if tag == "mac_mono" {
                roots = vec![format!("{root}/game.app")];
            }
            let expected = expectations.get(&tag).unwrap();
            let field = |key: &str| {
                expected
                    .iter()
                    .find(|(k, _)| k == key)
                    .unwrap_or_else(|| panic!("{tag} misses {key}"))
                    .1
                    .clone()
            };
            let resolve = |value: String| {
                value
                    .replace("{ROOT}", root.as_str())
                    .replace("{OBB}", obb_root.as_str())
            };

            let detected =
                detect_platform_structure(&filesystem, &roots, PlatformStructureLimits::default())
                    .unwrap();
            if field("platform") == "MixedGameStructure" {
                assert!(detected.is_none(), "{tag}: expected mixed");
                let mixed = MixedGameStructure::from_vfs_paths(
                    &filesystem,
                    &roots,
                    MixedGameStructureLimits::default(),
                )
                .unwrap();
                let mut files: Vec<String> = mixed
                    .registered_files
                    .iter()
                    .map(|f| format!("{}={}", f.file_name, f.virtual_path))
                    .collect();
                files.sort();
                let expected_files: Vec<String> = if field("files").is_empty() {
                    Vec::new()
                } else {
                    field("files")
                        .split(',')
                        .map(|f| resolve(f.to_owned()))
                        .collect()
                };
                assert_eq!(files, expected_files, "{tag} files");
                let mut datapaths = mixed.data_paths.clone();
                datapaths.sort();
                let mut expected_datapaths: Vec<String> = field("datapaths")
                    .split(',')
                    .map(|d| resolve(d.to_owned()))
                    .collect();
                expected_datapaths.sort();
                assert_eq!(datapaths, expected_datapaths, "{tag} datapaths");
                continue;
            }

            let structure = detected.unwrap_or_else(|| panic!("{tag}: expected platform"));
            assert_eq!(
                platform_class(structure.platform),
                field("platform"),
                "{tag} kind"
            );
            let expected_name =
                field("name").replace("{ROOT}", root.rsplit('/').next().unwrap_or(root.as_str()));
            assert_eq!(
                structure.name.as_deref().unwrap_or(""),
                expected_name,
                "{tag} name"
            );
            let mut datapaths = structure.data_paths.clone();
            datapaths.sort();
            let mut expected_datapaths: Vec<String> = field("datapaths")
                .split(',')
                .map(|d| resolve(d.to_owned()))
                .collect();
            expected_datapaths.sort();
            assert_eq!(datapaths, expected_datapaths, "{tag} datapaths");

            let inventory = structure
                .collect_game_files(
                    &filesystem,
                    PlatformFileInventoryOptions::default(),
                    PlatformFileInventoryLimits::default(),
                )
                .unwrap();
            assert_eq!(
                format!("{:?}", inventory.scripting_backend).to_uppercase(),
                field("backend").to_uppercase(),
                "{tag} backend"
            );
            let mut files: Vec<String> = inventory
                .registered_files
                .iter()
                .map(|f| format!("{}={}", f.file_name, f.virtual_path))
                .collect();
            files.sort();
            let expected_files: Vec<String> = if field("files").is_empty() {
                Vec::new()
            } else {
                field("files")
                    .split(',')
                    .map(|f| resolve(f.to_owned()))
                    .collect()
            };
            // The reference sorts file rows; the comparison sorts both sides.
            let mut expected_files = expected_files;
            expected_files.sort();
            assert_eq!(files, expected_files, "{tag} files");
            let mut assemblies: Vec<String> = inventory
                .assemblies
                .iter()
                .map(|f| format!("{}={}", f.file_name, f.virtual_path))
                .collect();
            assemblies.sort();
            let expected_assemblies: Vec<String> = if field("assemblies").is_empty() {
                Vec::new()
            } else {
                field("assemblies")
                    .split(',')
                    .map(|a| resolve(a.to_owned()))
                    .collect()
            };
            let mut expected_assemblies = expected_assemblies;
            expected_assemblies.sort();
            assert_eq!(assemblies, expected_assemblies, "{tag} assemblies");
        }
    }
