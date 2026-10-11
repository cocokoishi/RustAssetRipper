use super::*;

const PREDEFINED_ENGINE_SHADERS_2022_3_11: &str = include_str!("predefined_shaders_2022_3_11.tsv");
const PREDEFINED_ENGINE_MATERIALS_2022_3_11: &str =
    include_str!("predefined_materials_2022_3_11.tsv");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PredefinedEngineAssetTarget {
    file_id: i64,
    guid: [u8; 16],
}

fn predefined_engine_shader_target(
    shader_view: &ShaderView,
) -> Result<Option<PredefinedEngineAssetTarget>, Box<dyn Error>> {
    let Some(parsed) = shader_view.parsed.as_ref() else {
        return Ok(None);
    };
    for (line_index, line) in PREDEFINED_ENGINE_SHADERS_2022_3_11.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(4, '|');
        let Some(file_id_text) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Shader table row {} has no file ID",
                line_index + 1
            )));
        };
        let Some(resource_kind) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Shader table row {} has no resource kind",
                line_index + 1
            )));
        };
        let Some(name) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Shader table row {} has no name",
                line_index + 1
            )));
        };
        let Some(property_names_json) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Shader table row {} has no property names",
                line_index + 1
            )));
        };
        if name != parsed.name {
            continue;
        }
        let properties_match = if property_names_json.is_empty() {
            parsed.properties.is_empty()
        } else {
            parsed
                .properties
                .iter()
                .map(|property| property.name.as_str())
                .eq(property_names_json.split_whitespace())
        };
        if !properties_match {
            continue;
        }
        let file_id = file_id_text.parse::<i64>().map_err(|error| {
            invalid_project(format!(
                "predefined Shader table row {} has an invalid file ID: {error}",
                line_index + 1
            ))
        })?;
        let words = match resource_kind {
            "D" => crate::export::guid_policy::ENGINE_DEFAULT_RESOURCES_GUID_WORDS,
            "F" => crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS,
            other => {
                return Err(invalid_project(format!(
                    "predefined Shader table row {} has unknown resource kind {other:?}",
                    line_index + 1
                )));
            }
        };
        let mut guid = [0; 16];
        for (index, word) in words.iter().enumerate() {
            guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        return Ok(Some(PredefinedEngineAssetTarget { file_id, guid }));
    }
    Ok(None)
}

fn predefined_engine_material_target(
    name: &str,
    shader_name: Option<&str>,
    unity_version: Option<&str>,
) -> Result<Option<PredefinedEngineAssetTarget>, Box<dyn Error>> {
    let version_matches = unity_version
        .and_then(crate::resources::vertex_data_blob::UnityVersion::parse)
        .is_some_and(|version| version.major == 2022 && version.minor == 3 && version.patch >= 11);
    if !version_matches {
        return Ok(None);
    }

    for (line_index, line) in PREDEFINED_ENGINE_MATERIALS_2022_3_11.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(4, '|');
        let Some(file_id_text) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Material table row {} has no file ID",
                line_index + 1
            )));
        };
        let Some(resource_kind) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Material table row {} has no resource kind",
                line_index + 1
            )));
        };
        let Some(candidate_name) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Material table row {} has no name",
                line_index + 1
            )));
        };
        let Some(candidate_shader) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Material table row {} has no Shader name",
                line_index + 1
            )));
        };
        if candidate_name != name || candidate_shader != shader_name.unwrap_or_default() {
            continue;
        }
        let file_id = file_id_text.parse::<i64>().map_err(|error| {
            invalid_project(format!(
                "predefined Material table row {} has an invalid file ID: {error}",
                line_index + 1
            ))
        })?;
        let words = match resource_kind {
            "D" => crate::export::guid_policy::ENGINE_DEFAULT_RESOURCES_GUID_WORDS,
            "F" => crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS,
            other => {
                return Err(invalid_project(format!(
                    "predefined Material table row {} has unknown resource kind {other:?}",
                    line_index + 1
                )));
            }
        };
        let mut guid = [0; 16];
        for (index, word) in words.iter().enumerate() {
            guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        return Ok(Some(PredefinedEngineAssetTarget { file_id, guid }));
    }
    Ok(None)
}

