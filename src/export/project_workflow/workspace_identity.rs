use super::*;

pub(super) fn workspace_project_version(
    workspace: &SerializedFileWorkspace,
) -> Result<(&str, SceneUnityVersion), Box<dyn Error>> {
    if workspace.files().is_empty() {
        return Err(invalid_project("project export has no SerializedFiles"));
    }
    let editor_version_text = workspace
        .files()
        .iter()
        .find_map(|file| file.metadata.prefix.unity_version_text.as_deref())
        .ok_or_else(|| invalid_project("project export needs a SerializedFile Unity version"))?;
    let project_version = SceneUnityVersion::parse(editor_version_text).ok_or_else(|| {
        invalid_project(format!(
            "project export cannot parse Unity version {editor_version_text:?}"
        ))
    })?;
    Ok((editor_version_text, project_version))
}

/// Build the `PathIdMapExporter` input in source collection and object-table order.
pub(super) fn build_path_id_map(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
) -> Result<SerializedGameInfo, Box<dyn Error>> {
    let mut files = Vec::new();
    files
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_project("could not reserve PathIdMap serialized-file records"))?;
    for (file_index, file) in workspace.files().iter().enumerate() {
        let collection_name = crate::io::special_names::fix_file_identifier(&file.file_name);
        let objects_in_file = objects.objects_for_file(file_index);
        let mut assets = Vec::new();
        assets
            .try_reserve_exact(objects_in_file.len())
            .map_err(|_| invalid_project("could not reserve PathIdMap object records"))?;
        for object in objects_in_file {
            let crate::processing::asset_factory::FactoryAssetKind::Generated(generated_type) =
                &object.factory.kind
            else {
                // The source raw and TypeTree fallback wrappers do not implement generated
                // asset interfaces, even when their class ID has generated types in other
                // versions.
                continue;
            };
            let class_id = object.factory.class_id;
            if !has_path_id_map_generated_types(class_id) {
                continue;
            }
            let Some(interfaces) = generated_type_interfaces(class_id, generated_type) else {
                return Err(invalid_project(format!(
                    "PathIdMap has no frozen generated-interface record for class {class_id} type {generated_type:?} in {} PathID {}",
                    file.file_name, object.path_id
                )));
            };
            if !interfaces.is_recorded() {
                continue;
            }
            assets.push(PathIdMapAssetFacts {
                is_mesh: interfaces.is_mesh,
                is_texture: interfaces.is_texture,
                is_audio_clip: interfaces.is_audio_clip,
                is_text_asset: interfaces.is_text_asset,
                is_named: interfaces.is_named,
                name: interfaces.is_named.then_some(object.factory.name.as_str()),
                class_name: object.factory.class_name.as_ref(),
                path_id: object.path_id,
            });
        }
        files.push(build_serialized_file(Some(&collection_name), &assets));
    }
    Ok(SerializedGameInfo { files })
}

/// The named asset name (`INamed.Name`) of one decoded payload, if the TypeTree has one.
pub(super) fn object_asset_name(payload: &ParsedObjectPayload) -> Option<String> {
    for field in ["m_Name", "Name"] {
        if let Ok(Some(crate::serialized_file::object::SerializedValue::StringBytes(bytes))) =
            payload.root.unique_field_in_base_chain(field)
        {
            if !bytes.is_empty() {
                return Some(String::from_utf8_lossy(bytes).into_owned());
            }
        }
    }
    None
}

pub(super) fn validate_bounded_lighting_data_scene(
    hierarchy: &WorkspaceSceneHierarchyScene,
    owner: WorkspaceSceneObjectIdentity,
) -> Result<(), Box<dyn Error>> {
    let mut unsupported_objects = Vec::new();
    for collection in hierarchy.collections() {
        if collection.game_object_count() != 0
            || collection.transform_count() != 0
            || !collection.component_candidates().is_empty()
        {
            unsupported_objects.push(format!(
                "file {} has {} GameObjects, {} Transforms, and {} component candidates",
                collection.collection_file_index(),
                collection.game_object_count(),
                collection.transform_count(),
                collection.component_candidates().len()
            ));
        }
        for asset in collection.scene_assets() {
            if matches!(
                asset.category,
                WorkspaceSceneAssetCategory::GameObject
                    | WorkspaceSceneAssetCategory::PrefabInstance
            ) {
                unsupported_objects.push(format!(
                    "file {} object {} class {:?} is outside the bounded LightingDataAsset route",
                    asset.identity.file_index, asset.identity.object_index, asset.identity.class_id
                ));
            }
        }
    }
    if !unsupported_objects.is_empty() {
        return Err(invalid_project(format!(
            "LightingDataAsset generation for LightmapSettings {}:{} cannot omit renderer, terrain, light, or scene-object lighting data: {}",
            owner.file_index,
            owner.path_id,
            unsupported_objects.join("; ")
        )));
    }
    Ok(())
}

