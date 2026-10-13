use super::*;

pub(super) fn mono_importer_yaml(execution_order: i16) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    importer.insert_field(
        "serializedVersion",
        UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(2)),
    )?;
    importer.insert_field(
        "externalObjects",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field(
        "defaultReferences",
        UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow),
    )?;
    importer.insert_field(
        "executionOrder",
        UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(
            i64::from(execution_order),
        )),
    )?;
    let mut icon = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    icon.insert_field(
        "fileID",
        UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(0)),
    )?;
    importer.insert_field("icon", icon)?;
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field("assetBundleName", UnityYamlNode::string(""))?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

pub(super) fn plugin_importer_yaml(
    unity_version_text: &str,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let version = FactoryUnityVersion::parse(unity_version_text).ok_or_else(|| {
        invalid_project(format!(
            "PluginImporter export cannot parse Unity version {unity_version_text:?}"
        ))
    })?;
    if version.is_unsupported() {
        return Err(invalid_project(format!(
            "PluginImporter export does not support baseline-unsupported Unity version {unity_version_text:?}"
        )));
    }
    let version_bits = version.source_order_bits().ok_or_else(|| {
        invalid_project(format!(
            "PluginImporter export cannot order Unity version {unity_version_text:?}"
        ))
    })?;
    // The frozen class-1050 dispatch changes the generated PluginImporter type at 2023.3.0a15;
    // that version switches platformData from the old pair sequence to the named map.
    let named_platform_data_from = FactoryUnityVersion::parse("2023.3.0a15")
        .and_then(|version| version.source_order_bits())
        .ok_or_else(|| invalid_project("invalid internal PluginImporter version transition"))?;
    let uses_named_platform_data = version_bits >= named_platform_data_from;
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    importer.insert_field(
        "serializedVersion",
        UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(
            if uses_named_platform_data { 3 } else { 2 },
        )),
    )?;
    importer.insert_field(
        "externalObjects",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field(
        "iconMap",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field(
        "executionOrder",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field(
        "defineConstraints",
        UnityYamlNode::sequence(UnityYamlSequenceStyle::Block),
    )?;
    for field in [
        "isPreloaded",
        "isOverridable",
        "isExplicitlyReferenced",
        "validateReferences",
    ] {
        importer.insert_field(
            field,
            UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(0)),
        )?;
    }

    let platform_data = if uses_named_platform_data {
        let mut platform_data = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        let mut any_settings = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        any_settings.insert_field(
            "enabled",
            UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(1)),
        )?;
        any_settings.insert_field(
            "settings",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
        )?;
        platform_data.insert_field("Any", any_settings)?;

        let mut editor_settings = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        editor_settings.insert_field(
            "enabled",
            UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(0)),
        )?;
        let mut editor_defaults = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        editor_defaults.insert_field(
            "DefaultValueInitialized",
            UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::PlainString(
                "true".to_owned(),
            )),
        )?;
        editor_settings.insert_field("settings", editor_defaults)?;
        platform_data.insert_field("Editor", editor_settings)?;
        platform_data
    } else {
        let mut platform_data = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
        let mut any_platform = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        any_platform.insert_field("Any", UnityYamlNode::string(""))?;
        let mut any_settings = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        any_settings.insert_field(
            "enabled",
            UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(1)),
        )?;
        any_settings.insert_field(
            "settings",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
        )?;
        let mut any_entry = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        any_entry.insert_field("first", any_platform)?;
        any_entry.insert_field("second", any_settings)?;
        platform_data.push(any_entry)?;

        let mut editor_platform = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        editor_platform.insert_field("Editor", UnityYamlNode::string("Editor"))?;
        let mut editor_settings = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        editor_settings.insert_field(
            "enabled",
            UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(0)),
        )?;
        let mut initialized = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        initialized.insert_field(
            "DefaultValueInitialized",
            UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::PlainString(
                "true".to_owned(),
            )),
        )?;
        editor_settings.insert_field("settings", initialized)?;
        let mut editor_entry = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        editor_entry.insert_field("first", editor_platform)?;
        editor_entry.insert_field("second", editor_settings)?;
        platform_data.push(editor_entry)?;
        platform_data
    };
    importer.insert_field("platformData", platform_data)?;

    let mut output = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    output.insert_field(
        "dllType",
        UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(0)),
    )?;
    output.insert_field(
        "scriptingRuntimeVersion",
        UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(0)),
    )?;
    importer.insert_field("output", output)?;
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field("assetBundleName", UnityYamlNode::string(""))?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