fn resolved_material_shader_name(
    material: &crate::resources::material::MaterialView,
    source: &WorkspaceObject,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Option<Option<String>>, Box<dyn Error>> {
    let Some((file_id, path_id)) = material.shader else {
        return Ok(Some(None));
    };
    let mut matching_edges = reference_graph
        .references_from_object(source.file_index, source.object_index)
        .filter(|edge| {
            edge.field_path == "m_Shader" && edge.file_id == file_id && edge.path_id == path_id
        });
    let Some(edge) = matching_edges.next() else {
        return Err(invalid_project(format!(
            "Material {}:{} has non-null m_Shader PPtr ({file_id}, {path_id}) without a matching reference-graph edge",
            source.file_index, source.path_id
        )));
    };
    if matching_edges.next().is_some() {
        return Err(invalid_project(format!(
            "Material {}:{} has duplicate reference-graph edges for m_Shader PPtr ({file_id}, {path_id})",
            source.file_index, source.path_id
        )));
    }
    match &edge.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } if reference_graph.object_class_id(*target_file_index, *target_object_index)
            == Some(SHADER_CLASS_ID) =>
        {
            let Some(target) = objects.get(*target_file_index, *target_object_index) else {
                return Err(invalid_project(format!(
                    "Material {}:{} m_Shader resolves to missing workspace object {target_file_index}:{target_object_index}",
                    source.file_index, source.path_id
                )));
            };
            let Ok(payload) = &target.payload else {
                return Ok(None);
            };
            let view = ShaderView::from_value(&payload.root).map_err(|error| {
                invalid_project(format!(
                    "Material {}:{} resolved Shader {}:{} could not be read: {error}",
                    source.file_index, source.path_id, target.file_index, target.path_id
                ))
            })?;
            Ok(view.parsed.map(|form| Some(form.name)))
        }
        MultiFilePPtrResolution::PredefinedEngineAsset {
            resource,
            path_id,
            class_id,
            ..
        } if *class_id == SHADER_CLASS_ID => Ok(
            predefined_engine_shader_name(*resource, *path_id).map(|name| Some(name.to_owned()))
        ),
        _ => Ok(None),
    }
}

fn predefined_engine_shader_name(
    resource: crate::serialized_file::reference_graph::PredefinedEngineResourceKind,
    path_id: i64,
) -> Option<&'static str> {
    let resource_kind = match resource {
        crate::serialized_file::reference_graph::PredefinedEngineResourceKind::DefaultResources => {
            "D"
        }
    };
    PREDEFINED_ENGINE_SHADERS_2022_3_11
        .lines()
        .find_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let mut fields = line.splitn(4, '|');
            let file_id = fields.next()?.parse::<i64>().ok()?;
            let candidate_resource = fields.next()?;
            let name = fields.next()?;
            (file_id == path_id && candidate_resource == resource_kind).then_some(name)
        })
}

fn project_object_name(
    class_id: i32,
    payload: Option<&ParsedObjectPayload>,
    shader_view: Option<&ShaderView>,
) -> Option<String> {
    if class_id == SHADER_CLASS_ID {
        if let Some(name) = shader_view
            .and_then(|view| view.parsed.as_ref())
            .map(|form| form.name.as_str())
            .filter(|name| !name.is_empty())
        {
            // In the current Shader layout, the generated IShader.Name property reads
            // m_ParsedForm.Name. The root m_Name field is not the export identity.
            return Some(name.to_owned());
        }
    }
    payload.and_then(object_asset_name)
}

fn texture_has_exportable_image_payload(payload: &ParsedObjectPayload) -> bool {
    let Ok(view) = crate::resources::texture::Texture2DView::from_value(&payload.root) else {
        return false;
    };
    view.width > 0
        && view.height > 0
        && (!view.image_data.is_empty()
            || view
                .stream_data
                .as_ref()
                .is_some_and(|stream| stream.is_set() && stream.size > 0))
}

fn texture3d_has_exportable_image_payload(payload: &ParsedObjectPayload) -> bool {
    let Ok(view) = crate::resources::texture_array::Texture3DView::from_value(&payload.root) else {
        return false;
    };
    view.width > 0
        && view.height > 0
        && view.depth > 0
        && (!view.image_data.is_empty()
            || view
                .stream_data
                .as_ref()
                .is_some_and(|stream| stream.is_set() && stream.size > 0))
}

fn should_report_unported_texture_route(
    image_payload_exportable: bool,
    texture_route_supported: bool,
    yaml_lightmap_route: bool,
) -> bool {
    image_payload_exportable && !texture_route_supported && !yaml_lightmap_route
}

fn read_audio_data_for_project_export(
    context: &ProjectRoutingContext<'_, '_, '_>,
    file_index: usize,
    path_id: i64,
    view: &crate::resources::audio::AudioClipView,
) -> Result<Vec<u8>, Box<dyn Error>> {
    use crate::resources::audio::AudioDataSource;

    match view.audio_data_source() {
        AudioDataSource::Inline(bytes) => Ok(bytes.to_vec()),
        AudioDataSource::Empty => Ok(Vec::new()),
        AudioDataSource::Resource(resource) if resource.size == 0 => Ok(Vec::new()),
        AudioDataSource::Resource(resource) => {
            if resource.path.is_empty() {
                return Err(invalid_project(format!(
                    "file {file_index} AudioClip PathID {path_id} declares {} streamed bytes with an empty resource path",
                    resource.size
                )));
            }
            let source_bundle_index = context
                .workspace
                .files()
                .get(file_index)
                .ok_or_else(|| {
                    invalid_project(format!(
                        "AudioClip references absent SerializedFile {file_index}"
                    ))
                })?
                .container_bundle_index;
            match resolve_and_read_streamed_resource_range(
                context.filesystem,
                context.workspace,
                source_bundle_index,
                StreamedResourceRange {
                    path: &resource.path,
                    offset: resource.offset,
                    size: resource.size,
                },
                context.root_catalog,
                context.resource_provider,
                context.limits.streamed_content,
            )
            .map_err(|error| {
                invalid_project(format!(
                    "file {file_index} AudioClip PathID {path_id} stream {:?} could not be read: {error}",
                    resource.path
                ))
            })? {
                StreamedResourceContentResolution::Resolved { bytes, .. } => Ok(bytes),
                StreamedResourceContentResolution::Missing {
                    unavailable_data_paths,
                } => Err(invalid_project(format!(
                    "file {file_index} AudioClip PathID {path_id} stream {:?} is missing; unavailable data paths: [{}]",
                    resource.path,
                    unavailable_data_paths.join(", ")
                ))),
                StreamedResourceContentResolution::Unset => Err(invalid_project(format!(
                    "file {file_index} AudioClip PathID {path_id} stream {:?} became unset during resolution",
                    resource.path
                ))),
            }
        }
    }
}

