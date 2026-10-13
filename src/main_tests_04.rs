    #[test]
    fn export_file_yaml_preserves_object_order_and_streamed_resources() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("texture.resS");
        let output_path = directory.join("serialized-file.yaml");
        let serialized_bytes = make_streamed_image_texture_file_with_streams(&[
            ("texture.resS", 1, 2),
            ("texture.resS", 6, 2),
        ]);
        fs::write(&serialized_path, &serialized_bytes).unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                resource_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_file_yaml_to(
            &[serialized_path.clone(), resource_path.clone()],
            0,
            &output_path,
            &mut status,
        )
        .unwrap();
        let yaml = fs::read_to_string(&output_path).unwrap();
        let first_object = yaml.find("--- !u!189 &77").unwrap();
        let second_object = yaml.find("--- !u!189 &78").unwrap();
        assert!(first_object < second_object);
        let first_object_yaml = &yaml[first_object..second_object];
        let second_object_yaml = &yaml[second_object..];
        assert!(
            first_object_yaml.contains("m_ImageData_C189: 2"),
            "first streamed texture writes its byte count"
        );
        assert!(
            first_object_yaml.contains("_typelessdata: 3132"),
            "first streamed texture writes its inline bytes"
        );
        assert!(
            second_object_yaml.contains("m_ImageData_C189: 2"),
            "second streamed texture writes its byte count"
        );
        assert!(
            second_object_yaml.contains("_typelessdata: 3637"),
            "second streamed texture writes its inline bytes"
        );
        assert!(!yaml.contains("texture.resS"));
        assert_eq!(fs::read(&serialized_path).unwrap(), serialized_bytes);
        assert_eq!(fs::read(&resource_path).unwrap(), b"0123456789");
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote 2 ordered YAML objects from SerializedFile 0"));

        let original_yaml = yaml;
        let error = super::export_file_yaml_to(
            &[serialized_path, resource_path],
            0,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_to_string(&output_path).unwrap(), original_yaml);
        drop(input);
    }

    #[test]
    fn export_file_yaml_maps_intra_file_references_to_exported_path_ids() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("serialized-file.yaml");
        fs::write(&serialized_path, make_generation_10_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        super::run(
            [
                std::ffi::OsString::from("export-file-yaml"),
                std::ffi::OsString::from("0"),
                output_path.clone().into_os_string(),
                serialized_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();
        let yaml = fs::read_to_string(&output_path).unwrap();
        assert!(yaml.contains("--- !u!1 &42"));
        assert!(yaml.contains("m_Target: {fileID: 42}"));
        drop(input);
    }

    #[test]
    fn export_file_yaml_handles_source_generated_animation_workspace_without_typetrees() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("animation_workspace.assets");
        let output_path = directory.join("animation_workspace.yaml");
        let bytes = include_bytes!("resources/animation/animation_workspace_fixture.assets");
        fs::write(&serialized_path, bytes).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        super::run(
            [
                std::ffi::OsString::from("export-file-yaml"),
                std::ffi::OsString::from("0"),
                output_path.clone().into_os_string(),
                serialized_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        let yaml = fs::read_to_string(&output_path).unwrap();
        assert_eq!(yaml.matches("--- !u!").count(), 13);
        assert!(yaml.contains("m_SampleRate: 30"));
        assert!(yaml.contains("m_ConstantClip:"));
        assert!(yaml.contains("- 777\n          - 888"));
        assert!(yaml.contains("pptrCurveMapping:\n    - {fileID: 1010}\n    - {fileID: 0}"));
        assert!(yaml.contains("functionName: OnThing"));
        assert!(yaml.contains("m_PositionValues3: []"));
        assert_eq!(fs::read(&serialized_path).unwrap(), bytes);
        drop(input);
    }

    #[test]
    fn export_project_writes_a_cross_file_yaml_workspace() {
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("main.assets");
        let target_directory = directory.join("dependencies");
        fs::create_dir(&target_directory).unwrap();
        let target_path = target_directory.join("other.assets");
        let output_root = directory.join("project");
        fs::write(
            &source_path,
            make_generation_10_file_with_pointer(&["other.assets"], 1, 42),
        )
        .unwrap();
        fs::write(&target_path, make_generation_10_file()).unwrap();
        fs::create_dir(&output_root).unwrap();

        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![source_path.clone(), target_path.clone()],
            // Drop removes directories in reverse order, so keep parents before children.
            subdirectories: vec![
                target_directory.clone(),
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/GameObject"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
                target_directory.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        let source_asset = output_root.join("Assets/GameObject/GameObject.asset");
        let source_meta = output_root.join("Assets/GameObject/GameObject.asset.meta");
        let target_asset = output_root.join("Assets/GameObject/GameObject_0.asset");
        let target_meta = output_root.join("Assets/GameObject/GameObject_0.asset.meta");
        let source_yaml = fs::read_to_string(&source_asset).unwrap();
        let target_meta_text = fs::read_to_string(&target_meta).unwrap();
        let target_guid = target_meta_text
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("target asset sidecar has a GUID")
            .to_owned();

        assert!(source_yaml.contains("--- !u!1 &100000"));
        assert!(source_yaml.contains("m_Target: {fileID: 100000,"));
        assert!(source_yaml.contains(&format!("guid: {target_guid}")));
        assert!(fs::read_to_string(&target_asset)
            .unwrap()
            .contains("--- !u!1 &100000"));
        assert!(fs::read_to_string(&source_meta)
            .unwrap()
            .contains("NativeFormatImporter:"));
        assert_ne!(
            meta_guid(&source_meta),
            target_guid,
            "each exported object is its own baseline collection with its own GUID"
        );
        assert!(
            fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt"))
                .unwrap()
                .contains("2019.4.0f1")
        );
        assert!(
            fs::read_to_string(output_root.join("Packages/manifest.json"))
                .unwrap()
                .contains("\"dependencies\"")
        );
        input.files.extend([
            source_asset,
            source_meta,
            target_asset,
            target_meta,
            output_root.join("ProjectSettings/ProjectVersion.txt"),
            output_root.join("Packages/manifest.json"),
        ]);
    }

    #[test]
    fn export_project_routes_textasset_bytes_mode_and_importer() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn collect_output_tree(
            path: &std::path::Path,
            files: &mut Vec<PathBuf>,
            directories: &mut Vec<PathBuf>,
        ) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    directories.push(path.clone());
                    collect_output_tree(&path, files, directories);
                } else {
                    files.push(path);
                }
            }
        }

        fn source_textasset_meta(asset_name: &str) -> String {
            let prefix = format!("META|ExportedProject/Assets/TextAsset/{asset_name}.meta|");
            include_str!("export/textasset_project_fixture.txt")
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .unwrap_or_else(|| panic!("source TextAsset metadata row missing for {asset_name}"))
                .replace("\\n", "\n")
        }

        fn source_textasset_asset_row(asset_name: &str) -> String {
            let prefix = format!("ASSET|ExportedProject/Assets/TextAsset/{asset_name}|");
            include_str!("export/textasset_project_fixture.txt")
                .lines()
                .find(|line| line.starts_with(&prefix))
                .unwrap_or_else(|| panic!("source TextAsset asset row missing for {asset_name}"))
                .to_owned()
        }

        let fixture = assetripper::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|textasset_notree|"))
            .expect(
                "source-factory fixture includes a generated TextAsset without a file TypeTree",
            );
        let source_bytes = decode_hex(hex);
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("textasset_notree.assets");
        let output_root = directory.join("project-textasset-parse");
        fs::write(&source_path, &source_bytes).unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![source_path.clone()],
            subdirectories: Vec::new(),
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("export-project uses the source-generated TPK fallback");

        let text_asset_path = output_root.join("Assets/TextAsset/Hero.txt");
        let text_asset_meta_path = output_root.join("Assets/TextAsset/Hero.txt.meta");
        assert_eq!(
            source_textasset_asset_row("Hero.txt"),
            "ASSET|ExportedProject/Assets/TextAsset/Hero.txt|cHJpbnQoMSk="
        );
        assert_eq!(fs::read(&text_asset_path).unwrap(), b"print(1)");
        let normalized_text_asset_meta = fs::read_to_string(&text_asset_meta_path)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("guid:") && !line.starts_with("timeCreated:"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(
            normalized_text_asset_meta,
            source_textasset_meta("Hero.txt")
        );
        assert!(
            normalized_text_asset_meta.contains("TextScriptImporter:"),
            "{normalized_text_asset_meta}"
        );
        assert!(!normalized_text_asset_meta.contains("NativeFormatImporter:"));
        for field in [
            "externalObjects: {}",
            "userData:",
            "assetBundleName:",
            "assetBundleVariant:",
        ] {
            assert!(
                normalized_text_asset_meta.contains(field),
                "{normalized_text_asset_meta}"
            );
        }

        let settings_path = input.directory.join("textasset-bytes-settings.json");
        let default_settings = include_str!("export/settings_file_defaults.json");
        let bytes_settings =
            default_settings.replace("\"TextExportMode\": 2", "\"TextExportMode\": 0");
        assert_ne!(bytes_settings, default_settings);
        fs::write(&settings_path, bytes_settings).unwrap();
        let bytes_output_root = input.directory.join("project-textasset-bytes");
        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.clone().into_os_string(),
                bytes_output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("export-project applies TextExportMode::Bytes to TextAssets");
        let bytes_asset_path = bytes_output_root.join("Assets/TextAsset/Hero.bytes");
        let bytes_asset_meta_path = bytes_output_root.join("Assets/TextAsset/Hero.bytes.meta");
        assert_eq!(
            source_textasset_asset_row("Hero.bytes"),
            "ASSET|ExportedProject/Assets/TextAsset/Hero.bytes|cHJpbnQoMSk="
        );
        assert_eq!(fs::read(&bytes_asset_path).unwrap(), b"print(1)");
        let normalized_bytes_meta = fs::read_to_string(&bytes_asset_meta_path)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("guid:") && !line.starts_with("timeCreated:"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(normalized_bytes_meta, source_textasset_meta("Hero.bytes"));

        let mut exported_files = Vec::new();
        let mut exported_directories = Vec::new();
        collect_output_tree(&output_root, &mut exported_files, &mut exported_directories);
        collect_output_tree(
            &bytes_output_root,
            &mut exported_files,
            &mut exported_directories,
        );
        let mut asset_files = exported_files.iter().filter(|path| path
            .extension().is_some_and(|extension| extension == "asset")).cloned().collect::<Vec<_>>();
        asset_files.sort();
        let mut expected_settings = vec![
            output_root.join("ProjectSettings/EditorSettings.asset"),
            bytes_output_root.join("ProjectSettings/EditorSettings.asset"),
        ];
        expected_settings.sort();
        assert_eq!(asset_files, expected_settings, "TextAsset routes emit no raw YAML assets; only the source-generated EditorSettings are .asset files");
        assert_eq!(fs::read(&source_path).unwrap(), source_bytes);

        input.files.extend(exported_files);
        input.subdirectories.extend(exported_directories);
    }

    #[test]
    fn export_project_path_id_map_matches_source_for_all_asset_interfaces() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("export/path_id_map_source_2019_4_fixture.txt");

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        let input_bytes = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|PathMapFixture.assets|"))
            .map(decode_hex)
            .expect("source fixture contains its SerializedFile bytes");
        let source_input_identity = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT|PathMapFixture.assets|"))
            .expect("source fixture contains its SerializedFile identity");
        let source_input_columns = source_input_identity.split('|').collect::<Vec<_>>();
        assert_eq!(source_input_columns.len(), 2);
        assert_eq!(
            source_input_columns[0].parse::<usize>().unwrap(),
            input_bytes.len()
        );
        assert_eq!(
            source_input_columns[1],
            "821FEB2CB7CE45575D9BDD1690D918742C39FB96063070C70DC9456041102D36"
        );
        let expected_map = fixture
            .lines()
            .find_map(|line| line.strip_prefix("PATH_ID_MAP_HEX|"))
            .map(decode_hex)
            .expect("source fixture contains the production PathIdMap bytes");
        let source_identity = fixture
            .lines()
            .find_map(|line| line.strip_prefix("PATH_ID_MAP|"))
            .expect("source fixture contains PathIdMap length and hash");
        let source_columns = source_identity.split('|').collect::<Vec<_>>();
        assert_eq!(source_columns.len(), 2);
        assert_eq!(
            source_columns[0].parse::<usize>().unwrap(),
            expected_map.len()
        );
        assert_eq!(
            source_columns[1],
            "A72BFB37C562ABA832F0F6E427993013A3138D7ACDE14527B60EF7B65D2ACCBA"
        );

        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("PathMapFixture.assets");
        let output_root = directory.join("path-id-map-project");
        fs::write(&source_path, &input_bytes).unwrap();
        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("production export-project writes the all-interface source fixture");

        let path_id_map = fs::read(output_root.join("AuxiliaryFiles/path_id_map.json")).unwrap();
        assert_eq!(
            normalize_text_line_endings(&path_id_map),
            normalize_text_line_endings(&expected_map)
        );
        let path_id_map_text = std::str::from_utf8(&path_id_map).unwrap();
        for expected in [
            "\"PathID\": 101",
            "\"Name\": \"MeshFixture\"",
            "\"Type\": \"Mesh\"",
            "\"PathID\": 202",
            "\"Name\": \"TextureFixture\"",
            "\"Type\": \"Texture2D\"",
            "\"PathID\": 303",
            "\"Name\": \"AudioFixture\"",
            "\"Type\": \"AudioClip\"",
            "\"PathID\": 404",
            "\"Name\": \"TextFixture\"",
            "\"Type\": \"TextAsset\"",
        ] {
            assert!(path_id_map_text.contains(expected), "missing {expected}");
        }
        assert_eq!(fs::read(&source_path).unwrap(), input_bytes);
    }

    #[test]
    fn export_project_matches_source_for_empty_audio_yaml_fallback() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let input_fixture = include_str!("export/path_id_map_source_2019_4_fixture.txt");
        let output_fixture =
            include_str!("export/audio_project_fallback_source_2019_4_fixture.txt");

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn fixture_hex(fixture: &str, prefix: &str) -> Vec<u8> {
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("source audio fixture is missing {prefix}"));
            decode_hex(hex)
        }

        fn normalized_lines(text: &str) -> String {
            let normalized = text
                .replace("\r\n", "\n")
                .lines()
                .collect::<Vec<_>>()
                .join("\n");
            format!("{normalized}\n")
        }

        fn normalized_meta(text: &str) -> String {
            let normalized = text
                .lines()
                .map(|line| {
                    if line.starts_with("guid: ") {
                        "guid: <GUID>"
                    } else if line.starts_with("timeCreated: ") {
                        "timeCreated: <TIME>"
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{normalized}\n")
        }

        let input_bytes = fixture_hex(input_fixture, "INPUT_HEX|PathMapFixture.assets|");
        let expected_yaml = fixture_hex(output_fixture, "SOURCE_YAML_HEX|");
        let expected_meta = fixture_hex(output_fixture, "SOURCE_META_NORMALIZED_HEX|");
        let expected_yaml_row = output_fixture
            .lines()
            .find(|line| line.starts_with("SOURCE_YAML|"))
            .expect("source fixture records the AudioClip YAML identity")
            .split('|')
            .collect::<Vec<_>>();
        assert_eq!(expected_yaml_row.len(), 4);
        assert_eq!(
            expected_yaml_row[2].parse::<usize>().unwrap(),
            expected_yaml.len()
        );
        assert_eq!(
            expected_yaml_row[3],
            "D095CD9DF9BB19BB49E841079E94F1B4BE652E0C846B456FE0238786EACD018F"
        );

        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("PathMapFixture.assets");
        let output_root = directory.join("audio-project-fallback");
        fs::write(&source_path, &input_bytes).unwrap();
        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("production export-project writes the source AudioClip fallback");

        let audio_path = output_root.join("Assets/AudioClip/AudioFixture.audioclip");
        let audio_meta_path = output_root.join("Assets/AudioClip/AudioFixture.audioclip.meta");
        assert!(audio_path.is_file(), "source extension is audioclip");
        assert!(
            audio_meta_path.is_file(),
            "source YAML audio collection emits metadata"
        );
        assert!(
            !output_root
                .join("Assets/AudioClip/AudioFixture.asset")
                .exists(),
            "AudioClip must not silently use the generic asset extension"
        );
        let actual_yaml = fs::read_to_string(&audio_path).unwrap();
        assert_eq!(
            normalized_lines(&actual_yaml).as_bytes(),
            normalized_lines(std::str::from_utf8(&expected_yaml).unwrap()).as_bytes(),
            "empty AudioClip YAML matches frozen AssetRipper output"
        );
        let actual_meta = fs::read_to_string(&audio_meta_path).unwrap();
        assert_eq!(
            normalized_meta(&actual_meta).as_bytes(),
            expected_meta,
            "NativeFormatImporter metadata matches frozen AssetRipper output"
        );
        assert_eq!(fs::read(&source_path).unwrap(), input_bytes);
    }

    #[test]
    fn export_project_writes_streamed_audio_yaml_sidecar_and_rebases_its_range() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("export/audio_project_stream_source_2019_4_fixture.txt");

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn fixture_hex(fixture: &str, prefix: &str) -> Vec<u8> {
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("streamed AudioClip fixture is missing {prefix}"));
            decode_hex(hex)
        }

        fn source_output(fixture: &str, relative_path: &str) -> Vec<u8> {
            let prefix = format!("SOURCE_OUTPUT|ExportedProject/{relative_path}|");
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .unwrap_or_else(|| {
                    panic!("source streamed AudioClip output is missing {relative_path}")
                });
            decode_hex(hex)
        }

        fn normalized_lines(text: &str) -> String {
            let newline_normalized = text.replace("\r\n", "\n");
            let normalized = newline_normalized
                .lines()
                .map(|line| {
                    let key = line
                        .split_once(':')
                        .map(|(key, _)| key.trim())
                        .unwrap_or_default();
                    if key.to_ascii_lowercase().contains("source") {
                        line.replace('\\', "/")
                    } else {
                        line.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{normalized}\n")
        }

        fn normalized_meta(text: &str) -> String {
            let normalized = text
                .lines()
                .map(|line| {
                    if line.starts_with("guid: ") {
                        "guid: <GUID>"
                    } else if line.starts_with("timeCreated: ") {
                        "timeCreated: <TIME>"
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{normalized}\n")
        }

        let input_assets = fixture_hex(fixture, "INPUT_HEX|AudioFixture.assets|");
        let input_resource = fixture_hex(fixture, "INPUT_HEX|AudioFixture.resS|");
        let source_yaml = source_output(fixture, "Assets/AudioClip/AudioFixture.audioclip");
        let source_sidecar = source_output(fixture, "Assets/AudioClip/AudioFixture.audioclip.resS");
        let expected_meta = source_output(fixture, "Assets/AudioClip/AudioFixture.audioclip.meta");
        assert_eq!(source_sidecar, b"ABC");
        let source_yaml_text = std::str::from_utf8(&source_yaml).unwrap();
        assert!(source_yaml_text.contains("m_Offset: 6\n    m_Size: 3\n"));
        assert_eq!(source_sidecar.len(), 3);
        let expected_yaml = source_yaml_text.replacen(
            "m_Offset: 6\n    m_Size: 3\n",
            "m_Offset: 0\n    m_Size: 3\n",
            1,
        );

        let directory = TestInputDirectory::new_root();
        let input_root = directory.join("streamed-audio-input");
        fs::create_dir(&input_root).unwrap();
        let source_path = input_root.join("AudioFixture.assets");
        let resource_path = input_root.join("AudioFixture.resS");
        let settings_path = directory.join("audio-yaml-settings.json");
        let output_root = directory.join("streamed-audio-project");
        let settings = include_str!("export/settings_file_defaults.json");
        assert!(settings.contains("\"AudioExportFormat\": 2"));
        fs::write(&source_path, &input_assets).unwrap();
        fs::write(&resource_path, &input_resource).unwrap();
        fs::write(&settings_path, settings).unwrap();
        let _input_guard = TestInputDirectory {
            directory: directory.clone(),
            files: vec![
                source_path.clone(),
                resource_path.clone(),
                settings_path.clone(),
            ],
            subdirectories: vec![input_root.clone()],
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.into_os_string(),
                output_root.clone().into_os_string(),
                input_root.into_os_string(),
            ]
            .into_iter(),
        )
        .expect("production export-project resolves and stages streamed AudioClip bytes");

        let audio_path = output_root.join("Assets/AudioClip/AudioFixture.audioclip");
        let audio_sidecar_path = output_root.join("Assets/AudioClip/AudioFixture.audioclip.resS");
        let audio_meta_path = output_root.join("Assets/AudioClip/AudioFixture.audioclip.meta");
        let actual_yaml = fs::read_to_string(&audio_path).unwrap();
        assert_eq!(
            normalized_lines(&actual_yaml),
            normalized_lines(&expected_yaml),
            "the only source difference is rebasing the sliced sidecar offset to zero"
        );
        assert!(normalized_lines(&actual_yaml).contains(
            normalized_lines("m_Source: Assets\\AudioClip\\AudioFixture.audioclip.resS")
                .trim_end()
        ));
        assert!(
            normalized_lines(&actual_yaml).contains(
                normalized_lines("m_Offset: 0\n    m_Size: 3").trim_end()
            )
        );
        assert_eq!(fs::read(&audio_sidecar_path).unwrap(), source_sidecar);
        assert_eq!(
            normalized_meta(&fs::read_to_string(&audio_meta_path).unwrap()).as_bytes(),
            expected_meta.as_slice(),
            "NativeFormatImporter metadata matches the source output"
        );
        assert_eq!(fs::read(&source_path).unwrap(), input_assets);
        assert_eq!(fs::read(&resource_path).unwrap(), input_resource);
        assert!(!output_root
            .join("Assets/AudioClip/AudioFixture.asset")
            .exists());
        fs::remove_dir_all(&output_root).unwrap();
    }

    #[test]
    fn export_project_rejects_missing_or_truncated_streamed_audio_without_partial_assets() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("export/audio_project_stream_source_2019_4_fixture.txt");

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        let input_assets_hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|AudioFixture.assets|"))
            .expect("fixture has the AudioClip SerializedFile bytes");
        let input_assets = decode_hex(input_assets_hex);
        let settings = include_str!("export/settings_file_defaults.json");
        let cases: [(&str, Option<&[u8]>, &str); 2] = [
            ("missing", None, "is missing"),
            ("truncated", Some(b"prefixAB"), "range"),
        ];

        for (case_name, resource_bytes, expected_reason) in cases {
            let directory = TestInputDirectory::new_root();
            let input_root = directory.join(format!("{case_name}-audio-input"));
            fs::create_dir(&input_root).unwrap();
            let source_path = input_root.join("AudioFixture.assets");
            let resource_path = input_root.join("AudioFixture.resS");
            let settings_path = directory.join(format!("{case_name}-audio-settings.json"));
            let output_root = directory.join(format!("{case_name}-audio-project"));
            fs::write(&source_path, &input_assets).unwrap();
            if let Some(resource_bytes) = resource_bytes {
                fs::write(&resource_path, resource_bytes).unwrap();
            }
            fs::write(&settings_path, settings).unwrap();
            let mut files = vec![source_path.clone(), settings_path.clone()];
            if resource_path.is_file() {
                files.push(resource_path.clone());
            }
            let _input_guard = TestInputDirectory {
                directory: directory.clone(),
                files,
                subdirectories: vec![input_root.clone()],
            };

            let error = super::run(
                [
                    std::ffi::OsString::from("export-project"),
                    std::ffi::OsString::from("--settings"),
                    settings_path.into_os_string(),
                    output_root.clone().into_os_string(),
                    input_root.into_os_string(),
                ]
                .into_iter(),
            )
            .expect_err("declared streamed AudioClip bytes cannot be silently omitted")
            .to_string();
            assert!(error.contains("AudioClip stream"), "{error}");
            assert!(error.contains(expected_reason), "{error}");

            let audio_path = output_root.join("Assets/AudioClip/AudioFixture.audioclip");
            let sidecar_path = output_root.join("Assets/AudioClip/AudioFixture.audioclip.resS");
            let meta_path = output_root.join("Assets/AudioClip/AudioFixture.audioclip.meta");
            assert!(
                !audio_path.exists(),
                "failed export left an AudioClip asset"
            );
            assert!(
                !sidecar_path.exists(),
                "failed export left a resource sidecar"
            );
            assert!(!meta_path.exists(), "failed export left AudioClip metadata");
            if output_root.is_dir() {
                assert_eq!(
                    fs::read_dir(&output_root).unwrap().count(),
                    0,
                    "failed export may not leave partial project files"
                );
                fs::remove_dir(&output_root).unwrap();
            }
            assert_eq!(fs::read(&source_path).unwrap(), input_assets);
            if let Some(expected_resource_bytes) = resource_bytes {
                assert_eq!(fs::read(&resource_path).unwrap(), expected_resource_bytes);
            }
        }
    }

    #[test]
    fn export_project_matches_source_for_standalone_generated_texture_versions() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn source_row_bytes(fixture: &str, relative_path: &str) -> Vec<u8> {
            let source_path = if relative_path.starts_with("AuxiliaryFiles/") {
                relative_path.to_owned()
            } else {
                format!("ExportedProject/{relative_path}")
            };
            let prefix = format!("SOURCE_OUTPUT|{source_path}|");
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .unwrap_or_else(|| panic!("source texture fixture has {relative_path}"));
            decode_hex(hex)
        }

        fn normalize_meta(text: &str) -> String {
            let normalized = text
                .lines()
                .map(|line| {
                    if line.starts_with("guid: ") {
                        "guid: <GUID>"
                    } else if line.starts_with("timeCreated: ") {
                        "timeCreated: <TIME>"
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{normalized}\n")
        }

        fn collect_output_tree(
            path: &std::path::Path,
            files: &mut Vec<PathBuf>,
            directories: &mut Vec<PathBuf>,
        ) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    directories.push(path.clone());
                    collect_output_tree(&path, files, directories);
                } else {
                    files.push(path);
                }
            }
        }

        let settings = include_str!("export/settings_file_defaults.json")
            .replace("\"ImageExportFormat\": 4", "\"ImageExportFormat\": 0")
            .replace(
                "\"PreferOriginalTextureExtension\": true",
                "\"PreferOriginalTextureExtension\": false",
            );
        assert!(settings.contains("\"ImageExportFormat\": 0"));
        assert!(settings.contains("\"PreferOriginalTextureExtension\": false"));
        for (unity_version, storage, fixture) in [
            (
                "2019.4.40f1",
                "inline",
                include_str!("export/texture_project_source_2019_4_fixture.txt"),
            ),
            (
                "2022.3.22f1",
                "inline",
                include_str!("export/texture_project_source_2022_3_fixture.txt"),
            ),
            (
                "2019.4.40f1",
                "streamed",
                include_str!("export/texture_project_source_2019_4_streamed_fixture.txt"),
            ),
            (
                "2022.3.22f1",
                "streamed",
                include_str!("export/texture_project_source_2022_3_streamed_fixture.txt"),
            ),
        ] {
            let directory = TestInputDirectory::new_root();
            let mut has_serialized_file = false;
            let mut source_inputs = Vec::new();
            for input_row in fixture
                .lines()
                .filter_map(|line| line.strip_prefix("INPUT_HEX|"))
            {
                let (file_name, hex) = input_row
                    .split_once('|')
                    .expect("source input row contains a path and hex bytes");
                let bytes = decode_hex(hex);
                let path = directory.join(file_name);
                fs::write(&path, &bytes).unwrap();
                if file_name == "TextureFixture.assets" {
                    has_serialized_file = true;
                }
                source_inputs.push((path, bytes));
            }
            assert!(
                has_serialized_file,
                "source fixture contains its SerializedFile"
            );
            let input_paths = source_inputs
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            let settings_path = directory.join("texture-settings.json");
            let output_root = directory.join("project-texture");
            fs::write(&settings_path, settings.as_bytes()).unwrap();
            let mut input = TestInputDirectory {
                directory: directory.clone(),
                files: input_paths.clone(),
                subdirectories: Vec::new(),
            };
            input.files.push(settings_path.clone());

            let mut arguments = vec![
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.clone().into_os_string(),
                output_root.clone().into_os_string(),
            ];
            arguments.extend(
                input_paths
                    .iter()
                    .map(|path| path.as_os_str().to_os_string()),
            );
            super::run(arguments.into_iter()).unwrap_or_else(|error| {
                panic!("export-project failed for Unity {unity_version} {storage}: {error}")
            });

            let texture_path = output_root.join("Assets/Texture2D/TextureFixture.bmp");
            let texture_meta_path = output_root.join("Assets/Texture2D/TextureFixture.bmp.meta");
            let texture_bytes = fs::read(&texture_path).unwrap();
            assert_eq!(
                texture_bytes,
                source_row_bytes(fixture, "Assets/Texture2D/TextureFixture.bmp"),
                "converted image differs for Unity {unity_version} {storage}"
            );
            let texture_meta = normalize_meta(&fs::read_to_string(&texture_meta_path).unwrap());
            assert_eq!(
                texture_meta.as_bytes(),
                source_row_bytes(fixture, "Assets/Texture2D/TextureFixture.bmp.meta"),
                "TextureImporter sidecar differs for Unity {unity_version} {storage}"
            );
            assert_eq!(
                fs::read(output_root.join("ProjectSettings/ProjectVersion.txt")).unwrap(),
                source_row_bytes(fixture, "ProjectSettings/ProjectVersion.txt"),
                "project version output differs for Unity {unity_version} {storage}"
            );
            assert_eq!(
                fs::read(output_root.join("ProjectSettings/EditorSettings.asset")).unwrap(),
                source_row_bytes(fixture, "ProjectSettings/EditorSettings.asset"),
                "generated editor settings differ for Unity {unity_version} {storage}"
            );
            let source_manifest =
                String::from_utf8(source_row_bytes(fixture, "Packages/manifest.json"))
                    .expect("source package manifest is UTF-8");
            let rust_manifest =
                fs::read_to_string(output_root.join("Packages/manifest.json")).unwrap();
            let normalize_manifest = |text: &str| {
                let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
                normalized.trim_end_matches('\n').to_owned() + "\n"
            };
            assert_eq!(
                normalize_manifest(&rust_manifest),
                normalize_manifest(&source_manifest),
                "package manifest differs beyond line endings for Unity {unity_version} {storage}"
            );
            assert_eq!(
                normalize_text_line_endings(&fs::read(output_root.join("AuxiliaryFiles/path_id_map.json")).unwrap()),
                normalize_text_line_endings(&source_row_bytes(fixture, "AuxiliaryFiles/path_id_map.json")),
                "PathIdMap post-export output differs for Unity {unity_version} {storage}"
            );
            assert!(!output_root
                .join("Assets/Texture2D/TextureFixture.texture2D")
                .exists());
            for (source_input_path, source_input_bytes) in &source_inputs {
                assert_eq!(
                    fs::read(source_input_path).unwrap(),
                    *source_input_bytes,
                    "export modified input {} for Unity {unity_version} {storage}",
                    source_input_path.display()
                );
            }

            let mut exported_files = Vec::new();
            let mut exported_directories = Vec::new();
            collect_output_tree(&output_root, &mut exported_files, &mut exported_directories);
            let mut actual_paths = exported_files.iter()
                .map(|path| path.strip_prefix(&output_root).unwrap().to_str().unwrap().replace('\\', "/"))
                .collect::<Vec<_>>();
            let mut expected_paths = fixture.lines()
                .filter_map(|line| line.strip_prefix("SOURCE_OUTPUT|"))
                .map(|row| {
                    let path = row.split_once('|').unwrap().0;
                    path.strip_prefix("ExportedProject/").unwrap_or(path).to_owned()
                }).collect::<Vec<_>>();
            actual_paths.sort();
            expected_paths.sort();
            assert_eq!(actual_paths, expected_paths, "the entire exported file inventory matches the source for Unity {unity_version} {storage}");
            input.files.extend(exported_files);
            input.subdirectories.extend(exported_directories);
        }

        let streamed_fixture =
            include_str!("export/texture_project_source_2019_4_streamed_fixture.txt");
        let stream_input = |file_name: &str| {
            let prefix = format!("INPUT_HEX|{file_name}|");
            let hex = streamed_fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .unwrap_or_else(|| panic!("stream fixture contains {file_name}"));
            decode_hex(hex)
        };
        let serialized_bytes = stream_input("TextureFixture.assets");
        let resource_bytes = stream_input("TextureFixture.resS");
        for (supply_resource, resource_length) in [(false, 0), (true, resource_bytes.len() - 1)] {
            let directory = TestInputDirectory::new_root();
            let source_path = directory.join("TextureFixture.assets");
            let resource_path = directory.join("TextureFixture.resS");
            let settings_path = directory.join("texture-settings.json");
            let output_root = directory.join("project-texture-missing-stream");
            fs::write(&source_path, &serialized_bytes).unwrap();
            fs::write(&settings_path, settings.as_bytes()).unwrap();
            fs::create_dir(&output_root).unwrap();
            let mut input = TestInputDirectory {
                directory,
                files: vec![source_path.clone(), settings_path.clone()],
                subdirectories: vec![output_root.clone()],
            };
            let mut arguments = vec![
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--settings"),
                settings_path.into_os_string(),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ];
            if supply_resource {
                let invalid_resource = &resource_bytes[..resource_length];
                fs::write(&resource_path, invalid_resource).unwrap();
                input.files.push(resource_path.clone());
                arguments.push(resource_path.clone().into_os_string());
            }

            let error = super::run(arguments.into_iter()).unwrap_err();
            assert!(
                error.to_string().contains("TextureFixture.resS"),
                "stream resolution error lacks its resource path: {error}"
            );
            assert_eq!(
                fs::read_dir(&output_root).unwrap().count(),
                0,
                "failed streamed Texture2D export wrote partial project files"
            );
            assert_eq!(fs::read(&source_path).unwrap(), serialized_bytes);
            if supply_resource {
                assert_eq!(
                    fs::read(&resource_path).unwrap(),
                    &resource_bytes[..resource_length]
                );
            }
        }
    }

    #[test]
    fn export_project_routes_lightmap_textures_beside_their_scene() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("export/lightmap_project_source_2019_4_fixture.txt");

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn source_output(fixture: &str, relative_path: &str) -> Vec<u8> {
            let prefix = format!("SOURCE_OUTPUT|{relative_path}|");
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .unwrap_or_else(|| panic!("source lightmap fixture has {relative_path}"));
            decode_hex(hex)
        }

        fn normalize_generated_text(value: &str) -> String {
            value
                .lines()
                .map(|line| {
                    let mut normalized = String::with_capacity(line.len());
                    let mut cursor = 0;
                    while let Some(relative_start) = line[cursor..].find("guid: ") {
                        let start = cursor + relative_start;
                        normalized.push_str(&line[cursor..start]);
                        let value_start = start + "guid: ".len();
                        let value_end = line[value_start..]
                            .find(|character: char| {
                                character == ',' || character == '}' || character.is_whitespace()
                            })
                            .map_or(line.len(), |offset| value_start + offset);
                        normalized.push_str("guid: ");
                        let guid = &line[value_start..value_end];
                        if guid.len() == 32 && guid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                            normalized.push_str("<GUID>");
                        } else {
                            normalized.push_str(guid);
                        }
                        cursor = value_end;
                    }
                    normalized.push_str(&line[cursor..]);
                    if normalized.starts_with("timeCreated: ") {
                        "timeCreated: <TIME>".to_owned()
                    } else {
                        normalized
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        }

        fn meta_guid(meta: &str) -> &str {
            let guid = meta
                .lines()
                .find_map(|line| line.strip_prefix("guid: "))
                .expect("Unity metadata has a GUID");
            assert_eq!(
                guid.len(),
                32,
                "Unity metadata GUID has 32 hexadecimal digits"
            );
            assert!(guid.bytes().all(|byte| byte.is_ascii_hexdigit()));
            guid
        }

        let input_row = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT|LightmapFixture.assets|"))
            .expect("source fixture pins the generated SerializedFile identity");
        let input_columns = input_row.split('|').collect::<Vec<_>>();
        assert_eq!(input_columns.len(), 2);
        let input_bytes = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|LightmapFixture.assets|"))
            .map(decode_hex)
            .expect("source fixture contains the complete SerializedFile bytes");
        assert_eq!(
            input_columns[0].parse::<usize>().unwrap(),
            input_bytes.len()
        );
        assert_eq!(
            input_columns[1],
            "9F894E799A77E48EF8B85C47304091D1FD9792C9FF840DD2DC1CA34F9EBA9766"
        );

        let source_scene = String::from_utf8(source_output(
            fixture,
            "ExportedProject/Assets/Scenes/lightmapfixture.assets.unity",
        ))
        .expect("source Scene YAML is UTF-8");
        for expected in [
            "--- !u!104 &102\nRenderSettings:",
            "--- !u!157 &101\nLightmapSettings:",
        ] {
            assert!(
                source_scene.contains(expected),
                "source Scene lacks {expected}"
            );
        }
        assert!(source_scene
            .contains("m_LightingDataAsset: {fileID: 112000000, guid: <GUID>, type: 2}"));
        let source_lighting_data_asset = source_output(
            fixture,
            "ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset",
        );
        let source_lighting_data_asset =
            String::from_utf8(source_lighting_data_asset).expect("source LightingData is UTF-8");
        assert!(source_lighting_data_asset.contains("m_Name: LightingData"));
        assert!(source_lighting_data_asset.contains("m_Lightmaps:"));
        let source_texture = source_output(
            fixture,
            "ExportedProject/Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp",
        );
        let source_texture_meta = source_output(
            fixture,
            "ExportedProject/Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp.meta",
        );
        let source_lighting_data_meta = source_output(
            fixture,
            "ExportedProject/Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta",
        );
        let source_scene_meta = source_output(
            fixture,
            "ExportedProject/Assets/Scenes/lightmapfixture.assets.unity.meta",
        );

        let mut filesystem = VirtualFileSystem::new();
        insert_vfs_file(
            &mut filesystem,
            "/game/LightmapFixture.assets",
            &input_bytes,
        );
        let workspace = load_workspace_from_vfs(
            &filesystem,
            &[DependencyFilePath {
                file_name: "LightmapFixture.assets".to_owned(),
                virtual_path: "/game/LightmapFixture.assets".to_owned(),
            }],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let source_objects = &workspace.files()[0].metadata.objects;
        assert_eq!(source_objects.len(), 3);
        assert!(source_objects
            .iter()
            .all(|object| object.class_id.is_none()));
        assert_eq!(
            source_objects
                .iter()
                .map(|object| object.raw_type_id)
                .collect::<Vec<_>>(),
            [157, 104, 28]
        );
        let object_workspace =
            load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())
                .unwrap();
        let lightmap_version =
            assetripper::processing::asset_factory::FactoryUnityVersion::parse("2019.4.40f1")
                .unwrap();
        let (lighting_release_tree, lighting_editor_tree) =
            assetripper::processing::asset_factory::ClassLayoutSource::tpk_trees(
                assetripper::processing::asset_factory::reference_class_layouts(),
                1120,
                &lightmap_version,
            )
            .unwrap()
            .unwrap();
        for tree in [&lighting_release_tree, &lighting_editor_tree] {
            assert_eq!(
                tree.nodes
                    .first()
                    .and_then(|node| node.type_name.as_deref()),
                Some("LightingDataAsset")
            );
            for expected in [
                "m_Name",
                "m_Scene",
                "m_Lightmaps",
                "m_LightProbes",
                "m_EnlightenSceneMapping",
                "m_EnlightenData",
            ] {
                assert!(
                    tree.nodes
                        .iter()
                        .any(|node| node.name.as_deref() == Some(expected)),
                    "LightingDataAsset TypeTree lacks {expected}"
                );
            }
        }
        assert!(!lighting_release_tree.nodes.iter().any(|node| {
            matches!(
                node.name.as_deref(),
                Some(
                    "m_ObjectHideFlags"
                        | "m_CorrespondingSourceObject"
                        | "m_PrefabInstance"
                        | "m_PrefabAsset"
                )
            )
        }));
        assert_eq!(object_workspace.report().decoded_objects, 3);
        assert!(source_objects
            .iter()
            .enumerate()
            .all(|(object_index, source)| {
                object_workspace.get(0, object_index).is_some_and(|object| {
                    object.resolved_type_index.is_some() && object.raw_type_id == source.raw_type_id
                })
            }));
        let lightmap_root = &object_workspace
            .get(0, 0)
            .unwrap()
            .payload
            .as_ref()
            .unwrap()
            .root;
        let lightmap_fields = lightmap_root
            .struct_fields()
            .unwrap()
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>();
        assert!(lightmap_fields.contains(&"m_Lightmaps"));
        assert!(
            !lightmap_fields.contains(&"m_LightingDataAsset"),
            "the release payload has no serialized LDA PPtr; source availability is versioned"
        );
        let source_references = workspace
            .reference_graph(
                assetripper::serialized_file::reference_graph::MultiFileReferenceGraphLimits::default(),
            )
            .unwrap();
        let lightmap_texture_references = source_references
            .references()
            .iter()
            .filter(|reference| reference.field_path.starts_with("m_Lightmaps["))
            .map(|reference| reference.field_path)
            .collect::<Vec<_>>();
        assert_eq!(
            lightmap_texture_references,
            [
                "m_Lightmaps[0].m_Lightmap",
                "m_Lightmaps[0].m_DirLightmap",
                "m_Lightmaps[0].m_ShadowMask",
            ]
        );
        assert!(!source_references
            .references()
            .iter()
            .any(|reference| reference.field_path == "m_LightingDataAsset"));

        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("LightmapFixture.assets");
        let settings_path = directory.join("lightmap-settings.json");
        let output_root = directory.join("project-lightmap-image");
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
        .expect("production export-project writes the configured source lightmap route");

        let texture_path =
            output_root.join("Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp");
        let texture_meta_path =
            output_root.join("Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp.meta");
        let lighting_data_path =
            output_root.join("Assets/Scenes/lightmapfixture.assets/LightingData.asset");
        let lighting_data_meta_path =
            output_root.join("Assets/Scenes/lightmapfixture.assets/LightingData.asset.meta");
        let scene_path = output_root.join("Assets/Scenes/lightmapfixture.assets.unity");
        let scene_meta_path = output_root.join("Assets/Scenes/lightmapfixture.assets.unity.meta");
        assert_eq!(fs::read(&texture_path).unwrap(), source_texture);
        assert!(
            !output_root
                .join("Assets/Texture2D/LightmapFixture.bmp")
                .exists(),
            "a lightmap texture uses the owning Scene's directory"
        );
        let rust_scene = fs::read_to_string(scene_path).unwrap();
        assert_eq!(
            normalize_generated_text(&rust_scene),
            normalize_generated_text(&source_scene),
            "complete Scene YAML matches the source after GUID normalization"
        );
        let rust_lighting_data_asset = fs::read_to_string(&lighting_data_path).unwrap();
        assert_eq!(
            normalize_generated_text(&rust_lighting_data_asset),
            normalize_generated_text(&source_lighting_data_asset),
            "complete generated LightingDataAsset YAML matches the source after GUID normalization"
        );
        let rust_texture_meta = fs::read_to_string(&texture_meta_path).unwrap();
        let rust_lighting_data_meta = fs::read_to_string(&lighting_data_meta_path).unwrap();
        let rust_scene_meta = fs::read_to_string(&scene_meta_path).unwrap();
        assert_eq!(
            normalize_generated_text(&rust_texture_meta),
            normalize_generated_text(std::str::from_utf8(&source_texture_meta).unwrap()),
            "complete TextureImporter metadata matches the source after identity normalization"
        );
        assert_eq!(
            normalize_generated_text(&rust_lighting_data_meta),
            normalize_generated_text(std::str::from_utf8(&source_lighting_data_meta).unwrap()),
            "LightingDataAsset NativeFormatImporter metadata matches the source"
        );
        assert_eq!(
            normalize_generated_text(&rust_scene_meta),
            normalize_generated_text(std::str::from_utf8(&source_scene_meta).unwrap()),
            "Scene DefaultImporter metadata matches the source"
        );

        let scene_guid = meta_guid(&rust_scene_meta).to_owned();
        let lighting_data_guid = meta_guid(&rust_lighting_data_meta).to_owned();
        let texture_guid = meta_guid(&rust_texture_meta).to_owned();
        assert_ne!(scene_guid, lighting_data_guid);
        assert_ne!(scene_guid, texture_guid);
        assert_ne!(lighting_data_guid, texture_guid);
        assert!(
            rust_scene.contains(&format!(
                "m_LightingDataAsset: {{fileID: 112000000, guid: {lighting_data_guid}, type: 2}}"
            )),
            "Scene pointer resolves to the generated LightingDataAsset metadata GUID"
        );
        assert!(
            rust_lighting_data_asset.contains(&format!(
                "m_Scene: {{fileID: 102900000, guid: {scene_guid}, type: 3}}"
            )),
            "LightingDataAsset Scene pointer resolves to the owning Scene metadata GUID"
        );
        assert!(
            rust_lighting_data_asset.contains(&format!(
                "m_Lightmap: {{fileID: 2800000, guid: {texture_guid}, type: 3}}"
            )),
            "LightingDataAsset lightmap pointer resolves to the exported texture GUID"
        );
        assert!(rust_lighting_data_meta.contains("mainObjectFileID: 112000000"));
        for expected in [
            "--- !u!104 &102\nRenderSettings:",
            "--- !u!157 &101\nLightmapSettings:",
        ] {
            assert!(rust_scene.contains(expected), "Rust Scene lacks {expected}");
        }
        assert_eq!(fs::read(&source_path).unwrap(), input_bytes);
    }
