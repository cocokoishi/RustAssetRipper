impl Display for UnityYamlWorkspaceExportError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProcessedObjectIdentity { source } => {
                write!(f, "workspace YAML source is not a SerializedFile object: {source:?}")
            }
            Self::GeneratedTransformInvariant { source, message } => write!(
                f,
                "generated Transform YAML export failed for {source:?}: {message}"
            ),
            Self::GeneratedComponentOverlayInvariant { source, message } => write!(
                f,
                "generated GameObject component overlay export failed for {source:?}: {message}"
            ),
            Self::GeneratedPrefabInvariant { source, message } => write!(
                f,
                "generated legacy Prefab YAML export failed for {source:?}: {message}"
            ),
            Self::GeneratedLightingDataInvariant { source, message } => write!(
                f,
                "generated LightingDataAsset YAML export failed for {source:?}: {message}"
            ),
            Self::SceneManagerEditorFormatInvariant { source, message } => write!(
                f,
                "Scene manager editor-format materialization failed for {source:?}: {message}"
            ),
            Self::ImageTextureSkippedWithoutContent { source } => write!(
                f,
                "baseline YAML export skips image texture {source:?} because it has no inline or streamed data"
            ),
            Self::WorkspaceIdentityMismatch {
                source,
                object_file_index,
                object_index,
                path_id,
            } => write!(
                f,
                "workspace object file {object_file_index} object {object_index} PathID {path_id} does not match export identity {source:?}"
            ),
            Self::MissingWorkspaceFile { file_index } => {
                write!(f, "SerializedFile workspace has no file at index {file_index}")
            }
            Self::MissingMetadataObject {
                file_index,
                object_index,
            } => write!(
                f,
                "SerializedFile {file_index} has no metadata object {object_index}"
            ),
            Self::MetadataIdentityMismatch {
                file_index,
                object_index,
            } => write!(
                f,
                "workspace object {file_index}:{object_index} does not match its SerializedFile metadata"
            ),
            Self::MissingClassId {
                file_index,
                object_index,
            } => write!(f, "SerializedFile {file_index} object {object_index} has no class ID"),
            Self::ClassIdMismatch {
                source_class_id,
                object_class_id,
            } => write!(
                f,
                "export identity class ID {source_class_id} differs from decoded class ID {object_class_id}"
            ),
            Self::ObjectDecode {
                file_index,
                object_index,
                message,
            } => write!(
                f,
                "could not decode SerializedFile {file_index} object {object_index}: {message}"
            ),
            Self::PayloadIdentityMismatch {
                file_index,
                object_index,
                payload_object_index,
                payload_path_id,
            } => write!(
                f,
                "decoded payload {file_index}:{payload_object_index} PathID {payload_path_id} does not match workspace object {object_index}"
            ),
            Self::UnparsedTrailingPayload {
                file_index,
                object_index,
                byte_count,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} has {byte_count} unparsed trailing payload bytes"
            ),
            Self::MissingResolvedType {
                file_index,
                object_index,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} has no resolved type index"
            ),
            Self::InvalidResolvedType {
                file_index,
                object_index,
                type_index,
                type_count,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} has type index {type_index}, but only {type_count} types exist"
            ),
            Self::MissingTypeTree {
                file_index,
                object_index,
                type_index,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} type {type_index} has no TypeTree"
            ),
            Self::EmptyTypeTree {
                file_index,
                object_index,
                type_index,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} type {type_index} has an empty TypeTree"
            ),
            Self::UnityVersionUnavailable {
                file_index,
                unity_version_text,
            } => write!(
                f,
                "SerializedFile {file_index} has no parseable Unity version: {unity_version_text:?}"
            ),
            Self::ReferenceGraphIdentityMissing {
                file_index,
                object_index,
            } => write!(
                f,
                "reference graph has no identity for SerializedFile {file_index} object {object_index}"
            ),
            Self::ReferenceGraphIdentityMismatch {
                file_index,
                object_index,
                expected_path_id,
                graph_path_id,
            } => write!(
                f,
                "reference graph identity {file_index}:{object_index} PathID {graph_path_id} differs from expected PathID {expected_path_id}"
            ),
            Self::ReferenceGraphClassIdMismatch {
                file_index,
                object_index,
                expected_class_id,
                graph_class_id,
            } => write!(
                f,
                "reference graph identity {file_index}:{object_index} class ID {graph_class_id:?} differs from expected class ID {expected_class_id}"
            ),
            Self::ReferenceScanIncomplete {
                file_index,
                object_index,
            } => write!(
                f,
                "reference scan for SerializedFile {file_index} object {object_index} is incomplete"
            ),
            Self::ReferenceEdgeMissing {
                file_index,
                object_index,
                field_path,
                file_id,
                path_id,
            } => write!(
                f,
                "reference graph has no edge for SerializedFile {file_index} object {object_index} field {field_path:?} FileID {file_id} PathID {path_id}"
            ),
            Self::ReferenceEdgeAmbiguous {
                file_index,
                object_index,
                field_path,
                file_id,
                path_id,
            } => write!(
                f,
                "reference graph has multiple edges for SerializedFile {file_index} object {object_index} field {field_path:?} FileID {file_id} PathID {path_id}"
            ),
            Self::NonNullPointerResolvedAsNull {
                file_index,
                object_index,
                field_path,
                path_id,
            } => write!(
                f,
                "non-null PPtr at SerializedFile {file_index} object {object_index} field {field_path:?} PathID {path_id} was classified as null"
            ),
            Self::ReferenceTargetIdentityMissing {
                file_index,
                object_index,
            } => write!(
                f,
                "reference graph has no identity for target {file_index}:{object_index}"
            ),
            Self::ReferenceTargetClassIdMissing {
                file_index,
                object_index,
            } => write!(
                f,
                "reference graph has no class ID for target {file_index}:{object_index}"
            ),
            Self::StreamedAssetPreparation {
                file_index,
                object_index,
                source,
            } => write!(
                f,
                "could not stage streamed asset {file_index}:{object_index} for YAML: {source}"
            ),
            Self::StreamedResourceResolution {
                file_index,
                object_index,
                path,
                source,
            } => write!(
                f,
                "could not resolve streamed resource {path:?} for asset {file_index}:{object_index}: {source}"
            ),
            Self::StreamedResourceMissing {
                file_index,
                object_index,
                path,
                unavailable_data_paths,
            } => write!(
                f,
                "streamed resource {path:?} is missing for asset {file_index}:{object_index}; unavailable DataPaths: {unavailable_data_paths:?}"
            ),
            Self::StreamedResourceUnexpectedUnset {
                file_index,
                object_index,
                path,
            } => write!(
                f,
                "streamed resource {path:?} became unset while exporting asset {file_index}:{object_index}"
            ),
            Self::StreamedAssetInvariant {
                file_index,
                object_index,
                message,
            } => write!(
                f,
                "streamed asset export invariant failed for {file_index}:{object_index}: {message}"
            ),
            Self::PointerMap(source) => Display::fmt(source, f),
            Self::Conversion(source) => Display::fmt(source, f),
        }
    }
}

impl Error for UnityYamlWorkspaceExportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PointerMap(source) => Some(source),
            Self::Conversion(source) => Some(source),
            Self::StreamedAssetPreparation { source, .. } => Some(source),
            Self::StreamedResourceResolution { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Convert a materialized PrefabProcessor Transform into a Scene YAML document.
///
/// The processed collection's constructor PPtr is resolved through its planned dependency
/// FileID and then the project export pointer map. Only the planned non-null `m_GameObject`
/// pointer is accepted; default null PPtrs remain null and any other non-null pointer fails.
#[allow(clippy::too_many_arguments)]
pub fn generated_transform_payload_to_unity_yaml_document(
    payload: &WorkspaceGeneratedTransformPayload,
    metadata: &SerializedFileMetadata,
    source: ExportAssetIdentity,
    file_id: i64,
    constructor_pointer: WorkspaceGeneratedConstructorPointer,
    pointer_map: &ProjectExportPointerMap,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
) -> Result<UnityYamlDocument, UnityYamlWorkspaceExportError> {
    let invariant_error = |message: String| {
        UnityYamlWorkspaceExportError::GeneratedTransformInvariant { source, message }
    };
    if payload.asset.class_id != 4
        || source.class_id != 4
        || source.collection
            != (ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: payload.asset.collection_index,
            })
        || source.path_id != payload.asset.path_id
        || constructor_pointer.source_asset != payload.asset
        || constructor_pointer.field != WorkspaceGeneratedPointerField::TransformGameObject
        || constructor_pointer.file_id <= 0
        || constructor_pointer.path_id != constructor_pointer.target.path_id
        || constructor_pointer.target.class_id != Some(1)
    {
        return Err(invariant_error(
            "export identity or constructor PPtr does not match the planned Transform".to_owned(),
        ));
    }

    let serialized_type = metadata.types.get(payload.type_index).ok_or_else(|| {
        invariant_error(format!(
            "Transform type index {} is outside the {}-entry type table",
            payload.type_index,
            metadata.types.len()
        ))
    })?;
    if serialized_type.raw_type_id != 4 {
        return Err(invariant_error(format!(
            "serialized type {} has class ID {}, expected Transform class ID 4",
            payload.type_index, serialized_type.raw_type_id
        )));
    }
    let type_tree = serialized_type.type_tree.as_ref().ok_or_else(|| {
        invariant_error(format!(
            "Transform type {} has no TypeTree",
            payload.type_index
        ))
    })?;
    let type_root = type_tree.nodes.first().ok_or_else(|| {
        invariant_error(format!(
            "Transform type {} has an empty TypeTree",
            payload.type_index
        ))
    })?;
    let class_name = type_root.type_name.as_deref().ok_or_else(|| {
        invariant_error("Transform TypeTree root has no resolved class name".to_owned())
    })?;
    if class_name != "Transform" {
        return Err(invariant_error(format!(
            "class-ID-4 TypeTree root is named {class_name:?}, expected \"Transform\""
        )));
    }
    let unity_version_text = metadata.prefix.unity_version_text.as_deref();
    let unity_version = unity_version_text
        .and_then(SceneUnityVersion::parse)
        .ok_or_else(|| {
            invariant_error(format!(
                "Transform type has no parseable Unity version: {unity_version_text:?}"
            ))
        })?;

    let game_object_pointer = payload
        .root
        .unique_field_in_base_chain("m_GameObject")
        .map_err(|error| invariant_error(error.to_string()))?
        .ok_or_else(|| invariant_error("Transform has no m_GameObject field".to_owned()))?;
    if game_object_pointer.pptr()
        != Some((constructor_pointer.file_id, constructor_pointer.path_id))
    {
        return Err(invariant_error(
            "Transform m_GameObject value differs from the planned constructor PPtr".to_owned(),
        ));
    }

    let encoded = metadata
        .encode_object_payload_for_type(
            metadata.objects.len(),
            payload.type_index,
            &payload.root,
            &[],
            ObjectEncodeLimits::default(),
        )
        .map_err(|error| invariant_error(error.to_string()))?;
    if encoded != payload.bytes {
        return Err(invariant_error(
            "materialized Transform bytes do not match its typed value".to_owned(),
        ));
    }

    let hints = UnityYamlObjectConversionHints::from_type_tree(type_tree, unity_version, limits)
        .map_err(UnityYamlWorkspaceExportError::Conversion)?;
    let target_asset = ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: constructor_pointer.target.file_index,
        },
        object_index: constructor_pointer.target.object_index,
        path_id: constructor_pointer.target.path_id,
        class_id: 1,
    };
    let mut found_game_object_pointer = false;
    let mut pointer_error = None;
    let document = serialized_object_value_to_unity_yaml_document(
        source,
        class_name,
        type_root.version,
        file_id,
        false,
        &payload.root,
        &hints,
        UnityYamlObjectFieldMode::ProjectAsset,
        unity_version,
        missing_reference_guid,
        limits,
        |request: UnityYamlPPtrRequest<'_>| {
            if request.source != source {
                pointer_error = Some(invariant_error(
                    "YAML walker changed the generated Transform identity".to_owned(),
                ));
                return ProjectPointerResolution::Null;
            }
            if request.file_id == 0 && request.path_id == 0 {
                return match pointer_map
                    .resolve(ProjectPointerRequest::Null, current_collection_index)
                {
                    Ok(resolution) => resolution,
                    Err(error) => {
                        pointer_error = Some(UnityYamlWorkspaceExportError::PointerMap(error));
                        ProjectPointerResolution::Null
                    }
                };
            }
            let field_name = request.field_path.rsplit('.').next().unwrap_or_default();
            if field_name != "m_GameObject"
                || request.file_id != constructor_pointer.file_id
                || request.path_id != constructor_pointer.path_id
                || found_game_object_pointer
            {
                pointer_error = Some(invariant_error(format!(
                    "unexpected non-null Transform PPtr at {:?}: FileID {} PathID {}",
                    request.field_path, request.file_id, request.path_id
                )));
                return ProjectPointerResolution::Null;
            }
            found_game_object_pointer = true;
            match pointer_map.resolve(
                ProjectPointerRequest::ResolvedAsset(target_asset),
                current_collection_index,
            ) {
                Ok(ProjectPointerResolution::Mapped(pointer)) => {
                    ProjectPointerResolution::Mapped(pointer)
                }
                Ok(_) => {
                    pointer_error = Some(invariant_error(
                        "planned GameObject has no exported project pointer".to_owned(),
                    ));
                    ProjectPointerResolution::Null
                }
                Err(error) => {
                    pointer_error = Some(UnityYamlWorkspaceExportError::PointerMap(error));
                    ProjectPointerResolution::Null
                }
            }
        },
    );
    if let Some(error) = pointer_error {
        return Err(error);
    }
    if !found_game_object_pointer {
        return Err(invariant_error(
            "YAML walker did not encounter the planned m_GameObject PPtr".to_owned(),
        ));
    }
    document.map_err(UnityYamlWorkspaceExportError::Conversion)
}

