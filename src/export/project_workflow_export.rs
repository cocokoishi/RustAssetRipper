/// Execute project export with the imported assembly catalog available to script exporters.
#[allow(clippy::too_many_arguments)]
pub fn export_workspace_yaml_project_with_assembly_catalog(
    output_root: &Path,
    delete_existing: bool,
    filesystem: &VirtualFileSystem,
    root_catalog: &RootResourceFileCatalog,
    resource_provider: &PlatformResourceProvider<'_>,
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    settings: &ExportSettingsFile,
    assembly_manager_is_set: bool,
    managed_assemblies: Option<&ManagedAssemblyCatalog>,
    limits: WorkspaceProjectExportLimits,
) -> Result<WorkspaceProjectExportReport, Box<dyn Error>> {
    let (editor_version_text, project_version) = workspace_project_version(workspace)?;
    let has_mono_scripts = objects
        .objects()
        .iter()
        .any(|object| object.factory.class_id == 115);
    let reference_assembly_maps =
        source_reference_assembly_maps(has_mono_scripts, managed_assemblies)?;
    let settings_facts = ProjectExportSettings::from_export_settings(settings, project_version);
    if objects.report().object_count > limits.max_objects {
        return Err(invalid_project(format!(
            "project export has {} objects, exceeding the limit {}",
            objects.report().object_count,
            limits.max_objects
        )));
    }
    let opaque_objects = opaque_factory_object_keys(objects)?;
    let incomplete_objects = objects
        .objects()
        .iter()
        .filter(|object| {
            !object.is_fully_materialized()
                && !opaque_objects.contains(&(object.file_index, object.object_index))
        })
        .take(4)
        .map(|object| {
            let detail = object
                .payload
                .as_ref()
                .err()
                .map(ToString::to_string)
                .or_else(|| object.factory.log_message.clone())
                .unwrap_or_else(|| "the source factory retained unread structure data".into());
            format!(
                "file {} object {} PathID {} ({:?}): {detail}",
                object.file_index, object.object_index, object.path_id, object.factory.kind
            )
        })
        .collect::<Vec<_>>();
    if !incomplete_objects.is_empty() {
        return Err(invalid_project(format!(
            "project export requires every decoded object to be fully materialized; other than source-supported unknown/unreadable raw fallbacks, {} object(s) are incomplete: [{}]",
            objects.report().undecoded_objects.saturating_sub(opaque_objects.len()),
            incomplete_objects.join("; ")
        )));
    }
    if !reference_graph.is_complete_except_opaque_object_payloads(|file_index, object_index| {
        opaque_objects.contains(&(file_index, object_index))
    }) {
        let incomplete_scans = (0..reference_graph.file_count())
            .filter_map(|file_index| {
                let graph = reference_graph.managed_graph_for_file(file_index)?;
                (!graph.is_complete()).then(|| {
                    let issues = graph.issues();
                    format!(
                        "{}: {} scan issue(s), first issue {:?}",
                        reference_graph.normalized_file_names()[file_index],
                        issues.len(),
                        issues.first()
                    )
                })
            })
            .take(4)
            .collect::<Vec<_>>();
        let unresolved_references = reference_graph
            .references()
            .iter()
            .filter(|reference| {
                !matches!(
                    reference.resolution,
                    MultiFilePPtrResolution::Null | MultiFilePPtrResolution::Object { .. }
                )
            })
            .take(4)
            .map(|reference| format!("{reference:?}"))
            .collect::<Vec<_>>();
        return Err(invalid_project(format!(
            "project export requires a complete workspace PPtr scan; incomplete scans: [{}]; unresolved references: [{}]",
            incomplete_scans.join("; "),
            unresolved_references.join("; ")
        )));
    }
    let PreparedSceneCollections {
        processing: scene_processing,
        routed: mut scene_collections,
        member_keys: scene_member_keys,
        prefabs: mut prefab_collections,
        prefab_member_keys,
        processing_warnings,
        mut used_guids,
    } = prepare_scene_collections(
        filesystem,
        workspace,
        objects,
        reference_graph,
        project_version,
    )?;
    let lightmap_texture_directories =
        lightmap_texture_scene_directories(&scene_processing, objects, reference_graph)?;
    let mut next_guid_sequence = 0u64;

    // Pass 1: route every object through the registration stack. No file is planned yet, so a
    // mis-routed or unhandled object aborts with nothing written.
    let mut routed = Vec::new();
    routed
        .try_reserve_exact(objects.report().object_count)
        .map_err(|_| invalid_project("could not allocate routed project objects"))?;
    let mut notices: Vec<ProjectExportNotice> = Vec::new();
    let mut unresolved_script_content: Vec<ProjectExportNotice> = Vec::new();
    let mut fallback_notices: Vec<ProjectExportNotice> = Vec::new();
    let mut opaque_fallback_notices: Vec<ProjectExportNotice> = Vec::new();
    let mut next_collection_index = 0usize;
    for (file_index, file) in workspace.files().iter().enumerate() {
        for (object_index, object_info) in file.metadata.objects.iter().enumerate() {
            let source_key = source_object_key(file_index, object_index);
            if scene_member_keys.contains(&source_key) || prefab_member_keys.contains(&source_key) {
                continue;
            }
            let object = objects.get(file_index, object_index).ok_or_else(|| {
                invalid_project(format!(
                    "workspace object is missing at file {file_index}, object {object_index}"
                ))
            })?;
            if object_info.path_id == 0 {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} has PathID 0, which cannot identify a Unity YAML document"
                )));
            }
            // Newer SerializedFile generations omit the legacy object-table class ID;
            // the source factory resolves it from the serialized type table.
            let class_id = object.factory.class_id;
            let engine_guid = engine_resource_collection_guid(&file.file_name);
            let payload = object.payload.as_ref().ok();
            if payload.is_some() && !opaque_objects.contains(&(file_index, object_index)) {
                validate_decoded_object(object, object_info.path_id, reference_graph)?;
            }
            let facts = match &object.factory.kind {
                FactoryAssetKind::TypeTreeObject => ObjectClassFacts::type_tree_object(class_id),
                FactoryAssetKind::Unknown => ObjectClassFacts::with_known(class_id, false, false),
                FactoryAssetKind::Unreadable => ObjectClassFacts::new(class_id, true),
                FactoryAssetKind::Generated(_) => {
                    ObjectClassFacts::new(class_id, payload.is_none())
                }
            };
            let shader_view = if engine_guid.is_none() && class_id == SHADER_CLASS_ID {
                payload
                    .map(|payload| ShaderView::from_value(&payload.root))
                    .transpose()?
            } else {
                None
            };
            let audio_view = if engine_guid.is_none()
                && class_id == crate::resources::audio::AUDIO_CLIP_CLASS_ID
            {
                payload
                    .map(|payload| {
                        crate::resources::audio::AudioClipView::from_value(&payload.root)
                    })
                    .transpose()?
            } else {
                None
            };
            let shader_text = match shader_view.as_ref().and_then(|view| view.script.as_deref()) {
                None => crate::export::collection_claims::ShaderTextState::NoScript,
                Some(script) if script_has_decompiled_text(script) => {
                    crate::export::collection_claims::ShaderTextState::DecompiledSource
                }
                Some(_) => crate::export::collection_claims::ShaderTextState::CompiledOrEmpty,
            };
            let text_asset_script = if engine_guid.is_none() && class_id == 49 {
                payload.map(text_asset_script_bytes).transpose()?.flatten()
            } else {
                None
            };
            let standalone_texture = engine_guid.is_none()
                && class_id == 28
                && matches!(&object.factory.kind, FactoryAssetKind::Generated(_))
                && object.recovered_type_tree.is_some()
                && !has_sprite_reference_to_object(reference_graph, file_index, object_index);
            let lightmap_scene_directory = lightmap_texture_directories
                .get(&(file_index, object_index))
                .cloned();
            let lightmap_image_route = lightmap_scene_directory.is_some()
                && !matches!(
                    settings.lightmap_texture_export_format,
                    crate::export::settings::LightmapTextureExportFormat::Yaml
                );
            let texture_importer_supported =
                TextureImporterLayout::for_project_version(project_version).is_some();
            let texture_route_supported = texture_importer_supported
                && ((standalone_texture && lightmap_scene_directory.is_none())
                    || lightmap_image_route);
            let mut claim_facts = claim_facts_for_shader_text(facts, settings_facts, shader_text);
            claim_facts.engine_file = engine_guid.is_some();
            claim_facts.data_valid = text_asset_script
                .as_ref()
                .is_some_and(|script| !script.is_empty());
            if lightmap_scene_directory.is_some() {
                // LightingDataProcessor assigns the generated LightingDataAsset as the
                // main asset for every texture referenced by LightmapSettings.Lightmaps.
                claim_facts.main = ClaimMainKind::LightingData;
            } else if texture_route_supported {
                // SpriteProcessor creates this generated main asset for a standalone ITexture2D.
                // Keep selection in the ordinary registration walk with the processed fact.
                claim_facts.main = ClaimMainKind::SpriteInformation;
            }
            let selection = select_collection_with_claim_facts(facts, settings_facts, claim_facts)?;
            if selection.exporter == "LightmapTextureAssetExporter" && !texture_importer_supported {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} PathID {} selected LightmapTextureAssetExporter, but Unity {}.{} has no source-verified TextureImporter layout",
                    object.path_id, project_version.major, project_version.minor
                )));
            }
            let audio_yaml_route = if selection.exporter == "YamlAudioExporter" {
                let view = audio_view.as_ref().ok_or_else(|| {
                    invalid_project(
                        "YamlAudioExporter was selected without decoded AudioClip fields",
                    )
                })?;
                let has_inline_data = view
                    .audio_data
                    .as_ref()
                    .is_some_and(|bytes| !bytes.is_empty());
                let has_stream_data = view
                    .resource
                    .as_ref()
                    .is_some_and(|resource| resource.is_set());
                let can_preserve_audio = match settings.audio_export_format {
                    crate::export::settings::AudioExportFormat::Yaml => true,
                    crate::export::settings::AudioExportFormat::Default
                    | crate::export::settings::AudioExportFormat::PreferWav => {
                        !has_inline_data && !has_stream_data
                    }
                    crate::export::settings::AudioExportFormat::Native => false,
                };
                if !can_preserve_audio {
                    return Err(invalid_project(format!(
                        "file {file_index} object {object_index} PathID {} selected YamlAudioExporter, but this project route cannot preserve its inline or streamed AudioClip data under {:?}",
                        object.path_id, settings.audio_export_format
                    )));
                }
                true
            } else {
                false
            };
            if class_id == crate::resources::audio::AUDIO_CLIP_CLASS_ID
                && matches!(
                    selection.exporter,
                    "NativeAudioExporter" | "AudioClipExporter"
                )
            {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} PathID {} selected {}, whose binary AudioClip project route is not integrated",
                    object.path_id, selection.exporter
                )));
            }
            let audio_yaml_resource = if audio_yaml_route {
                let view = audio_view.as_ref().ok_or_else(|| {
                    invalid_project(
                        "YamlAudioExporter was selected without decoded AudioClip fields",
                    )
                })?;
                match view.resource.as_ref() {
                    None => None,
                    Some(resource) if resource.size > 0 && resource.path.is_empty() => {
                        return Err(invalid_project(format!(
                            "file {file_index} object {object_index} PathID {} AudioClip declares {} streamed bytes with an empty resource path",
                            object.path_id, resource.size
                        )));
                    }
                    Some(resource) => Some(resource.clone()),
                }
            } else {
                None
            };
            let text_asset_export = if selection.exporter == "TextAssetExporter" {
                let script = text_asset_script.ok_or_else(|| {
                    invalid_project("TextAssetExporter was selected without decoded m_Script bytes")
                })?;
                let text = String::from_utf8_lossy(&script);
                let extension = text_export_extension(None, settings.text_export_mode, &text);
                Some(TextAssetExportDetails { script, extension })
            } else {
                None
            };
            let texture_export = if texture_route_supported {
                let expected_exporter = if lightmap_image_route {
                    "LightmapTextureAssetExporter"
                } else {
                    "TextureAssetExporter"
                };
                if selection.exporter != expected_exporter {
                    return Err(invalid_project(format!(
                        "generated Texture2D selected {}, expected the registered {expected_exporter}",
                        selection.exporter,
                    )));
                }
                let payload = payload.ok_or_else(|| {
                    invalid_project(
                        "texture image exporter was selected without decoded Texture2D fields",
                    )
                })?;
                let asset_bundle_name = object_asset_bundle_name(payload);
                let plan = if lightmap_image_route {
                    plan_lightmap_texture_project_export(
                        &payload.root,
                        project_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )
                } else {
                    plan_texture_project_export(
                        &payload.root,
                        project_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )
                };
                Some(plan.map_err(|error| {
                    invalid_project(format!(
                        "file {file_index} object {object_index} PathID {} Texture2D project planning failed: {error}",
                        object.path_id
                    ))
                })?)
            } else {
                None
            };
            let kind = selection.kind();
            let script_export = if class_id == 115 && engine_guid.is_none() {
                plan_script_export_details(
                    object,
                    settings,
                    assembly_manager_is_set,
                    managed_assemblies,
                    editor_version_text,
                    reference_assembly_maps.as_ref(),
                )?
            } else {
                None
            };
            if let Some(script) = script_export.as_ref().filter(|script| {
                script.missing_assembly || script.source_script_present == Some(false)
            }) {
                let reason = match (script.missing_assembly, script.source_script_present) {
                    (true, Some(false)) => {
                        "the assembly and script type are absent; its source decompile pointer is retained and no .cs file is emitted"
                    }
                    (false, Some(false)) => {
                        "the imported manager has no matching script type; its source pointer is retained and no .cs file is emitted"
                    }
                    (true, _) => {
                            "the assembly is absent from the imported catalog; its source GUID/fileID pointer is retained and the missing script DLL is not fabricated"
                    }
                    (false, _) => {
                        "the source script content is unavailable; its pointer is retained and no .cs file is emitted"
                    }
                };
                unresolved_script_content.push(ProjectExportNotice {
                    subject: format!(
                        "MonoScript assembly {:?} ({}.{}, ScriptExportMode::{:?})",
                        script.identity.assembly,
                        script.identity.namespace,
                        script.identity.class,
                        settings.script_export_mode
                    ),
                    reason,
                });
            }
            let is_opaque_fallback = opaque_objects.contains(&(file_index, object_index));
            let raw_fallback_kind_matches = match kind {
                ExportCollectionKind::Unknown => {
                    matches!(&object.factory.kind, FactoryAssetKind::Unknown)
                }
                ExportCollectionKind::Unreadable => {
                    matches!(&object.factory.kind, FactoryAssetKind::Unreadable)
                }
                _ => true,
            };
            if matches!(
                kind,
                ExportCollectionKind::Unknown | ExportCollectionKind::Unreadable
            ) && (!is_opaque_fallback || !raw_fallback_kind_matches)
            {
                return Err(invalid_project(format!(
                    "{} selected a raw fallback exporter, but the source factory did not retain an opaque UnknownObject or UnreadableObject payload",
                    selection.collection_name
                )));
            }
            let (route, skip_reason, fallback_reason) = if engine_guid.is_some() {
                if selection.exporter != "EngineAssetsExporter"
                    || kind != ExportCollectionKind::Redirect
                {
                    return Err(invalid_project(format!(
                        "engine resource collection {} selected {}, expected EngineAssetsExporter redirect",
                        file.file_name, selection.exporter
                    )));
                }
                (ObjectRoute::EngineRedirect, "", None)
            } else {
                match kind {
                ExportCollectionKind::Manager => (ObjectRoute::Manager, "", None),
                ExportCollectionKind::Unknown => (ObjectRoute::Unknown, "", None),
                ExportCollectionKind::Unreadable => (ObjectRoute::Unreadable, "", None),
                ExportCollectionKind::Asset if selection.exporter == "TextAssetExporter" => {
                    (ObjectRoute::TextAsset, "", None)
                }
                ExportCollectionKind::Asset if audio_yaml_route => {
                    (ObjectRoute::AudioYaml, "", None)
                }
                ExportCollectionKind::Asset if selection.exporter == "YamlShaderExporter" => {
                    (ObjectRoute::ShaderYaml, "", None)
                }
                ExportCollectionKind::Asset
                    if selection.exporter == "DummyShaderTextExporter"
                        || selection.exporter == "SimpleShaderExporter" =>
                {
                    (ObjectRoute::ShaderText, "", None)
                }
                ExportCollectionKind::Asset if texture_export.is_some() => {
                    (ObjectRoute::TextureImage, "", None)
                }
                ExportCollectionKind::Asset => {
                    (ObjectRoute::Asset, "", asset_family_fallback(selection.collection_name))
                }
                ExportCollectionKind::Script | ExportCollectionKind::EmptyScript
                    if let Some(script) = script_export.as_ref() => (
                    if script.export_type == AssemblyExportType::Save
                        || (script.export_type == AssemblyExportType::Skip
                            && assembly_manager_is_set)
                        || (script.export_type == AssemblyExportType::Decompile
                            && assembly_manager_is_set
                            && script.source_script_present == Some(false))
                    {
                        ObjectRoute::ScriptPointerOnly
                    } else {
                        ObjectRoute::Script
                    },
                    "",
                    None,
                ),
                ExportCollectionKind::Script | ExportCollectionKind::EmptyScript
                    if class_id == 115 => (
                    ObjectRoute::Skipped,
                    "injected MonoScripts have no standalone script file in the source exporter",
                    None,
                ),
                ExportCollectionKind::Scene | ExportCollectionKind::Prefab => (
                    ObjectRoute::Asset,
                    "",
                    Some(
                        "the specialized collection renderer is not ported; the object was written \
                         through the generic asset family",
                    ),
                ),
                ExportCollectionKind::Script | ExportCollectionKind::EmptyScript => (
                    ObjectRoute::Skipped,
                    "a script export collection was selected without a MonoScript object",
                    None,
                ),
                ExportCollectionKind::Fail => (
                    ObjectRoute::Skipped,
                    "FailExportCollection writes nothing and reports a failed export",
                    None,
                ),
                ExportCollectionKind::Empty => (
                    ObjectRoute::Skipped,
                    "EmptyExportCollection is not exportable; the baseline writes nothing",
                    None,
                ),
                ExportCollectionKind::Skip => (
                    ObjectRoute::Skipped,
                    "SkipExportCollection is not exportable; references become missing references",
                    None,
                ),
                ExportCollectionKind::Deleted => (
                    ObjectRoute::Skipped,
                    "DeletedAssetsExportCollection is not exportable",
                    None,
                ),
                ExportCollectionKind::Redirect => (
                    ObjectRoute::Skipped,
                    "RedirectExportCollection is not exportable",
                    None,
                ),
                }
            };
            let fallback_reason = if standalone_texture && !texture_route_supported {
                Some("the source TextureExportCollection image/importer route is not yet ported for this Unity importer version or Sprite group")
            } else {
                fallback_reason
            };
            let label = if route == ObjectRoute::Skipped {
                format!("{} (class {class_id})", selection.collection_name)
            } else {
                selection.collection_name.to_owned()
            };
            if route == ObjectRoute::Skipped {
                notices.push(ProjectExportNotice {
                    subject: label.clone(),
                    reason: skip_reason,
                });
            }
            if let Some(reason) = fallback_reason {
                fallback_notices.push(ProjectExportNotice {
                    subject: label.clone(),
                    reason,
                });
            }
            if matches!(route, ObjectRoute::Unknown | ObjectRoute::Unreadable) {
                opaque_fallback_notices.push(ProjectExportNotice {
                    subject: label,
                    reason: "raw bytes are preserved, but this object has no decoded fields or reference scan",
                });
            }
            let collection_index = if route == ObjectRoute::Skipped {
                usize::MAX
            } else {
                let index = next_collection_index;
                next_collection_index = next_collection_index
                    .checked_add(1)
                    .ok_or_else(|| invalid_project("project collection count overflows"))?;
                index
            };
            let (class_name, named_name) = if opaque_objects.contains(&(file_index, object_index)) {
                (
                    object.factory.class_name.to_string(),
                    Some(object.factory.name.clone()),
                )
            } else {
                (
                    object_class_name(class_id, payload.map(|payload| payload.type_name.as_str())),
                    payload.and_then(object_asset_name),
                )
            };
            let file_id = match route {
                ObjectRoute::Asset
                | ObjectRoute::AudioYaml
                | ObjectRoute::TextureImage
                | ObjectRoute::TextAsset
                | ObjectRoute::ShaderText
                | ObjectRoute::ShaderYaml => crate::export::ids::get_main_export_id(class_id, 0)
                    .map_err(|error| {
                        invalid_project(format!(
                            "class {class_id} export file ID could not be planned: {error}"
                        ))
                    })?,
                ObjectRoute::Script | ObjectRoute::ScriptPointerOnly => {
                    script_export
                        .as_ref()
                        .ok_or_else(|| invalid_project("MonoScript route has no export details"))?
                        .pointer_file_id
                }
                _ => object_info.path_id,
            };
            let guid = if let Some(engine_guid) = engine_guid {
                used_guids.insert(engine_guid);
                engine_guid
            } else if let Some(script) = script_export.as_ref() {
                script.guid
            } else {
                mint_unique_collection_guid(&mut next_guid_sequence, &mut used_guids)?
            };
            let pointer_guid = script_export
                .as_ref()
                .map_or(guid, |script| script.pointer_guid);
            routed.push(RoutedObject {
                object,
                class_id,
                class_name: class_name.clone(),
                named_name,
                original_directory: lightmap_scene_directory,
                facts,
                shader_view,
                audio_yaml_resource,
                text_asset_export,
                texture_export,
                script_export,
                selection,
                route,
                collection_index,
                guid,
                pointer_guid,
                file_id,
            });
        }
    }

    let saved_assemblies = if assembly_manager_is_set && has_mono_scripts {
        let catalog = managed_assemblies.ok_or_else(|| {
            invalid_project(
                "manager-backed MonoScript project export requires the imported assembly catalog",
            )
        })?;
        plan_saved_assembly_exports(
            catalog,
            reference_assembly_maps.as_ref(),
            editor_version_text,
            settings,
        )?
    } else {
        Vec::new()
    };

    for prefab in &mut prefab_collections {
        prefab.collection_index = next_collection_index;
        next_collection_index = next_collection_index
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        prefab.guid = mint_unique_collection_guid(&mut next_guid_sequence, &mut used_guids)?;
    }

    for scene in &mut scene_collections {
        scene.collection_index = next_collection_index;
        next_collection_index = next_collection_index
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        scene.guid = match scene.guid_plan {
            SceneDefinitionGuidPlan::Preserve(words) if !words.is_zero() => {
                scene_guid_words_to_bytes(words)
            }
            SceneDefinitionGuidPlan::Preserve(_) | SceneDefinitionGuidPlan::GenerateAtExport(_) => {
                mint_unique_collection_guid(&mut next_guid_sequence, &mut used_guids)?
            }
        };
        used_guids.insert(scene.guid);
    }

    let mut lighting_data_assets = Vec::new();
    lighting_data_assets
        .try_reserve_exact(scene_collections.len())
        .map_err(|_| invalid_project("could not allocate generated LightingDataAsset routes"))?;
    for scene in &scene_collections {
        let hierarchy = scene_processing
            .hierarchies
            .scenes()
            .get(scene.scene_index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "SceneDefinition {} has no hierarchy while planning LightingDataAsset",
                    scene.scene_index
                ))
            })?;
        let Some(owner_identity) = hierarchy.lighting_data_dependency_owner() else {
            continue;
        };
        validate_bounded_lighting_data_scene(hierarchy, owner_identity)?;
        let render_settings_identity =
            scene_manager_identity(hierarchy, SceneManagerKind::RenderSettings).ok_or_else(
                || {
                    invalid_project(format!(
                        "LightmapSettings {}:{} has no RenderSettings owner",
                        owner_identity.file_index, owner_identity.path_id
                    ))
                },
            )?;
        let source_owner = export_identity_for_scene_object(owner_identity)?;
        let owner_file = workspace
            .files()
            .get(owner_identity.file_index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "LightmapSettings file index {} is outside the workspace",
                    owner_identity.file_index
                ))
            })?;
        let version_text = owner_file
            .metadata
            .prefix
            .unity_version_text
            .as_deref()
            .ok_or_else(|| {
                invalid_project(format!(
                    "LightmapSettings file {} has no Unity version for LightingDataAsset generation",
                    owner_file.file_name
                ))
            })?;
        let source_version = FactoryUnityVersion::parse(version_text).ok_or_else(|| {
            invalid_project(format!(
                "LightmapSettings file {} has unparseable Unity version {version_text:?}",
                owner_file.file_name
            ))
        })?;
        let scene_version = SceneUnityVersion::parse(version_text).ok_or_else(|| {
            invalid_project(format!(
                "LightmapSettings file {} has unsupported Unity version {version_text:?}",
                owner_file.file_name
            ))
        })?;
        let (type_tree, _) = reference_class_layouts()
            .tpk_trees(1120, &source_version)
            .map_err(|error| {
                invalid_project(format!(
                    "LightingDataAsset TypeTree lookup failed for Unity {version_text}: {error}"
                ))
            })?
            .ok_or_else(|| {
                invalid_project(format!(
                    "LightingDataAsset has no reference TypeTree for Unity {version_text}"
                ))
            })?;
        let source_owner_payload = objects
            .get(owner_identity.file_index, owner_identity.object_index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "LightmapSettings {}:{} has no materialized workspace object",
                    owner_identity.file_index, owner_identity.path_id
                ))
            })?
            .payload
            .as_ref()
            .map_err(|error| {
                invalid_project(format!(
                    "LightmapSettings {}:{} could not be decoded for LightingDataAsset generation: {error}",
                    owner_identity.file_index, owner_identity.path_id
                ))
            })?;
        let source_render_settings_payload = objects
            .get(
                render_settings_identity.file_index,
                render_settings_identity.object_index,
            )
            .ok_or_else(|| {
                invalid_project(format!(
                    "RenderSettings {}:{} has no materialized workspace object",
                    render_settings_identity.file_index, render_settings_identity.path_id
                ))
            })?
            .payload
            .as_ref()
            .map_err(|error| {
                invalid_project(format!(
                    "RenderSettings {}:{} could not be decoded for LightingDataAsset generation: {error}",
                    render_settings_identity.file_index, render_settings_identity.path_id
                ))
            })?;
        let root = build_lighting_data_asset_root(
            &type_tree,
            owner_identity.object_index,
            &source_owner_payload.root,
            &source_render_settings_payload.root,
            &source_version,
            scene.guid,
        )?;
        let collection_index = next_collection_index;
        next_collection_index = next_collection_index
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        let guid = mint_unique_collection_guid(&mut next_guid_sequence, &mut used_guids)?;
        lighting_data_assets.push(RoutedLightingDataAsset {
            scene_index: scene.scene_index,
            source_owner,
            scene_guid: scene.guid,
            collection_index,
            identity: ExportAssetIdentity {
                collection: ExportAssetCollectionIdentity::ProcessedCollection { collection_index },
                object_index: 0,
                path_id: 1,
                class_id: 1120,
            },
            guid,
            original_directory: scene_processing.hierarchies.scenes()[scene.scene_index]
                .plan
                .path
                .clone(),
            source_version: scene_version,
            type_tree: (*type_tree).clone(),
            root,
        });
    }

    // Pass 2: build the project pointer map from exactly the objects that will be written, so a
    // reference to a skipped asset resolves to the baseline missing-reference pointer.
    let scene_assignment_count = scene_collections
        .iter()
        .try_fold(0usize, |count, scene| {
            count.checked_add(scene.assignments.len())
        })
        .ok_or_else(|| invalid_project("project pointer count overflows"))?;
    let pointer_capacity = routed
        .len()
        .checked_add(scene_assignment_count)
        .and_then(|count| count.checked_add(lighting_data_assets.len()))
        .and_then(|count| {
            prefab_collections.iter().try_fold(count, |total, prefab| {
                total.checked_add(prefab.export_ids.registered_assets.len())
            })
        })
        .ok_or_else(|| invalid_project("project pointer count overflows"))?;
    let mut pointer_entries = Vec::new();
    pointer_entries
        .try_reserve_exact(pointer_capacity)
        .map_err(|_| invalid_project("could not allocate project export pointer entries"))?;
    let mut collection_metadata = Vec::new();
    collection_metadata
        .try_reserve_exact(next_collection_index)
        .map_err(|_| invalid_project("could not allocate project export collection metadata"))?;
    let mut collection_count = 0usize;
    for entry in &routed {
        if entry.route == ObjectRoute::Skipped {
            continue;
        }
        if entry.route != ObjectRoute::EngineRedirect {
            collection_count += 1;
        }
        // UnknownExportCollection and UnreadableExportCollection write raw files without
        // `.meta` files or export GUIDs. Do not register their synthetic in-memory GUIDs as
        // Unity project pointers; references to them resolve through the explicit missing-map
        // path instead of targeting a GUID that no output metadata defines.
        if matches!(entry.route, ObjectRoute::Unknown | ObjectRoute::Unreadable) {
            continue;
        }
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: entry.collection_index,
            guid: entry.pointer_guid,
            asset_type: if matches!(
                entry.route,
                ObjectRoute::Script | ObjectRoute::ScriptPointerOnly | ObjectRoute::TextAsset
            ) {
                UnityProjectAssetType::Meta
            } else if entry.route == ObjectRoute::EngineRedirect {
                UnityProjectAssetType::Internal
            } else {
                UnityProjectAssetType::Serialized
            },
        });
        pointer_entries.push(ExportedAssetPointerEntry {
            asset: identity_of(entry),
            file_id: entry.file_id,
            collection_index: entry.collection_index,
        });
    }
    for scene in &scene_collections {
        collection_count = collection_count
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: scene.collection_index,
            guid: scene.guid,
            asset_type: UnityProjectAssetType::Serialized,
        });
        pointer_entries.extend(scene.assignments.iter().map(|assignment| {
            ExportedAssetPointerEntry {
                asset: assignment.asset,
                file_id: assignment.file_id,
                collection_index: scene.collection_index,
            }
        }));
    }
    for prefab in &prefab_collections {
        let source_pointer = generate_prefab_source_pointer(prefab.guid, prefab.representation)
            .map_err(|error| invalid_project(format!("Prefab pointer planning failed: {error}")))?;
        collection_count = collection_count
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: prefab.collection_index,
            guid: prefab.guid,
            asset_type: source_pointer.asset_type,
        });
        pointer_entries.extend(
            prefab
                .export_ids
                .registered_assets
                .iter()
                .map(|assignment| ExportedAssetPointerEntry {
                    asset: assignment.asset,
                    file_id: assignment.file_id,
                    collection_index: prefab.collection_index,
                }),
        );
    }
    for lighting_data in &lighting_data_assets {
        collection_count = collection_count
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: lighting_data.collection_index,
            guid: lighting_data.guid,
            asset_type: UnityProjectAssetType::Serialized,
        });
        pointer_entries.push(ExportedAssetPointerEntry {
            asset: lighting_data.identity,
            file_id: 112_000_000,
            collection_index: lighting_data.collection_index,
        });
    }
    let pointer_map =
        ProjectExportPointerMap::new(&pointer_entries, &collection_metadata, limits.pointer_map)?;
    let streamed_resources = WorkspaceStreamedYamlResourceContext {
        filesystem,
        root_catalog,
        provider: resource_provider,
        content_limits: limits.streamed_content,
        staging_limits: StreamedYamlAssetLimits::default(),
    };

    // Pass 3: materialize every routed object into a staged file.
    let mut planner = ProjectPathPlanner::new(ProjectPathPlanner::host_platform());
    let mut files = Vec::new();
    let staged_file_capacity = collection_count
        .checked_add(saved_assemblies.len())
        .ok_or_else(|| invalid_project("staged project file count overflows"))?;
    files
        .try_reserve_exact(staged_file_capacity)
        .map_err(|_| invalid_project("could not allocate staged project asset files"))?;
    let mut support_files = Vec::new();
    let mut support_file_indexes = HashMap::new();
    let mut total_bytes = 0usize;
    let mut exported_object_count = 0usize;
    let mut raw_file_cache: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut saved_assemblies_emitted = false;
    for entry in &routed {
        if entry.class_id == 115 && !saved_assemblies_emitted {
            for assembly in &saved_assemblies {
                let input_bytes = filesystem
                    .read_all_bytes(
                        &assembly.source_virtual_path,
                        limits.max_total_asset_payload_bytes,
                    )
                    .map_err(|error| {
                        invalid_project(format!(
                            "managed assembly {:?} could not be read from {}: {error}",
                            assembly.assembly_name, assembly.source_virtual_path
                        ))
                    })?;
                let file_name = assembly
                    .relative_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&assembly.relative_path);
                let serialized = match assembly.generated_forwarder.as_deref() {
                    Some("System.Runtime.CompilerServices.Unsafe") => {
                        ManagedAssembly::create_unsafe_forwarder(
                            &assembly.source_virtual_path,
                            &input_bytes,
                            ManagedAssemblyLimits::default(),
                        )
                    }
                    Some("System.Runtime.InteropServices") => {
                        ManagedAssembly::create_interop_services_forwarder(
                            &assembly.source_virtual_path,
                            &input_bytes,
                            ManagedAssemblyLimits::default(),
                        )
                    }
                    Some(helper_name) => {
                        return Err(invalid_project(format!(
                            "unknown synthesized forwarding assembly {helper_name:?}"
                        )))
                    }
                    None => ManagedAssembly::serialize_for_export(
                        file_name,
                        &input_bytes,
                        ManagedAssemblyLimits::default(),
                    ),
                };
                let bytes = serialized.map_err(|error| {
                    invalid_project(format!(
                        "managed assembly {:?} could not be exported from {}: {error}",
                        assembly.assembly_name, assembly.source_virtual_path
                    ))
                })?;
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
                    &mut support_files,
                    &mut support_file_indexes,
                    relative_path,
                    text.as_bytes(),
                    files.len(),
                )?;
            }
        }
        let mut identity = super::asset_path::ExportAssetPathIdentity {
            class_name: entry.class_name.clone(),
            named_name: entry.named_name.clone(),
            original_directory: entry.original_directory.clone(),
            ..Default::default()
        };
        if identity.class_name.is_empty() {
            identity.class_name = entry.class_name.clone();
        }
        let planned_audio_asset_path = if entry.route == ObjectRoute::AudioYaml {
            Some(planner.plan_asset_path(&identity, "audioclip")?)
        } else {
            None
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
        let bytes = match entry.route {
            ObjectRoute::Unknown | ObjectRoute::Unreadable => raw_payload_bytes(
                filesystem,
                workspace,
                entry.object,
                &mut raw_file_cache,
                limits,
            )?,
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
                                "Texture2D references absent SerializedFile {}",
                                entry.object.file_index
                            ))
                        })?;
                let stream_content = if texture.view.image_data.is_empty() {
                    if let Some(stream_info) = texture
                        .view
                        .stream_data
                        .as_ref()
                        .filter(|info| info.is_set())
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
                                "file {} object {} PathID {} Texture2D stream {:?} could not be read: {error}",
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
                                    "file {} object {} PathID {} Texture2D stream {:?} is missing; unavailable data paths: [{}]",
                                    entry.object.file_index,
                                    entry.object.object_index,
                                    entry.object.path_id,
                                    stream_info.path,
                                    unavailable_data_paths.join(", ")
                                )))
                            }
                            StreamedResourceContentResolution::Unset => {
                                return Err(invalid_project(format!(
                                    "file {} object {} PathID {} Texture2D stream {:?} became unset during resolution",
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
                            "file {} object {} PathID {} Texture2D image conversion failed: {error}",
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
                let document = workspace_object_to_unity_yaml_document_with_streamed_resources(
                    workspace,
                    reference_graph,
                    &pointer_map,
                    entry.object,
                    source,
                    entry.file_id,
                    entry.collection_index,
                    None,
                    limits.yaml_collection.object_conversion,
                    &streamed_resources,
                )?;
                let Some(mut document) = document else {
                    notices.push(ProjectExportNotice {
                        subject: entry.selection.collection_name.to_owned(),
                        reason:
                            "the baseline exporter skipped an image texture with no usable data",
                    });
                    continue;
                };
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
                write_unity_yaml_documents(&[document], limits.yaml_write)?.into_bytes()
            }
            ObjectRoute::EngineRedirect | ObjectRoute::Skipped => {
                unreachable!("non-exportable routes are filtered above")
            }
        };
        exported_object_count = exported_object_count
            .checked_add(1)
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
                &mut support_files,
                &mut support_file_indexes,
                relative_path.clone(),
                sidecar_bytes,
                files.len(),
            )?;
        }
        let relative_path = match entry.route {
            ObjectRoute::Asset => {
                let extension = export_extension_for(entry.facts);
                let extension = if extension.is_empty() {
                    DEFAULT_EXPORT_EXTENSION
                } else {
                    extension
                };
                planner.plan_asset_path(&identity, extension)?
            }
            ObjectRoute::AudioYaml => planned_audio_asset_path
                .ok_or_else(|| invalid_project("AudioYaml route has no planned asset path"))?,
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
                Some(PlannedProjectMeta {
                    importer_class: "NativeFormatImporter".to_owned(),
                    importer: native_format_importer_for_version(
                        entry.file_id,
                        project_version,
                        entry
                            .object
                            .payload
                            .as_ref()
                            .ok()
                            .and_then(object_asset_bundle_name)
                            .as_deref(),
                    )?,
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
                    &pointer_map,
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
        for entry in &routed {
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
                &mut support_files,
                &mut support_file_indexes,
                relative_path,
                &bytes,
                files.len(),
            )?;
        }
    }

    for prefab in &prefab_collections {
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
            &pointer_map,
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
            &super::asset_path::ExportAssetPathIdentity {
                class_name: prefab.class_name.clone(),
                named_name: prefab.named_name.clone(),
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
                importer: prefab_importer(prefab.asset_bundle_name.as_deref())?,
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

    let mut scene_paths_seen: HashSet<String> = HashSet::new();
    for scene in &mut scene_collections {
        let path_plan =
            plan_scene_export_paths(&scene.path, ProjectPathPlanner::host_platform(), "")?;
        if scene_paths_seen.contains(&path_plan.relative_path) {
            if scene_build_settings_entry_is_duplicate(&scene_processing, scene.path_source) {
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
            &pointer_map,
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

    for lighting_data in &lighting_data_assets {
        let bytes = generated_lighting_data_asset_to_yaml(
            lighting_data,
            reference_graph,
            &pointer_map,
            &routed,
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
                importer: native_format_importer_for_version(112_000_000, project_version, None)?,
            }),
            label: "LightingData".to_owned(),
        });
    }

    let manifest = package_manifest_json(project_version);
    let path_id_map = build_path_id_map(workspace, objects)?;
    let path_id_map_bytes = serialize_path_id_map(&path_id_map).into_bytes();
    let plan = ProjectExportPlan {
        project_version_text: editor_version_text.to_owned(),
        project_version_file: project_version_file(editor_version_text, project_version),
        package_manifest: manifest,
        files,
        support_files,
        post_export_files: vec![PlannedProjectPostExportFile {
            relative_path: PATH_ID_MAP_FILE.to_owned(),
            bytes: path_id_map_bytes,
        }],
        skipped: notices,
        fallbacks: fallback_notices,
    };
    let report = execute_project_export(
        output_root,
        delete_existing,
        &plan,
        limits.project_output,
        limits.yaml_write,
    )?;
    Ok(WorkspaceProjectExportReport {
        serialized_file_count: workspace.files().len(),
        exported_object_count,
        asset_file_count: report.asset_file_count,
        support_file_count: report.support_file_count,
        post_export_file_count: report.post_export_file_count,
        meta_file_count: report.meta_file_count,
        total_asset_payload_bytes: total_bytes,
        project_version,
        collection_count,
        skipped_objects: report.skipped,
        unresolved_script_content,
        fallbacks: report.fallbacks,
        opaque_fallbacks: opaque_fallback_notices,
        processing_warnings,
    })
}
