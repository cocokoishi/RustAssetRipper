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

    /// Build a `Mesh` file matching the **production** class-43 layout at 2019.4.0f1.
    ///
    /// The ten-node `make_mesh_file` sketch above decodes only under the synthetic layout source:
    /// production places `m_Shapes` between `m_SubMeshes` and `m_BindPose`, then
    /// `m_BoneNameHashes`, `m_RootBoneNameHash`, `m_BonesAABB`, `m_VariableBoneCountWeights`, the
    /// compression byte, three readability flags, `m_IndexFormat`, `m_IndexBuffer`,
    /// `m_VertexData`, `m_CompressedMesh`, `m_LocalAABB`, `m_MeshUsageFlags`, the two baked
    /// collision meshes, two mesh metrics and `m_StreamData`. This builder writes that full field
    /// list with empty containers where the reference has none, and one triangle of real geometry.
    ///
    /// The vertex stream is one `Float32` dimension-3 channel at offset 0 with stride 12.
    pub(super) fn make_production_mesh_file() -> Vec<u8> {
        let mut strings = Vec::new();
        let t = |strings: &mut Vec<u8>, value: &str| push_type_tree_string(strings, value);
        let mesh_type = t(&mut strings, "Mesh");
        let empty = t(&mut strings, "");
        let array_type = t(&mut strings, "Array");
        let int_type = t(&mut strings, "int");
        let uint_type = t(&mut strings, "unsigned int");
        let uint8_type = t(&mut strings, "UInt8");
        let char_type = t(&mut strings, "char");
        let bool_type = t(&mut strings, "bool");
        let float_type = t(&mut strings, "float");
        let vector_type = t(&mut strings, "vector");
        let vector3_type = t(&mut strings, "Vector3f");
        let aabb_type = t(&mut strings, "AABB");
        let sub_mesh_type = t(&mut strings, "SubMesh");
        let blend_data_type = t(&mut strings, "BlendShapeData");
        let channel_info_type = t(&mut strings, "ChannelInfo");
        let vertex_data_type = t(&mut strings, "VertexData");
        let typeless_type = t(&mut strings, "TypelessData");
        let compressed_type = t(&mut strings, "CompressedMesh");
        let packed_type = t(&mut strings, "PackedBitVector");
        let variable_type = t(&mut strings, "VariableBoneCountWeights");
        let streaming_type = t(&mut strings, "StreamingInfo");

        let f_name = t(&mut strings, "m_Name");
        let f_size = t(&mut strings, "size");
        let f_data = t(&mut strings, "data");
        let f_sub_meshes = t(&mut strings, "m_SubMeshes");
        let f_first_byte = t(&mut strings, "firstByte");
        let f_index_count = t(&mut strings, "indexCount");
        let f_topology = t(&mut strings, "topology");
        let f_base_vertex = t(&mut strings, "baseVertex");
        let f_first_vertex = t(&mut strings, "firstVertex");
        let f_vertex_count = t(&mut strings, "vertexCount");
        let f_local_aabb = t(&mut strings, "localAABB");
        let f_center = t(&mut strings, "m_Center");
        let f_extent = t(&mut strings, "m_Extent");
        let f_x = t(&mut strings, "x");
        let f_y = t(&mut strings, "y");
        let f_z = t(&mut strings, "z");
        let f_shapes = t(&mut strings, "m_Shapes");
        let f_bs_vertices = t(&mut strings, "vertices");
        let f_shapes_vec = t(&mut strings, "shapes");
        let f_channels = t(&mut strings, "channels");
        let f_full_weights = t(&mut strings, "fullWeights");
        let f_bind_pose = t(&mut strings, "m_BindPose");
        let f_bone_hashes = t(&mut strings, "m_BoneNameHashes");
        let f_root_bone_hash = t(&mut strings, "m_RootBoneNameHash");
        let f_bones_aabb = t(&mut strings, "m_BonesAABB");
        let f_variable = t(&mut strings, "m_VariableBoneCountWeights");
        let f_data_vec = t(&mut strings, "m_Data");
        let f_mesh_compression = t(&mut strings, "m_MeshCompression");
        let f_is_readable = t(&mut strings, "m_IsReadable");
        let f_keep_vertices = t(&mut strings, "m_KeepVertices");
        let f_keep_indices = t(&mut strings, "m_KeepIndices");
        let f_index_format = t(&mut strings, "m_IndexFormat");
        let f_index_buffer = t(&mut strings, "m_IndexBuffer");
        let f_vertex_data = t(&mut strings, "m_VertexData");
        let f_vd_count = t(&mut strings, "m_VertexCount");
        let f_vd_channels = t(&mut strings, "m_Channels");
        let f_stream = t(&mut strings, "stream");
        let f_offset = t(&mut strings, "offset");
        let f_format = t(&mut strings, "format");
        let f_dimension = t(&mut strings, "dimension");
        let f_data_size = t(&mut strings, "m_DataSize");
        let f_compressed = t(&mut strings, "m_CompressedMesh");
        let f_num_items = t(&mut strings, "m_NumItems");
        let f_range = t(&mut strings, "m_Range");
        let f_start = t(&mut strings, "m_Start");
        let f_bit_size = t(&mut strings, "m_BitSize");
        let f_local_aabb2 = t(&mut strings, "m_LocalAABB");
        let f_usage_flags = t(&mut strings, "m_MeshUsageFlags");
        let f_baked_convex = t(&mut strings, "m_BakedConvexCollisionMesh");
        let f_baked_tri = t(&mut strings, "m_BakedTriangleCollisionMesh");
        let f_metric0 = t(&mut strings, "m_MeshMetrics[0]");
        let f_metric1 = t(&mut strings, "m_MeshMetrics[1]");
        let f_stream_data = t(&mut strings, "m_StreamData");
        let f_path = t(&mut strings, "path");

        let mut nodes: Vec<(u8, u32, u32, i32)> = Vec::new();
        fn push(
            nodes: &mut Vec<(u8, u32, u32, i32)>,
            level: u8,
            type_offset: u32,
            name_offset: u32,
            byte_size: i32,
        ) {
            nodes.push((level, type_offset, name_offset, byte_size));
        }
        // A `string` field: Array -> size (int), data (char).
        fn push_string(
            nodes: &mut Vec<(u8, u32, u32, i32)>,
            level: u8,
            name: u32,
            types: (u32, u32, u32),
            names: (u32, u32),
        ) {
            let (array_type, int_type, char_type) = types;
            let (size, data) = names;
            push(nodes, level, array_type, name, -1);
            push(nodes, level + 1, array_type, array_type, -1);
            push(nodes, level + 2, int_type, size, 4);
            push(nodes, level + 2, char_type, data, 1);
        }
        // An empty `vector`: Array -> size only.
        fn push_empty_vector(
            nodes: &mut Vec<(u8, u32, u32, i32)>,
            level: u8,
            name: u32,
            types: (u32, u32, u32),
            size: u32,
        ) {
            let (vector_type, array_type, int_type) = types;
            push(nodes, level, vector_type, name, -1);
            push(nodes, level + 1, array_type, array_type, -1);
            push(nodes, level + 2, int_type, size, 4);
        }
        // A `Vector3f` of three floats.
        fn push_vector3(
            nodes: &mut Vec<(u8, u32, u32, i32)>,
            level: u8,
            name: u32,
            types: (u32, u32),
            components: (u32, u32, u32),
        ) {
            let (vector3_type, float_type) = types;
            push(nodes, level, vector3_type, name, 12);
            for component in [components.0, components.1, components.2] {
                push(nodes, level + 1, float_type, component, 4);
            }
        }

        let string_types = (array_type, int_type, char_type);
        let vector_types = (vector_type, array_type, int_type);
        let vector3_types = (vector3_type, float_type);
        let coords = (f_x, f_y, f_z);

        push(&mut nodes, 0, mesh_type, empty, -1);
        push_string(
            &mut nodes,
            1,
            f_name,
            string_types,
            (f_size, f_data),
        );
        // m_SubMeshes: one 48-byte SubMesh.
        push(&mut nodes, 1, vector_type, f_sub_meshes, -1);
        push(&mut nodes, 2, array_type, array_type, -1);
        push(&mut nodes, 3, int_type, f_size, 4);
        push(&mut nodes, 3, sub_mesh_type, f_data, 48);
        push(&mut nodes, 4, uint_type, f_first_byte, 4);
        push(&mut nodes, 4, uint_type, f_index_count, 4);
        push(&mut nodes, 4, int_type, f_topology, 4);
        push(&mut nodes, 4, uint_type, f_base_vertex, 4);
        push(&mut nodes, 4, uint_type, f_first_vertex, 4);
        push(&mut nodes, 4, uint_type, f_vertex_count, 4);
        push(&mut nodes, 4, aabb_type, f_local_aabb, 24);
        push_vector3(&mut nodes, 5, f_center, vector3_types, coords);
        push_vector3(&mut nodes, 5, f_extent, vector3_types, coords);
        // m_Shapes: a BlendShapeData with four empty vectors.
        push(&mut nodes, 1, blend_data_type, f_shapes, -1);
        for name in [f_bs_vertices, f_shapes_vec, f_channels, f_full_weights] {
            push_empty_vector(&mut nodes, 2, name, vector_types, f_size);
        }
        // Bone data: three empty vectors, a root hash, an empty AABB vector, then the
        // variable-bone-count weights' own empty vector.
        for name in [f_bind_pose, f_bone_hashes] {
            push_empty_vector(&mut nodes, 1, name, vector_types, f_size);
        }
        push(&mut nodes, 1, uint_type, f_root_bone_hash, 4);
        push_empty_vector(&mut nodes, 1, f_bones_aabb, vector_types, f_size);
        push(&mut nodes, 1, variable_type, f_variable, -1);
        push_empty_vector(&mut nodes, 2, f_data_vec, vector_types, f_size);
        push(&mut nodes, 1, uint8_type, f_mesh_compression, 1);
        push(&mut nodes, 1, bool_type, f_is_readable, 1);
        push(&mut nodes, 1, bool_type, f_keep_vertices, 1);
        push(&mut nodes, 1, bool_type, f_keep_indices, 1);
        push(&mut nodes, 1, int_type, f_index_format, 4);
        push(&mut nodes, 1, vector_type, f_index_buffer, -1);
        push(&mut nodes, 2, array_type, array_type, -1);
        push(&mut nodes, 3, int_type, f_size, 4);
        push(&mut nodes, 3, uint8_type, f_data, 1);
        // m_VertexData: count, one channel, then the raw data blob.
        push(&mut nodes, 1, vertex_data_type, f_vertex_data, -1);
        push(&mut nodes, 2, uint_type, f_vd_count, 4);
        push(&mut nodes, 2, vector_type, f_vd_channels, -1);
        push(&mut nodes, 3, array_type, array_type, -1);
        push(&mut nodes, 4, int_type, f_size, 4);
        push(&mut nodes, 4, channel_info_type, f_data, 4);
        push(&mut nodes, 5, uint8_type, f_stream, 1);
        push(&mut nodes, 5, uint8_type, f_offset, 1);
        push(&mut nodes, 5, uint8_type, f_format, 1);
        push(&mut nodes, 5, uint8_type, f_dimension, 1);
        push(&mut nodes, 2, typeless_type, f_data_size, -1);
        push(&mut nodes, 3, int_type, f_size, 4);
        push(&mut nodes, 3, uint8_type, f_data, 1);
        // m_CompressedMesh: ten PackedBitVectors. The production order interleaves the two
        // shapes - `m_Vertices`, `m_UV`, `m_Normals` and `m_Tangents` carry a range and a start,
        // then `m_Weights`, `m_NormalSigns` and `m_TangentSigns` do not, then `m_FloatColors`
        // does, and `m_BoneIndices` and `m_Triangles` do not. The payload is positional, so
        // reading that order matters.
        let packed: [(u32, bool); 10] = [
            (t(&mut strings, "m_Vertices"), true),
            (t(&mut strings, "m_UV"), true),
            (t(&mut strings, "m_Normals"), true),
            (t(&mut strings, "m_Tangents"), true),
            (t(&mut strings, "m_Weights"), false),
            (t(&mut strings, "m_NormalSigns"), false),
            (t(&mut strings, "m_TangentSigns"), false),
            (t(&mut strings, "m_FloatColors"), true),
            (t(&mut strings, "m_BoneIndices"), false),
            (t(&mut strings, "m_Triangles"), false),
        ];
        push(&mut nodes, 1, compressed_type, f_compressed, -1);
        for (name, has_range) in packed {
            push(&mut nodes, 2, packed_type, name, -1);
            push(&mut nodes, 3, uint_type, f_num_items, 4);
            if has_range {
                push(&mut nodes, 3, float_type, f_range, 4);
                push(&mut nodes, 3, float_type, f_start, 4);
            }
            push(&mut nodes, 3, vector_type, f_data_vec, -1);
            push(&mut nodes, 4, array_type, array_type, -1);
            push(&mut nodes, 5, int_type, f_size, 4);
            push(&mut nodes, 5, uint8_type, f_data, 1);
            push(&mut nodes, 3, uint8_type, f_bit_size, 1);
        }
        push(&mut nodes, 2, uint_type, t(&mut strings, "m_UVInfo"), 4);
        push(&mut nodes, 1, aabb_type, f_local_aabb2, 24);
        push_vector3(&mut nodes, 2, f_center, vector3_types, coords);
        push_vector3(&mut nodes, 2, f_extent, vector3_types, coords);
        push(&mut nodes, 1, int_type, f_usage_flags, 4);
        push_empty_vector(&mut nodes, 1, f_baked_convex, vector_types, f_size);
        push_empty_vector(&mut nodes, 1, f_baked_tri, vector_types, f_size);
        push(&mut nodes, 1, float_type, f_metric0, 4);
        push(&mut nodes, 1, float_type, f_metric1, 4);
        push(&mut nodes, 1, streaming_type, f_stream_data, -1);
        push(&mut nodes, 2, uint_type, f_offset, 4);
        push(&mut nodes, 2, uint_type, f_size, 4);
        push_string(&mut nodes, 2, f_path, string_types, (f_size, f_data));

        let vertex_data: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let mut vertex_bytes = Vec::with_capacity(36);
        for value in vertex_data {
            vertex_bytes.extend_from_slice(&value.to_le_bytes());
        }

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "Tri");
        // m_SubMeshes: one sub-mesh with three indices.
        put_i32_le(&mut payload, 1);
        put_u32_le(&mut payload, 0); // firstByte
        put_u32_le(&mut payload, 3); // indexCount
        put_i32_le(&mut payload, 0); // topology
        put_u32_le(&mut payload, 0); // baseVertex
        put_u32_le(&mut payload, 0); // firstVertex
        put_u32_le(&mut payload, 3); // vertexCount
        for _ in 0..6 {
            put_f32_le(&mut payload, 0.0); // localAABB center + extent
        }
        // m_Shapes: four empty vectors.
        for _ in 0..4 {
            put_i32_le(&mut payload, 0);
        }
        // m_BindPose, m_BoneNameHashes, m_RootBoneNameHash, m_BonesAABB, then the
        // VariableBoneCountWeights' own empty vector.
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        put_u32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        // m_MeshCompression, the three readability flags, m_IndexFormat.
        payload.push(0);
        payload.push(1);
        payload.push(1);
        payload.push(1);
        put_i32_le(&mut payload, 0); // m_IndexFormat: 16-bit
        // m_IndexBuffer: 0, 1, 2 as 16-bit indices.
        put_i32_le(&mut payload, 6);
        for value in [0u16, 1, 2] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.resize(payload.len() + 2, 0);
        // m_VertexData
        put_u32_le(&mut payload, 3); // m_VertexCount
        put_i32_le(&mut payload, 1); // m_Channels size
        payload.push(0); // stream
        payload.push(0); // offset
        payload.push(0); // format (Float32)
        payload.push(3); // dimension
        put_i32_le(&mut payload, i32::try_from(vertex_bytes.len()).unwrap());
        payload.extend_from_slice(&vertex_bytes);
        // m_CompressedMesh: every PackedBitVector empty, in the production field order. The
        // reader aligns the next 4-byte field after each trailing `m_BitSize` byte, so every
        // vector ends on a word boundary.
        for extra_floats in [2, 2, 2, 2, 0, 0, 0, 2, 0, 0] {
            put_u32_le(&mut payload, 0); // m_NumItems
            for _ in 0..extra_floats {
                put_f32_le(&mut payload, 0.0); // m_Range, m_Start
            }
            put_i32_le(&mut payload, 0); // m_Data size
            payload.push(0); // m_BitSize
            while !payload.len().is_multiple_of(4) {
                payload.push(0);
            }
        }
        put_u32_le(&mut payload, 0); // m_UVInfo
        // m_LocalAABB center + extent.
        for _ in 0..6 {
            put_f32_le(&mut payload, 0.0);
        }
        put_i32_le(&mut payload, 0); // m_MeshUsageFlags
        put_i32_le(&mut payload, 0); // m_BakedConvexCollisionMesh (empty)
        put_i32_le(&mut payload, 0); // m_BakedTriangleCollisionMesh (empty)
        put_f32_le(&mut payload, 0.0); // m_MeshMetrics[0]
        put_f32_le(&mut payload, 0.0); // m_MeshMetrics[1]
        put_u32_le(&mut payload, 0); // m_StreamData.offset
        put_u32_le(&mut payload, 0); // m_StreamData.size
        put_i32_le(&mut payload, 0); // m_StreamData.path (empty)
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        finish_single_object_file(b"2019.4.0f1\0", 43, &nodes, &strings, &payload, 42)
    }

    #[test]
    fn production_mesh_fixture_decodes_and_builds_a_glb() {
        use crate::{load_object_workspace, load_workspace_from_vfs};
        use assetripper::io::dependency::DependencyFilePath;
        use assetripper::io::filesystem::VirtualFileSystem;
        use assetripper::serialized_file::object_workspace::ObjectWorkspaceLimits;
        use assetripper::serialized_file::workspace::SerializedFileWorkspaceLimits;

        let _production = crate::SourceFactoryLayoutGuard::use_source_factory();
        let bytes = make_production_mesh_file();
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem
            .write_all_bytes("/game/mesh.assets", &bytes)
            .unwrap();
        let input = DependencyFilePath {
            file_name: "mesh.assets".to_owned(),
            virtual_path: "/game/mesh.assets".to_owned(),
        };
        let workspace = load_workspace_from_vfs(
            &filesystem,
            &[input],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("the workspace loads");
        let objects =
            load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())
                .expect("the object workspace loads");
        let object = objects.get(0, 0).expect("the mesh exists");
        let payload = object
            .payload
            .as_ref()
            .unwrap_or_else(|error| panic!("the mesh must decode: {error}"));
        let view = assetripper::resources::mesh::MeshView::from_value(
            &payload.root,
            assetripper::resources::vertex_data_blob::UnityVersion {
                major: 2019,
                minor: 4,
                patch: 0,
            },
            assetripper::resources::vertex_data_blob::EndianType::Little,
        )
        .unwrap_or_else(|error| panic!("the mesh view reads: {error}"));
        let mesh_data = view
            .read_mesh_data(
                assetripper::resources::packed_bit_vector::PackedBitVectorLimits::default(),
            )
            .unwrap_or_else(|error| panic!("the mesh data reads: {error}"))
            .expect("the mesh has data");
        assert_eq!(mesh_data.vertices.len(), 3);
        let exported = assetripper::resources::glb::build_glb("Tri", &mesh_data)
            .unwrap_or_else(|error| panic!("the GLB builds: {error}"));
        assert_eq!(&exported.bytes[0..4], b"glTF");
    }

    /// A class-43 `Mesh` whose vertex data is a single Float32 position channel and whose index
    /// buffer holds one 16-bit triangle.
    pub(super) fn make_mesh_file() -> Vec<u8> {
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
        make_material_file_with_reference_tpk_and_properties_and_texture_pointers(
            b"2019.4.40f1\0",
            material_2019_4_tpk_release_tree(),
            1,
            false,
            Some((2, 88)),
            None,
            &["shader.assets", "texture.assets"],
        )
    }

    fn make_material_file_with_reference_2019_4_tpk_and_two_texture_references() -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties_and_texture_pointers(
            b"2019.4.40f1\0",
            material_2019_4_tpk_release_tree(),
            2,
            false,
            Some((2, 88)),
            Some((3, 89)),
            &["shader.assets", "texture_a.assets", "texture_b.assets"],
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
        make_material_file_with_reference_tpk_and_properties_and_texture_pointers(
            version,
            type_tree,
            saved_property_count,
            duplicate_keys,
            None,
            None,
            &["shader.assets"],
        )
    }

    fn make_material_file_with_reference_tpk_and_properties_and_texture_pointers(
        version: &[u8],
        type_tree: assetripper::serialized_file::metadata::TypeTree,
        saved_property_count: usize,
        duplicate_keys: bool,
        texture_pointer: Option<(i32, i64)>,
        second_texture_pointer: Option<(i32, i64)>,
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
                let (texture_file_id, texture_path_id) = second_texture_pointer.unwrap_or_default();
                put_i32_le(&mut payload, texture_file_id);
                put_i64_le(&mut payload, texture_path_id);
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
        make_audio_clip_file_with_version(data, sound_type, b"3.5.0f1\0")
    }

    fn make_audio_clip_file_with_version(
        data: &[u8],
        sound_type: Option<i32>,
        version: &[u8],
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
        finish_single_object_file(version, 83, &nodes, &strings, &payload, 42)
    }

    /// Build a `VideoClip` file for the video route and tab, matching the **production**
    /// class-329 layout at 3.5.0f1.
    ///
    /// That record is `m_Name`, `m_OriginalPath`, `m_ProxyWidth`, `m_ProxyHeight`, `Width`,
    /// `Height`, a `double` `m_FrameRate`, a `UInt64` `m_FrameCount`, `m_Format`, three empty
    /// audio vectors, a `StreamedResource` `m_ExternalResources` (whose fields are `m_Source`,
    /// `m_Offset`, `m_Size`, not the `StreamingInfo` spelling), and a trailing `m_HasSplitAlpha`
    /// bool. A fixture that writes only the two strings and a `StreamingInfo` fails at
    /// `m_FrameCount`.
    pub(super) fn make_video_clip_file(
        original_path: &str,
        resource_path: &str,
        size: u32,
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let clip_type = push_type_tree_string(&mut strings, "VideoClip");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let char_type = push_type_tree_string(&mut strings, "char");
        let unsigned_int_type = push_type_tree_string(&mut strings, "unsigned int");
        let double_type = push_type_tree_string(&mut strings, "double");
        let uint64_type = push_type_tree_string(&mut strings, "UInt64");
        let uint32_type = push_type_tree_string(&mut strings, "UInt32");
        let uint16_type = push_type_tree_string(&mut strings, "UInt16");
        let bool_type = push_type_tree_string(&mut strings, "bool");
        let vector_type = push_type_tree_string(&mut strings, "vector");

        let original_path_field = push_type_tree_string(&mut strings, "m_OriginalPath");
        let proxy_width_field = push_type_tree_string(&mut strings, "m_ProxyWidth");
        let proxy_height_field = push_type_tree_string(&mut strings, "m_ProxyHeight");
        let width_field = push_type_tree_string(&mut strings, "Width");
        let height_field = push_type_tree_string(&mut strings, "Height");
        let frame_rate_field = push_type_tree_string(&mut strings, "m_FrameRate");
        let frame_count_field = push_type_tree_string(&mut strings, "m_FrameCount");
        let format_field = push_type_tree_string(&mut strings, "m_Format");
        let channel_count_field = push_type_tree_string(&mut strings, "m_AudioChannelCount");
        let sample_rate_field = push_type_tree_string(&mut strings, "m_AudioSampleRate");
        let language_field = push_type_tree_string(&mut strings, "m_AudioLanguage");
        let streamed_type = push_type_tree_string(&mut strings, "StreamedResource");
        let external_field = push_type_tree_string(&mut strings, "m_ExternalResources");
        let source_field = push_type_tree_string(&mut strings, "m_Source");
        let offset_field = push_type_tree_string(&mut strings, "m_Offset");
        let resource_size_field = push_type_tree_string(&mut strings, "m_Size");
        let split_alpha_field = push_type_tree_string(&mut strings, "m_HasSplitAlpha");

        let mut nodes = Vec::new();
        nodes.push((0, clip_type, empty_name, -1));
        let table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field: push_type_tree_string(&mut strings, "data"),
        };
        string_tree_nodes(&mut nodes, 1, &table, name_field);
        string_tree_nodes(&mut nodes, 1, &table, original_path_field);
        nodes.push((1, unsigned_int_type, proxy_width_field, 4));
        nodes.push((1, unsigned_int_type, proxy_height_field, 4));
        nodes.push((1, unsigned_int_type, width_field, 4));
        nodes.push((1, unsigned_int_type, height_field, 4));
        nodes.push((1, double_type, frame_rate_field, 8));
        nodes.push((1, uint64_type, frame_count_field, 8));
        nodes.push((1, int_type, format_field, 4));
        // Three empty vectors: a single `Array` child each, with no elements.
        nodes.push((1, vector_type, channel_count_field, -1));
        nodes.push((2, array_type, empty_name, -1));
        nodes.push((3, int_type, size_field, 4));
        nodes.push((1, vector_type, sample_rate_field, -1));
        nodes.push((2, array_type, empty_name, -1));
        nodes.push((3, int_type, size_field, 4));
        nodes.push((1, vector_type, language_field, -1));
        nodes.push((2, array_type, empty_name, -1));
        nodes.push((3, int_type, size_field, 4));
        // The streamed descriptor uses the `StreamedResource` spelling of its three fields.
        nodes.push((1, streamed_type, external_field, -1));
        string_tree_nodes(&mut nodes, 2, &table, source_field);
        nodes.push((2, uint64_type, offset_field, 8));
        nodes.push((2, uint64_type, resource_size_field, 8));
        nodes.push((1, bool_type, split_alpha_field, 1));

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestVideo");
        put_typetree_string_value(&mut payload, original_path);
        put_u32_le(&mut payload, 0); // m_ProxyWidth
        put_u32_le(&mut payload, 0); // m_ProxyHeight
        put_u32_le(&mut payload, 0); // Width
        put_u32_le(&mut payload, 0); // Height
        payload.extend_from_slice(&0f64.to_le_bytes()); // m_FrameRate
        payload.extend_from_slice(&0u64.to_le_bytes()); // m_FrameCount
        put_i32_le(&mut payload, 0); // m_Format
        put_i32_le(&mut payload, 0); // m_AudioChannelCount (empty)
        put_i32_le(&mut payload, 0); // m_AudioSampleRate (empty)
        put_i32_le(&mut payload, 0); // m_AudioLanguage (empty)
        put_typetree_string_value(&mut payload, resource_path); // m_Source
        payload.extend_from_slice(&0u64.to_le_bytes()); // m_Offset
        payload.extend_from_slice(&u64::from(size).to_le_bytes()); // m_Size
        payload.push(0); // m_HasSplitAlpha
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        // The `UInt16`/`UInt32`/`string` data leaves are declared for completeness; no vector
        // has elements, so only the count words above appear.
        let _ = (uint16_type, uint32_type);
        finish_single_object_file(b"3.5.0f1\0", 329, &nodes, &strings, &payload, 42)
    }

    #[test]
    fn generated_video_clip_layout_decodes_under_production_layouts() {
        use crate::{load_object_workspace, load_workspace_from_vfs};
        use assetripper::io::dependency::DependencyFilePath;
        use assetripper::io::filesystem::VirtualFileSystem;
        use assetripper::serialized_file::object_workspace::ObjectWorkspaceLimits;
        use assetripper::serialized_file::workspace::SerializedFileWorkspaceLimits;

        let _production = crate::SourceFactoryLayoutGuard::use_source_factory();
        let bytes = make_video_clip_file("Assets/movie.mp4", "probe.resS", 30);
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem
            .write_all_bytes("/game/video.assets", &bytes)
            .unwrap();
        let input = DependencyFilePath {
            file_name: "video.assets".to_owned(),
            virtual_path: "/game/video.assets".to_owned(),
        };
        let workspace = load_workspace_from_vfs(
            &filesystem,
            &[input],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("the workspace loads");
        let objects =
            load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())
                .expect("the object workspace loads");
        let object = objects.get(0, 0).expect("the clip exists");
        if std::env::var("VIDEO_TREE_DEBUG").is_ok() {
            use assetripper::processing::asset_factory::{
                ClassLayoutSource, FactoryUnityVersion,
            };
            let layouts = assetripper::processing::asset_factory::reference_class_layouts();
            let version = FactoryUnityVersion::parse("3.5.0f1").expect("version");
            if let Ok(Some((release, _))) = layouts.tpk_trees(329, &version) {
                for node in &release.nodes {
                    eprintln!(
                        "VIDEOTREE|{}|{}|{}|{}",
                        node.level,
                        node.type_name.as_deref().unwrap_or(""),
                        node.name.as_deref().unwrap_or(""),
                        node.byte_size
                    );
                }
            }
        }
        let payload = object
            .payload
            .as_ref()
            .unwrap_or_else(|error| panic!("the clip must decode: {error}"));
        let view =
            assetripper::resources::video_clip::VideoClipView::from_value(&payload.root)
                .expect("the clip exposes route fields");
        assert_eq!(view.try_get_extension_from_path(), Some("mp4"));
        assert!(view.check_integrity());
        let info = view.external_resources.as_ref().expect("resources are present");
        assert_eq!(info.path, "probe.resS");
        assert_eq!(info.offset, 0);
        assert_eq!(info.size, 30);
    }

    /// Build a `Font` file for the font route and tab.
    ///
    /// The TypeTree and payload follow the **production** class-128 layout at 3.5.0f1, which the
    /// asset factory prefers over any file TypeTree. `m_FontData` sits after `m_Name`,
    /// `m_AsciiStartOffset`, `m_FontCountX/Y`, `m_Kerning`, `m_LineSpacing`, an empty
    /// `m_PerCharacterKerning`, `m_ConvertCase`, a null `m_DefaultMaterial`, an empty
    /// `m_CharacterRects`, a null `m_Texture`, an empty `m_KerningValues` and a `m_GridFont`
    /// bool - so a fixture that writes only `m_Name` and `m_FontData` decodes as unreadable.
    pub(super) fn make_font_file(data: &[u8]) -> Vec<u8> {
        let mut strings = Vec::new();
        let font_type = push_type_tree_string(&mut strings, "Font");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let sint32_type = push_type_tree_string(&mut strings, "SInt32");
        let int_type = push_type_tree_string(&mut strings, "int");
        let float_type = push_type_tree_string(&mut strings, "float");
        let bool_type = push_type_tree_string(&mut strings, "bool");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let vector_type = push_type_tree_string(&mut strings, "vector");
        let font_data_field = push_type_tree_string(&mut strings, "m_FontData");

        let ascii_field = push_type_tree_string(&mut strings, "m_AsciiStartOffset");
        let count_x_field = push_type_tree_string(&mut strings, "m_FontCountX");
        let count_y_field = push_type_tree_string(&mut strings, "m_FontCountY");
        let kerning_field = push_type_tree_string(&mut strings, "m_Kerning");
        let line_spacing_field = push_type_tree_string(&mut strings, "m_LineSpacing");
        let per_char_field = push_type_tree_string(&mut strings, "m_PerCharacterKerning");
        let convert_case_field = push_type_tree_string(&mut strings, "m_ConvertCase");
        let default_material_field = push_type_tree_string(&mut strings, "m_DefaultMaterial");
        let pptr_material_type = push_type_tree_string(&mut strings, "PPtr<Material>");
        let char_rects_field = push_type_tree_string(&mut strings, "m_CharacterRects");
        let texture_field = push_type_tree_string(&mut strings, "m_Texture");
        let pptr_texture_type = push_type_tree_string(&mut strings, "PPtr<Texture>");
        let kerning_values_field = push_type_tree_string(&mut strings, "m_KerningValues");
        let map_type = push_type_tree_string(&mut strings, "map");
        let grid_font_field = push_type_tree_string(&mut strings, "m_GridFont");
        let font_size_field = push_type_tree_string(&mut strings, "m_FontSize");
        let ascent_field = push_type_tree_string(&mut strings, "m_Ascent");
        let default_style_field = push_type_tree_string(&mut strings, "m_DefaultStyle");
        let font_names_field = push_type_tree_string(&mut strings, "m_FontNames");
        let unsigned_int_type = push_type_tree_string(&mut strings, "unsigned int");
        let file_id_field = push_type_tree_string(&mut strings, "m_FileID");
        let path_id_field = push_type_tree_string(&mut strings, "m_PathID");

        let mut nodes = Vec::new();
        nodes.push((0, font_type, empty_name, -1));
        let table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &table, name_field);
        nodes.push((1, int_type, ascii_field, 4));
        nodes.push((1, int_type, count_x_field, 4));
        nodes.push((1, int_type, count_y_field, 4));
        nodes.push((1, float_type, kerning_field, 4));
        nodes.push((1, float_type, line_spacing_field, 4));
        // An empty `vector` is a single `Array` child with no elements.
        nodes.push((1, vector_type, per_char_field, -1));
        nodes.push((2, array_type, empty_name, -1));
        nodes.push((3, sint32_type, size_field, 4));
        nodes.push((3, int_type, convert_case_field, 4));
        // Both PPtrs are two `SInt32` leaves.
        nodes.push((1, pptr_material_type, default_material_field, 8));
        nodes.push((2, sint32_type, file_id_field, 4));
        nodes.push((2, sint32_type, path_id_field, 4));
        nodes.push((1, vector_type, char_rects_field, -1));
        nodes.push((2, array_type, empty_name, -1));
        nodes.push((3, sint32_type, size_field, 4));
        nodes.push((1, pptr_texture_type, texture_field, 8));
        nodes.push((2, sint32_type, file_id_field, 4));
        nodes.push((2, sint32_type, path_id_field, 4));
        nodes.push((1, map_type, kerning_values_field, -1));
        nodes.push((2, array_type, empty_name, -1));
        nodes.push((3, sint32_type, size_field, 4));
        nodes.push((1, bool_type, grid_font_field, 1));
        // `m_FontData` is a `vector` whose `Array` has `char` data leaves.
        nodes.push((1, vector_type, font_data_field, -1));
        nodes.push((2, array_type, empty_name, -1));
        nodes.push((3, sint32_type, size_field, 4));
        nodes.push((3, char_type, data_field, 1));
        // The production record continues past `m_FontData`; the reader walks the whole tree,
        // so an absent tail reads as truncated input rather than an unused extra.
        nodes.push((1, float_type, font_size_field, 4));
        nodes.push((1, float_type, ascent_field, 4));
        nodes.push((1, unsigned_int_type, default_style_field, 4));
        nodes.push((1, vector_type, font_names_field, -1));
        nodes.push((2, array_type, empty_name, -1));
        nodes.push((3, sint32_type, size_field, 4));
        nodes.push((3, string_type, data_field, -1));
        nodes.push((4, array_type, empty_name, -1));
        nodes.push((5, sint32_type, size_field, 4));
        nodes.push((5, char_type, data_field, 1));

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestFont");
        put_i32_le(&mut payload, 0); // m_AsciiStartOffset
        put_i32_le(&mut payload, 0); // m_FontCountX
        put_i32_le(&mut payload, 0); // m_FontCountY
        put_f32_le(&mut payload, 0.0); // m_Kerning
        put_f32_le(&mut payload, 0.0); // m_LineSpacing
        put_i32_le(&mut payload, 0); // m_PerCharacterKerning (empty)
        put_i32_le(&mut payload, 0); // m_ConvertCase
        put_i32_le(&mut payload, 0); // m_DefaultMaterial.m_FileID
        put_i32_le(&mut payload, 0); // m_DefaultMaterial.m_PathID
        put_i32_le(&mut payload, 0); // m_CharacterRects (empty)
        put_i32_le(&mut payload, 0); // m_Texture.m_FileID
        put_i32_le(&mut payload, 0); // m_Texture.m_PathID
        put_i32_le(&mut payload, 0); // m_KerningValues (empty)
        payload.push(0); // m_GridFont
        // The reader aligns the following `int` after the bool run.
        while !payload.len().is_multiple_of(4) {
            payload.push(0);
        }
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap()); // m_FontData count
        payload.extend_from_slice(data);
        // The production record continues past `m_FontData`.
        put_f32_le(&mut payload, 0.0); // m_FontSize
        put_f32_le(&mut payload, 0.0); // m_Ascent
        put_i32_le(&mut payload, 0); // m_DefaultStyle
        put_i32_le(&mut payload, 0); // m_FontNames (empty)
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        finish_single_object_file(b"3.5.0f1\0", 128, &nodes, &strings, &payload, 42)
    }

    #[test]
    fn generated_font_layout_decodes_under_production_layouts() {
        use crate::{load_object_workspace, load_workspace_from_vfs};
        use assetripper::io::dependency::DependencyFilePath;
        use assetripper::io::filesystem::VirtualFileSystem;
        use assetripper::serialized_file::object_workspace::ObjectWorkspaceLimits;
        use assetripper::serialized_file::workspace::SerializedFileWorkspaceLimits;

        let _production = crate::SourceFactoryLayoutGuard::use_source_factory();
        let bytes = make_font_file(b"OTTOfont-bytes");
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem.write_all_bytes("/game/font.assets", &bytes).unwrap();
        let input = DependencyFilePath {
            file_name: "font.assets".to_owned(),
            virtual_path: "/game/font.assets".to_owned(),
        };
        let workspace = load_workspace_from_vfs(
            &filesystem,
            &[input],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("the workspace loads");
        let objects =
            load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())
                .expect("the object workspace loads");
        let object = objects.get(0, 0).expect("the font exists");
        let payload = object
            .payload
            .as_ref()
            .unwrap_or_else(|error| panic!("the font must decode: {error}"));
        let data = assetripper::export::resource_extensions::font_data_bytes(&payload.root)
            .expect("the font data reads")
            .expect("m_FontData is present");
        assert_eq!(data, b"OTTOfont-bytes");
        assert_eq!(
            assetripper::export::resource_extensions::font_extension_and_mime(&data),
            Some(("otf", "font/otf"))
        );
    }

    /// Build an `AudioClip` file whose TypeTree and payload match the **production** class-83
    /// layout at 3.5.0f1, which the asset factory prefers over a file TypeTree.
    ///
    /// The older hand-written builders above place `m_AudioData` directly after `m_Name`, which
    /// suits the synthetic test layout source but not production: the reference class-83 record
    /// has `m_Format`, `m_Type`, `m_3D`, `m_UseHardware` and `m_Stream` before `m_AudioData`, so
    /// a fixture written for the short shape decodes with every field misaligned. This builder
    /// mirrors the production node list in order.
    pub(super) fn make_generated_audio_clip_file(data: &[u8]) -> Vec<u8> {
        let mut strings = Vec::new();
        let clip_type = push_type_tree_string(&mut strings, "AudioClip");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let sint32_type = push_type_tree_string(&mut strings, "SInt32");
        let size_field = push_type_tree_string(&mut strings, "size");
        let char_type = push_type_tree_string(&mut strings, "char");
        let data_field = push_type_tree_string(&mut strings, "data");
        let format_field = push_type_tree_string(&mut strings, "m_Format");
        let type_field = push_type_tree_string(&mut strings, "m_Type");
        let three_d_field = push_type_tree_string(&mut strings, "m_3D");
        let bool_type = push_type_tree_string(&mut strings, "bool");
        let use_hardware_field = push_type_tree_string(&mut strings, "m_UseHardware");
        let int_type = push_type_tree_string(&mut strings, "int");
        let stream_field = push_type_tree_string(&mut strings, "m_Stream");
        let vector_type = push_type_tree_string(&mut strings, "vector");
        let audio_data_field = push_type_tree_string(&mut strings, "m_AudioData");
        let uint8_type = push_type_tree_string(&mut strings, "UInt8");

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
        nodes.push((1, sint32_type, format_field, 4));
        nodes.push((1, sint32_type, type_field, 4));
        nodes.push((1, bool_type, three_d_field, 1));
        nodes.push((1, bool_type, use_hardware_field, 1));
        nodes.push((1, int_type, stream_field, 4));
        nodes.push((1, vector_type, audio_data_field, -1));
        nodes.push((2, array_type, empty_name, -1));
        nodes.push((3, sint32_type, size_field, 4));
        nodes.push((3, uint8_type, data_field, 1));

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestClip");
        put_i32_le(&mut payload, 0); // m_Format
        // `m_Type` is an FMOD sound type; the decoder maps it to a raw extension. A value is
        // present in the reference layout, so it must be written here too.
        put_i32_le(&mut payload, 1); // m_Type
        payload.push(0); // m_3D
        payload.push(0); // m_UseHardware
        // The reader aligns an `int` after the bool run, so `m_Stream` starts on a word.
        while !payload.len().is_multiple_of(4) {
            payload.push(0);
        }
        put_i32_le(&mut payload, 0); // m_Stream
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap()); // m_AudioData count
        payload.extend_from_slice(data);
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        finish_single_object_file(b"3.5.0f1\0", 83, &nodes, &strings, &payload, 42)
    }

    #[test]
    fn generated_audio_clip_layout_decodes_under_production_layouts() {
        use crate::{load_object_workspace, load_workspace_from_vfs};
        use assetripper::io::dependency::DependencyFilePath;
        use assetripper::io::filesystem::VirtualFileSystem;
        use assetripper::serialized_file::object_workspace::ObjectWorkspaceLimits;
        use assetripper::serialized_file::workspace::SerializedFileWorkspaceLimits;

        let _production = crate::SourceFactoryLayoutGuard::use_source_factory();
        let bytes = make_generated_audio_clip_file(b"OggS-fake-payload");
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem
            .write_all_bytes("/game/audio.assets", &bytes)
            .unwrap();
        let input = DependencyFilePath {
            file_name: "audio.assets".to_owned(),
            virtual_path: "/game/audio.assets".to_owned(),
        };
        let workspace = load_workspace_from_vfs(
            &filesystem,
            &[input],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("the workspace loads");
        let objects = load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .expect("the object workspace loads");
        let object = objects.get(0, 0).expect("the clip exists");
        let payload = object
            .payload
            .as_ref()
            .unwrap_or_else(|error| panic!("the clip must decode: {error}"));
        let view = assetripper::resources::audio::AudioClipView::from_value(&payload.root)
            .expect("the clip exposes audio fields");
        assert_eq!(
            view.audio_data.as_deref(),
            Some(b"OggS-fake-payload".as_slice())
        );
        assert_eq!(view.sound_type, Some(1));
    }

    fn make_streamed_audio_clip_file(        stream_path: &str,
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