pub(super) fn route_project_objects<'context, 'provider, 'graph>(
    context: &ProjectRoutingContext<'context, 'provider, 'graph>,
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
    let scriptable_object_groups = context.scriptable_object_groups;
    let controller_collections = context.controller_collections;
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
    let mut opaque_fallback_notices: Vec<ProjectExportNotice> = Vec::new();
    for (file_index, file) in workspace.files().iter().enumerate() {
        for (object_index, object_info) in file.metadata.objects.iter().enumerate() {
            let source_key = source_object_key(file_index, object_index);
            if scene_member_keys.contains(&source_key)
                || prefab_member_keys.contains(&source_key)
                || controller_collections
                    .member_keys
                    .contains(&(file_index, object_index))
                || scriptable_object_groups
                    .member_keys
                    .contains(&(file_index, object_index))
            {
                continue;
            }
            let scriptable_group_plan = scriptable_object_groups
                .roots
                .get(&(file_index, object_index));
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
            let source_engine_guid = engine_resource_collection_guid(&file.file_name);
            let payload = object.payload.as_ref().ok();
            if payload.is_some() && !opaque_objects.contains(&(file_index, object_index)) {
                validate_decoded_object(object, object_info.path_id, reference_graph)?;
            }
            let facts = match &object.factory.kind {
                FactoryAssetKind::TypeTreeObject
                    if class_id == crate::resources::texture_array::TEXTURE3D_CLASS_ID =>
                {
                    // Texture3D has a source-generated collection even when its fields were
                    // decoded through the owning TypeTree instead of a generated class body.
                    ObjectClassFacts::new(class_id, payload.is_none())
                }
                FactoryAssetKind::TypeTreeObject => ObjectClassFacts::type_tree_object(class_id),
                FactoryAssetKind::Unknown => ObjectClassFacts::with_known(class_id, false, false),
                FactoryAssetKind::Unreadable => ObjectClassFacts::new(class_id, true),
                FactoryAssetKind::Generated(_) => {
                    ObjectClassFacts::new(class_id, payload.is_none())
                }
            };
            let shader_view = if source_engine_guid.is_none() && class_id == SHADER_CLASS_ID {
                payload
                    .map(|payload| ShaderView::from_value(&payload.root))
                    .transpose()?
            } else {
                None
            };
            let predefined_engine_shader =
                if source_engine_guid.is_none() && class_id == SHADER_CLASS_ID {
                    shader_view
                        .as_ref()
                        .map(predefined_engine_shader_target)
                        .transpose()?
                        .flatten()
                } else {
                    None
                };
            let material_view = if source_engine_guid.is_none() && class_id == 21 {
                payload
                    .map(|payload| crate::resources::material::MaterialView::from_value(&payload.root))
                    .transpose()
                    .map_err(|error| {
                        invalid_project(format!(
                            "Material {}:{} could not be read for predefined engine-asset matching: {error}",
                            file_index, object_info.path_id
                        ))
                    })?
            } else {
                None
            };
            let predefined_engine_material = if let Some(material) = material_view.as_ref() {
                let resolved_shader_name =
                    resolved_material_shader_name(material, object, objects, reference_graph)?;
                match (material.name.as_deref(), resolved_shader_name) {
                    (Some(name), Some(shader_name)) => predefined_engine_material_target(
                        name,
                        shader_name.as_deref(),
                        file.metadata.prefix.unity_version_text.as_deref(),
                    )?,
                    _ => None,
                }
            } else {
                None
            };
            let predefined_engine_target = predefined_engine_shader.or(predefined_engine_material);
            let engine_guid =
                source_engine_guid.or_else(|| predefined_engine_target.map(|target| target.guid));
            let audio_view = if source_engine_guid.is_none()
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
            let mut audio_binary_export = None;
            let audio_claim_decode = if let Some(view) = audio_view.as_ref().filter(|_| {
                project_version.major == 2022
                    && project_version.minor == 3
                    && matches!(
                        settings.audio_export_format,
                        crate::export::settings::AudioExportFormat::Default
                            | crate::export::settings::AudioExportFormat::PreferWav
                    )
            }) {
                let data = read_audio_data_for_project_export(
                    context,
                    file_index,
                    object_info.path_id,
                    view,
                )?;
                let name = view
                    .name
                    .as_deref()
                    .map(String::from_utf8_lossy)
                    .unwrap_or_default();
                match crate::export::audio_export::try_decode_audio_with_limit(
                    &name,
                    view.sound_type,
                    &data,
                    crate::export::audio_export::MAX_DECODED_AUDIO_BYTES
                        .min(context.limits.max_total_asset_payload_bytes),
                ) {
                    Ok((bytes, extension)) if !bytes.is_empty() => {
                        let collection_extension = crate::export::audio_export::plan_audio_clip_collection(
                            crate::export::audio_export::AudioDecodeFact {
                                is_audio_clip: true,
                                decode_succeeded: true,
                                decoded_is_empty: false,
                                decoded_extension: extension,
                                format: settings.audio_export_format,
                            },
                        )
                        .map_err(|refusal| {
                            invalid_project(format!(
                                "AudioClip {}:{} decoded successfully but collection planning refused it: {refusal:?}",
                                file_index, object_info.path_id
                            ))
                        })?;
                        let audio_importer = audio_importer_yaml_2022_3(
                            view,
                            payload.and_then(object_asset_bundle_name).as_deref(),
                        )?;
                        audio_binary_export = Some(AudioBinaryExportDetails {
                            bytes,
                            decoded_extension: extension,
                            collection_extension,
                            importer: audio_importer,
                        });
                        crate::export::collection_claims::AudioClaimDecode::Decoded {
                            is_ogg: extension == "ogg",
                        }
                    }
                    Ok(_) => crate::export::collection_claims::AudioClaimDecode::Empty,
                    Err(
                        error @ crate::export::audio_export::AudioDecodeError::DecodedDataLimitExceeded { .. }
                        | error @ crate::export::audio_export::AudioDecodeError::OutputAllocationFailed,
                    ) => {
                        return Err(invalid_project(format!(
                            "file {file_index} AudioClip PathID {} could not be safely decoded: {error}",
                            object_info.path_id
                        )));
                    }
                    Err(_) => crate::export::collection_claims::AudioClaimDecode::Undecodable,
                }
            } else {
                crate::export::collection_claims::AudioClaimDecode::Undecodable
            };
            let shader_text = match shader_view.as_ref().and_then(|view| view.script.as_deref()) {
                None => crate::export::collection_claims::ShaderTextState::NoScript,
                Some(script) if script_has_decompiled_text(script) => {
                    crate::export::collection_claims::ShaderTextState::DecompiledSource
                }
                Some(_) => crate::export::collection_claims::ShaderTextState::CompiledOrEmpty,
            };
            let text_asset_script = if source_engine_guid.is_none() && class_id == 49 {
                payload.map(text_asset_script_bytes).transpose()?.flatten()
            } else {
                None
            };
            let font_data = if source_engine_guid.is_none() && class_id == 128 {
                payload
                    .map(|payload| font_asset_data(&payload.root))
                    .transpose()?
                    .flatten()
            } else {
                None
            };
            let texture_image_exportable = engine_guid.is_none()
                && matches!(class_id, 28 | crate::resources::texture::CUBEMAP_CLASS_ID)
                && matches!(&object.factory.kind, FactoryAssetKind::Generated(_))
                && payload.is_some_and(texture_has_exportable_image_payload);
            let texture3d_image_exportable = engine_guid.is_none()
                && class_id == crate::resources::texture_array::TEXTURE3D_CLASS_ID
                && payload.is_some_and(texture3d_has_exportable_image_payload);
            let lightmap_scene_directory = lightmap_texture_directories
                .get(&(file_index, object_index))
                .cloned();
            let yaml_lightmap_route = lightmap_scene_directory.is_some()
                && matches!(
                    settings.lightmap_texture_export_format,
                    crate::export::settings::LightmapTextureExportFormat::Yaml
                );
            let lightmap_image_route = lightmap_scene_directory.is_some() && !yaml_lightmap_route;
            let texture_importer_supported =
                TextureImporterLayout::for_project_version(project_version).is_some();
            let cubemap_importer_supported = class_id
                != crate::resources::texture::CUBEMAP_CLASS_ID
                || TextureImporterLayout::for_project_version(project_version)
                    == Some(TextureImporterLayout::Unity2022_3);
            let texture_route_supported = texture_importer_supported
                && cubemap_importer_supported
                && ((texture_image_exportable && lightmap_scene_directory.is_none())
                    || lightmap_image_route
                    || (texture3d_image_exportable && lightmap_scene_directory.is_none()));
            let mut claim_facts = claim_facts_for_shader_text(facts, settings_facts, shader_text);
            claim_facts.audio_decode = audio_claim_decode;
            claim_facts.engine_file = engine_guid.is_some();
            claim_facts.data_valid = match class_id {
                49 => text_asset_script
                    .as_ref()
                    .is_some_and(|script| !script.is_empty()),
                128 => font_data.is_some_and(|data| !data.is_empty()),
                crate::resources::texture_array::TEXTURE3D_CLASS_ID => texture3d_image_exportable,
                _ => false,
            };
            if class_id == 128 {
                claim_facts.main = ClaimMainKind::Font;
            } else if lightmap_scene_directory.is_some() {
                // LightingDataProcessor assigns the generated LightingDataAsset as the
                // main asset for every texture referenced by LightmapSettings.Lightmaps.
                claim_facts.main = ClaimMainKind::LightingData;
            } else if texture_route_supported
                && class_id != crate::resources::texture_array::TEXTURE3D_CLASS_ID
            {
                // SpriteProcessor assigns exportable textures a generated main asset, including
                // textures referenced by Sprite objects. Keep image routing in the ordinary
                // registration walk with the processed fact.
                claim_facts.main = ClaimMainKind::SpriteInformation;
            } else if scriptable_group_plan.is_some() {
                claim_facts.main = ClaimMainKind::ScriptableObjectGroup;
            }
            let selection = select_collection_with_claim_facts(facts, settings_facts, claim_facts)?;
            if scriptable_group_plan.is_some()
                && selection.exporter != "ScriptableObjectGroupExporter"
            {
                return Err(invalid_project(format!(
                    "MonoBehaviour {}:{} was identified as a generated ScriptableObjectGroup root, but registration selected {}",
                    file_index, object_info.path_id, selection.exporter
                )));
            }
            if selection.exporter == "LightmapTextureAssetExporter" && !texture_importer_supported {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} PathID {} selected LightmapTextureAssetExporter, but Unity {}.{} has no source-verified TextureImporter layout",
                    object.path_id, project_version.major, project_version.minor
                )));
            }
            let audio_yaml_route = if selection.exporter == "YamlAudioExporter" {
                audio_view.as_ref().ok_or_else(|| {
                    invalid_project(
                        "YamlAudioExporter was selected without decoded AudioClip fields",
                    )
                })?;
                if settings.audio_export_format
                    == crate::export::settings::AudioExportFormat::Native
                {
                    return Err(invalid_project(format!(
                        "file {file_index} object {object_index} PathID {} selected YamlAudioExporter under Native, whose binary AudioClip project route is not integrated",
                        object.path_id
                    )));
                }
                // The source registers this YAML collection as the fallback after
                // AudioClipExporter declines undecodable payloads. It preserves inline
                // m_AudioData and stages a resolved m_Resource as a .resS sidecar even
                // when Default or PreferWav is configured.
                true
            } else {
                false
            };
            if class_id == crate::resources::audio::AUDIO_CLIP_CLASS_ID
                && selection.exporter == "NativeAudioExporter"
            {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} PathID {} selected NativeAudioExporter, whose binary native AudioClip route is not integrated",
                    object.path_id
                )));
            }
            let audio_binary_route = selection.exporter == "AudioClipExporter";
            if audio_binary_route && audio_binary_export.is_none() {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} PathID {} selected AudioClipExporter without retained decoded audio bytes",
                    object.path_id
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
                let expected_exporter =
                    if class_id == crate::resources::texture_array::TEXTURE3D_CLASS_ID {
                        "TextureArrayAssetExporter"
                    } else if lightmap_image_route {
                        "LightmapTextureAssetExporter"
                    } else {
                        "TextureAssetExporter"
                    };
                if selection.exporter != expected_exporter {
                    return Err(invalid_project(format!(
                        "generated texture class {class_id} selected {}, expected the registered {expected_exporter}",
                        selection.exporter,
                    )));
                }
                let payload = payload.ok_or_else(|| {
                    invalid_project(
                        "texture image exporter was selected without decoded texture fields",
                    )
                })?;
                let asset_bundle_name = object_asset_bundle_name(payload);
                let source_unity_version = file
                    .metadata
                    .prefix
                    .unity_version_text
                    .as_deref()
                    .and_then(crate::resources::vertex_data_blob::UnityVersion::parse);
                let plan = if class_id == crate::resources::texture_array::TEXTURE3D_CLASS_ID {
                    plan_texture3d_project_export_for_source_version(
                        &payload.root,
                        project_version,
                        source_unity_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )
                } else if class_id == crate::resources::texture::CUBEMAP_CLASS_ID {
                    plan_cubemap_project_export_for_source_version(
                        &payload.root,
                        project_version,
                        source_unity_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )
                } else if lightmap_image_route {
                    plan_lightmap_texture_project_export_for_source_version(
                        &payload.root,
                        project_version,
                        source_unity_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )
                } else {
                    plan_texture_project_export_for_source_version(
                        &payload.root,
                        project_version,
                        source_unity_version,
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
            let script_export = if class_id == 115 && source_engine_guid.is_none() {
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
                    ExportCollectionKind::Asset if selection.exporter == "FontAssetExporter" => {
                        if class_id != 128 || font_data.map_or(true, |data| data.is_empty()) {
                            return Err(invalid_project(format!(
                                "FontAssetExporter selected class {class_id} at {file_index}:{} without non-empty Font.m_FontData",
                                object_info.path_id
                            )));
                        }
                        (ObjectRoute::FontBinary, "", None)
                    }
                    ExportCollectionKind::Asset if audio_binary_route => {
                        (ObjectRoute::AudioBinary, "", None)
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
                    ExportCollectionKind::Asset => (
                        ObjectRoute::Asset,
                        "",
                        asset_family_fallback(selection.collection_name),
                    ),
                    ExportCollectionKind::Script | ExportCollectionKind::EmptyScript
                        if let Some(script) = script_export.as_ref() =>
                    {
                        (
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
                        )
                    }
                    ExportCollectionKind::Script | ExportCollectionKind::EmptyScript
                        if class_id == 115 =>
                    {
                        (
                            ObjectRoute::Skipped,
                            "injected MonoScripts have no standalone script file in the source exporter",
                            None,
                        )
                    }
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
            let fallback_reason = if should_report_unported_texture_route(
                texture_image_exportable || texture3d_image_exportable,
                texture_route_supported,
                yaml_lightmap_route,
            ) {
                Some(
                    "the source TextureExportCollection image/importer route is not yet ported for this Unity importer version or Sprite group",
                )
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
                    project_object_name(class_id, payload, shader_view.as_ref()),
                )
            };
            let file_id = match route {
                ObjectRoute::Asset
                | ObjectRoute::AudioYaml
                | ObjectRoute::AudioBinary
                | ObjectRoute::FontBinary
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
                ObjectRoute::EngineRedirect => {
                    predefined_engine_target.map_or(object_info.path_id, |target| target.file_id)
                }
                _ => object_info.path_id,
            };
            let scriptable_object_group = scriptable_group_plan
                .map(|plan| {
                    let mut children = Vec::new();
                    children
                        .try_reserve_exact(plan.children.len())
                        .map_err(|_| {
                            invalid_project(format!(
                                "could not allocate ScriptableObjectGroup children for {}:{}",
                                file_index, object_info.path_id
                            ))
                        })?;
                    for member in &plan.children {
                        let child = objects
                            .get(member.file_index, member.object_index)
                            .ok_or_else(|| {
                                invalid_project(format!(
                                    "ScriptableObjectGroup {}:{} member {}:{} is absent from ObjectWorkspace",
                                    file_index,
                                    object_info.path_id,
                                    member.file_index,
                                    member.object_index
                                ))
                            })?;
                        if child.factory.class_id != 114 {
                            return Err(invalid_project(format!(
                                "ScriptableObjectGroup {}:{} member {}:{} has class ID {}, expected MonoBehaviour (114)",
                                file_index,
                                object_info.path_id,
                                member.file_index,
                                child.path_id,
                                child.factory.class_id
                            )));
                        }
                        children.push(RoutedScriptableObjectGroupChild {
                            object: child,
                            file_id: member.file_id,
                        });
                    }
                    Ok::<_, Box<dyn Error>>(RoutedScriptableObjectGroup {
                        extension: plan.extension,
                        children,
                    })
                })
                .transpose()?;
            let controller_collection = controller_collections
                .roots
                .get(&(file_index, object_index))
                .map(|plan| {
                    let mut children = Vec::new();
                    children
                        .try_reserve_exact(plan.children.len())
                        .map_err(|_| {
                            invalid_project("could not allocate routed AnimatorController members")
                        })?;
                    for member in &plan.children {
                        let object = objects.get(member.key.0, member.key.1).ok_or_else(|| {
                            invalid_project(
                                "routed AnimatorController member is absent from ObjectWorkspace",
                            )
                        })?;
                        children.push(RoutedControllerMember {
                            object,
                            file_id: member.file_id,
                            register_global: member.register_global,
                        });
                    }
                    Ok::<_, Box<dyn Error>>(RoutedControllerCollection {
                        is_release: plan.is_release,
                        children,
                    })
                })
                .transpose()?;
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
                audio_binary_export,
                text_asset_export,
                font_collection: None,
                texture_export,
                script_export,
                scriptable_object_group,
                controller_collection,
                selection,
                route,
                fallback_reason,
                collection_index,
                guid,
                pointer_guid,
                file_id,
            });
        }
    }

    attach_font_collection_subassets(&mut routed, objects, reference_graph)?;
    let fallback_notices = routed
        .iter()
        .filter_map(|entry| {
            entry
                .fallback_reason
                .map(|reason| (entry.selection.collection_name, reason))
        })
        .map(|(collection_name, reason)| ProjectExportNotice {
            subject: collection_name.to_owned(),
            reason,
        })
        .collect();

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

