    use super::{
        asset_family_fallback, export_workspace_yaml_project_with_assembly_catalog,
        export_workspace_yaml_project_with_settings, new_collection_guid, package_manifest_json,
        plugin_importer_yaml, source_reference_assembly_maps, WorkspaceProjectExportLimits,
    };
    use crate::export::settings::ExportDefaults;
    use crate::export::settings_file::ExportSettingsFile;
    use crate::export::workspace_yaml::tests::{
        load_workspace_bytes, load_workspace_bytes_with_source_factory,
        load_workspace_named_bytes_with_source_factory, make_existing_prefab_instance_file,
        make_game_object_without_transform_file,
    };
    use crate::io::dependency::DependencyFilePath;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::io::platform_structure::PlatformResourceProvider;
    use crate::processing::asset_factory::FactoryUnityVersion;
    use crate::processing::managed_assembly::{
        ManagedAssembly, ManagedAssemblyCatalog, ManagedAssemblyCatalogLimits,
        ManagedAssemblyLimits,
    };
    use crate::processing::managed_assembly_schema::ManagedSchemaLimits;
    use crate::processing::scene_naming::SceneUnityVersion;
    use crate::resource::RootResourceFileCatalog;
    use crate::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
    use crate::serialized_file::reference_graph::{
        MultiFilePPtrResolution, MultiFileReferenceGraphLimits,
    };
    use crate::serialized_file::workspace::{
        SerializedFileWorkspace, SerializedFileWorkspaceLimits,
    };

    #[test]
    fn project_collection_guids_are_nonzero_and_unique_per_collection() {
        let first = new_collection_guid(0);
        let second = new_collection_guid(1);
        assert_ne!(first, [0; 16]);
        assert_ne!(second, [0; 16]);
        assert_ne!(first, second);
    }

    #[test]
    fn project_manifest_keeps_the_android_jni_version_gate() {
        let before = SceneUnityVersion::parse("2019.1.0f1").unwrap();
        let after = SceneUnityVersion::parse("2019.2.0f1").unwrap();
        assert!(!package_manifest_json(before).contains("com.unity.modules.androidjni"));
        assert!(package_manifest_json(after).contains("\"com.unity.modules.androidjni\""));
    }

    #[test]
    fn plugin_importer_layout_tracks_the_source_class_version_transition() {
        let cases = [
            (
                "2019.1.0f1",
                include_str!(
                    "../processing/managed_assembly_unsafe_forwarder_2019a1_source_meta.txt"
                ),
            ),
            (
                "2023.3.0a14",
                include_str!(
                    "../processing/managed_assembly_unsafe_forwarder_2023a14_source_meta.txt"
                ),
            ),
            (
                "2023.3.0a15",
                include_str!(
                    "../processing/managed_assembly_unsafe_forwarder_2023a15_source_meta.txt"
                ),
            ),
            (
                "2023.3.0a16",
                include_str!(
                    "../processing/managed_assembly_unsafe_forwarder_2023a16_source_meta.txt"
                ),
            ),
        ];
        for (version, source_meta) in cases {
            let importer = plugin_importer_yaml(version).expect("source-backed importer version");
            let rust_importer = crate::export::unity_yaml::write_unity_yaml_mapping(
                &importer,
                crate::export::unity_yaml::UnityYamlWriteLimits::default(),
            )
            .expect("PluginImporter YAML serializes");
            let source_importer = source_meta
                .replace("\r\n", "\n")
                .split_once("PluginImporter:\n")
                .expect("source metadata has a PluginImporter body")
                .1
                .to_owned();
            let indented_rust_importer = rust_importer
                .lines()
                .map(|line| format!("  {line}\n"))
                .collect::<String>();
            assert_eq!(indented_rust_importer, source_importer, "Unity {version}");
        }
    }

    #[test]
    fn project_reference_map_selects_the_manager_mscorlib_version() {
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "mscorlib.dll",
                include_bytes!("../processing/managed_assembly_mscorlib2_fixture.dll"),
                ManagedAssemblyLimits::default(),
            )
            .unwrap();
        assert_eq!(catalog.assembly("mscorlib").unwrap().version_major, Some(2));

        let version = FactoryUnityVersion::parse("2019.1.0f1").unwrap();
        let manager_maps = source_reference_assembly_maps(true, Some(&catalog))
            .unwrap()
            .unwrap();
        assert!(manager_maps
            .guid_for(&version, "Mono.Cecil")
            .unwrap()
            .is_some());

        let fallback_maps = source_reference_assembly_maps(true, None).unwrap().unwrap();
        assert_eq!(
            fallback_maps.guid_for(&version, "Mono.Cecil").unwrap(),
            None
        );
        assert!(source_reference_assembly_maps(false, Some(&catalog))
            .unwrap()
            .is_none());
    }

    #[test]
    fn baseline_yaml_and_shader_collections_do_not_report_a_generic_fallback() {
        for name in [
            "AssetExportCollection",
            "ScriptableObjectExportCollection",
            "ScriptableObjectGroupExportCollection",
            "YamlStreamedAssetExportCollection",
            "ShaderExportCollection",
        ] {
            assert!(asset_family_fallback(name).is_none(), "{name}");
        }
        for name in [
            "TextAssetExportCollection",
            "AudioClipExportCollection",
            "TerrainYamlExportCollection",
        ] {
            assert!(asset_family_fallback(name).is_some(), "{name}");
        }
    }

    #[test]
    fn project_export_keeps_source_managed_registry_cycles_and_cross_file_pointer() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        for file_name in ["managed.assets", "target.assets"] {
            let bytes = match file_name {
                "managed.assets" => {
                    include_bytes!("managed_reference_project_fixture/managed.assets").as_slice()
                }
                "target.assets" => {
                    include_bytes!("managed_reference_project_fixture/target.assets").as_slice()
                }
                _ => unreachable!(),
            };
            filesystem
                .write_all_bytes(&format!("/game/{file_name}"), bytes)
                .unwrap();
        }
        let inputs = [
            DependencyFilePath {
                file_name: "managed.assets".to_owned(),
                virtual_path: "/game/managed.assets".to_owned(),
            },
            DependencyFilePath {
                file_name: "target.assets".to_owned(),
                virtual_path: "/game/target.assets".to_owned(),
            },
        ];
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        assert_eq!(workspace.files().len(), 2);
        assert_eq!(
            workspace
                .files()
                .iter()
                .map(|file| file.metadata.objects.len())
                .sum::<usize>(),
            2
        );
        let root_file_index = workspace
            .files()
            .iter()
            .position(|file| file.file_name == "managed.assets")
            .unwrap();
        let target_file_index = workspace
            .files()
            .iter()
            .position(|file| file.file_name == "target.assets")
            .unwrap();

        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(
            objects.get(root_file_index, 0).unwrap().factory.class_id,
            114
        );
        assert_eq!(
            objects.get(target_file_index, 0).unwrap().factory.class_id,
            114
        );

        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let pptr = reference_graph
            .references()
            .iter()
            .find(|reference| {
                reference.source_file_index == root_file_index
                    && reference.source_path_id == 42
                    && reference.field_path == "references.RefIds[0].data.m_Asset"
            })
            .expect("NodeA's serialized PPtr is scanned from the managed payload");
        assert_eq!(pptr.file_id, 1);
        assert_eq!(pptr.path_id, 900);
        assert_eq!(
            pptr.resolution,
            MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index: 0,
            }
        );

        let managed_graph = reference_graph
            .managed_graph_for_file(root_file_index)
            .expect("managed graph exists for the source file");
        assert_eq!(managed_graph.managed_objects().len(), 2);
        assert_eq!(managed_graph.managed_objects()[0].reference_id, 100);
        assert_eq!(managed_graph.managed_objects()[0].class_name, "NodeA");
        assert_eq!(managed_graph.managed_objects()[1].reference_id, 200);
        assert_eq!(managed_graph.managed_objects()[1].class_name, "NodeB");
        assert_eq!(managed_graph.managed_references().len(), 3);
        assert!(managed_graph.managed_references().iter().all(|reference| matches!(
            reference.resolution,
            crate::serialized_file::reference_graph::SerializedManagedReferenceResolution::Object { .. }
        )));

        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root =
            crate::test_support::unique_test_directory("project-managed-reference-e2e");
        let report = export_workspace_yaml_project_with_settings(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &reference_graph,
            &settings,
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();

        let root_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
        let target_path = output_root.join("Assets/MonoBehaviour/ReferencedRoot.asset");
        let target_meta_path = output_root.join("Assets/MonoBehaviour/ReferencedRoot.asset.meta");
        let rust_root_yaml = std::fs::read_to_string(&root_path).unwrap();
        let source_root_yaml =
            include_str!("managed_reference_project_fixture/source_ManagedRoot.asset");
        let source_target_meta =
            include_str!("managed_reference_project_fixture/source_ReferencedRoot.asset.meta");
        let rust_target_meta = std::fs::read_to_string(&target_meta_path).unwrap();
        let guid_from_meta = |text: &str| -> String {
            text.lines()
                .find_map(|line| line.strip_prefix("guid: "))
                .expect("Unity .meta has a GUID")
                .to_owned()
        };
        let source_target_guid = guid_from_meta(source_target_meta);
        let rust_target_guid = guid_from_meta(&rust_target_meta);
        let normalized_source_yaml = source_root_yaml.replace(&source_target_guid, "<TARGET_GUID>");
        let normalized_rust_yaml = rust_root_yaml.replace(&rust_target_guid, "<TARGET_GUID>");
        assert_eq!(
            normalized_source_yaml.trim_end(),
            normalized_rust_yaml.trim_end(),
            "managed-reference MonoBehaviour YAML should match the source after GUID normalization"
        );
        let source_registry = source_root_yaml
            .split_once("  m_Root:\n")
            .expect("source YAML includes the managed-reference body")
            .1
            .replace(&source_target_guid, "<TARGET_GUID>");
        let rust_registry = rust_root_yaml
            .split_once("  m_Root:\n")
            .expect("Rust YAML includes the managed-reference body")
            .1
            .replace(&rust_target_guid, "<TARGET_GUID>");
        assert_eq!(source_registry.trim_end(), rust_registry.trim_end());
        assert!(rust_root_yaml.contains(&format!(
            "m_Asset: {{fileID: 11400000, guid: {rust_target_guid}, type: 2}}"
        )));
        assert!(rust_root_yaml.contains("        m_Link:\n          rid: 200\n"));
        assert!(rust_root_yaml.contains("        m_Link:\n          rid: 100\n"));
        let target_yaml = std::fs::read_to_string(target_path).unwrap();
        let source_target_yaml =
            include_str!("managed_reference_project_fixture/source_ReferencedRoot.asset");
        assert_eq!(
            source_target_yaml.trim_end(),
            target_yaml.trim_end(),
            "referenced MonoBehaviour YAML should match the source"
        );
        assert!(target_yaml.contains("m_Name: ReferencedRoot"));
        assert!(target_yaml.contains("m_Health: 789"));
        assert_eq!(report.asset_file_count, 2);
        assert_eq!(report.meta_file_count, 2);
        assert_eq!(report.exported_object_count, 2);
        assert!(report.processing_warnings.is_empty());
        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn project_export_writes_a_processed_prefab_collection_with_generated_transform() {
        let bytes = make_game_object_without_transform_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = ObjectWorkspace::load_from_vfs_with_layout_source(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
            &crate::processing::asset_factory::FileTypeTreeTestLayouts,
        )
        .unwrap();
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root = crate::test_support::unique_test_directory("project-prefab-e2e");

        let report = export_workspace_yaml_project_with_settings(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &reference_graph,
            &settings,
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();

        let prefab_path = output_root.join("Assets/GameObject/Root.prefab");
        let prefab_meta_path = output_root.join("Assets/GameObject/Root.prefab.meta");
        let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
        let prefab_meta = std::fs::read_to_string(&prefab_meta_path).unwrap();
        let game_object_id = yaml_document_file_id(&prefab_yaml, 1);
        let transform_id = yaml_document_file_id(&prefab_yaml, 4);
        assert!(prefab_yaml.contains("m_Name: Root"), "{prefab_yaml}");
        assert!(prefab_yaml.contains("m_LocalPosition:"), "{prefab_yaml}");
        assert!(
            prefab_yaml.contains(&format!("component: {{fileID: {transform_id}}}")),
            "{prefab_yaml}"
        );
        assert!(
            prefab_yaml.contains(&format!("m_GameObject: {{fileID: {game_object_id}}}")),
            "{prefab_yaml}"
        );
        assert!(!prefab_yaml.contains("!u!1001"), "{prefab_yaml}");
        assert!(prefab_meta.contains("PrefabImporter:"), "{prefab_meta}");
        assert!(prefab_meta.contains("externalObjects: {}"), "{prefab_meta}");
        assert!(prefab_meta.contains("userData:\n"), "{prefab_meta}");
        assert!(prefab_meta.contains("assetBundleName:\n"), "{prefab_meta}");
        assert!(
            prefab_meta.contains("assetBundleVariant:\n"),
            "{prefab_meta}"
        );
        assert!(!output_root.join("Assets/PrefabInstance").exists());
        assert_eq!(report.asset_file_count, 1);
        assert_eq!(report.meta_file_count, 1);
        assert_eq!(report.collection_count, 1);
        assert_eq!(report.exported_object_count, 2);

        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn project_export_matches_source_for_a_generated_legacy_prefab() {
        let bytes = include_bytes!("legacy_prefab_2017_4_input.unity");
        let (filesystem, workspace) =
            load_workspace_named_bytes_with_source_factory("LegacyRoot.unity", bytes);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.objects().len(), 2);
        assert!(objects.objects().iter().all(|object| matches!(
            &object.factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(_)
        )));
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root = crate::test_support::unique_test_directory("project-legacy-prefab-e2e");

        let report = export_workspace_yaml_project_with_settings(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &reference_graph,
            &settings,
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();

        let prefab_path = output_root.join("Assets/GameObject/LegacyRoot.prefab");
        let prefab_meta_path = output_root.join("Assets/GameObject/LegacyRoot.prefab.meta");
        let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
        let expected_yaml = include_str!("legacy_prefab_source_2017_4_fixture.yaml");
        assert_eq!(
            prefab_yaml.replace("\r\n", "\n"),
            expected_yaml.replace("\r\n", "\n")
        );
        let prefab_meta = std::fs::read_to_string(&prefab_meta_path).unwrap();
        assert!(
            prefab_meta.contains("NativeFormatImporter:"),
            "{prefab_meta}"
        );
        assert!(
            prefab_meta.contains("mainObjectFileID: 100100000"),
            "{prefab_meta}"
        );
        assert!(!prefab_meta.contains("PrefabImporter:"), "{prefab_meta}");
        assert_eq!(report.asset_file_count, 1);
        assert_eq!(report.meta_file_count, 1);
        assert_eq!(report.post_export_file_count, 1);
        assert_eq!(report.collection_count, 1);
        assert_eq!(report.exported_object_count, 3);
        assert!(report.processing_warnings.is_empty());

        let path_id_map =
            std::fs::read(output_root.join("AuxiliaryFiles/path_id_map.json")).unwrap();
        let expected_path_id_map = include_str!("legacy_prefab_source_2017_4_probe_fixture.txt")
            .lines()
            .find_map(|line| line.strip_prefix("FILE|AuxiliaryFiles/path_id_map.json|"))
            .expect("frozen source fixture records the PathIdMap output")
            .split('|')
            .collect::<Vec<_>>();
        assert_eq!(path_id_map.len().to_string(), expected_path_id_map[0]);
        let expected_empty_map = crate::export::path_id_map::serialize_path_id_map(
            &crate::export::path_id_map::SerializedGameInfo {
                files: vec![crate::export::path_id_map::SerializedFileInfo {
                    name: Some("legacyroot.unity".to_owned()),
                    assets: Vec::new(),
                }],
            },
        );
        assert_eq!(
            path_id_map,
            expected_empty_map.as_bytes(),
            "empty PathIdMap body differs from the source fixture's collection name and shape"
        );

        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn project_export_matches_source_for_a_no_tree_generated_prefab_hierarchy() {
        let fixture = include_str!("no_tree_prefab_source_2019_4_input.hex");
        let hex = fixture
            .lines()
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .expect("source-authored input fixture has a hex payload");
        assert_eq!(hex.len() % 2, 0, "source fixture hex has an odd length");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("source fixture contains ASCII hexadecimal bytes")
            })
            .collect::<Vec<_>>();
        assert_eq!(bytes.len(), 360);

        let (filesystem, workspace) =
            load_workspace_named_bytes_with_source_factory("NoTreeRoot.unity", &bytes);
        let serialized_file = workspace.files().first().expect("one source file");
        assert_eq!(serialized_file.metadata.objects.len(), 2);
        assert!(serialized_file
            .metadata
            .types
            .iter()
            .all(|serialized_type| serialized_type
                .type_tree
                .as_ref()
                .is_none_or(|tree| tree.nodes.is_empty())));
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.objects().len(), 2);
        assert!(objects.objects().iter().all(|object| {
            matches!(
                &object.factory.kind,
                crate::processing::asset_factory::FactoryAssetKind::Generated(_)
            ) && object.recovered_type_tree.is_some()
                && object.payload.is_ok()
        }));
        assert_eq!(objects.get(0, 0).unwrap().factory.name, "NoTreeRoot");

        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        assert!(reference_graph.references().iter().any(|reference| {
            reference.source_path_id == 10
                && reference.path_id == 11
                && reference.field_path == "m_Component[0].component"
                && matches!(
                    reference.resolution,
                    MultiFilePPtrResolution::Object {
                        target_file_index: 0,
                        target_object_index: 1,
                    }
                )
        }));
        assert!(reference_graph.references().iter().any(|reference| {
            reference.source_path_id == 11
                && reference.path_id == 10
                && reference.field_path == "m_GameObject"
                && matches!(
                    reference.resolution,
                    MultiFilePPtrResolution::Object {
                        target_file_index: 0,
                        target_object_index: 0,
                    }
                )
        }));

        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root = crate::test_support::unique_test_directory("project-no-tree-prefab-e2e");
        let report = export_workspace_yaml_project_with_settings(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &reference_graph,
            &settings,
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();

        let prefab_path = output_root.join("Assets/GameObject/NoTreeRoot.prefab");
        let prefab_meta_path = output_root.join("Assets/GameObject/NoTreeRoot.prefab.meta");
        let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
        let source_yaml = include_str!("no_tree_prefab_source_2019_4.yaml");
        assert_eq!(
            prefab_yaml.replace("\r\n", "\n"),
            source_yaml.replace("\r\n", "\n")
        );
        assert!(!output_root.join("Assets/PrefabInstance").exists());
        let prefab_meta = std::fs::read_to_string(prefab_meta_path).unwrap();
        let source_meta = include_str!("no_tree_prefab_source_2019_4.meta.yaml");
        let normalize_meta = |text: &str| {
            text.lines()
                .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(normalize_meta(&prefab_meta), normalize_meta(source_meta));
        assert_eq!(report.asset_file_count, 1);
        assert_eq!(report.meta_file_count, 1);
        assert_eq!(report.collection_count, 1);
        assert_eq!(report.exported_object_count, 2);
        assert!(report.processing_warnings.is_empty());

        std::fs::remove_dir_all(output_root).unwrap();
    }

    /// Reads one `KEY|field|field...` row from the frozen complex managed-schema project fixture.
    fn managed_schema_fixture_row(key: &str) -> Vec<String> {
        include_str!("managed_schema_project_fixture.txt")
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}|")))
            .unwrap_or_else(|| panic!("frozen source fixture has no {key} row"))
            .split('|')
            .map(str::to_owned)
            .collect()
    }

    fn managed_schema_fixture_hex() -> Vec<u8> {
        let hex = &managed_schema_fixture_row("INPUT_HEX")[0];
        assert_eq!(
            hex.len() % 2,
            0,
            "frozen source fixture hex has an odd length"
        );
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("frozen source fixture holds ASCII hexadecimal bytes")
            })
            .collect()
    }

    fn managed_reference_schema_fixture_row(key: &str) -> Vec<String> {
        managed_reference_schema_fixture_row_from(
            include_str!("../processing/managed_reference_schema_project_fixture.txt"),
            key,
        )
    }

    fn managed_reference_schema_fixture_row_from(fixture: &str, key: &str) -> Vec<String> {
        fixture
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}|")))
            .unwrap_or_else(|| panic!("frozen source fixture has no {key} row"))
            .split('|')
            .map(str::to_owned)
            .collect()
    }

    fn managed_reference_schema_fixture_input(file_name: &str) -> Vec<u8> {
        managed_reference_schema_fixture_input_from(
            include_str!("../processing/managed_reference_schema_project_fixture.txt"),
            file_name,
        )
    }

    fn managed_reference_schema_fixture_input_from(fixture: &str, file_name: &str) -> Vec<u8> {
        let prefix = format!("INPUT_HEX|{file_name}|");
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("frozen source fixture has no input named {file_name}"));
        managed_reference_schema_decode_hex(hex)
    }

    fn managed_reference_schema_decode_hex(hex: &str) -> Vec<u8> {
        assert_eq!(hex.len() % 2, 0, "source fixture hex has an even length");
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("source fixture contains valid hexadecimal")
            })
            .collect()
    }

    fn decode_base64(value: &str) -> Vec<u8> {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        assert_eq!(value.len() % 4, 0, "base64 length is a multiple of four");
        let mut bytes = Vec::with_capacity(value.len() / 4 * 3);
        let chunks = value.as_bytes().chunks_exact(4);
        let chunk_count = chunks.len();
        for (chunk_index, chunk) in chunks.enumerate() {
            let last_chunk = chunk_index + 1 == chunk_count;
            let padding = match &chunk[2..] {
                b"==" => 2,
                [_, b'='] => 1,
                _ => 0,
            };
            assert!(padding == 0 || last_chunk, "base64 padding is final");
            assert!(
                chunk[0] != b'=' && chunk[1] != b'=',
                "base64 has two leading sextets"
            );
            assert!(
                chunk[2] != b'=' || chunk[3] == b'=',
                "base64 padding is contiguous"
            );
            let decode = |byte: u8| {
                ALPHABET
                    .iter()
                    .position(|candidate| *candidate == byte)
                    .expect("frozen source fixture base64 uses the standard alphabet")
                    as u8
            };
            let first = decode(chunk[0]);
            let second = decode(chunk[1]);
            let third = if chunk[2] == b'=' {
                0
            } else {
                decode(chunk[2])
            };
            let fourth = if chunk[3] == b'=' {
                0
            } else {
                decode(chunk[3])
            };
            if padding == 2 {
                assert_eq!(second & 0x0f, 0, "unused base64 bits are zero");
            } else if padding == 1 {
                assert_eq!(third & 0x03, 0, "unused base64 bits are zero");
            }
            bytes.push((first << 2) | (second >> 4));
            if padding < 2 {
                bytes.push((second << 4) | (third >> 2));
            }
            if padding == 0 {
                bytes.push((third << 6) | fourth);
            }
        }
        bytes
    }

    fn normalized_meta_text(text: &str) -> String {
        text.lines()
            .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn project_export_matches_source_for_a_complex_no_type_tree_managed_schema() {
        // The frozen source snapshot exported a source-authored Unity 2019.4.40f1 (generation 21)
        // SerializedFile that carries a no-TypeTree MonoBehaviour with inherited scalar fields, an
        // int array, a `List<float>`, a nested serializable struct with a string, a serialized
        // private bool, an enum, and a `UnityEngine.Object` pointer to a sibling TextAsset. This
        // regresses the whole complex no-TypeTree schema/export route against those source rows.
        //
        // The only permitted normalization is the generated TextAsset GUID, which the source mints
        // per export; see `managed_schema_project_fixture.txt`. The source additionally wrote
        // `ProjectSettings/EditorSettings.asset` and a manager-resolved `System.Runtime.CompilerServices.Unsafe.dll`;
        // Rust still does not emit either, so the complete file tree is deliberately not asserted
        // here and those gaps stay open under `EXPORT-UNITY-001`.
        let input_bytes = managed_schema_fixture_hex();
        let assembly_bytes =
            include_bytes!("../processing/managed_assembly_schema_complex_fixture.dll");
        let assembly_row = managed_schema_fixture_row("ASSEMBLY");
        assert_eq!(assembly_row[0], "ComplexFixtureScripts.dll");
        assert_eq!(
            assembly_row[1].parse::<usize>().unwrap(),
            assembly_bytes.len()
        );

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem
            .write_all_bytes("/game/managed_schema.assets", &input_bytes)
            .unwrap();
        filesystem
            .write_all_bytes("/game/ComplexFixtureScripts.dll", assembly_bytes)
            .unwrap();
        let inputs = [
            DependencyFilePath {
                file_name: "managed_schema.assets".to_owned(),
                virtual_path: "/game/managed_schema.assets".to_owned(),
            },
            DependencyFilePath {
                file_name: "ComplexFixtureScripts.dll".to_owned(),
                virtual_path: "/game/ComplexFixtureScripts.dll".to_owned(),
            },
        ];
        let (catalog, assembly_report) = ManagedAssemblyCatalog::load_from_vfs(
            &filesystem,
            &inputs,
            ManagedAssemblyCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(assembly_report.loaded_count, 1, "{assembly_report:?}");
        assert_eq!(assembly_report.candidate_count, 2, "{assembly_report:?}");
        assert!(
            assembly_report.skipped_non_cli.is_empty(),
            "{assembly_report:?}"
        );
        assert_eq!(
            assembly_report.processed_bytes,
            u64::try_from(assembly_bytes.len()).unwrap()
        );
        assert!(catalog.contains_assembly("ComplexFixtureScripts"));
        assert!(catalog.is_valid(
            &crate::processing::script_identity::ScriptIdentifier::try_new(
                "ComplexFixtureScripts",
                "Game",
                "RichController",
            )
            .unwrap()
        ));

        let mut workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs[..1],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let mut objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let recovery = objects
            .recover_mono_behaviour_structures(
                &filesystem,
                &mut workspace,
                &catalog,
                ObjectWorkspaceLimits::default(),
                ManagedSchemaLimits::default(),
            )
            .unwrap();
        assert_eq!(
            recovery.behaviours.len(),
            1,
            "one MonoBehaviour is recovered"
        );
        assert!(matches!(
            recovery.behaviours[0].outcome,
            crate::serialized_file::object_workspace::ManagedBehaviourRecoveryOutcome::Recovered { .. }
        ));
        // The MonoScript, the assembly-backed MonoBehaviour, and the sibling TextAsset all decode
        // through the recovered source structures on this route.
        let object_report = objects.report();
        assert_eq!(
            object_report.object_count, 3,
            "all input objects are reported"
        );
        assert_eq!(object_report.decoded_objects, 3, "all objects are decoded");
        assert_eq!(
            object_report.undecoded_objects, 0,
            "no object remains opaque"
        );
        assert_eq!(object_report.total_undecoded_raw_bytes, 0);
        assert!(object_report.is_complete());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());

        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root = crate::test_support::unique_test_directory("project-managed-schema-e2e");
        let report = export_workspace_yaml_project_with_assembly_catalog(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &reference_graph,
            &settings,
            true,
            Some(&catalog),
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();
        assert!(report.processing_warnings.is_empty(), "{report:?}");
        // The MonoScript supplies the schema and the assembly is a supporting file; only the
        // recovered MonoBehaviour and TextAsset count as exported SerializedFile objects.
        assert_eq!(report.exported_object_count, 2);
        assert_eq!(report.asset_file_count, 3);

        // The MonoBehaviour document must be byte-identical to the frozen source output once the
        // generated TextAsset GUID is substituted for the source GUID the source minted for it.
        let text_asset_row = managed_schema_fixture_row("TARGET");
        let source_text_asset_guid = managed_schema_fixture_row("TARGET_GUID")[0].clone();
        let source_text_asset_bytes = decode_base64(&text_asset_row[3]);
        let rust_text_asset_meta =
            std::fs::read_to_string(output_root.join("Assets/TextAsset/Target.txt.meta")).unwrap();
        let rust_text_asset_guid = rust_text_asset_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("Rust TextAsset export carries a GUID")
            .to_owned();
        assert_ne!(
            rust_text_asset_guid, source_text_asset_guid,
            "each export mints its own TextAsset GUID"
        );

        let yaml_row = managed_schema_fixture_row("YAML");
        let source_yaml = String::from_utf8(decode_base64(&yaml_row[3])).unwrap();
        let normalized_source_yaml =
            source_yaml.replace(&source_text_asset_guid, &rust_text_asset_guid);
        let rust_yaml =
            std::fs::read_to_string(output_root.join("Assets/MonoBehaviour/RichRoot.asset"))
                .unwrap();
        assert_eq!(
            rust_yaml.replace("\r\n", "\n"),
            normalized_source_yaml.replace("\r\n", "\n")
        );
        assert_eq!(
            yaml_row[1].parse::<usize>().unwrap(),
            source_yaml.len(),
            "the frozen source MonoBehaviour document length is pinned"
        );
        // The nested `UnityEngine.Object` pointer must resolve as a Meta asset, matching
        // `TextAssetExporter`'s inherited `BinaryAssetExporter.ToExportType` of `AssetType.Meta`.
        assert!(
            rust_yaml.contains(&format!(
                "  target: {{fileID: 4900000, guid: {rust_text_asset_guid}, type: 3}}"
            )),
            "{rust_yaml}"
        );
        assert!(
            rust_yaml.contains(
                "m_Script: {fileID: -915535769, guid: b19bcd3f8dffca40198d81b88e4464bb, type: 3}"
            ),
            "{rust_yaml}"
        );

        // TextAsset content and its importer sidecar must match the source output exactly.
        assert_eq!(
            std::fs::read(output_root.join("Assets/TextAsset/Target.txt")).unwrap(),
            source_text_asset_bytes
        );
        assert_eq!(
            text_asset_row[1].parse::<usize>().unwrap(),
            source_text_asset_bytes.len()
        );
        let source_target_meta =
            String::from_utf8(decode_base64(&managed_schema_fixture_row("TARGET_META")[0]))
                .unwrap();
        assert_eq!(
            normalized_meta_text(&rust_text_asset_meta),
            normalized_meta_text(&source_target_meta)
        );

        // The source reserializes manager-backed assemblies after clearing its stream cache.
        let exported_assembly =
            std::fs::read(output_root.join("Assets/Plugins/ComplexFixtureScripts.dll")).unwrap();
        let source_saved_assembly = include_bytes!(
            "../processing/managed_assembly_schema_complex_source_saved_fixture.dll"
        );
        let exported_assembly_model = ManagedAssembly::parse(
            "ComplexFixtureScripts.dll",
            &exported_assembly,
            ManagedAssemblyLimits::default(),
        )
        .expect("exported complex assembly parses");
        let source_assembly_model = ManagedAssembly::parse(
            "ComplexFixtureScripts.dll",
            source_saved_assembly,
            ManagedAssemblyLimits::default(),
        )
        .expect("source-saved complex assembly parses");
        assert_eq!(
            exported_assembly_model.types, source_assembly_model.types,
            "project export preserves the source-saved managed type model"
        );
        assert_ne!(exported_assembly, assembly_bytes);
        let rust_plugin_meta = std::fs::read_to_string(
            output_root.join("Assets/Plugins/ComplexFixtureScripts.dll.meta"),
        )
        .unwrap();
        let source_plugin_meta =
            String::from_utf8(decode_base64(&managed_schema_fixture_row("PLUGIN_META")[0]))
                .unwrap();
        assert_eq!(
            normalized_meta_text(&rust_plugin_meta),
            normalized_meta_text(&source_plugin_meta)
        );

        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn project_export_recovers_assembly_resolved_managed_references_without_file_typetrees() {
        let source_fixture =
            include_str!("../processing/managed_reference_schema_project_fixture.txt");
        let input_bytes = managed_reference_schema_fixture_input("managed_reference_schema.assets");
        let target_input_bytes = managed_reference_schema_fixture_input("target.assets");
        let assembly_bytes = include_bytes!("../processing/managed_reference_schema_fixture.dll");
        let assembly_row = managed_reference_schema_fixture_row("ASSEMBLY");
        assert_eq!(assembly_row[0], "ManagedReferenceFixtureScripts.dll");
        assert_eq!(
            assembly_row[1].parse::<usize>().unwrap(),
            assembly_bytes.len()
        );
        assert_eq!(
            assembly_row[2],
            "1B20CEFF8CE7DE3E996D6497F3F7892B6472D53261DCED73F12D623FFDE902BB"
        );
        let source_assembly_output_row = managed_reference_schema_fixture_row("ASSEMBLY_OUTPUT");
        assert_eq!(
            source_assembly_output_row[0],
            "ManagedReferenceFixtureScripts.dll"
        );
        assert_eq!(
            source_assembly_output_row[1],
            assembly_bytes.len().to_string()
        );
        assert_eq!(
            source_assembly_output_row[2],
            "1E8A6019535D7DF90091471C9C9734575BEA84EE2925F5B3C67BB88931780886"
        );
        assert_ne!(
            assembly_row[2], source_assembly_output_row[2],
            "the source-managed post-process reserializes the assembly"
        );
        for object in [
            "OBJECT|TextAsset|9|Target",
            "OBJECT|MonoBehaviour|42|ManagedRoot",
            "OBJECT|MonoScript|7|GraphHost",
        ] {
            assert!(
                source_fixture.lines().any(|line| line == object),
                "{object}"
            );
        }

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem
            .write_all_bytes("/game/managed_reference_schema.assets", &input_bytes)
            .unwrap();
        filesystem
            .write_all_bytes("/game/target.assets", &target_input_bytes)
            .unwrap();
        filesystem
            .write_all_bytes("/game/ManagedReferenceFixtureScripts.dll", assembly_bytes)
            .unwrap();
        let inputs = [
            DependencyFilePath {
                file_name: "managed_reference_schema.assets".to_owned(),
                virtual_path: "/game/managed_reference_schema.assets".to_owned(),
            },
            DependencyFilePath {
                file_name: "target.assets".to_owned(),
                virtual_path: "/game/target.assets".to_owned(),
            },
            DependencyFilePath {
                file_name: "ManagedReferenceFixtureScripts.dll".to_owned(),
                virtual_path: "/game/ManagedReferenceFixtureScripts.dll".to_owned(),
            },
        ];
        let (catalog, assembly_report) = ManagedAssemblyCatalog::load_from_vfs(
            &filesystem,
            &inputs,
            ManagedAssemblyCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(assembly_report.loaded_count, 1, "{assembly_report:?}");
        assert!(catalog.contains_assembly("ManagedReferenceFixtureScripts"));
        assert!(catalog.is_valid(
            &crate::processing::script_identity::ScriptIdentifier::try_new(
                "ManagedReferenceFixtureScripts",
                "Game",
                "GraphHost",
            )
            .unwrap()
        ));

        let mut workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs[..2],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        assert_eq!(workspace.files().len(), 2);
        assert!(workspace.files().iter().all(|file| file
            .metadata
            .types
            .iter()
            .all(|serialized_type| serialized_type.type_tree.is_none())));
        let mut objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let recovery = objects
            .recover_mono_behaviour_structures(
                &filesystem,
                &mut workspace,
                &catalog,
                ObjectWorkspaceLimits::default(),
                ManagedSchemaLimits::default(),
            )
            .unwrap();
        assert_eq!(recovery.behaviours.len(), 1);
        assert_eq!(
            recovery.behaviours[0]
                .script
                .as_ref()
                .and_then(|script| script.name.as_deref()),
            Some("GraphHost")
        );
        assert!(matches!(
            recovery.behaviours[0].outcome,
            crate::serialized_file::object_workspace::ManagedBehaviourRecoveryOutcome::Recovered { .. }
        ));
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.report().object_count, 3);

        let root_file_index = workspace
            .files()
            .iter()
            .position(|file| file.file_name == "managed_reference_schema.assets")
            .unwrap();
        let target_file_index = workspace
            .files()
            .iter()
            .position(|file| file.file_name == "target.assets")
            .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete(), "{reference_graph:?}");
        let target_reference = reference_graph
            .references()
            .iter()
            .find(|reference| {
                reference.source_file_index == root_file_index
                    && reference.source_path_id == 42
                    && reference.field_path == "m_Structure.references.RefIds[0].data.target"
            })
            .unwrap_or_else(|| {
                panic!(
                    "assembly-resolved NodeA target PPtr is scanned; actual references: {:?}",
                    reference_graph.references()
                )
            });
        assert_eq!(target_reference.file_id, 1);
        assert_eq!(target_reference.path_id, 9);
        assert_eq!(
            target_reference.resolution,
            MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index: 0,
            }
        );

        let managed_graph = reference_graph
            .managed_graph_for_file(root_file_index)
            .expect("recovered MonoBehaviour managed graph exists");
        assert_eq!(managed_graph.managed_objects().len(), 2);
        assert_eq!(managed_graph.managed_objects()[0].reference_id, 101);
        assert_eq!(managed_graph.managed_objects()[0].class_name, "NodeA");
        assert_eq!(managed_graph.managed_objects()[1].reference_id, 202);
        assert_eq!(managed_graph.managed_objects()[1].class_name, "NodeB");
        assert_eq!(managed_graph.managed_references().len(), 3);
        assert!(managed_graph.managed_references().iter().all(|reference| matches!(
            reference.resolution,
            crate::serialized_file::reference_graph::SerializedManagedReferenceResolution::Object { .. }
        )));
        let root_edge = managed_graph
            .managed_references()
            .iter()
            .find(|reference| reference.field_path == "m_Structure.root")
            .expect("root points to the first registry object");
        assert_eq!(root_edge.reference_id, 101);
        let node_a_back_edge = managed_graph
            .managed_references()
            .iter()
            .find(|reference| reference.field_path == "m_Structure.references.RefIds[0].data.next")
            .expect("NodeA points to NodeB");
        assert_eq!(node_a_back_edge.reference_id, 202);
        let node_b_back_edge = managed_graph
            .managed_references()
            .iter()
            .find(|reference| reference.field_path == "m_Structure.references.RefIds[1].data.next")
            .expect("NodeB points back to NodeA");
        assert_eq!(node_b_back_edge.reference_id, 101);

        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root =
            crate::test_support::unique_test_directory("project-managed-reference-assembly-e2e");
        let report = export_workspace_yaml_project_with_assembly_catalog(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &reference_graph,
            &settings,
            true,
            Some(&catalog),
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();
        assert!(report.processing_warnings.is_empty(), "{report:?}");
        assert_eq!(report.exported_object_count, 2);
        assert_eq!(report.asset_file_count, 3);

        let source_artifact = |key: &str, expected_path: &str| {
            let row = managed_reference_schema_fixture_row(key);
            assert_eq!(row[0], expected_path);
            let expected_length = row[1].parse::<usize>().unwrap();
            assert_eq!(row[2].len(), 64, "source artifact SHA-256 is present");
            let bytes = managed_reference_schema_decode_hex(&row[3]);
            assert_eq!(
                bytes.len(),
                expected_length,
                "source artifact length is pinned"
            );
            bytes
        };
        let source_yaml = String::from_utf8(source_artifact(
            "YAML_HEX",
            "ExportedProject/Assets/MonoBehaviour/ManagedRoot.asset",
        ))
        .unwrap();
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
        let target_path = output_root.join("Assets/TextAsset/Target.txt");
        let target_meta_path = output_root.join("Assets/TextAsset/Target.txt.meta");
        let rust_target_guid = std::fs::read_to_string(&target_meta_path)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("Rust TextAsset importer metadata includes a GUID")
            .to_owned();
        let rust_yaml =
            std::fs::read_to_string(output_root.join("Assets/MonoBehaviour/ManagedRoot.asset"))
                .unwrap();
        let rust_script_guid = rust_yaml
            .lines()
            .find(|line| line.trim_start().starts_with("m_Script:"))
            .and_then(|line| line.split_once("guid: ").map(|(_, value)| value))
            .map(|value| {
                value
                    .chars()
                    .take_while(|character| character.is_ascii_hexdigit())
                    .collect::<String>()
            })
            .filter(|guid| guid.len() == 32)
            .expect("Rust MonoBehaviour script pointer includes a GUID");
        let expected_yaml = source_yaml
            .replace("__TEXTASSET_GUID__", &rust_target_guid)
            .replace("__SCRIPT_GUID__", &rust_script_guid);
        assert_eq!(
            rust_yaml.replace("\r\n", "\n"),
            expected_yaml.replace("\r\n", "\n"),
            "assembly-resolved fields, managed IDs, cycle edges, and external PPtr match source YAML"
        );
        assert_eq!(std::fs::read(&target_path).unwrap(), source_target);
        assert_eq!(
            normalized_meta_text(&std::fs::read_to_string(&target_meta_path).unwrap()) + "\n",
            String::from_utf8(source_target_meta).unwrap()
        );
        let plugin_meta_path =
            output_root.join("Assets/Plugins/ManagedReferenceFixtureScripts.dll.meta");
        assert_eq!(
            normalized_meta_text(&std::fs::read_to_string(&plugin_meta_path).unwrap()) + "\n",
            String::from_utf8(source_plugin_meta).unwrap()
        );
        let exported_assembly =
            std::fs::read(output_root.join("Assets/Plugins/ManagedReferenceFixtureScripts.dll"))
                .unwrap();
        let exported_assembly_model = ManagedAssembly::parse(
            "ManagedReferenceFixtureScripts.dll",
            &exported_assembly,
            ManagedAssemblyLimits::default(),
        )
        .expect("exported managed-reference assembly parses");
        let source_saved_assembly =
            include_bytes!("../processing/managed_reference_schema_source_saved_fixture.dll");
        let source_assembly_model = ManagedAssembly::parse(
            "ManagedReferenceFixtureScripts.dll",
            source_saved_assembly,
            ManagedAssemblyLimits::default(),
        )
        .expect("source-saved managed-reference assembly parses");
        assert_eq!(
            exported_assembly_model.types, source_assembly_model.types,
            "project export preserves the source-saved managed type model"
        );
        assert_ne!(exported_assembly, assembly_bytes);
        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn project_export_recovers_legacy_assembly_managed_references_without_file_typetrees() {
        let fixture =
            include_str!("../processing/managed_reference_legacy_schema_project_fixture.txt");
        assert!(fixture
            .lines()
            .any(|line| line == "UNITY_VERSION|2019.4.40f1"));
        assert!(fixture
            .lines()
            .any(|line| line == "REGISTRY_FORMAT|1|terminated index IDs"));
        let root_bytes =
            managed_reference_schema_fixture_input_from(fixture, "managed_reference_schema.assets");
        let target_bytes = managed_reference_schema_fixture_input_from(fixture, "target.assets");
        let assembly_bytes = include_bytes!("../processing/managed_reference_schema_fixture.dll");

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem
            .write_all_bytes("/game/managed_reference_schema.assets", &root_bytes)
            .unwrap();
        filesystem
            .write_all_bytes("/game/target.assets", &target_bytes)
            .unwrap();
        filesystem
            .write_all_bytes("/game/ManagedReferenceFixtureScripts.dll", assembly_bytes)
            .unwrap();
        let inputs = [
            DependencyFilePath {
                file_name: "managed_reference_schema.assets".to_owned(),
                virtual_path: "/game/managed_reference_schema.assets".to_owned(),
            },
            DependencyFilePath {
                file_name: "target.assets".to_owned(),
                virtual_path: "/game/target.assets".to_owned(),
            },
            DependencyFilePath {
                file_name: "ManagedReferenceFixtureScripts.dll".to_owned(),
                virtual_path: "/game/ManagedReferenceFixtureScripts.dll".to_owned(),
            },
        ];
        let (catalog, assembly_report) = ManagedAssemblyCatalog::load_from_vfs(
            &filesystem,
            &inputs,
            ManagedAssemblyCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(assembly_report.loaded_count, 1, "{assembly_report:?}");
        let mut workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs[..2],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(workspace.files().iter().all(|file| file
            .metadata
            .types
            .iter()
            .all(|serialized_type| serialized_type.type_tree.is_none())));
        let mut objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let recovery = objects
            .recover_mono_behaviour_structures(
                &filesystem,
                &mut workspace,
                &catalog,
                ObjectWorkspaceLimits::default(),
                ManagedSchemaLimits::default(),
            )
            .unwrap();
        assert_eq!(recovery.behaviours.len(), 1);
        assert!(matches!(
            recovery.behaviours[0].outcome,
            crate::serialized_file::object_workspace::ManagedBehaviourRecoveryOutcome::Recovered { .. }
        ));
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.report().object_count, 3);

        let root_file_index = workspace
            .files()
            .iter()
            .position(|file| file.file_name == "managed_reference_schema.assets")
            .unwrap();
        let target_file_index = workspace
            .files()
            .iter()
            .position(|file| file.file_name == "target.assets")
            .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete(), "{reference_graph:?}");
        let target_reference = reference_graph
            .references()
            .iter()
            .find(|reference| {
                reference.source_file_index == root_file_index
                    && reference.source_path_id == 42
                    && reference.field_path == "m_Structure.references.RefIds[0].data.target"
            })
            .expect("legacy registry's assembly-resolved PPtr is scanned");
        assert_eq!(target_reference.file_id, 1);
        assert_eq!(target_reference.path_id, 9);
        assert!(matches!(
            target_reference.resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: target,
                target_object_index: 0,
            } if target == target_file_index
        ));
        let managed_graph = reference_graph
            .managed_graph_for_file(root_file_index)
            .expect("legacy managed-reference graph exists");
        assert_eq!(managed_graph.managed_objects().len(), 2);
        assert_eq!(managed_graph.managed_objects()[0].reference_id, 0);
        assert_eq!(managed_graph.managed_objects()[0].class_name, "NodeA");
        assert_eq!(managed_graph.managed_objects()[1].reference_id, 1);
        assert_eq!(managed_graph.managed_objects()[1].class_name, "NodeB");
        assert_eq!(managed_graph.managed_references().len(), 3);
        assert!(managed_graph.managed_references().iter().all(|reference| matches!(
            reference.resolution,
            crate::serialized_file::reference_graph::SerializedManagedReferenceResolution::Object { .. }
        )));
        let root_edge = managed_graph
            .managed_references()
            .iter()
            .find(|reference| reference.field_path == "m_Structure.root")
            .expect("legacy root index points to the first registry entry");
        assert_eq!(root_edge.reference_id, 0);
        let node_a_edge = managed_graph
            .managed_references()
            .iter()
            .find(|reference| reference.field_path == "m_Structure.references.RefIds[0].data.next")
            .expect("legacy NodeA points to NodeB");
        assert_eq!(node_a_edge.reference_id, 1);
        let node_b_edge = managed_graph
            .managed_references()
            .iter()
            .find(|reference| reference.field_path == "m_Structure.references.RefIds[1].data.next")
            .expect("legacy NodeB points back to NodeA");
        assert_eq!(node_b_edge.reference_id, 0);
    }

    #[test]
    fn project_export_routes_an_existing_modern_prefab_instance_collection() {
        let bytes = make_existing_prefab_instance_file();
        if let Ok(path) = std::env::var("ASSET_RIPPER_REFERENCE_PREFAB_FIXTURE") {
            std::fs::write(path, &bytes).unwrap();
        }
        let (filesystem, workspace) = load_workspace_bytes_with_source_factory(&bytes);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(
            objects
                .objects()
                .iter()
                .all(|object| object.payload.is_ok()),
            "undecoded Prefab fixture objects: {:?}",
            objects
                .objects()
                .iter()
                .filter_map(|object| object.payload.as_ref().err())
                .collect::<Vec<_>>()
        );
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.objects().len(), 3);
        assert_eq!(
            objects.get(0, 0).unwrap().factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(
                "GameObject_2018_3".to_owned()
            )
        );
        assert_eq!(objects.get(0, 0).unwrap().factory.name, "Root");
        assert_eq!(
            objects.get(0, 1).unwrap().factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(
                "Transform_2018_3".to_owned()
            )
        );
        assert_eq!(
            objects.get(0, 2).unwrap().factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(
                "PrefabInstance_2018_3".to_owned()
            )
        );
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(
            reference_graph.is_complete(),
            "reference graph is incomplete"
        );
        let resolves_to = |source_path_id, target_path_id| {
            reference_graph.references().iter().any(|reference| {
                reference.source_path_id == source_path_id
                    && reference.path_id == target_path_id
                    && reference.file_id == 0
                    && matches!(
                        reference.resolution,
                        MultiFilePPtrResolution::Object {
                            target_file_index: 0,
                            ..
                        }
                    )
            })
        };
        assert!(
            resolves_to(10, 11),
            "GameObject component PPtr was not retained"
        );
        assert!(
            resolves_to(11, 10),
            "Transform-to-GameObject PPtr was not retained"
        );
        assert!(
            resolves_to(12, 10),
            "PrefabInstance root PPtr was not retained"
        );
        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root = crate::test_support::unique_test_directory("project-existing-prefab-e2e");

        let report = export_workspace_yaml_project_with_settings(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &reference_graph,
            &settings,
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();

        let prefab_path = output_root.join("Assets/PrefabInstance/Root.prefab");
        let prefab_meta_path = output_root.join("Assets/PrefabInstance/Root.prefab.meta");
        let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
        let prefab_meta = std::fs::read_to_string(&prefab_meta_path).unwrap();
        let game_object_id = yaml_document_file_id(&prefab_yaml, 1);
        let transform_id = yaml_document_file_id(&prefab_yaml, 4);
        assert!(prefab_yaml.contains("m_Name: Root"), "{prefab_yaml}");
        assert!(
            prefab_yaml.contains(&format!("component: {{fileID: {transform_id}}}")),
            "{prefab_yaml}"
        );
        assert!(
            prefab_yaml.contains(&format!("m_GameObject: {{fileID: {game_object_id}}}")),
            "{prefab_yaml}"
        );
        assert!(!prefab_yaml.contains("!u!1001"), "{prefab_yaml}");
        assert!(prefab_meta.contains("PrefabImporter:"), "{prefab_meta}");
        assert_eq!(report.asset_file_count, 1);
        assert_eq!(report.meta_file_count, 1);
        assert_eq!(report.collection_count, 1);
        assert_eq!(report.exported_object_count, 2);
        assert!(report.processing_warnings.is_empty());

        let source_rows = include_str!("../processing/prefab_instance_runtime_fixture.txt");
        assert!(source_rows.lines().any(|line| {
            line == "PREFAB-HIER|root=10:Root|prefab=12|gameObjects=GameObject_2018_3#10:Root|components=Transform_2018_3#11:null|prefabInstances=|exportable=GameObject_2018_3#10:Root,Transform_2018_3#11:null|hidden=PrefabInstance_2018_3#12:null"
        }));

        std::fs::remove_dir_all(output_root).unwrap();
    }

    fn yaml_document_file_id(yaml: &str, class_id: i32) -> String {
        let prefix = format!("--- !u!{class_id} &");
        yaml.lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .and_then(|tail| tail.split_whitespace().next())
            .expect("the expected Unity YAML document exists")
            .to_owned()
    }
