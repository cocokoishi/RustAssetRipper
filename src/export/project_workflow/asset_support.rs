use super::*;

pub(super) fn text_asset_script_bytes(
    payload: &ParsedObjectPayload,
) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
    let script = payload
        .root
        .unique_field_in_base_chain("m_Script")
        .map_err(|error| invalid_project(format!("TextAsset m_Script lookup failed: {error}")))?;
    let Some(crate::serialized_file::object::SerializedValue::StringBytes(bytes)) = script else {
        if script.is_none() {
            return Ok(None);
        }
        return Err(invalid_project(
            "TextAsset m_Script field is not a serialized string",
        ));
    };
    let mut output = Vec::new();
    output
        .try_reserve_exact(bytes.len())
        .map_err(|_| invalid_project("could not allocate TextAsset script bytes"))?;
    output.extend_from_slice(bytes);
    Ok(Some(output))
}

pub(super) fn object_asset_bundle_name(payload: &ParsedObjectPayload) -> Option<String> {
    for field in ["AssetBundleName_R", "AssetBundleName", "m_AssetBundleName"] {
        if let Ok(Some(crate::serialized_file::object::SerializedValue::StringBytes(bytes))) =
            payload.root.unique_field_in_base_chain(field)
        {
            return Some(String::from_utf8_lossy(bytes).into_owned());
        }
    }
    None
}

pub(super) fn shader_non_modifiable_textures_yaml(
    shader: &ShaderView,
    entry: &RoutedObject<'_>,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    project_version: SceneUnityVersion,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let Some(textures) = shader.non_modifiable_textures.as_deref() else {
        return Ok(UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow));
    };
    if textures.is_empty() {
        return Ok(UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow));
    }
    let mut output = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for (index, texture) in textures.iter().enumerate() {
        let expected_path = format!("m_NonModifiableTextures[{index}].second");
        let mut matching = reference_graph
            .references_from_object(entry.object.file_index, entry.object.object_index)
            .filter(|reference| {
                reference.field_path == expected_path
                    && reference.file_id == texture.file_id
                    && reference.path_id == texture.path_id
            });
        let reference = matching.next().ok_or_else(|| {
            invalid_project(format!(
                "ShaderImporter texture key {:?} at index {index} has no matching PPtr scan edge at {expected_path:?}",
                texture.name
            ))
        })?;
        if matching.next().is_some() {
            return Err(invalid_project(format!(
                "ShaderImporter texture key {:?} at index {index} has duplicate PPtr scan edges",
                texture.name
            )));
        }

        let resolution = match &reference.resolution {
            MultiFilePPtrResolution::Null => ProjectPointerResolution::Null,
            MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index,
            } => {
                let target = objects
                    .get(*target_file_index, *target_object_index)
                    .ok_or_else(|| {
                        invalid_project(format!(
                            "ShaderImporter texture key {:?} resolves to absent workspace object {target_file_index}:{target_object_index}",
                            texture.name
                        ))
                    })?;
                let identity = ExportAssetIdentity {
                    collection: ExportAssetCollectionIdentity::SerializedFile {
                        file_index: target.file_index,
                    },
                    object_index: target.object_index,
                    path_id: target.path_id,
                    class_id: target.factory.class_id,
                };
                pointer_map.resolve(
                    ProjectPointerRequest::ResolvedAsset(identity),
                    entry.collection_index,
                )?
            }
            unresolved => {
                return Err(invalid_project(format!(
                    "ShaderImporter texture key {:?} at {expected_path:?} has unresolved input PPtr {:?}",
                    texture.name, unresolved
                )));
            }
        };
        let pointer = project_pointer_to_unity_yaml(
            &resolution,
            project_version,
            Some(crate::export::script_hashing::UnityGuid::MISSING_REFERENCE.to_byte_array()),
        )
        .map_err(|error| {
            invalid_project(format!(
                "ShaderImporter texture key {:?} PPtr could not be converted to project YAML: {error}",
                texture.name
            ))
        })?;
        let mut pair = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        pair.insert_field(texture.name.clone(), pointer)?;
        output.push(pair)?;
    }
    Ok(output)
}