fn attach_font_collection_subassets<'object>(
    routed: &mut [RoutedObject<'object>],
    objects: &'object ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<(), Box<dyn Error>> {
    let mut prepared = Vec::new();
    let mut owners = HashMap::<(usize, usize), usize>::new();
    for (root_index, root) in routed.iter().enumerate() {
        if root.route != ObjectRoute::FontBinary {
            continue;
        }
        let mut children = Vec::new();
        for (field, class_id, expected_name) in [
            ("m_DefaultMaterial", 21, "Font Material"),
            ("m_Texture", 28, "Font Texture"),
        ] {
            let Some(child) = font_collection_subasset(
                root,
                field,
                class_id,
                expected_name,
                objects,
                reference_graph,
            )?
            else {
                continue;
            };
            let key = (child.object.file_index, child.object.object_index);
            if let Some(previous_root) = owners.insert(key, root_index) {
                return Err(invalid_project(format!(
                    "Font subasset {}:{} is claimed by Font collections at routed indexes {previous_root} and {root_index}",
                    key.0, key.1
                )));
            }
            children.push(child);
        }
        prepared.push((root_index, RoutedFontCollection { children }));
    }

    for (root_index, font_collection) in prepared {
        routed[root_index].font_collection = Some(font_collection);
    }
    for (entry_index, entry) in routed.iter_mut().enumerate() {
        let key = (entry.object.file_index, entry.object.object_index);
        if let Some(root_index) = owners.get(&key).copied() {
            if root_index == entry_index {
                return Err(invalid_project(format!(
                    "Font collection at routed index {root_index} claims itself as a generated subasset"
                )));
            }
            entry.route = ObjectRoute::Skipped;
            entry.collection_index = usize::MAX;
            entry.fallback_reason = None;
        }
    }
    Ok(())
}

