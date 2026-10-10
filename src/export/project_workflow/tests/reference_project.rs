use super::*;

#[test]
fn project_export_keeps_source_managed_registry_cycles_and_cross_file_pointer() {
    let mut filesystem = VirtualFileSystem::new();
    filesystem.create_directory(Some("/game")).unwrap();
    for file_name in ["managed.assets", "target.assets"] {
        let bytes = match file_name {
            "managed.assets" => {
                include_bytes!("../../managed_reference_project_fixture/managed.assets").as_slice()
            }
            "target.assets" => {
                include_bytes!("../../managed_reference_project_fixture/target.assets").as_slice()
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

    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
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
    let output_root = crate::test_support::unique_test_directory("project-managed-reference-e2e");
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
        include_str!("../../managed_reference_project_fixture/source_ManagedRoot.asset");
    let source_target_meta =
        include_str!("../../managed_reference_project_fixture/source_ReferencedRoot.asset.meta");
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
        include_str!("../../managed_reference_project_fixture/source_ReferencedRoot.asset");
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
