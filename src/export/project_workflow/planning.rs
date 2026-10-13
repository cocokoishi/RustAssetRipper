use super::*;
pub(super) struct RoutedAssetPlanningContext<'context, 'object, 'provider, 'graph> {
    pub(super) filesystem: &'context VirtualFileSystem,
    pub(super) root_catalog: &'context RootResourceFileCatalog,
    pub(super) resource_provider: &'context PlatformResourceProvider<'provider>,
    pub(super) workspace: &'context SerializedFileWorkspace,
    pub(super) objects: &'context ObjectWorkspace,
    pub(super) reference_graph: &'context MultiFileReferenceGraph<'graph>,
    pub(super) settings: &'context ExportSettingsFile,
    pub(super) routed: &'context [RoutedObject<'object>],
    pub(super) saved_assemblies: &'context [SavedAssemblyExport],
    pub(super) editor_version_text: &'context str,
    pub(super) project_version: SceneUnityVersion,
    pub(super) assembly_manager_is_set: bool,
    pub(super) pointer_map: &'context ProjectExportPointerMap,
    pub(super) streamed_resources: WorkspaceStreamedYamlResourceContext<'context, 'provider>,
    pub(super) processed_animation_roots: &'context HashMap<(usize, usize), SerializedValue>,
    pub(super) limits: WorkspaceProjectExportLimits,
    /// Bundle behaviours split out of controller hierarchies, keyed by
    /// workspace object. Their singleton documents render the stamped flags.
    pub(super) split_behaviour_keys: &'context HashSet<(usize, usize)>,
}

pub(super) struct PrefabPlanningContext<'context, 'graph> {
    pub(super) workspace: &'context SerializedFileWorkspace,
    pub(super) objects: &'context ObjectWorkspace,
    pub(super) reference_graph: &'context MultiFileReferenceGraph<'graph>,
    pub(super) pointer_map: &'context ProjectExportPointerMap,
    pub(super) prefabs: &'context [RoutedPrefabCollection],
    pub(super) scene_processing: &'context WorkspaceSceneProcessing,
    pub(super) editor_version_text: &'context str,
    pub(super) project_version: SceneUnityVersion,
    pub(super) limits: WorkspaceProjectExportLimits,
}

pub(super) struct ScenePlanningContext<'context, 'graph, 'provider> {
    pub(super) workspace: &'context SerializedFileWorkspace,
    pub(super) objects: &'context ObjectWorkspace,
    pub(super) reference_graph: &'context MultiFileReferenceGraph<'graph>,
    pub(super) pointer_map: &'context ProjectExportPointerMap,
    pub(super) scenes: &'context [RoutedSceneCollection],
    pub(super) scene_processing: &'context WorkspaceSceneProcessing,
    pub(super) lighting_data_assets: &'context [RoutedLightingDataAsset],
    pub(super) streamed_resources: WorkspaceStreamedYamlResourceContext<'context, 'provider>,
    pub(super) project_version: SceneUnityVersion,
    pub(super) limits: WorkspaceProjectExportLimits,
}

pub(super) struct LightingDataPlanningContext<'context, 'object, 'graph> {
    pub(super) lighting_data_assets: &'context [RoutedLightingDataAsset],
    pub(super) reference_graph: &'context MultiFileReferenceGraph<'graph>,
    pub(super) pointer_map: &'context ProjectExportPointerMap,
    pub(super) routed: &'context [RoutedObject<'object>],
    pub(super) limits: WorkspaceProjectExportLimits,
    pub(super) project_version: SceneUnityVersion,
}

pub(super) struct StagedProjectFiles {
    pub(super) files: Vec<PlannedProjectFile>,
    pub(super) support_files: Vec<PlannedProjectSupportFile>,
    pub(super) total_bytes: usize,
    pub(super) exported_object_count: usize,
}

pub(super) struct ProjectExportPlanBuilder {
    planner: ProjectPathPlanner,
    files: Vec<PlannedProjectFile>,
    support_files: Vec<PlannedProjectSupportFile>,
    support_file_indexes: HashMap<String, usize>,
    total_bytes: usize,
    exported_object_count: usize,
    /// GUID mint state forwarded from routing; the controller arm mints file
    /// GUIDs for hierarchy-dropped sub-assets that export separately.
    pub(super) next_guid_sequence: u64,
    pub(super) used_guids: HashSet<[u8; 16]>,
}

impl ProjectExportPlanBuilder {
    pub(super) fn new(
        collection_count: usize,
        saved_assembly_count: usize,
    ) -> Result<Self, Box<dyn Error>> {
        let staged_file_capacity = collection_count
            .checked_add(saved_assembly_count)
            .ok_or_else(|| invalid_project("staged project file count overflows"))?;
        let mut files = Vec::new();
        files
            .try_reserve_exact(staged_file_capacity)
            .map_err(|_| invalid_project("could not allocate staged project asset files"))?;
        Ok(Self {
            planner: ProjectPathPlanner::new(ProjectPathPlanner::host_platform()),
            files,
            support_files: Vec::new(),
            support_file_indexes: HashMap::new(),
            total_bytes: 0,
            exported_object_count: 0,
            next_guid_sequence: 0,
            used_guids: HashSet::new(),
        })
    }

    pub(super) fn finish(self) -> StagedProjectFiles {
        StagedProjectFiles {
            files: self.files,
            support_files: self.support_files,
            total_bytes: self.total_bytes,
            exported_object_count: self.exported_object_count,
        }
    }
}