pub(super) fn scene_manager_identity(
    hierarchy: &WorkspaceSceneHierarchyScene,
    kind: SceneManagerKind,
) -> Option<WorkspaceSceneObjectIdentity> {
    hierarchy
        .collections()
        .iter()
        .flat_map(|collection| collection.scene_assets())
        .find(|asset| asset.category == WorkspaceSceneAssetCategory::LevelGameManager(kind))
        .map(|asset| asset.identity)
}

pub(super) fn export_identity_for_scene_object(
    identity: WorkspaceSceneObjectIdentity,
) -> Result<ExportAssetIdentity, Box<dyn Error>> {
    let class_id = identity.class_id.ok_or_else(|| {
        invalid_project(format!(
            "workspace object {}:{} has no class ID for project export",
            identity.file_index, identity.path_id
        ))
    })?;
    if identity.path_id == 0 {
        return Err(invalid_project(format!(
            "workspace object {}:{} has a null PathID",
            identity.file_index, identity.path_id
        )));
    }
    Ok(ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: identity.file_index,
        },
        object_index: identity.object_index,
        path_id: identity.path_id,
        class_id,
    })
}

pub(super) fn build_lighting_data_asset_root(
    type_tree: &crate::serialized_file::metadata::TypeTree,
    object_index: usize,
    lightmap_settings: &SerializedValue,
    render_settings: &SerializedValue,
    source_version: &FactoryUnityVersion,
    scene_guid: [u8; 16],
) -> Result<SerializedValue, Box<dyn Error>> {
    let mut root = default_value_for_type_tree_node(
        type_tree,
        object_index,
        0,
        ObjectDefaultValueLimits::default(),
    )
    .map_err(|error| {
        invalid_project(format!(
            "LightingDataAsset default value could not be built from its reference TypeTree: {error}"
        ))
    })?;
    prepend_lighting_data_object_fields(&mut root, source_version)?;

    set_required_lighting_data_field(
        &mut root,
        "m_Name",
        SerializedValue::StringBytes(b"LightingData".to_vec()),
    )?;
    if unique_lighting_data_field_index(&root, "m_Scene")?.is_some() {
        set_required_lighting_data_field(
            &mut root,
            "m_Scene",
            SerializedValue::PPtr {
                file_id: 0,
                // A non-null sentinel forces the YAML converter through the generated SceneAsset
                // pointer resolver. It is never written to a SerializedFile or emitted as YAML.
                path_id: i64::MIN,
            },
        )?;
    } else if unique_lighting_data_field_index(&root, "m_SceneGUID")?.is_some() {
        set_lighting_data_guid_field(&mut root, "m_SceneGUID", scene_guid)?;
    }
    let enlighten_data = create_empty_lighting_enlighten_data(source_version)?;
    let enlighten_data_value =
        lighting_data_bytes_field_value(&root, "m_EnlightenData", enlighten_data)?;
    set_required_lighting_data_field(&mut root, "m_EnlightenData", enlighten_data_value)?;
    if unique_lighting_data_field_index(&root, "m_EnlightenDataVersion")?.is_some() {
        set_required_lighting_data_field(
            &mut root,
            "m_EnlightenDataVersion",
            SerializedValue::I32(112),
        )?;
    }

    copy_required_lighting_data_field(lightmap_settings, &mut root, "m_Lightmaps", "m_Lightmaps")?;
    copy_optional_lighting_data_field(
        lightmap_settings,
        &mut root,
        "m_LightmapsMode",
        "m_LightmapsMode",
    )?;
    copy_required_lighting_data_field(
        lightmap_settings,
        &mut root,
        "m_EnlightenSceneMapping",
        "m_EnlightenSceneMapping",
    )?;
    copy_optional_lighting_data_field(
        lightmap_settings,
        &mut root,
        "m_LightProbes",
        "m_LightProbes",
    )?;
    copy_render_settings_probe(
        render_settings,
        &mut root,
        "m_AmbientProbe",
        "m_BakedAmbientProbeInLinear",
        "m_BakedAmbientProbesInLinear",
    )?;
    copy_render_settings_probe(
        render_settings,
        &mut root,
        "m_AmbientProbeInGamma",
        "m_BakedAmbientProbeInGamma",
        "m_BakedAmbientProbesInGamma",
    )?;

    if contains_non_null_pointer(render_settings) {
        return Err(invalid_project(
            "bounded LightingDataAsset generation cannot omit a non-null RenderSettings object reference used by the source reflection path",
        ));
    }
    Ok(root)
}
