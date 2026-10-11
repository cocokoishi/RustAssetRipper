    #[test]
    fn export_project_matches_source_lightmap_schema_families() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn fixture_output(fixture: &str, relative_path: &str) -> Vec<u8> {
            let prefix = format!("SOURCE_OUTPUT|{relative_path}|");
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .unwrap_or_else(|| panic!("source lightmap fixture lacks {relative_path}"));
            decode_hex(hex)
        }

        fn normalize_generated_project_text(text: &str) -> String {
            let mut output = String::with_capacity(text.len());
            for line in text.lines() {
                let characters = line.chars().collect::<Vec<_>>();
                let mut normalized = String::with_capacity(line.len());
                let mut index = 0;
                while index < characters.len() {
                    let is_guid = index + 32 <= characters.len()
                        && (index == 0 || !characters[index - 1].is_ascii_hexdigit())
                        && characters[index..index + 32]
                            .iter()
                            .all(|character| character.is_ascii_hexdigit())
                        && (index + 32 == characters.len()
                            || !characters[index + 32].is_ascii_hexdigit());
                    if is_guid {
                        normalized.push_str("<GUID>");
                        index += 32;
                    } else {
                        normalized.push(characters[index]);
                        index += 1;
                    }
                }
                if normalized.starts_with("timeCreated: ") {
                    normalized = "timeCreated: <TIME>".to_owned();
                }
                output.push_str(&normalized);
                output.push('\n');
            }
            output
        }

        fn metadata_guid(meta: &str) -> &str {
            let guid = meta
                .lines()
                .find_map(|line| line.strip_prefix("guid: "))
                .expect("metadata contains a GUID");
            assert_eq!(guid.len(), 32);
            assert!(guid.bytes().all(|byte| byte.is_ascii_hexdigit()));
            guid
        }

        let fixtures = [
            (
                "5.0.0f1",
                include_str!("export/lightmap_project_source_5_0_fixture.txt"),
                "none",
                "legacy-parent",
            ),
            (
                "5.3.0f1",
                include_str!("export/lightmap_project_source_5_3_fixture.txt"),
                "guid",
                "legacy-parent",
            ),
            (
                "5.4.0f1",
                include_str!("export/lightmap_project_source_5_4_fixture.txt"),
                "none",
                "legacy-parent",
            ),
            (
                "5.5.0f1",
                include_str!("export/lightmap_project_source_5_5_fixture.txt"),
                "none",
                "legacy-parent",
            ),
            (
                "5.6.0f1",
                include_str!("export/lightmap_project_source_5_6_fixture.txt"),
                "none",
                "legacy-parent",
            ),
            (
                "2017.2.0f3",
                include_str!("export/lightmap_project_source_2017_2_fixture.txt"),
                "pointer",
                "legacy-parent",
            ),
            (
                "2017.4.40f1",
                include_str!("export/lightmap_project_source_2017_4_fixture.txt"),
                "pointer",
                "legacy-parent",
            ),
            (
                "2018.2.21f1",
                include_str!("export/lightmap_project_source_2018_2_fixture.txt"),
                "pointer",
                "corresponding-internal",
            ),
            (
                "2018.3.0f2",
                include_str!("export/lightmap_project_source_2018_3_fixture.txt"),
                "pointer",
                "modern-prefab",
            ),
            (
                "2022.3.0f1",
                include_str!("export/lightmap_project_source_2022_3_fixture.txt"),
                "pointer",
                "modern-prefab",
            ),
        ];

        for (version, fixture, scene_link, object_fields) in fixtures {
            let version_row = format!("UNITY_VERSION|{version}");
            assert!(fixture.lines().any(|line| line == version_row));
            let input_row = fixture
                .lines()
                .find_map(|line| line.strip_prefix("INPUT|LightmapFixture.assets|"))
                .expect("fixture has a SerializedFile identity row");
            let columns = input_row.split('|').collect::<Vec<_>>();
            assert_eq!(columns.len(), 2);
            let input_bytes = fixture
                .lines()
                .find_map(|line| line.strip_prefix("INPUT_HEX|LightmapFixture.assets|"))
                .map(decode_hex)
                .expect("fixture contains complete SerializedFile bytes");
            assert_eq!(columns[0].parse::<usize>().unwrap(), input_bytes.len());

            let directory = TestInputDirectory::new_root();
            let source_path = directory.join("LightmapFixture.assets");
            let settings_path = directory.join("lightmap-settings.json");
            let output_root = directory.join(format!("project-lightmap-{version}"));
            fs::write(&source_path, &input_bytes).unwrap();
            let settings = include_str!("export/settings_file_defaults.json")
                .replace("\"ImageExportFormat\": 4", "\"ImageExportFormat\": 0")
                .replace(
                    "\"LightmapTextureExportFormat\": 2",
                    "\"LightmapTextureExportFormat\": 1",
                )
                .replace(
                    "\"PreferOriginalTextureExtension\": true",
                    "\"PreferOriginalTextureExtension\": false",
                );
            fs::write(&settings_path, settings).unwrap();
            super::run(
                [
                    std::ffi::OsString::from("export-project"),
                    std::ffi::OsString::from("--settings"),
                    settings_path.into_os_string(),
                    output_root.clone().into_os_string(),
                    source_path.clone().into_os_string(),
                ]
                .into_iter(),
            )
            .unwrap_or_else(|error| {
                panic!("Unity {version} source-matched export failed: {error}")
            });

            let outputs = [
                (
                    "Assets/Scenes/lightmapfixture.assets.unity",
                    "ExportedProject/Assets/Scenes/lightmapfixture.assets.unity",
                    true,
                ),
                (
                    "Assets/Scenes/lightmapfixture.assets.unity.meta",
                    "ExportedProject/Assets/Scenes/lightmapfixture.assets.unity.meta",
                    true,
                ),
                (
                    "Assets/Scenes/lightmapfixture.assets/LightingData.asset",
                    "ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset",
                    true,
                ),
                (
                    "Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta",
                    "ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta",
                    true,
                ),
                (
                    "Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp",
                    "ExportedProject/Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp",
                    false,
                ),
                (
                    "Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp.meta",
                    "ExportedProject/Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp.meta",
                    true,
                ),
            ];
            for (rust_relative, source_relative, is_text) in outputs {
                let actual = fs::read(output_root.join(rust_relative)).unwrap_or_else(|error| {
                    panic!("Unity {version} Rust output {rust_relative} is missing: {error}")
                });
                let expected = fixture_output(fixture, source_relative);
                if is_text {
                    assert_eq!(
                        normalize_generated_project_text(std::str::from_utf8(&actual).unwrap()),
                        normalize_generated_project_text(std::str::from_utf8(&expected).unwrap()),
                        "Unity {version} output {rust_relative} differs from frozen source"
                    );
                } else {
                    assert_eq!(actual, expected, "Unity {version} image bytes differ");
                }
            }

            let scene =
                fs::read_to_string(output_root.join("Assets/Scenes/lightmapfixture.assets.unity"))
                    .unwrap();
            let lighting_data = fs::read_to_string(
                output_root.join("Assets/Scenes/lightmapfixture.assets/LightingData.asset"),
            )
            .unwrap();
            let scene_meta = fs::read_to_string(
                output_root.join("Assets/Scenes/lightmapfixture.assets.unity.meta"),
            )
            .unwrap();
            let lighting_data_meta = fs::read_to_string(
                output_root.join("Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta"),
            )
            .unwrap();
            let texture_meta = fs::read_to_string(
                output_root.join("Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp.meta"),
            )
            .unwrap();
            let scene_guid = metadata_guid(&scene_meta);
            let lighting_data_guid = metadata_guid(&lighting_data_meta);
            let texture_guid = metadata_guid(&texture_meta);
            assert_ne!(scene_guid, lighting_data_guid);
            assert_ne!(scene_guid, texture_guid);
            assert_ne!(lighting_data_guid, texture_guid);
            let scene_lighting_data_field = if version == "5.0.0f1" {
                "m_LightmapSnapshot"
            } else {
                "m_LightingDataAsset"
            };
            assert!(scene.contains(&format!(
                "{scene_lighting_data_field}: {{fileID: 112000000, guid: {lighting_data_guid}, type: 2}}"
            )));
            assert!(lighting_data.contains(&format!(
                "m_Lightmap: {{fileID: 2800000, guid: {texture_guid}, type: 3}}"
            )));
            match scene_link {
                "pointer" => assert!(lighting_data.contains(&format!(
                    "m_Scene: {{fileID: 102900000, guid: {scene_guid}, type: 3}}"
                ))),
                "guid" => assert!(lighting_data.contains(&format!("m_SceneGUID: {scene_guid}"))),
                "none" => assert!(!lighting_data
                    .lines()
                    .any(|line| line.trim_start().starts_with("m_Scene:")
                        || line.trim_start().starts_with("m_SceneGUID:"))),
                other => panic!("invalid fixture scene-link mode {other}"),
            }
            match object_fields {
                "legacy-parent" => {
                    assert!(lighting_data.contains("m_PrefabParentObject: {fileID: 0}"));
                    assert!(lighting_data.contains("m_PrefabInternal: {fileID: 0}"));
                    assert!(!lighting_data.contains("m_CorrespondingSourceObject:"));
                }
                "corresponding-internal" => {
                    assert!(lighting_data.contains("m_CorrespondingSourceObject: {fileID: 0}"));
                    assert!(lighting_data.contains("m_PrefabInternal: {fileID: 0}"));
                    assert!(!lighting_data.contains("m_PrefabParentObject:"));
                }
                "modern-prefab" => {
                    assert!(lighting_data.contains("m_CorrespondingSourceObject: {fileID: 0}"));
                    assert!(lighting_data.contains("m_PrefabInstance: {fileID: 0}"));
                    assert!(lighting_data.contains("m_PrefabAsset: {fileID: 0}"));
                    assert!(!lighting_data.contains("m_PrefabInternal:"));
                }
                other => panic!("invalid fixture object-field mode {other}"),
            }
            assert_eq!(fs::read(&source_path).unwrap(), input_bytes);
        }
    }

    #[test]
    fn export_project_matches_source_5_6_mesh_renderer_lighting_data() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn fixture_value(fixture: &str, prefix: &str) -> Vec<u8> {
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("source lightmap fixture lacks {prefix}"));
            decode_hex(hex)
        }

        fn normalize(text: &str) -> String {
            let mut output = String::with_capacity(text.len());
            for line in text.lines() {
                let chars = line.chars().collect::<Vec<_>>();
                let mut normalized = String::with_capacity(line.len());
                let mut index = 0;
                while index < chars.len() {
                    let is_guid = index + 32 <= chars.len()
                        && (index == 0 || !chars[index - 1].is_ascii_hexdigit())
                        && chars[index..index + 32]
                            .iter()
                            .all(|character| character.is_ascii_hexdigit())
                        && (index + 32 == chars.len()
                            || !chars[index + 32].is_ascii_hexdigit());
                    if is_guid {
                        normalized.push_str("<GUID>");
                        index += 32;
                    } else {
                        normalized.push(chars[index]);
                        index += 1;
                    }
                }
                if normalized.starts_with("timeCreated: ") {
                    normalized = "timeCreated: <TIME>".to_owned();
                }
                output.push_str(&normalized);
                output.push('\n');
            }
            output
        }

        fn metadata_guid(meta: &str) -> &str {
            let guid = meta
                .lines()
                .find_map(|line| line.strip_prefix("guid: "))
                .expect("metadata contains a GUID");
            assert_eq!(guid.len(), 32);
            assert!(guid.bytes().all(|byte| byte.is_ascii_hexdigit()));
            guid
        }

        let fixture = include_str!("export/lightmap_project_source_5_6_renderer_fixture.txt");
        assert!(fixture.lines().any(|line| line == "UNITY_VERSION|5.6.0f1"));
        let input_bytes = fixture_value(fixture, "INPUT_HEX|LightmapFixture.assets|");
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("LightmapFixture.assets");
        let settings_path = directory.join("lightmap-settings.json");
        let output_root = directory.join("project-lightmap-5.6-mesh-renderer");
        fs::write(&source_path, &input_bytes).unwrap();
        let settings = include_str!("export/settings_file_defaults.json")
            .replace("\"ImageExportFormat\": 4", "\"ImageExportFormat\": 0")
            .replace(
                "\"LightmapTextureExportFormat\": 2",
                "\"LightmapTextureExportFormat\": 1",
            )
            .replace(
                "\"PreferOriginalTextureExtension\": true",
                "\"PreferOriginalTextureExtension\": false",
            );
        fs::write(&settings_path, settings).unwrap();
        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.into_os_string(),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("Unity 5.6 source-authored MeshRenderer export succeeds");

        let source_lighting_data = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset|",
        );
        let source_lighting_data = std::str::from_utf8(&source_lighting_data).unwrap();
        let lighting_data_path = output_root.join(
            "Assets/Scenes/lightmapfixture.assets/LightingData.asset",
        );
        let lighting_data = fs::read_to_string(&lighting_data_path).unwrap();
        assert_eq!(
            normalize(&lighting_data),
            normalize(source_lighting_data),
            "Unity 5.6 LightingDataAsset with a lightmapped MeshRenderer differs from frozen source"
        );
        assert!(lighting_data.contains("lightmapIndex: 0"));
        assert!(lighting_data.contains("lightmapIndexDynamic: 65535"));
        assert!(lighting_data.contains("lightmapST: {x: 1, y: 1, z: 0, w: 0}"));
        assert!(lighting_data.contains("targetObject: 105"));

        let lighting_data_meta = fs::read_to_string(format!(
            "{}.meta",
            lighting_data_path.display()
        ))
        .unwrap();
        let source_lighting_data_meta = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta|",
        );
        assert_eq!(
            normalize(&lighting_data_meta),
            normalize(std::str::from_utf8(&source_lighting_data_meta).unwrap()),
            "Unity 5.6 LightingDataAsset metadata differs from frozen source"
        );
        let scene = fs::read_to_string(
            output_root.join("Assets/Scenes/lightmapfixture.assets.unity"),
        )
        .unwrap();
        let lighting_data_guid = metadata_guid(&lighting_data_meta);
        assert!(scene.contains("--- !u!23 &105\nMeshRenderer:"));
        assert!(scene.contains(&format!(
            "m_LightingDataAsset: {{fileID: 112000000, guid: {lighting_data_guid}, type: 2}}"
        )));
        assert_eq!(fs::read(&source_path).unwrap(), input_bytes);
    }

    #[test]
    fn export_project_maps_5_6_lightmapped_renderer_uv_mesh() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn fixture_value(fixture: &str, prefix: &str) -> Vec<u8> {
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("source lightmap fixture lacks {prefix}"));
            decode_hex(hex)
        }

        fn normalize(text: &str) -> String {
            let mut output = String::with_capacity(text.len());
            for line in text.lines() {
                let chars = line.chars().collect::<Vec<_>>();
                let mut normalized = String::with_capacity(line.len());
                let mut index = 0;
                while index < chars.len() {
                    let is_guid = index + 32 <= chars.len()
                        && (index == 0 || !chars[index - 1].is_ascii_hexdigit())
                        && chars[index..index + 32]
                            .iter()
                            .all(|character| character.is_ascii_hexdigit())
                        && (index + 32 == chars.len()
                            || !chars[index + 32].is_ascii_hexdigit());
                    if is_guid {
                        normalized.push_str("<GUID>");
                        index += 32;
                    } else {
                        normalized.push(chars[index]);
                        index += 1;
                    }
                }
                if normalized.starts_with("timeCreated: ") {
                    normalized = "timeCreated: <TIME>".to_owned();
                }
                output.push_str(&normalized);
                output.push('\n');
            }
            output
        }

        fn metadata_guid(meta: &str) -> &str {
            let guid = meta
                .lines()
                .find_map(|line| line.strip_prefix("guid: "))
                .expect("metadata contains a GUID");
            assert_eq!(guid.len(), 32);
            assert!(guid.bytes().all(|byte| byte.is_ascii_hexdigit()));
            guid
        }

        let fixture = include_str!("export/lightmap_project_source_5_6_meshfilter_fixture.txt");
        assert!(fixture.lines().any(|line| line == "UNITY_VERSION|5.6.0f1"));
        let input_bytes = fixture_value(fixture, "INPUT_HEX|LightmapFixture.assets|");
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("LightmapFixture.assets");
        let settings_path = directory.join("lightmap-settings.json");
        let output_root = directory.join("project-lightmap-5.6-meshfilter");
        fs::write(&source_path, &input_bytes).unwrap();
        let settings = include_str!("export/settings_file_defaults.json")
            .replace("\"ImageExportFormat\": 4", "\"ImageExportFormat\": 0")
            .replace(
                "\"LightmapTextureExportFormat\": 2",
                "\"LightmapTextureExportFormat\": 1",
            )
            .replace(
                "\"PreferOriginalTextureExtension\": true",
                "\"PreferOriginalTextureExtension\": false",
            );
        fs::write(&settings_path, settings).unwrap();
        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.into_os_string(),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("Unity 5.6 source-authored MeshFilter/MeshRenderer export succeeds");

        let lighting_data_path = output_root.join(
            "Assets/Scenes/lightmapfixture.assets/LightingData.asset",
        );
        let lighting_data = fs::read_to_string(&lighting_data_path).unwrap();
        let source_lighting_data = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset|",
        );
        assert_eq!(
            normalize(&lighting_data),
            normalize(std::str::from_utf8(&source_lighting_data).unwrap()),
            "Unity 5.6 LightingDataAsset UvMesh pointer differs from frozen source"
        );
        assert!(lighting_data.contains("uvMesh: {fileID: 4300000, guid: "));
        assert!(lighting_data.contains("lightmapIndex: 0"));
        assert!(lighting_data.contains("targetObject: 105"));

        let mesh_meta_path = output_root.join("Assets/Mesh/LightmappedMesh.asset.meta");
        let mesh_meta = fs::read_to_string(&mesh_meta_path).unwrap();
        let source_mesh_meta = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Mesh/LightmappedMesh.asset.meta|",
        );
        assert_eq!(
            normalize(&mesh_meta),
            normalize(std::str::from_utf8(&source_mesh_meta).unwrap()),
            "Unity 5.6 Mesh metadata differs from frozen source"
        );
        let mesh_guid = metadata_guid(&mesh_meta);
        assert!(lighting_data.contains(&format!(
            "uvMesh: {{fileID: 4300000, guid: {mesh_guid}, type: 2}}"
        )));

        let scene = fs::read_to_string(
            output_root.join("Assets/Scenes/lightmapfixture.assets.unity"),
        )
        .unwrap();
        let source_scene = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets.unity|",
        );
        assert_eq!(
            normalize(&scene),
            normalize(std::str::from_utf8(&source_scene).unwrap()),
            "complete Unity 5.6 Scene editor-format YAML differs from frozen source"
        );
        assert!(scene.contains("--- !u!33 &106\nMeshFilter:"));
        assert!(scene.contains("--- !u!23 &105\nMeshRenderer:"));
        assert!(scene.contains(&format!(
            "m_Mesh: {{fileID: 4300000, guid: {mesh_guid}, type: 2}}"
        )));
        assert_eq!(fs::read(&source_path).unwrap(), input_bytes);
    }

    #[test]
    fn export_project_maps_5_6_light_to_lighting_data() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn fixture_value(fixture: &str, prefix: &str) -> Vec<u8> {
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("source light fixture lacks {prefix}"));
            decode_hex(hex)
        }

        fn normalize(text: &str) -> String {
            let mut output = String::with_capacity(text.len());
            for line in text.lines() {
                let chars = line.chars().collect::<Vec<_>>();
                let mut normalized = String::with_capacity(line.len());
                let mut index = 0;
                while index < chars.len() {
                    let is_guid = index + 32 <= chars.len()
                        && (index == 0 || !chars[index - 1].is_ascii_hexdigit())
                        && chars[index..index + 32]
                            .iter()
                            .all(|character| character.is_ascii_hexdigit())
                        && (index + 32 == chars.len()
                            || !chars[index + 32].is_ascii_hexdigit());
                    if is_guid {
                        normalized.push_str("<GUID>");
                        index += 32;
                    } else {
                        normalized.push(chars[index]);
                        index += 1;
                    }
                }
                if normalized.starts_with("timeCreated: ") {
                    normalized = "timeCreated: <TIME>".to_owned();
                }
                output.push_str(&normalized);
                output.push('\n');
            }
            output
        }

        let fixture = include_str!("export/lightmap_project_source_5_6_light_fixture.txt");
        assert!(fixture.lines().any(|line| line == "UNITY_VERSION|5.6.0f1"));
        let input_bytes = fixture_value(fixture, "INPUT_HEX|LightmapFixture.assets|");
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("LightmapFixture.assets");
        let settings_path = directory.join("lightmap-light-settings.json");
        let output_root = directory.join("project-lightmap-5.6-light");
        fs::write(&source_path, &input_bytes).unwrap();
        let settings = include_str!("export/settings_file_defaults.json")
            .replace("\"ImageExportFormat\": 4", "\"ImageExportFormat\": 0")
            .replace(
                "\"LightmapTextureExportFormat\": 2",
                "\"LightmapTextureExportFormat\": 1",
            )
            .replace(
                "\"PreferOriginalTextureExtension\": true",
                "\"PreferOriginalTextureExtension\": false",
            );
        fs::write(&settings_path, settings).unwrap();
        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.clone().into_os_string(),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("Unity 5.6 Light LightingDataAsset export succeeds");

        let lighting_data_path =
            output_root.join("Assets/Scenes/lightmapfixture.assets/LightingData.asset");
        let lighting_data = fs::read_to_string(&lighting_data_path).unwrap();
        let source_lighting_data = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset|",
        );
        assert_eq!(
            normalize(&lighting_data),
            normalize(std::str::from_utf8(&source_lighting_data).unwrap()),
            "complete Unity 5.6 LightingDataAsset Light arrays differ from frozen source"
        );
        assert!(lighting_data.contains("m_Lights:\n  - targetObject: 105\n    targetPrefab: 0"));
        assert!(lighting_data.contains(
            "m_LightBakingOutputs:\n  - probeOcclusionLightIndex: 0\n    occlusionMaskChannel: 0\n    lightmappingMask: 0"
        ));

        let lighting_data_meta = fs::read_to_string(format!(
            "{}.meta",
            lighting_data_path.display()
        ))
        .unwrap();
        let source_lighting_data_meta = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta|",
        );
        assert_eq!(
            normalize(&lighting_data_meta),
            normalize(std::str::from_utf8(&source_lighting_data_meta).unwrap()),
            "Unity 5.6 LightingDataAsset metadata differs from frozen source"
        );
        let lighting_data_guid = lighting_data_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("LightingDataAsset metadata contains a GUID");
        let scene = fs::read_to_string(
            output_root.join("Assets/Scenes/lightmapfixture.assets.unity"),
        )
        .unwrap();
        let source_scene = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets.unity|",
        );
        assert_eq!(
            normalize(&scene),
            normalize(std::str::from_utf8(&source_scene).unwrap()),
            "complete Unity 5.6 Scene output for the Light case differs from frozen source"
        );
        assert!(scene.contains("--- !u!108 &105\nLight:"));
        assert!(scene.contains(&format!(
            "m_LightingDataAsset: {{fileID: 112000000, guid: {lighting_data_guid}, type: 2}}"
        )));
        assert_eq!(fs::read(&source_path).unwrap(), input_bytes);

        let baking_fixture = include_str!("export/lightmap_project_source_5_6_light_baking_fixture.txt");
        assert!(baking_fixture
            .lines()
            .any(|line| line == "UNITY_VERSION|5.6.0f1"));
        let baking_input_bytes =
            fixture_value(baking_fixture, "INPUT_HEX|LightmapFixture.assets|");
        fs::write(&source_path, &baking_input_bytes).unwrap();
        let baking_output_root = directory.join("project-lightmap-5.6-light-baking");
        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.clone().into_os_string(),
                baking_output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("Unity 5.6 nonzero Light BakingOutput export succeeds");

        let baking_lighting_data_path = baking_output_root
            .join("Assets/Scenes/lightmapfixture.assets/LightingData.asset");
        let baking_lighting_data = fs::read_to_string(&baking_lighting_data_path).unwrap();
        let source_baking_lighting_data = fixture_value(
            baking_fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset|",
        );
        assert_eq!(
            normalize(&baking_lighting_data),
            normalize(std::str::from_utf8(&source_baking_lighting_data).unwrap()),
            "complete Unity 5.6 nonzero Light BakingOutput differs from frozen source"
        );
        assert!(baking_lighting_data.contains(
            "m_LightBakingOutputs:\n  - probeOcclusionLightIndex: 7\n    occlusionMaskChannel: 3\n    lightmappingMask: 258"
        ));

        let baking_lighting_data_meta = fs::read_to_string(format!(
            "{}.meta",
            baking_lighting_data_path.display()
        ))
        .unwrap();
        let source_baking_lighting_data_meta = fixture_value(
            baking_fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta|",
        );
        assert_eq!(
            normalize(&baking_lighting_data_meta),
            normalize(std::str::from_utf8(&source_baking_lighting_data_meta).unwrap()),
            "Unity 5.6 nonzero Light LightingDataAsset metadata differs from frozen source"
        );
        let baking_lighting_data_guid = baking_lighting_data_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("nonzero Light LightingDataAsset metadata contains a GUID");
        let baking_scene = fs::read_to_string(
            baking_output_root.join("Assets/Scenes/lightmapfixture.assets.unity"),
        )
        .unwrap();
        let source_baking_scene = fixture_value(
            baking_fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets.unity|",
        );
        assert_eq!(
            normalize(&baking_scene),
            normalize(std::str::from_utf8(&source_baking_scene).unwrap()),
            "complete Unity 5.6 Scene output for nonzero Light BakingOutput differs from frozen source"
        );
        assert!(baking_scene.contains(&format!(
            "m_LightingDataAsset: {{fileID: 112000000, guid: {baking_lighting_data_guid}, type: 2}}"
        )));
        assert_eq!(fs::read(&source_path).unwrap(), baking_input_bytes);

        let multiple_fixture =
            include_str!("export/lightmap_project_source_5_6_multiple_lights_fixture.txt");
        assert!(multiple_fixture
            .lines()
            .any(|line| line == "UNITY_VERSION|5.6.0f1"));
        let multiple_input_bytes =
            fixture_value(multiple_fixture, "INPUT_HEX|LightmapFixture.assets|");
        fs::write(&source_path, &multiple_input_bytes).unwrap();
        let multiple_output_root = directory.join("project-lightmap-5.6-multiple-lights");
        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.clone().into_os_string(),
                multiple_output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("Unity 5.6 multiple Light LightingDataAsset export succeeds");

        let multiple_lighting_data_path = multiple_output_root
            .join("Assets/Scenes/lightmapfixture.assets/LightingData.asset");
        let multiple_lighting_data = fs::read_to_string(&multiple_lighting_data_path).unwrap();
        let source_multiple_lighting_data = fixture_value(
            multiple_fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset|",
        );
        assert_eq!(
            normalize(&multiple_lighting_data),
            normalize(std::str::from_utf8(&source_multiple_lighting_data).unwrap()),
            "complete Unity 5.6 multiple-Light LightingDataAsset differs from frozen source"
        );
        assert!(multiple_lighting_data.contains(
            "m_Lights:\n  - targetObject: 105\n    targetPrefab: 0\n  - targetObject: 106\n    targetPrefab: 0"
        ));
        assert!(multiple_lighting_data.contains(
            "m_LightBakingOutputs:\n  - probeOcclusionLightIndex: 7\n    occlusionMaskChannel: 3\n    lightmappingMask: 258\n  - probeOcclusionLightIndex: 4\n    occlusionMaskChannel: 2\n    lightmappingMask: 513"
        ));

        let multiple_lighting_data_meta = fs::read_to_string(format!(
            "{}.meta",
            multiple_lighting_data_path.display()
        ))
        .unwrap();
        let source_multiple_lighting_data_meta = fixture_value(
            multiple_fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta|",
        );
        assert_eq!(
            normalize(&multiple_lighting_data_meta),
            normalize(std::str::from_utf8(&source_multiple_lighting_data_meta).unwrap()),
            "Unity 5.6 multiple-Light LightingDataAsset metadata differs from frozen source"
        );
        let multiple_lighting_data_guid = multiple_lighting_data_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("multiple-Light LightingDataAsset metadata contains a GUID");
        let multiple_scene = fs::read_to_string(
            multiple_output_root.join("Assets/Scenes/lightmapfixture.assets.unity"),
        )
        .unwrap();
        let source_multiple_scene = fixture_value(
            multiple_fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets.unity|",
        );
        assert_eq!(
            normalize(&multiple_scene),
            normalize(std::str::from_utf8(&source_multiple_scene).unwrap()),
            "complete Unity 5.6 multiple-Light Scene differs from frozen source"
        );
        assert!(multiple_scene.contains("--- !u!108 &105\nLight:"));
        assert!(multiple_scene.contains("--- !u!108 &106\nLight:"));
        assert!(multiple_scene.contains(&format!(
            "m_LightingDataAsset: {{fileID: 112000000, guid: {multiple_lighting_data_guid}, type: 2}}"
        )));
        assert_eq!(fs::read(&source_path).unwrap(), multiple_input_bytes);

        let multiple_gameobjects_fixture = include_str!(
            "export/lightmap_project_source_5_6_multiple_gameobjects_fixture.txt"
        );
        assert!(multiple_gameobjects_fixture
            .lines()
            .any(|line| line == "UNITY_VERSION|5.6.0f1"));
        let multiple_gameobjects_input_bytes =
            fixture_value(multiple_gameobjects_fixture, "INPUT_HEX|LightmapFixture.assets|");
        fs::write(&source_path, &multiple_gameobjects_input_bytes).unwrap();
        let multiple_gameobjects_output_root =
            directory.join("project-lightmap-5.6-multiple-gameobjects");
        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.into_os_string(),
                multiple_gameobjects_output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("Unity 5.6 Lights on separate GameObjects export to LightingDataAsset");

        let multiple_gameobjects_lighting_data_path = multiple_gameobjects_output_root
            .join("Assets/Scenes/lightmapfixture.assets/LightingData.asset");
        let multiple_gameobjects_lighting_data =
            fs::read_to_string(&multiple_gameobjects_lighting_data_path).unwrap();
        let source_multiple_gameobjects_lighting_data = fixture_value(
            multiple_gameobjects_fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset|",
        );
        assert_eq!(
            normalize(&multiple_gameobjects_lighting_data),
            normalize(std::str::from_utf8(&source_multiple_gameobjects_lighting_data).unwrap()),
            "complete Unity 5.6 separate-GameObject LightingDataAsset differs from frozen source"
        );
        assert!(multiple_gameobjects_lighting_data.contains(
            "m_Lights:\n  - targetObject: 105\n    targetPrefab: 0\n  - targetObject: 109\n    targetPrefab: 0"
        ));
        assert!(multiple_gameobjects_lighting_data.contains(
            "m_LightBakingOutputs:\n  - probeOcclusionLightIndex: 7\n    occlusionMaskChannel: 3\n    lightmappingMask: 258\n  - probeOcclusionLightIndex: 4\n    occlusionMaskChannel: 2\n    lightmappingMask: 513"
        ));
        let multiple_gameobjects_lighting_data_meta = fs::read_to_string(format!(
            "{}.meta",
            multiple_gameobjects_lighting_data_path.display()
        ))
        .unwrap();
        let source_multiple_gameobjects_lighting_data_meta = fixture_value(
            multiple_gameobjects_fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta|",
        );
        assert_eq!(
            normalize(&multiple_gameobjects_lighting_data_meta),
            normalize(std::str::from_utf8(&source_multiple_gameobjects_lighting_data_meta).unwrap()),
            "Unity 5.6 separate-GameObject LightingDataAsset metadata differs from frozen source"
        );
        let multiple_gameobjects_lighting_data_guid = multiple_gameobjects_lighting_data_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("separate-GameObject LightingDataAsset metadata contains a GUID");
        let multiple_gameobjects_scene = fs::read_to_string(
            multiple_gameobjects_output_root.join("Assets/Scenes/lightmapfixture.assets.unity"),
        )
        .unwrap();
        let source_multiple_gameobjects_scene = fixture_value(
            multiple_gameobjects_fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets.unity|",
        );
        assert_eq!(
            normalize(&multiple_gameobjects_scene),
            normalize(std::str::from_utf8(&source_multiple_gameobjects_scene).unwrap()),
            "complete Unity 5.6 separate-GameObject Scene differs from frozen source"
        );
        assert!(multiple_gameobjects_scene.contains("--- !u!108 &105\nLight:"));
        assert!(multiple_gameobjects_scene.contains("--- !u!108 &109\nLight:"));
        assert!(multiple_gameobjects_scene.contains(&format!(
            "m_LightingDataAsset: {{fileID: 112000000, guid: {multiple_gameobjects_lighting_data_guid}, type: 2}}"
        )));
        assert_eq!(fs::read(&source_path).unwrap(), multiple_gameobjects_input_bytes);
    }

    #[test]
    fn export_project_preserves_5_6_render_settings_halo_texture_with_lighting_data() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();

        fn fixture_value(fixture: &str, prefix: &str) -> Vec<u8> {
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("source fixture lacks {prefix}"));
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn normalize(text: &str) -> String {
            let mut output = String::with_capacity(text.len());
            for line in text.lines() {
                let chars = line.chars().collect::<Vec<_>>();
                let mut normalized = String::with_capacity(line.len());
                let mut index = 0;
                while index < chars.len() {
                    let is_guid = index + 32 <= chars.len()
                        && (index == 0 || !chars[index - 1].is_ascii_hexdigit())
                        && chars[index..index + 32]
                            .iter()
                            .all(|character| character.is_ascii_hexdigit())
                        && (index + 32 == chars.len()
                            || !chars[index + 32].is_ascii_hexdigit());
                    if is_guid {
                        normalized.push_str("<GUID>");
                        index += 32;
                    } else {
                        normalized.push(chars[index]);
                        index += 1;
                    }
                }
                if normalized.starts_with("timeCreated: ") {
                    normalized = "timeCreated: <TIME>".to_owned();
                }
                output.push_str(&normalized);
                output.push('\n');
            }
            output
        }

        let fixture = include_str!(
            "export/lightmap_project_source_5_6_render_settings_halo_texture_fixture.txt"
        );
        assert!(fixture.lines().any(|line| line == "UNITY_VERSION|5.6.0f1"));
        let input_bytes = fixture_value(fixture, "INPUT_HEX|LightmapFixture.assets|");
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("LightmapFixture.assets");
        let settings_path = directory.join("lightmap-render-settings-halo.json");
        let output_root = directory.join("project-lightmap-5.6-render-settings-halo");
        fs::write(&source_path, &input_bytes).unwrap();
        let settings = include_str!("export/settings_file_defaults.json")
            .replace("\"ImageExportFormat\": 4", "\"ImageExportFormat\": 0")
            .replace(
                "\"LightmapTextureExportFormat\": 2",
                "\"LightmapTextureExportFormat\": 1",
            )
            .replace(
                "\"PreferOriginalTextureExtension\": true",
                "\"PreferOriginalTextureExtension\": false",
            );
        fs::write(&settings_path, settings).unwrap();
        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.into_os_string(),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("Unity 5.6 RenderSettings HaloTexture reference exports with LightingDataAsset");

        let scene_path = output_root.join("Assets/Scenes/lightmapfixture.assets.unity");
        let scene = fs::read_to_string(&scene_path).unwrap();
        let source_scene = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets.unity|",
        );
        assert_eq!(
            normalize(&scene),
            normalize(std::str::from_utf8(&source_scene).unwrap()),
            "complete Unity 5.6 Scene with a RenderSettings texture reference differs from frozen source"
        );

        let lighting_data_path =
            output_root.join("Assets/Scenes/lightmapfixture.assets/LightingData.asset");
        let lighting_data = fs::read_to_string(&lighting_data_path).unwrap();
        let source_lighting_data = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset|",
        );
        assert_eq!(
            normalize(&lighting_data),
            normalize(std::str::from_utf8(&source_lighting_data).unwrap()),
            "complete Unity 5.6 LightingDataAsset with a RenderSettings texture reference differs from frozen source"
        );

        let lighting_data_meta = fs::read_to_string(format!("{}.meta", lighting_data_path.display()))
            .unwrap();
        let source_lighting_data_meta = fixture_value(
            fixture,
            "SOURCE_OUTPUT|ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta|",
        );
        assert_eq!(
            normalize(&lighting_data_meta),
            normalize(std::str::from_utf8(&source_lighting_data_meta).unwrap()),
            "Unity 5.6 LightingDataAsset metadata differs from frozen source"
        );
        let lightmap_texture_meta = fs::read_to_string(
            output_root.join("Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp.meta"),
        )
        .unwrap();
        let texture_guid = lightmap_texture_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("lightmap texture metadata contains a GUID");
        assert!(scene.contains(&format!(
            "m_HaloTexture: {{fileID: 2800000, guid: {texture_guid}, type: 3}}"
        )));
        assert!(lighting_data.contains(&format!(
            "m_Lightmap: {{fileID: 2800000, guid: {texture_guid}, type: 3}}"
        )));
        assert_eq!(fs::read(&source_path).unwrap(), input_bytes);
    }

    #[test]
    fn export_project_recovers_assembly_backed_monobehaviour_structure() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("mono_unloaded_valid.assets");
        let assembly_path = directory.join("Assembly-CSharp.dll");
        let output_root = directory.join("project-managed-structure");
        fs::write(
            &source_path,
            source_factory_fixture_bytes("mono_unloaded_valid"),
        )
        .unwrap();
        fs::write(
            &assembly_path,
            include_bytes!("processing/managed_assembly_fixture.dll"),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![source_path.clone(), assembly_path.clone()],
            subdirectories: Vec::new(),
        };
        let mut status = Vec::new();

        super::export_project_to(
            std::slice::from_ref(&directory),
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect("project export recovers assembly-backed MonoBehaviour fields");

        let status = String::from_utf8(status).expect("status output is UTF-8");
        assert!(
            status.contains("MonoBehaviour script structures: 1 recovered, 0 unresolved."),
            "{status}"
        );
        let assets = project_tree(&output_root)
            .into_iter()
            .filter(|path| path.ends_with(".asset"))
            .map(|path| fs::read_to_string(output_root.join(path)).unwrap())
            .collect::<Vec<_>>();
        let yaml = assets
            .iter()
            .find(|yaml| yaml.contains("inherited: 72623859790382856"))
            .expect("the Unity project contains the recovered MonoBehaviour fields");
        assert!(yaml.contains("m_ObjectHideFlags: 0"), "{yaml}");
        assert!(
            yaml.contains("m_CorrespondingSourceObject: {fileID: 0}"),
            "{yaml}"
        );
        assert!(yaml.contains("m_PrefabInstance: {fileID: 0}"), "{yaml}");
        assert!(yaml.contains("m_PrefabAsset: {fileID: 0}"), "{yaml}");
        assert!(yaml.contains("m_EditorHideFlags: 0"), "{yaml}");
        assert!(yaml.contains("m_EditorClassIdentifier:"), "{yaml}");
        assert!(!yaml.contains("m_Structure:"), "{yaml}");
        assert!(yaml.contains("inherited: 72623859790382856"), "{yaml}");
        assert!(yaml.contains("health: 1234"), "{yaml}");
        assert!(yaml.contains("speed: 1.5"), "{yaml}");
        assert_eq!(
            yaml.replace("\r\n", "\n"),
            include_str!("processing/managed_assembly_project_fixture.txt").replace("\r\n", "\n"),
            "the complete MonoBehaviour YAML, including the source script pointer, matches the frozen source ExportHandler fixture"
        );
        let script_path = output_root.join("Assets/Scripts/Assembly-CSharp/Game/HeroController.cs");
        let script_meta_path =
            output_root.join("Assets/Scripts/Assembly-CSharp/Game/HeroController.cs.meta");
        let script_bytes = fs::read(&script_path).expect("exported MonoScript source");
        let script_fixture_hex =
            include_str!("processing/managed_assembly_script_fixture.hex").trim();
        assert_eq!(
            script_fixture_hex.len() % 2,
            0,
            "script fixture has even hex length"
        );
        let script_fixture = script_fixture_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("script fixture hex is valid")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            script_bytes, script_fixture,
            "the exported source fallback matches the frozen AssetRipper script"
        );
        let script_meta = fs::read_to_string(&script_meta_path).expect("MonoImporter sidecar");
        let without_time_created = |value: &str| {
            value
                .lines()
                .filter(|line| !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        };
        assert_eq!(
            without_time_created(&script_meta),
            include_str!("processing/managed_assembly_script_meta_fixture.txt")
                .replace("\r\n", "\n"),
            "MonoImporter metadata matches the source after excluding only its creation timestamp"
        );
        assert!(
            !output_root.join("Assets/MonoScript/HeroController.asset").exists(),
            "a MonoScript is exported as its deterministic .cs target, not a generic serialized asset"
        );
        assert_eq!(
            fs::read(&source_path).unwrap(),
            source_factory_fixture_bytes("mono_unloaded_valid"),
            "project recovery must preserve the source SerializedFile"
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        input
            .subdirectories
            .extend(project_directories(&output_root));
    }

    #[test]
    fn export_project_matches_complex_missing_typetree_script_fields() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("processing/managed_assembly_schema_project_fixture.txt");
        let decode_hex = |hex: &str| {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                        .expect("source fixture contains valid hexadecimal")
                })
                .collect::<Vec<_>>()
        };
        let artifact_bytes = |prefix: &str| {
            let row = fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("managed-schema fixture includes {prefix}"));
            let mut fields = row.splitn(3, '|');
            let expected_length = fields
                .next()
                .expect("fixture artifact has a length")
                .parse::<usize>()
                .expect("fixture artifact length is numeric");
            let expected_hash = fields.next().expect("fixture artifact has a SHA-256");
            assert_eq!(expected_hash.len(), 64, "fixture hash is SHA-256");
            let bytes = decode_hex(
                fields
                    .next()
                    .expect("fixture artifact has hexadecimal bytes"),
            );
            assert_eq!(bytes.len(), expected_length, "fixture artifact length");
            bytes
        };

        let input_bytes = decode_hex(
            fixture
                .lines()
                .find_map(|line| line.strip_prefix("INPUT_HEX|managed_schema.assets|"))
                .expect("managed-schema fixture includes the SerializedFile input"),
        );
        let assembly_bytes =
            include_bytes!("processing/managed_assembly_schema_complex_fixture.dll");
        let assembly_row = fixture
            .lines()
            .find(|line| line.starts_with("ASSEMBLY|ComplexFixtureScripts.dll|"))
            .expect("managed-schema fixture includes the assembly identity");
        assert!(
            assembly_row
                .contains("|5120|D6E4155E931CFBC44ED92A44CC2436873D71DFBE293E9C568304DD118F2D6389"),
            "{assembly_row}"
        );
        assert_eq!(assembly_bytes.len(), 5_120);
        for object in [
            "OBJECT|MonoBehaviour|42|RichRoot",
            "OBJECT|MonoScript|7|RichController",
            "OBJECT|TextAsset|9|Target",
        ] {
            assert!(
                fixture.lines().any(|line| line == object),
                "missing {object}"
            );
        }

        let source_yaml =
            artifact_bytes("YAML_HEX|ExportedProject/Assets/MonoBehaviour/RichRoot.asset|");
        let source_target =
            artifact_bytes("TARGET_HEX|ExportedProject/Assets/TextAsset/Target.txt|");
        let source_target_meta =
            artifact_bytes("TARGET_META_HEX|ExportedProject/Assets/TextAsset/Target.txt.meta|");
        let source_plugin_meta = artifact_bytes(
            "PLUGIN_META_HEX|ExportedProject/Assets/Plugins/ComplexFixtureScripts.dll.meta|",
        );
        let source_yaml = String::from_utf8(source_yaml).expect("source YAML is UTF-8");
        assert_eq!(
            source_yaml.matches("__TEXTASSET_GUID__").count(),
            1,
            "the source fixture normalizes only the generated TextAsset pointer GUID"
        );

        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("managed_schema.assets");
        let assembly_path = directory.join("ComplexFixtureScripts.dll");
        let output_root = directory.join("project-managed-schema");
        fs::write(&source_path, &input_bytes).unwrap();
        fs::write(&assembly_path, assembly_bytes).unwrap();
        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![source_path.clone(), assembly_path.clone()],
            subdirectories: Vec::new(),
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                directory.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("the product project-export route recovers the complex managed schema");

        let behaviour_path = output_root.join("Assets/MonoBehaviour/RichRoot.asset");
        let target_path = output_root.join("Assets/TextAsset/Target.txt");
        let target_meta_path = output_root.join("Assets/TextAsset/Target.txt.meta");
        let plugin_meta_path = output_root.join("Assets/Plugins/ComplexFixtureScripts.dll.meta");
        let target_guid = meta_guid(&target_meta_path);
        let expected_yaml = source_yaml.replace("__TEXTASSET_GUID__", &target_guid);
        let actual_yaml = fs::read(&behaviour_path).expect("recovered MonoBehaviour YAML");
        assert_eq!(
            actual_yaml,
            expected_yaml.as_bytes(),
            "all recovered fields and both object pointers match the source project export after normalizing only the generated target GUID"
        );
        assert_eq!(
            fs::read(&target_path).expect("exported TextAsset"),
            source_target,
            "TextAsset bytes match the source project export"
        );

        let normalize_meta = |path: &std::path::Path| {
            fs::read_to_string(path)
                .expect("importer sidecar is readable")
                .lines()
                .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        };
        assert_eq!(
            normalize_meta(&target_meta_path).as_bytes(),
            source_target_meta,
            "TextScriptImporter metadata matches after removing only generated GUID and time"
        );
        assert_eq!(
            normalize_meta(&plugin_meta_path).as_bytes(),
            source_plugin_meta,
            "PluginImporter metadata matches after removing only generated GUID and time"
        );
        let exported_assembly =
            fs::read(output_root.join("Assets/Plugins/ComplexFixtureScripts.dll"))
                .expect("source-saved manager-backed assembly");
        let exported_assembly_model = ManagedAssembly::parse(
            "ComplexFixtureScripts.dll",
            &exported_assembly,
            ManagedAssemblyLimits::default(),
        )
        .expect("exported complex assembly parses");
        let source_assembly_model = ManagedAssembly::parse(
            "ComplexFixtureScripts.dll",
            include_bytes!("processing/managed_assembly_schema_complex_source_saved_fixture.dll"),
            ManagedAssemblyLimits::default(),
        )
        .expect("source-saved complex assembly parses");
        assert_eq!(
            exported_assembly_model.types, source_assembly_model.types,
            "project export preserves the source-saved managed type model"
        );
        assert_eq!(fs::read(&source_path).unwrap(), input_bytes);
        assert_eq!(fs::read(&assembly_path).unwrap(), assembly_bytes);

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        input
            .subdirectories
            .extend(project_directories(&output_root));
        drop(input);
    }

    fn assert_assembly_resolved_managed_reference_cli(fixture: &str) {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let decode_hex = |hex: &str| {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                        .expect("source fixture contains valid hexadecimal")
                })
                .collect::<Vec<_>>()
        };
        let input_bytes = |file_name: &str| {
            let prefix = format!("INPUT_HEX|{file_name}|");
            decode_hex(
                fixture
                    .lines()
                    .find_map(|line| line.strip_prefix(&prefix))
                    .unwrap_or_else(|| panic!("source fixture has input {file_name}")),
            )
        };
        let source_artifact = |key: &str, expected_path: &str| {
            let prefix = format!("{key}|");
            let row = fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .unwrap_or_else(|| panic!("source fixture has artifact {key}"));
            let mut fields = row.splitn(4, '|');
            assert_eq!(fields.next(), Some(expected_path));
            let expected_length = fields
                .next()
                .expect("source artifact length")
                .parse::<usize>()
                .expect("source artifact length is numeric");
            assert_eq!(fields.next().expect("source artifact SHA-256").len(), 64);
            let bytes = decode_hex(fields.next().expect("source artifact bytes"));
            assert_eq!(bytes.len(), expected_length);
            bytes
        };

        let assembly_bytes = include_bytes!("processing/managed_reference_schema_fixture.dll");
        let assembly_row = fixture
            .lines()
            .find(|line| line.starts_with("ASSEMBLY|ManagedReferenceFixtureScripts.dll|"))
            .expect("source fixture records the managed assembly");
        assert!(assembly_row
            .contains("|5120|1B20CEFF8CE7DE3E996D6497F3F7892B6472D53261DCED73F12D623FFDE902BB"));

        let directory = TestInputDirectory::new_root();
        let root_path = directory.join("managed_reference_schema.assets");
        let target_input_path = directory.join("target.assets");
        let assembly_path = directory.join("ManagedReferenceFixtureScripts.dll");
        fs::write(&root_path, input_bytes("managed_reference_schema.assets")).unwrap();
        fs::write(&target_input_path, input_bytes("target.assets")).unwrap();
        fs::write(&assembly_path, assembly_bytes).unwrap();
        let output_root = directory.join("project-managed-reference-schema");

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                directory.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("the product CLI recovers assembly-resolved managed references");

        let behaviour_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
        let target_path = output_root.join("Assets/TextAsset/Target.txt");
        let target_meta_path = output_root.join("Assets/TextAsset/Target.txt.meta");
        let plugin_path = output_root.join("Assets/Plugins/ManagedReferenceFixtureScripts.dll");
        let plugin_meta_path =
            output_root.join("Assets/Plugins/ManagedReferenceFixtureScripts.dll.meta");
        let source_yaml = String::from_utf8(source_artifact(
            "YAML_HEX",
            "ExportedProject/Assets/MonoBehaviour/ManagedRoot.asset",
        ))
        .expect("source MonoBehaviour YAML is UTF-8");
        let source_target =
            source_artifact("TARGET_HEX", "ExportedProject/Assets/TextAsset/Target.txt");
        let source_target_meta = source_artifact(
            "TARGET_META_HEX",
            "ExportedProject/Assets/TextAsset/Target.txt.meta",
        );
        let source_plugin_meta = source_artifact(
            "PLUGIN_META_HEX",
            "ExportedProject/Assets/Plugins/ManagedReferenceFixtureScripts.dll.meta",
        );
        let expected_yaml = source_yaml
            .replace("__TEXTASSET_GUID__", &meta_guid(&target_meta_path))
            .replace("__SCRIPT_GUID__", &meta_guid(&plugin_meta_path));
        let actual_yaml = fs::read_to_string(&behaviour_path).expect("project MonoBehaviour YAML");
        assert_eq!(
            actual_yaml.replace("\r\n", "\n"),
            expected_yaml.replace("\r\n", "\n"),
            "the CLI project output preserves the managed registry, cycle, and cross-file PPtr"
        );
        assert_eq!(fs::read(&target_path).unwrap(), source_target);

        let normalize_meta = |path: &std::path::Path| {
            fs::read_to_string(path)
                .expect("importer sidecar is readable")
                .lines()
                .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        };
        assert_eq!(
            normalize_meta(&target_meta_path).as_bytes(),
            source_target_meta
        );
        assert_eq!(
            normalize_meta(&plugin_meta_path).as_bytes(),
            source_plugin_meta
        );
        let exported_assembly = fs::read(&plugin_path).unwrap();
        let exported_assembly_model = ManagedAssembly::parse(
            "ManagedReferenceFixtureScripts.dll",
            &exported_assembly,
            ManagedAssemblyLimits::default(),
        )
        .expect("exported managed-reference assembly parses");
        let source_assembly_model = ManagedAssembly::parse(
            "ManagedReferenceFixtureScripts.dll",
            include_bytes!("processing/managed_reference_schema_source_saved_fixture.dll"),
            ManagedAssemblyLimits::default(),
        )
        .expect("source-saved managed-reference assembly parses");
        assert_eq!(
            exported_assembly_model.types, source_assembly_model.types,
            "project export preserves the source-saved managed type model"
        );
        assert_ne!(exported_assembly, assembly_bytes);
        assert_eq!(
            fs::read(&root_path).unwrap(),
            input_bytes("managed_reference_schema.assets")
        );
        assert_eq!(
            fs::read(&target_input_path).unwrap(),
            input_bytes("target.assets")
        );
        assert_eq!(fs::read(&assembly_path).unwrap(), assembly_bytes);
        assert!(output_root
            .join("ProjectSettings/ProjectVersion.txt")
            .is_file());
        assert!(output_root.join("Packages/manifest.json").is_file());
    }

    #[test]
    fn export_project_recovers_assembly_resolved_managed_references_without_file_typetrees() {
        assert_assembly_resolved_managed_reference_cli(include_str!(
            "processing/managed_reference_schema_project_fixture.txt"
        ));
    }

    #[test]
    fn export_project_recovers_legacy_assembly_managed_references_without_file_typetrees() {
        assert_assembly_resolved_managed_reference_cli(include_str!(
            "processing/managed_reference_legacy_schema_project_fixture.txt"
        ));
    }

    #[test]
    fn export_project_matches_source_generic_empty_script_without_a_manager() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("processing/generic_script_project_fixture.txt");
        let fixture_hex = |prefix: &str| {
            fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("generic MonoScript fixture includes {prefix}"))
        };
        let decode_hex = |hex: &str| {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                        .expect("fixture hex is valid")
                })
                .collect::<Vec<_>>()
        };

        let input_bytes = decode_hex(fixture_hex("INPUT_HEX|generic_script.assets|"));
        assert_eq!(input_bytes.len(), 200);
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("generic_script.assets");
        let output_root = directory.join("project-generic-empty-script");
        fs::write(&source_path, &input_bytes).unwrap();
        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![source_path.clone()],
            subdirectories: Vec::new(),
        };
        let mut status = Vec::new();
        super::export_project_to(
            std::slice::from_ref(&source_path),
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect("a source EmptyScriptExportCollection produces the generic script route");

        let script_relative = "Assets/Scripts/GameScripts/Game/Widget`1.cs";
        let sidecar_relative = "Assets/Scripts/GameScripts/Game/Widget`1.cs.meta";
        let definition_relative = "Assets/Scripts/GameScripts/GameScripts.asmdef";
        let read_expected = |relative: &str| {
            let prefix = format!("CONTENT_HEX|{relative}|");
            decode_hex(fixture_hex(&prefix))
        };
        let script_path = output_root.join(script_relative);
        let sidecar_path = output_root.join(sidecar_relative);
        let definition_path = output_root.join(definition_relative);
        let script = fs::read(&script_path).expect("generic C# output");
        assert_eq!(script, read_expected(script_relative));
        let sidecar = fs::read_to_string(&sidecar_path).expect("MonoImporter sidecar");
        let normalized_sidecar = sidecar
            .lines()
            .filter(|line| !line.starts_with("timeCreated: "))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(
            normalized_sidecar.as_bytes(),
            read_expected(sidecar_relative),
            "the source MonoImporter sidecar matches after excluding only timeCreated"
        );
        assert_eq!(
            fs::read(&definition_path).expect("source assembly definition"),
            read_expected(definition_relative),
            "the no-manager assembly definition matches source JSON bytes"
        );
        assert_eq!(fs::read(&source_path).unwrap(), input_bytes);

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        input
            .subdirectories
            .extend(project_directories(&output_root));
    }

    #[test]
    fn export_project_matches_source_reference_monoscript_pointer_without_a_manager() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("processing/reference_script_project_fixture.txt");
        let decode_hex = |hex: &str| {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                        .expect("fixture hex is valid")
                })
                .collect::<Vec<_>>()
        };
        let input_bytes = |name: &str| {
            let prefix = format!("INPUT_HEX|{name}|");
            fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .map(decode_hex)
                .unwrap_or_else(|| panic!("reference MonoScript fixture includes {prefix}"))
        };
        let expected_file = |relative: &str| {
            let prefix = format!("CONTENT_HEX|{relative}|");
            fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .map(decode_hex)
                .unwrap_or_else(|| panic!("reference MonoScript fixture includes {prefix}"))
        };

        let directory = TestInputDirectory::new_root();
        let script_path = directory.join("reference_script.assets");
        let behaviour_path = directory.join("reference_behaviour.assets");
        let output_root = directory.join("project-reference-script");
        let script_bytes = input_bytes("reference_script.assets");
        let behaviour_bytes = input_bytes("reference_behaviour.assets");
        fs::write(&script_path, &script_bytes).unwrap();
        fs::write(&behaviour_path, &behaviour_bytes).unwrap();
        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![script_path.clone(), behaviour_path.clone()],
            subdirectories: Vec::new(),
        };
        let mut status = Vec::new();
        super::export_project_to(
            &[script_path.clone(), behaviour_path.clone()],
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect("no-manager reference MonoScript and its dependent MonoBehaviour export");

        let script_relative = "Assets/Scripts/UnityEngine/UnityEngine/Object.cs";
        let sidecar_relative = "Assets/Scripts/UnityEngine/UnityEngine/Object.cs.meta";
        let definition_relative = "Assets/Scripts/UnityEngine/UnityEngine.asmdef";
        let behaviour_relative = "Assets/MonoBehaviour/ReferenceRoot.asset";
        let script = fs::read(output_root.join(script_relative)).expect("reference script output");
        assert_eq!(script, expected_file(script_relative));
        let sidecar = fs::read_to_string(output_root.join(sidecar_relative))
            .expect("script MonoImporter sidecar");
        let normalized_sidecar = sidecar
            .lines()
            .filter(|line| !line.starts_with("timeCreated: "))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(
            normalized_sidecar.as_bytes(),
            expected_file(sidecar_relative)
        );
        assert_eq!(
            fs::read(output_root.join(definition_relative)).expect("source assembly definition"),
            expected_file(definition_relative)
        );
        let behaviour = fs::read(output_root.join(behaviour_relative))
            .expect("MonoBehaviour referencing the framework MonoScript");
        assert_eq!(
            behaviour,
            expected_file(behaviour_relative),
            "the m_Script pointer must use the versioned reference GUID and script file ID"
        );
        assert_eq!(fs::read(&script_path).unwrap(), script_bytes);
        assert_eq!(fs::read(&behaviour_path).unwrap(), behaviour_bytes);

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        input
            .subdirectories
            .extend(project_directories(&output_root));
    }

    #[test]
    fn export_project_matches_source_manager_backed_mscorlib2_reference_pointer() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("processing/reference_script_mscorlib2_project_fixture.txt");
        let decode_hex = |hex: &str| {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                        .expect("fixture hex is valid")
                })
                .collect::<Vec<_>>()
        };
        let input_bytes = |name: &str| {
            let prefix = format!("INPUT_HEX|{name}|");
            fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .map(decode_hex)
                .unwrap_or_else(|| panic!("manager-backed reference fixture includes {prefix}"))
        };
        let expected_behaviour = fixture
            .lines()
            .find_map(|line| {
                line.strip_prefix("CONTENT_HEX|Assets/MonoBehaviour/ReferenceRoot.asset|")
            })
            .map(decode_hex)
            .expect("manager-backed fixture includes the source MonoBehaviour YAML");

        let directory = TestInputDirectory::new_root();
        let input_root = directory.join("manager-reference-input");
        fs::create_dir(&input_root).unwrap();
        let script_path = input_root.join("reference_script.assets");
        let behaviour_path = input_root.join("reference_behaviour.assets");
        let mscorlib_path = input_root.join("mscorlib.dll");
        let script_bytes = input_bytes("reference_script.assets");
        let behaviour_bytes = input_bytes("reference_behaviour.assets");
        let mscorlib_bytes = input_bytes("mscorlib.dll");
        fs::write(&script_path, &script_bytes).unwrap();
        fs::write(&behaviour_path, &behaviour_bytes).unwrap();
        fs::write(&mscorlib_path, &mscorlib_bytes).unwrap();
        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![
                script_path.clone(),
                behaviour_path.clone(),
                mscorlib_path.clone(),
            ],
            subdirectories: vec![input_root.clone()],
        };
        let output_root = directory.join("manager-reference-project");
        let mut status = Vec::new();
        super::export_project_to(
            &[input_root],
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect("manager-backed Mono.Cecil reference pointer export");

        let status = String::from_utf8(status).unwrap();
        assert!(status.contains(
            "Mono assembly metadata: 1 loaded from 1 candidates (0 native/non-CLI skipped)"
        ));
        assert!(status.contains("0 resolved, 0 valid, 0 abstract, 1 unresolved"));
        let behaviour_output = output_root.join("Assets/MonoBehaviour/ReferenceRoot.asset");
        assert_eq!(fs::read(&behaviour_output).unwrap(), expected_behaviour);
        let plugin_dlls = project_tree(&output_root)
            .iter()
            .filter(|path| path.starts_with("Assets/Plugins/") && path.ends_with(".dll"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            plugin_dlls,
            vec![
                "Assets/Plugins/System.Runtime.CompilerServices.Unsafe.dll".to_owned(),
                "Assets/Plugins/System.Runtime.InteropServices.dll".to_owned(),
            ],
            "the Unity 2019.1 manager map routes both generated forwarding modules into the project"
        );
        let unsafe_output =
            fs::read(output_root.join("Assets/Plugins/System.Runtime.CompilerServices.Unsafe.dll"))
                .unwrap();
        let normalize_coff_timestamp = |bytes: &[u8]| {
            let mut normalized = bytes.to_vec();
            let pe_header = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
            normalized[pe_header + 8..pe_header + 12].fill(0);
            normalized
        };
        let expected_unsafe =
            assetripper::processing::managed_assembly::ManagedAssembly::create_unsafe_forwarder(
                "mscorlib.dll",
                &mscorlib_bytes,
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            normalize_coff_timestamp(&unsafe_output),
            normalize_coff_timestamp(&expected_unsafe),
            "the project route writes the generated Unsafe forwarder with only its COFF timestamp varying"
        );
        let interop_output =
            fs::read(output_root.join("Assets/Plugins/System.Runtime.InteropServices.dll"))
                .unwrap();
        let expected_interop =
            assetripper::processing::managed_assembly::ManagedAssembly::create_interop_services_forwarder(
                "mscorlib.dll",
                &mscorlib_bytes,
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            normalize_coff_timestamp(&interop_output),
            normalize_coff_timestamp(&expected_interop),
            "the project route writes the generated InteropServices forwarder with only its COFF timestamp varying"
        );
        let normalize_plugin_meta = |text: &str| {
            text.lines()
                .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n")
        };
        for (relative_meta, source_fixture) in [
            (
                "Assets/Plugins/System.Runtime.CompilerServices.Unsafe.dll.meta",
                include_str!("processing/managed_assembly_unsafe_forwarder_2019a1_source_meta.txt"),
            ),
            (
                "Assets/Plugins/System.Runtime.InteropServices.dll.meta",
                include_str!(
                    "processing/managed_assembly_interop_forwarder_2019a1_source_meta.txt"
                ),
            ),
        ] {
            let rust_meta = fs::read_to_string(output_root.join(relative_meta)).unwrap();
            assert_eq!(
                normalize_plugin_meta(&rust_meta),
                normalize_plugin_meta(source_fixture),
                "the manager helper uses the frozen-source PluginImporter v2 layout"
            );
        }
        assert!(
            !project_tree(&output_root)
                .iter()
                .any(|path| path.to_ascii_lowercase().ends_with(".cs")),
            "a reference-map MonoScript uses the reference pointer without a local script file"
        );
        assert_eq!(fs::read(&script_path).unwrap(), script_bytes);
        assert_eq!(fs::read(&behaviour_path).unwrap(), behaviour_bytes);
        assert_eq!(fs::read(&mscorlib_path).unwrap(), mscorlib_bytes);

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        input
            .subdirectories
            .extend(project_directories(&output_root));
    }

    #[test]
    fn export_project_preserves_manager_backed_pointer_when_saved_assembly_is_absent() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture =
            include_str!("processing/managed_reference_missing_assembly_project_fixture.txt");
        let decode_hex = |hex: &str| {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                        .expect("fixture hex is valid")
                })
                .collect::<Vec<_>>()
        };
        let input_bytes = |name: &str| {
            let prefix = format!("INPUT_HEX|{name}|");
            fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .map(decode_hex)
                .unwrap_or_else(|| panic!("missing-assembly fixture includes {prefix}"))
        };
        let expected_behaviour = fixture
            .lines()
            .find_map(|line| {
                line.strip_prefix("CONTENT_HEX|Assets/MonoBehaviour/ReferenceRoot.asset|")
            })
            .map(decode_hex)
            .expect("missing-assembly fixture includes the source MonoBehaviour YAML");

        let directory = TestInputDirectory::new_root();
        let input_root = directory.join("manager-missing-assembly-input");
        fs::create_dir(&input_root).unwrap();
        let script_path = input_root.join("reference_script.assets");
        let behaviour_path = input_root.join("reference_behaviour.assets");
        let mscorlib_path = input_root.join("mscorlib.dll");
        let script_bytes = input_bytes("reference_script.assets");
        let behaviour_bytes = input_bytes("reference_behaviour.assets");
        let mscorlib_bytes = input_bytes("mscorlib.dll");
        fs::write(&script_path, &script_bytes).unwrap();
        fs::write(&behaviour_path, &behaviour_bytes).unwrap();
        fs::write(&mscorlib_path, &mscorlib_bytes).unwrap();
        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![
                script_path.clone(),
                behaviour_path.clone(),
                mscorlib_path.clone(),
            ],
            subdirectories: vec![input_root.clone()],
        };
        let output_root = directory.join("manager-missing-assembly-project");
        let mut status = Vec::new();
        super::export_project_to(
            &[input_root],
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect("preserve a manager-backed MonoScript pointer without its DLL input");

        let status = String::from_utf8(status).unwrap();
        assert!(status.contains(
            "Mono assembly metadata: 1 loaded from 1 candidates (0 native/non-CLI skipped)"
        ));
        assert!(status.contains("MonoScript types: 0 resolved, 0 valid, 0 abstract, 1 unresolved"));
        assert!(status.contains("1 MonoScript entries have unavailable source content"));
        assert!(status.contains(
            "source GUID/fileID pointer is retained and the missing script DLL is not fabricated"
        ));
        let behaviour_output = output_root.join("Assets/MonoBehaviour/ReferenceRoot.asset");
        assert_eq!(fs::read(&behaviour_output).unwrap(), expected_behaviour);
        let plugin_dlls = project_tree(&output_root)
            .iter()
            .filter(|path| path.starts_with("Assets/Plugins/") && path.ends_with(".dll"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            plugin_dlls,
            vec!["Assets/Plugins/System.Runtime.CompilerServices.Unsafe.dll".to_owned()],
            "the source compiler helper is exported, while the absent Mono.Cecil DLL is not fabricated"
        );
        let helper_meta = fs::read_to_string(
            output_root.join("Assets/Plugins/System.Runtime.CompilerServices.Unsafe.dll.meta"),
        )
        .unwrap();
        let source_helper_meta =
            include_str!("processing/managed_assembly_unsafe_forwarder_2023a16_source_meta.txt");
        let normalize_meta = |text: &str| {
            text.lines()
                .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(
            normalize_meta(&helper_meta),
            normalize_meta(source_helper_meta),
            "Unity 2023.3 uses the source PluginImporter v3 named platform layout"
        );
        assert_eq!(fs::read(&script_path).unwrap(), script_bytes);
        assert_eq!(fs::read(&behaviour_path).unwrap(), behaviour_bytes);
        assert_eq!(fs::read(&mscorlib_path).unwrap(), mscorlib_bytes);

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        input
            .subdirectories
            .extend(project_directories(&output_root));
    }

    #[test]
    fn export_project_decompiled_mode_keeps_pointer_without_absent_script_type_file() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!(
            "processing/managed_reference_missing_assembly_decompiled_project_fixture.txt"
        );
        let decode_hex = |hex: &str| {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                        .expect("fixture hex is valid")
                })
                .collect::<Vec<_>>()
        };
        let input_bytes = |name: &str| {
            let prefix = format!("INPUT_HEX|{name}|");
            fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .map(decode_hex)
                .unwrap_or_else(|| panic!("decompiled fixture includes {prefix}"))
        };
        let expected_behaviour = fixture
            .lines()
            .find_map(|line| {
                line.strip_prefix("CONTENT_HEX|Assets/MonoBehaviour/ReferenceRoot.asset|")
            })
            .map(decode_hex)
            .expect("decompiled fixture includes the source MonoBehaviour YAML");

        let directory = TestInputDirectory::new_root();
        let input_root = directory.join("manager-decompiled-missing-type-input");
        fs::create_dir(&input_root).unwrap();
        let script_path = input_root.join("reference_script.assets");
        let behaviour_path = input_root.join("reference_behaviour.assets");
        let mscorlib_path = input_root.join("mscorlib.dll");
        let script_bytes = input_bytes("reference_script.assets");
        let behaviour_bytes = input_bytes("reference_behaviour.assets");
        let mscorlib_bytes = input_bytes("mscorlib.dll");
        fs::write(&script_path, &script_bytes).unwrap();
        fs::write(&behaviour_path, &behaviour_bytes).unwrap();
        fs::write(&mscorlib_path, &mscorlib_bytes).unwrap();
        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![
                script_path.clone(),
                behaviour_path.clone(),
                mscorlib_path.clone(),
            ],
            subdirectories: vec![input_root.clone()],
        };
        let output_root = directory.join("manager-decompiled-missing-type-project");
        let mut settings = default_export_settings();
        settings.script_export_mode = ScriptExportMode::Decompiled;
        let mut status = Vec::new();
        super::export_project_to(&[input_root], &output_root, false, &settings, &mut status)
            .expect("preserve the decompile pointer without inventing missing script content");

        let status = String::from_utf8(status).unwrap();
        assert!(status.contains(
            "Mono assembly metadata: 1 loaded from 1 candidates (0 native/non-CLI skipped)"
        ));
        assert!(status.contains("MonoScript types: 0 resolved, 0 valid, 0 abstract, 1 unresolved"));
        assert!(status.contains("1 MonoScript entries have unavailable source content"));
        assert!(status.contains("no .cs file is emitted"));
        let behaviour_output = output_root.join("Assets/MonoBehaviour/ReferenceRoot.asset");
        assert_eq!(fs::read(&behaviour_output).unwrap(), expected_behaviour);
        assert!(
            !project_tree(&output_root).iter().any(|path| {
                let path = path.to_ascii_lowercase();
                path.contains("mono.cecil") && path.ends_with(".cs")
            }),
            "the source manager does not emit a Mono.Cecil script file for an absent type"
        );
        assert_eq!(fs::read(&script_path).unwrap(), script_bytes);
        assert_eq!(fs::read(&behaviour_path).unwrap(), behaviour_bytes);
        assert_eq!(fs::read(&mscorlib_path).unwrap(), mscorlib_bytes);

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        input
            .subdirectories
            .extend(project_directories(&output_root));
    }

    #[test]
    fn export_project_resolves_cross_file_reference_through_nested_unityfs() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let source_fixture = include_str!("export/managed_reference_nested_bundle_fixture.txt");
        let source_bundle_hex = source_fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|workspace.bundle|"))
            .expect("source-runtime fixture includes the complete nested UnityFS input");
        let source_bundle_bytes = source_bundle_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("source bundle fixture contains valid hex")
            })
            .collect::<Vec<_>>();
        let bundled_fixture =
            include_bytes!("export/managed_reference_project_fixture/nested_reference.bundle");
        assert_eq!(bundled_fixture, source_bundle_bytes.as_slice());

        let directory = TestInputDirectory::new_root();
        let bundle_path = directory.join("workspace.bundle");
        fs::write(&bundle_path, bundled_fixture).expect("source-runtime nested UnityFS fixture");
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![bundle_path.clone()],
            subdirectories: Vec::new(),
        };
        let output_root = directory.join("project-nested-bundle");
        let mut status = Vec::new();
        super::export_project_to(
            std::slice::from_ref(&bundle_path),
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect("cross-file PPtrs resolve through nested UnityFS bundles");

        let status = String::from_utf8(status).expect("export status is UTF-8");
        assert!(status.contains("2 SerializedFiles"), "{status}");
        assert!(status.contains("2 objects in 2 asset files"), "{status}");

        let root_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
        let root_meta_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset.meta");
        let target_path = output_root.join("Assets/MonoBehaviour/ReferencedRoot.asset");
        let target_meta_path = output_root.join("Assets/MonoBehaviour/ReferencedRoot.asset.meta");
        let rust_root_yaml = fs::read_to_string(&root_path).expect("root YAML is exported");
        let rust_target_yaml =
            fs::read_to_string(&target_path).expect("nested target YAML is exported");
        let normalize_meta = |path: &std::path::Path| {
            let normalized = fs::read_to_string(path)
                .expect("asset metadata is readable")
                .lines()
                .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n");
            normalized.trim_end_matches(['\r', '\n']).to_owned() + "\n"
        };
        assert_eq!(
            include_str!("export/managed_reference_project_fixture/source_nested_ManagedRoot.asset.meta.normalized"),
            normalize_meta(&root_meta_path)
        );
        assert_eq!(
            include_str!("export/managed_reference_project_fixture/source_nested_ReferencedRoot.asset.meta.normalized"),
            normalize_meta(&target_meta_path)
        );
        let source_target_meta = include_str!(
            "export/managed_reference_project_fixture/source_ReferencedRoot.asset.meta"
        );
        let source_guid = source_target_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("frozen source target metadata has a GUID");
        let rust_guid = meta_guid(&target_meta_path);
        let source_root_yaml =
            include_str!("export/managed_reference_project_fixture/source_ManagedRoot.asset");
        let source_target_yaml =
            include_str!("export/managed_reference_project_fixture/source_ReferencedRoot.asset");
        assert_eq!(
            source_root_yaml.replace(source_guid, "<TARGET_GUID>").trim_end(),
            rust_root_yaml.replace(&rust_guid, "<TARGET_GUID>").trim_end(),
            "the source-generated root YAML and nested-bundle export agree after GUID normalization"
        );
        assert_eq!(
            source_target_yaml.trim_end(),
            rust_target_yaml.trim_end(),
            "the referenced object loaded from the nested bundle keeps its source YAML"
        );
        assert!(rust_root_yaml.contains(&format!(
            "m_Asset: {{fileID: 11400000, guid: {rust_guid}, type: 2}}"
        )));
        assert!(rust_root_yaml.contains("        m_Link:\n          rid: 200\n"));
        assert!(rust_root_yaml.contains("        m_Link:\n          rid: 100\n"));
        assert!(rust_target_yaml.contains("m_Health: 789"));

        fs::remove_dir_all(&output_root).expect("remove nested-bundle project output");
        drop(input);
    }

    #[test]
    fn export_project_preserves_source_controller_behaviours_and_shared_editor_members() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let release =
            include_str!("resources/animator_behaviour_state_machine_source_2022_3_fixture.txt");
        let editor = include_str!(
            "resources/animator_behaviour_state_machine_editor_source_2022_3_input.hex"
        );
        let shared =
            include_str!("resources/animator_shared_controller_editor_source_2022_3_input.hex");
        let release_hex = release
            .lines()
            .find_map(|line| line.strip_prefix("SOURCE_SERIALIZED_FILE_HEX|"))
            .unwrap();
        let input_hex = |fixture: &'static str| {
            fixture
                .lines()
                .find(|line| !line.starts_with('#') && !line.is_empty())
                .unwrap()
        };
        for (name, hex, controllers) in [
            ("ReleaseController.assets", release_hex, 1),
            ("EditorController.assets", input_hex(editor), 1),
            ("SharedControllers.assets", input_hex(shared), 2),
        ] {
            let bytes = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>();
            let directory = TestInputDirectory::new_root();
            let source_path = directory.join(name);
            fs::write(&source_path, &bytes).unwrap();
            let input = TestInputDirectory {
                directory: directory.clone(),
                files: vec![source_path.clone()],
                subdirectories: Vec::new(),
            };
            let output_root = directory.join("project");
            let mut status = Vec::new();
            super::export_project_to(
                std::slice::from_ref(&source_path),
                &output_root,
                false,
                &default_export_settings(),
                &mut status,
            )
            .unwrap();
            let status = String::from_utf8(status).unwrap();
            assert!(
                status.contains(&format!(
                    "{} objects in {} asset files",
                    controllers * 26,
                    controllers
                )),
                "{status}"
            );
            let controller_directory = output_root.join("Assets/AnimatorController");
            let mut guids = std::collections::HashSet::new();
            for controller_name in ["Nested State Machine Source", "Shared State Machine Source"]
                .into_iter()
                .take(controllers)
            {
                let path = controller_directory.join(format!("{controller_name}.controller"));
                let yaml = fs::read_to_string(&path).unwrap();
                let ids = yaml
                    .lines()
                    .filter_map(|line| {
                        line.strip_prefix("--- !u!")
                            .map(|line| line.split_once(" &").unwrap().1.parse::<i64>().unwrap())
                    })
                    .collect::<std::collections::HashSet<_>>();
                assert_eq!(ids.len(), 26);
                assert_eq!(yaml.matches("--- !u!114 ").count(), 2);
                assert_eq!(yaml.matches("--- !u!1107 ").count(), 4);
                assert!(!yaml.contains("m_HideFlags:"));
                for reference in yaml.split("{fileID: ").skip(1) {
                    let id = reference.split_once('}').unwrap().0.parse::<i64>().unwrap();
                    assert!(
                        id == 0 || ids.contains(&id),
                        "{controller_name} has an undefined local fileID {id}"
                    );
                }
                let meta = controller_directory.join(format!("{controller_name}.controller.meta"));
                guids.insert(meta_guid(&meta));
                let meta = fs::read_to_string(meta).unwrap();
                assert!(meta.contains("NativeFormatImporter:"));
                assert!(meta.contains("mainObjectFileID: 9100000"));
            }
            assert_eq!(guids.len(), controllers);
            assert!(!output_root.join("Assets/MonoBehaviour").exists());
            assert!(!output_root.join("Assets/AnimatorStateMachine").exists());
            assert!(
                fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt"))
                    .unwrap()
                    .contains("2022.3.0f1")
            );
            assert_eq!(fs::read(&source_path).unwrap(), bytes);
            fs::remove_dir_all(&output_root).unwrap();
            drop(input);
        }
    }
