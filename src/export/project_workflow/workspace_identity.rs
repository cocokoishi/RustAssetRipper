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
    assignments: &[SceneExportIdAssignment],
) -> Result<(), Box<dyn Error>> {
    let mut unsupported_objects = Vec::new();
    const RENDERER_CLASS_IDS: [i32; 18] = [
        23,
        25,
        26,
        96,
        120,
        137,
        161,
        199,
        212,
        227,
        331,
        483_693_784,
        73_398_921,
        1_120_581_460,
        1_931_382_933,
        1_931_382_934,
        1_971_053_207,
        646_504_946,
    ];
    for assignment in assignments {
        let class_id = assignment.asset.class_id;
        if class_id == 108 || class_id == 218 {
            unsupported_objects.push(format!(
                "scene object {:?}:{} class {class_id} has LightingDataAsset data that is not implemented",
                assignment.asset.collection, assignment.asset.path_id
            ));
        } else if RENDERER_CLASS_IDS.contains(&class_id) && class_id != 23 {
            unsupported_objects.push(format!(
                "renderer scene object {:?}:{} class {class_id} is outside the supported MeshRenderer route",
                assignment.asset.collection, assignment.asset.path_id
            ));
        }
    }
    for collection in hierarchy.collections() {
        for asset in collection.scene_assets() {
            if asset.category == WorkspaceSceneAssetCategory::PrefabInstance {
                unsupported_objects.push(format!(
                    "file {} object {} is a PrefabInstance outside the bounded LightingDataAsset route",
                    asset.identity.file_index, asset.identity.object_index
                ));
            }
        }
        for candidate in collection.component_candidates() {
            let Some(class_id) = candidate.identity.class_id else {
                unsupported_objects.push(format!(
                    "file {} object {} has an unclassified component in the LightingDataAsset scene",
                    candidate.identity.file_index, candidate.identity.object_index
                ));
                continue;
            };
            if (class_id == 108 || class_id == 218)
                || (RENDERER_CLASS_IDS.contains(&class_id) && class_id != 23)
            {
                unsupported_objects.push(format!(
                    "file {} object {} class {class_id} has LightingDataAsset data that is not implemented",
                    candidate.identity.file_index, candidate.identity.object_index
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

pub(super) fn collect_lighting_data_renderer_inputs<'object>(
    assignments: &[SceneExportIdAssignment],
    objects: &'object ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Vec<LightingDataRendererInput<'object>>, Box<dyn Error>> {
    let mut mesh_filters_by_game_object =
        HashMap::<ExportAssetObjectKey, Vec<ExportAssetIdentity>>::new();
    mesh_filters_by_game_object
        .try_reserve(assignments.len())
        .map_err(|_| invalid_project("could not allocate LightingDataAsset MeshFilter index"))?;
    for assignment in assignments
        .iter()
        .filter(|assignment| assignment.asset.class_id == 33)
    {
        let file_index = serialized_file_index(assignment.asset, "MeshFilter")?;
        let object = objects
            .get(file_index, assignment.asset.object_index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "LightingDataAsset MeshFilter {}:{} is missing from ObjectWorkspace",
                    file_index, assignment.asset.path_id
                ))
            })?;
        if object.path_id != assignment.asset.path_id || object.factory.class_id != 33 {
            return Err(invalid_project(format!(
                "LightingDataAsset MeshFilter identity mismatch at {}:{}",
                file_index, assignment.asset.path_id
            )));
        }
        let payload = object.payload.as_ref().map_err(|error| {
            invalid_project(format!(
                "LightingDataAsset MeshFilter {}:{} could not be decoded: {error}",
                file_index, assignment.asset.path_id
            ))
        })?;
        let game_object_key =
            component_game_object_key(assignment.asset, &payload.root, reference_graph)?;
        let filters = mesh_filters_by_game_object
            .entry(game_object_key)
            .or_default();
        filters.try_reserve(1).map_err(|_| {
            invalid_project("could not allocate LightingDataAsset MeshFilter associations")
        })?;
        filters.push(assignment.asset);
    }

    let mut renderers = Vec::new();
    renderers
        .try_reserve(assignments.len())
        .map_err(|_| invalid_project("could not allocate LightingDataAsset renderer inputs"))?;
    for assignment in assignments
        .iter()
        .filter(|assignment| assignment.asset.class_id == 23)
    {
        let ExportAssetCollectionIdentity::SerializedFile { file_index } =
            assignment.asset.collection
        else {
            return Err(invalid_project(format!(
                "LightingDataAsset MeshRenderer {} belongs to an unmaterialized collection",
                assignment.asset.path_id
            )));
        };
        let object = objects
            .get(file_index, assignment.asset.object_index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "LightingDataAsset MeshRenderer {}:{} is missing from ObjectWorkspace",
                    file_index, assignment.asset.path_id
                ))
            })?;
        if object.path_id != assignment.asset.path_id || object.factory.class_id != 23 {
            return Err(invalid_project(format!(
                "LightingDataAsset MeshRenderer identity mismatch at {}:{}",
                file_index, assignment.asset.path_id
            )));
        }
        let payload = object.payload.as_ref().map_err(|error| {
            invalid_project(format!(
                "LightingDataAsset MeshRenderer {}:{} could not be decoded: {error}",
                file_index, assignment.asset.path_id
            ))
        })?;
        let uv_mesh_target = if scene_renderer_is_lightmapped(assignment.asset, &payload.root)? {
            let game_object_key =
                component_game_object_key(assignment.asset, &payload.root, reference_graph)?;
            if let Some(filters) = mesh_filters_by_game_object.get(&game_object_key) {
                if filters.len() != 1 {
                    return Err(invalid_project(format!(
                        "GameObject {:?} has {} MeshFilters in a LightingDataAsset scene; source TryGetComponent selection is ambiguous",
                        game_object_key,
                        filters.len()
                    )));
                }
                let mesh_filter = filters[0];
                let filter_file_index = serialized_file_index(mesh_filter, "MeshFilter")?;
                let mesh_filter_object = objects
                    .get(filter_file_index, mesh_filter.object_index)
                    .ok_or_else(|| {
                    invalid_project(format!(
                        "LightingDataAsset MeshFilter {}:{} is missing from ObjectWorkspace",
                        filter_file_index, mesh_filter.path_id
                    ))
                })?;
                let mesh_filter_payload = mesh_filter_object.payload.as_ref().map_err(|error| {
                    invalid_project(format!(
                        "LightingDataAsset MeshFilter {}:{} could not be decoded: {error}",
                        filter_file_index, mesh_filter.path_id
                    ))
                })?;
                mesh_filter_mesh_target(mesh_filter, &mesh_filter_payload.root, reference_graph)?
            } else {
                None
            }
        } else {
            None
        };
        renderers.push(LightingDataRendererInput {
            identity: assignment.asset,
            file_id: assignment.file_id,
            value: &payload.root,
            uv_mesh_target,
        });
    }
    Ok(renderers)
}