pub(super) fn plan_routed_asset_files(
    context: &RoutedAssetPlanningContext<'_, '_, '_, '_>,
    builder: &mut ProjectExportPlanBuilder,
    notices: &mut Vec<ProjectExportNotice>,
) -> Result<(), Box<dyn Error>> {
    let RoutedAssetPlanningContext {
        filesystem,
        root_catalog,
        resource_provider,
        workspace,
        objects,
        reference_graph,
        settings,
        routed,
        saved_assemblies,
        editor_version_text,
        project_version,
        assembly_manager_is_set,
        pointer_map,
        streamed_resources,
        processed_animation_roots,
        limits,
        split_behaviour_keys,
    } = *context;
    let mut total_bytes = builder.total_bytes;
    let mut exported_object_count = builder.exported_object_count;
    let planner = &mut builder.planner;
    let files = &mut builder.files;
    let support_files = &mut builder.support_files;
    let support_file_indexes = &mut builder.support_file_indexes;
    let mut raw_file_cache: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut saved_assemblies_emitted = false;
    for entry in routed {
        if entry.class_id == 115 && !saved_assemblies_emitted {
            for assembly in saved_assemblies {
                let bytes = materialize_assembly_export(
                    filesystem,
                    assembly,
                    limits
                        .max_total_asset_payload_bytes
                        .saturating_sub(total_bytes),
                )?;
                total_bytes = total_bytes
                    .checked_add(bytes.len())
                    .ok_or_else(|| invalid_project("project output byte count overflows"))?;
                if total_bytes > limits.max_total_asset_payload_bytes {
                    return Err(invalid_project(format!(
                        "project output uses {total_bytes} bytes, exceeding the limit {}",
                        limits.max_total_asset_payload_bytes
                    )));
                }
                files.push(PlannedProjectFile {
                    collection_name: "ScriptExportCollection",
                    kind: ExportCollectionKind::Script,
                    relative_path: assembly.relative_path.clone(),
                    guid: assembly.guid,
                    bytes,
                    meta: Some(PlannedProjectMeta {
                        importer_class: "PluginImporter".to_owned(),
                        importer: plugin_importer_yaml(editor_version_text)?,
                    }),
                    label: assembly.assembly_name.clone(),
                });
            }
            saved_assemblies_emitted = true;
        }
        if matches!(
            entry.route,
            ObjectRoute::Skipped | ObjectRoute::EngineRedirect
        ) {
            continue;
        }
        if entry.route == ObjectRoute::ScriptPointerOnly {
            continue;
        }
        if entry.route == ObjectRoute::ShaderYaml {
            let patch_version = (
                project_version.major,
                project_version.minor,
                project_version.patch,
            );
            for (name, text) in yaml_shader_patch_texts(yaml_shader_patches(patch_version)) {
                let relative_path = format!(
                    "{}{}",
                    unity_patches::PATCHES_RELATIVE_DIRECTORY,
                    unity_patches::patch_file_name(name)
                );
                add_project_support_file(
                    support_files,
                    support_file_indexes,
                    relative_path,
                    text.as_bytes(),
                    files.len(),
                )?;
            }
        }
        let mut identity = crate::export::asset_path::ExportAssetPathIdentity {
            class_name: entry.class_name.clone(),
            named_name: entry.named_name.clone(),
            original_directory: entry.original_directory.clone(),
            ..Default::default()
        };
        if identity.class_name.is_empty() {
            identity.class_name = entry.class_name.clone();
        }
        let planned_audio_asset_path = match entry.route {
            ObjectRoute::AudioYaml => Some(planner.plan_asset_path(&identity, "audioclip")?),
            ObjectRoute::AudioBinary => {
                let audio = entry.audio_binary_export.as_ref().ok_or_else(|| {
                    invalid_project("AudioClip binary route has no decoded audio details")
                })?;
                if audio.decoded_extension == "ogg" && audio.collection_extension == "wav" {
                    return Err(invalid_project(format!(
                        "AudioClip {}:{} requires Ogg-to-WAV conversion under PreferWav, which is not implemented",
                        entry.object.file_index, entry.object.path_id
                    )));
                }
                Some(planner.plan_asset_path(&identity, &audio.collection_extension)?)
            }
            _ => None,
        };
        let planned_audio_sidecar_path = match (
            entry.audio_yaml_resource.as_ref(),
            planned_audio_asset_path.as_ref(),
        ) {
            (Some(resource), Some(asset_path)) if resource.size > 0 => {
                Some(format!("{asset_path}.resS"))
            }
            (Some(resource), None) if resource.size > 0 => {
                return Err(invalid_project(
                    "AudioClip stream has no planned AudioClip asset path",
                ));
            }
            _ => None,
        };
        let planned_audio_yaml_sidecar_source = planned_audio_sidecar_path
            .as_deref()
            .map(project_relative_path_for_host);
        let audio_sidecar_bytes = if let Some(resource) = entry
            .audio_yaml_resource
            .as_ref()
            .filter(|resource| resource.size > 0)
        {
            if resource.path.is_empty() {
                return Err(invalid_project(format!(
                    "file {} object {} PathID {} AudioClip declares {} streamed bytes with an empty resource path",
                    entry.object.file_index,
                    entry.object.object_index,
                    entry.object.path_id,
                    resource.size
                )));
            }
            let source_bundle_index = workspace
                .files()
                .get(entry.object.file_index)
                .ok_or_else(|| {
                    invalid_project(format!(
                        "AudioClip references absent SerializedFile {}",
                        entry.object.file_index
                    ))
                })?
                .container_bundle_index;
            let resolution = resolve_and_read_streamed_resource_range(
                filesystem,
                workspace,
                source_bundle_index,
                StreamedResourceRange {
                    path: &resource.path,
                    offset: resource.offset,
                    size: resource.size,
                },
                root_catalog,
                resource_provider,
                limits.streamed_content,
            )
            .map_err(|error| {
                invalid_project(format!(
                    "file {} object {} PathID {} AudioClip stream {:?} could not be read: {error}",
                    entry.object.file_index,
                    entry.object.object_index,
                    entry.object.path_id,
                    resource.path
                ))
            })?;
            match resolution {
                StreamedResourceContentResolution::Resolved { bytes, .. } => Some(bytes),
                StreamedResourceContentResolution::Missing {
                    unavailable_data_paths,
                } => {
                    return Err(invalid_project(format!(
                        "file {} object {} PathID {} AudioClip stream {:?} is missing; unavailable data paths: [{}]",
                        entry.object.file_index,
                        entry.object.object_index,
                        entry.object.path_id,
                        resource.path,
                        unavailable_data_paths.join(", ")
                    )));
                }
                StreamedResourceContentResolution::Unset => {
                    return Err(invalid_project(format!(
                        "file {} object {} PathID {} AudioClip stream {:?} became unset during resolution",
                        entry.object.file_index,
                        entry.object.object_index,
                        entry.object.path_id,
                        resource.path
                    )));
                }
            }
        } else {
            None
        };
        let mut additional_exported_document_count = 0usize;
        let bytes = match entry.route {
            ObjectRoute::Unknown | ObjectRoute::Unreadable => raw_payload_bytes(
                filesystem,
                workspace,
                entry.object,
                &mut raw_file_cache,
                limits,
            )?,
            ObjectRoute::AudioBinary => entry
                .audio_binary_export
                .as_ref()
                .ok_or_else(|| {
                    invalid_project("AudioClip binary route has no decoded audio bytes")
                })?
                .bytes
                .clone(),
            ObjectRoute::ShaderText => {
                let shader = entry.shader_view.as_ref().ok_or_else(|| {
                    invalid_project(format!(
                        "{} selected a text Shader exporter without a decoded Shader view",
                        entry.selection.exporter
                    ))
                })?;
                export_shader_text(shader, settings.shader_export_mode).map_err(|error| {
                    invalid_project(format!(
                        "file {} object {} PathID {} Shader text export failed: {error}",
                        entry.object.file_index, entry.object.object_index, entry.object.path_id
                    ))
                })?
            }
            ObjectRoute::Script => {
                let script = entry.script_export.as_ref().ok_or_else(|| {
                    invalid_project("MonoScript route has no validated script identity")
                })?;
                empty_script_content(&script.identity.namespace, &script.identity.class)
                    .map_err(|error| {
                        invalid_project(format!(
                            "MonoScript dummy source generation failed: {error}"
                        ))
                    })?
                    .into_bytes()
            }
            ObjectRoute::ScriptPointerOnly => {
                unreachable!("pointer-only MonoScripts are filtered before file planning")
            }
            ObjectRoute::TextAsset => {
                let script = &entry
                    .text_asset_export
                    .as_ref()
                    .ok_or_else(|| invalid_project("TextAsset route has no script bytes"))?
                    .script;
                let mut output = Vec::new();
                output
                    .try_reserve_exact(script.len())
                    .map_err(|_| invalid_project("could not allocate TextAsset export bytes"))?;
                output.extend_from_slice(script);
                output
            }
            ObjectRoute::FontBinary => {
                let payload = entry.object.payload.as_ref().map_err(|error| {
                    invalid_project(format!(
                        "Font {}:{} has no decoded payload: {error}",
                        entry.object.file_index, entry.object.path_id
                    ))
                })?;
                let data = font_asset_data(&payload.root)?.ok_or_else(|| {
                    invalid_project(format!(
                        "Font {}:{} has no m_FontData for binary export",
                        entry.object.file_index, entry.object.path_id
                    ))
                })?;
                if data.is_empty() {
                    return Err(invalid_project(format!(
                        "Font {}:{} has empty m_FontData after FontAssetExporter admission",
                        entry.object.file_index, entry.object.path_id
                    )));
                }
                data.to_vec()?
            }
            ObjectRoute::TextureImage => {
                let texture = entry.texture_export.as_ref().ok_or_else(|| {
                    invalid_project("TextureExportCollection route has no decoded texture plan")
                })?;
                let loaded_file =
                    workspace
                        .files()
                        .get(entry.object.file_index)
                        .ok_or_else(|| {
                            invalid_project(format!(
                                "texture references absent SerializedFile {}",
                                entry.object.file_index
                            ))
                        })?;
                let stream_content = if texture.view.image_data().is_empty() {
                    if let Some(stream_info) =
                        texture.view.stream_data().filter(|info| info.is_set())
                    {
                        match resolve_and_read_streamed_resource_info(
                            filesystem,
                            workspace,
                            loaded_file.container_bundle_index,
                            stream_info,
                            root_catalog,
                            resource_provider,
                            limits.streamed_content,
                        )
                        .map_err(|error| {
                            invalid_project(format!(
                                "file {} object {} PathID {} texture stream {:?} could not be read: {error}",
                                entry.object.file_index,
                                entry.object.object_index,
                                entry.object.path_id,
                                stream_info.path
                            ))
                        })? {
                            StreamedResourceContentResolution::Resolved { bytes, .. } => {
                                Some(bytes)
                            }
                            StreamedResourceContentResolution::Missing {
                                unavailable_data_paths,
                            } => {
                                return Err(invalid_project(format!(
                                    "file {} object {} PathID {} texture stream {:?} is missing; unavailable data paths: [{}]",
                                    entry.object.file_index,
                                    entry.object.object_index,
                                    entry.object.path_id,
                                    stream_info.path,
                                    unavailable_data_paths.join(", ")
                                )))
                            }
                            StreamedResourceContentResolution::Unset => {
                                return Err(invalid_project(format!(
                                    "file {} object {} PathID {} texture stream {:?} became unset during resolution",
                                    entry.object.file_index,
                                    entry.object.object_index,
                                    entry.object.path_id,
                                    stream_info.path
                                )))
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                texture
                    .export_image(
                        stream_content.as_deref(),
                        loaded_file
                            .metadata
                            .prefix
                            .target_platform
                            .unwrap_or_default() as i32,
                        loaded_file.metadata.prefix.body_endian
                            == crate::io::binary::Endian::Little,
                    )
                    .map_err(|error| {
                        invalid_project(format!(
                            "file {} object {} PathID {} texture image conversion failed: {error}",
                            entry.object.file_index,
                            entry.object.object_index,
                            entry.object.path_id
                        ))
                    })?
            }
            ObjectRoute::Asset
            | ObjectRoute::AudioYaml
            | ObjectRoute::ShaderYaml
            | ObjectRoute::Manager => {
                let source = identity_of(entry);
                let animation_clip_root = processed_animation_roots
                    .get(&(entry.object.file_index, entry.object.object_index));
                let document =
                    workspace_object_to_unity_yaml_document_with_processed_animation_root(
                        workspace,
                        reference_graph,
                        pointer_map,
                        entry.object,
                        source,
                        entry.file_id,
                        entry.collection_index,
                        None,
                        limits.yaml_collection.object_conversion,
                        animation_clip_root,
                        &streamed_resources,
                        true,
                    )?;
                let Some(mut document) = document else {
                    if entry.scriptable_object_group.is_some()
                        || entry.controller_collection.is_some()
                    {
                        return Err(invalid_project(format!(
                            "{} root {}:{} produced no YAML document",
                            entry.selection.collection_name,
                            entry.object.file_index,
                            entry.object.path_id
                        )));
                    }
                    notices.push(ProjectExportNotice {
                        subject: entry.selection.collection_name.to_owned(),
                        reason:
                            "the baseline exporter skipped an image texture with no usable data",
                    });
                    continue;
                };
                // Split controller behaviours render the processor-stamped
                // HideFlags, mirroring range-resolved MainAsset objects.
                if entry.class_id == 114
                    && split_behaviour_keys
                        .contains(&(entry.object.file_index, entry.object.object_index))
                {
                    hide_controller_behaviour(&mut document)?;
                }
                if entry.route == ObjectRoute::AudioYaml {
                    add_generated_audio_yaml_defaults(
                        &mut document,
                        match &entry.object.factory.kind {
                            FactoryAssetKind::Generated(generated_type) => {
                                Some(generated_type.as_str())
                            }
                            _ => None,
                        },
                    )?;
                    if let Some(resource) = entry.audio_yaml_resource.as_ref() {
                        rewrite_audio_yaml_resource(
                            &mut document,
                            resource,
                            planned_audio_yaml_sidecar_source.as_deref(),
                            audio_sidecar_bytes.as_deref(),
                        )?;
                    }
                }
                if let Some(group) = &entry.scriptable_object_group {
                    let document_capacity =
                        group.children.len().checked_add(1).ok_or_else(|| {
                            invalid_project("ScriptableObjectGroup document count overflows")
                        })?;
                    let mut documents = Vec::new();
                    documents
                        .try_reserve_exact(document_capacity)
                        .map_err(|_| {
                            invalid_project(format!(
                                "could not allocate {} ScriptableObjectGroup YAML documents",
                                document_capacity
                            ))
                        })?;
                    documents.push(document);
                    for child in &group.children {
                        let source = ExportAssetIdentity {
                            collection: ExportAssetCollectionIdentity::SerializedFile {
                                file_index: child.object.file_index,
                            },
                            object_index: child.object.object_index,
                            path_id: child.object.path_id,
                            class_id: child.object.factory.class_id,
                        };
                        let document =
                            workspace_object_to_unity_yaml_document_with_streamed_resources(
                                workspace,
                                reference_graph,
                                pointer_map,
                                child.object,
                                source,
                                child.file_id,
                                entry.collection_index,
                                None,
                                limits.yaml_collection.object_conversion,
                                &streamed_resources,
                            )?
                            .ok_or_else(|| {
                                invalid_project(format!(
                                    "ScriptableObjectGroup member {}:{} produced no YAML document",
                                    child.object.file_index, child.object.path_id
                                ))
                            })?;
                        documents.push(document);
                    }
                    additional_exported_document_count = documents.len().saturating_sub(1);
                    write_unity_yaml_documents(&documents, limits.yaml_write)?.into_bytes()
                } else if let Some(group) = &entry.controller_collection {
                    let source_file =
                        workspace
                            .files()
                            .get(entry.object.file_index)
                            .ok_or_else(|| {
                                invalid_project(
                                    "AnimatorController references absent workspace file",
                                )
                            })?;
                    let rebuilt = if group.is_release {
                        let payload = entry.object.payload.as_ref().map_err(|error| invalid_project(format!(
                            "AnimatorController file {} object {} PathID {} could not be decoded: {error}",
                            entry.object.file_index, entry.object.object_index, entry.object.path_id,
                        )))?;
                        rebuild_animator_controller_documents(
                            document,
                            payload,
                            source_file.metadata.prefix.body_endian
                                == crate::io::binary::Endian::Little,
                            project_version,
                            &mut builder.next_guid_sequence,
                            &mut builder.used_guids,
                        )?
                    } else {
                        RebuiltControllerDocuments {
                            documents: vec![document],
                            dropped: Vec::new(),
                        }
                    };
                    let mut documents = rebuilt.documents;
                    documents.try_reserve(group.children.len()).map_err(|_| {
                        invalid_project("could not allocate AnimatorController member documents")
                    })?;
                    for child in &group.children {
                        let identity = ExportAssetIdentity {
                            collection: ExportAssetCollectionIdentity::SerializedFile {
                                file_index: child.object.file_index,
                            },
                            object_index: child.object.object_index,
                            path_id: child.object.path_id,
                            class_id: child.object.factory.class_id,
                        };
                        let mut document =
                            workspace_object_to_unity_yaml_document_with_streamed_resources(
                                workspace,
                                reference_graph,
                                pointer_map,
                                child.object,
                                identity,
                                child.file_id,
                                entry.collection_index,
                                None,
                                limits.yaml_collection.object_conversion,
                                &streamed_resources,
                            )?
                            .ok_or_else(|| {
                                invalid_project(format!(
                                    "AnimatorController member {}:{} produced no YAML document",
                                    child.object.file_index, child.object.path_id,
                                ))
                            })?;
                        if group.is_release && child.object.factory.class_id == 114 {
                            hide_controller_behaviour(&mut document)?;
                        }
                        documents.push(document);
                    }
                    if group.is_release {
                        normalize_controller_collection_documents(&mut documents, project_version)?;
                    }
                    additional_exported_document_count = documents.len().saturating_sub(1);
                    let controller_bytes =
                        write_unity_yaml_documents(&documents, limits.yaml_write)?.into_bytes();
                    // Hierarchy-dropped states and BlendTrees export as separate
                    // single-asset files, mirroring the frozen exporter's orphan
                    // `.state`/`.blendtree` files with class-main anchors.
                    for dropped in rebuilt.dropped {
                        let (class_name, extension) = match dropped.class_id {
                            1102 => ("AnimatorState", "state"),
                            206 => ("BlendTree", "blendtree"),
                            other => {
                                return Err(invalid_project(format!(
                                    "AnimatorController dropped asset has unexpected class {other}"
                                )));
                            }
                        };
                        let identity = crate::export::asset_path::ExportAssetPathIdentity {
                            class_name: class_name.to_owned(),
                            named_name: Some(dropped.name.clone()),
                            ..Default::default()
                        };
                        let relative_path = planner.plan_asset_path(&identity, extension)?;
                        let bytes = write_unity_yaml_documents(
                            std::slice::from_ref(&dropped.document),
                            limits.yaml_write,
                        )?
                        .into_bytes();
                        total_bytes = total_bytes.checked_add(bytes.len()).ok_or_else(|| {
                            invalid_project("project output byte count overflows")
                        })?;
                        if total_bytes > limits.max_total_asset_payload_bytes {
                            return Err(invalid_project(format!(
                                "project output uses {total_bytes} bytes, exceeding the limit {}",
                                limits.max_total_asset_payload_bytes
                            )));
                        }
                        exported_object_count = exported_object_count
                            .checked_add(1)
                            .ok_or_else(|| invalid_project("exported object count overflows"))?;
                        files.push(PlannedProjectFile {
                            collection_name: "AssetExportCollection",
                            kind: ExportCollectionKind::Asset,
                            relative_path,
                            guid: dropped.guid,
                            bytes,
                            meta: Some(PlannedProjectMeta {
                                importer_class: "NativeFormatImporter".to_owned(),
                                importer: native_format_importer_for_version(
                                    dropped.main_file_id,
                                    project_version,
                                    None,
                                )?,
                            }),
                            label: dropped.name,
                        });
                    }
                    controller_bytes
                } else {
                    write_unity_yaml_documents(&[document], limits.yaml_write)?.into_bytes()
                }
            }
            ObjectRoute::EngineRedirect | ObjectRoute::Skipped => {
                unreachable!("non-exportable routes are filtered above")
            }
        };
        exported_object_count = exported_object_count
            .checked_add(1)
            .and_then(|count| count.checked_add(additional_exported_document_count))
            .ok_or_else(|| invalid_project("exported object count overflows"))?;
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid_project("project output byte count overflows"))?;
        if total_bytes > limits.max_total_asset_payload_bytes {
            return Err(invalid_project(format!(
                "project output uses {total_bytes} bytes, exceeding the limit {}",
                limits.max_total_asset_payload_bytes
            )));
        }
        if let Some(sidecar_bytes) = audio_sidecar_bytes.as_deref() {
            let relative_path = planned_audio_sidecar_path.as_ref().ok_or_else(|| {
                invalid_project("AudioClip sidecar bytes have no planned support-file path")
            })?;
            total_bytes = total_bytes
                .checked_add(sidecar_bytes.len())
                .ok_or_else(|| invalid_project("project output byte count overflows"))?;
            if total_bytes > limits.max_total_asset_payload_bytes {
                return Err(invalid_project(format!(
                    "project output uses {total_bytes} bytes, exceeding the limit {}",
                    limits.max_total_asset_payload_bytes
                )));
            }
            add_project_support_file(
                support_files,
                support_file_indexes,
                relative_path.clone(),
                sidecar_bytes,
                files.len(),
            )?;
        }
        let relative_path = match entry.route {
            ObjectRoute::Asset => {
                if let Some(group) = &entry.scriptable_object_group {
                    planner.plan_asset_path(&identity, group.extension)?
                } else {
                    let extension = export_extension_for(entry.facts);
                    let extension = if extension.is_empty() {
                        DEFAULT_EXPORT_EXTENSION
                    } else {
                        extension
                    };
                    planner.plan_asset_path(&identity, extension)?
                }
            }
            ObjectRoute::AudioYaml => planned_audio_asset_path
                .ok_or_else(|| invalid_project("AudioYaml route has no planned asset path"))?,
            ObjectRoute::AudioBinary => planned_audio_asset_path
                .ok_or_else(|| invalid_project("AudioBinary route has no planned asset path"))?,
            ObjectRoute::TextureImage => {
                let texture = entry.texture_export.as_ref().ok_or_else(|| {
                    invalid_project("TextureExportCollection route has no planned image extension")
                })?;
                planner.plan_asset_path(&identity, &texture.extension)?
            }
            ObjectRoute::TextAsset => {
                let text_asset = entry
                    .text_asset_export
                    .as_ref()
                    .ok_or_else(|| invalid_project("TextAsset route has no planned extension"))?;
                planner.plan_asset_path(&identity, &text_asset.extension)?
            }
            ObjectRoute::FontBinary => {
                let payload = entry.object.payload.as_ref().map_err(|error| {
                    invalid_project(format!(
                        "Font {}:{} has no decoded payload while planning its extension: {error}",
                        entry.object.file_index, entry.object.path_id
                    ))
                })?;
                let data = font_asset_data(&payload.root)?.ok_or_else(|| {
                    invalid_project(format!(
                        "Font {}:{} has no m_FontData while planning its extension",
                        entry.object.file_index, entry.object.path_id
                    ))
                })?;
                planner.plan_asset_path(&identity, data.extension()?)?
            }
            ObjectRoute::ShaderText => planner.plan_asset_path(&identity, "shader")?,
            ObjectRoute::ShaderYaml => planner.plan_asset_path(&identity, YAML_SHADER_EXTENSION)?,
            ObjectRoute::Script => {
                let script = entry.script_export.as_ref().ok_or_else(|| {
                    invalid_project("MonoScript route has no validated script identity")
                })?;
                plan_script_export_paths(
                    &script.identity.assembly,
                    &script.identity.namespace,
                    &script.identity.class,
                    ProjectPathPlanner::host_platform(),
                    "",
                )?
                .relative_asset_path
            }
            ObjectRoute::ScriptPointerOnly => {
                unreachable!("pointer-only MonoScripts are filtered before path planning")
            }
            ObjectRoute::EngineRedirect => {
                unreachable!("engine redirects are filtered before path planning")
            }
            ObjectRoute::Manager => planner.plan_manager_path(&entry.class_name)?,
            ObjectRoute::Unknown => planner.plan_unknown_path(
                &entry.class_name,
                entry.named_name.as_deref().unwrap_or(&entry.class_name),
            )?,
            ObjectRoute::Unreadable => planner.plan_unreadable_path(
                &entry.class_name,
                entry.named_name.as_deref().unwrap_or(&entry.class_name),
            )?,
            ObjectRoute::Skipped => unreachable!("skipped routes are filtered above"),
        };
        let meta = match entry.route {
            ObjectRoute::Asset | ObjectRoute::AudioYaml | ObjectRoute::ShaderYaml => {
                let asset_bundle_name = entry
                    .object
                    .payload
                    .as_ref()
                    .ok()
                    .and_then(object_asset_bundle_name);
                let importer = if entry.class_id == 43 {
                    mesh_native_format_importer_for_version(
                        entry.file_id,
                        project_version,
                        asset_bundle_name.as_deref(),
                    )?
                } else {
                    native_format_importer_for_version(
                        entry.file_id,
                        project_version,
                        asset_bundle_name.as_deref(),
                    )?
                };
                Some(PlannedProjectMeta {
                    importer_class: "NativeFormatImporter".to_owned(),
                    importer,
                })
            }
            ObjectRoute::AudioBinary => {
                let audio = entry.audio_binary_export.as_ref().ok_or_else(|| {
                    invalid_project("AudioClip binary route has no importer metadata")
                })?;
                Some(PlannedProjectMeta {
                    importer_class: "AudioImporter".to_owned(),
                    importer: audio.importer.clone(),
                })
            }
            ObjectRoute::TextureImage => Some(PlannedProjectMeta {
                importer_class: "TextureImporter".to_owned(),
                importer: entry
                    .texture_export
                    .as_ref()
                    .ok_or_else(|| {
                        invalid_project("TextureExportCollection route has no importer data")
                    })?
                    .importer
                    .clone(),
            }),
            ObjectRoute::TextAsset => {
                let asset_bundle_name = entry
                    .object
                    .payload
                    .as_ref()
                    .ok()
                    .and_then(object_asset_bundle_name);
                Some(PlannedProjectMeta {
                    importer_class: "TextScriptImporter".to_owned(),
                    importer: default_importer(asset_bundle_name.as_deref())?,
                })
            }
            ObjectRoute::FontBinary => {
                let payload = entry.object.payload.as_ref().map_err(|error| {
                    invalid_project(format!(
                        "Font {}:{} has no decoded payload for TrueTypeFontImporter metadata: {error}",
                        entry.object.file_index, entry.object.path_id
                    ))
                })?;
                let values = font_importer_values(payload)?;
                let fallback_references = font_fallback_references_yaml(
                    entry,
                    &values,
                    objects,
                    reference_graph,
                    pointer_map,
                    project_version,
                )?;
                Some(PlannedProjectMeta {
                    importer_class: "TrueTypeFontImporter".to_owned(),
                    importer: font_importer_yaml(&values, fallback_references)?,
                })
            }
            ObjectRoute::Script => {
                let script = entry.script_export.as_ref().ok_or_else(|| {
                    invalid_project("MonoScript route has no validated script identity")
                })?;
                Some(PlannedProjectMeta {
                    importer_class: "MonoImporter".to_owned(),
                    importer: mono_importer_yaml(script.execution_order)?,
                })
            }
            ObjectRoute::ScriptPointerOnly => {
                unreachable!("pointer-only MonoScripts are filtered before metadata planning")
            }
            ObjectRoute::ShaderText => {
                let shader = entry.shader_view.as_ref().ok_or_else(|| {
                    invalid_project("ShaderImporter requires a decoded Shader view")
                })?;
                let non_modifiable_textures = shader_non_modifiable_textures_yaml(
                    shader,
                    entry,
                    objects,
                    reference_graph,
                    pointer_map,
                    project_version,
                )?;
                Some(PlannedProjectMeta {
                    importer_class: "ShaderImporter".to_owned(),
                    importer: shader_importer_for_version_with_non_modifiable_textures(
                        project_version,
                        entry
                            .object
                            .payload
                            .as_ref()
                            .ok()
                            .and_then(object_asset_bundle_name)
                            .as_deref(),
                        non_modifiable_textures,
                    )?,
                })
            }
            ObjectRoute::Manager
            | ObjectRoute::EngineRedirect
            | ObjectRoute::Unknown
            | ObjectRoute::Unreadable => None,
            ObjectRoute::Skipped => unreachable!("skipped routes are filtered above"),
        };
        files.push(PlannedProjectFile {
            collection_name: entry.selection.collection_name,
            kind: entry.selection.kind(),
            relative_path,
            guid: entry.guid,
            bytes,
            meta,
            label: entry.selection.collection_name.to_owned(),
        });
    }

    // `EmptyScriptExportCollection` writes an asmdef for each non-predefined assembly when no
    // managed assembly manager was initialized. Its details have no module, so the source JSON
    // contains an empty reference array.
    if !assembly_manager_is_set
        && emit_assembly_definitions(
            routed
                .iter()
                .any(|entry| entry.route == ObjectRoute::Script),
            project_version,
        )
    {
        let mut emitted_assemblies = HashSet::new();
        for entry in routed {
            if entry.route != ObjectRoute::Script {
                continue;
            }
            let script = entry.script_export.as_ref().ok_or_else(|| {
                invalid_project("MonoScript route has no validated script identity")
            })?;
            let assembly = &script.identity.assembly;
            if is_predefined_assembly(assembly) || !emitted_assemblies.insert(assembly.clone()) {
                continue;
            }
            let assembly_folder = remove_assembly_file_extension(assembly);
            let relative_path = format!(
                "Assets/{}/{}/{}",
                scripts_folder_name(assembly_folder),
                assembly_folder,
                assembly_definition_file_name(assembly_folder)
            );
            let bytes = assembly_definition_json(assembly_folder, &[]).into_bytes();
            add_project_support_file(
                support_files,
                support_file_indexes,
                relative_path,
                &bytes,
                files.len(),
            )?;
        }
    }

    builder.total_bytes = total_bytes;
    builder.exported_object_count = exported_object_count;
    Ok(())
}
pub(super) fn plan_prefab_files(
    context: &PrefabPlanningContext<'_, '_>,
    builder: &mut ProjectExportPlanBuilder,
) -> Result<(), Box<dyn Error>> {
    let PrefabPlanningContext {
        workspace,
        objects,
        reference_graph,
        pointer_map,
        prefabs,
        scene_processing,
        editor_version_text,
        project_version,
        limits,
    } = *context;
    let mut total_bytes = builder.total_bytes;
    let mut exported_object_count = builder.exported_object_count;
    let planner = &mut builder.planner;
    let files = &mut builder.files;
    for prefab in prefabs {
        let mut generated_transform_context =
            WorkspaceGeneratedTransformYamlContext::with_materialized_prefab_transforms(
                &scene_processing.processed_assets,
                &scene_processing.generated_transforms,
            );
        let generated_legacy_prefab = prefab.representation
            == PrefabAssetRepresentation::LegacyPrefabMarker
            && prefab
                .export_ids
                .registered_assets
                .iter()
                .any(|assignment| {
                    assignment.is_main_asset
                        && matches!(
                            assignment.asset.collection,
                            ExportAssetCollectionIdentity::ProcessedCollection { .. }
                        )
                });
        if generated_legacy_prefab {
            generated_transform_context = generated_transform_context
                .with_legacy_prefab_export(&prefab.export_ids, editor_version_text);
        }
        let yaml = workspace_prefab_hierarchy_to_unity_yaml_with_generated_transforms(
            workspace,
            objects,
            reference_graph,
            pointer_map,
            &prefab.export_ids,
            prefab.collection_index,
            None,
            limits.yaml_collection,
            &generated_transform_context,
        )?;
        exported_object_count = exported_object_count
            .checked_add(count_unity_yaml_documents(&yaml))
            .ok_or_else(|| invalid_project("exported object count overflows"))?;
        total_bytes = total_bytes
            .checked_add(yaml.len())
            .ok_or_else(|| invalid_project("project output byte count overflows"))?;
        if total_bytes > limits.max_total_asset_payload_bytes {
            return Err(invalid_project(format!(
                "project output uses {total_bytes} bytes, exceeding the limit {}",
                limits.max_total_asset_payload_bytes
            )));
        }

        let relative_path = planner.plan_asset_path(
            &crate::export::asset_path::ExportAssetPathIdentity {
                class_name: prefab.class_name.clone(),
                named_name: prefab.named_name.clone(),
                original_directory: prefab.original_directory.clone(),
                ..Default::default()
            },
            "prefab",
        )?;
        let meta = if prefab.representation == PrefabAssetRepresentation::LegacyPrefabMarker {
            let main_file_id = prefab
                .export_ids
                .registered_assets
                .iter()
                .find(|assignment| assignment.is_main_asset)
                .map(|assignment| assignment.file_id)
                .ok_or_else(|| invalid_project("Prefab export plan has no main asset ID"))?;
            Some(PlannedProjectMeta {
                importer_class: "NativeFormatImporter".to_owned(),
                importer: native_format_importer_for_version(
                    main_file_id,
                    project_version,
                    prefab.asset_bundle_name.as_deref(),
                )?,
            })
        } else {
            Some(PlannedProjectMeta {
                importer_class: "PrefabImporter".to_owned(),
                importer: prefab_importer(project_version, prefab.asset_bundle_name.as_deref())?,
            })
        };
        files.push(PlannedProjectFile {
            collection_name: "PrefabExportCollection",
            kind: ExportCollectionKind::Prefab,
            relative_path,
            guid: prefab.guid,
            bytes: yaml.into_bytes(),
            meta,
            label: prefab.name.clone(),
        });
    }

    builder.total_bytes = total_bytes;
    builder.exported_object_count = exported_object_count;
    Ok(())
}
pub(super) fn plan_scene_files(
    context: &ScenePlanningContext<'_, '_, '_>,
    builder: &mut ProjectExportPlanBuilder,
    notices: &mut Vec<ProjectExportNotice>,
) -> Result<(), Box<dyn Error>> {
    let ScenePlanningContext {
        workspace,
        objects,
        reference_graph,
        pointer_map,
        scenes,
        scene_processing,
        lighting_data_assets,
        streamed_resources,
        project_version,
        limits,
    } = *context;
    let mut total_bytes = builder.total_bytes;
    let mut exported_object_count = builder.exported_object_count;
    let files = &mut builder.files;
    let mut scene_paths_seen: HashSet<String> = HashSet::new();
    for scene in scenes {
        let path_plan =
            plan_scene_export_paths(&scene.path, ProjectPathPlanner::host_platform(), "")?;
        if scene_paths_seen.contains(&path_plan.relative_path) {
            if scene_build_settings_entry_is_duplicate(scene_processing, scene.path_source) {
                notices.push(ProjectExportNotice {
                    subject: format!("{} ({})", scene.name, path_plan.relative_path),
                    reason: "a previous duplicate BuildSettings scene already wrote this path",
                });
                continue;
            }
            return Err(invalid_project(format!(
                "scene {:?} resolves to duplicate output path {:?} without a duplicate BuildSettings entry",
                scene.name, path_plan.relative_path
            )));
        }
        scene_paths_seen.insert(path_plan.relative_path.clone());

        let generated_assignment_count = scene
            .assignments
            .iter()
            .filter(|assignment| {
                matches!(
                    assignment.asset.collection,
                    ExportAssetCollectionIdentity::ProcessedCollection { .. }
                )
            })
            .count();
        let generated_transforms: Vec<_> = scene_processing
            .generated_transforms
            .iter()
            .filter(|transform| {
                scene.assignments.iter().any(|assignment| {
                    matches!(
                        assignment.asset.collection,
                        ExportAssetCollectionIdentity::ProcessedCollection {
                            collection_index
                        } if collection_index == transform.asset.collection_index
                    ) && assignment.asset.path_id == transform.asset.path_id
                })
            })
            .collect();
        if generated_assignment_count != generated_transforms.len() {
            return Err(invalid_project(format!(
                "scene {:?} has {generated_assignment_count} generated assignments but {} materialized generated Transforms",
                scene.name,
                generated_transforms.len(),
            )));
        }
        let generated_transform_context = (!generated_transforms.is_empty()).then(|| {
            WorkspaceGeneratedTransformYamlContext::with_materialized_scene_transforms(
                &scene_processing.processed_assets,
                scene.scene_index,
                &scene_processing.generated_transforms,
            )
        });
        let generated_lighting_data_pointer = lighting_data_assets
            .iter()
            .find(|asset| asset.scene_index == scene.scene_index)
            .map(|asset| WorkspaceGeneratedLightingDataYamlPointer {
                source: asset.source_owner,
                target: asset.identity,
            });
        let scene_yaml = workspace_scene_assignments_to_unity_yaml_with_project_generated_assets(
            workspace,
            objects,
            reference_graph,
            pointer_map,
            &scene.exportable_assignments,
            scene.collection_index,
            None,
            limits.yaml_collection,
            &streamed_resources,
            generated_transform_context.as_ref(),
            generated_lighting_data_pointer.as_ref(),
        )?;
        exported_object_count = exported_object_count
            .checked_add(count_unity_yaml_documents(&scene_yaml))
            .ok_or_else(|| invalid_project("exported object count overflows"))?;
        let bytes = scene_yaml.into_bytes();
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid_project("project output byte count overflows"))?;
        if total_bytes > limits.max_total_asset_payload_bytes {
            return Err(invalid_project(format!(
                "project output uses {total_bytes} bytes, exceeding the limit {}",
                limits.max_total_asset_payload_bytes
            )));
        }
        files.push(PlannedProjectFile {
            collection_name: "SceneExportCollection",
            kind: ExportCollectionKind::Scene,
            relative_path: path_plan.relative_path,
            guid: scene.guid,
            bytes,
            meta: Some(PlannedProjectMeta {
                importer_class: "DefaultImporter".to_owned(),
                importer: scene_default_importer(
                    project_version,
                    scene.asset_bundle_name.as_deref(),
                )?,
            }),
            label: scene.name.clone(),
        });
    }

    builder.total_bytes = total_bytes;
    builder.exported_object_count = exported_object_count;
    Ok(())
}
pub(super) fn plan_lighting_data_files(
    context: &LightingDataPlanningContext<'_, '_, '_>,
    builder: &mut ProjectExportPlanBuilder,
) -> Result<(), Box<dyn Error>> {
    let LightingDataPlanningContext {
        lighting_data_assets,
        reference_graph,
        pointer_map,
        routed,
        limits,
        project_version,
    } = *context;
    let mut total_bytes = builder.total_bytes;
    let mut exported_object_count = builder.exported_object_count;
    let planner = &mut builder.planner;
    let files = &mut builder.files;
    for lighting_data in lighting_data_assets {
        let bytes = generated_lighting_data_asset_to_yaml(
            lighting_data,
            reference_graph,
            pointer_map,
            routed,
            limits,
        )?;
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid_project("project output byte count overflows"))?;
        if total_bytes > limits.max_total_asset_payload_bytes {
            return Err(invalid_project(format!(
                "project output uses {total_bytes} bytes, exceeding the limit {}",
                limits.max_total_asset_payload_bytes
            )));
        }
        exported_object_count = exported_object_count
            .checked_add(count_unity_yaml_documents(
                std::str::from_utf8(&bytes).map_err(|error| {
                    invalid_project(format!(
                        "generated LightingDataAsset YAML is not UTF-8: {error}"
                    ))
                })?,
            ))
            .ok_or_else(|| invalid_project("exported object count overflows"))?;
        let relative_path = planner.plan_asset_path(
            &ExportAssetPathIdentity {
                class_name: "LightingDataAsset".to_owned(),
                named_name: Some("LightingData".to_owned()),
                original_directory: Some(lighting_data.original_directory.clone()),
                ..Default::default()
            },
            DEFAULT_EXPORT_EXTENSION,
        )?;
        files.push(PlannedProjectFile {
            collection_name: "LightingDataAssetExportCollection",
            kind: ExportCollectionKind::Asset,
            relative_path,
            guid: lighting_data.guid,
            bytes,
            meta: Some(PlannedProjectMeta {
                importer_class: "NativeFormatImporter".to_owned(),
                importer: lighting_data_native_format_importer_for_version(
                    112_000_000,
                    project_version,
                    None,
                )?,
            }),
            label: "LightingData".to_owned(),
        });
    }

    builder.total_bytes = total_bytes;
    builder.exported_object_count = exported_object_count;
    Ok(())
}
