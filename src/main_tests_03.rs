    #[test]
    fn export_audio_reports_a_non_audio_object_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.wav");
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
            error.to_string().contains("not a class-83 AudioClip"),
            "{error}"
        );
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_audio_settings_file_prefer_wav_converts_vorbis_ogg_to_float_wav() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let settings_path = directory.join("AssetRipper.Settings.json");
        // The committed modified probe fixture configures `PreferWav` audio output.
        fs::write(
            &settings_path,
            include_str!("export/settings_file_modified.json"),
        )
        .unwrap();
        // Sound type 14 decodes as `ogg`, which `PreferWav` converts to `wav`.
        let ogg = include_bytes!("export/ogg_to_wav_test_fixture.ogg");
        fs::write(
            &serialized_path,
            make_audio_clip_file(ogg, Some(14)),
        )
        .unwrap();
        let output_path = directory.join("clip.wav");
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
        super::export_audio_from_arguments(arguments.into_iter()).unwrap();
        let converted = fs::read(&output_path).unwrap();
        let reference = include_bytes!("export/ogg_to_wav_test_fixture_reference.wav");
        assert_eq!(converted.len(), reference.len());
        assert_eq!(&converted[..58], &reference[..58]);
        let largest_sample_error = converted[58..]
            .chunks_exact(4)
            .zip(reference[58..].chunks_exact(4))
            .map(|(actual, expected)| {
                let actual = f32::from_le_bytes(actual.try_into().unwrap());
                let expected = f32::from_le_bytes(expected.try_into().unwrap());
                (actual - expected).abs()
            })
            .fold(0.0f32, f32::max);
        assert!(
            largest_sample_error <= 1.0e-6,
            "largest reference sample error was {largest_sample_error}"
        );
        drop(input);
    }

    #[test]
    fn export_audio_settings_file_prefer_wav_keeps_non_ogg_bytes() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let settings_path = directory.join("AssetRipper.Settings.json");
        fs::write(
            &settings_path,
            include_str!("export/settings_file_modified.json"),
        )
        .unwrap();
        // Sound type 20 decodes as `wav`, so `PreferWav` changes nothing.
        let data = b"RIFF....";
        fs::write(&serialized_path, make_audio_clip_file(data, Some(20))).unwrap();
        let output_path = directory.join("clip.wav");
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
        // `export_audio_from_arguments` reports to stdout internally; the byte-level
        // assertion below carries this test.
        super::export_audio_from_arguments(arguments.into_iter()).unwrap();
        assert_eq!(fs::read(&output_path).unwrap(), data);
        drop(input);
    }

    #[test]
    fn export_audio_settings_file_problems_are_input_errors() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.wav");
        let bad_path = directory.join("bad.json");
        fs::write(
            &serialized_path,
            make_audio_clip_file(b"RIFF....", Some(20)),
        )
        .unwrap();
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
        let error = super::export_audio_from_arguments(arguments.into_iter()).unwrap_err();
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
        let error = super::export_audio_from_arguments(arguments.into_iter()).unwrap_err();
        assert!(error.to_string().contains("bad.json"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn default_path_settings_absent_file_keeps_builtin_defaults() {
        let directory = TestInputDirectory::new_root();
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: Vec::new(),
            subdirectories: Vec::new(),
        };

        assert!(
            super::load_default_export_settings_from_dir(&directory.join("empty"))
                .unwrap()
                .is_none()
        );
        drop(input);
    }

    #[test]
    fn default_path_settings_present_file_loads_the_export_record() {
        use assetripper::export::settings::AudioExportFormat;
        use assetripper::export::settings::ImageExportFormat;
        use assetripper::export::settings::ShaderExportMode;

        let directory = TestInputDirectory::new_root();
        let subdir = directory.join("exe");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(
            subdir.join("AssetRipper.Settings.json"),
            "{\"Import\": {\"ScriptContentLevel\": 2, \"StreamingAssetsMode\": 1, \"DefaultVersion\": \"0.0.0a0\", \"TargetVersion\": \"0.0.0a0\"}, \"Processing\": {\"EnablePrefabOutlining\": false, \"EnableStaticMeshSeparation\": true, \"EnableAssetDeduplication\": false, \"RemoveNullableAttributes\": false, \"PublicizeAssemblies\": false, \"BundledAssetsExportMode\": 2}, \"Export\": {\"AudioExportFormat\": 3, \"ImageExportFormat\": 0, \"LightmapTextureExportFormat\": 2, \"ScriptExportMode\": 1, \"ScriptLanguageVersion\": -1, \"ScriptTypesFullyQualified\": false, \"ShaderExportMode\": 2, \"SpriteExportMode\": 0, \"TextExportMode\": 2, \"ExportUnreadableAssets\": false, \"PreferOriginalTextureExtension\": true, \"SaveSettingsToDisk\": false, \"LanguageCode\": null}}",
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![subdir.join("AssetRipper.Settings.json")],
            subdirectories: vec![subdir.clone()],
        };

        let export = super::load_default_export_settings_from_dir(&subdir)
            .unwrap()
            .expect("the stored file must load");
        assert_eq!(export.audio_export_format, AudioExportFormat::PreferWav);
        assert_eq!(export.image_export_format, ImageExportFormat::Bmp);
        assert_eq!(export.shader_export_mode, ShaderExportMode::Decompile);
        drop(input);
    }

    #[test]
    fn default_path_settings_broken_file_is_a_path_carrying_error() {
        let directory = TestInputDirectory::new_root();
        let subdir = directory.join("exe");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(
            subdir.join("AssetRipper.Settings.json"),
            b"{\"Import\": {}}",
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![subdir.join("AssetRipper.Settings.json")],
            subdirectories: vec![subdir.clone()],
        };

        let error = super::load_default_export_settings_from_dir(&subdir).unwrap_err();
        assert!(
            error.to_string().contains("AssetRipper.Settings.json"),
            "{error}"
        );
        drop(input);
    }

    #[test]
    fn default_path_save_then_load_round_trips_builtin_defaults() {
        use assetripper::export::settings::{
            ExportDefaults, ImageExportFormat, ImportDefaults, ProcessingDefaults,
        };
        use assetripper::export::settings_file::SerializedSettingsFile;

        let directory = TestInputDirectory::new_root();
        let subdir = directory.join("exe");
        std::fs::create_dir(&subdir).unwrap();
        let input = TestInputDirectory {
            directory,
            files: Vec::new(),
            subdirectories: vec![subdir.clone()],
        };

        let defaults = SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        );
        let path = super::save_serialized_settings_to_dir(&defaults, &subdir).unwrap();
        assert!(path.exists());
        let loaded = super::load_default_export_settings_from_dir(&subdir)
            .unwrap()
            .expect("the stored file must load");
        assert_eq!(loaded.image_export_format, ImageExportFormat::Png);
        // Overwriting is unconditional, mirroring `File.Create`.
        let path_again = super::save_serialized_settings_to_dir(&defaults, &subdir).unwrap();
        assert_eq!(path, path_again);
        drop(input);
    }

    #[test]
    fn default_path_save_explicit_source_stores_the_validated_record() {
        use assetripper::export::settings::ImageExportFormat;

        let directory = TestInputDirectory::new_root();
        let subdir = directory.join("exe");
        std::fs::create_dir(&subdir).unwrap();
        let source = subdir.join("source.json");
        std::fs::write(
            &source,
            "{\"Import\": {\"ScriptContentLevel\": 2, \"StreamingAssetsMode\": 1, \"DefaultVersion\": \"0.0.0a0\", \"TargetVersion\": \"0.0.0a0\"}, \"Processing\": {\"EnablePrefabOutlining\": false, \"EnableStaticMeshSeparation\": true, \"EnableAssetDeduplication\": false, \"RemoveNullableAttributes\": false, \"PublicizeAssemblies\": false, \"BundledAssetsExportMode\": 2}, \"Export\": {\"AudioExportFormat\": 3, \"ImageExportFormat\": 0, \"LightmapTextureExportFormat\": 2, \"ScriptExportMode\": 1, \"ScriptLanguageVersion\": -1, \"ScriptTypesFullyQualified\": false, \"ShaderExportMode\": 2, \"SpriteExportMode\": 0, \"TextExportMode\": 2, \"ExportUnreadableAssets\": false, \"PreferOriginalTextureExtension\": true, \"SaveSettingsToDisk\": false, \"LanguageCode\": null}}",
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![source.clone()],
            subdirectories: vec![subdir.clone()],
        };

        let settings = super::read_serialized_settings_file(&source).unwrap();
        super::save_serialized_settings_to_dir(&settings, &subdir).unwrap();
        let loaded = super::load_default_export_settings_from_dir(&subdir)
            .unwrap()
            .expect("the stored file must load");
        assert_eq!(loaded.image_export_format, ImageExportFormat::Bmp);

        let broken = subdir.join("broken.json");
        std::fs::write(&broken, b"{\"Import\": {}}").unwrap();
        let error = super::read_serialized_settings_file(&broken).unwrap_err();
        assert!(error.to_string().contains("broken.json"), "{error}");
        drop(input);
    }

    #[test]
    fn default_path_delete_removes_present_and_ignores_absent() {
        use assetripper::export::settings::{ExportDefaults, ImportDefaults, ProcessingDefaults};
        use assetripper::export::settings_file::SerializedSettingsFile;

        let directory = TestInputDirectory::new_root();
        let subdir = directory.join("exe");
        std::fs::create_dir(&subdir).unwrap();
        let input = TestInputDirectory {
            directory,
            files: Vec::new(),
            subdirectories: vec![subdir.clone()],
        };

        // Absent is a success without effect, mirroring the source check.
        assert!(!super::delete_serialized_settings_in_dir(&subdir).unwrap());
        let defaults = SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        );
        super::save_serialized_settings_to_dir(&defaults, &subdir).unwrap();
        assert!(super::delete_serialized_settings_in_dir(&subdir).unwrap());
        assert!(super::load_default_export_settings_from_dir(&subdir)
            .unwrap()
            .is_none());
        assert!(!super::delete_serialized_settings_in_dir(&subdir).unwrap());
        drop(input);
    }

    #[test]
    fn cli_classifier_pins_the_settings_persistence_spellings() {
        use super::{classify_cli_command, CliCommand};

        assert_eq!(
            classify_cli_command("save-default-settings"),
            CliCommand::SaveDefaultSettings
        );
        assert_eq!(
            classify_cli_command("delete-default-settings"),
            CliCommand::DeleteDefaultSettings
        );
        assert_eq!(
            classify_cli_command("Save-Default-Settings"),
            CliCommand::Unknown
        );
    }

    /// A class-43 `Mesh` whose vertex data is a single Float32 position channel and whose index
    /// buffer holds one 16-bit triangle.
    fn make_mesh_file() -> Vec<u8> {
        let mut strings = Vec::new();
        let mesh_type = push_type_tree_string(&mut strings, "Mesh");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let vector_type = push_type_tree_string(&mut strings, "vector");
        let sub_meshes_field = push_type_tree_string(&mut strings, "m_SubMeshes");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let sub_mesh_type = push_type_tree_string(&mut strings, "SubMesh");
        let uint32_type = push_type_tree_string(&mut strings, "UInt32");
        let first_byte_field = push_type_tree_string(&mut strings, "firstByte");
        let index_count_field = push_type_tree_string(&mut strings, "indexCount");
        let topology_field = push_type_tree_string(&mut strings, "topology");
        let base_vertex_field = push_type_tree_string(&mut strings, "baseVertex");
        let first_vertex_field = push_type_tree_string(&mut strings, "firstVertex");
        let vertex_count_field = push_type_tree_string(&mut strings, "vertexCount");
        let local_aabb_field = push_type_tree_string(&mut strings, "localAABB");
        let aabb_type = push_type_tree_string(&mut strings, "AABB");
        let center_field = push_type_tree_string(&mut strings, "m_Center");
        let extent_field = push_type_tree_string(&mut strings, "m_Extent");
        let vector3_type = push_type_tree_string(&mut strings, "Vector3");
        let x_field = push_type_tree_string(&mut strings, "x");
        let y_field = push_type_tree_string(&mut strings, "y");
        let z_field = push_type_tree_string(&mut strings, "z");
        let float_type = push_type_tree_string(&mut strings, "float");
        let typeless_data_type = push_type_tree_string(&mut strings, "TypelessData");
        let uint8_type = push_type_tree_string(&mut strings, "UInt8");
        let char_type = push_type_tree_string(&mut strings, "char");
        let index_buffer_field = push_type_tree_string(&mut strings, "m_IndexBuffer");
        let index_format_field = push_type_tree_string(&mut strings, "m_IndexFormat");
        let vertex_data_field = push_type_tree_string(&mut strings, "m_VertexData");
        let vertex_data_type = push_type_tree_string(&mut strings, "VertexData");
        let vertex_count_name = push_type_tree_string(&mut strings, "m_VertexCount");
        let channels_field = push_type_tree_string(&mut strings, "m_Channels");
        let channel_info_type = push_type_tree_string(&mut strings, "ChannelInfo");
        let dimension_field = push_type_tree_string(&mut strings, "dimension");
        let offset_field = push_type_tree_string(&mut strings, "offset");
        let format_field = push_type_tree_string(&mut strings, "format");
        let stream_field = push_type_tree_string(&mut strings, "stream");
        let streams_field = push_type_tree_string(&mut strings, "m_Streams");
        let stream_info_type = push_type_tree_string(&mut strings, "StreamInfo");
        let channel_mask_field = push_type_tree_string(&mut strings, "channelMask");
        let stream_offset_field = push_type_tree_string(&mut strings, "offset");
        let stride_field = push_type_tree_string(&mut strings, "stride");
        let divider_op_field = push_type_tree_string(&mut strings, "dividerOp");
        let uint16_type = push_type_tree_string(&mut strings, "UInt16");
        let frequency_field = push_type_tree_string(&mut strings, "frequency");
        let data_size_field = push_type_tree_string(&mut strings, "m_DataSize");

        let mut nodes: Vec<(u8, u32, u32, i32)> = Vec::new();
        fn push_node(
            nodes: &mut Vec<(u8, u32, u32, i32)>,
            level: u8,
            type_offset: u32,
            name_offset: u32,
            byte_size: i32,
        ) {
            nodes.push((level, type_offset, name_offset, byte_size));
        }

        // Level 0: the Mesh root.
        push_node(&mut nodes, 0, mesh_type, empty_name, -1);
        // Level 1: m_Name (string).
        push_node(&mut nodes, 1, string_type, name_field, -1);
        push_node(&mut nodes, 2, array_type, array_type, -1);
        push_node(&mut nodes, 3, int_type, size_field, 4);
        push_node(&mut nodes, 3, char_type, data_field, 1);
        // Level 1: m_SubMeshes (vector of SubMesh).
        push_node(&mut nodes, 1, vector_type, sub_meshes_field, -1);
        push_node(&mut nodes, 2, array_type, array_type, -1);
        push_node(&mut nodes, 3, int_type, size_field, 4);
        push_node(&mut nodes, 3, sub_mesh_type, data_field, -1);
        push_node(&mut nodes, 4, uint32_type, first_byte_field, 4);
        push_node(&mut nodes, 4, uint32_type, index_count_field, 4);
        push_node(&mut nodes, 4, int_type, topology_field, 4);
        push_node(&mut nodes, 4, uint32_type, base_vertex_field, 4);
        push_node(&mut nodes, 4, uint32_type, first_vertex_field, 4);
        push_node(&mut nodes, 4, uint32_type, vertex_count_field, 4);
        push_node(&mut nodes, 4, aabb_type, local_aabb_field, -1);
        for field in [center_field, extent_field] {
            push_node(&mut nodes, 5, vector3_type, field, -1);
            for component in [x_field, y_field, z_field] {
                push_node(&mut nodes, 6, float_type, component, 4);
            }
        }
        // Level 1: m_IndexBuffer (TypelessData).
        push_node(&mut nodes, 1, typeless_data_type, index_buffer_field, -1);
        push_node(&mut nodes, 2, int_type, size_field, 4);
        push_node(&mut nodes, 2, uint8_type, data_field, 1);
        // Level 1: m_IndexFormat (int).
        push_node(&mut nodes, 1, int_type, index_format_field, 4);
        // Level 1: m_VertexData (VertexData).
        push_node(&mut nodes, 1, vertex_data_type, vertex_data_field, -1);
        push_node(&mut nodes, 2, uint32_type, vertex_count_name, 4);
        push_node(&mut nodes, 2, vector_type, channels_field, -1);
        push_node(&mut nodes, 3, array_type, array_type, -1);
        push_node(&mut nodes, 4, int_type, size_field, 4);
        push_node(&mut nodes, 4, channel_info_type, data_field, -1);
        for field in [dimension_field, offset_field, format_field, stream_field] {
            push_node(&mut nodes, 5, uint8_type, field, 1);
        }
        push_node(&mut nodes, 2, vector_type, streams_field, -1);
        push_node(&mut nodes, 3, array_type, array_type, -1);
        push_node(&mut nodes, 4, int_type, size_field, 4);
        push_node(&mut nodes, 4, stream_info_type, data_field, -1);
        push_node(&mut nodes, 5, uint32_type, channel_mask_field, 4);
        push_node(&mut nodes, 5, uint32_type, stream_offset_field, 4);
        push_node(&mut nodes, 5, uint8_type, stride_field, 1);
        push_node(&mut nodes, 5, uint8_type, divider_op_field, 1);
        push_node(&mut nodes, 5, uint16_type, frequency_field, 2);
        push_node(&mut nodes, 2, typeless_data_type, data_size_field, -1);
        push_node(&mut nodes, 3, int_type, size_field, 4);
        push_node(&mut nodes, 3, uint8_type, data_field, 1);

        let vertex_data: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let mut vertex_bytes = Vec::with_capacity(36);
        for value in vertex_data {
            vertex_bytes.extend_from_slice(&value.to_le_bytes());
        }

        let mut payload = Vec::new();
        // m_Name
        put_i32_le(&mut payload, 3);
        payload.extend_from_slice(b"Tri");
        payload.resize(payload.len() + 1, 0);
        // m_SubMeshes: one sub-mesh with three indices.
        put_i32_le(&mut payload, 1);
        put_u32_le(&mut payload, 0); // firstByte
        put_u32_le(&mut payload, 3); // indexCount
        put_i32_le(&mut payload, 0); // topology (Triangles)
        put_u32_le(&mut payload, 0); // baseVertex
        put_u32_le(&mut payload, 0); // firstVertex
        put_u32_le(&mut payload, 3); // vertexCount
        for _ in 0..6 {
            put_f32_le(&mut payload, 0.0); // m_Center then m_Extent
        }
        // m_IndexBuffer: 0, 1, 2 as 16-bit indices.
        put_i32_le(&mut payload, 6);
        for value in [0u16, 1, 2] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.resize(payload.len() + 2, 0);
        // m_IndexFormat: 16-bit.
        put_i32_le(&mut payload, 0);
        // m_VertexData
        put_u32_le(&mut payload, 3); // m_VertexCount
        put_i32_le(&mut payload, 1); // m_Channels size
        payload.push(3); // dimension
        payload.push(0); // offset
        payload.push(0); // format (Float32)
        payload.push(0); // stream
        put_i32_le(&mut payload, 1); // m_Streams size
        put_u32_le(&mut payload, 1); // channelMask
        put_u32_le(&mut payload, 0); // offset
        payload.push(12); // stride
        payload.push(0); // dividerOp
        put_u16_le(&mut payload, 0); // frequency
        put_i32_le(&mut payload, i32::try_from(vertex_bytes.len()).unwrap());
        payload.extend_from_slice(&vertex_bytes);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 43);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size) in &nodes {
            put_node(
                &mut metadata,
                *level,
                *type_offset,
                *name_offset,
                *byte_size,
                0,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 43);
        put_u16_le(&mut metadata, 43);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut bytes = Vec::with_capacity(file_size);
        put_i32_le_as_be(&mut bytes, i32::try_from(metadata.len()).unwrap());
        put_u32_be(&mut bytes, u32::try_from(file_size).unwrap());
        put_u32_be(&mut bytes, 10);
        put_u32_be(&mut bytes, u32::try_from(data_offset).unwrap());
        bytes.push(0);
        bytes.extend_from_slice(&[0; 3]);
        bytes.extend_from_slice(&metadata);
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn put_typetree_string_value(payload: &mut Vec<u8>, text: &str) {
        put_i32_le(payload, i32::try_from(text.len()).unwrap());
        payload.extend_from_slice(text.as_bytes());
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
    }

    fn finish_single_object_file(
        version: &[u8],
        class_id: i32,
        nodes: &[(u8, u32, u32, i32)],
        strings: &[u8],
        payload: &[u8],
        path_id: i32,
    ) -> Vec<u8> {
        finish_single_object_file_with_externals(
            version,
            class_id,
            nodes,
            strings,
            payload,
            path_id,
            &[],
        )
    }

    fn finish_single_object_file_with_externals(
        version: &[u8],
        class_id: i32,
        nodes: &[(u8, u32, u32, i32)],
        strings: &[u8],
        payload: &[u8],
        path_id: i32,
        external_names: &[&str],
    ) -> Vec<u8> {
        let mut metadata = Vec::new();
        metadata.extend_from_slice(version);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, class_id);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size) in nodes {
            put_node(
                &mut metadata,
                *level,
                *type_offset,
                *name_offset,
                *byte_size,
                0,
            );
        }
        metadata.extend_from_slice(strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, path_id);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, class_id);
        put_u16_le(&mut metadata, u16::try_from(class_id).unwrap());
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
        for external_name in external_names {
            metadata.push(0);
            metadata.extend_from_slice(&[0; 16]);
            put_i32_le(&mut metadata, 0);
            metadata.extend_from_slice(external_name.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut bytes = Vec::with_capacity(file_size);
        put_i32_le_as_be(&mut bytes, i32::try_from(metadata.len()).unwrap());
        put_u32_be(&mut bytes, u32::try_from(file_size).unwrap());
        put_u32_be(&mut bytes, 10);
        put_u32_be(&mut bytes, u32::try_from(data_offset).unwrap());
        bytes.push(0);
        bytes.extend_from_slice(&[0; 3]);
        bytes.extend_from_slice(&metadata);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn finish_single_object_file_with_tpk_tree(
        version: &[u8],
        class_id: i32,
        type_tree: &assetripper::serialized_file::metadata::TypeTree,
        payload: &[u8],
        path_id: i32,
        external_names: &[&str],
    ) -> Vec<u8> {
        let mut metadata = Vec::new();
        metadata.extend_from_slice(version);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, class_id);
        put_i32_le(&mut metadata, i32::try_from(type_tree.nodes.len()).unwrap());
        put_i32_le(
            &mut metadata,
            i32::try_from(type_tree.string_buffer.len()).unwrap(),
        );
        for node in &type_tree.nodes {
            put_node_with_version_and_meta(
                &mut metadata,
                u16::try_from(node.version).unwrap(),
                node.level,
                node.type_string_offset.unwrap(),
                node.name_string_offset.unwrap(),
                node.byte_size,
                node.index,
                node.meta_flags,
            );
        }
        metadata.extend_from_slice(&type_tree.string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, path_id);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, class_id);
        put_u16_le(&mut metadata, u16::try_from(class_id).unwrap());
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
        for external_name in external_names {
            metadata.push(0);
            metadata.extend_from_slice(&[0; 16]);
            put_i32_le(&mut metadata, 0);
            metadata.extend_from_slice(external_name.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut bytes = Vec::with_capacity(file_size);
        put_i32_le_as_be(&mut bytes, i32::try_from(metadata.len()).unwrap());
        put_u32_be(&mut bytes, u32::try_from(file_size).unwrap());
        put_u32_be(&mut bytes, 10);
        put_u32_be(&mut bytes, u32::try_from(data_offset).unwrap());
        bytes.push(0);
        bytes.extend_from_slice(&[0; 3]);
        bytes.extend_from_slice(&metadata);
        bytes.extend_from_slice(payload);
        bytes
    }

    /// Offsets into the synthetic TypeTree string buffer for `string` nodes.
    struct StringTreeTable {
        string_type: u32,
        array_type: u32,
        int_type: u32,
        size_field: u32,
        char_type: u32,
        data_field: u32,
    }

    fn string_tree_nodes(
        nodes: &mut Vec<(u8, u32, u32, i32)>,
        level: u8,
        table: &StringTreeTable,
        name: u32,
    ) {
        nodes.push((level, table.string_type, name, -1));
        nodes.push((level + 1, table.array_type, table.array_type, -1));
        nodes.push((level + 2, table.int_type, table.size_field, 4));
        nodes.push((level + 2, table.char_type, table.data_field, 1));
    }

    fn make_shader_file(script: &str) -> Vec<u8> {
        make_shader_file_with_path_id(script, 42)
    }

    fn make_shader_file_with_path_id(script: &str, path_id: i32) -> Vec<u8> {
        let mut strings = Vec::new();
        let shader_type = push_type_tree_string(&mut strings, "Shader");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let script_field = push_type_tree_string(&mut strings, "m_Script");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");

        let mut nodes = Vec::new();
        nodes.push((0, shader_type, empty_name, -1));
        let table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &table, name_field);
        string_tree_nodes(&mut nodes, 1, &table, script_field);

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "Test/Simple");
        put_typetree_string_value(&mut payload, script);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file(b"5.0.0f1\0", 48, &nodes, &strings, &payload, path_id)
    }

    fn make_shader_file_with_non_modifiable_texture_null() -> Vec<u8> {
        make_shader_file_with_non_modifiable_texture_pointer(0, 0, &[])
    }

    fn make_shader_file_with_non_modifiable_texture_pointer(
        file_id: i32,
        path_id: i64,
        external_names: &[&str],
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let shader_type = push_type_tree_string(&mut strings, "Shader");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let script_field = push_type_tree_string(&mut strings, "m_Script");
        let non_modifiable_field = push_type_tree_string(&mut strings, "m_NonModifiableTextures");
        let map_type = push_type_tree_string(&mut strings, "map");
        let pair_type = push_type_tree_string(&mut strings, "pair");
        let first_field = push_type_tree_string(&mut strings, "first");
        let second_field = push_type_tree_string(&mut strings, "second");
        let pptr_texture_type = push_type_tree_string(&mut strings, "PPtr<Texture>");
        let file_id_field = push_type_tree_string(&mut strings, "m_FileID");
        let path_id_type = push_type_tree_string(&mut strings, "SInt64");
        let path_id_field = push_type_tree_string(&mut strings, "m_PathID");

        let mut nodes = vec![(0, shader_type, empty_name, -1)];
        let string_table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &string_table, name_field);
        string_tree_nodes(&mut nodes, 1, &string_table, script_field);
        nodes.extend([
            (1, map_type, non_modifiable_field, -1),
            (2, array_type, array_type, -1),
            (3, int_type, size_field, 4),
            (3, pair_type, data_field, -1),
        ]);
        string_tree_nodes(&mut nodes, 4, &string_table, first_field);
        nodes.extend([
            (4, pptr_texture_type, second_field, -1),
            (5, int_type, file_id_field, 4),
            (5, path_id_type, path_id_field, 8),
        ]);

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "Test/Simple");
        put_typetree_string_value(&mut payload, "Shader \"Test/Simple\" { }\n");
        put_i32_le(&mut payload, 1);
        put_typetree_string_value(&mut payload, "_MainTex");
        put_i32_le(&mut payload, file_id);
        put_i64_le(&mut payload, path_id);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file_with_externals(
            b"2018.2.21f1\0",
            48,
            &nodes,
            &strings,
            &payload,
            42,
            external_names,
        )
    }

    fn assert_material_project_yaml_matches_fixture(material_file: Vec<u8>, fixture: &str) {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(&material_path, material_file).unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        assert_eq!(material_yaml.replace(shader_guid, "<SHADER_GUID>"), fixture);

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    fn make_minimal_texture_file(name: &str, path_id: i32) -> Vec<u8> {
        let mut strings = Vec::new();
        let texture_type = push_type_tree_string(&mut strings, "Texture2D");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let char_type = push_type_tree_string(&mut strings, "char");
        let data_field = push_type_tree_string(&mut strings, "data");
        let mut nodes = vec![(0, texture_type, empty_name, -1)];
        string_tree_nodes(
            &mut nodes,
            1,
            &StringTreeTable {
                string_type,
                array_type,
                int_type,
                size_field,
                char_type,
                data_field,
            },
            name_field,
        );
        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, name);
        finish_single_object_file(b"2018.2.21f1\0", 28, &nodes, &strings, &payload, path_id)
    }

    fn make_material_file() -> Vec<u8> {
        let mut strings = Vec::new();
        let material_type = push_type_tree_string(&mut strings, "Material");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let queue_field = push_type_tree_string(&mut strings, "m_CustomRenderQueue");

        let mut nodes = Vec::new();
        nodes.push((0, material_type, empty_name, -1));
        let table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &table, name_field);
        nodes.push((1, int_type, queue_field, 4));

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestMat");
        put_i32_le(&mut payload, -1);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file(b"2019.4.0f1\0", 21, &nodes, &strings, &payload, 42)
    }

    fn make_material_file_with_shader(shader_path_id: i32, external_name: &str) -> Vec<u8> {
        let mut strings = Vec::new();
        let material_type = push_type_tree_string(&mut strings, "Material");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let pptr_shader_type = push_type_tree_string(&mut strings, "PPtr<Shader>");
        let shader_field = push_type_tree_string(&mut strings, "m_Shader");
        let int_type = push_type_tree_string(&mut strings, "int");
        let file_id_field = push_type_tree_string(&mut strings, "m_FileID");
        let path_id_type = push_type_tree_string(&mut strings, "SInt32");
        let path_id_field = push_type_tree_string(&mut strings, "m_PathID");
        let queue_field = push_type_tree_string(&mut strings, "m_CustomRenderQueue");

        let mut nodes = vec![(0, material_type, empty_name, -1)];
        let string_table = StringTreeTable {
            string_type,
            array_type: push_type_tree_string(&mut strings, "Array"),
            int_type,
            size_field: push_type_tree_string(&mut strings, "size"),
            char_type: push_type_tree_string(&mut strings, "char"),
            data_field: push_type_tree_string(&mut strings, "data"),
        };
        string_tree_nodes(&mut nodes, 1, &string_table, name_field);
        nodes.extend([
            (1, pptr_shader_type, shader_field, -1),
            (2, int_type, file_id_field, 4),
            (2, path_id_type, path_id_field, 4),
            (1, int_type, queue_field, 4),
        ]);

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestMat");
        put_i32_le(&mut payload, 1);
        put_i32_le(&mut payload, shader_path_id);
        put_i32_le(&mut payload, -1);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file_with_externals(
            b"2019.4.0f1\0",
            21,
            &nodes,
            &strings,
            &payload,
            42,
            &[external_name],
        )
    }

    fn material_2018_tpk_release_tree() -> assetripper::serialized_file::metadata::TypeTree {
        material_tpk_release_tree(
            include_str!("export/material_tpk_2018_fixture.txt"),
            "TPK|21|2018.2.21f1|release",
        )
    }

    fn material_2018_3_tpk_release_tree() -> assetripper::serialized_file::metadata::TypeTree {
        material_tpk_release_tree(
            include_str!("export/material_tpk_2018_3_fixture.txt"),
            "TPK|21|2018.3.0f1|release",
        )
    }

    fn material_2019_4_tpk_release_tree() -> assetripper::serialized_file::metadata::TypeTree {
        material_tpk_release_tree(
            include_str!("export/material_tpk_2019_4_fixture.txt"),
            "TPK|21|2019.4.40f1|release",
        )
    }

    fn material_2022_3_tpk_release_tree() -> assetripper::serialized_file::metadata::TypeTree {
        material_tpk_release_tree(
            include_str!("export/material_tpk_2022_3_fixture.txt"),
            "TPK|21|2022.3.22f1|release",
        )
    }

    fn material_6000_4_tpk_release_tree() -> assetripper::serialized_file::metadata::TypeTree {
        material_tpk_release_tree(
            include_str!("export/material_tpk_6000_4_fixture.txt"),
            "TPK|21|6000.4.0f1|release",
        )
    }

    fn material_2017_4_tpk_release_tree() -> assetripper::serialized_file::metadata::TypeTree {
        material_tpk_release_tree(
            include_str!("export/material_tpk_2017_4_fixture.txt"),
            "TPK|21|2017.4.40f1|release",
        )
    }

    fn material_tpk_release_tree(
        fixture: &str,
        release_header: &str,
    ) -> assetripper::serialized_file::metadata::TypeTree {
        let mut nodes = Vec::new();
        let mut string_buffer = Vec::new();
        let mut in_release_tree = false;
        for line in fixture.lines() {
            if line.starts_with(release_header) {
                in_release_tree = true;
                continue;
            }
            if line.starts_with("TPK|") {
                in_release_tree = false;
                continue;
            }
            if !in_release_tree {
                continue;
            }
            let Some(row) = line.strip_prefix("NODE|") else {
                continue;
            };
            let fields = row.split('|').collect::<Vec<_>>();
            assert_eq!(
                fields.len(),
                5,
                "invalid committed Material TPK row: {line}"
            );
            let level = fields[0].parse::<u8>().unwrap();
            let type_name = fields[1];
            let name = fields[2];
            let version = fields[3].strip_prefix('v').unwrap().parse::<i32>().unwrap();
            let meta_flags = fields[4]
                .strip_prefix("meta")
                .unwrap()
                .parse::<u32>()
                .unwrap();
            let type_string_offset = u32::try_from(string_buffer.len()).unwrap();
            string_buffer.extend_from_slice(type_name.as_bytes());
            string_buffer.push(0);
            let name_string_offset = u32::try_from(string_buffer.len()).unwrap();
            string_buffer.extend_from_slice(name.as_bytes());
            string_buffer.push(0);
            let byte_size = match type_name {
                "char" | "UInt8" => 1,
                "int" | "unsigned int" | "UInt32" | "SInt32" | "float" => 4,
                "SInt64" | "UInt64" => 8,
                _ => -1,
            };
            nodes.push(assetripper::serialized_file::metadata::TypeTreeNode {
                level,
                version,
                type_flags: 0,
                type_string_offset: Some(type_string_offset),
                name_string_offset: Some(name_string_offset),
                type_name: Some(type_name.to_owned()),
                name: Some(name.to_owned()),
                byte_size,
                index: -1,
                meta_flags,
                reference_type_hash: None,
            });
        }
        assert_eq!(
            nodes.first().and_then(|node| node.type_name.as_deref()),
            Some("Material")
        );
        assetripper::serialized_file::metadata::TypeTree {
            nodes,
            string_buffer,
        }
    }

    fn make_material_file_with_reference_2018_tpk() -> Vec<u8> {
        make_material_file_with_reference_2018_tpk_and_properties(false)
    }

    fn make_material_file_with_reference_2018_tpk_and_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_2018_tpk_and_properties(true)
    }

    fn make_material_file_with_reference_2018_tpk_and_properties(
        include_saved_properties: bool,
    ) -> Vec<u8> {
        make_material_file_with_reference_2018_tpk_and_property_count(
            usize::from(include_saved_properties),
            false,
        )
    }

    fn make_material_file_with_reference_2018_tpk_with_multiple_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_2018_tpk_and_property_count(2, false)
    }

    fn make_material_file_with_reference_2018_tpk_with_duplicate_saved_property_keys() -> Vec<u8> {
        make_material_file_with_reference_2018_tpk_and_property_count(2, true)
    }

    fn make_material_file_with_reference_2018_tpk_and_property_count(
        saved_property_count: usize,
        duplicate_keys: bool,
    ) -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties(
            b"2018.2.21f1\0",
            material_2018_tpk_release_tree(),
            saved_property_count,
            duplicate_keys,
        )
    }

    fn make_material_file_with_reference_2018_3_tpk_and_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties(
            b"2018.3.0f1\0",
            material_2018_3_tpk_release_tree(),
            1,
            false,
        )
    }

    fn make_material_file_with_reference_2019_4_tpk_and_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties(
            b"2019.4.40f1\0",
            material_2019_4_tpk_release_tree(),
            1,
            false,
        )
    }

    fn make_material_file_with_reference_2022_3_tpk_and_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties(
            b"2022.3.22f1\0",
            material_2022_3_tpk_release_tree(),
            1,
            false,
        )
    }

    fn make_material_file_with_reference_6000_4_tpk_and_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties(
            b"6000.4.0f1\0",
            material_6000_4_tpk_release_tree(),
            1,
            false,
        )
    }

    fn make_material_file_with_reference_2019_4_tpk_and_texture_reference() -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties_and_texture_pointer(
            b"2019.4.40f1\0",
            material_2019_4_tpk_release_tree(),
            1,
            false,
            Some((2, 88)),
            &["shader.assets", "texture.assets"],
        )
    }

    fn make_material_file_with_reference_2017_4_tpk_and_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties(
            b"2017.4.40f1\0",
            material_2017_4_tpk_release_tree(),
            1,
            false,
        )
    }

    fn make_material_file_with_reference_tpk_and_properties(
        version: &[u8],
        type_tree: assetripper::serialized_file::metadata::TypeTree,
        saved_property_count: usize,
        duplicate_keys: bool,
    ) -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties_and_texture_pointer(
            version,
            type_tree,
            saved_property_count,
            duplicate_keys,
            None,
            &["shader.assets"],
        )
    }

    fn make_material_file_with_reference_tpk_and_properties_and_texture_pointer(
        version: &[u8],
        type_tree: assetripper::serialized_file::metadata::TypeTree,
        saved_property_count: usize,
        duplicate_keys: bool,
        texture_pointer: Option<(i32, i64)>,
        external_names: &[&str],
    ) -> Vec<u8> {
        let has_split_keywords = type_tree
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some("m_ValidKeywords"));
        let has_int_map = type_tree
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some("m_Ints"));
        let has_build_texture_stacks = type_tree
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some("m_BuildTextureStacks"));
        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "MaterialFixture");
        put_i32_le(&mut payload, 1);
        put_i64_le(&mut payload, 42);
        if has_split_keywords {
            put_i32_le(&mut payload, 1);
            put_typetree_string_value(&mut payload, "_FEATURE_A");
            put_i32_le(&mut payload, 1);
            put_typetree_string_value(&mut payload, "_FEATURE_B");
        } else {
            put_typetree_string_value(&mut payload, "_FEATURE_A _FEATURE_B");
        }
        put_u32_le(&mut payload, 4);
        payload.push(1);
        payload.push(1);
        while !payload.len().is_multiple_of(4) {
            payload.push(0);
        }
        put_i32_le(&mut payload, 2450);
        put_i32_le(&mut payload, 1);
        put_typetree_string_value(&mut payload, "RenderType");
        put_typetree_string_value(&mut payload, "TransparentCutout");
        put_i32_le(&mut payload, 0); // disabledShaderPasses
        if saved_property_count > 0 {
            let map_length = i32::try_from(saved_property_count).unwrap();
            put_i32_le(&mut payload, map_length);
            put_typetree_string_value(&mut payload, "_MainTex");
            let (texture_file_id, texture_path_id) = texture_pointer.unwrap_or_default();
            put_i32_le(&mut payload, texture_file_id);
            put_i64_le(&mut payload, texture_path_id);
            put_f32_le(&mut payload, 1.25);
            put_f32_le(&mut payload, -2.5);
            put_f32_le(&mut payload, 0.125);
            put_f32_le(&mut payload, 0.75);
            if has_int_map {
                put_i32_le(&mut payload, 0);
            }
            if saved_property_count > 1 {
                put_typetree_string_value(
                    &mut payload,
                    if duplicate_keys {
                        "_MainTex"
                    } else {
                        "_ATexture"
                    },
                );
                put_i32_le(&mut payload, 0);
                put_i64_le(&mut payload, 0);
                put_f32_le(&mut payload, 2.0);
                put_f32_le(&mut payload, 0.5);
                put_f32_le(&mut payload, -0.25);
                put_f32_le(&mut payload, 0.125);
            }

            put_i32_le(&mut payload, map_length);
            put_typetree_string_value(
                &mut payload,
                if saved_property_count > 1 && !duplicate_keys {
                    "_ZFloat"
                } else {
                    "_Glossiness"
                },
            );
            put_f32_le(&mut payload, 0.375);
            if saved_property_count > 1 {
                put_typetree_string_value(
                    &mut payload,
                    if duplicate_keys {
                        "_Glossiness"
                    } else {
                        "_AFloat"
                    },
                );
                put_f32_le(&mut payload, -0.125);
            }

            put_i32_le(&mut payload, map_length);
            put_typetree_string_value(
                &mut payload,
                if saved_property_count > 1 && !duplicate_keys {
                    "_ZColor"
                } else {
                    "_Color"
                },
            );
            put_f32_le(&mut payload, 0.125);
            put_f32_le(&mut payload, 0.25);
            put_f32_le(&mut payload, 0.5);
            put_f32_le(&mut payload, 0.75);
            if saved_property_count > 1 {
                put_typetree_string_value(
                    &mut payload,
                    if duplicate_keys { "_Color" } else { "_AColor" },
                );
                put_f32_le(&mut payload, 1.0);
                put_f32_le(&mut payload, 0.5);
                put_f32_le(&mut payload, 0.25);
                put_f32_le(&mut payload, 1.0);
            }
        } else {
            put_i32_le(&mut payload, 0);
            if has_int_map {
                put_i32_le(&mut payload, 0);
            }
            put_i32_le(&mut payload, 0);
            put_i32_le(&mut payload, 0);
        }
        if has_build_texture_stacks {
            put_i32_le(&mut payload, 0);
        }
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file_with_tpk_tree(
            version,
            21,
            &type_tree,
            &payload,
            42,
            external_names,
        )
    }

    fn make_audio_clip_file(data: &[u8], sound_type: Option<i32>) -> Vec<u8> {
        let mut strings = Vec::new();
        let clip_type = push_type_tree_string(&mut strings, "AudioClip");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let typeless_type = push_type_tree_string(&mut strings, "TypelessData");
        let audio_data_field = push_type_tree_string(&mut strings, "m_AudioData");
        let uint8_type = push_type_tree_string(&mut strings, "UInt8");
        let type_field = push_type_tree_string(&mut strings, "m_Type");

        let mut nodes = Vec::new();
        nodes.push((0, clip_type, empty_name, -1));
        let table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &table, name_field);
        nodes.push((1, typeless_type, audio_data_field, -1));
        nodes.push((2, int_type, size_field, 4));
        nodes.push((2, uint8_type, data_field, 1));
        if sound_type.is_some() {
            nodes.push((1, int_type, type_field, 4));
        }

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestClip");
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap());
        payload.extend_from_slice(data);
        if let Some(sound_type) = sound_type {
            put_i32_le(&mut payload, sound_type);
        }
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file(b"3.5.0f1\0", 83, &nodes, &strings, &payload, 42)
    }

    fn make_streamed_audio_clip_file(
        stream_path: &str,
        stream_offset: u64,
        stream_size: u32,
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let clip_type = push_type_tree_string(&mut strings, "AudioClip");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let resource_type = push_type_tree_string(&mut strings, "StreamingInfo");
        let resource_field = push_type_tree_string(&mut strings, "m_Resource");
        let path_field = push_type_tree_string(&mut strings, "path");
        let offset_type = push_type_tree_string(&mut strings, "UInt64");
        let offset_field = push_type_tree_string(&mut strings, "offset");
        let uint32_type = push_type_tree_string(&mut strings, "UInt32");

        let mut nodes = Vec::new();
        nodes.push((0, clip_type, empty_name, -1));
        let table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &table, name_field);
        nodes.push((1, resource_type, resource_field, -1));
        string_tree_nodes(&mut nodes, 2, &table, path_field);
        nodes.push((2, offset_type, offset_field, 8));
        nodes.push((2, uint32_type, size_field, 4));

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestClip");
        put_typetree_string_value(&mut payload, stream_path);
        payload.extend_from_slice(&stream_offset.to_le_bytes());
        put_u32_le(&mut payload, stream_size);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file(b"2019.4.0f1\0", 83, &nodes, &strings, &payload, 42)
    }