#[allow(clippy::too_many_arguments)]
fn workspace_generated_prefab_assignment_to_unity_yaml_document(
    workspace: &SerializedFileWorkspace,
    pointer_map: &ProjectExportPointerMap,
    source: ExportAssetIdentity,
    file_id: i64,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    context: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<UnityYamlDocument, UnityYamlWorkspaceExportError> {
    let invariant_error = |message: String| {
        UnityYamlWorkspaceExportError::GeneratedPrefabInvariant { source, message }
    };
    if !context.processed_assets.is_complete() {
        return Err(invariant_error(format!(
            "processed asset plan is incomplete: {}",
            context
                .processed_assets
                .incomplete_reason()
                .unwrap_or("no reason was recorded")
        )));
    }
    let Some(legacy) = context.legacy_prefab_export else {
        return Err(invariant_error(
            "generated class-1001 assignment has no legacy Prefab export context".to_owned(),
        ));
    };
    if legacy.export_ids.representation != PrefabAssetRepresentation::LegacyPrefabMarker
        || !legacy.export_ids.emit_prefab_asset
    {
        return Err(invariant_error(
            "generated class-1001 assignment is not the emitted legacy Prefab asset".to_owned(),
        ));
    }
    let ExportAssetCollectionIdentity::ProcessedCollection { collection_index } = source.collection
    else {
        return Err(invariant_error(
            "generated Prefab assignment has SerializedFile collection scope".to_owned(),
        ));
    };
    let collection = context
        .processed_assets
        .collections()
        .get(collection_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "processed collection {collection_index} is absent from the asset plan"
            ))
        })?;
    if collection.collection_index != collection_index
        || collection.kind != WorkspaceProcessedCollectionKind::GeneratedPrefabs
    {
        return Err(invariant_error(format!(
            "processed collection {collection_index} is not the GeneratedPrefabs collection"
        )));
    }
    let asset = collection.assets.get(source.object_index).ok_or_else(|| {
        invariant_error(format!(
            "processed collection {collection_index} has no planned asset at object index {}",
            source.object_index
        ))
    })?;
    if asset.identity.collection_index != collection_index
        || asset.identity.path_id != source.path_id
        || asset.identity.class_id != source.class_id
        || source.class_id != 1001
        || source.path_id == 0
    {
        return Err(invariant_error(
            "generated Prefab identity disagrees with the YAML assignment".to_owned(),
        ));
    }
    let WorkspaceProcessedPrefabAssetKind::GeneratedPrefabInstance { root_game_object } =
        &asset.kind
    else {
        return Err(invariant_error(format!(
            "processed asset {}:{} is not a generated Prefab instance",
            collection_index, source.object_index
        )));
    };
    let main_assignments = legacy
        .export_ids
        .registered_assets
        .iter()
        .filter(|assignment| assignment.is_main_asset);
    let main = main_assignments
        .clone()
        .next()
        .ok_or_else(|| invariant_error("legacy Prefab plan has no main asset".to_owned()))?;
    if main_assignments.count() != 1
        || main.asset != source
        || main.file_id != file_id
        || !legacy.export_ids.exportable_assets.contains(&source)
    {
        return Err(invariant_error(
            "legacy Prefab main assignment differs from the generated YAML identity".to_owned(),
        ));
    }

    let mut constructor_pointers = context
        .processed_assets
        .constructor_pointers()
        .iter()
        .filter(|pointer| {
            pointer.source_asset == asset.identity
                && pointer.field == WorkspaceGeneratedPointerField::PrefabInstanceRootGameObject
        });
    let constructor_pointer = constructor_pointers.next().copied().ok_or_else(|| {
        invariant_error("generated Prefab has no constructor root GameObject PPtr".to_owned())
    })?;
    if constructor_pointers.next().is_some()
        || constructor_pointer.target != *root_game_object
        || constructor_pointer.file_id <= 0
        || constructor_pointer.path_id != root_game_object.path_id
    {
        return Err(invariant_error(
            "generated Prefab root GameObject PPtr disagrees with the processed plan".to_owned(),
        ));
    }

    let target_file = workspace
        .files()
        .get(root_game_object.file_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "root GameObject file {} is absent from the workspace",
                root_game_object.file_index
            ))
        })?;
    let target_object = target_file
        .metadata
        .objects
        .get(root_game_object.object_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "root GameObject object {} is absent from file {}",
                root_game_object.object_index, root_game_object.file_index
            ))
        })?;
    if target_object.path_id != root_game_object.path_id || target_object.raw_type_id != 1 {
        return Err(invariant_error(
            "root GameObject identity disagrees with its SerializedFile object record".to_owned(),
        ));
    }

    let Some(factory_version) = FactoryUnityVersion::parse(legacy.unity_version_text) else {
        return Err(invariant_error(format!(
            "legacy Prefab has no parseable Unity version: {:?}",
            legacy.unity_version_text
        )));
    };
    let Some(unity_version) = SceneUnityVersion::parse(legacy.unity_version_text) else {
        return Err(invariant_error(format!(
            "legacy Prefab has no parseable Unity version: {:?}",
            legacy.unity_version_text
        )));
    };
    if unity_version.major > 2018 || (unity_version.major == 2018 && unity_version.minor >= 3) {
        return Err(invariant_error(format!(
            "Unity {}.{} uses PrefabImporter assets instead of legacy Prefab documents",
            unity_version.major, unity_version.minor
        )));
    }
    let (_, editor_tree) = reference_class_layouts()
        .tpk_trees(1001, &factory_version)
        .map_err(|error| invariant_error(format!("could not load class-1001 TPK layout: {error}")))?
        .ok_or_else(|| {
            invariant_error(format!(
                "source TPK has no class-1001 layout for Unity {}",
                factory_version.text
            ))
        })?;
    let editor_tree = editor_tree.as_ref();
    let type_root = editor_tree
        .nodes
        .first()
        .ok_or_else(|| invariant_error("class-1001 editor TypeTree is empty".to_owned()))?;
    if type_root.type_name.as_deref() != Some("Prefab") {
        return Err(invariant_error(format!(
            "class-1001 editor TypeTree root is {:?}, expected Prefab",
            type_root.type_name
        )));
    }

    let type_index = target_file.metadata.types.len();
    let mut metadata = target_file.metadata.clone();
    metadata.types.push(SerializedType {
        raw_type_id: 1001,
        type_id_for_hash: 1001,
        is_stripped_type: false,
        script_type_index: -1,
        script_id: None,
        old_type_hash: None,
        type_tree: Some(editor_tree.clone()),
        type_dependencies: Vec::new(),
    });
    let mut root = metadata
        .default_object_value_for_type(
            metadata.objects.len(),
            type_index,
            context.default_value_limits,
        )
        .map_err(|error| {
            invariant_error(format!(
                "could not construct legacy Prefab payload: {error}"
            ))
        })?;

    let hide_flags = root
        .unique_field_in_base_chain_mut("m_ObjectHideFlags")
        .map_err(|error| invariant_error(error.to_string()))?
        .ok_or_else(|| {
            invariant_error("Prefab TypeTree has no m_ObjectHideFlags field".to_owned())
        })?;
    match hide_flags {
        SerializedValue::U32(value) => *value = 1,
        value => {
            return Err(invariant_error(format!(
                "m_ObjectHideFlags has value shape {value:?}, expected UInt32"
            )))
        }
    }
    let root_pointer = root
        .unique_field_in_base_chain_mut("m_RootGameObject")
        .map_err(|error| invariant_error(error.to_string()))?
        .ok_or_else(|| {
            invariant_error("Prefab TypeTree has no m_RootGameObject field".to_owned())
        })?;
    let SerializedValue::PPtr {
        file_id: root_file_id,
        path_id: root_path_id,
    } = root_pointer
    else {
        return Err(invariant_error(format!(
            "m_RootGameObject has value shape {root_pointer:?}, expected PPtr"
        )));
    };
    *root_file_id = constructor_pointer.file_id;
    *root_path_id = constructor_pointer.path_id;
    let is_prefab_parent = root
        .unique_field_in_base_chain_mut("m_IsPrefabParent")
        .map_err(|error| invariant_error(error.to_string()))?
        .ok_or_else(|| {
            invariant_error("Prefab TypeTree has no m_IsPrefabParent field".to_owned())
        })?;
    match is_prefab_parent {
        SerializedValue::Bool(value) => *value = true,
        value => {
            return Err(invariant_error(format!(
                "m_IsPrefabParent has value shape {value:?}, expected Bool"
            )))
        }
    }
    metadata
        .encode_object_payload_for_type(
            metadata.objects.len(),
            type_index,
            &root,
            &[],
            context.encode_limits,
        )
        .map_err(|error| {
            invariant_error(format!(
                "legacy Prefab payload does not match its TypeTree: {error}"
            ))
        })?;

    let mut hints =
        UnityYamlObjectConversionHints::from_type_tree(editor_tree, unity_version, limits)
            .map_err(UnityYamlWorkspaceExportError::Conversion)?;
    hints.field_name_aliases.clear();
    hints.preserve_type_tree_field_names = true;
    let target_asset = ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: root_game_object.file_index,
        },
        object_index: root_game_object.object_index,
        path_id: root_game_object.path_id,
        class_id: 1,
    };
    let mut root_pointer_seen = false;
    let mut pointer_error = None;
    let document = serialized_object_value_to_unity_yaml_document(
        source,
        "Prefab",
        type_root.version,
        file_id,
        false,
        &root,
        &hints,
        UnityYamlObjectFieldMode::ProjectAsset,
        unity_version,
        missing_reference_guid,
        limits,
        |request: UnityYamlPPtrRequest<'_>| {
            if request.source != source {
                pointer_error = Some(invariant_error(
                    "YAML walker changed the generated Prefab identity".to_owned(),
                ));
                return ProjectPointerResolution::Null;
            }
            if request.file_id == 0 && request.path_id == 0 {
                return match pointer_map
                    .resolve(ProjectPointerRequest::Null, current_collection_index)
                {
                    Ok(resolution) => resolution,
                    Err(error) => {
                        pointer_error = Some(UnityYamlWorkspaceExportError::PointerMap(error));
                        ProjectPointerResolution::Null
                    }
                };
            }
            if request.field_path.rsplit('.').next() != Some("m_RootGameObject")
                || request.file_id != constructor_pointer.file_id
                || request.path_id != constructor_pointer.path_id
                || root_pointer_seen
            {
                pointer_error = Some(invariant_error(format!(
                    "unexpected non-null Prefab PPtr at {:?}: FileID {} PathID {}",
                    request.field_path, request.file_id, request.path_id
                )));
                return ProjectPointerResolution::Null;
            }
            root_pointer_seen = true;
            match pointer_map.resolve(
                ProjectPointerRequest::ResolvedAsset(target_asset),
                current_collection_index,
            ) {
                Ok(ProjectPointerResolution::Mapped(pointer)) => {
                    ProjectPointerResolution::Mapped(pointer)
                }
                Ok(_) => {
                    pointer_error = Some(invariant_error(
                        "planned root GameObject has no exported project pointer".to_owned(),
                    ));
                    ProjectPointerResolution::Null
                }
                Err(error) => {
                    pointer_error = Some(UnityYamlWorkspaceExportError::PointerMap(error));
                    ProjectPointerResolution::Null
                }
            }
        },
    );
    if let Some(error) = pointer_error {
        return Err(error);
    }
    if !root_pointer_seen {
        return Err(invariant_error(
            "YAML walker did not encounter the planned m_RootGameObject PPtr".to_owned(),
        ));
    }
    document.map_err(UnityYamlWorkspaceExportError::Conversion)
}
