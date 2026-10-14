    #[test]
    fn aggregate_remaining_budget_rejects_a_consumed_overrun() {
        assert_eq!(remaining_budget_usize(10, 4, "input count").unwrap(), 6);
        assert!(remaining_budget_usize(10, 11, "input count").is_err());
        assert_eq!(remaining_budget_u64(20, 7, "input bytes").unwrap(), 13);
        assert!(remaining_budget_u64(20, 21, "input bytes").is_err());
    }

    #[test]
    fn aggregate_mount_report_adds_all_counters_with_checked_arithmetic() {
        let mut total = BatchMountReport {
            directory_entries_scanned: 1,
            discovered_inputs: 2,
            discovered_source_bytes: 10,
            completed_inputs: 1,
            mounted_entries: 3,
            mounted_bytes: 12,
            ..BatchMountReport::default()
        };
        let mut next = BatchMountReport {
            directory_entries_scanned: 4,
            discovered_inputs: 4,
            discovered_source_bytes: 20,
            completed_inputs: 3,
            mounted_entries: 5,
            mounted_bytes: 30,
            ..BatchMountReport::default()
        };
        merge_mount_report(&mut total, &mut next).unwrap();
        assert_eq!(total.directory_entries_scanned, 5);
        assert_eq!(total.discovered_inputs, 6);
        assert_eq!(total.discovered_source_bytes, 30);
        assert_eq!(total.completed_inputs, 4);
        assert_eq!(total.mounted_entries, 8);
        assert_eq!(total.mounted_bytes, 42);
    }

    #[test]
    fn logical_input_dedup_keeps_first_occurrences_like_select_unique_paths() {
        let inputs = [
            PathBuf::from("game/main.assets"),
            PathBuf::from("game/main.assets"),
            PathBuf::from("game/main.assets.split0"),
            PathBuf::from("other/shared.assets"),
            PathBuf::from("other/shared.assets.split1"),
        ];
        let unique = super::deduplicate_logical_input_paths(&inputs);
        assert_eq!(
            unique,
            [
                PathBuf::from("game/main.assets"),
                PathBuf::from("other/shared.assets"),
            ]
        );
        // Distinct inputs keep their order; exact spellings still compare unequal.
        let distinct = [PathBuf::from("a/x.assets"), PathBuf::from("b/x.assets")];
        assert_eq!(super::deduplicate_logical_input_paths(&distinct), distinct);
    }

    #[test]
    fn directory_inspection_loads_a_synthetic_serialized_file() {
        let input = TestInputDirectory::new();
        super::inspect_directories(std::slice::from_ref(&input.directory)).unwrap();
    }

    #[test]
    fn empty_directory_inspection_reports_unprocessed_empty_bundle() {
        let directory = TestInputDirectory::new_root();
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: Vec::new(),
            subdirectories: Vec::new(),
        };
        let mut output = Vec::new();
        super::inspect_directories_to(std::slice::from_ref(&directory), &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();

        let reference = include_str!("io/cli_inspect_fixture.txt")
            .lines()
            .find(|line| line.starts_with("EMPTY_DIRECTORY|"))
            .expect("source runtime fixture contains the empty directory outcome");
        assert_eq!(
            reference,
            "EMPTY_DIRECTORY|collections=0|processors_requested=0"
        );
        assert!(
            summary.contains(
                "No valid assets were found in the selected directories; processing was skipped."
            ),
            "{summary}"
        );
        drop(input);
    }

    #[test]
    fn resource_only_directory_matches_the_source_no_asset_process_gate() {
        let directory = TestInputDirectory::new_root();
        let resource_path = directory.join("notes.bytes");
        fs::write(&resource_path, b"plain resource bytes").unwrap();
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![resource_path],
            subdirectories: Vec::new(),
        };

        let reference = include_str!("io/cli_inspect_fixture.txt")
            .lines()
            .find(|line| line.starts_with("LOAD|resource_only_directory|"))
            .expect("source runtime fixture contains the resource-only outcome");
        assert_eq!(
            reference,
            "LOAD|resource_only_directory|collections=0|names=|processors_requested=0"
        );
        let mut output = Vec::new();
        super::inspect_directories_to(std::slice::from_ref(&directory), &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(
            summary.contains(
                "No valid assets were found in the selected directories; processing was skipped."
            ),
            "{summary}"
        );
        drop(input);
    }

    #[test]
    fn inspect_cli_rejects_empty_and_missing_directory_inputs() {
        use std::ffi::OsString;

        let source_empty = include_str!("io/cli_inspect_fixture.txt")
            .lines()
            .find(|line| line.starts_with("LOAD|no_paths|"))
            .expect("source runtime fixture contains the no-path error");
        assert_eq!(
            source_empty,
            "LOAD|no_paths|exception=ArgumentException|message=Game files not found (Parameter 'paths')"
        );
        let error = super::run([OsString::from("inspect")].into_iter()).unwrap_err();
        assert_eq!(error.to_string(), "inspect requires at least one input");

        let parent = TestInputDirectory::new_root();
        let missing = parent.join("missing");
        assert!(!missing.exists());
        let source_missing = include_str!("io/cli_inspect_fixture.txt")
            .lines()
            .find(|line| line.starts_with("LOAD|missing_directory|"))
            .expect("source runtime fixture contains the missing-directory error");
        assert_eq!(
            source_missing,
            "LOAD|missing_directory|exception=Exception|message=Neither file nor directory at '$ROOT\\missing' exists"
        );
        let mut output = Vec::new();
        let error =
            super::inspect_directories_to(std::slice::from_ref(&missing), &mut output).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("input '{}' is not a directory", missing.display())
        );
        drop(TestInputDirectory {
            directory: parent,
            files: Vec::new(),
            subdirectories: Vec::new(),
        });
    }

    /// End-to-end discovery differential against the reference `GameStructure`
    /// dump committed in `bundle_discovery_fixture.txt` (probe
    /// `Temp/opencode/bundleprobe2`): the same two C#-constructed UnityFS
    /// bundles are written to a temp dir, inspected, and the summary must show
    /// the reference file/collection/object inventory (both entry namings
    /// discovered, both SerializedFiles parsed, both objects with PathID 8001
    /// decoded). The reference-only injected engine collections stay outside:
    /// the Rust workspace holds exactly the two file collections.
    #[test]
    fn directory_inspection_reproduces_the_reference_bundle_inventory() {
        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "odd hex length");
            let bytes: Vec<u8> = hex
                .as_bytes()
                .chunks(2)
                .map(|pair| {
                    let text = std::str::from_utf8(pair).unwrap();
                    u8::from_str_radix(text, 16).unwrap()
                })
                .collect();
            assert_eq!(bytes.len(), hex.len() / 2);
            bytes
        }

        let fixture = include_str!("io/bundle_discovery_fixture.txt");
        let directory = TestInputDirectory::new_root();
        let mut bundle_names = Vec::new();
        let mut reference_assets = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            match parts[0] {
                "BUNDLE_HEX" => {
                    assert_eq!(parts.len(), 3, "{line}");
                    fs::write(directory.join(parts[1]), decode_hex(parts[2])).unwrap();
                    bundle_names.push(parts[1].to_owned());
                }
                "REF" => {
                    if parts[1] == "ASSET" {
                        reference_assets.push(parts.join("|"));
                    }
                }
                kind => panic!("unknown fixture row {kind}: {line}"),
            }
        }
        assert_eq!(bundle_names.len(), 2, "two reference bundles pin the input");
        assert_eq!(
            reference_assets,
            vec![
                "REF|ASSET|1|8001|GameObject".to_owned(),
                "REF|ASSET|1|8001|GameObject".to_owned(),
            ]
        );
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: bundle_names
                .iter()
                .map(|name| directory.join(name))
                .collect(),
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_directories_to(std::slice::from_ref(&directory), &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();

        assert!(
            summary.contains("platform: mixed; registered files: 2; assemblies: 0"),
            "{summary}"
        );
        assert!(summary.contains("SerializedFiles: 2"), "{summary}");
        assert!(
            summary.contains("Workspace collections: 2 (2 objects, 0 external identifiers)"),
            "{summary}"
        );
        assert!(
            summary.contains("Object payload decode: 2/2 decoded"),
            "{summary}"
        );
        // Collections are named by UnityFS entry name on both sides; indices
        // follow discovery order, so only the names are pinned.
        assert!(summary.contains("\"cab-test\""), "{summary}");
        assert!(summary.contains("\"main.assets\""), "{summary}");
        assert!(summary.contains("PathID 8001"), "{summary}");
        drop(input);
    }

    #[test]
    fn explicit_file_inspection_registers_only_selected_resource_inputs_as_roots() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let second_resource_path = directory.join("second.resS");
        let selected_resource_path = directory.join("direct.resS");
        let unselected_resource_path = directory.join("unselected.resS");
        fs::write(&serialized_path, make_generation_10_file()).unwrap();
        fs::write(&second_resource_path, b"second explicit root stream").unwrap();
        fs::write(&selected_resource_path, b"explicit root stream").unwrap();
        fs::write(&unselected_resource_path, b"not an explicit input").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                second_resource_path.clone(),
                selected_resource_path.clone(),
                unselected_resource_path,
            ],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_explicit_files_to(
            &[
                serialized_path,
                second_resource_path,
                selected_resource_path,
            ],
            &mut output,
        )
        .unwrap();
        let summary = String::from_utf8(output).unwrap();

        assert!(summary.contains("Explicit GameBundle root ResourceFiles: 2"));
        // Root resources enumerate in file-stack pop order (last input first),
        // matching baseline `GameBundle.FromPaths` — proven by the round-60
        // runtime differential on `IO-RESOURCE-RESOLUTION-001`.
        let first_resource_index = summary
            .find("root resource[0]: name=\"direct.resS\"")
            .unwrap();
        let second_resource_index = summary
            .find("root resource[1]: name=\"second.resS\"")
            .unwrap();
        assert!(first_resource_index < second_resource_index);
        assert!(!summary.contains("unselected.resS"));
        assert!(summary.contains("explicit file import: 3 logical inputs"));
        drop(input);
    }

    #[test]
    fn inspection_reports_failed_bundle_entries_and_project_export_refuses_partial_mounts() {
        fn decode_hex(hex: &str) -> Vec<u8> {
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        let row = include_str!("io/io_differential_fixture.txt")
            .lines()
            .find(|line| line.starts_with("SCHEME|ufs_v6_lzham_block|"))
            .expect("reference fixture contains the failed-entry continuation case");
        let input_hex = row
            .split('|')
            .find_map(|field| field.strip_prefix("in="))
            .expect("fixture row contains input bytes");
        let directory = TestInputDirectory::new_root();
        let bundle_path = directory.join("partial.bundle");
        fs::write(&bundle_path, decode_hex(input_hex)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![bundle_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut inspection = Vec::new();
        let inspect_error =
            super::inspect_explicit_files_to(std::slice::from_ref(&bundle_path), &mut inspection)
                .expect_err("the fixture has no SerializedFile or explicit root resource input");
        let inspection = String::from_utf8(inspection).unwrap();
        assert!(
            inspection.contains("UnityFS entries recorded as failed: 1"),
            "{inspection}\n{inspect_error}"
        );
        assert!(inspection.contains("unsupported.bin"), "{inspection}");

        let output_root = input.directory.join("partial-project");
        let mut status = Vec::new();
        let export_error = super::export_project_to(
            std::slice::from_ref(&bundle_path),
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect_err("project export must refuse a failed UnityFS entry");
        assert!(
            export_error
                .to_string()
                .contains("refusing to export an incomplete container mount"),
            "{export_error}"
        );
        assert!(!output_root.exists(), "partial project output was created");
        drop(input);
    }

    #[test]
    fn explicit_file_inspection_resolves_decoded_streaming_info_from_selected_root_resource() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("texture.resS");
        fs::write(
            &serialized_path,
            make_streaming_info_file("texture.resS", 2, 3),
        )
        .unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), resource_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_explicit_files_to(&[serialized_path, resource_path], &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();

        assert!(summary.contains(
            "StreamingInfo resources: 1 candidates, 0 unset, 0 bundle ranges, 1 explicit root ranges, 0 external ranges, 0 missing, 0 invalid ranges, 0 resolution errors, 0 scan errors, 0 undecoded objects"
        ));
        assert!(summary.contains("complete=true"));
        assert!(!summary.contains("StreamingInfo diagnostic:"));
        drop(input);
    }

    #[test]
    fn export_object_yaml_writes_streamed_image_texture_and_refuses_overwrite() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("texture.resS");
        let output_path = directory.join("image-texture.yaml");
        fs::write(&serialized_path, make_streamed_image_texture_file()).unwrap();
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
        super::export_object_yaml_to(
            &[serialized_path.clone(), resource_path.clone()],
            0,
            77,
            &output_path,
            &mut status,
        )
        .unwrap();
        let yaml = fs::read_to_string(&output_path).unwrap();
        assert!(yaml.contains("--- !u!189 &1"));
        assert!(
            yaml.contains("m_ImageData_C189: 3"),
            "streamed texture writes its byte count"
        );
        assert!(
            yaml.contains("_typelessdata: 323334"),
            "streamed texture writes its inline bytes"
        );
        assert!(yaml.contains("offset: 0"));
        assert!(yaml.contains("size: 0"));
        assert!(!yaml.contains("texture.resS"));
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote one YAML object from SerializedFile 0 PathID 77"));

        let original_yaml = yaml;
        let error = super::export_object_yaml_to(
            &[serialized_path, resource_path],
            0,
            77,
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
    fn export_object_yaml_reports_missing_streamed_resource_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("image-texture.yaml");
        fs::write(&serialized_path, make_streamed_image_texture_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let result =
            super::export_object_yaml_to(&[serialized_path], 0, 77, &output_path, &mut Vec::new());
        assert!(result.is_err());
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_object_yaml_skips_image_texture_without_data_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("image-texture.yaml");
        fs::write(
            &serialized_path,
            make_streamed_image_texture_file_with_stream("", 3),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_object_yaml_to(&[serialized_path], 0, 77, &output_path, &mut status).unwrap();
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Skipped SerializedFile 0 PathID 77"));
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_texture_writes_png_and_refuses_overwrite() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("texture.png");
        fs::write(&serialized_path, make_texture2d_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_texture_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            assetripper::export::settings::ImageExportFormat::Png,
            true,
            &output_path,
            &mut status,
        )
        .unwrap();
        let png = fs::read(&output_path).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a png texture from SerializedFile 0 PathID 42"));

        let original = png;
        let error = super::export_texture_to(
            &[serialized_path],
            0,
            42,
            assetripper::export::settings::ImageExportFormat::Png,
            true,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&output_path).unwrap(), original);
        drop(input);
    }

    #[test]
    fn export_cubemap_skips_the_vertical_flip() {
        // The class-89 payload shares the class-28 fields (1x2 RGBA rows
        // [10,20,30,255] and [40,50,60,255]); as a cubemap the rows keep file
        // order, so the bottom-up BMP tail holds row 1 first as BGRA. A flipped
        // class-28 export of the same bytes would end with row 0 instead.
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("cubemap.bmp");
        fs::write(
            &serialized_path,
            make_texture_file(89, "Cubemap", 1, 2, &[10, 20, 30, 255, 40, 50, 60, 255]),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_texture_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            assetripper::export::settings::ImageExportFormat::Bmp,
            true,
            &output_path,
            &mut status,
        )
        .unwrap();
        let bmp = fs::read(&output_path).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(&bmp[122..], &[60, 50, 40, 255, 30, 20, 10, 255]);
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a bmp texture from SerializedFile 0 PathID 42"));
        drop(input);
    }

    /// Class-117/187/188 textures export through the `GraphicsFormat` route:
    /// each synthetic file carries one RGBA pixel (format discriminant 4) and
    /// must yield a one-pixel PNG.
    #[test]
    fn export_array_textures_write_pngs() {
        let face_pixels: Vec<u8> = (0..6)
            .flat_map(|face| [10 + face, 20 + face, 30 + face, 255])
            .collect();
        for (class_id, root_name, height, third, size_name, size_value, data) in [
            (
                117,
                "Texture3D",
                Some(1),
                Some(("m_Depth", 1)),
                "m_CompleteImageSize",
                4,
                vec![10, 20, 30, 255],
            ),
            (
                187,
                "Texture2DArray",
                Some(1),
                Some(("m_Depth", 1)),
                "m_DataSize",
                4,
                vec![10, 20, 30, 255],
            ),
            (
                188,
                "CubemapArray",
                None,
                Some(("m_CubemapCount", 1)),
                "m_DataSize",
                24,
                face_pixels.clone(),
            ),
        ] {
            let directory = TestInputDirectory::new_root();
            let serialized_path = directory.join("main.assets");
            let output_path = directory.join("texture.png");
            fs::write(
                &serialized_path,
                make_array_texture_file(
                    class_id, root_name, height, third, size_name, true, size_value, &data,
                ),
            )
            .unwrap();
            let input = TestInputDirectory {
                directory,
                files: vec![serialized_path.clone(), output_path.clone()],
                subdirectories: Vec::new(),
            };

            let mut status = Vec::new();
            super::export_texture_to(
                std::slice::from_ref(&serialized_path),
                0,
                42,
                assetripper::export::settings::ImageExportFormat::Png,
                true,
                &output_path,
                &mut status,
            )
            .unwrap();
            let png = fs::read(&output_path).unwrap();
            assert_eq!(
                &png[..8],
                &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
                "class {class_id}"
            );
            drop(input);
        }
    }

    #[test]
    fn export_texture_reports_a_non_texture_object_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("texture.png");
        fs::write(
            &serialized_path,
            make_build_settings_file(&["Assets/A.unity"]),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let result = super::export_texture_to(
            &[serialized_path],
            0,
            42,
            assetripper::export::settings::ImageExportFormat::Png,
            true,
            &output_path,
            &mut Vec::new(),
        );
        assert!(result.is_err());
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_texture_settings_file_selects_the_fallback_format() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let settings_path = directory.join("AssetRipper.Settings.json");
        // The committed modified probe fixture configures BMP output.
        fs::write(
            &settings_path,
            include_str!("export/settings_file_modified.json"),
        )
        .unwrap();
        let output_path = directory.join("texture.bin");
        fs::write(&serialized_path, make_texture2d_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                settings_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        // An unrecognized extension falls back to the configured BMP format.
        let arguments = vec![
            OsString::from("--settings"),
            settings_path.clone().into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        super::export_texture_from_arguments(arguments.into_iter()).unwrap();
        assert_eq!(&fs::read(&output_path).unwrap()[..2], b"BM");
        drop(input);
    }

    #[test]
    fn export_texture_settings_file_prefer_original_falls_back_to_configured() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let settings_path = directory.join("AssetRipper.Settings.json");
        // PNG configured with original-extension preference: bundle-decoded assets
        // carry no original path, so the `.bin` output falls back to PNG.
        fs::write(
            &settings_path,
            "{\"Import\": {\"ScriptContentLevel\": 2, \"StreamingAssetsMode\": 1, \"DefaultVersion\": \"0.0.0a0\", \"TargetVersion\": \"0.0.0a0\"}, \"Processing\": {\"EnablePrefabOutlining\": false, \"EnableStaticMeshSeparation\": true, \"EnableAssetDeduplication\": false, \"RemoveNullableAttributes\": false, \"PublicizeAssemblies\": false, \"BundledAssetsExportMode\": 2}, \"Export\": {\"AudioExportFormat\": 2, \"ImageExportFormat\": 4, \"LightmapTextureExportFormat\": 2, \"ScriptExportMode\": 1, \"ScriptLanguageVersion\": -1, \"ScriptTypesFullyQualified\": false, \"ShaderExportMode\": 0, \"SpriteExportMode\": 0, \"TextExportMode\": 2, \"ExportUnreadableAssets\": false, \"PreferOriginalTextureExtension\": true, \"SaveSettingsToDisk\": false, \"LanguageCode\": null}}",
        )
        .unwrap();
        let output_path = directory.join("texture.bin");
        fs::write(&serialized_path, make_texture2d_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                settings_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        let arguments = vec![
            OsString::from("--settings"),
            settings_path.into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.into_os_string(),
        ];
        super::export_texture_from_arguments(arguments.into_iter()).unwrap();
        assert_eq!(
            &fs::read(&output_path).unwrap()[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
        drop(input);
    }

    #[test]
    fn export_texture_settings_file_problems_are_input_errors() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("texture.bin");
        let bad_path = directory.join("bad.json");
        fs::write(&serialized_path, make_texture2d_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                output_path.clone(),
                bad_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        // A missing settings file names its path.
        let arguments = vec![
            OsString::from("--settings"),
            OsString::from("no-such-file.json"),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        let error = super::export_texture_from_arguments(arguments.into_iter()).unwrap_err();
        assert!(error.to_string().contains("no-such-file.json"), "{error}");
        assert!(!output_path.exists());

        // A malformed settings file is rejected before any export work.
        let settings_path = bad_path;
        fs::write(&settings_path, b"{\"Import\": {}}").unwrap();
        let arguments = vec![
            OsString::from("--settings"),
            settings_path.into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        let error = super::export_texture_from_arguments(arguments.into_iter()).unwrap_err();
        assert!(error.to_string().contains("bad.json"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_shader_settings_file_selects_the_fallback_mode() {
        use std::ffi::OsString;

        // Compiled text (`Program` present): `Simple` does not admit it, so the mode
        // decides between the dummy bytes and the YAML-asset redirect.
        let script = "Shader \"Test/Compiled\" {\n\tSubShader {\n\t\tPass {\n\t\t\tProgram \"vp\" {\n\t\t\t\tSubProgram \"d3d11\" {\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}";
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let settings_path = directory.join("AssetRipper.Settings.json");
        // YAML mode with default versions and language settings.
        fs::write(
            &settings_path,
            "{\"Import\": {\"ScriptContentLevel\": 2, \"StreamingAssetsMode\": 1, \"DefaultVersion\": \"0.0.0a0\", \"TargetVersion\": \"0.0.0a0\"}, \"Processing\": {\"EnablePrefabOutlining\": false, \"EnableStaticMeshSeparation\": true, \"EnableAssetDeduplication\": false, \"RemoveNullableAttributes\": false, \"PublicizeAssemblies\": false, \"BundledAssetsExportMode\": 2}, \"Export\": {\"AudioExportFormat\": 2, \"ImageExportFormat\": 4, \"LightmapTextureExportFormat\": 2, \"ScriptExportMode\": 1, \"ScriptLanguageVersion\": -1, \"ScriptTypesFullyQualified\": false, \"ShaderExportMode\": 1, \"SpriteExportMode\": 0, \"TextExportMode\": 2, \"ExportUnreadableAssets\": false, \"PreferOriginalTextureExtension\": true, \"SaveSettingsToDisk\": false, \"LanguageCode\": null}}",
        )
        .unwrap();
        let output_path = directory.join("shader.shader");
        fs::write(&serialized_path, make_shader_file(script)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                settings_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        // The settings file selects YAML mode, whose text path reports the asset
        // redirect instead of writing dummy bytes.
        let arguments = vec![
            OsString::from("--settings"),
            settings_path.clone().into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        let error = super::export_shader_from_arguments(arguments.into_iter()).unwrap_err();
        assert!(error.to_string().contains("writes a YAML asset"), "{error}");
        assert!(!output_path.exists());

        // An explicit flag beats the settings file on any order.
        let arguments = vec![
            OsString::from("--shader-export-mode"),
            OsString::from("dummy"),
            OsString::from("--settings"),
            settings_path.into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        super::export_shader_from_arguments(arguments.into_iter()).unwrap();
        assert!(output_path.exists());
        drop(input);
    }

    #[test]
    fn export_mesh_writes_a_glb_and_refuses_overwrite() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("mesh.glb");
        fs::write(&serialized_path, make_mesh_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_mesh_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            &output_path,
            &mut status,
        )
        .unwrap();
        let glb = fs::read(&output_path).unwrap();
        assert_eq!(&glb[..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize,
            glb.len(),
            "the declared total length matches the file"
        );
        let status = String::from_utf8(status).unwrap();
        assert!(status.contains("Wrote a GLB with 3 vertices"), "{status}");
        assert!(status.contains("1 sub-meshes"), "{status}");
        assert!(status.contains("3 emitted indices"), "{status}");
        // The JSON chunk carries the mesh name and one SubMesh_0 node.
        let json_length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&glb[20..20 + json_length]).unwrap();
        assert!(json.contains("\"Tri\""), "{json}");
        assert!(json.contains("SubMesh_0"), "{json}");

        let original = glb;
        let error = super::export_mesh_to(&[serialized_path], 0, 42, &output_path, &mut Vec::new())
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&output_path).unwrap(), original);
        drop(input);
    }

    #[test]
    fn export_mesh_reports_a_non_mesh_object_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("mesh.glb");
        fs::write(&serialized_path, make_texture2d_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_mesh_to(&[serialized_path], 0, 42, &output_path, &mut Vec::new())
            .unwrap_err();
        assert!(error.to_string().contains("not a class-43 Mesh"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_mesh_requires_an_existing_path_id() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("mesh.glb");
        fs::write(&serialized_path, make_mesh_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_mesh_to(&[serialized_path], 0, 7, &output_path, &mut Vec::new())
            .unwrap_err();
        assert!(error.to_string().contains("PathID 7"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_shader_writes_simple_bytes_and_refuses_overwrite() {
        use assetripper::export::settings::ShaderExportMode;

        let script = "Shader \"Test/Simple\" {\n\tSubShader {\n\t\tPass {}\n\t}\n}";
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("shader.shader");
        fs::write(&serialized_path, make_shader_file(script)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_shader_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            ShaderExportMode::Dummy,
            &output_path,
            &mut status,
        )
        .unwrap();
        // Decompiled text wins under every mode with the raw script bytes.
        assert_eq!(fs::read(&output_path).unwrap(), script.as_bytes());
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a shader from SerializedFile 0 PathID 42"));

        let original = fs::read(&output_path).unwrap();
        let error = super::export_shader_to(
            &[serialized_path],
            0,
            42,
            ShaderExportMode::Dummy,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&output_path).unwrap(), original);
        drop(input);
    }

    #[test]
    fn export_shader_renders_dummy_for_compiled_scripts() {
        use assetripper::export::settings::ShaderExportMode;

        let script = "Shader \"Test/Prog\" {\n\tSubShader {\n\t\tProgram \"vp\" {\n\t\t}\n\t}\n}";
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("shader.shader");
        fs::write(&serialized_path, make_shader_file(script)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        super::export_shader_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            ShaderExportMode::Decompile,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap();
        let text = fs::read_to_string(&output_path).unwrap();
        // Header slice plus marker plus fallback body plus brace, like the probed rows.
        assert!(text.starts_with("Shader \"Test/Prog\" {\n\t\t//DummyShaderTextExporter\n"));
        assert!(text.contains("\n\tSubShader{\n"));
        assert!(text.ends_with("\t}\n}"));
        drop(input);
    }

    #[test]
    fn export_shader_yaml_mode_reports_asset_not_text() {
        use assetripper::export::settings::ShaderExportMode;

        let script = "Shader \"Test/Prog\" {\n\tSubShader {\n\t\tProgram \"vp\" {\n\t\t}\n\t}\n}";
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("shader.shader");
        fs::write(&serialized_path, make_shader_file(script)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_shader_to(
            &[serialized_path],
            0,
            42,
            ShaderExportMode::Yaml,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("writes a YAML asset"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_shader_reports_a_non_shader_object_without_creating_output() {
        use assetripper::export::settings::ShaderExportMode;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("shader.shader");
        fs::write(
            &serialized_path,
            make_build_settings_file(&["Assets/A.unity"]),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_shader_to(
            &[serialized_path],
            0,
            42,
            ShaderExportMode::Dummy,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("not a class-48 Shader"),
            "{error}"
        );
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_object_yaml_emits_material_fields() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("material.mat");
        fs::write(&serialized_path, make_material_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_object_yaml_to(&[serialized_path], 0, 42, &output_path, &mut status).unwrap();
        let yaml = fs::read_to_string(&output_path).unwrap();
        assert!(yaml.contains("--- !u!21 &1"), "{yaml}");
        assert!(yaml.contains("m_Name"), "{yaml}");
        assert!(yaml.contains("TestMat"), "{yaml}");
        assert!(yaml.contains("m_CustomRenderQueue"), "{yaml}");
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote one YAML object from SerializedFile 0 PathID 42"));
        drop(input);
    }

    #[test]
    fn export_audio_writes_typed_bytes_and_refuses_overwrite() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.wav");
        let data = b"RIFF....";
        fs::write(&serialized_path, make_audio_clip_file(data, Some(20))).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_audio_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut status,
        )
        .unwrap();
        assert_eq!(fs::read(&output_path).unwrap(), data);
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a wav audio from SerializedFile 0 PathID 42"));

        let original = fs::read(&output_path).unwrap();
        let error = super::export_audio_to(
            &[serialized_path],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&output_path).unwrap(), original);
        drop(input);
    }

    #[test]
    fn export_audio_passes_tracker_bytes_through_untouched() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("music.it");
        let data = b"IMPM....";
        fs::write(&serialized_path, make_audio_clip_file(data, None)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_audio_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut status,
        )
        .unwrap();
        assert_eq!(fs::read(&output_path).unwrap(), data);
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a it audio from SerializedFile 0 PathID 42"));
        drop(input);
    }

    #[test]
    fn export_audio_reports_undecodable_bytes_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.bin");
        fs::write(&serialized_path, make_audio_clip_file(b"ABCD", None)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_audio_to(
            &[serialized_path],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("Failed to convert audio"),
            "{error}"
        );
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_audio_reports_fsb5_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.fsb");
        fs::write(&serialized_path, make_audio_clip_file(b"FSB5....", None)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_audio_to(
            &[serialized_path],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("unsupported FSB5 version"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_audio_reports_empty_data_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.wav");
        fs::write(&serialized_path, make_audio_clip_file(&[], Some(20))).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_audio_to(
            &[serialized_path],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("audio data could not be found"),
            "{error}"
        );
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_audio_resolves_streamed_resource_bytes() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("audio.resS");
        let output_path = directory.join("music.it");
        let content = b"IMPM\0\0\0\0";
        fs::write(
            &serialized_path,
            make_streamed_audio_clip_file("audio.resS", 0, 8),
        )
        .unwrap();
        fs::write(&resource_path, content).unwrap();
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
        super::export_audio_to(
            &[serialized_path.clone(), resource_path.clone()],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut status,
        )
        .unwrap();
        assert_eq!(fs::read(&output_path).unwrap(), content);
        // streaming round-trip above; FSB5 remux coverage follows at EOF.
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a it audio from SerializedFile 0 PathID 42"));
        drop(input);
    }

    #[test]
    fn export_audio_remuxes_silent_fsb5_to_ogg_and_wav() {
        // The silent stereo FSB5 container from the decode suite (version 1,
        // one stream, Vorbis mode, table setup 0xA72297FF, two zero packets)
        // embedded as inline clip bytes: Default export remuxes the rebuilt
        // Ogg while PreferWav converts it to a 128-frame silent WAV.
        fn silent_stereo_fsb5_container() -> Vec<u8> {
            let mut fsb5 = vec![0u8; 60];
            fsb5[..4].copy_from_slice(b"FSB5");
            fsb5[4..8].copy_from_slice(&1u32.to_le_bytes());
            fsb5[8..12].copy_from_slice(&1u32.to_le_bytes());
            fsb5[12..16].copy_from_slice(&16u32.to_le_bytes());
            fsb5[20..24].copy_from_slice(&6u32.to_le_bytes());
            fsb5[24..28].copy_from_slice(&15u32.to_le_bytes());
            let sample_header = (u64::from(128u32) << 34) | (1u64 << 5) | (8u64 << 1) | 1;
            fsb5.extend_from_slice(&sample_header.to_le_bytes());
            fsb5.extend_from_slice(&[0x08, 0, 0, 0x16]);
            fsb5.extend_from_slice(&0xA722_97FFu32.to_le_bytes());
            fsb5.extend_from_slice(&[1u8, 0, 0, 1, 0, 0]);
            fsb5
        }

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let ogg_path = directory.join("clip.ogg");
        let wav_path = directory.join("clip.wav");
        fs::write(
            &serialized_path,
            make_audio_clip_file(&silent_stereo_fsb5_container(), None),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                ogg_path.clone(),
                wav_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_audio_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            super::SettingsAudioFormat::Default,
            &ogg_path,
            &mut status,
        )
        .unwrap();
        let ogg = fs::read(&ogg_path).unwrap();
        assert_eq!(&ogg[..4], b"OggS");
        let mut reader = ogg::PacketReader::new(std::io::Cursor::new(ogg.as_slice()));
        let mut packets = Vec::new();
        while let Some(packet) = reader.read_packet().unwrap() {
            packets.push(packet.data);
        }
        assert_eq!(packets.len(), 5);
        assert_eq!(packets[3], [0]);
        assert_eq!(packets[4], [0]);
        assert!(String::from_utf8(status).unwrap().contains(
            "Wrote a ogg audio from SerializedFile 0 PathID 42"
        ));

        let mut status = Vec::new();
        super::export_audio_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            super::SettingsAudioFormat::PreferWav,
            &wav_path,
            &mut status,
        )
        .unwrap();
        let wav = fs::read(&wav_path).unwrap();
        assert_eq!(u32::from_le_bytes(wav[46..50].try_into().unwrap()), 128);
        assert_eq!(wav.len(), 58 + 128 * 4 * 2);
        for sample in wav[58..].chunks_exact(4) {
            assert_eq!(f32::from_le_bytes(sample.try_into().unwrap()), 0.0);
        }
        assert!(String::from_utf8(status).unwrap().contains(
            "Wrote a wav audio from SerializedFile 0 PathID 42"
        ));
        drop(input);
    }
