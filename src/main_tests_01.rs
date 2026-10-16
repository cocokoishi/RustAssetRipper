    use std::io;

    use super::{
        load_object_workspace, load_workspace_from_vfs, merge_mount_report, remaining_budget_u64,
        remaining_budget_usize, source_bundle_for_collection, ManagedAssembly,
        ManagedAssemblyLimits, WorkspaceSerializedBundle,
    };
    use assetripper::export::settings::{ExportDefaults, ScriptExportMode, ShaderExportMode};
    use assetripper::export::settings_file::ExportSettingsFile;
    use assetripper::io::dependency::DependencyFilePath;
    use assetripper::io::filesystem::VirtualFileSystem;
    use assetripper::io::platform_file_inventory::PlatformWorkspaceLoadOptions;
    use assetripper::io::platform_structure::{
        detect_platform_structure, PlatformKind, PlatformStructure, PlatformStructureLimits,
    };
    use assetripper::scheme::BatchMountReport;
    use assetripper::scheme::FileKind;
    use assetripper::serialized_file::collection_workspace::{
        WorkspaceAssetCollections, WorkspaceCollectionLimits,
    };
    use assetripper::serialized_file::object_workspace::ObjectWorkspaceLimits;
    use assetripper::serialized_file::workspace::{
        SerializedFileDependencyOutcome, SerializedFileWorkspaceLimits,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    /// Normalize only text line endings in frozen source exports to the host form.
    fn normalize_text_line_endings(bytes: &[u8]) -> Vec<u8> {
        let mut normalized = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            normalized.push(bytes[index]);
            index += 1;
        }
        normalized
    }

    /// The built-in `ExportSettings` record, used by project-export tests without a settings file.
    fn default_export_settings() -> ExportSettingsFile {
        ExportSettingsFile::from(ExportDefaults::default())
    }

    /// Decode one source-authored SerializedFile fixture for factory/project integration tests.
    fn source_factory_fixture_bytes(case_name: &str) -> Vec<u8> {
        let prefix = format!("SF_HEX|{case_name}|");
        let hex = assetripper::processing::asset_factory::PROBE_FIXTURE
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("source-factory fixture includes {case_name}"));
        assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    /// The `guid:` line of one importer sidecar.
    fn meta_guid(path: &std::path::Path) -> String {
        fs::read_to_string(path)
            .expect("sidecar is readable")
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("sidecar has a GUID")
            .to_owned()
    }

    /// Every file below one directory as portable, sorted project-relative paths.
    fn project_tree(root: &std::path::Path) -> Vec<String> {
        fn walk(root: &std::path::Path, current: &std::path::Path, output: &mut Vec<String>) {
            for entry in fs::read_dir(current).expect("output directory is readable") {
                let entry = entry.expect("directory entry is readable");
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, output);
                } else {
                    let relative = path
                        .strip_prefix(root)
                        .expect("output path is below the export root");
                    output.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut output = Vec::new();
        walk(root, root, &mut output);
        output.sort();
        output
    }

    /// Every output directory below one root, parent before child for safe reverse cleanup.
    fn project_directories(root: &std::path::Path) -> Vec<PathBuf> {
        fn walk(current: &std::path::Path, output: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(current).expect("output directory is readable") {
                let entry = entry.expect("directory entry is readable");
                let path = entry.path();
                if path.is_dir() {
                    output.push(path.clone());
                    walk(&path, output);
                }
            }
        }
        let mut output = vec![root.to_path_buf()];
        walk(root, &mut output);
        output
    }

    /// A class-129 `PlayerSettings` object: the `TypeTreeObject { IsPlayerSettings }` manager path.
    fn make_player_settings_file() -> Vec<u8> {
        let mut strings = Vec::new();
        let root_type = push_type_tree_string(&mut strings, "PlayerSettings");
        let empty_name = push_type_tree_string(&mut strings, "");
        let int_type = push_type_tree_string(&mut strings, "int");
        let value_name = push_type_tree_string(&mut strings, "m_Value");
        let nodes = [
            (0u8, root_type, empty_name, -1i32),
            (1, int_type, value_name, 4),
        ];
        let mut payload = Vec::new();
        put_i32_le(&mut payload, 7);
        finish_single_object_file(b"2019.4.0f1\0", 129, &nodes, &strings, &payload, 42)
    }

    #[test]
    fn streamed_scene_bundle_lookup_uses_direct_collections_without_a_scope_path() {
        let bundles = [WorkspaceSerializedBundle {
            scope_path: None,
            name_fixed: None,
            parent_bundle_index: None,
            parent_entry_index: None,
            direct_collection_file_indices: vec![3, 1],
            direct_resource_file_indices: Vec::new(),
        }];

        let (bundle_index, bundle) = source_bundle_for_collection(Some(0), &bundles).unwrap();
        assert_eq!(bundle_index, 0);
        assert_eq!(bundle.scope_path, None);
        assert_eq!(bundle.direct_collection_file_indices, [3, 1]);

        let error = source_bundle_for_collection(None, &bundles).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    struct TestInputDirectory {
        directory: PathBuf,
        files: Vec<PathBuf>,
        subdirectories: Vec<PathBuf>,
    }

    impl TestInputDirectory {
        fn new_root() -> PathBuf {
            loop {
                let index = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let directory = std::env::current_dir()
                    .unwrap()
                    .join("target")
                    .join(format!("cli-input-{}-{index}", std::process::id()));
                match fs::create_dir(&directory) {
                    Ok(()) => return directory,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        panic!(
                            "failed to create test input root {}: {error}",
                            directory.display()
                        )
                    }
                }
            }
        }

        fn new() -> Self {
            let directory = Self::new_root();
            let file = directory.join("main.assets");
            fs::write(&file, make_generation_10_file()).unwrap();
            Self {
                directory,
                files: vec![file],
                subdirectories: Vec::new(),
            }
        }

        fn new_windows_platform() -> Self {
            let directory = Self::new_root();
            let game_name = directory.file_name().unwrap().to_str().unwrap();
            let executable = directory.join(format!("{game_name}.exe"));
            let data_directory = directory.join(format!("{game_name}_Data"));
            fs::create_dir(&data_directory).unwrap();
            fs::write(&executable, b"synthetic executable marker").unwrap();

            let primary = data_directory.join("globalgamemanagers");
            fs::write(
                &primary,
                make_generation_10_file_with_externals(&["shared.assets", "data.unity3d"]),
            )
            .unwrap();
            let container_path = data_directory.join("data.unity3d");
            fs::write(
                &container_path,
                make_webdata_fixture(&[("shared.assets", &make_generation_10_file())]),
            )
            .unwrap();

            Self {
                directory,
                files: vec![executable, primary, container_path],
                subdirectories: vec![data_directory],
            }
        }
    }

    impl Drop for TestInputDirectory {
        fn drop(&mut self) {
            for file in &self.files {
                let _ = fs::remove_file(file);
            }
            for directory in self.subdirectories.iter().rev() {
                let _ = fs::remove_dir(directory);
            }
            let _ = fs::remove_dir(&self.directory);
        }
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

    fn put_u32_be(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn put_i32_le_as_be(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn put_f32_le(output: &mut Vec<u8>, value: f32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    pub(super) fn put_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        put_node_with_meta(output, level, type_offset, name_offset, byte_size, index, 0);
    }

    fn put_node_with_meta(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
        meta_flags: u32,
    ) {
        put_node_with_version_and_meta(
            output,
            1,
            level,
            type_offset,
            name_offset,
            byte_size,
            index,
            meta_flags,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn put_node_with_version_and_meta(
        output: &mut Vec<u8>,
        version: u16,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
        meta_flags: u32,
    ) {
        put_u16_le(output, version);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, meta_flags);
    }

    fn insert_vfs_file(filesystem: &mut VirtualFileSystem, path: &str, bytes: &[u8]) {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, bytes).unwrap();
    }

    fn push_asset_bundle_node(
        strings: &mut Vec<u8>,
        nodes: &mut Vec<(u8, u32, u32, i32, i32)>,
        level: u8,
        type_name: &str,
        field_name: &str,
        byte_size: i32,
    ) {
        let type_offset = push_type_tree_string(strings, type_name);
        let name_offset = push_type_tree_string(strings, field_name);
        let index = i32::try_from(nodes.len()).unwrap();
        nodes.push((level, type_offset, name_offset, byte_size, index));
    }

    fn put_asset_bundle_string_payload(output: &mut Vec<u8>, value: &str) {
        let bytes = value.as_bytes();
        put_i32_le(output, i32::try_from(bytes.len()).unwrap());
        output.extend_from_slice(bytes);
        let padding = (4 - output.len() % 4) % 4;
        output.resize(output.len() + padding, 0);
    }

    fn make_asset_bundle_file() -> Vec<u8> {
        make_asset_bundle_file_fixture(false)
    }

    fn make_asset_bundle_file_with_local_reference() -> Vec<u8> {
        make_asset_bundle_file_fixture(true)
    }

    fn make_asset_bundle_file_fixture(include_local_target: bool) -> Vec<u8> {
        let mut strings = Vec::new();
        let mut nodes = Vec::new();
        push_asset_bundle_node(&mut strings, &mut nodes, 0, "AssetBundle", "", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 1, "map", "m_Container", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 2, "Array", "Array", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 3, "int", "size", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 3, "pair", "data", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 4, "string", "first", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 5, "Array", "Array", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 6, "int", "size", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 6, "char", "data", 1);
        push_asset_bundle_node(&mut strings, &mut nodes, 4, "AssetInfo", "second", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 5, "int", "preloadIndex", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 5, "int", "preloadSize", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 5, "PPtr<Object>", "asset", 12);
        push_asset_bundle_node(&mut strings, &mut nodes, 6, "int", "m_FileID", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 6, "SInt64", "m_PathID", 8);
        push_asset_bundle_node(
            &mut strings,
            &mut nodes,
            1,
            "bool",
            "m_IsStreamedSceneAssetBundle",
            1,
        );
        push_asset_bundle_node(&mut strings, &mut nodes, 1, "map", "m_SceneHashes", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 2, "Array", "Array", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 3, "int", "size", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 3, "pair", "data", -1);
        for member in ["first", "second"] {
            push_asset_bundle_node(&mut strings, &mut nodes, 4, "string", member, -1);
            push_asset_bundle_node(&mut strings, &mut nodes, 5, "Array", "Array", -1);
            push_asset_bundle_node(&mut strings, &mut nodes, 6, "int", "size", 4);
            push_asset_bundle_node(&mut strings, &mut nodes, 6, "char", "data", 1);
        }

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 142);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in nodes {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, if include_local_target { 2 } else { 1 });
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, 1);
        put_asset_bundle_string_payload(&mut payload, "Assets/Scenes/Other.unity");
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        let container_path_id = if include_local_target { 43_i64 } else { 0 };
        payload.extend_from_slice(&container_path_id.to_le_bytes());
        payload.push(1);
        put_i32_le(&mut payload, 1);
        put_asset_bundle_string_payload(&mut payload, "Assets/Scenes/BundleScene.unity");
        put_asset_bundle_string_payload(&mut payload, "CAB-Scene");

        let mut target_payload = Vec::new();
        if include_local_target {
            put_i32_le(&mut target_payload, 0);
            target_payload.push(0);
            put_i32_le(&mut target_payload, 0);
        }

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 142);
        put_u16_le(&mut metadata, 142);
        put_u16_le(&mut metadata, 0);
        if include_local_target {
            put_i32_le(&mut metadata, 43);
            put_u32_le(&mut metadata, u32::try_from(payload.len()).unwrap());
            put_i32_le(&mut metadata, i32::try_from(target_payload.len()).unwrap());
            put_i32_le(&mut metadata, 142);
            put_u16_le(&mut metadata, 142);
            put_u16_le(&mut metadata, 0);
        }
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let file_size = data_offset + payload.len() + target_payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file.extend_from_slice(&target_payload);
        file
    }

    fn make_resource_manager_file_with_local_reference() -> Vec<u8> {
        let mut manager_strings = Vec::new();
        let mut manager_nodes = Vec::new();
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            0,
            "ResourceManager",
            "",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            1,
            "map",
            "m_Container",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            2,
            "Array",
            "Array",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            3,
            "int",
            "size",
            4,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            3,
            "pair",
            "data",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            4,
            "string",
            "first",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            5,
            "Array",
            "Array",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            6,
            "int",
            "size",
            4,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            6,
            "char",
            "data",
            1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            4,
            "PPtr<Object>",
            "second",
            12,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            5,
            "int",
            "m_FileID",
            4,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            5,
            "SInt64",
            "m_PathID",
            8,
        );

        let mut target_strings = Vec::new();
        let mut target_nodes = Vec::new();
        push_asset_bundle_node(&mut target_strings, &mut target_nodes, 0, "Shader", "", -1);
        push_asset_bundle_node(
            &mut target_strings,
            &mut target_nodes,
            1,
            "string",
            "m_Name",
            -1,
        );
        push_asset_bundle_node(
            &mut target_strings,
            &mut target_nodes,
            2,
            "Array",
            "Array",
            -1,
        );
        push_asset_bundle_node(&mut target_strings, &mut target_nodes, 3, "int", "size", 4);
        push_asset_bundle_node(&mut target_strings, &mut target_nodes, 3, "char", "data", 1);

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 2);
        for (class_id, strings, nodes) in [
            (147, manager_strings, manager_nodes),
            (48, target_strings, target_nodes),
        ] {
            put_i32_le(&mut metadata, class_id);
            put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
            put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
            for (level, type_offset, name_offset, byte_size, index) in nodes {
                put_node(
                    &mut metadata,
                    level,
                    type_offset,
                    name_offset,
                    byte_size,
                    index,
                );
            }
            metadata.extend_from_slice(&strings);
        }

        let mut manager_payload = Vec::new();
        put_i32_le(&mut manager_payload, 2);
        for path in ["Textures/inner/wood.shader", "wood.shader"] {
            put_asset_bundle_string_payload(&mut manager_payload, path);
            put_i32_le(&mut manager_payload, 0);
            manager_payload.extend_from_slice(&43_i64.to_le_bytes());
        }
        let mut target_payload = Vec::new();
        put_asset_bundle_string_payload(&mut target_payload, "Wood");

        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 2);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(manager_payload.len()).unwrap());
        put_i32_le(&mut metadata, 147);
        put_u16_le(&mut metadata, 147);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 43);
        put_u32_le(&mut metadata, u32::try_from(manager_payload.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(target_payload.len()).unwrap());
        put_i32_le(&mut metadata, 48);
        put_u16_le(&mut metadata, 48);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let file_size = data_offset + manager_payload.len() + target_payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&manager_payload);
        file.extend_from_slice(&target_payload);
        file
    }

    fn make_generation_10_file() -> Vec<u8> {
        make_generation_10_file_with_externals(&[])
    }

    fn make_mono_behaviour_with_game_object_pointer(path_id: i64) -> Vec<u8> {
        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2));
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        let hex = assetripper::processing::asset_factory::PROBE_FIXTURE
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|mono_with_structure|"))
            .expect("the source-factory fixture includes a typed MonoBehaviour");
        let bytes = decode_hex(hex);
        let parsed = assetripper::serialized_file::file::SerializedFile::parse(&bytes).unwrap();
        let mut payload = parsed.decode_object_payload(0).unwrap();
        let game_object = payload
            .root
            .unique_field_in_base_chain_mut("m_GameObject")
            .unwrap()
            .expect("the MonoBehaviour TypeTree contains m_GameObject");
        *game_object = assetripper::serialized_file::object::SerializedValue::PPtr {
            file_id: 0,
            path_id,
        };
        let script = payload
            .root
            .unique_field_in_base_chain_mut("m_Script")
            .unwrap()
            .expect("the MonoBehaviour TypeTree contains m_Script");
        *script = assetripper::serialized_file::object::SerializedValue::PPtr {
            file_id: 0,
            path_id: 0,
        };
        parsed
            .rewrite_object_payloads(
                &[(0, payload)],
                assetripper::serialized_file::file::SerializedFileRewriteLimits::default(),
            )
            .unwrap()
    }

    fn make_generation_10_file_with_externals(external_names: &[&str]) -> Vec<u8> {
        make_generation_10_file_with_pointer_path_id(external_names, 42)
    }

    fn make_generation_10_file_with_pointer_path_id(
        external_names: &[&str],
        pointer_path_id: i32,
    ) -> Vec<u8> {
        make_generation_10_file_with_pointer(external_names, 0, pointer_path_id)
    }

    fn make_generation_10_file_with_pointer(
        external_names: &[&str],
        pointer_file_id: i32,
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
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        put_node(&mut metadata, 0, root_type, empty_name, -1, 0);
        put_node(&mut metadata, 1, int_type, value_name, 4, 1);
        put_node(&mut metadata, 1, pptr_type, target_name, 8, 2);
        put_node(&mut metadata, 2, int32_type, file_id_name, 4, 3);
        put_node(&mut metadata, 2, int32_type, path_id_name, 4, 4);
        metadata.extend_from_slice(&string_buffer);

        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 12);
        put_i32_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
        for external_name in external_names {
            metadata.push(0);
            for word in [0, 0, 0, 0] {
                put_u32_le(&mut metadata, word);
            }
            put_i32_le(&mut metadata, 0);
            metadata.extend_from_slice(external_name.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let mut payload = Vec::new();
        put_i32_le(&mut payload, 0x1234_5678);
        put_i32_le(&mut payload, pointer_file_id);
        put_i32_le(&mut payload, pointer_path_id);
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_streaming_info_file(path: &str, offset: u64, size: u32) -> Vec<u8> {
        make_streaming_info_file_with_offset_width(path, offset, size, true)
    }

    fn make_streaming_info_file_with_offset_width(
        path: &str,
        offset: u64,
        size: u32,
        offset_is_u64: bool,
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, "Texture2D");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let streaming_info_type = push_type_tree_string(&mut string_buffer, "StreamingInfo");
        let stream_data_name = push_type_tree_string(&mut string_buffer, "m_StreamData");
        let string_type = push_type_tree_string(&mut string_buffer, "string");
        let path_name = push_type_tree_string(&mut string_buffer, "path");
        let array_type = push_type_tree_string(&mut string_buffer, "Array");
        let size_name = push_type_tree_string(&mut string_buffer, "size");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let data_name = push_type_tree_string(&mut string_buffer, "data");
        let char_type = push_type_tree_string(&mut string_buffer, "char");
        let offset_type_name = if offset_is_u64 { "UInt64" } else { "UInt32" };
        let offset_type = push_type_tree_string(&mut string_buffer, offset_type_name);
        let offset_name = push_type_tree_string(&mut string_buffer, "offset");
        let uint32_type = push_type_tree_string(&mut string_buffer, "UInt32");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 28);
        put_i32_le(&mut metadata, 8);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty_name, -1, 0),
            (1, streaming_info_type, stream_data_name, -1, 1),
            (2, string_type, path_name, -1, 2),
            (3, array_type, array_type, -1, 3),
            (4, int_type, size_name, 4, 4),
            (4, char_type, data_name, 1, 5),
            (
                2,
                offset_type,
                offset_name,
                if offset_is_u64 { 8 } else { 4 },
                6,
            ),
            (2, uint32_type, size_name, 4, 7),
        ] {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, i32::try_from(path.len()).unwrap());
        payload.extend_from_slice(path.as_bytes());
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        if offset_is_u64 {
            payload.extend_from_slice(&offset.to_le_bytes());
        } else {
            put_u32_le(&mut payload, u32::try_from(offset).unwrap());
        }
        put_u32_le(&mut payload, size);

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 28);
        put_u16_le(&mut metadata, 28);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_streamed_image_texture_file() -> Vec<u8> {
        make_streamed_image_texture_file_with_stream("texture.resS", 3)
    }

    fn make_streamed_image_texture_file_with_stream(
        stream_path: &str,
        stream_size: u32,
    ) -> Vec<u8> {
        make_streamed_image_texture_file_with_streams(&[(stream_path, 2, stream_size)])
    }

    fn make_streamed_image_texture_file_with_streams(streams: &[(&str, u64, u32)]) -> Vec<u8> {
        make_streamed_texture_file_for_class(
            189,
            "ImageTexture",
            "m_StreamData_C189",
            "m_ImageData_C189",
            "2019.4.0f1",
            true,
            streams,
        )
    }

    fn make_streamed_texture2d_file(stream_path: &str, offset: u64, size: u32) -> Vec<u8> {
        make_streamed_texture_file_for_class(
            28,
            "Texture2D",
            "m_StreamData",
            "image data",
            "2022.3.22f1",
            true,
            &[(stream_path, offset, size)],
        )
    }

    fn make_streamed_texture_file_for_class(
        class_id: i32,
        root_type_name: &str,
        stream_field_name: &str,
        inline_field_name: &str,
        unity_version: &str,
        offset_is_u64: bool,
        streams: &[(&str, u64, u32)],
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, root_type_name);
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let streaming_info_type = push_type_tree_string(&mut string_buffer, "StreamingInfo");
        let stream_data_name = push_type_tree_string(&mut string_buffer, stream_field_name);
        let string_type = push_type_tree_string(&mut string_buffer, "string");
        let path_name = push_type_tree_string(&mut string_buffer, "path");
        let array_type = push_type_tree_string(&mut string_buffer, "Array");
        let size_name = push_type_tree_string(&mut string_buffer, "size");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let data_name = push_type_tree_string(&mut string_buffer, "data");
        let char_type = push_type_tree_string(&mut string_buffer, "char");
        let offset_type_name = if offset_is_u64 { "UInt64" } else { "UInt32" };
        let offset_type = push_type_tree_string(&mut string_buffer, offset_type_name);
        let offset_name = push_type_tree_string(&mut string_buffer, "offset");
        let uint32_type = push_type_tree_string(&mut string_buffer, "UInt32");
        let typeless_data_type = push_type_tree_string(&mut string_buffer, "TypelessData");
        let inline_data_name = push_type_tree_string(&mut string_buffer, inline_field_name);
        let uint8_type = push_type_tree_string(&mut string_buffer, "UInt8");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(unity_version.as_bytes());
        metadata.push(0);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, class_id);
        put_i32_le(&mut metadata, 11);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty_name, -1, 0),
            (1, streaming_info_type, stream_data_name, -1, 1),
            (2, string_type, path_name, -1, 2),
            (3, array_type, array_type, -1, 3),
            (4, int_type, size_name, 4, 4),
            (4, char_type, data_name, 1, 5),
            (
                2,
                offset_type,
                offset_name,
                if offset_is_u64 { 8 } else { 4 },
                6,
            ),
            (2, uint32_type, size_name, 4, 7),
            (1, typeless_data_type, inline_data_name, -1, 8),
            (2, int_type, size_name, 4, 9),
            (2, uint8_type, data_name, 1, 10),
        ] {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(streams.len()).unwrap());

        let mut payload = Vec::new();
        for (object_index, (stream_path, stream_offset, stream_size)) in streams.iter().enumerate()
        {
            let padding = (4 - payload.len() % 4) % 4;
            payload.resize(payload.len() + padding, 0);
            let byte_start = payload.len();
            let mut object_payload = Vec::new();
            put_i32_le(
                &mut object_payload,
                i32::try_from(stream_path.len()).unwrap(),
            );
            object_payload.extend_from_slice(stream_path.as_bytes());
            let padding = (4 - object_payload.len() % 4) % 4;
            object_payload.resize(object_payload.len() + padding, 0);
            if offset_is_u64 {
                object_payload.extend_from_slice(&stream_offset.to_le_bytes());
            } else {
                put_u32_le(
                    &mut object_payload,
                    u32::try_from(*stream_offset).expect("fixture offset fits UInt32"),
                );
            }
            put_u32_le(&mut object_payload, *stream_size);
            put_i32_le(&mut object_payload, 0);

            put_i32_le(&mut metadata, 77 + i32::try_from(object_index).unwrap());
            put_i32_le(&mut metadata, i32::try_from(byte_start).unwrap());
            put_i32_le(&mut metadata, i32::try_from(object_payload.len()).unwrap());
            put_i32_le(&mut metadata, class_id);
            put_u16_le(&mut metadata, class_id as u16);
            put_u16_le(&mut metadata, 0);
            payload.extend_from_slice(&object_payload);
        }
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_texture2d_file() -> Vec<u8> {
        make_texture_file(28, "Texture2D", 2, 1, &[10, 20, 30, 255, 40, 50, 60, 255])
    }

    /// The committed reference dump of the class-28 release TypeTree.
    pub(super) const TEXTURE_TYPETREE_FIXTURE: &str =
        include_str!("resources/texture_typetree_fixture.txt");

    /// The reference-dumped release TypeTree decodes a texture through the file-TypeTree
    /// route, which the hand-authored 10-node sketch cannot (it omits `m_IsReadable`).
    #[test]
    fn reference_texture_typetree_decodes_through_the_file_route() {
        let bytes = make_reference_texture_file(2, 1, &[10, 20, 30, 255, 40, 50, 60, 255]);
        let file = assetripper::serialized_file::file::SerializedFile::parse(&bytes)
            .expect("the reference texture file parses");
        assert_eq!(file.metadata.objects.len(), 1);
        let type_index = file.metadata.objects[0]
            .resolved_type_index
            .expect("the object resolves its serialized type");
        let tree = file.metadata.types[type_index]
            .type_tree
            .as_ref()
            .expect("the fixture carries the file TypeTree");
        assert_eq!(tree.nodes.len(), 46);

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem
            .write_all_bytes("/game/z-texture.assets", &bytes)
            .unwrap();
        let input_path = DependencyFilePath {
            file_name: "z-texture.assets".to_owned(),
            virtual_path: "/game/z-texture.assets".to_owned(),
        };
        let workspace = load_workspace_from_vfs(
            &filesystem,
            &[input_path],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("the workspace loads");
        let objects =
            load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())
                .expect("the object workspace loads");
        let object = objects.get(0, 0).expect("the texture object exists");
        let payload = object
            .payload
            .as_ref()
            .unwrap_or_else(|error| panic!("the texture must decode, not fall back: {error}"));
        let view = assetripper::resources::texture::Texture2DView::from_value(&payload.root)
            .expect("the decoded payload exposes texture fields");
        assert_eq!((view.width, view.height), (2, 1));
        assert_eq!(view.texture_format, 4);
        assert_eq!(view.image_data, [10, 20, 30, 255, 40, 50, 60, 255]);
    }

    /// Build a synthetic Texture2D SerializedFile whose TypeTree is the real release tree.
    ///
    /// The hand-authored sketch used by the export tests suffices for the class-fallback
    /// route, but the file-TypeTree route (which the web load takes) reads every node
    /// including `m_IsReadable`, so a fixture omitting it decodes as `UnreadableObject`.
    /// This emits the committed 46-node dump verbatim and writes the payload in node order.
    pub(super) fn make_reference_texture_file(width: i32, height: i32, data: &[u8]) -> Vec<u8> {
        struct DumpedNode {
            level: u8,
            type_text: String,
            name_text: String,
            byte_size: i32,
        }

        let mut nodes: Vec<DumpedNode> = Vec::new();
        for line in TEXTURE_TYPETREE_FIXTURE.lines() {
            // The header comments describe this same format, so skip them.
            if line.starts_with('#') {
                continue;
            }
            let Some(rest) = line.strip_prefix("NODE|") else {
                continue;
            };
            let fields: Vec<&str> = rest.split('|').collect();
            // After the `NODE|` prefix: [0]=version, [1]=level, [2]=type, [3]=name,
            // [4]=byteSize.
            nodes.push(DumpedNode {
                level: fields[1].parse().expect("node level"),
                type_text: fields[2].to_owned(),
                name_text: fields[3].to_owned(),
                byte_size: fields[4].parse().expect("byte size"),
            });
        }
        assert_eq!(nodes.len(), 46, "the texture TypeTree fixture has 46 nodes");
        assert_eq!(nodes[0].type_text, "Texture2D");
        assert_eq!(nodes[0].level, 0);

        let mut string_buffer = Vec::new();
        let mut offsets = std::collections::HashMap::new();
        let mut intern = |text: &str, buffer: &mut Vec<u8>| -> u32 {
            if let Some(offset) = offsets.get(text) {
                return *offset;
            }
            let offset = push_type_tree_string(buffer, text);
            offsets.insert(text.to_owned(), offset);
            offset
        };

        let mut node_records = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            let type_offset = intern(&node.type_text, &mut string_buffer);
            let name_offset = intern(&node.name_text, &mut string_buffer);
            node_records.push((
                node.level,
                type_offset,
                name_offset,
                node.byte_size,
                i32::try_from(index).unwrap(),
            ));
        }

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.40f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 28);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in node_records {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        // Payload in node order, matching the reader's own alignment rules: strings are an
        // `i32` length then bytes then align-to-4, and `vector`/`TypelessData` arrays are an
        // aligned `i32` length then the element bytes, aligned again.
        let mut payload = Vec::new();
        put_i32_le(&mut payload, 0); // m_Name (string: length 0)
        payload.push(0); // m_IsAlphaChannelOptional
        put_i32_le(&mut payload, width); // m_Width
        put_i32_le(&mut payload, height); // m_Height
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap()); // m_CompleteImageSize
        put_i32_le(&mut payload, 0); // m_MipsStripped
        put_i32_le(&mut payload, 4); // m_TextureFormat (RGBA32)
        put_i32_le(&mut payload, 1); // m_MipCount
        payload.push(1); // m_IsReadable
        payload.push(0); // m_IsPreProcessed
        payload.push(0); // m_IgnoreMipmapLimit
        put_i32_le(&mut payload, 0); // m_MipmapLimitGroupName (string: length 0)
        payload.push(0); // m_StreamingMipmaps
        put_i32_le(&mut payload, 0); // m_StreamingMipmapsPriority
        put_i32_le(&mut payload, 1); // m_ImageCount
        put_i32_le(&mut payload, 2); // m_TextureDimension (2D)
        put_i32_le(&mut payload, 1); // m_TextureSettings.m_FilterMode
        put_i32_le(&mut payload, 1); // m_TextureSettings.m_Aniso
        put_f32_le(&mut payload, 0.0); // m_TextureSettings.m_MipBias
        put_i32_le(&mut payload, 1); // m_TextureSettings.m_WrapU
        put_i32_le(&mut payload, 1); // m_TextureSettings.m_WrapV
        put_i32_le(&mut payload, 1); // m_TextureSettings.m_WrapW
        put_i32_le(&mut payload, 1); // m_LightmapFormat
        put_i32_le(&mut payload, 1); // m_ColorSpace
        put_i32_le(&mut payload, 0); // m_PlatformBlob (empty vector)
        while payload.len() % 4 != 0 {
            payload.push(0);
        }
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap()); // image data length
        payload.extend_from_slice(data);
        while payload.len() % 4 != 0 {
            payload.push(0);
        }
        put_u32_le(&mut payload, 0); // m_StreamData.offset low
        put_u32_le(&mut payload, 0); // m_StreamData.offset high
        put_u32_le(&mut payload, 0); // m_StreamData.size
        put_i32_le(&mut payload, 0); // m_StreamData.path (string: length 0)
        while payload.len() % 4 != 0 {
            payload.push(0);
        }

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 28);
        put_u16_le(&mut metadata, 28);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_texture_file(
        class_id: i32,
        root_type_name: &str,
        width: i32,
        height: i32,
        data: &[u8],
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, root_type_name);
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let width_name = push_type_tree_string(&mut string_buffer, "m_Width");
        let height_name = push_type_tree_string(&mut string_buffer, "m_Height");
        let format_name = push_type_tree_string(&mut string_buffer, "m_TextureFormat");
        let complete_size_name = push_type_tree_string(&mut string_buffer, "m_CompleteImageSize");
        let image_count_name = push_type_tree_string(&mut string_buffer, "m_ImageCount");
        let uint8_type = push_type_tree_string(&mut string_buffer, "UInt8");
        let lightmap_name = push_type_tree_string(&mut string_buffer, "m_LightmapFormat");
        let typeless_data_type = push_type_tree_string(&mut string_buffer, "TypelessData");
        let image_data_name = push_type_tree_string(&mut string_buffer, "image data");
        let size_name = push_type_tree_string(&mut string_buffer, "size");
        let data_name = push_type_tree_string(&mut string_buffer, "data");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, class_id);
        put_i32_le(&mut metadata, 10);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty_name, -1, 0),
            (1, int_type, width_name, 4, 1),
            (1, int_type, height_name, 4, 2),
            (1, int_type, format_name, 4, 3),
            (1, int_type, complete_size_name, 4, 4),
            (1, int_type, image_count_name, 4, 5),
            (1, uint8_type, lightmap_name, 1, 6),
            (1, typeless_data_type, image_data_name, -1, 7),
            (2, int_type, size_name, 4, 8),
            (2, uint8_type, data_name, 1, 9),
        ] {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, width); // m_Width
        put_i32_le(&mut payload, height); // m_Height
        put_i32_le(&mut payload, 4); // m_TextureFormat (RGBA32)
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap()); // m_CompleteImageSize
        put_i32_le(&mut payload, 1); // m_ImageCount
        payload.push(0); // m_LightmapFormat
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap());
        payload.extend_from_slice(data);
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, class_id);
        put_u16_le(&mut metadata, class_id as u16);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_build_settings_file(scene_paths: &[&str]) -> Vec<u8> {
        make_build_settings_file_with_path_id(scene_paths, 42)
    }

    /// Synthetic class-117/187/188 texture file: 1x1(x1) RGBA pixels behind a
    /// `GraphicsFormat` discriminant of 4 (RGBA32 decode). `height` is absent
    /// on class 188 (its height equals its width); `third` is `m_Depth` on 117
    /// and 187, `m_CubemapCount` on 188; `size` is `m_DataSize` (u32) on
    /// 187/188 and `m_CompleteImageSize` (u32 spelling) on 117.
    #[allow(clippy::too_many_arguments)]
    fn make_array_texture_file(
        class_id: i32,
        root_type_name: &str,
        height: Option<i32>,
        third: Option<(&str, i32)>,
        size_name: &str,
        size_is_u32: bool,
        size_value: u32,
        data: &[u8],
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, root_type_name);
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let uint_type = push_type_tree_string(&mut string_buffer, "UInt32");
        let width_name = push_type_tree_string(&mut string_buffer, "m_Width");
        let height_name = push_type_tree_string(&mut string_buffer, "m_Height");
        let third_name = third.map(|(name, _)| push_type_tree_string(&mut string_buffer, name));
        let format_name = push_type_tree_string(&mut string_buffer, "m_Format");
        let size_name = push_type_tree_string(&mut string_buffer, size_name);
        let typeless_data_type = push_type_tree_string(&mut string_buffer, "TypelessData");
        let image_data_name = push_type_tree_string(&mut string_buffer, "m_ImageData");
        let size_field_name = push_type_tree_string(&mut string_buffer, "size");
        let data_field_name = push_type_tree_string(&mut string_buffer, "data");
        let uint8_type = push_type_tree_string(&mut string_buffer, "UInt8");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, class_id);
        let size_type = if size_is_u32 { uint_type } else { int_type };
        let mut array_nodes = vec![(0, root_type, empty_name, -1, 0)];
        array_nodes.push((1, int_type, width_name, 4, 1));
        let mut array_index = 2;
        if height.is_some() {
            array_nodes.push((1, int_type, height_name, 4, array_index));
            array_index += 1;
        }
        if let Some(third) = third_name {
            array_nodes.push((1, int_type, third, 4, array_index));
            array_index += 1;
        }
        array_nodes.push((1, int_type, format_name, 4, array_index));
        array_index += 1;
        array_nodes.push((1, size_type, size_name, 4, array_index));
        array_index += 1;
        array_nodes.push((1, typeless_data_type, image_data_name, -1, array_index));
        array_index += 1;
        array_nodes.push((2, int_type, size_field_name, 4, array_index));
        array_index += 1;
        array_nodes.push((2, uint8_type, data_field_name, 1, array_index));
        put_i32_le(&mut metadata, i32::try_from(array_nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in array_nodes {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, 1); // m_Width
        if let Some(height) = height {
            put_i32_le(&mut payload, height); // m_Height
        }
        if let Some((_, third)) = third {
            put_i32_le(&mut payload, third); // m_Depth or m_CubemapCount
        }
        put_i32_le(&mut payload, 4); // m_Format (GraphicsFormat RGBA32 route)
        if size_is_u32 {
            put_u32_le(&mut payload, size_value);
        } else {
            put_i32_le(&mut payload, size_value as i32);
        }
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap());
        payload.extend_from_slice(data);
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, class_id);
        put_u16_le(&mut metadata, class_id as u16);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }
    fn make_build_settings_file_with_path_id(scene_paths: &[&str], object_path_id: i32) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let build_settings_type = push_type_tree_string(&mut string_buffer, "BuildSettings");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let vector_type = push_type_tree_string(&mut string_buffer, "vector");
        let scenes_field = push_type_tree_string(&mut string_buffer, "m_Scenes");
        let array_type = push_type_tree_string(&mut string_buffer, "Array");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let size_field = push_type_tree_string(&mut string_buffer, "size");
        let string_type = push_type_tree_string(&mut string_buffer, "string");
        let data_field = push_type_tree_string(&mut string_buffer, "data");
        let char_type = push_type_tree_string(&mut string_buffer, "char");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 141);
        put_i32_le(&mut metadata, 8);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, build_settings_type, empty_name, -1, 0),
            (1, vector_type, scenes_field, -1, 1),
            (2, array_type, array_type, -1, 2),
            (3, int_type, size_field, 4, 3),
            (3, string_type, data_field, -1, 4),
            (4, array_type, array_type, -1, 5),
            (5, int_type, size_field, 4, 6),
            (5, char_type, data_field, 1, 7),
        ] {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, object_path_id);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, i32::try_from(scene_paths.len()).unwrap());
        for scene_path in scene_paths {
            let scene_path = scene_path.as_bytes();
            put_i32_le(&mut payload, i32::try_from(scene_path.len()).unwrap());
            payload.extend_from_slice(scene_path);
            let padding = (4 - scene_path.len() % 4) % 4;
            payload.resize(payload.len() + padding, 0);
        }
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 141);
        put_u16_le(&mut metadata, 141);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_occlusion_settings_file(scene_guid: [u32; 4]) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, "OcclusionCullingSettings");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let guid_type = push_type_tree_string(&mut string_buffer, "GUID");
        let scene_guid_name = push_type_tree_string(&mut string_buffer, "m_SceneGUID");
        let uint32_type = push_type_tree_string(&mut string_buffer, "UInt32");
        let word_names = [
            push_type_tree_string(&mut string_buffer, "m_Data_0_"),
            push_type_tree_string(&mut string_buffer, "m_Data_1_"),
            push_type_tree_string(&mut string_buffer, "m_Data_2_"),
            push_type_tree_string(&mut string_buffer, "m_Data_3_"),
        ];

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 29);
        put_i32_le(&mut metadata, 6);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        put_node(&mut metadata, 0, root_type, empty_name, -1, 0);
        put_node(&mut metadata, 1, guid_type, scene_guid_name, 16, 1);
        for (index, name) in word_names.into_iter().enumerate() {
            put_node(
                &mut metadata,
                2,
                uint32_type,
                name,
                4,
                i32::try_from(index + 2).unwrap(),
            );
        }
        metadata.extend_from_slice(&string_buffer);

        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 16);
        put_i32_le(&mut metadata, 29);
        put_u16_le(&mut metadata, 29);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let mut payload = Vec::new();
        for word in scene_guid {
            put_u32_le(&mut payload, word);
        }
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn put_i64_le(output: &mut Vec<u8>, value: i64) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    /// Single-`GameObject` SerializedFile mirroring the proven layout in
    /// `processing::scene` tests: `m_Name`, `m_IsActive`, and an
    /// `m_Component` PPtr vector whose entries address `(file_id, path_id)`
    /// with `file_id` indexing this file's own external table.
    fn make_inspect_game_object_file(
        name: &str,
        path_id: i32,
        components: &[(i32, i32)],
        external_path: Option<&str>,
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let root_type = push_type_tree_string(&mut strings, "GameObject");
        let empty = push_type_tree_string(&mut strings, "");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let string_type = push_type_tree_string(&mut strings, "string");
        let array = push_type_tree_string(&mut strings, "Array");
        let size = push_type_tree_string(&mut strings, "size");
        let int_type = push_type_tree_string(&mut strings, "int");
        let data = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let active_field = push_type_tree_string(&mut strings, "m_IsActive");
        let bool_type = push_type_tree_string(&mut strings, "bool");
        let component_field = push_type_tree_string(&mut strings, "m_Component");
        let vector_type = push_type_tree_string(&mut strings, "vector");
        let pptr_type = push_type_tree_string(&mut strings, "PPtr<Component>");
        let file_id_field = push_type_tree_string(&mut strings, "m_FileID");
        let path_id_field = push_type_tree_string(&mut strings, "m_PathID");
        let int32_type = push_type_tree_string(&mut strings, "SInt32");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 12);
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty, -1, 0),
            (1, string_type, name_field, -1, 1),
            (2, array, array, -1, 2),
            (3, int_type, size, 4, 3),
            (3, char_type, data, 1, 4),
            (1, bool_type, active_field, 1, 5),
            (1, vector_type, component_field, -1, 6),
            (2, array, array, -1, 7),
            (3, int_type, size, 4, 8),
            (3, pptr_type, data, -1, 9),
            (4, int32_type, file_id_field, 4, 10),
            (4, int32_type, path_id_field, 4, 11),
        ] {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, path_id);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, i32::try_from(name.len()).unwrap());
        payload.extend_from_slice(name.as_bytes());
        let name_padding = (4 - name.len() % 4) % 4;
        payload.resize(payload.len() + name_padding, 0);
        payload.push(1);
        put_i32_le(&mut payload, i32::try_from(components.len()).unwrap());
        for &(file_id, target_path_id) in components {
            put_i32_le(&mut payload, file_id);
            put_i32_le(&mut payload, target_path_id);
        }
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::from(external_path.is_some()));
        if let Some(external_path) = external_path {
            metadata.push(0);
            metadata.extend_from_slice(&[0; 16]);
            put_i32_le(&mut metadata, 0);
            metadata.extend_from_slice(external_path.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        finish_inspect_serialized_file(metadata, payload)
    }

    /// Single-`Transform` SerializedFile with the same node layout as the
    /// `processing::scene` fixture: `m_GameObject`, rotation/position/scale,
    /// `m_Children`, `m_Father`, `m_RootOrder`, and the euler hint.
    #[allow(clippy::too_many_arguments)]
    fn make_inspect_transform_file(
        path_id: i32,
        game_object: (i32, i64),
        children: &[(i32, i64)],
        father: (i32, i64),
        root_order: i32,
        external_names: &[&str],
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let root_type = push_type_tree_string(&mut strings, "Transform");
        let empty = push_type_tree_string(&mut strings, "");
        let pptr_game_object = push_type_tree_string(&mut strings, "PPtr<GameObject>");
        let pptr_transform = push_type_tree_string(&mut strings, "PPtr<Transform>");
        let quaternion_type = push_type_tree_string(&mut strings, "Quaternion");
        let vector3_type = push_type_tree_string(&mut strings, "Vector3");
        let float_type = push_type_tree_string(&mut strings, "float");
        let int_type = push_type_tree_string(&mut strings, "int");
        let vector_type = push_type_tree_string(&mut strings, "vector");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let file_id_field = push_type_tree_string(&mut strings, "m_FileID");
        let file_id_type = push_type_tree_string(&mut strings, "SInt32");
        let path_id_field = push_type_tree_string(&mut strings, "m_PathID");
        let path_id_type = push_type_tree_string(&mut strings, "SInt64");
        let game_object_field = push_type_tree_string(&mut strings, "m_GameObject");
        let local_rotation_field = push_type_tree_string(&mut strings, "m_LocalRotation");
        let local_position_field = push_type_tree_string(&mut strings, "m_LocalPosition");
        let local_scale_field = push_type_tree_string(&mut strings, "m_LocalScale");
        let children_field = push_type_tree_string(&mut strings, "m_Children");
        let father_field = push_type_tree_string(&mut strings, "m_Father");
        let root_order_field = push_type_tree_string(&mut strings, "m_RootOrder");
        let euler_hint_field = push_type_tree_string(&mut strings, "m_LocalEulerAnglesHint");
        let x_field = push_type_tree_string(&mut strings, "x");
        let y_field = push_type_tree_string(&mut strings, "y");
        let z_field = push_type_tree_string(&mut strings, "z");
        let w_field = push_type_tree_string(&mut strings, "w");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");

        let mut nodes = Vec::new();
        let mut add_node = |level, type_offset, name_offset, byte_size: i32| {
            let index = i32::try_from(nodes.len()).unwrap();
            nodes.push((level, type_offset, name_offset, byte_size, index));
        };
        add_node(0, root_type, empty, -1);
        add_node(1, pptr_game_object, game_object_field, -1);
        add_node(2, file_id_type, file_id_field, 4);
        add_node(2, path_id_type, path_id_field, 8);
        add_node(1, quaternion_type, local_rotation_field, 16);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);
        add_node(2, float_type, w_field, 4);
        add_node(1, vector3_type, local_position_field, 12);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);
        add_node(1, vector3_type, local_scale_field, 12);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);
        add_node(1, vector_type, children_field, -1);
        add_node(2, array_type, array_type, -1);
        add_node(3, int_type, size_field, 4);
        add_node(3, pptr_transform, data_field, -1);
        add_node(4, file_id_type, file_id_field, 4);
        add_node(4, path_id_type, path_id_field, 8);
        add_node(1, pptr_transform, father_field, -1);
        add_node(2, file_id_type, file_id_field, 4);
        add_node(2, path_id_type, path_id_field, 8);
        add_node(1, int_type, root_order_field, 4);
        add_node(1, vector3_type, euler_hint_field, 12);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 4);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in nodes {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, path_id);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, game_object.0);
        put_i64_le(&mut payload, game_object.1);
        for value in [0.0, 0.0, 0.0, 1.0] {
            put_f32_le(&mut payload, value);
        }
        for value in [0.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
            put_f32_le(&mut payload, value);
        }
        put_i32_le(&mut payload, i32::try_from(children.len()).unwrap());
        for &(file_id, child_path_id) in children {
            put_i32_le(&mut payload, file_id);
            put_i64_le(&mut payload, child_path_id);
        }
        put_i32_le(&mut payload, father.0);
        put_i64_le(&mut payload, father.1);
        put_i32_le(&mut payload, root_order);
        for value in [0.0, 0.0, 0.0] {
            put_f32_le(&mut payload, value);
        }

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 4);
        put_u16_le(&mut metadata, 4);
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

        finish_inspect_serialized_file(metadata, payload)
    }

    fn finish_inspect_serialized_file(metadata: Vec<u8>, payload: Vec<u8>) -> Vec<u8> {
        let data_offset = 20usize + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
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