fn font_collection_subasset<'object>(
    root: &RoutedObject<'object>,
    field_name: &str,
    expected_class_id: i32,
    expected_name: &str,
    objects: &'object ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Option<RoutedFontCollectionChild<'object>>, Box<dyn Error>> {
    let payload = root.object.payload.as_ref().map_err(|error| {
        invalid_project(format!(
            "Font {}:{} has no decoded payload while planning generated subassets: {error}",
            root.object.file_index, root.object.path_id
        ))
    })?;
    let value = payload
        .root
        .unique_field_in_base_chain(field_name)
        .map_err(|error| invalid_project(format!("Font {field_name} lookup failed: {error}")))?;
    let Some(value) = value else {
        return Ok(None);
    };
    let SerializedValue::PPtr { file_id, path_id } = value else {
        return Err(invalid_project(format!(
            "Font {}:{} field {field_name} is not a PPtr",
            root.object.file_index, root.object.path_id
        )));
    };
    if *path_id == 0 {
        return Ok(None);
    }
    let mut edges = reference_graph
        .references_from_object(root.object.file_index, root.object.object_index)
        .filter(|edge| {
            edge.field_path == field_name && edge.file_id == *file_id && edge.path_id == *path_id
        });
    let edge = edges.next().ok_or_else(|| {
        invalid_project(format!(
            "Font {}:{} has non-null {field_name} PPtr ({file_id}, {path_id}) without a matching reference-graph edge",
            root.object.file_index, root.object.path_id
        ))
    })?;
    if edges.next().is_some() {
        return Err(invalid_project(format!(
            "Font {}:{} has duplicate reference-graph edges for {field_name} PPtr ({file_id}, {path_id})",
            root.object.file_index, root.object.path_id
        )));
    }
    let (target_file_index, target_object_index) = match &edge.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => (*target_file_index, *target_object_index),
        MultiFilePPtrResolution::PredefinedEngineAsset { .. } => return Ok(None),
        MultiFilePPtrResolution::Null => {
            return Err(invalid_project(format!(
                "Font {}:{} non-null {field_name} PPtr was classified as null",
                root.object.file_index, root.object.path_id
            )));
        }
        unresolved => {
            return Err(invalid_project(format!(
                "Font {}:{} {field_name} PPtr ({file_id}, {path_id}) is unresolved: {unresolved:?}",
                root.object.file_index, root.object.path_id
            )));
        }
    };
    let target = objects
        .get(target_file_index, target_object_index)
        .ok_or_else(|| {
            invalid_project(format!(
                "Font {}:{} {field_name} resolves to absent workspace object {target_file_index}:{target_object_index}",
                root.object.file_index, root.object.path_id
            ))
        })?;
    if target.factory.class_id != expected_class_id {
        return Err(invalid_project(format!(
            "Font {}:{} {field_name} resolves to class {}, expected {expected_class_id}",
            root.object.file_index, root.object.path_id, target.factory.class_id
        )));
    }
    if target.factory.name != expected_name {
        return Ok(None);
    }
    let file_id =
        crate::export::ids::get_main_export_id(expected_class_id, 0).map_err(|error| {
            invalid_project(format!(
                "Font generated {expected_name:?} file ID could not be planned: {error}"
            ))
        })?;
    Ok(Some(RoutedFontCollectionChild {
        object: target,
        file_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        predefined_engine_material_target, predefined_engine_shader_target, project_object_name,
        should_report_unported_texture_route,
    };
    use crate::export::project_workflow::SHADER_CLASS_ID;
    use crate::resources::shader::{ParsedShaderForm, ParsedShaderProperty, ShaderView};

    #[test]
    fn default_yaml_lightmaps_do_not_report_an_unported_image_route() {
        assert!(!should_report_unported_texture_route(true, false, true));
        assert!(should_report_unported_texture_route(true, false, false));
        assert!(!should_report_unported_texture_route(true, true, false));
        assert!(!should_report_unported_texture_route(false, false, false));
    }

    #[test]
    fn unity_2022_3_predefined_shader_matches_name_and_ordered_properties() {
        let shader = ShaderView {
            parsed: Some(ParsedShaderForm {
                name: "Unlit/Color".to_owned(),
                properties: vec![ParsedShaderProperty {
                    name: "_Color".to_owned(),
                    ..ParsedShaderProperty::default()
                }],
                ..ParsedShaderForm::default()
            }),
            ..ShaderView::default()
        };

        let target = predefined_engine_shader_target(&shader)
            .unwrap()
            .expect("the 2022.3 TPK contains Unlit/Color");
        let mut expected_guid = [0; 16];
        for (index, word) in crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS
            .iter()
            .enumerate()
        {
            expected_guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        assert_eq!(target.file_id, 10755);
        assert_eq!(target.guid, expected_guid);

        let mut different_properties = shader;
        different_properties.parsed.as_mut().unwrap().properties[0].name = "_MainTex".to_owned();
        assert_eq!(
            predefined_engine_shader_target(&different_properties).unwrap(),
            None
        );
    }

    #[test]
    fn unity_2022_3_predefined_material_matches_name_and_resolved_shader() {
        let target = predefined_engine_material_target(
            "Default-Material",
            Some("Standard"),
            Some("2022.3.22f1"),
        )
        .unwrap()
        .expect("2022.3 TPK maps Default-Material to builtin extra");
        let mut expected_guid = [0; 16];
        for (index, word) in crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS
            .iter()
            .enumerate()
        {
            expected_guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        assert_eq!(target.file_id, 10303);
        assert_eq!(target.guid, expected_guid);
        assert_eq!(
            predefined_engine_material_target(
                "Default-Material",
                Some("Unlit/Color"),
                Some("2022.3.22f1")
            )
            .unwrap(),
            None,
            "the resolved Shader name is part of the baseline cache identity"
        );
        assert_eq!(
            predefined_engine_material_target(
                "Default-Material",
                Some("Standard"),
                Some("2022.3.10f1")
            )
            .unwrap(),
            None,
            "a TPK snapshot cannot be applied before its source version"
        );
    }

    #[test]
    fn unity_2022_3_predefined_standard_shader_matches_all_ordered_properties() {
        let property_names = [
            "_Color",
            "_MainTex",
            "_Cutoff",
            "_Glossiness",
            "_GlossMapScale",
            "_SmoothnessTextureChannel",
            "_Metallic",
            "_MetallicGlossMap",
            "_SpecularHighlights",
            "_GlossyReflections",
            "_BumpScale",
            "_BumpMap",
            "_Parallax",
            "_ParallaxMap",
            "_OcclusionStrength",
            "_OcclusionMap",
            "_EmissionColor",
            "_EmissionMap",
            "_DetailMask",
            "_DetailAlbedoMap",
            "_DetailNormalMapScale",
            "_DetailNormalMap",
            "_UVSec",
            "_Mode",
            "_SrcBlend",
            "_DstBlend",
            "_ZWrite",
        ];
        let shader = ShaderView {
            parsed: Some(ParsedShaderForm {
                name: "Standard".to_owned(),
                properties: property_names
                    .iter()
                    .map(|name| ParsedShaderProperty {
                        name: (*name).to_owned(),
                        ..ParsedShaderProperty::default()
                    })
                    .collect(),
                ..ParsedShaderForm::default()
            }),
            ..ShaderView::default()
        };

        let target = predefined_engine_shader_target(&shader)
            .unwrap()
            .expect("the 2022.3 TPK contains Standard");
        assert_eq!(target.file_id, 46);
        let mut expected_guid = [0; 16];
        for (index, word) in crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS
            .iter()
            .enumerate()
        {
            expected_guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        assert_eq!(target.guid, expected_guid);

        let mut reordered = shader;
        reordered.parsed.as_mut().unwrap().properties.swap(0, 1);
        assert_eq!(
            predefined_engine_shader_target(&reordered).unwrap(),
            None,
            "property ordering is part of the predefined shader identity"
        );
    }

    #[test]
    fn shader_export_name_comes_from_parsed_form() {
        let shader = ShaderView {
            parsed: Some(ParsedShaderForm {
                name: "Hidden/lilToonOutline".to_owned(),
                ..ParsedShaderForm::default()
            }),
            ..ShaderView::default()
        };

        assert_eq!(
            project_object_name(SHADER_CLASS_ID, None, Some(&shader)).as_deref(),
            Some("Hidden/lilToonOutline")
        );
    }
}
