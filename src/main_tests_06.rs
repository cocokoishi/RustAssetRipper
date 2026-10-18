    #[test]
    fn export_project_redirects_engine_resource_references_to_predefined_guid_and_path_id() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        fn assert_case(fixture: &str, engine_file_name: &str, expected_guid: &str) {
            let fixture_value = |prefix: &str| {
                fixture
                    .lines()
                    .find_map(|line| line.strip_prefix(prefix))
                    .unwrap_or_else(|| panic!("source engine-resource fixture is missing {prefix}"))
            };
            let decode_hex = |hex: &str| {
                assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
                hex.as_bytes()
                    .chunks_exact(2)
                    .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                    .collect::<Vec<_>>()
            };
            let decode_base64 = |encoded: &str| {
                fn digit(byte: u8) -> u8 {
                    match byte {
                        b'A'..=b'Z' => byte - b'A',
                        b'a'..=b'z' => byte - b'a' + 26,
                        b'0'..=b'9' => byte - b'0' + 52,
                        b'+' => 62,
                        b'/' => 63,
                        _ => panic!("source fixture contains invalid base64"),
                    }
                }
                assert!(encoded.len().is_multiple_of(4), "base64 length is aligned");
                let mut output = Vec::with_capacity(encoded.len() / 4 * 3);
                let mut buffer = 0u32;
                let mut bits = 0u8;
                for byte in encoded.bytes() {
                    if byte == b'=' {
                        break;
                    }
                    buffer = (buffer << 6) | u32::from(digit(byte));
                    bits += 6;
                    if bits >= 8 {
                        bits -= 8;
                        output.push((buffer >> bits) as u8);
                        buffer &= (1 << bits) - 1;
                    }
                }
                output
            };
            let managed_bytes = decode_hex(fixture_value("INPUT_HEX|managed.assets|"));
            let engine_bytes = decode_hex(fixture_value(&format!("INPUT_HEX|{engine_file_name}|")));
            let source_root = decode_base64(fixture_value(
                "CONTENT|ExportedProject/Assets/MonoBehaviour/ManagedRoot.asset|",
            ));
            let source_meta = decode_base64(fixture_value(
                "CONTENT|ExportedProject/Assets/MonoBehaviour/ManagedRoot.asset.meta|",
            ));

            let directory = TestInputDirectory::new_root();
            let managed_path = directory.join("managed.assets");
            let engine_path = directory.join(engine_file_name);
            fs::write(&managed_path, &managed_bytes).expect("write source-generated root file");
            fs::write(&engine_path, &engine_bytes).expect("write source-generated engine file");
            let input = TestInputDirectory {
                directory: directory.clone(),
                files: vec![managed_path.clone(), engine_path.clone()],
                subdirectories: Vec::new(),
            };
            let output_root = directory.join("project-engine-resource-reference");
            let mut status = Vec::new();
            super::export_project_to(
                std::slice::from_ref(&directory),
                &output_root,
                false,
                &default_export_settings(),
                &mut status,
            )
            .expect("engine resource references use the source predefined pointer");

            let status = String::from_utf8(status).expect("export status is UTF-8");
            assert!(status.contains("2 SerializedFiles"), "{status}");
            assert!(status.contains("1 objects in 1 asset files"), "{status}");
            let root_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
            let root_meta_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset.meta");
            let rust_root_yaml = fs::read(&root_path).expect("root YAML is exported");
            assert_eq!(
                source_root, rust_root_yaml,
                "source and Rust root YAML match exactly for {engine_file_name}"
            );
            let normalize_meta = |bytes: &[u8]| {
                String::from_utf8(bytes.to_vec())
                    .expect("metadata is UTF-8")
                    .lines()
                    .filter(|line| {
                        !line.starts_with("guid: ") && !line.starts_with("timeCreated: ")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            };
            assert_eq!(
                normalize_meta(&source_meta),
                normalize_meta(&fs::read(&root_meta_path).expect("root metadata is exported")),
                "source and Rust NativeFormatImporter metadata match after generated identity fields"
            );
            let root_yaml_text = String::from_utf8(rust_root_yaml).expect("root YAML is UTF-8");
            assert!(
                root_yaml_text.contains(&format!(
                    "m_Asset: {{fileID: 900, guid: {expected_guid}, type: 0}}"
                )),
                "the root PPtr keeps the predefined GUID, original PathID, and internal type"
            );
            assert!(
                !output_root
                    .join("Assets/MonoBehaviour/EngineResourceTarget.asset")
                    .exists(),
                "redirected engine objects have no standalone project file"
            );
            assert_eq!(fs::read(&managed_path).unwrap(), managed_bytes);
            assert_eq!(fs::read(&engine_path).unwrap(), engine_bytes);

            fs::remove_dir_all(&output_root).expect("remove engine-reference project output");
            drop(input);
        }

        assert_case(
            include_str!("export/engine_resource_reference_project_fixture.txt"),
            "unity default resources",
            "0000000000000000e000000000000000",
        );
        assert_case(
            include_str!("export/engine_builtin_extra_reference_project_fixture.txt"),
            "unity builtin extra",
            "0000000000000000f000000000000000",
        );
        assert_case(
            include_str!("export/engine_resource_underscore_reference_project_fixture.txt"),
            "unity_default_resources",
            "0000000000000000e000000000000000",
        );
        assert_case(
            include_str!("export/engine_builtin_extra_underscore_reference_project_fixture.txt"),
            "unity_builtin_extra",
            "0000000000000000f000000000000000",
        );
    }

    #[test]
    fn export_project_uses_source_entry_order_for_duplicate_nested_bundle_dependencies() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let source_fixture = include_str!("export/managed_reference_sibling_bundle_fixture.txt");
        assert!(source_fixture.contains("SELECTED|target.assets|FirstTarget|"));
        let source_bundle_hex = source_fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|workspace.bundle|"))
            .expect("source-runtime fixture includes the sibling UnityFS input");
        let source_bundle_bytes = source_bundle_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("source bundle fixture contains valid hex")
            })
            .collect::<Vec<_>>();
        let bundled_fixture = include_bytes!(
            "export/managed_reference_project_fixture/nested_sibling_reference.bundle"
        );
        assert_eq!(bundled_fixture, source_bundle_bytes.as_slice());

        let directory = TestInputDirectory::new_root();
        let bundle_path = directory.join("workspace.bundle");
        fs::write(&bundle_path, bundled_fixture).expect("source-runtime sibling UnityFS fixture");
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![bundle_path.clone()],
            subdirectories: Vec::new(),
        };
        let (mut graph_filesystem, selected_paths, mut graph_mount_report) =
            super::mount_project_inputs(std::slice::from_ref(&bundle_path)).unwrap();
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &graph_filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (graph_workspace, _) = super::load_workspace_with_mount_report(
            &mut graph_filesystem,
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            &mixed.data_paths,
            super::WorkspaceLoadLimits::default(),
            &mut graph_mount_report,
        )
        .unwrap();
        let graph = graph_workspace
            .reference_graph(
                assetripper::serialized_file::reference_graph::MultiFileReferenceGraphLimits::default(),
            )
            .unwrap();
        let root_file_index = graph_workspace
            .files()
            .iter()
            .position(|file| file.file_name == "managed.assets")
            .unwrap();
        let root_pointer = graph
            .references()
            .iter()
            .find(|reference| {
                reference.source_file_index == root_file_index
                    && reference.field_path == "references.RefIds[0].data.m_Asset"
            })
            .unwrap();
        let assetripper::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index: 0,
        } = root_pointer.resolution
        else {
            panic!("the source-backed nested PPtr must resolve to an object");
        };
        assert!(graph_workspace.files()[target_file_index]
            .virtual_path
            .ends_with("first.bundle/target.assets"));
        let output_root = directory.join("project-sibling-bundle");
        let mut status = Vec::new();
        super::export_project_to(
            std::slice::from_ref(&bundle_path),
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect("duplicate sibling collection references resolve in source entry order");

        let status = String::from_utf8(status).expect("export status is UTF-8");
        assert!(status.contains("3 SerializedFiles"), "{status}");
        assert!(status.contains("3 objects in 3 asset files"), "{status}");

        let root_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
        let root_meta_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset.meta");
        let first_target_path = output_root.join("Assets/MonoBehaviour/FirstTarget.asset");
        let first_target_meta_path =
            output_root.join("Assets/MonoBehaviour/FirstTarget.asset.meta");
        let second_target_path = output_root.join("Assets/MonoBehaviour/SecondTarget.asset");
        let second_target_meta_path =
            output_root.join("Assets/MonoBehaviour/SecondTarget.asset.meta");
        let root_yaml = fs::read_to_string(&root_path).expect("root YAML is exported");
        let first_target_yaml =
            fs::read_to_string(&first_target_path).expect("first sibling target YAML is exported");
        let second_target_yaml = fs::read_to_string(&second_target_path)
            .expect("second sibling target YAML is exported");
        let first_target_guid = meta_guid(&first_target_meta_path);
        let second_target_guid = meta_guid(&second_target_meta_path);
        let source_target_meta = include_str!(
            "export/managed_reference_project_fixture/source_ReferencedRoot.asset.meta"
        );
        let source_guid = source_target_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("frozen source target metadata has a GUID");
        let source_root_yaml =
            include_str!("export/managed_reference_project_fixture/source_ManagedRoot.asset");
        assert_eq!(
            source_root_yaml
                .replace(source_guid, "<TARGET_GUID>")
                .trim_end(),
            root_yaml
                .replace(&first_target_guid, "<TARGET_GUID>")
                .trim_end(),
            "the root PPtr maps to the first same-named collection in bundle-entry order"
        );
        assert!(root_yaml.contains(&format!(
            "m_Asset: {{fileID: 11400000, guid: {first_target_guid}, type: 2}}"
        )));
        assert!(
            !root_yaml.contains(&second_target_guid),
            "the root PPtr must not map to the later duplicate collection"
        );
        assert_eq!(
            include_str!(
                "export/managed_reference_project_fixture/source_sibling_FirstTarget.asset"
            )
            .trim_end(),
            first_target_yaml.trim_end()
        );
        assert_eq!(
            include_str!(
                "export/managed_reference_project_fixture/source_sibling_SecondTarget.asset"
            )
            .trim_end(),
            second_target_yaml.trim_end()
        );
        assert!(first_target_yaml.contains("m_Health: 789"));
        assert!(second_target_yaml.contains("m_Health: 456"));
        assert!(root_yaml.contains("        m_Link:\n          rid: 200\n"));
        assert!(root_yaml.contains("        m_Link:\n          rid: 100\n"));

        let normalize_meta = |path: &std::path::Path| {
            let normalized = fs::read_to_string(path)
                .expect("asset metadata is readable")
                .lines()
                .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n");
            normalized.trim_end_matches(['\r', '\n']).to_owned() + "\n"
        };
        let expected_meta = include_str!(
            "export/managed_reference_project_fixture/source_nested_ReferencedRoot.asset.meta.normalized"
        );
        assert_eq!(expected_meta, normalize_meta(&root_meta_path));
        assert_eq!(expected_meta, normalize_meta(&first_target_meta_path));
        assert_eq!(expected_meta, normalize_meta(&second_target_meta_path));
        let source_path_id_map_hex = source_fixture
            .lines()
            .find_map(|line| line.strip_prefix("SOURCE_OUTPUT|AuxiliaryFiles/path_id_map.json|"))
            .expect("source fixture records the serialized collection order in PathIdMap");
        let source_path_id_map = source_path_id_map_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("source PathIdMap fixture contains valid hex")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            normalize_text_line_endings(&fs::read(output_root.join("AuxiliaryFiles/path_id_map.json"))
                .expect("project export writes the PathIdMap post-export file")),
            normalize_text_line_endings(&source_path_id_map),
            "PathIdMap collection names/order differ across sibling nested bundles"
        );
        assert_eq!(fs::read(&bundle_path).unwrap(), bundled_fixture);

        fs::remove_dir_all(&output_root).expect("remove sibling-bundle project output");
        drop(input);
    }

    #[test]
    fn export_project_resolves_dependency_from_nested_bundle_scope_like_source() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let source_fixture =
            include_str!("export/managed_reference_nested_sibling_bundle_fixture.txt");
        assert!(source_fixture
            .contains("SELECTED|source.bundle/managed.assets|target.assets|FirstTarget|"));
        let fixture_value = |prefix: &str| {
            source_fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("nested-scope source fixture is missing {prefix}"))
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
        let decode_base64 = |encoded: &str| {
            fn digit(byte: u8) -> u8 {
                match byte {
                    b'A'..=b'Z' => byte - b'A',
                    b'a'..=b'z' => byte - b'a' + 26,
                    b'0'..=b'9' => byte - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    _ => panic!("fixture base64 has an invalid digit"),
                }
            }
            let mut output = Vec::new();
            let mut buffer = 0u32;
            let mut bits = 0u8;
            for byte in encoded.bytes() {
                if byte == b'=' {
                    break;
                }
                buffer = (buffer << 6) | u32::from(digit(byte));
                bits += 6;
                if bits >= 8 {
                    bits -= 8;
                    output.push((buffer >> bits) as u8);
                    buffer &= (1 << bits) - 1;
                }
            }
            output
        };
        let source_bundle_hex = fixture_value("INPUT_HEX|workspace.bundle|");
        let source_bundle_bytes = decode_hex(source_bundle_hex);
        let bundled_fixture = include_bytes!(
            "export/managed_reference_project_fixture/nested_source_sibling_reference.bundle"
        );
        assert_eq!(bundled_fixture, source_bundle_bytes.as_slice());
        let source_root_yaml = String::from_utf8(decode_base64(fixture_value("ROOT_YAML|")))
            .expect("source root YAML is UTF-8");

        let directory = TestInputDirectory::new_root();
        let bundle_path = directory.join("workspace.bundle");
        fs::write(&bundle_path, bundled_fixture).expect("nested source-scope UnityFS fixture");
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![bundle_path.clone()],
            subdirectories: Vec::new(),
        };
        let (mut graph_filesystem, selected_paths, mut graph_mount_report) =
            super::mount_project_inputs(std::slice::from_ref(&bundle_path)).unwrap();
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &graph_filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (graph_workspace, _) = super::load_workspace_with_mount_report(
            &mut graph_filesystem,
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            &mixed.data_paths,
            super::WorkspaceLoadLimits::default(),
            &mut graph_mount_report,
        )
        .unwrap();
        let graph = graph_workspace
            .reference_graph(
                assetripper::serialized_file::reference_graph::MultiFileReferenceGraphLimits::default(),
            )
            .unwrap();
        let root_file_index = graph_workspace
            .files()
            .iter()
            .position(|file| file.virtual_path.ends_with("source.bundle/managed.assets"))
            .expect("managed source collection is nested under source.bundle");
        let root_pointer = graph
            .references()
            .iter()
            .find(|reference| {
                reference.source_file_index == root_file_index
                    && reference.field_path == "references.RefIds[0].data.m_Asset"
            })
            .expect("managed root has its external m_Asset pointer");
        let assetripper::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index: 0,
        } = root_pointer.resolution
        else {
            panic!("the nested source-scope PPtr must resolve to an object");
        };
        let target_path = &graph_workspace.files()[target_file_index].virtual_path;
        assert!(
            target_path.ends_with("first.bundle/target.assets"),
            "nested collection resolved to {target_path}"
        );
        assert_eq!(
            graph_workspace.resolve_dependency_collections().unwrap()[root_file_index][0],
            Some(target_file_index),
            "collection dependency resolution and the PPtr graph select the same sibling"
        );

        let output_root = directory.join("project-nested-source-scope");
        let mut status = Vec::new();
        super::export_project_to(
            std::slice::from_ref(&bundle_path),
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect("nested source-scope dependency resolves like AssetRipper");
        let status = String::from_utf8(status).expect("export status is UTF-8");
        assert!(status.contains("3 SerializedFiles"), "{status}");
        assert!(status.contains("3 objects in 3 asset files"), "{status}");

        let root_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
        let first_target_path = output_root.join("Assets/MonoBehaviour/FirstTarget.asset");
        let second_target_path = output_root.join("Assets/MonoBehaviour/SecondTarget.asset");
        let first_target_meta_path =
            output_root.join("Assets/MonoBehaviour/FirstTarget.asset.meta");
        let second_target_meta_path =
            output_root.join("Assets/MonoBehaviour/SecondTarget.asset.meta");
        let root_yaml = fs::read_to_string(&root_path).expect("root YAML is exported");
        let first_target_yaml =
            fs::read_to_string(&first_target_path).expect("first sibling target YAML is exported");
        let second_target_yaml = fs::read_to_string(&second_target_path)
            .expect("second sibling target YAML is exported");
        let first_target_guid = meta_guid(&first_target_meta_path);
        let second_target_guid = meta_guid(&second_target_meta_path);
        assert_eq!(
            source_root_yaml.trim_end(),
            root_yaml
                .replace(&first_target_guid, "<TARGET_GUID>")
                .trim_end(),
            "the source runtime and Rust export resolve from the nested collection identically"
        );
        assert!(root_yaml.contains(&format!(
            "m_Asset: {{fileID: 11400000, guid: {first_target_guid}, type: 2}}"
        )));
        assert!(!root_yaml.contains(&second_target_guid));
        assert_eq!(
            include_str!(
                "export/managed_reference_project_fixture/source_sibling_FirstTarget.asset"
            )
            .trim_end(),
            first_target_yaml.trim_end()
        );
        assert_eq!(
            include_str!(
                "export/managed_reference_project_fixture/source_sibling_SecondTarget.asset"
            )
            .trim_end(),
            second_target_yaml.trim_end()
        );
        assert!(first_target_yaml.contains("m_Health: 789"));
        assert!(second_target_yaml.contains("m_Health: 456"));
        assert!(root_yaml.contains("        m_Link:\n          rid: 200\n"));
        assert!(root_yaml.contains("        m_Link:\n          rid: 100\n"));

        let normalize_meta = |path: &std::path::Path| {
            let normalized = fs::read_to_string(path)
                .expect("asset metadata is readable")
                .lines()
                .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n");
            normalized.trim_end_matches(['\r', '\n']).to_owned() + "\n"
        };
        let expected_meta = include_str!(
            "export/managed_reference_project_fixture/source_nested_ReferencedRoot.asset.meta.normalized"
        );
        assert_eq!(expected_meta, normalize_meta(&first_target_meta_path));
        assert_eq!(expected_meta, normalize_meta(&second_target_meta_path));
        assert_eq!(fs::read(&bundle_path).unwrap(), bundled_fixture);

        fs::remove_dir_all(&output_root).expect("remove nested source-scope project output");
        drop(input);
    }

    #[test]
    fn export_project_keeps_nested_missing_dependency_as_source_missing_pointer() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let source_fixture =
            include_str!("export/managed_reference_missing_nested_dependency_fixture.txt");
        assert!(source_fixture.contains("MISSING|source.bundle/managed.assets|missing.assets"));
        assert!(source_fixture.contains(
            "POINTER|m_Asset: {fileID: 0, guid: 0000000deadbeef15deadf00d0000000, type: 2}"
        ));
        let source_root_yaml = include_str!(
            "export/managed_reference_project_fixture/source_nested_missing_external_reference.asset"
        );
        let bundled_fixture = include_bytes!(
            "export/managed_reference_project_fixture/nested_missing_external_reference.bundle"
        );

        let directory = TestInputDirectory::new_root();
        let bundle_path = directory.join("workspace.bundle");
        fs::write(&bundle_path, bundled_fixture)
            .expect("nested missing-dependency UnityFS fixture");
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![bundle_path.clone()],
            subdirectories: Vec::new(),
        };
        let (mut filesystem, selected_paths, mut mount_report) =
            super::mount_project_inputs(std::slice::from_ref(&bundle_path)).unwrap();
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (workspace, dependency_report) = super::load_workspace_with_mount_report(
            &mut filesystem,
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            &mixed.data_paths,
            super::WorkspaceLoadLimits::default(),
            &mut mount_report,
        )
        .unwrap();
        assert!(!dependency_report.is_complete());
        let requester_file_index = workspace
            .files()
            .iter()
            .position(|file| file.virtual_path.ends_with("source.bundle/managed.assets"))
            .expect("missing requester stays nested under source.bundle");
        let node_a_type_index = workspace.files()[requester_file_index]
            .metadata
            .reference_types
            .iter()
            .position(|reference| reference.class_name.as_deref() == Some("NodeA"))
            .expect("serialized reference table retains NodeA's TypeTree");
        assert!(workspace.files()[requester_file_index]
            .metadata
            .reference_types[node_a_type_index]
            .type_tree
            .as_ref()
            .expect("NodeA TypeTree is available")
            .nodes
            .iter()
            .any(|node| node.type_name.as_deref() == Some("PPtr<$Object>")));
        let object_workspace = super::load_object_workspace(
            &filesystem,
            &workspace,
            super::ObjectWorkspaceLimits::default(),
        )
        .expect("nested managed registry is decoded");
        let managed_root = object_workspace
            .objects_for_file(requester_file_index)
            .iter()
            .find(|object| object.path_id == 42)
            .expect("nested workspace contains ManagedRoot");
        let managed_root_payload = managed_root
            .payload
            .as_ref()
            .expect("ManagedRoot payload is decoded");
        let assetripper::serialized_file::object::SerializedValue::ManagedReferenceRegistry(
            registry,
        ) = managed_root_payload
            .root
            .unique_field("references")
            .unwrap()
            .expect("ManagedRoot retains its reference registry")
        else {
            panic!("ManagedRoot references field is a managed registry");
        };
        assert_eq!(
            registry.entries[0].reference_type_index,
            Some(node_a_type_index),
            "each managed registry entry retains its serialized TypeTree identity"
        );
        assert!(dependency_report.requests.iter().any(|request| {
            request.source_file_index == requester_file_index
                && request.dependency_name == "missing.assets"
                && matches!(
                    &request.outcome,
                    assetripper::serialized_file::workspace::SerializedFileDependencyOutcome::Missing
                )
        }));

        let graph = workspace
            .reference_graph(
                assetripper::serialized_file::reference_graph::MultiFileReferenceGraphLimits::default(),
            )
            .unwrap();
        let root_pointer = graph
            .references()
            .iter()
            .find(|reference| {
                reference.source_file_index == requester_file_index
                    && reference.field_path == "references.RefIds[0].data.m_Asset"
            })
            .expect("managed root retains its external m_Asset pointer");
        assert_eq!(
            root_pointer.resolution,
            assetripper::serialized_file::reference_graph::MultiFilePPtrResolution::MissingDependency {
                external_index: 0,
                path_id: 900,
            }
        );
        assert!(!graph.is_complete());

        let output_root = directory.join("project-nested-missing-dependency");
        super::export_project_to(
            std::slice::from_ref(&bundle_path),
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .expect("nested missing dependency exports with the source MissingReference pointer");
        let root_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
        let root_yaml = fs::read_to_string(&root_path).expect("managed root YAML is exported");
        assert_eq!(source_root_yaml.trim_end(), root_yaml.trim_end());
        assert_eq!(fs::read(&bundle_path).unwrap(), bundled_fixture);

        fs::remove_dir_all(&output_root).expect("remove nested missing-dependency project output");
        drop(input);
    }

    #[test]
    fn export_project_preserves_nested_missing_array_pointers() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let source_fixture =
            include_str!("export/managed_reference_missing_nested_array_dependency_fixture.txt");
        assert!(source_fixture.contains("MISSING|source.bundle/managed.assets|missing.assets"));
        assert!(source_fixture.contains(
            "POINTERS|- {fileID: 0, guid: 0000000deadbeef15deadf00d0000000, type: 2}|- {fileID: 0, guid: 0000000deadbeef15deadf00d0000000, type: 2}"
        ));
        let source_root_yaml = include_str!(
            "export/managed_reference_project_fixture/source_nested_missing_external_array_reference.asset"
        );
        let bundled_fixture = include_bytes!(
            "export/managed_reference_project_fixture/nested_missing_external_array_reference.bundle"
        );

        let directory = TestInputDirectory::new_root();
        let bundle_path = directory.join("workspace.bundle");
        fs::write(&bundle_path, bundled_fixture)
            .expect("nested missing-array-dependency UnityFS fixture");
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![bundle_path.clone()],
            subdirectories: Vec::new(),
        };
        let (mut filesystem, selected_paths, mut mount_report) =
            super::mount_project_inputs(std::slice::from_ref(&bundle_path)).unwrap();
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (workspace, dependency_report) = super::load_workspace_with_mount_report(
            &mut filesystem,
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            &mixed.data_paths,
            super::WorkspaceLoadLimits::default(),
            &mut mount_report,
        )
        .unwrap();
        assert!(!dependency_report.is_complete());
        let requester_file_index = workspace
            .files()
            .iter()
            .position(|file| file.virtual_path.ends_with("source.bundle/managed.assets"))
            .expect("missing requester stays nested under source.bundle");
        let node_a_type_index = workspace.files()[requester_file_index]
            .metadata
            .reference_types
            .iter()
            .position(|reference| reference.class_name.as_deref() == Some("NodeA"))
            .expect("serialized reference table retains NodeA's TypeTree");
        let node_a_tree = workspace.files()[requester_file_index]
            .metadata
            .reference_types[node_a_type_index]
            .type_tree
            .as_ref()
            .expect("NodeA TypeTree is available");
        assert!(node_a_tree.nodes.iter().any(|node| {
            node.type_name.as_deref() == Some("vector") && node.name.as_deref() == Some("m_Assets")
        }));
        assert!(node_a_tree.nodes.iter().any(|node| {
            node.type_name.as_deref() == Some("Array") && node.name.as_deref() == Some("Array")
        }));
        assert!(node_a_tree
            .nodes
            .iter()
            .any(|node| node.type_name.as_deref() == Some("PPtr<$Object>")));

        let object_workspace = super::load_object_workspace(
            &filesystem,
            &workspace,
            super::ObjectWorkspaceLimits::default(),
        )
        .expect("nested managed registry is decoded");
        let managed_root = object_workspace
            .objects_for_file(requester_file_index)
            .iter()
            .find(|object| object.path_id == 42)
            .expect("nested workspace contains ManagedRoot");
        let managed_root_payload = managed_root
            .payload
            .as_ref()
            .expect("ManagedRoot payload is decoded");
        let assetripper::serialized_file::object::SerializedValue::ManagedReferenceRegistry(
            registry,
        ) = managed_root_payload
            .root
            .unique_field("references")
            .unwrap()
            .expect("ManagedRoot retains its reference registry")
        else {
            panic!("ManagedRoot references field is a managed registry");
        };
        assert_eq!(
            registry.entries[0].reference_type_index,
            Some(node_a_type_index),
            "each managed registry entry retains its serialized TypeTree identity"
        );
        assert!(dependency_report.requests.iter().any(|request| {
            request.source_file_index == requester_file_index
                && request.dependency_name == "missing.assets"
                && matches!(
                    &request.outcome,
                    assetripper::serialized_file::workspace::SerializedFileDependencyOutcome::Missing
                )
        }));

        let graph = workspace
            .reference_graph(
                assetripper::serialized_file::reference_graph::MultiFileReferenceGraphLimits::default(),
            )
            .unwrap();
        let managed_array_pointers = graph
            .references()
            .iter()
            .filter(|reference| {
                reference.source_file_index == requester_file_index
                    && reference
                        .field_path
                        .starts_with("references.RefIds[0].data.m_Assets[")
            })
            .collect::<Vec<_>>();
        assert_eq!(managed_array_pointers.len(), 2);
        for (index, path_id) in [(0, 900), (1, 901)] {
            let expected_path = format!("references.RefIds[0].data.m_Assets[{index}]");
            let pointer = managed_array_pointers
                .iter()
                .find(|reference| reference.field_path == expected_path)
                .unwrap_or_else(|| {
                    panic!("managed array pointer path {expected_path} is retained")
                });
            assert_eq!(
                pointer.resolution,
                assetripper::serialized_file::reference_graph::MultiFilePPtrResolution::MissingDependency {
                    external_index: 0,
                    path_id,
                }
            );
        }
        assert!(!graph.is_complete());

        let output_root = directory.join("project-nested-missing-array-dependency");
        super::export_project_to(
            std::slice::from_ref(&bundle_path),
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .expect("nested missing array dependencies export as source MissingReference pointers");
        let root_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
        let root_yaml = fs::read_to_string(&root_path).expect("managed root YAML is exported");
        assert_eq!(source_root_yaml.trim_end(), root_yaml.trim_end());
        assert_eq!(fs::read(&bundle_path).unwrap(), bundled_fixture);

        fs::remove_dir_all(&output_root)
            .expect("remove nested missing array dependency project output");
        drop(input);
    }

    #[test]
    fn export_project_resolves_nested_array_pointers_to_source_target_objects() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let source_fixture =
            include_str!("export/managed_reference_resolved_nested_array_dependency_fixture.txt");
        assert!(source_fixture.contains(
            "RESOLVED|source.bundle/managed.assets|target.assets|source.bundle/target.assets"
        ));
        let source_root_yaml = include_str!(
            "export/managed_reference_project_fixture/source_nested_resolved_array_reference.asset"
        );
        let source_target_yaml = include_str!(
            "export/managed_reference_project_fixture/source_nested_resolved_array_target.asset"
        );
        let bundled_fixture = include_bytes!(
            "export/managed_reference_project_fixture/nested_resolved_array_reference.bundle"
        );

        let directory = TestInputDirectory::new_root();
        let bundle_path = directory.join("workspace.bundle");
        fs::write(&bundle_path, bundled_fixture)
            .expect("nested resolved-array dependency UnityFS fixture");
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![bundle_path.clone()],
            subdirectories: Vec::new(),
        };
        let (mut filesystem, selected_paths, mut mount_report) =
            super::mount_project_inputs(std::slice::from_ref(&bundle_path)).unwrap();
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (workspace, dependency_report) = super::load_workspace_with_mount_report(
            &mut filesystem,
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            &mixed.data_paths,
            super::WorkspaceLoadLimits::default(),
            &mut mount_report,
        )
        .unwrap();
        assert!(dependency_report.is_complete());
        let requester_file_index = workspace
            .files()
            .iter()
            .position(|file| file.virtual_path.ends_with("source.bundle/managed.assets"))
            .expect("nested requester remains inside source.bundle");
        let target_file_index = workspace
            .files()
            .iter()
            .position(|file| file.virtual_path.ends_with("source.bundle/target.assets"))
            .expect("resolved target remains inside source.bundle");
        let node_a_type_index = workspace.files()[requester_file_index]
            .metadata
            .reference_types
            .iter()
            .position(|reference| reference.class_name.as_deref() == Some("NodeA"))
            .expect("serialized reference table retains NodeA's TypeTree");
        let node_a_tree = workspace.files()[requester_file_index]
            .metadata
            .reference_types[node_a_type_index]
            .type_tree
            .as_ref()
            .expect("NodeA TypeTree is available");
        assert!(node_a_tree.nodes.iter().any(|node| {
            node.type_name.as_deref() == Some("vector") && node.name.as_deref() == Some("m_Assets")
        }));
        assert!(node_a_tree.nodes.iter().any(|node| {
            node.type_name.as_deref() == Some("Array") && node.name.as_deref() == Some("Array")
        }));
        assert!(node_a_tree
            .nodes
            .iter()
            .any(|node| node.type_name.as_deref() == Some("PPtr<$Object>")));

        let object_workspace = super::load_object_workspace(
            &filesystem,
            &workspace,
            super::ObjectWorkspaceLimits::default(),
        )
        .expect("nested managed registry and target object are decoded");
        let managed_root = object_workspace
            .objects_for_file(requester_file_index)
            .iter()
            .find(|object| object.path_id == 42)
            .expect("nested workspace contains ManagedRoot");
        let managed_root_payload = managed_root
            .payload
            .as_ref()
            .expect("ManagedRoot payload is decoded");
        let assetripper::serialized_file::object::SerializedValue::ManagedReferenceRegistry(
            registry,
        ) = managed_root_payload
            .root
            .unique_field("references")
            .unwrap()
            .expect("ManagedRoot retains its reference registry")
        else {
            panic!("ManagedRoot references field is a managed registry");
        };
        assert_eq!(
            registry.entries[0].reference_type_index,
            Some(node_a_type_index),
            "each managed registry entry retains its serialized TypeTree identity"
        );
        let target_object_index = object_workspace
            .objects_for_file(target_file_index)
            .iter()
            .position(|object| object.path_id == 900)
            .expect("target file contains ReferencedRoot at PathID 900");
        let target_object =
            &object_workspace.objects_for_file(target_file_index)[target_object_index];
        assert_eq!(target_object.raw_type_id, 114);
        let loaded_dependency = dependency_report
            .requests
            .iter()
            .find(|request| {
                request.source_file_index == requester_file_index
                    && request.dependency_name == "target.assets"
            })
            .expect("nested requester records its target.assets dependency");
        assert!(matches!(
            &loaded_dependency.outcome,
            assetripper::serialized_file::workspace::SerializedFileDependencyOutcome::AlreadyLoaded {
                file_index,
            } if *file_index == target_file_index
        ));
        assert_eq!(
            workspace.resolve_dependency_collections().unwrap()[requester_file_index][0],
            Some(target_file_index),
            "dependency collection lookup and PPtr resolution select the same nested file"
        );

        let graph = workspace
            .reference_graph(
                assetripper::serialized_file::reference_graph::MultiFileReferenceGraphLimits::default(),
            )
            .unwrap();
        let managed_array_pointers = graph
            .references()
            .iter()
            .filter(|reference| {
                reference.source_file_index == requester_file_index
                    && reference
                        .field_path
                        .starts_with("references.RefIds[0].data.m_Assets[")
            })
            .collect::<Vec<_>>();
        assert_eq!(managed_array_pointers.len(), 2);
        for index in [0, 1] {
            let expected_path = format!("references.RefIds[0].data.m_Assets[{index}]");
            let pointer = managed_array_pointers
                .iter()
                .find(|reference| reference.field_path == expected_path)
                .unwrap_or_else(|| {
                    panic!("managed array pointer path {expected_path} is retained")
                });
            assert_eq!(
                pointer.resolution,
                assetripper::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                    target_file_index,
                    target_object_index,
                }
            );
        }
        assert!(graph.is_complete());

        let output_root = directory.join("project-nested-resolved-array-dependency");
        let mut status = Vec::new();
        super::export_project_to(
            std::slice::from_ref(&bundle_path),
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect("nested array PPtrs export with source target mappings");
        let status = String::from_utf8(status).expect("export status is UTF-8");
        assert!(status.contains("2 SerializedFiles"), "{status}");
        assert!(status.contains("2 objects in 2 asset files"), "{status}");
        let root_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
        let target_path = output_root.join("Assets/MonoBehaviour/ReferencedRoot.asset");
        let target_meta_path = output_root.join("Assets/MonoBehaviour/ReferencedRoot.asset.meta");
        let root_yaml = fs::read_to_string(&root_path).expect("managed root YAML is exported");
        let target_yaml =
            fs::read_to_string(&target_path).expect("resolved target YAML is exported");
        let target_meta =
            fs::read_to_string(&target_meta_path).expect("target metadata is exported");
        let target_guid = target_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("target metadata has its GUID");
        assert_eq!(root_yaml.matches(target_guid).count(), 2);
        assert_eq!(
            source_root_yaml.trim_end(),
            root_yaml.replace(target_guid, "<TARGET_GUID>").trim_end()
        );
        assert_eq!(source_target_yaml.trim_end(), target_yaml.trim_end());
        assert_eq!(fs::read(&bundle_path).unwrap(), bundled_fixture);

        fs::remove_dir_all(&output_root)
            .expect("remove nested resolved array dependency project output");
        drop(input);
    }

    #[test]
    fn export_project_resolves_nested_array_pointers_to_distinct_target_objects() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let source_fixture = include_str!(
            "export/managed_reference_distinct_resolved_nested_array_dependency_fixture.txt"
        );
        assert!(source_fixture.contains(
            "DISTINCT_RESOLVED|source.bundle/managed.assets|target.assets|source.bundle/target.assets|900|901"
        ));
        let source_root_yaml = include_str!(
            "export/managed_reference_project_fixture/source_nested_distinct_resolved_array_reference.asset"
        );
        let source_target_900_yaml = include_str!(
            "export/managed_reference_project_fixture/source_nested_distinct_resolved_array_target_900.asset"
        );
        let source_target_901_yaml = include_str!(
            "export/managed_reference_project_fixture/source_nested_distinct_resolved_array_target_901.asset"
        );
        let bundled_fixture = include_bytes!(
            "export/managed_reference_project_fixture/nested_distinct_resolved_array_reference.bundle"
        );

        let directory = TestInputDirectory::new_root();
        let bundle_path = directory.join("workspace.bundle");
        fs::write(&bundle_path, bundled_fixture)
            .expect("nested distinct resolved-array dependency UnityFS fixture");
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![bundle_path.clone()],
            subdirectories: Vec::new(),
        };
        let (mut filesystem, selected_paths, mut mount_report) =
            super::mount_project_inputs(std::slice::from_ref(&bundle_path)).unwrap();
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (workspace, dependency_report) = super::load_workspace_with_mount_report(
            &mut filesystem,
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            &mixed.data_paths,
            super::WorkspaceLoadLimits::default(),
            &mut mount_report,
        )
        .unwrap();
        assert!(dependency_report.is_complete());
        let requester_file_index = workspace
            .files()
            .iter()
            .position(|file| file.virtual_path.ends_with("source.bundle/managed.assets"))
            .expect("nested requester remains inside source.bundle");
        let target_file_index = workspace
            .files()
            .iter()
            .position(|file| file.virtual_path.ends_with("source.bundle/target.assets"))
            .expect("resolved targets remain inside source.bundle");
        let node_a_type_index = workspace.files()[requester_file_index]
            .metadata
            .reference_types
            .iter()
            .position(|reference| reference.class_name.as_deref() == Some("NodeA"))
            .expect("serialized reference table retains NodeA's TypeTree");
        let node_a_tree = workspace.files()[requester_file_index]
            .metadata
            .reference_types[node_a_type_index]
            .type_tree
            .as_ref()
            .expect("NodeA TypeTree is available");
        assert!(node_a_tree.nodes.iter().any(|node| {
            node.type_name.as_deref() == Some("vector") && node.name.as_deref() == Some("m_Assets")
        }));
        assert!(node_a_tree.nodes.iter().any(|node| {
            node.type_name.as_deref() == Some("Array") && node.name.as_deref() == Some("Array")
        }));
        assert!(node_a_tree
            .nodes
            .iter()
            .any(|node| node.type_name.as_deref() == Some("PPtr<$Object>")));

        let object_workspace = super::load_object_workspace(
            &filesystem,
            &workspace,
            super::ObjectWorkspaceLimits::default(),
        )
        .expect("nested managed registry and both target objects are decoded");
        let managed_root = object_workspace
            .objects_for_file(requester_file_index)
            .iter()
            .find(|object| object.path_id == 42)
            .expect("nested workspace contains ManagedRoot");
        let managed_root_payload = managed_root
            .payload
            .as_ref()
            .expect("ManagedRoot payload is decoded");
        let assetripper::serialized_file::object::SerializedValue::ManagedReferenceRegistry(
            registry,
        ) = managed_root_payload
            .root
            .unique_field("references")
            .unwrap()
            .expect("ManagedRoot retains its reference registry")
        else {
            panic!("ManagedRoot references field is a managed registry");
        };
        assert_eq!(
            registry.entries[0].reference_type_index,
            Some(node_a_type_index),
            "each managed registry entry retains its serialized TypeTree identity"
        );
        let target_objects = object_workspace.objects_for_file(target_file_index);
        let target_900_index = target_objects
            .iter()
            .position(|object| object.path_id == 900)
            .expect("target file contains ReferencedRoot at PathID 900");
        let target_901_index = target_objects
            .iter()
            .position(|object| object.path_id == 901)
            .expect("target file contains AlternateReferencedRoot at PathID 901");
        assert_ne!(target_900_index, target_901_index);
        assert_eq!(target_objects[target_900_index].raw_type_id, 114);
        assert_eq!(target_objects[target_901_index].raw_type_id, 114);
        let loaded_dependency = dependency_report
            .requests
            .iter()
            .find(|request| {
                request.source_file_index == requester_file_index
                    && request.dependency_name == "target.assets"
            })
            .expect("nested requester records its target.assets dependency");
        assert!(matches!(
            &loaded_dependency.outcome,
            assetripper::serialized_file::workspace::SerializedFileDependencyOutcome::AlreadyLoaded {
                file_index,
            } if *file_index == target_file_index
        ));
        assert_eq!(
            workspace.resolve_dependency_collections().unwrap()[requester_file_index][0],
            Some(target_file_index),
            "dependency collection lookup and both PPtr targets select the same nested file"
        );

        let graph = workspace
            .reference_graph(
                assetripper::serialized_file::reference_graph::MultiFileReferenceGraphLimits::default(),
            )
            .unwrap();
        let managed_array_pointers = graph
            .references()
            .iter()
            .filter(|reference| {
                reference.source_file_index == requester_file_index
                    && reference
                        .field_path
                        .starts_with("references.RefIds[0].data.m_Assets[")
            })
            .collect::<Vec<_>>();
        assert_eq!(managed_array_pointers.len(), 2);
        for (index, target_object_index) in [(0, target_900_index), (1, target_901_index)] {
            let expected_path = format!("references.RefIds[0].data.m_Assets[{index}]");
            let pointer = managed_array_pointers
                .iter()
                .find(|reference| reference.field_path == expected_path)
                .unwrap_or_else(|| {
                    panic!("managed array pointer path {expected_path} is retained")
                });
            assert_eq!(
                pointer.resolution,
                assetripper::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                    target_file_index,
                    target_object_index,
                }
            );
        }
        assert_ne!(target_900_index, target_901_index);
        assert!(graph.is_complete());

        let output_root = directory.join("project-nested-distinct-resolved-array-dependency");
        let mut status = Vec::new();
        super::export_project_to(
            std::slice::from_ref(&bundle_path),
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect("distinct nested array PPtrs export with source target mappings");
        let status = String::from_utf8(status).expect("export status is UTF-8");
        assert!(status.contains("2 SerializedFiles"), "{status}");
        assert!(status.contains("3 objects in 3 asset files"), "{status}");
        let root_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
        let target_900_path = output_root.join("Assets/MonoBehaviour/ReferencedRoot.asset");
        let target_900_meta_path =
            output_root.join("Assets/MonoBehaviour/ReferencedRoot.asset.meta");
        let target_901_path =
            output_root.join("Assets/MonoBehaviour/AlternateReferencedRoot.asset");
        let target_901_meta_path =
            output_root.join("Assets/MonoBehaviour/AlternateReferencedRoot.asset.meta");
        let root_yaml = fs::read_to_string(&root_path).expect("managed root YAML is exported");
        let target_900_yaml =
            fs::read_to_string(&target_900_path).expect("PathID 900 target YAML is exported");
        let target_900_meta = fs::read_to_string(&target_900_meta_path)
            .expect("PathID 900 target metadata is exported");
        let target_901_yaml =
            fs::read_to_string(&target_901_path).expect("PathID 901 target YAML is exported");
        let target_901_meta = fs::read_to_string(&target_901_meta_path)
            .expect("PathID 901 target metadata is exported");
        let target_900_guid = target_900_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("PathID 900 target metadata has its GUID");
        let target_901_guid = target_901_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("PathID 901 target metadata has its GUID");
        assert_ne!(target_900_guid, target_901_guid);
        assert_eq!(root_yaml.matches(target_900_guid).count(), 1);
        assert_eq!(root_yaml.matches(target_901_guid).count(), 1);
        let normalized_root_yaml = root_yaml
            .replace(target_900_guid, "<TARGET_900_GUID>")
            .replace(target_901_guid, "<TARGET_901_GUID>");
        assert_eq!(source_root_yaml.trim_end(), normalized_root_yaml.trim_end());
        assert_eq!(
            source_target_900_yaml.trim_end(),
            target_900_yaml.trim_end()
        );
        assert_eq!(
            source_target_901_yaml.trim_end(),
            target_901_yaml.trim_end()
        );
        assert_eq!(fs::read(&bundle_path).unwrap(), bundled_fixture);

        fs::remove_dir_all(&output_root)
            .expect("remove nested distinct resolved array dependency project output");
        drop(input);
    }

    #[test]
    fn export_project_saves_managed_assembly_and_maps_script_pointer() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("processing/managed_assembly_saved_project_fixture.txt");
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
                .unwrap_or_else(|| panic!("saved-script fixture includes {prefix}"))
        };
        let source_output = |relative: &str| {
            let prefix = format!("CONTENT_HEX|{relative}|");
            fixture
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .map(decode_hex)
                .unwrap_or_else(|| panic!("saved-script fixture includes {prefix}"))
        };

        let directory = TestInputDirectory::new_root();
        let script_path = directory.join("saved_script.assets");
        let behaviour_path = directory.join("saved_behaviour.assets");
        let assembly_path = directory.join("FixtureScripts.dll");
        let script_bytes = input_bytes("saved_script.assets");
        let behaviour_bytes = input_bytes("saved_behaviour.assets");
        fs::write(&script_path, &script_bytes).unwrap();
        fs::write(&behaviour_path, &behaviour_bytes).unwrap();
        fs::write(
            &assembly_path,
            include_bytes!("processing/managed_assembly_fixture.dll"),
        )
        .unwrap();
        let output_root = directory.join("project-saved-script");
        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![
                script_path.clone(),
                behaviour_path.clone(),
                assembly_path.clone(),
            ],
            subdirectories: Vec::new(),
        };
        let mut settings = default_export_settings();
        settings.script_export_mode = ScriptExportMode::DllExportWithoutRenaming;
        let mut status = Vec::new();
        super::export_project_to(
            std::slice::from_ref(&directory),
            &output_root,
            false,
            &settings,
            &mut status,
        )
        .expect("manager-backed DllExportWithoutRenaming saves the assembly");

        let assembly_output = output_root.join("Assets/Plugins/FixtureScripts.dll");
        let assembly_meta = output_root.join("Assets/Plugins/FixtureScripts.dll.meta");
        let behaviour_output = output_root.join("Assets/MonoBehaviour/SavedScriptRoot.asset");
        let exported_assembly = fs::read(&assembly_output).expect("saved assembly bytes");
        let exported_assembly_model = ManagedAssembly::parse(
            "FixtureScripts.dll",
            &exported_assembly,
            ManagedAssemblyLimits::default(),
        )
        .expect("exported saved-script assembly parses");
        let source_assembly_model = ManagedAssembly::parse(
            "FixtureScripts.dll",
            include_bytes!("processing/managed_assembly_saved_source_saved_fixture.dll"),
            ManagedAssemblyLimits::default(),
        )
        .expect("source-saved script assembly parses");
        assert_eq!(
            exported_assembly_model.types, source_assembly_model.types,
            "manager-backed Save preserves the source-saved managed type model"
        );
        assert_ne!(
            exported_assembly,
            include_bytes!("processing/managed_assembly_fixture.dll"),
            "manager-backed Save must not copy the imported DLL bytes"
        );
        let normalized_meta = fs::read_to_string(&assembly_meta)
            .expect("PluginImporter sidecar")
            .lines()
            .filter(|line| !line.starts_with("timeCreated: "))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(
            normalized_meta.as_bytes(),
            source_output("Assets/Plugins/FixtureScripts.dll.meta"),
            "PluginImporter metadata matches the frozen 2019.4 source output after removing only its generated timestamp"
        );
        assert_eq!(
            fs::read(&behaviour_output).expect("saved-assembly MonoBehaviour YAML"),
            source_output("Assets/MonoBehaviour/SavedScriptRoot.asset"),
            "the MonoScript pointer uses the assembly GUID and script file ID"
        );
        assert!(
            !output_root
                .join("Assets/Scripts/FixtureScripts/Game/HeroController.cs")
                .exists(),
            "a manager-backed Save route does not emit the empty dummy script"
        );
        assert_eq!(fs::read(&script_path).unwrap(), script_bytes);
        assert_eq!(fs::read(&behaviour_path).unwrap(), behaviour_bytes);
        let status = String::from_utf8(status).unwrap();
        assert!(status.contains("MonoScript types: 1 resolved"), "{status}");

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
    fn export_project_skips_unknown_object_when_raw_fallback_setting_is_disabled() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let bytes = source_factory_fixture_bytes("unknown_no_tree");
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("unknown_no_tree.assets");
        let output_root = directory.join("project-unknown-class-default");
        fs::write(&source_path, &bytes).unwrap();
        let mut input = TestInputDirectory {
            directory,
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
        .unwrap();
        let status = String::from_utf8(status).unwrap();
        assert!(
            status.contains("SkipExportCollection (class 9999)"),
            "{status}"
        );
        assert!(!output_root.join("AssetRipper/UnknownAssets").exists());
        assert!(output_root
            .join("ProjectSettings/ProjectVersion.txt")
            .is_file());
        assert_eq!(fs::read(&source_path).unwrap(), bytes);
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

    #[test]
    fn export_project_skips_unreadable_object_when_raw_fallback_setting_is_disabled() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let bytes = source_factory_fixture_bytes("unreadable_no_tree");
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("unreadable_no_tree.assets");
        let output_root = directory.join("project-unreadable-class-default");
        fs::write(&source_path, &bytes).unwrap();
        let mut input = TestInputDirectory {
            directory,
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
        .unwrap();
        let status = String::from_utf8(status).unwrap();
        assert!(
            status.contains("SkipExportCollection (class 9999)"),
            "{status}"
        );
        assert!(!output_root.join("AssetRipper/UnreadableAssets").exists());
        assert!(output_root
            .join("ProjectSettings/ProjectVersion.txt")
            .is_file());
        assert_eq!(fs::read(&source_path).unwrap(), bytes);
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

    #[test]
    fn export_project_preserves_unknown_raw_bytes_without_meta_when_enabled() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let raw_payload = [1, 2, 3, 4, 5, 6, 7, 8];
        let source_bytes = source_factory_fixture_bytes("unknown_no_tree");
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("unknown_no_tree.assets");
        let output_root = directory.join("project-unknown-class-enabled");
        fs::write(&source_path, &source_bytes).unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![source_path.clone()],
            subdirectories: Vec::new(),
        };
        let mut settings = default_export_settings();
        settings.export_unreadable_assets = true;
        let mut status = Vec::new();
        super::export_project_to(
            std::slice::from_ref(&source_path),
            &output_root,
            false,
            &settings,
            &mut status,
        )
        .unwrap();
        let status = String::from_utf8(status).unwrap();
        assert!(
            status.contains("fields and embedded references remain opaque"),
            "{status}"
        );

        let raw_path =
            output_root.join("AssetRipper/UnknownAssets/9999/Unknown9999_3FCA88C5.unknown");
        assert_eq!(fs::read(&raw_path).unwrap(), raw_payload);
        assert!(!raw_path
            .with_file_name("Unknown9999_3FCA88C5.unknown.meta")
            .exists());
        assert_eq!(fs::read(&source_path).unwrap(), source_bytes);
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

    #[test]
    fn export_project_preserves_unreadable_raw_bytes_without_meta_when_enabled() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let raw_payload = [1, 2, 3, 4, 5, 6, 7, 8];
        let source_bytes = source_factory_fixture_bytes("unreadable_no_tree");
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("unreadable_no_tree.assets");
        let output_root = directory.join("project-unreadable-version");
        fs::write(&source_path, &source_bytes).unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![source_path.clone()],
            subdirectories: Vec::new(),
        };
        let mut settings = default_export_settings();
        settings.export_unreadable_assets = true;
        let mut status = Vec::new();
        super::export_project_to(
            std::slice::from_ref(&source_path),
            &output_root,
            false,
            &settings,
            &mut status,
        )
        .unwrap();
        let status = String::from_utf8(status).unwrap();
        assert!(
            status.contains("fields and embedded references remain opaque"),
            "{status}"
        );

        let raw_path = output_root
            .join("AssetRipper/UnreadableAssets/9999/Unreadable9999_3FCA88C5.unreadable");
        let raw_files = project_tree(&output_root)
            .into_iter()
            .filter(|path| path.ends_with(".unreadable"))
            .collect::<Vec<_>>();
        assert_eq!(raw_files.len(), 1, "{raw_files:?}");
        assert_eq!(
            raw_files[0],
            "AssetRipper/UnreadableAssets/9999/Unreadable9999_3FCA88C5.unreadable"
        );
        let raw_bytes = fs::read(&raw_path).unwrap();
        assert_eq!(raw_bytes, raw_payload);
        let meta_name = format!("{}.meta", raw_path.file_name().unwrap().to_string_lossy());
        assert!(!raw_path.with_file_name(meta_name).exists());
        assert_eq!(fs::read(&source_path).unwrap(), source_bytes);
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

    #[test]
    fn export_project_materializes_streamed_texture2d_and_fails_closed_when_resource_is_missing() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("texture.resS");
        let output_root = directory.join("project");
        let missing_output_root = directory.join("project-missing-resource");
        let serialized_bytes = make_streamed_texture2d_file("texture.resS", 2, 3);
        fs::write(&serialized_path, &serialized_bytes).unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        fs::create_dir(&output_root).unwrap();
        fs::create_dir(&missing_output_root).unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), resource_path.clone()],
            subdirectories: vec![
                output_root.clone(),
                missing_output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Texture2D"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                serialized_path.clone().into_os_string(),
                resource_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        let asset_path = output_root.join("Assets/Texture2D/Texture2D.texture2D");
        let meta_path = output_root.join("Assets/Texture2D/Texture2D.texture2D.meta");
        let yaml = fs::read_to_string(&asset_path).unwrap();
        assert!(yaml.contains("--- !u!28 &2800000"));
        assert!(
            yaml.contains("image data: 3"),
            "Texture2D writes its byte count"
        );
        assert!(
            yaml.contains("_typelessdata: 323334"),
            "Texture2D writes its inline bytes"
        );
        assert!(yaml.contains("offset: 0"));
        assert!(yaml.contains("size: 0"));
        assert!(!yaml.contains("texture.resS"));
        assert!(fs::read_to_string(&meta_path)
            .unwrap()
            .contains("NativeFormatImporter:"));
        assert!(
            fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt"))
                .unwrap()
                .contains("2022.3.22f1")
        );
        assert_eq!(fs::read(&serialized_path).unwrap(), serialized_bytes);
        assert_eq!(fs::read(&resource_path).unwrap(), b"0123456789");

        let error = super::run(
            [
                std::ffi::OsString::from("export-project"),
                missing_output_root.clone().into_os_string(),
                serialized_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("texture.resS"), "{error}");
        assert_eq!(fs::read_dir(&missing_output_root).unwrap().count(), 0);
        assert_eq!(fs::read(&serialized_path).unwrap(), serialized_bytes);

        input.files.extend([
            asset_path,
            meta_path,
            output_root.join("ProjectSettings/ProjectVersion.txt"),
            output_root.join("Packages/manifest.json"),
        ]);
    }

    #[test]
    fn export_project_resolves_streamed_texture2d_from_platform_data_path() {
        let scratch = TestInputDirectory::new_root();
        let platform_root = scratch.join("game");
        let data_directory = platform_root.join("game_Data");
        let executable = platform_root.join("game.exe");
        let serialized_path = data_directory.join("globalgamemanagers");
        let resource_path = data_directory.join("texture.resS");
        let output_root = scratch.join("project");
        fs::create_dir(&platform_root).unwrap();
        fs::create_dir(&data_directory).unwrap();
        fs::write(&executable, b"synthetic executable marker").unwrap();
        let serialized_bytes = make_streamed_texture2d_file("texture.resS", 2, 3);
        fs::write(&serialized_path, &serialized_bytes).unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        fs::create_dir(&output_root).unwrap();

        let asset_path = output_root.join("Assets/Texture2D/Texture2D.texture2D");
        let meta_path = output_root.join("Assets/Texture2D/Texture2D.texture2D.meta");
        let input = TestInputDirectory {
            directory: scratch,
            files: vec![
                executable,
                serialized_path.clone(),
                resource_path.clone(),
                asset_path.clone(),
                meta_path.clone(),
                output_root.join("ProjectSettings/ProjectVersion.txt"),
                output_root.join("Packages/manifest.json"),
            ],
            subdirectories: vec![
                platform_root,
                data_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Texture2D"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                input.directory.join("game").into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        let source_rows = include_str!("export/streamed_image_resource_fixture.txt");
        assert!(source_rows
            .lines()
            .any(|line| { line == "SOURCE_PROVIDER|calls=1|name=texture.resS" }));
        assert!(source_rows.lines().any(|line| {
            line == "SOURCE_STREAMED_YAML|class=28|data=323334|offset=0|size=0|path_empty=true"
        }));
        let yaml = fs::read_to_string(&asset_path).unwrap();
        assert!(yaml.contains("--- !u!28 &2800000"));
        assert!(
            yaml.contains("image data: 3"),
            "Texture2D writes its byte count"
        );
        assert!(
            yaml.contains("_typelessdata: 323334"),
            "Texture2D writes its inline bytes"
        );
        assert!(yaml.contains("offset: 0"));
        assert!(yaml.contains("size: 0"));
        assert!(!yaml.contains("texture.resS"));
        assert!(fs::read_to_string(&meta_path)
            .unwrap()
            .contains("NativeFormatImporter:"));
        assert!(
            fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt"))
                .unwrap()
                .contains("2022.3.22f1")
        );
        assert_eq!(fs::read(&serialized_path).unwrap(), serialized_bytes);
        assert_eq!(fs::read(&resource_path).unwrap(), b"0123456789");
    }

    #[test]
    fn export_project_dispatches_collections_and_writes_a_complete_unity_project() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();

        let main_path = directory.join("main.assets");
        fs::write(
            &main_path,
            make_generation_10_file_with_pointer(&["other.assets"], 1, 42),
        )
        .unwrap();
        let other_path = assets_directory.join("other.assets");
        fs::write(&other_path, make_generation_10_file()).unwrap();
        let texture_path = assets_directory.join("texture.assets");
        fs::write(&texture_path, make_texture2d_file()).unwrap();
        let material_path = assets_directory.join("material.assets");
        fs::write(
            &material_path,
            make_material_file_with_shader(42, "shader.assets"),
        )
        .unwrap();
        let mesh_path = assets_directory.join("mesh.assets");
        fs::write(&mesh_path, make_mesh_file()).unwrap();
        let player_path = assets_directory.join("player.assets");
        fs::write(&player_path, make_player_settings_file()).unwrap();
        let build_settings_path = assets_directory.join("settings.assets");
        fs::write(
            &build_settings_path,
            make_build_settings_file(&["Assets/Scenes/level0.unity"]),
        )
        .unwrap();
        let bundle_path = assets_directory.join("bundle.assets");
        fs::write(&bundle_path, make_asset_bundle_file()).unwrap();
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &shader_path,
            make_shader_file_with_path_id("Shader \"x\" { }\n", 42),
        )
        .unwrap();

        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![
                main_path.clone(),
                other_path.clone(),
                texture_path.clone(),
                material_path.clone(),
                mesh_path.clone(),
                player_path.clone(),
                build_settings_path.clone(),
                bundle_path.clone(),
                shader_path.clone(),
            ],
            // Drop removes directories in reverse order, so keep parents before children.
            subdirectories: vec![
                assets_directory.clone(),
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/GameObject"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Mesh"),
                output_root.join("Assets/Shader"),
                output_root.join("Assets/Texture2D"),
                output_root.join("AuxiliaryFiles"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        let mut status = Vec::new();
        super::export_project_to(
            &[main_path, assets_directory],
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .unwrap();
        let status = String::from_utf8(status).unwrap();

        let actual = project_tree(&output_root);
        assert_eq!(
            actual,
            vec![
                "Assets/GameObject/GameObject.asset".to_owned(),
                "Assets/GameObject/GameObject.asset.meta".to_owned(),
                "Assets/GameObject/GameObject_0.asset".to_owned(),
                "Assets/GameObject/GameObject_0.asset.meta".to_owned(),
                "Assets/Material/TestMat.mat".to_owned(),
                "Assets/Material/TestMat.mat.meta".to_owned(),
                "Assets/Mesh/Tri.asset".to_owned(),
                "Assets/Mesh/Tri.asset.meta".to_owned(),
                "Assets/Shader/Test_Simple.shader".to_owned(),
                "Assets/Shader/Test_Simple.shader.meta".to_owned(),
                "Assets/Texture2D/Texture2D.png".to_owned(),
                "Assets/Texture2D/Texture2D.png.meta".to_owned(),
                "AuxiliaryFiles/path_id_map.json".to_owned(),
                "Packages/manifest.json".to_owned(),
                "ProjectSettings/EditorSettings.asset".to_owned(),
                "ProjectSettings/ProjectSettings.asset".to_owned(),
                "ProjectSettings/ProjectVersion.txt".to_owned(),
            ],
            "actual project tree: {actual:#?}\nstatus:\n{status}"
        );
        let path_id_map = fs::read_to_string(output_root.join("AuxiliaryFiles/path_id_map.json"))
            .expect("project export writes the PathIdMap post-export file");
        assert!(path_id_map.contains("\"Type\": \"Mesh\""), "{path_id_map}");
        assert!(
            path_id_map.contains("\"Type\": \"Texture2D\""),
            "{path_id_map}"
        );
        let texture_image = fs::read(output_root.join("Assets/Texture2D/Texture2D.png"))
            .expect("a decodable embedded-TypeTree texture exports as an image");
        assert!(texture_image.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(fs::read_to_string(output_root.join("Assets/Texture2D/Texture2D.png.meta"))
            .unwrap()
            .contains("TextureImporter:"));
        assert!(
            path_id_map.contains("\"Type\": \"Shader\""),
            "{path_id_map}"
        );

        // The cross-file PPtr renders the target object's own collection GUID.
        let target_guid = meta_guid(&output_root.join("Assets/GameObject/GameObject_0.asset.meta"));
        let main_yaml = fs::read_to_string(output_root.join("Assets/GameObject/GameObject.asset"))
            .expect("cross-file asset is readable");
        assert!(main_yaml.contains("--- !u!1 &100000"), "{main_yaml}");
        assert!(
            main_yaml.contains(&format!("guid: {target_guid}")),
            "{main_yaml}"
        );

        // Native asset sidecars use AssetRipper's class-prefixed main export IDs. The
        // TextureImporter sidecar is checked above because it uses Unity's image importer.
        for (relative, main_id) in [
            ("Assets/GameObject/GameObject.asset.meta", 100_000),
            ("Assets/GameObject/GameObject_0.asset.meta", 100_000),
            ("Assets/Material/TestMat.mat.meta", 2_100_000),
            ("Assets/Mesh/Tri.asset.meta", 4_300_000),
        ] {
            let meta = fs::read_to_string(output_root.join(relative)).unwrap();
            assert!(
                meta.contains("NativeFormatImporter:"),
                "{relative}:\n{meta}"
            );
            assert!(
                meta.contains(&format!("mainObjectFileID: {main_id}")),
                "{relative}:\n{meta}"
            );
        }

        // The source selects ShaderImporter for decompiled text and Material shader pointers
        // use the shader collection's exported file ID/GUID with Meta pointer type 3.
        let shader_meta_path = "Assets/Shader/Test_Simple.shader.meta";
        let shader_meta = fs::read_to_string(output_root.join(shader_meta_path)).unwrap();
        assert!(shader_meta.contains("ShaderImporter:"), "{shader_meta}");
        assert!(shader_meta.contains("externalObjects: {}"), "{shader_meta}");
        assert!(shader_meta.contains("defaultTextures: []"), "{shader_meta}");
        assert!(
            shader_meta.contains("nonModifiableTextures: []"),
            "{shader_meta}"
        );
        assert!(!shader_meta.contains("mainObjectFileID:"), "{shader_meta}");
        let shader_guid = meta_guid(&output_root.join(shader_meta_path));
        assert_eq!(
            fs::read(output_root.join("Assets/Shader/Test_Simple.shader")).unwrap(),
            b"Shader \"x\" { }\n"
        );
        let material_yaml = fs::read_to_string(output_root.join("Assets/Material/TestMat.mat"))
            .expect("material YAML is readable");
        assert!(
            material_yaml.contains(&format!(
                "m_Shader: {{fileID: 4800000, guid: {shader_guid}, type: 3}}"
            )),
            "material did not preserve its cross-file Shader reference:\n{material_yaml}"
        );

        // The settings-file family writes one file per object with unique GUIDs.
        let mut guids: Vec<String> = [
            "Assets/GameObject/GameObject.asset.meta",
            "Assets/GameObject/GameObject_0.asset.meta",
            "Assets/Material/TestMat.mat.meta",
            "Assets/Mesh/Tri.asset.meta",
            shader_meta_path,
            "Assets/Texture2D/Texture2D.png.meta",
        ]
        .iter()
        .map(|relative| meta_guid(&output_root.join(relative)))
        .collect();
        guids.sort();
        let guid_count = guids.len();
        guids.dedup();
        assert_eq!(guids.len(), guid_count, "collection GUIDs must be unique");

        // The manager family has a fixed destination and no sidecar; the two non-exportable
        // collections wrote nothing and are reported explicitly.
        assert!(!output_root
            .join("ProjectSettings/ProjectSettings.asset.meta")
            .exists());
        assert!(!output_root.join("Assets/BuildSettings").exists());
        assert!(!output_root.join("Assets/AssetBundle").exists());
        assert!(
            status.contains("non-exportable baseline collection"),
            "{status}"
        );
        assert!(status.contains("EmptyExportCollection"), "{status}");
        assert!(
            !status.contains("generic asset-family renderer"),
            "this fixture's generic baseline routes should not be reported as specialized-renderer fallbacks:\n{status}"
        );
        assert!(
            !status.contains("ShaderExportCollection"),
            "shader used an unexpected generic fallback:\n{status}"
        );

        // Post-exporters: ProjectVersion and the default package manifest.
        assert!(
            fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt"))
                .unwrap()
                .contains("2019.4.0f1")
        );
        let manifest = fs::read_to_string(output_root.join("Packages/manifest.json")).unwrap();
        assert!(
            manifest.contains("\"com.unity.modules.androidjni\""),
            "{manifest}"
        );

        input.files.extend([
            output_root.join("Assets/GameObject/GameObject.asset"),
            output_root.join("Assets/GameObject/GameObject.asset.meta"),
            output_root.join("Assets/GameObject/GameObject_0.asset"),
            output_root.join("Assets/GameObject/GameObject_0.asset.meta"),
            output_root.join("Assets/Material/TestMat.mat"),
            output_root.join("Assets/Material/TestMat.mat.meta"),
            output_root.join("Assets/Mesh/Tri.asset"),
            output_root.join("Assets/Mesh/Tri.asset.meta"),
            output_root.join("Assets/Shader/Test_Simple.shader"),
            output_root.join("Assets/Shader/Test_Simple.shader.meta"),
            output_root.join("Assets/Texture2D/Texture2D.texture2D"),
            output_root.join("Assets/Texture2D/Texture2D.texture2D.meta"),
            output_root.join("ProjectSettings/ProjectSettings.asset"),
            output_root.join("ProjectSettings/ProjectVersion.txt"),
            output_root.join("Packages/manifest.json"),
        ]);
        input
            .subdirectories
            .push(output_root.join("Assets/Shader/Test"));
        drop(input);
    }

    #[test]
    fn export_project_yaml_shader_writes_source_versioned_patch_and_importer() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let shader_path = assets_directory.join("shader.assets");
        let script = "Shader \"x\" {\nSubShader {\nProgram \"vp\" {}\n}\n}\n";
        fs::write(&shader_path, make_shader_file_with_path_id(script, 42)).unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Shader"),
                output_root.join("Assets/Editor"),
                output_root.join("Assets/Editor/AssetRipperPatches"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };
        let mut settings = default_export_settings();
        settings.shader_export_mode = ShaderExportMode::Yaml;
        let mut status = Vec::new();
        super::export_project_to(&[shader_path], &output_root, false, &settings, &mut status)
            .unwrap();
        let status = String::from_utf8(status).unwrap();

        let asset_path = output_root.join("Assets/Shader/Test_Simple.asset");
        let meta_path = output_root.join("Assets/Shader/Test_Simple.asset.meta");
        assert!(asset_path.is_file());
        assert!(!output_root
            .join("Assets/Shader/Test_Simple.shader")
            .exists());
        assert!(
            fs::read_to_string(&asset_path)
                .unwrap()
                .contains("--- !u!48 &4800000"),
            "YAML Shader asset uses the Shader collection export ID"
        );
        let meta = fs::read_to_string(meta_path).unwrap();
        assert!(meta.contains("NativeFormatImporter:"), "{meta}");
        assert!(meta.contains("  name:\n"), "{meta}");
        assert!(!meta.contains("externalObjects:"), "{meta}");
        assert!(!meta.contains("mainObjectFileID:"), "{meta}");

        let patch_path =
            output_root.join("Assets/Editor/AssetRipperPatches/AvoidSavingYamlShaders.cs");
        assert!(patch_path.is_file());
        assert!(!output_root
            .join("Assets/Editor/AssetRipperPatches/YamlShaderPostprocessor.cs")
            .exists());
        assert!(status.contains("support-file count 1"), "{status}");

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_shader_importer_matches_the_reference_nonmodifiable_texture_map() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let expected = include_str!("export/shader_nonmodifiable_fixture.txt");
        let expected = expected
            .lines()
            .find_map(|line| line.strip_prefix("META|"))
            .expect("source fixture has a normalized importer row")
            .replace("\\n", "\n");
        let meta_path = output_root.join("Assets/Shader/Test_Simple.shader.meta");
        let meta = fs::read_to_string(meta_path).unwrap();
        let normalized = meta
            .lines()
            .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(normalized, expected);

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_shader_importer_maps_nonmodifiable_texture_pointers() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let shader_path = assets_directory.join("shader.assets");
        let texture_path = assets_directory.join("texture.assets");
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_pointer(1, 88, &["texture.assets"]),
        )
        .unwrap();
        fs::write(
            &texture_path,
            make_minimal_texture_file("ExternalTexture", 88),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![shader_path.clone(), texture_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Shader"),
                output_root.join("Assets/Texture2D"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[shader_path, texture_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let texture_meta =
            fs::read_to_string(output_root.join("Assets/Texture2D/ExternalTexture.texture2D.meta"))
                .unwrap();
        let texture_guid = texture_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Texture2D sidecar has a GUID");
        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        assert!(
            shader_meta.contains(&format!(
                "  - _MainTex: {{fileID: 2800000, guid: {texture_guid}, type: 2}}"
            )),
            "ShaderImporter texture entry points to the exported Texture2D sidecar: {shader_meta}"
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_yaml_matches_the_reference_2018_2_project_probe() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(&material_path, make_material_file_with_reference_2018_tpk()).unwrap();
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
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_preserves_original_streaming_tree_and_unreferenced_manager_assembly() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("export/post_export_source_2022_3_fixture.txt");
        let decode_hex = |hex: &str| {
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };
        let directory = TestInputDirectory::new_root();
        let game = directory.join("game");
        let data = game.join("game_Data");
        let streaming = data.join("StreamingAssets");
        let managed = data.join("Managed");
        fs::create_dir_all(&streaming).unwrap();
        fs::create_dir(&managed).unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: Vec::new(),
            subdirectories: vec![
                game.clone(),
                data.clone(),
                streaming.clone(),
                managed.clone(),
            ],
        };
        let executable = game.join("game.exe");
        fs::write(&executable, b"generic executable marker").unwrap();
        input.files.push(executable);
        let texture_fixture = include_str!("export/texture_project_source_2022_3_fixture.txt");
        let texture_hex = texture_fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|"))
            .unwrap()
            .split_once('|')
            .unwrap()
            .1;
        let texture = data.join("globalgamemanagers.assets");
        fs::write(&texture, decode_hex(texture_hex)).unwrap();
        input.files.push(texture);
        let assembly = managed.join("FixtureScripts.dll");
        fs::write(
            &assembly,
            include_bytes!("processing/managed_assembly_fixture.dll"),
        )
        .unwrap();
        input.files.push(assembly.clone());
        for line in fixture.lines() {
            if let Some(path) = line.strip_prefix("INPUT_DIR|") {
                let path = streaming.join(path);
                fs::create_dir_all(&path).unwrap();
                input.subdirectories.push(path);
            } else if let Some(file) = line.strip_prefix("INPUT_FILE|") {
                let (path, hex) = file.split_once('|').unwrap();
                let path = streaming.join(path);
                fs::write(&path, decode_hex(hex)).unwrap();
                input.files.push(path);
            }
        }
        let original = input
            .files
            .iter()
            .map(|path| (path.clone(), fs::read(path).unwrap()))
            .collect::<Vec<_>>();
        for (ignore, disable_scripts) in [(false, false), (true, false), (false, true)] {
            let output = input.directory.join(match (ignore, disable_scripts) {
                (true, _) => "ignored-project",
                (_, true) => "scripts-disabled-project",
                _ => "copied-project",
            });
            let mut settings =
                assetripper::export::settings_file::SerializedSettingsFile::from_defaults(
                    Default::default(),
                    Default::default(),
                    Default::default(),
                );
            settings.export.image_export_format = assetripper::export::settings::ImageExportFormat::Bmp;
            settings.import.streaming_assets_mode = if ignore {
                assetripper::export::settings::StreamingAssetsMode::Ignore
            } else {
                assetripper::export::settings::StreamingAssetsMode::Extract
            };
            if disable_scripts {
                settings.import.script_content_level =
                    assetripper::export::settings::ScriptContentLevel::Level0;
            }
            if ignore {
                let settings_path = input.directory.join("ignore.Settings.json");
                fs::write(
                    &settings_path,
                    assetripper::export::settings_file::write_settings_file(&settings).unwrap(),
                )
                .unwrap();
                input.files.push(settings_path.clone());
                super::export_project_from_arguments(
                    [
                        output.as_os_str().to_owned(),
                        std::ffi::OsString::from("--settings"),
                        settings_path.as_os_str().to_owned(),
                        game.as_os_str().to_owned(),
                    ]
                    .into_iter(),
                )
                .unwrap();
            } else {
                super::export_project_to_with_serialized_settings(
                    std::slice::from_ref(&game),
                    &output,
                    false,
                    &settings,
                    &mut Vec::new(),
                )
                .unwrap();
            }
            assert!(output
                .join("ProjectSettings/EditorSettings.asset")
                .is_file());
            assert_eq!(output.join("Assets/StreamingAssets").exists(), !ignore);
            for line in fixture.lines() {
                if let Some(file) =
                    line.strip_prefix("SOURCE_FILE|extract|ExportedProject/Assets/StreamingAssets/")
                {
                    let (path, hex) = file.split_once('|').unwrap();
                    if !ignore {
                        assert_eq!(
                            fs::read(output.join("Assets/StreamingAssets").join(path)).unwrap(),
                            decode_hex(hex),
                            "source bytes for {path}"
                        );
                    }
                } else if let Some(path) =
                    line.strip_prefix("SOURCE_DIR|extract|ExportedProject/Assets/StreamingAssets/")
                {
                    if !ignore {
                        assert!(output.join("Assets/StreamingAssets").join(path).is_dir());
                    }
                }
            }
            if disable_scripts {
                assert!(!output.join("AuxiliaryFiles/GameAssemblies").exists());
            } else {
                let saved =
                    fs::read(output.join("AuxiliaryFiles/GameAssemblies/FixtureScripts.dll")).unwrap();
                let source_saved_hex = fixture
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix(
                            "SOURCE_FILE|extract|AuxiliaryFiles/GameAssemblies/FixtureScripts.dll|",
                        )
                    })
                    .unwrap();
                let source_saved = decode_hex(source_saved_hex);
                let actual_model = ManagedAssembly::parse(
                    "FixtureScripts.dll",
                    &saved,
                    ManagedAssemblyLimits::default(),
                )
                .unwrap();
                let source_model = ManagedAssembly::parse(
                    "FixtureScripts.dll",
                    &source_saved,
                    ManagedAssemblyLimits::default(),
                )
                .unwrap();
                assert_eq!(actual_model.types, source_model.types);
            }
            assert!(
                !output.join("Assets/Plugins/FixtureScripts.dll").exists(),
                "an unreferenced manager assembly still has auxiliary output"
            );
            assert!(!output
                .join("AuxiliaryFiles/GameAssemblies/FixtureScripts.dll.meta")
                .exists());
            for (path, bytes) in &original {
                assert_eq!(
                    &fs::read(path).unwrap(),
                    bytes,
                    "original {}",
                    path.display()
                );
            }
            input
                .files
                .extend(project_tree(&output).iter().map(|path| output.join(path)));
            let mut pending = vec![output];
            while let Some(path) = pending.pop() {
                input.subdirectories.push(path.clone());
                for entry in fs::read_dir(path).unwrap() {
                    let entry = entry.unwrap();
                    if entry.file_type().unwrap().is_dir() {
                        pending.push(entry.path());
                    }
                }
            }
        }
        // Parent-first cleanup ordering includes all copied empty directories.
        input
            .subdirectories
            .sort_by_key(|path| path.components().count());
    }