pub(super) fn default_importer(
    asset_bundle_name: Option<&str>,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer =
        UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Block);
    importer.insert_field(
        "externalObjects",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field(
        "assetBundleName",
        UnityYamlNode::string(asset_bundle_name.unwrap_or("")),
    )?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

pub(super) fn scene_default_importer(
    version: SceneUnityVersion,
    asset_bundle_name: Option<&str>,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    if (version.major, version.minor) < (2018, 3) {
        importer.insert_field("name", UnityYamlNode::string(""))?;
    }
    if (version.major, version.minor) >= (2017, 2) {
        importer.insert_field(
            "externalObjects",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
        )?;
    }
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field(
        "assetBundleName",
        UnityYamlNode::string(asset_bundle_name.unwrap_or("")),
    )?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

/// `PrefabExportCollection.CreateImporter` modern branch (`PrefabImporter.Create` with the
/// full export version). Frozen TPK class 468431735 proves the field gates:
/// `m_UnableToImportOnPreviousDomainReload` exists 2018.3 through 2019.x but not 2020.1+;
/// `m_VariantParentGUID` exists 2022.2+ but not 2022.1 or earlier. Fresh importers are never
/// variants, so the parent GUID renders as 32 zeros.
pub(super) fn prefab_importer(
    version: SceneUnityVersion,
    asset_bundle_name: Option<&str>,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    importer.insert_field(
        "externalObjects",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field("addedObjectFileIDs", UnityYamlNode::string(""))?;
    importer.insert_field(
        "isPrefabVariant",
        UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(0)),
    )?;
    if (version.major, version.minor) < (2020, 1) {
        importer.insert_field(
            "unableToImportOnPreviousDomainReload",
            UnityYamlNode::scalar(crate::export::unity_yaml::UnityYamlScalar::Signed(0)),
        )?;
    }
    if (version.major, version.minor) >= (2022, 2) {
        importer.insert_field(
            "variantParentGUID",
            UnityYamlNode::string("00000000000000000000000000000000"),
        )?;
    }
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field(
        "assetBundleName",
        UnityYamlNode::string(asset_bundle_name.unwrap_or("")),
    )?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

pub(super) fn count_unity_yaml_documents(yaml: &str) -> usize {
    yaml.lines()
        .filter(|line| line.starts_with("--- !u!"))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::unity_yaml::{write_unity_yaml_mapping, UnityYamlWriteLimits};

    fn render_prefab_importer(version: &str) -> String {
        let node =
            prefab_importer(SceneUnityVersion::parse(version).unwrap(), Some("bundle")).unwrap();
        write_unity_yaml_mapping(&node, UnityYamlWriteLimits::default()).unwrap()
    }

    /// Frozen TPK class 468431735 proves the gates: `unableToImport...` exists
    /// 2018.3-2019.x but not 2020.1+; `variantParentGUID` exists 2022.2+ but
    /// not earlier. The order matches the source meta files.
    #[test]
    fn prefab_importer_fields_follow_frozen_version_gates() {
        let old = render_prefab_importer("2019.4.40f1");
        assert!(
            old.contains("unableToImportOnPreviousDomainReload: 0"),
            "{old}"
        );
        assert!(!old.contains("variantParentGUID"), "{old}");

        let mid = render_prefab_importer("2021.3.0f1");
        assert!(
            !mid.contains("unableToImportOnPreviousDomainReload"),
            "{mid}"
        );
        assert!(!mid.contains("variantParentGUID"), "{mid}");

        let new = render_prefab_importer("2022.3.22f1");
        assert!(
            !new.contains("unableToImportOnPreviousDomainReload"),
            "{new}"
        );
        assert!(
            new.contains("variantParentGUID: 00000000000000000000000000000000"),
            "{new}"
        );
        let variant = new.find("variantParentGUID").unwrap();
        assert!(new.find("isPrefabVariant").unwrap() < variant, "{new}");
        assert!(variant < new.find("userData").unwrap(), "{new}");
        assert!(new.contains("assetBundleName: bundle"), "{new}");
    }
}
