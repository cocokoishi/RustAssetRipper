use super::*;

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
                "../../../processing/managed_assembly_unsafe_forwarder_2019a1_source_meta.txt"
            ),
        ),
        (
            "2023.3.0a14",
            include_str!(
                "../../../processing/managed_assembly_unsafe_forwarder_2023a14_source_meta.txt"
            ),
        ),
        (
            "2023.3.0a15",
            include_str!(
                "../../../processing/managed_assembly_unsafe_forwarder_2023a15_source_meta.txt"
            ),
        ),
        (
            "2023.3.0a16",
            include_str!(
                "../../../processing/managed_assembly_unsafe_forwarder_2023a16_source_meta.txt"
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
            include_bytes!("../../../processing/managed_assembly_mscorlib2_fixture.dll"),
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
