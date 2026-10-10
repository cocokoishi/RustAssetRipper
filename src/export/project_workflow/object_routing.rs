use super::*;

pub(super) fn route_project_objects<'context, 'graph>(
    context: &ProjectRoutingContext<'context, 'graph>,
) -> Result<RoutedProjectObjects<'context>, Box<dyn Error>> {
    let workspace = context.workspace;
    let objects = context.objects;
    let reference_graph = context.reference_graph;
    let settings = context.settings;
    let project_version = context.project_version;
    let editor_version_text = context.editor_version_text;
    let settings_facts = context.settings_facts;
    let assembly_manager_is_set = context.assembly_manager_is_set;
    let managed_assemblies = context.managed_assemblies;
    let reference_assembly_maps = context.reference_assembly_maps;
    let opaque_objects = context.opaque_objects;
    let scene_member_keys = context.scene_member_keys;
    let prefab_member_keys = context.prefab_member_keys;
    let lightmap_texture_directories = context.lightmap_texture_directories;
    let mut used_guids = context.existing_guids.clone();
    let mut next_guid_sequence = 0u64;
    let mut next_collection_index = 0usize;
    let mut routed = Vec::new();
    routed
        .try_reserve_exact(objects.report().object_count)
        .map_err(|_| invalid_project("could not allocate routed project objects"))?;
    let mut notices: Vec<ProjectExportNotice> = Vec::new();
    let mut unresolved_script_content: Vec<ProjectExportNotice> = Vec::new();
    let mut fallback_notices: Vec<ProjectExportNotice> = Vec::new();
    let mut opaque_fallback_notices: Vec<ProjectExportNotice> = Vec::new();
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
                    reference_assembly_maps,
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

    Ok(RoutedProjectObjects {
        routed,
        notices,
        unresolved_script_content,
        fallback_notices,
        opaque_fallback_notices,
        next_collection_index,
        next_guid_sequence,
        used_guids,
    })
}