fn scene_renderer_is_lightmapped(
    renderer: ExportAssetIdentity,
    root: &SerializedValue,
) -> Result<bool, Box<dyn Error>> {
    let value = root
        .unique_field_in_base_chain("m_LightmapIndex")
        .map_err(|error| {
            invalid_project(format!(
                "LightingDataAsset MeshRenderer {:?}:{} m_LightmapIndex lookup failed: {error}",
                renderer.collection, renderer.path_id
            ))
        })?
        .ok_or_else(|| {
            invalid_project(format!(
                "LightingDataAsset MeshRenderer {:?}:{} TypeTree has no m_LightmapIndex field",
                renderer.collection, renderer.path_id
            ))
        })?;
    let index = match value {
        SerializedValue::U8(index) => u16::from(*index),
        SerializedValue::U16(index) => *index,
        value => {
            return Err(invalid_project(format!(
                "LightingDataAsset MeshRenderer {:?}:{} has unsupported m_LightmapIndex value {value:?}",
                renderer.collection, renderer.path_id
            )))
        }
    };
    Ok(index != u16::MAX)
}

fn serialized_file_index(
    identity: ExportAssetIdentity,
    role: &str,
) -> Result<usize, Box<dyn Error>> {
    match identity.collection {
        ExportAssetCollectionIdentity::SerializedFile { file_index } => Ok(file_index),
        ExportAssetCollectionIdentity::ProcessedCollection { .. } => Err(invalid_project(format!(
            "LightingDataAsset {role} {} is in an unmaterialized collection",
            identity.path_id
        ))),
    }
}