pub(super) fn add_project_support_file(
    files: &mut Vec<PlannedProjectSupportFile>,
    indexes: &mut HashMap<String, usize>,
    relative_path: String,
    bytes: &[u8],
    before_file_index: usize,
) -> Result<(), Box<dyn Error>> {
    if let Some(index) = indexes.get(&relative_path).copied() {
        let existing = files
            .get(index)
            .ok_or_else(|| invalid_project("Shader support-file index is inconsistent"))?;
        if existing.bytes != bytes {
            return Err(invalid_project(format!(
                "project exporters generated conflicting support file {relative_path:?}"
            )));
        }
        if before_file_index < existing.before_file_index {
            files[index].before_file_index = before_file_index;
        }
        return Ok(());
    }

    let mut owned_bytes = Vec::new();
    owned_bytes
        .try_reserve_exact(bytes.len())
        .map_err(|_| invalid_project("could not allocate Shader support file bytes"))?;
    owned_bytes.extend_from_slice(bytes);
    files
        .try_reserve(1)
        .map_err(|_| invalid_project("could not allocate Shader support-file list"))?;
    indexes
        .try_reserve(1)
        .map_err(|_| invalid_project("could not allocate Shader support-file index"))?;
    let index = files.len();
    indexes.insert(relative_path.clone(), index);
    files.push(PlannedProjectSupportFile {
        relative_path,
        bytes: owned_bytes,
        before_file_index,
    });
    Ok(())
}

/// The collection names whose generic YAML document is the baseline content.
pub(super) fn asset_family_fallback(collection_name: &str) -> Option<&'static str> {
    match collection_name {
        "AssetExportCollection"
        | "ScriptableObjectExportCollection"
        | "ScriptableObjectGroupExportCollection"
        | "YamlStreamedAssetExportCollection"
        | "ShaderExportCollection" => None,
        "AnimatorControllerExportCollection" => Some(
            "the animator-controller renderer is not ported; the object was written through the \
             generic asset family",
        ),
        "AudioMixerExportCollection" => Some(
            "the audio-mixer renderer is not ported; the object was written through the generic \
             asset family",
        ),
        "TerrainYamlExportCollection" => Some(
            "the terrain renderer is not ported; the object was written through the generic asset \
             family",
        ),
        _ => Some(
            "the specialized exporter for this collection is not wired into the project path; the \
             object was written through the generic asset family",
        ),
    }
}

pub(super) fn raw_payload_bytes(
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    object: &WorkspaceObject,
    cache: &mut HashMap<usize, Vec<u8>>,
    limits: WorkspaceProjectExportLimits,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let file = workspace.files().get(object.file_index).ok_or_else(|| {
        invalid_project(format!(
            "raw payload needs workspace file {}",
            object.file_index
        ))
    })?;
    if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(object.file_index) {
        let bytes =
            filesystem.read_all_bytes(&file.virtual_path, limits.max_total_asset_payload_bytes)?;
        entry.insert(bytes);
    }
    let bytes = cache
        .get(&object.file_index)
        .expect("file bytes cached above");
    let start = usize::try_from(object.raw_payload_range.start)
        .map_err(|_| invalid_project("raw payload offset does not fit the host address space"))?;
    let end = usize::try_from(object.raw_payload_range.end)
        .map_err(|_| invalid_project("raw payload end does not fit the host address space"))?;
    let slice = bytes
        .get(start..end)
        .ok_or(ProjectExportError::MissingRawPayload {
            file_index: object.file_index,
            object_index: object.object_index,
        })?;
    Ok(slice.to_vec())
}

pub(super) fn validate_decoded_object(
    object: &WorkspaceObject,
    path_id: i64,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<(), Box<dyn Error>> {
    if reference_graph.object_scan_is_complete(object.file_index, object.object_index) != Some(true)
    {
        return Err(invalid_project(format!(
            "file {} object {} PathID {path_id} has no complete PPtr scan",
            object.file_index, object.object_index
        )));
    }
    for reference in reference_graph.references_from_object(object.file_index, object.object_index)
    {
        match &reference.resolution {
            MultiFilePPtrResolution::Null | MultiFilePPtrResolution::Object { .. } => {}
            unresolved => {
                return Err(invalid_project(format!(
                    "file {} object {} PathID {path_id} has unresolved PPtr at {:?}: {unresolved:?}",
                    object.file_index, object.object_index, reference.field_path
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn new_collection_guid(sequence: u64) -> [u8; 16] {
    static NEXT_GUID: AtomicU64 = AtomicU64::new(0);
    let counter = NEXT_GUID.fetch_add(1, Ordering::Relaxed);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let identity = (sequence, counter, std::process::id(), time);
    let first = std::collections::hash_map::RandomState::new().hash_one((&identity, 0u8));
    let second = std::collections::hash_map::RandomState::new().hash_one((&identity, 1u8));
    let mut guid = [0u8; 16];
    guid[..8].copy_from_slice(&first.to_le_bytes());
    guid[8..].copy_from_slice(&second.to_le_bytes());
    if guid == [0; 16] {
        guid[0] = 1;
    }
    guid
}

pub(super) fn invalid_project(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}