fn component_game_object_key(
    component: ExportAssetIdentity,
    root: &SerializedValue,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<ExportAssetObjectKey, Box<dyn Error>> {
    let file_index = serialized_file_index(component, "component")?;
    let pointer = root
        .unique_field_in_base_chain("m_GameObject")
        .map_err(|error| {
            invalid_project(format!(
                "LightingDataAsset component {:?}:{} m_GameObject lookup failed: {error}",
                component.collection, component.path_id
            ))
        })?
        .ok_or_else(|| {
            invalid_project(format!(
                "LightingDataAsset component {:?}:{} TypeTree has no m_GameObject field",
                component.collection, component.path_id
            ))
        })?;
    let SerializedValue::PPtr { file_id, path_id } = pointer else {
        return Err(invalid_project(format!(
            "LightingDataAsset component {:?}:{} m_GameObject is not a PPtr",
            component.collection, component.path_id
        )));
    };
    if *path_id == 0 {
        return Err(invalid_project(format!(
            "LightingDataAsset component {:?}:{} has a null m_GameObject",
            component.collection, component.path_id
        )));
    }
    let mut matches = reference_graph
        .references_from_object(file_index, component.object_index)
        .filter(|edge| {
            edge.file_id == *file_id
                && edge.path_id == *path_id
                && edge.field_path.ends_with("m_GameObject")
        });
    let edge = matches.next().ok_or_else(|| {
        invalid_project(format!(
            "LightingDataAsset component {:?}:{} m_GameObject has no matching reference-graph edge",
            component.collection, component.path_id
        ))
    })?;
    if matches.next().is_some() {
        return Err(invalid_project(format!(
            "LightingDataAsset component {:?}:{} m_GameObject matches multiple reference-graph edges",
            component.collection, component.path_id
        )));
    }
    match &edge.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => {
            if reference_graph.object_class_id(*target_file_index, *target_object_index) != Some(1)
            {
                return Err(invalid_project(format!(
                    "LightingDataAsset component {:?}:{} m_GameObject does not resolve to a GameObject",
                    component.collection, component.path_id
                )));
            }
            Ok(ExportAssetObjectKey {
                collection: ExportAssetCollectionIdentity::SerializedFile {
                    file_index: *target_file_index,
                },
                object_index: *target_object_index,
            })
        }
        resolution => Err(invalid_project(format!(
            "LightingDataAsset component {:?}:{} m_GameObject is unresolved: {resolution:?}",
            component.collection, component.path_id
        ))),
    }
}

fn mesh_filter_mesh_target(
    mesh_filter: ExportAssetIdentity,
    root: &SerializedValue,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Option<ExportAssetIdentity>, Box<dyn Error>> {
    let file_index = serialized_file_index(mesh_filter, "MeshFilter")?;
    let pointer = root
        .unique_field_in_base_chain("m_Mesh")
        .map_err(|error| {
            invalid_project(format!(
                "LightingDataAsset MeshFilter {:?}:{} m_Mesh lookup failed: {error}",
                mesh_filter.collection, mesh_filter.path_id
            ))
        })?
        .ok_or_else(|| {
            invalid_project(format!(
                "LightingDataAsset MeshFilter {:?}:{} TypeTree has no m_Mesh field",
                mesh_filter.collection, mesh_filter.path_id
            ))
        })?;
    let SerializedValue::PPtr { file_id, path_id } = pointer else {
        return Err(invalid_project(format!(
            "LightingDataAsset MeshFilter {:?}:{} m_Mesh is not a PPtr",
            mesh_filter.collection, mesh_filter.path_id
        )));
    };
    if *path_id == 0 {
        return Ok(None);
    }
    let mut matches = reference_graph
        .references_from_object(file_index, mesh_filter.object_index)
        .filter(|edge| {
            edge.file_id == *file_id
                && edge.path_id == *path_id
                && edge.field_path.ends_with("m_Mesh")
        });
    let edge = matches.next().ok_or_else(|| {
        invalid_project(format!(
            "LightingDataAsset MeshFilter {:?}:{} m_Mesh has no matching reference-graph edge",
            mesh_filter.collection, mesh_filter.path_id
        ))
    })?;
    if matches.next().is_some() {
        return Err(invalid_project(format!(
            "LightingDataAsset MeshFilter {:?}:{} m_Mesh matches multiple reference-graph edges",
            mesh_filter.collection, mesh_filter.path_id
        )));
    }
    let (target_file_index, target_object_index) = match &edge.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => (*target_file_index, *target_object_index),
        resolution => {
            return Err(invalid_project(format!(
                "LightingDataAsset MeshFilter {:?}:{} m_Mesh is unresolved: {resolution:?}",
                mesh_filter.collection, mesh_filter.path_id
            )))
        }
    };
    if reference_graph.object_class_id(target_file_index, target_object_index) != Some(43) {
        return Err(invalid_project(format!(
            "LightingDataAsset MeshFilter {:?}:{} m_Mesh does not resolve to class 43",
            mesh_filter.collection, mesh_filter.path_id
        )));
    }
    let target = reference_graph
        .object_identity(target_file_index, target_object_index)
        .ok_or_else(|| {
            invalid_project(format!(
                "LightingDataAsset MeshFilter {:?}:{} m_Mesh target has no object identity",
                mesh_filter.collection, mesh_filter.path_id
            ))
        })?;
    Ok(Some(ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: target_file_index,
        },
        object_index: target_object_index,
        path_id: target.path_id,
        class_id: 43,
    }))
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
