use std::str;

use crate::processing::asset_factory::{
    reference_class_layouts, ClassLayoutSource, FactoryAssetKind, FactoryUnityVersion,
};
use crate::processing::scene_naming::SceneUnityVersion;
use crate::resource::{resolve_and_read_streamed_resource_info, StreamedResourceContentResolution};
use crate::serialized_file::object::{
    default_value_for_type_tree_node, ObjectDefaultValueLimits, SerializedFieldValue,
    SerializedValue,
};
use crate::serialized_file::object_workspace::WorkspaceObject;
use crate::serialized_file::reference_graph::{MultiFilePPtrResolution, MultiFileReferenceGraph};
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::super::pointer_map::{
    ProjectExportPointerMap, ProjectPointerRequest, ProjectPointerResolution, UnityMetaPointer,
    UnityProjectAssetType, UnresolvedProjectPointer,
};
use super::super::scene_collection::{ExportAssetCollectionIdentity, ExportAssetIdentity};
use super::super::streamed_assets::{
    has_streamed_yaml_asset_field, prepare_streamed_yaml_asset, StreamedYamlAssetError,
    StreamedYamlAssetKind, StreamedYamlAssetPreparation,
};
use super::super::unity_yaml::UnityYamlDocument;
use super::super::value_yaml::{
    serialized_object_value_to_unity_yaml_document, UnityYamlObjectConversionError,
    UnityYamlObjectConversionHints, UnityYamlObjectConversionLimits, UnityYamlObjectFieldMode,
    UnityYamlPPtrRequest,
};
use super::error::UnityYamlWorkspaceExportError;
use super::model::{
    WorkspaceGeneratedComponentYamlOverlay, WorkspaceGeneratedComponentYamlPointer,
    WorkspaceGeneratedLightingDataYamlPointer, WorkspaceGeneratedPrefabInternalPointer,
    WorkspaceStreamedYamlResourceContext, WorkspaceYamlExportContext,
};

pub(super) struct WorkspaceYamlObjectConversionRequest<'object> {
    pub(super) object: &'object WorkspaceObject,
    pub(super) source: ExportAssetIdentity,
    pub(super) file_id: i64,
    pub(super) processed_animation_root: Option<&'object SerializedValue>,
    pub(super) animation_clip_editor_format: bool,
    pub(super) generated_component_overlay: Option<&'object WorkspaceGeneratedComponentYamlOverlay>,
    pub(super) generated_prefab_internal: Option<&'object WorkspaceGeneratedPrefabInternalPointer>,
    pub(super) generated_lighting_data: Option<&'object WorkspaceGeneratedLightingDataYamlPointer>,
}

impl<'workspace, 'graph, 'provider> WorkspaceYamlExportContext<'workspace, 'graph, 'provider> {
    pub(super) fn convert(
        &self,
        request: WorkspaceYamlObjectConversionRequest<'_>,
    ) -> Result<Option<UnityYamlDocument>, UnityYamlWorkspaceExportError> {
        let WorkspaceYamlObjectConversionRequest {
            object,
            source,
            file_id,
            processed_animation_root,
            animation_clip_editor_format,
            generated_component_overlay,
            generated_prefab_internal,
            generated_lighting_data,
        } = request;
        let workspace = self.workspace;
        let reference_graph = self.reference_graph;
        let pointer_map = self.pointer_map;
        let current_collection_index = self.current_collection_index;
        let missing_reference_guid = self.missing_reference_guid;
        let limits = self.limits;
        let streamed_resources = self.streamed_resources.as_ref();
        let file_index = match source.collection {
            ExportAssetCollectionIdentity::SerializedFile { file_index } => file_index,
            ExportAssetCollectionIdentity::ProcessedCollection { .. } => {
                return Err(UnityYamlWorkspaceExportError::ProcessedObjectIdentity { source });
            }
        };
        if object.file_index != file_index
            || object.object_index != source.object_index
            || object.path_id != source.path_id
        {
            return Err(UnityYamlWorkspaceExportError::WorkspaceIdentityMismatch {
                source,
                object_file_index: object.file_index,
                object_index: object.object_index,
                path_id: object.path_id,
            });
        }

        let file = workspace
            .files()
            .get(file_index)
            .ok_or(UnityYamlWorkspaceExportError::MissingWorkspaceFile { file_index })?;
        let metadata_object = file.metadata.objects.get(object.object_index).ok_or(
            UnityYamlWorkspaceExportError::MissingMetadataObject {
                file_index,
                object_index: object.object_index,
            },
        )?;
        if metadata_object.path_id != source.path_id
            || metadata_object.resolved_type_index != object.resolved_type_index
            || metadata_object.class_id.map(i32::from) != object.class_id.map(i32::from)
            || crate::processing::asset_factory::resolve_read_class_id(metadata_object.raw_type_id)
                != object.factory.class_id
            || metadata_object.is_stripped != object.is_stripped
        {
            return Err(UnityYamlWorkspaceExportError::MetadataIdentityMismatch {
                file_index,
                object_index: object.object_index,
            });
        }
        let class_id = object.factory.class_id;
        if source.class_id != class_id {
            return Err(UnityYamlWorkspaceExportError::ClassIdMismatch {
                source_class_id: source.class_id,
                object_class_id: class_id,
            });
        }
        let payload = object.payload.as_ref().map_err(|source| {
            UnityYamlWorkspaceExportError::ObjectDecode {
                file_index,
                object_index: object.object_index,
                message: source.to_string(),
            }
        })?;
        if payload.object_index != object.object_index || payload.path_id != source.path_id {
            return Err(UnityYamlWorkspaceExportError::PayloadIdentityMismatch {
                file_index,
                object_index: object.object_index,
                payload_object_index: payload.object_index,
                payload_path_id: payload.path_id,
            });
        }
        if let Some(overlay) = generated_component_overlay {
            if class_id != 1
                || overlay.payload.object_index != payload.object_index
                || overlay.payload.path_id != payload.path_id
                || overlay.payload.type_name != payload.type_name
                || overlay.payload.consumed_bytes != payload.consumed_bytes
                || overlay.payload.trailing_bytes != payload.trailing_bytes
                || overlay.pointer.source != source
                || overlay.pointer.file_id <= 0
                || overlay.pointer.path_id == 0
                || overlay.pointer.target.class_id != 4
                || !matches!(
                    overlay.pointer.target.collection,
                    ExportAssetCollectionIdentity::ProcessedCollection { .. }
                )
            {
                return Err(UnityYamlWorkspaceExportError::GeneratedComponentOverlayInvariant {
                source,
                message: "edited GameObject payload or planned component pointer disagrees with the source identity".to_owned(),
            });
            }
        }
        if processed_animation_root.is_some()
            && (class_id != 74
                || generated_component_overlay.is_some()
                || generated_prefab_internal.is_some()
                || generated_lighting_data.is_some())
        {
            return Err(UnityYamlWorkspaceExportError::AnimationClipEditorFormatInvariant {
                source,
                message: "processed AnimationClip root is attached to an incompatible object or another generated overlay".to_owned(),
            });
        }
        if !payload.trailing_bytes.is_empty() {
            return Err(UnityYamlWorkspaceExportError::UnparsedTrailingPayload {
                file_index,
                object_index: object.object_index,
                byte_count: payload.trailing_bytes.len(),
            });
        }

        let type_index = object.resolved_type_index.ok_or(
            UnityYamlWorkspaceExportError::MissingResolvedType {
                file_index,
                object_index: object.object_index,
            },
        )?;
        let serialized_type = file.metadata.types.get(type_index).ok_or(
            UnityYamlWorkspaceExportError::InvalidResolvedType {
                file_index,
                object_index: object.object_index,
                type_index,
                type_count: file.metadata.types.len(),
            },
        )?;
        let type_tree = object
            .recovered_type_tree
            .as_deref()
            .or(serialized_type.type_tree.as_ref())
            .ok_or(UnityYamlWorkspaceExportError::MissingTypeTree {
                file_index,
                object_index: object.object_index,
                type_index,
            })?;
        let unity_version_text = file.metadata.prefix.unity_version_text.as_deref();
        let unity_version = unity_version_text
            .and_then(SceneUnityVersion::parse)
            .ok_or_else(|| UnityYamlWorkspaceExportError::UnityVersionUnavailable {
                file_index,
                unity_version_text: unity_version_text.map(str::to_owned),
            })?;

        let graph_identity = reference_graph
            .object_identity(file_index, object.object_index)
            .ok_or(
                UnityYamlWorkspaceExportError::ReferenceGraphIdentityMissing {
                    file_index,
                    object_index: object.object_index,
                },
            )?;
        if graph_identity.path_id != source.path_id {
            return Err(
                UnityYamlWorkspaceExportError::ReferenceGraphIdentityMismatch {
                    file_index,
                    object_index: object.object_index,
                    expected_path_id: source.path_id,
                    graph_path_id: graph_identity.path_id,
                },
            );
        }
        if reference_graph.object_class_id(file_index, object.object_index) != Some(class_id) {
            return Err(
                UnityYamlWorkspaceExportError::ReferenceGraphClassIdMismatch {
                    file_index,
                    object_index: object.object_index,
                    expected_class_id: class_id,
                    graph_class_id: reference_graph
                        .object_class_id(file_index, object.object_index),
                },
            );
        }
        if reference_graph.object_scan_is_complete(file_index, object.object_index) != Some(true) {
            return Err(UnityYamlWorkspaceExportError::ReferenceScanIncomplete {
                file_index,
                object_index: object.object_index,
            });
        }

        let mut streamed_export_root = None;
        let streamed_asset_kind = if class_id == 43 {
            Some(StreamedYamlAssetKind::Mesh)
        } else if crate::export::project_exporter::class_interfaces(class_id)
            .contains(&"IImageTexture")
        {
            Some(StreamedYamlAssetKind::ImageTexture)
        } else {
            None
        };
        if let Some(asset_kind) = streamed_asset_kind {
            let staging_limits = streamed_resources
                .map(|context| context.staging_limits)
                .unwrap_or_default();
            let has_stream =
                has_streamed_yaml_asset_field(&payload.root, asset_kind).map_err(|source| {
                    UnityYamlWorkspaceExportError::StreamedAssetPreparation {
                        file_index,
                        object_index: object.object_index,
                        source: Box::new(source),
                    }
                })?;
            if has_stream {
                if payload.allocated_bytes > staging_limits.max_source_value_copy_bytes {
                    return Err(UnityYamlWorkspaceExportError::StreamedAssetPreparation {
                        file_index,
                        object_index: object.object_index,
                        source: Box::new(StreamedYamlAssetError::SourceValueCopyLimitExceeded {
                            size: payload.allocated_bytes,
                            limit: staging_limits.max_source_value_copy_bytes,
                        }),
                    });
                }
                let mut export_root = payload.root.clone();
                match prepare_streamed_yaml_asset(
                    &mut export_root,
                    asset_kind,
                    None,
                    staging_limits,
                ) {
                    Ok(
                        StreamedYamlAssetPreparation::NoStreamField
                        | StreamedYamlAssetPreparation::StreamUnset
                        | StreamedYamlAssetPreparation::ExistingInlineData { .. },
                    ) => {}
                    Ok(StreamedYamlAssetPreparation::ImageTextureWithoutData) => return Ok(None),
                    Ok(StreamedYamlAssetPreparation::Materialized { .. }) => {
                        return Err(UnityYamlWorkspaceExportError::StreamedAssetInvariant {
                            file_index,
                            object_index: object.object_index,
                            message: "staging reported materialization without supplied bytes"
                                .to_owned(),
                        });
                    }
                    Err(StreamedYamlAssetError::StreamContentRequired { resource }) => {
                        if asset_kind == StreamedYamlAssetKind::ImageTexture && resource.size == 0 {
                            return Ok(None);
                        }
                        let Some(context) = streamed_resources else {
                            return Err(UnityYamlWorkspaceExportError::StreamedAssetPreparation {
                                file_index,
                                object_index: object.object_index,
                                source: Box::new(StreamedYamlAssetError::StreamContentRequired {
                                    resource: resource.clone(),
                                }),
                            });
                        };
                        let resolution = resolve_and_read_streamed_resource_info(
                            context.filesystem,
                            workspace,
                            file.container_bundle_index,
                            &resource,
                            context.root_catalog,
                            context.provider,
                            context.content_limits,
                        )
                        .map_err(|source| {
                            UnityYamlWorkspaceExportError::StreamedResourceResolution {
                                file_index,
                                object_index: object.object_index,
                                path: resource.path.clone(),
                                source: Box::new(source),
                            }
                        })?;
                        let bytes = match resolution {
                            StreamedResourceContentResolution::Resolved { bytes, .. } => bytes,
                            StreamedResourceContentResolution::Missing {
                                unavailable_data_paths,
                            } => {
                                return Err(
                                    UnityYamlWorkspaceExportError::StreamedResourceMissing {
                                        file_index,
                                        object_index: object.object_index,
                                        path: resource.path,
                                        unavailable_data_paths,
                                    },
                                );
                            }
                            StreamedResourceContentResolution::Unset => {
                                return Err(
                                UnityYamlWorkspaceExportError::StreamedResourceUnexpectedUnset {
                                    file_index,
                                    object_index: object.object_index,
                                    path: resource.path,
                                },
                            );
                            }
                        };
                        if asset_kind == StreamedYamlAssetKind::ImageTexture && bytes.is_empty() {
                            return Ok(None);
                        }
                        let preparation = prepare_streamed_yaml_asset(
                            &mut export_root,
                            asset_kind,
                            Some(&bytes),
                            context.staging_limits,
                        )
                        .map_err(|source| {
                            UnityYamlWorkspaceExportError::StreamedAssetPreparation {
                                file_index,
                                object_index: object.object_index,
                                source: Box::new(source),
                            }
                        })?;
                        if preparation == StreamedYamlAssetPreparation::ImageTextureWithoutData {
                            return Ok(None);
                        }
                    }
                    Err(source) => {
                        return Err(UnityYamlWorkspaceExportError::StreamedAssetPreparation {
                            file_index,
                            object_index: object.object_index,
                            source: Box::new(source),
                        });
                    }
                }
                streamed_export_root = Some(export_root);
            }
        }
        let mut export_root = streamed_export_root
            .or_else(|| processed_animation_root.cloned())
            .or_else(|| generated_component_overlay.map(|overlay| overlay.payload.root.clone()));
        if let Some(pointer) = generated_prefab_internal {
            if export_root.is_none() {
                export_root = Some(payload.root.clone());
            }
            let Some(root) = export_root.as_mut() else {
                return Err(UnityYamlWorkspaceExportError::GeneratedPrefabInvariant {
                    source,
                    message: "generated Prefab pointer overlay has no object value".to_owned(),
                });
            };
            apply_generated_prefab_internal_overlay(root, pointer)?;
        }
        let mut generated_lighting_data_field_path = None;
        if let Some(pointer) = generated_lighting_data.filter(|pointer| pointer.source == source) {
            if source.class_id != 157 {
                return Err(UnityYamlWorkspaceExportError::GeneratedLightingDataInvariant {
                source,
                message: format!(
                    "planned LightingDataAsset pointer belongs to class {}, expected LightmapSettings class 157",
                    source.class_id
                ),
            });
            }
            if export_root.is_none() {
                export_root = Some(payload.root.clone());
            }
            let Some(root) = export_root.as_mut() else {
                return Err(
                    UnityYamlWorkspaceExportError::GeneratedLightingDataInvariant {
                        source,
                        message: "LightmapSettings pointer overlay has no decoded value".to_owned(),
                    },
                );
            };
            generated_lighting_data_field_path = Some(
                apply_generated_lighting_data_pointer_overlay(root, pointer)?,
            );
        }
        let export_root = export_root.as_ref().unwrap_or(&payload.root);

        let mut editor_manager_type_tree = None;
        let mut editor_manager_root = None;
        let unity_5_6_scene_object = unity_version_text
            .and_then(FactoryUnityVersion::parse)
            .is_some_and(|version| version.major == 5 && version.minor == 6);
        if ((animation_clip_editor_format && class_id == 74)
            || matches!(class_id, 104 | 157)
            || (unity_5_6_scene_object && matches!(class_id, 1 | 4 | 23 | 33 | 108)))
            && matches!(&object.factory.kind, FactoryAssetKind::Generated(_))
        {
            let unity_version_text = unity_version_text.ok_or_else(|| {
                scene_editor_format_error(
                    source,
                    class_id,
                    "source Unity version is unavailable".to_owned(),
                )
            })?;
            let factory_version =
                FactoryUnityVersion::parse(unity_version_text).ok_or_else(|| {
                    scene_editor_format_error(
                        source,
                        class_id,
                        format!("source Unity version {unity_version_text:?} is invalid"),
                    )
                })?;
            let (_, editor_tree) = reference_class_layouts()
            .tpk_trees(class_id, &factory_version)
            .map_err(|error| {
                scene_editor_format_error(
                    source,
                    class_id,
                    format!("could not load the editor TypeTree: {error}"),
                )
            })?
            .ok_or_else(|| {
                scene_editor_format_error(
                    source,
                    class_id,
                    format!(
                        "no reference editor TypeTree exists for class {class_id} at Unity {unity_version_text}"
                    ),
                )
            })?;
            let editor_type_name = editor_tree
                .nodes
                .first()
                .and_then(|node| node.type_name.as_deref())
                .ok_or_else(|| {
                    scene_editor_format_error(
                        source,
                        class_id,
                        "reference editor TypeTree has no root type name".to_owned(),
                    )
                })?;
            let source_type_name = type_tree
                .nodes
                .first()
                .and_then(|node| node.type_name.as_deref())
                .ok_or_else(|| {
                    scene_editor_format_error(
                        source,
                        class_id,
                        "source TypeTree has no root type name".to_owned(),
                    )
                })?;
            if editor_type_name != source_type_name {
                return Err(
                scene_editor_format_error(
                    source,
                    class_id,
                    format!(
                        "editor TypeTree root {editor_type_name:?} differs from source root {source_type_name:?}"
                    ),
                ),
            );
            }
            let mut root = default_value_for_type_tree_node(
                &editor_tree,
                object.object_index,
                0,
                ObjectDefaultValueLimits::default(),
            )
            .map_err(|error| {
                scene_editor_format_error(
                    source,
                    class_id,
                    format!("could not construct editor defaults: {error}"),
                )
            })?;
            merge_editor_default_value(&mut root, export_root, "")
                .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            if matches!(class_id, 104 | 157) {
                ensure_scene_manager_hide_flags(&mut root)
                    .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            if class_id == 157 {
                apply_lightmap_settings_editor_defaults(&mut root)
                    .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            if animation_clip_editor_format && class_id == 74 {
                apply_animation_clip_editor_format(
                    &mut root,
                    export_root,
                    processed_animation_root.is_some(),
                )
                .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            if matches!(class_id, 1 | 4 | 23) {
                apply_scene_object_editor_format(
                    &mut root,
                    export_root,
                    class_id,
                    source,
                    file_index,
                    object.object_index,
                    reference_graph,
                )
                .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            editor_manager_type_tree = Some(editor_tree);
            editor_manager_root = Some(root);
            if let Some(pointer) =
                generated_lighting_data.filter(|pointer| pointer.source == source)
            {
                let Some(root) = editor_manager_root.as_mut() else {
                    return Err(
                    UnityYamlWorkspaceExportError::GeneratedLightingDataInvariant {
                        source,
                        message: "editor LightmapSettings root is unavailable for the generated pointer overlay"
                            .to_owned(),
                    },
                );
                };
                generated_lighting_data_field_path = Some(
                    apply_generated_lighting_data_pointer_overlay(root, pointer)?,
                );
            }
        }
        // Keep the decoded release/generated tree distinct from the editor tree. The editor
        // format needs defaults from its TypeTree, while conversion hints for fields omitted
        // from that editor layout must fall back to the original source layout.
        let source_type_tree = type_tree;
        let type_tree = editor_manager_type_tree
            .as_ref()
            .map_or(type_tree, |tree| tree.as_ref());
        let type_root =
            type_tree
                .nodes
                .first()
                .ok_or(UnityYamlWorkspaceExportError::EmptyTypeTree {
                    file_index,
                    object_index: object.object_index,
                    type_index,
                })?;
        let export_root = editor_manager_root.as_ref().unwrap_or(export_root);
        let mut hints =
            UnityYamlObjectConversionHints::from_type_tree(type_tree, unity_version, limits)
                .map_err(UnityYamlWorkspaceExportError::Conversion)?;
        if editor_manager_type_tree.is_some() {
            let source_hints = UnityYamlObjectConversionHints::from_type_tree(
                source_type_tree,
                unity_version,
                limits,
            )
            .map_err(UnityYamlWorkspaceExportError::Conversion)?;
            merge_editor_yaml_hints_with_source_fallback(&mut hints, source_hints)
                .map_err(UnityYamlWorkspaceExportError::Conversion)?;
        }
        let generated_game_object_or_transform = matches!(class_id, 1 | 4)
            && matches!(&object.factory.kind, FactoryAssetKind::Generated(_));
        let generated_controller_member =
            matches!(class_id, 91 | 114 | 206 | 1101 | 1102 | 1107 | 1109)
                && matches!(&object.factory.kind, FactoryAssetKind::Generated(_));
        if generated_game_object_or_transform
            || generated_controller_member
            || (animation_clip_editor_format && class_id == 74)
            || (editor_manager_type_tree.is_some() && matches!(class_id, 23 | 33 | 104 | 108 | 157))
            || (generated_prefab_internal.is_some()
                && matches!(&object.factory.kind, FactoryAssetKind::Generated(_)))
        {
            // Generated source classes walk their original member names. TPK recovery has already
            // selected the class layout; YAML compatibility aliases would rename serialized fields.
            if class_id == 91 && matches!(&object.factory.kind, FactoryAssetKind::Generated(_)) {
                // AnimatorController ValueArray shapes still require these vector-field aliases.
                hints.field_name_aliases.retain(|_, alias| {
                    matches!(
                        alias.as_str(),
                        "m_PositionValues3"
                            | "m_PositionValues4"
                            | "m_ScaleValues3"
                            | "m_ScaleValues4"
                    )
                });
            } else {
                hints.field_name_aliases.clear();
            }
            hints.preserve_type_tree_field_names = true;
        }

        let mut pointer_error = None;
        let mut generated_component_pointer_seen = false;
        let mut generated_prefab_internal_pointer_seen = false;
        let mut generated_lighting_data_pointer_seen = false;
        let mut generated_lighting_data_pointer_observations = Vec::new();
        let document = serialized_object_value_to_unity_yaml_document(
            source,
            &payload.type_name,
            type_root.version,
            file_id,
            // SerializedFile's stripped bit selects a serialized type layout; the baseline YAML
            // marker comes from the owning hierarchy's StrippedAssets set, which is not wired
            // here (see `is_stripped_document` for the decision shape).
            false,
            export_root,
            &hints,
            UnityYamlObjectFieldMode::ProjectAsset,
            unity_version,
            missing_reference_guid,
            limits,
            |request| {
                if generated_lighting_data.is_some()
                    && generated_lighting_data_pointer_observations.len() < 16
                {
                    generated_lighting_data_pointer_observations.push(format!(
                        "source {:?}, field {:?}, fileID {}, PathID {}",
                        request.source, request.field_path, request.file_id, request.path_id
                    ));
                }
                if let (Some(pointer), Some(field_path)) = (
                    generated_lighting_data,
                    generated_lighting_data_field_path.as_deref(),
                ) {
                    if request.source == pointer.source && request.field_path == field_path {
                        return resolve_generated_lighting_data_pointer(
                            request,
                            pointer,
                            pointer_map,
                            current_collection_index,
                            &mut generated_lighting_data_pointer_seen,
                            &mut pointer_error,
                        );
                    }
                }
                if let Some(pointer) = generated_prefab_internal {
                    if request.source == pointer.source
                        && request.field_path.rsplit('.').next() == Some("m_PrefabInternal")
                    {
                        return resolve_generated_prefab_internal_pointer(
                            request,
                            pointer,
                            pointer_map,
                            current_collection_index,
                            &mut generated_prefab_internal_pointer_seen,
                            &mut pointer_error,
                        );
                    }
                }
                if let Some(overlay) = generated_component_overlay {
                    if request.source == overlay.pointer.source
                        && request.field_path == overlay.pointer.field_path
                    {
                        return resolve_generated_component_pointer(
                            request,
                            &overlay.pointer,
                            pointer_map,
                            current_collection_index,
                            &mut generated_component_pointer_seen,
                            &mut pointer_error,
                        );
                    }
                }
                resolve_workspace_pointer(
                    request,
                    reference_graph,
                    pointer_map,
                    current_collection_index,
                    &mut pointer_error,
                )
            },
        );
        if let Some(error) = pointer_error {
            return Err(error);
        }
        let document = document.map_err(UnityYamlWorkspaceExportError::Conversion)?;
        if generated_component_overlay.is_some() && !generated_component_pointer_seen {
            return Err(
                UnityYamlWorkspaceExportError::GeneratedComponentOverlayInvariant {
                    source,
                    message: "generated component PPtr was not visited by the YAML walker"
                        .to_owned(),
                },
            );
        }
        if generated_prefab_internal.is_some() && !generated_prefab_internal_pointer_seen {
            return Err(UnityYamlWorkspaceExportError::GeneratedPrefabInvariant {
                source,
                message: "generated m_PrefabInternal PPtr was not visited by the YAML walker"
                    .to_owned(),
            });
        }
        if generated_lighting_data_field_path.is_some() && !generated_lighting_data_pointer_seen {
            let field_path = generated_lighting_data_field_path
                .as_deref()
                .unwrap_or("<missing>");
            let root_fields = export_root
                .struct_fields()
                .map(|fields| {
                    fields
                        .iter()
                        .map(|field| field.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| "<non-struct>".to_owned());
            let pointer_value = export_root
                .unique_field_in_base_chain("m_LightingDataAsset")
                .map(|value| format!("{value:?}"))
                .unwrap_or_else(|error| format!("<lookup failed: {error}>"));
            let pointer_observation = if generated_lighting_data_pointer_observations.is_empty() {
                "no non-null class-157 PPtr was presented to the pointer resolver".to_owned()
            } else {
                generated_lighting_data_pointer_observations.join("; ")
            };
            let root_special_schemas = format!(
                "SceneObjectIdentifier={}, Hash128={}",
                hints.scene_object_identifier_schemas.contains_key(""),
                hints.hash128_schemas.contains_key("")
            );
            let lightmaps_value = export_root
                .unique_field_in_base_chain("m_Lightmaps")
                .map(|value| format!("{value:?}"))
                .unwrap_or_else(|error| format!("<lookup failed: {error}>"));
            return Err(
            UnityYamlWorkspaceExportError::GeneratedLightingDataInvariant {
                source,
                message: format!(
                    "generated m_LightingDataAsset pointer at {field_path:?} was not visited; final value: {pointer_value}; observed requests: {pointer_observation}; root schemas: {root_special_schemas}; m_Lightmaps: {lightmaps_value}; final root fields: {root_fields}"
                ),
            },
        );
        }
        Ok(Some(document))
    }
}

fn resolve_generated_component_pointer(
    request: UnityYamlPPtrRequest<'_>,
    planned: &WorkspaceGeneratedComponentYamlPointer,
    pointer_map: &ProjectExportPointerMap,
    current_collection_index: usize,
    seen: &mut bool,
    error: &mut Option<UnityYamlWorkspaceExportError>,
) -> ProjectPointerResolution {
    let invariant_error =
        |message: String| UnityYamlWorkspaceExportError::GeneratedComponentOverlayInvariant {
            source: planned.source,
            message,
        };
    if request.file_id != planned.file_id || request.path_id != planned.path_id {
        *error = Some(invariant_error(format!(
            "planned component PPtr at {:?} has FileID/PathID ({}, {}), expected ({}, {})",
            request.field_path, request.file_id, request.path_id, planned.file_id, planned.path_id
        )));
        return ProjectPointerResolution::Null;
    }
    if *seen {
        *error = Some(invariant_error(
            "planned generated component PPtr was visited more than once".to_owned(),
        ));
        return ProjectPointerResolution::Null;
    }
    *seen = true;
    match pointer_map.resolve(
        ProjectPointerRequest::ResolvedAsset(planned.target),
        current_collection_index,
    ) {
        Ok(ProjectPointerResolution::Mapped(pointer)) => ProjectPointerResolution::Mapped(pointer),
        Ok(_) => {
            *error = Some(invariant_error(
                "planned Transform has no exported project pointer".to_owned(),
            ));
            ProjectPointerResolution::Null
        }
        Err(error_source) => {
            *error = Some(UnityYamlWorkspaceExportError::PointerMap(error_source));
            ProjectPointerResolution::Null
        }
    }
}

fn resolve_generated_lighting_data_pointer(
    request: UnityYamlPPtrRequest<'_>,
    planned: &WorkspaceGeneratedLightingDataYamlPointer,
    pointer_map: &ProjectExportPointerMap,
    current_collection_index: usize,
    seen: &mut bool,
    error: &mut Option<UnityYamlWorkspaceExportError>,
) -> ProjectPointerResolution {
    let invariant_error =
        |message: String| UnityYamlWorkspaceExportError::GeneratedLightingDataInvariant {
            source: planned.source,
            message,
        };
    if request.file_id != 0 || request.path_id != planned.target.path_id {
        *error = Some(invariant_error(format!(
            "planned m_LightingDataAsset PPtr has FileID/PathID ({}, {}), expected (0, {})",
            request.file_id, request.path_id, planned.target.path_id
        )));
        return ProjectPointerResolution::Null;
    }
    if *seen {
        *error = Some(invariant_error(
            "generated m_LightingDataAsset pointer was visited more than once".to_owned(),
        ));
        return ProjectPointerResolution::Null;
    }
    *seen = true;
    match pointer_map.resolve(
        ProjectPointerRequest::ResolvedAsset(planned.target),
        current_collection_index,
    ) {
        Ok(ProjectPointerResolution::Mapped(pointer)) => ProjectPointerResolution::Mapped(pointer),
        Ok(_) => {
            *error = Some(invariant_error(
                "generated LightingDataAsset has no exported project pointer".to_owned(),
            ));
            ProjectPointerResolution::Null
        }
        Err(source) => {
            *error = Some(UnityYamlWorkspaceExportError::PointerMap(source));
            ProjectPointerResolution::Null
        }
    }
}

fn apply_generated_lighting_data_pointer_overlay(
    root: &mut SerializedValue,
    pointer: &WorkspaceGeneratedLightingDataYamlPointer,
) -> Result<String, UnityYamlWorkspaceExportError> {
    let invariant_error =
        |message: String| UnityYamlWorkspaceExportError::GeneratedLightingDataInvariant {
            source: pointer.source,
            message,
        };
    if pointer.source.class_id != 157
        || pointer.target.class_id != 1120
        || pointer.target.path_id == 0
        || !matches!(
            pointer.target.collection,
            ExportAssetCollectionIdentity::ProcessedCollection { .. }
        )
    {
        return Err(invariant_error(
            "source LightmapSettings or generated LightingDataAsset identity is invalid".to_owned(),
        ));
    }

    let canonical_path =
        base_chain_field_path(root, "m_LightingDataAsset").map_err(invariant_error)?;
    let legacy_path = base_chain_field_path(root, "m_LightmapSnapshot").map_err(invariant_error)?;
    if canonical_path.is_some() && legacy_path.is_some() {
        return Err(invariant_error(
            "LightmapSettings contains both canonical and legacy LightingDataAsset fields"
                .to_owned(),
        ));
    }
    let (field_name, field_path) = if let Some(path) = canonical_path {
        ("m_LightingDataAsset", path)
    } else if let Some(path) = legacy_path {
        ("m_LightmapSnapshot", path)
    } else {
        let SerializedValue::Struct(fields) = root else {
            return Err(invariant_error(
                "LightmapSettings root is not a struct".to_owned(),
            ));
        };
        fields.try_reserve(1).map_err(|_| {
            invariant_error("could not allocate the generated LightingDataAsset field".to_owned())
        })?;
        let insertion_index = fields
            .iter()
            .position(|field| field.name == "m_UseShadowmask")
            .unwrap_or(fields.len());
        fields.insert(
            insertion_index,
            SerializedFieldValue {
                name: "m_LightingDataAsset".to_owned(),
                value: SerializedValue::PPtr {
                    file_id: 0,
                    path_id: pointer.target.path_id,
                },
            },
        );
        return Ok("m_LightingDataAsset".to_owned());
    };

    let value = root
        .unique_field_in_base_chain_mut(field_name)
        .map_err(|error| invariant_error(error.to_string()))?
        .ok_or_else(|| invariant_error(format!("field {field_name} disappeared during overlay")))?;
    *value = SerializedValue::PPtr {
        file_id: 0,
        path_id: pointer.target.path_id,
    };
    Ok(field_path)
}

fn base_chain_field_path(
    root: &SerializedValue,
    field_name: &str,
) -> Result<Option<String>, String> {
    let mut scope = root;
    let mut base_depth = 0usize;
    loop {
        if scope
            .unique_field(field_name)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            let mut path = String::new();
            for _ in 0..base_depth {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str("Base");
            }
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(field_name);
            return Ok(Some(path));
        }
        let Some(base) = scope
            .unique_field("Base")
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        if !matches!(base, SerializedValue::Struct(_)) {
            return Ok(None);
        }
        scope = base;
        base_depth = base_depth
            .checked_add(1)
            .ok_or_else(|| "LightmapSettings Base path depth overflows".to_owned())?;
    }
}

fn apply_generated_prefab_internal_overlay(
    root: &mut SerializedValue,
    pointer: &WorkspaceGeneratedPrefabInternalPointer,
) -> Result<(), UnityYamlWorkspaceExportError> {
    let invariant_error =
        |message: String| UnityYamlWorkspaceExportError::GeneratedPrefabInvariant {
            source: pointer.source,
            message,
        };
    if matches!(
        pointer.source.collection,
        ExportAssetCollectionIdentity::ProcessedCollection { .. }
    ) || pointer.source.path_id == 0
        || pointer.target.class_id != 1001
        || pointer.target.path_id == 0
        || !matches!(
            pointer.target.collection,
            ExportAssetCollectionIdentity::ProcessedCollection { .. }
        )
    {
        return Err(invariant_error(
            "source or generated Prefab identity is invalid".to_owned(),
        ));
    }
    let value = root
        .unique_field_in_base_chain_mut("m_PrefabInternal")
        .map_err(|error| invariant_error(error.to_string()))?
        .ok_or_else(|| {
            invariant_error("hierarchy asset has no m_PrefabInternal field".to_owned())
        })?;
    let SerializedValue::PPtr { file_id, path_id } = value else {
        return Err(invariant_error(format!(
            "m_PrefabInternal has value shape {value:?} instead of PPtr"
        )));
    };
    *file_id = 0;
    *path_id = pointer.target.path_id;
    Ok(())
}

fn resolve_generated_prefab_internal_pointer(
    request: UnityYamlPPtrRequest<'_>,
    planned: &WorkspaceGeneratedPrefabInternalPointer,
    pointer_map: &ProjectExportPointerMap,
    current_collection_index: usize,
    seen: &mut bool,
    error: &mut Option<UnityYamlWorkspaceExportError>,
) -> ProjectPointerResolution {
    let invariant_error =
        |message: String| UnityYamlWorkspaceExportError::GeneratedPrefabInvariant {
            source: planned.source,
            message,
        };
    if request.source != planned.source
        || request.file_id != 0
        || request.path_id != planned.target.path_id
    {
        *error = Some(invariant_error(format!(
            "planned m_PrefabInternal pointer at {:?} has FileID/PathID ({}, {}), expected (0, {})",
            request.field_path, request.file_id, request.path_id, planned.target.path_id
        )));
        return ProjectPointerResolution::Null;
    }
    if *seen {
        *error = Some(invariant_error(
            "generated m_PrefabInternal pointer was visited more than once".to_owned(),
        ));
        return ProjectPointerResolution::Null;
    }
    *seen = true;
    match pointer_map.resolve(
        ProjectPointerRequest::ResolvedAsset(planned.target),
        current_collection_index,
    ) {
        Ok(ProjectPointerResolution::Mapped(pointer)) => ProjectPointerResolution::Mapped(pointer),
        Ok(_) => {
            *error = Some(invariant_error(
                "generated Prefab has no exported project pointer".to_owned(),
            ));
            ProjectPointerResolution::Null
        }
        Err(error_source) => {
            *error = Some(UnityYamlWorkspaceExportError::PointerMap(error_source));
            ProjectPointerResolution::Null
        }
    }
}

fn merge_editor_default_value(
    defaults: &mut SerializedValue,
    source: &SerializedValue,
    field_path: &str,
) -> Result<(), String> {
    match (defaults, source) {
        (SerializedValue::Struct(default_fields), SerializedValue::Struct(source_fields)) => {
            for source_field in source_fields {
                let child_path = if field_path.is_empty() {
                    source_field.name.clone()
                } else {
                    format!("{field_path}.{}", source_field.name)
                };
                let Some(default_index) = default_fields
                    .iter()
                    .position(|field| field.name == source_field.name)
                else {
                    // ProjectYamlWalker uses the editor-visible generated class members.
                    // A field that exists only in the release TypeTree is not part of that
                    // YAML schema (for example lightmaps moved to LightingDataAsset).
                    continue;
                };
                if default_fields
                    .iter()
                    .skip(default_index + 1)
                    .any(|field| field.name == source_field.name)
                {
                    return Err(format!("editor TypeTree has duplicate field {child_path}"));
                }
                merge_editor_default_value(
                    &mut default_fields[default_index].value,
                    &source_field.value,
                    &child_path,
                )?;
            }
            Ok(())
        }
        (default, source) => {
            *default = source.clone();
            Ok(())
        }
    }
}

fn scene_editor_format_error(
    source: ExportAssetIdentity,
    class_id: i32,
    message: String,
) -> UnityYamlWorkspaceExportError {
    if class_id == 74 {
        UnityYamlWorkspaceExportError::AnimationClipEditorFormatInvariant { source, message }
    } else if matches!(class_id, 104 | 157) {
        UnityYamlWorkspaceExportError::SceneManagerEditorFormatInvariant { source, message }
    } else {
        UnityYamlWorkspaceExportError::SceneObjectEditorFormatInvariant { source, message }
    }
}

fn apply_animation_clip_editor_format(
    editor_root: &mut SerializedValue,
    source_root: &SerializedValue,
    processed: bool,
) -> Result<(), String> {
    let settings = editor_root
        .unique_field_in_base_chain_mut("m_AnimationClipSettings")
        .map_err(|error| format!("could not inspect editor m_AnimationClipSettings: {error}"))?
        .ok_or_else(|| "editor AnimationClip has no m_AnimationClipSettings field".to_owned())?;

    set_animation_clip_editor_field(settings, "m_StopTime", SerializedValue::F32(1.0))?;
    set_animation_clip_editor_field(
        settings,
        "m_KeepOriginalPositionY",
        SerializedValue::Bool(true),
    )?;
    if !processed {
        return Ok(());
    }

    let muscle = source_root
        .unique_field_in_base_chain("m_MuscleClip")
        .map_err(|error| format!("could not inspect source m_MuscleClip: {error}"))?
        .ok_or_else(|| "processed AnimationClip has no source m_MuscleClip".to_owned())?;
    let muscle_fields = [
        (
            "m_StartTime",
            &["m_StartTime", "startTime", "StartTime"][..],
        ),
        ("m_StopTime", &["m_StopTime", "stopTime", "StopTime"][..]),
        (
            "m_OrientationOffsetY",
            &[
                "m_OrientationOffsetY",
                "orientationOffsetY",
                "OrientationOffsetY",
            ][..],
        ),
        ("m_Level", &["m_Level", "level", "Level"][..]),
        (
            "m_CycleOffset",
            &["m_CycleOffset", "cycleOffset", "CycleOffset"][..],
        ),
        ("m_LoopTime", &["m_LoopTime", "loopTime", "LoopTime"][..]),
        (
            "m_LoopBlend",
            &["m_LoopBlend", "loopBlend", "LoopBlend"][..],
        ),
        (
            "m_LoopBlendOrientation",
            &[
                "m_LoopBlendOrientation",
                "loopBlendOrientation",
                "LoopBlendOrientation",
            ][..],
        ),
        (
            "m_LoopBlendPositionY",
            &[
                "m_LoopBlendPositionY",
                "loopBlendPositionY",
                "LoopBlendPositionY",
            ][..],
        ),
        (
            "m_LoopBlendPositionXZ",
            &[
                "m_LoopBlendPositionXZ",
                "loopBlendPositionXZ",
                "LoopBlendPositionXZ",
            ][..],
        ),
        (
            "m_KeepOriginalOrientation",
            &[
                "m_KeepOriginalOrientation",
                "keepOriginalOrientation",
                "KeepOriginalOrientation",
            ][..],
        ),
        (
            "m_KeepOriginalPositionY",
            &[
                "m_KeepOriginalPositionY",
                "keepOriginalPositionY",
                "KeepOriginalPositionY",
            ][..],
        ),
        (
            "m_KeepOriginalPositionXZ",
            &[
                "m_KeepOriginalPositionXZ",
                "keepOriginalPositionXZ",
                "KeepOriginalPositionXZ",
            ][..],
        ),
        (
            "m_HeightFromFeet",
            &["m_HeightFromFeet", "heightFromFeet", "HeightFromFeet"][..],
        ),
    ];
    for (target_name, source_aliases) in muscle_fields {
        let source_value = unique_animation_clip_field(muscle, source_aliases, target_name)?;
        set_animation_clip_editor_field(settings, target_name, source_value.clone())?;
    }
    set_animation_clip_editor_field(
        settings,
        "m_AdditiveReferencePoseTime",
        SerializedValue::F32(0.0),
    )?;
    set_animation_clip_editor_field(
        settings,
        "m_HasAdditiveReferencePose",
        SerializedValue::Bool(false),
    )?;
    set_animation_clip_editor_field(settings, "m_Mirror", SerializedValue::Bool(false))?;
    Ok(())
}

fn unique_animation_clip_field<'a>(
    value: &'a SerializedValue,
    aliases: &[&str],
    field: &str,
) -> Result<&'a SerializedValue, String> {
    let mut found = None;
    for &alias in aliases {
        let candidate = value
            .unique_field_in_base_chain(alias)
            .map_err(|error| format!("could not inspect source {field}: {error}"))?;
        if let Some(candidate) = candidate {
            if found.is_some() {
                return Err(format!("source {field} has ambiguous aliases {aliases:?}"));
            }
            found = Some(candidate);
        }
    }
    found.ok_or_else(|| format!("source AnimationClip has no {field} field"))
}

fn set_animation_clip_editor_field(
    settings: &mut SerializedValue,
    field: &str,
    value: SerializedValue,
) -> Result<(), String> {
    let target = settings
        .unique_field_in_base_chain_mut(field)
        .map_err(|error| format!("could not inspect editor AnimationClip.{field}: {error}"))?
        .ok_or_else(|| format!("editor AnimationClip has no {field} field"))?;
    if std::mem::discriminant(target) != std::mem::discriminant(&value) {
        return Err(format!(
            "editor AnimationClip.{field} has value {target:?}, incompatible with source value {value:?}"
        ));
    }
    *target = value;
    Ok(())
}

fn apply_scene_object_editor_format(
    editor_root: &mut SerializedValue,
    source_root: &SerializedValue,
    class_id: i32,
    source: ExportAssetIdentity,
    file_index: usize,
    object_index: usize,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<(), String> {
    match class_id {
        1 => apply_game_object_editor_format(editor_root, source_root),
        4 => apply_transform_editor_format(
            editor_root,
            source_root,
            source,
            file_index,
            object_index,
            reference_graph,
        ),
        23 => apply_renderer_editor_format(editor_root),
        _ => Ok(()),
    }
}

fn apply_game_object_editor_format(
    editor_root: &mut SerializedValue,
    source_root: &SerializedValue,
) -> Result<(), String> {
    let tag_id = match source_root
        .unique_field_in_base_chain("m_Tag")
        .map_err(|error| format!("could not inspect GameObject.m_Tag: {error}"))?
    {
        Some(SerializedValue::I8(value)) => i32::from(*value),
        Some(SerializedValue::U8(value)) => i32::from(*value),
        Some(SerializedValue::I16(value)) => i32::from(*value),
        Some(SerializedValue::U16(value)) => i32::from(*value),
        Some(SerializedValue::I32(value)) => *value,
        Some(SerializedValue::U32(value)) => i32::try_from(*value)
            .map_err(|_| format!("GameObject.m_Tag value {value} exceeds Int32"))?,
        Some(value) => {
            return Err(format!(
                "GameObject.m_Tag has value {value:?}, expected an integer"
            ));
        }
        None => return Err("source GameObject has no m_Tag field".to_owned()),
    };
    let tag_name = match tag_id {
        0 => "Untagged".to_owned(),
        1 => "Respawn".to_owned(),
        2 => "Finish".to_owned(),
        3 => "EditorOnly".to_owned(),
        5 => "MainCamera".to_owned(),
        6 => "Player".to_owned(),
        7 => "GameController".to_owned(),
        value => format!("unknown_{value}"),
    };
    let tag_string = editor_root
        .unique_field_in_base_chain_mut("m_TagString")
        .map_err(|error| format!("could not inspect GameObject.m_TagString: {error}"))?
        .ok_or_else(|| "editor GameObject has no m_TagString field".to_owned())?;
    let SerializedValue::StringBytes(bytes) = tag_string else {
        return Err(format!(
            "editor GameObject.m_TagString has value {tag_string:?}, expected a string"
        ));
    };
    bytes.clear();
    bytes
        .try_reserve_exact(tag_name.len())
        .map_err(|_| "could not allocate GameObject.m_TagString".to_owned())?;
    bytes.extend_from_slice(tag_name.as_bytes());
    Ok(())
}

fn apply_transform_editor_format(
    editor_root: &mut SerializedValue,
    source_root: &SerializedValue,
    source: ExportAssetIdentity,
    file_index: usize,
    object_index: usize,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<(), String> {
    if editor_root
        .unique_field_in_base_chain("m_RootOrder")
        .map_err(|error| format!("could not inspect Transform.m_RootOrder: {error}"))?
        .is_some()
    {
        let root_order_value = calculate_scene_transform_root_order(
            source_root,
            source,
            file_index,
            object_index,
            reference_graph,
        )?;
        let root_order = editor_root
            .unique_field_in_base_chain_mut("m_RootOrder")
            .map_err(|error| format!("could not update Transform.m_RootOrder: {error}"))?
            .ok_or_else(|| "editor Transform lost m_RootOrder".to_owned())?;
        let SerializedValue::I32(value) = root_order else {
            return Err(format!(
                "editor Transform.m_RootOrder has value {root_order:?}, expected Int32"
            ));
        };
        *value = root_order_value;
    }

    if editor_root
        .unique_field_in_base_chain("m_LocalEulerAnglesHint")
        .map_err(|error| format!("could not inspect Transform.m_LocalEulerAnglesHint: {error}"))?
        .is_some()
    {
        let rotation = source_root
            .unique_field_in_base_chain("m_LocalRotation")
            .map_err(|error| format!("could not inspect Transform.m_LocalRotation: {error}"))?
            .ok_or_else(|| "source Transform has no m_LocalRotation field".to_owned())?;
        let rotation = read_scene_quaternion(rotation, "Transform.m_LocalRotation")?;
        let euler = crate::resources::quaternion::euler_hint_from_rotation(rotation);
        set_scene_vector3(
            editor_root,
            "m_LocalEulerAnglesHint",
            euler,
            "Transform.m_LocalEulerAnglesHint",
        )?;
    }
    Ok(())
}

fn calculate_scene_transform_root_order(
    source_root: &SerializedValue,
    source: ExportAssetIdentity,
    file_index: usize,
    object_index: usize,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<i32, String> {
    let (file_id, path_id) = match source_root
        .unique_field_in_base_chain("m_Father")
        .map_err(|error| format!("could not inspect Transform.m_Father: {error}"))?
    {
        Some(SerializedValue::PPtr { file_id, path_id }) => (*file_id, *path_id),
        Some(value) => {
            return Err(format!(
                "Transform.m_Father has value {value:?}, expected a PPtr"
            ));
        }
        None => return Err("source Transform has no m_Father field".to_owned()),
    };
    if path_id == 0 {
        return Ok(0);
    }

    let mut father_edges = reference_graph
        .references_from_object(file_index, object_index)
        .filter(|edge| {
            edge.field_path.rsplit('.').next() == Some("m_Father")
                && edge.file_id == file_id
                && edge.path_id == path_id
        });
    let father_edge = father_edges
        .next()
        .ok_or_else(|| "non-null Transform.m_Father has no reference graph edge".to_owned())?;
    if father_edges.next().is_some() {
        return Err("Transform.m_Father has duplicate reference graph edges".to_owned());
    }
    let (parent_file_index, parent_object_index) = match &father_edge.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => (*target_file_index, *target_object_index),
        resolution => {
            return Err(format!(
                "non-null Transform.m_Father resolves as {resolution:?}"
            ));
        }
    };
    if reference_graph.object_class_id(parent_file_index, parent_object_index) != Some(4) {
        return Err("Transform.m_Father does not resolve to a Transform object".to_owned());
    }
    if reference_graph.object_scan_is_complete(parent_file_index, parent_object_index) != Some(true)
    {
        return Err("parent Transform reference scan is incomplete".to_owned());
    }

    let mut root_order = None::<usize>;
    for edge in reference_graph.references_from_object(parent_file_index, parent_object_index) {
        let Some(child_index) = scene_transform_child_index(edge.field_path)? else {
            continue;
        };
        if matches!(
            &edge.resolution,
            MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index,
            } if *target_file_index == file_index && *target_object_index == object_index
        ) {
            if root_order.is_some_and(|previous| previous == child_index) {
                return Err(format!(
                    "parent Transform has duplicate child reference at index {child_index}"
                ));
            }
            root_order = Some(root_order.map_or(child_index, |previous| previous.max(child_index)));
        }
    }
    let root_order = root_order.ok_or_else(|| {
        format!("Transform {source:?} was not found among its father's serialized children")
    })?;
    i32::try_from(root_order)
        .map_err(|_| format!("Transform root order {root_order} exceeds Int32"))
}

fn scene_transform_child_index(field_path: &str) -> Result<Option<usize>, String> {
    let leaf = field_path.rsplit('.').next().unwrap_or(field_path);
    let Some(index) = leaf.strip_prefix("m_Children[") else {
        return Ok(None);
    };
    let index = index
        .strip_suffix(']')
        .ok_or_else(|| format!("malformed Transform child field path {field_path:?}"))?;
    let index = index
        .parse::<usize>()
        .map_err(|_| format!("malformed Transform child index in {field_path:?}"))?;
    Ok(Some(index))
}

fn read_scene_quaternion(value: &SerializedValue, context: &str) -> Result<[f32; 4], String> {
    let SerializedValue::Struct(fields) = value else {
        return Err(format!("{context} has value {value:?}, expected a struct"));
    };
    let mut components = [0.0; 4];
    for (index, name) in ["x", "y", "z", "w"].into_iter().enumerate() {
        let mut matching = fields.iter().filter(|field| field.name == name);
        let field = matching
            .next()
            .ok_or_else(|| format!("{context} has no {name} component"))?;
        if matching.next().is_some() {
            return Err(format!("{context} has duplicate {name} components"));
        }
        let SerializedValue::F32(value) = &field.value else {
            return Err(format!("{context}.{name} is not Float32"));
        };
        components[index] = *value;
    }
    Ok(components)
}

fn set_scene_vector3(
    root: &mut SerializedValue,
    field_name: &str,
    components: [f32; 3],
    context: &str,
) -> Result<(), String> {
    let value = root
        .unique_field_in_base_chain_mut(field_name)
        .map_err(|error| format!("could not inspect {context}: {error}"))?
        .ok_or_else(|| format!("editor object has no {context} field"))?;
    let SerializedValue::Struct(fields) = value else {
        return Err(format!("{context} has value {value:?}, expected a struct"));
    };
    for (name, component) in ["x", "y", "z"].into_iter().zip(components) {
        let mut matching = fields.iter_mut().filter(|field| field.name == name);
        let field = matching
            .next()
            .ok_or_else(|| format!("{context} has no {name} component"))?;
        if matching.next().is_some() {
            return Err(format!("{context} has duplicate {name} components"));
        }
        let SerializedValue::F32(value) = &mut field.value else {
            return Err(format!("{context}.{name} is not Float32"));
        };
        *value = component;
    }
    Ok(())
}

fn apply_renderer_editor_format(root: &mut SerializedValue) -> Result<(), String> {
    const DEFAULTS: &[(&str, SceneEditorFormatDefault)] = &[
        ("m_ScaleInLightmap", SceneEditorFormatDefault::F32(1.0)),
        ("m_ReceiveGI", SceneEditorFormatDefault::I32(1)),
        ("m_PreserveUVs", SceneEditorFormatDefault::Bool(false)),
        (
            "m_IgnoreNormalsForChartDetection",
            SceneEditorFormatDefault::Bool(false),
        ),
        ("m_ImportantGI", SceneEditorFormatDefault::Bool(false)),
        (
            "m_StitchLightmapSeams",
            SceneEditorFormatDefault::Bool(false),
        ),
        (
            "m_SelectedEditorRenderState",
            SceneEditorFormatDefault::I32(3),
        ),
        ("m_MinimumChartSize", SceneEditorFormatDefault::I32(4)),
        ("m_AutoUVMaxDistance", SceneEditorFormatDefault::F32(0.5)),
        ("m_AutoUVMaxAngle", SceneEditorFormatDefault::F32(89.0)),
    ];
    for (field, default) in DEFAULTS {
        set_scene_editor_format_default(root, field, *default)?;
    }
    if let Some(lightmap_parameters) = root
        .unique_field_in_base_chain_mut("m_LightmapParameters")
        .map_err(|error| format!("could not inspect m_LightmapParameters: {error}"))?
    {
        let SerializedValue::PPtr { file_id, path_id } = lightmap_parameters else {
            return Err(format!(
                "m_LightmapParameters has value {lightmap_parameters:?}, expected a PPtr"
            ));
        };
        *file_id = 0;
        *path_id = 0;
    }
    Ok(())
}

fn merge_editor_yaml_hints_with_source_fallback(
    editor: &mut UnityYamlObjectConversionHints,
    source: UnityYamlObjectConversionHints,
) -> Result<(), UnityYamlObjectConversionError> {
    macro_rules! merge_map {
        ($field:ident, $resource:literal) => {{
            let requested = source.$field.len();
            editor.$field.try_reserve(requested).map_err(|_| {
                UnityYamlObjectConversionError::AllocationFailed {
                    resource: $resource,
                    requested,
                }
            })?;
            for (path, value) in source.$field {
                editor.$field.entry(path).or_insert(value);
            }
        }};
    }
    macro_rules! merge_set {
        ($field:ident, $resource:literal) => {{
            let requested = source.$field.len();
            editor.$field.try_reserve(requested).map_err(|_| {
                UnityYamlObjectConversionError::AllocationFailed {
                    resource: $resource,
                    requested,
                }
            })?;
            editor.$field.extend(source.$field);
        }};
    }

    merge_map!(
        empty_array_kinds,
        "Unity YAML editor empty-array source fallback"
    );
    merge_map!(
        serialized_version_paths,
        "Unity YAML editor serialized-version source fallback"
    );
    merge_set!(
        flow_mapped_struct_paths,
        "Unity YAML editor flow-mapping source fallback"
    );
    merge_set!(
        force_block_sequence_paths,
        "Unity YAML editor block-sequence source fallback"
    );
    merge_set!(
        importer_excluded_field_paths,
        "Unity YAML editor importer-field source fallback"
    );
    merge_map!(
        dictionary_representations,
        "Unity YAML editor dictionary source fallback"
    );
    merge_map!(hash128_schemas, "Unity YAML editor Hash128 source fallback");
    merge_map!(guid_schemas, "Unity YAML editor GUID source fallback");
    merge_map!(
        property_name_schemas,
        "Unity YAML editor PropertyName source fallback"
    );
    merge_map!(
        scene_object_identifier_schemas,
        "Unity YAML editor SceneObjectIdentifier source fallback"
    );
    merge_map!(
        field_name_aliases,
        "Unity YAML editor field-alias source fallback"
    );
    Ok(())
}

fn ensure_scene_manager_hide_flags(root: &mut SerializedValue) -> Result<(), String> {
    match root.unique_field_in_base_chain_mut("m_ObjectHideFlags") {
        Ok(Some(value)) => match value {
            SerializedValue::U32(_) => {
                *value = SerializedValue::U32(0);
                return Ok(());
            }
            other => {
                return Err(format!(
                    "m_ObjectHideFlags has value {other:?}, expected UInt32"
                ));
            }
        },
        Ok(None) => {}
        Err(error) => return Err(format!("could not inspect m_ObjectHideFlags: {error}")),
    }
    let SerializedValue::Struct(fields) = root else {
        return Err("Scene manager editor default root is not a struct".to_owned());
    };
    fields
        .try_reserve(1)
        .map_err(|_| "could not allocate Scene manager object base field".to_owned())?;
    fields.insert(
        0,
        SerializedFieldValue {
            name: "m_ObjectHideFlags".to_owned(),
            value: SerializedValue::U32(0),
        },
    );
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum SceneEditorFormatDefault {
    Bool(bool),
    I32(i32),
    F32(f32),
    String(&'static str),
}

fn apply_lightmap_settings_editor_defaults(root: &mut SerializedValue) -> Result<(), String> {
    set_scene_editor_format_default(root, "m_GIWorkflowMode", SceneEditorFormatDefault::I32(1))?;
    const DEFAULTS: &[(&str, SceneEditorFormatDefault)] = &[
        ("m_AO", SceneEditorFormatDefault::Bool(false)),
        ("m_AOAmount", SceneEditorFormatDefault::I32(0)),
        ("m_AOContrast", SceneEditorFormatDefault::F32(1.0)),
        ("m_AOMaxDistance", SceneEditorFormatDefault::F32(1.0)),
        ("m_AtlasSize", SceneEditorFormatDefault::I32(1024)),
        ("m_BakeBackend", SceneEditorFormatDefault::I32(0)),
        ("m_BakeResolution", SceneEditorFormatDefault::F32(40.0)),
        ("m_BounceBoost", SceneEditorFormatDefault::F32(1.0)),
        ("m_BounceIntensity", SceneEditorFormatDefault::F32(1.0)),
        ("m_Bounces", SceneEditorFormatDefault::I32(1)),
        ("m_CompAOExponent", SceneEditorFormatDefault::F32(1.0)),
        ("m_CompAOExponentDirect", SceneEditorFormatDefault::F32(0.0)),
        (
            "m_DirectLightInLightProbes",
            SceneEditorFormatDefault::Bool(true),
        ),
        (
            "m_ExportTrainingData",
            SceneEditorFormatDefault::Bool(false),
        ),
        (
            "m_ExtractAmbientOcclusion",
            SceneEditorFormatDefault::Bool(false),
        ),
        ("m_ExtractAO", SceneEditorFormatDefault::Bool(false)),
        ("m_FinalGather", SceneEditorFormatDefault::Bool(false)),
        (
            "m_FinalGatherContrastThreshold",
            SceneEditorFormatDefault::F32(0.05),
        ),
        (
            "m_FinalGatherFiltering",
            SceneEditorFormatDefault::Bool(true),
        ),
        (
            "m_FinalGatherGradientThreshold",
            SceneEditorFormatDefault::F32(0.0),
        ),
        (
            "m_FinalGatherInterpolationPoints",
            SceneEditorFormatDefault::I32(15),
        ),
        ("m_FinalGatherRayCount", SceneEditorFormatDefault::I32(256)),
        ("m_FinalGatherRays", SceneEditorFormatDefault::I32(1000)),
        ("m_LastUsedResolution", SceneEditorFormatDefault::F32(0.0)),
        ("m_Resolution", SceneEditorFormatDefault::F32(2.0)),
        ("m_LightmapsBakeMode", SceneEditorFormatDefault::I32(1)),
        (
            "m_LightProbeSampleCountMultiplier",
            SceneEditorFormatDefault::F32(4.0),
        ),
        ("m_LockAtlas", SceneEditorFormatDefault::Bool(false)),
        (
            "m_LODSurfaceMappingDistance",
            SceneEditorFormatDefault::F32(1.0),
        ),
        ("m_MixedBakeMode", SceneEditorFormatDefault::I32(2)),
        ("m_Padding", SceneEditorFormatDefault::I32(2)),
        ("m_PVRBounces", SceneEditorFormatDefault::I32(2)),
        ("m_PVRCulling", SceneEditorFormatDefault::Bool(true)),
        ("m_PVRDenoiserTypeAO", SceneEditorFormatDefault::I32(1)),
        ("m_PVRDenoiserTypeDirect", SceneEditorFormatDefault::I32(1)),
        (
            "m_PVRDenoiserTypeIndirect",
            SceneEditorFormatDefault::I32(1),
        ),
        ("m_PVRDirectSampleCount", SceneEditorFormatDefault::I32(32)),
        ("m_PVREnvironmentMIS", SceneEditorFormatDefault::I32(1)),
        (
            "m_PVREnvironmentImportanceSampling",
            SceneEditorFormatDefault::Bool(true),
        ),
        (
            "m_PVREnvironmentReferencePointCount",
            SceneEditorFormatDefault::I32(2048),
        ),
        (
            "m_PVREnvironmentSampleCount",
            SceneEditorFormatDefault::I32(256),
        ),
        (
            "m_PVRFilteringAtrousColorSigma",
            SceneEditorFormatDefault::F32(1.0),
        ),
        (
            "m_PVRFilteringAtrousNormalSigma",
            SceneEditorFormatDefault::F32(1.0),
        ),
        (
            "m_PVRFilteringAtrousPositionSigma",
            SceneEditorFormatDefault::F32(1.0),
        ),
        (
            "m_PVRFilteringAtrousPositionSigmaAO",
            SceneEditorFormatDefault::F32(1.0),
        ),
        (
            "m_PVRFilteringAtrousPositionSigmaDirect",
            SceneEditorFormatDefault::F32(0.5),
        ),
        (
            "m_PVRFilteringAtrousPositionSigmaIndirect",
            SceneEditorFormatDefault::F32(2.0),
        ),
        (
            "m_PVRFilteringGaussRadiusAO",
            SceneEditorFormatDefault::I32(2),
        ),
        (
            "m_PVRFilteringGaussRadiusAO_Int32",
            SceneEditorFormatDefault::I32(2),
        ),
        (
            "m_PVRFilteringGaussRadiusAO_Single",
            SceneEditorFormatDefault::F32(2.0),
        ),
        (
            "m_PVRFilteringGaussRadiusDirect",
            SceneEditorFormatDefault::I32(1),
        ),
        (
            "m_PVRFilteringGaussRadiusDirect_Int32",
            SceneEditorFormatDefault::I32(1),
        ),
        (
            "m_PVRFilteringGaussRadiusDirect_Single",
            SceneEditorFormatDefault::F32(1.0),
        ),
        (
            "m_PVRFilteringGaussRadiusIndirect",
            SceneEditorFormatDefault::I32(5),
        ),
        (
            "m_PVRFilteringGaussRadiusIndirect_Int32",
            SceneEditorFormatDefault::I32(5),
        ),
        (
            "m_PVRFilteringGaussRadiusIndirect_Single",
            SceneEditorFormatDefault::F32(5.0),
        ),
        ("m_PVRFilteringMode", SceneEditorFormatDefault::I32(1)),
        ("m_PVRFilterTypeAO", SceneEditorFormatDefault::I32(0)),
        ("m_PVRFilterTypeDirect", SceneEditorFormatDefault::I32(0)),
        ("m_PVRFilterTypeIndirect", SceneEditorFormatDefault::I32(0)),
        ("m_PVRSampleCount", SceneEditorFormatDefault::I32(500)),
        ("m_PVRSampling", SceneEditorFormatDefault::I32(1)),
        ("m_ReflectionCompression", SceneEditorFormatDefault::I32(2)),
        (
            "m_ShowResolutionOverlay",
            SceneEditorFormatDefault::Bool(true),
        ),
        ("m_SkyLightIntensity", SceneEditorFormatDefault::F32(0.0)),
        ("m_StationaryBakeMode", SceneEditorFormatDefault::I32(1)),
        ("m_TextureCompression", SceneEditorFormatDefault::Bool(true)),
        ("m_TextureHeight", SceneEditorFormatDefault::I32(1024)),
        ("m_TextureWidth", SceneEditorFormatDefault::I32(1024)),
        (
            "m_TrainingDataDestination",
            SceneEditorFormatDefault::String("TrainingData"),
        ),
    ];
    let Some(editor_settings) = root
        .unique_field_in_base_chain_mut("m_LightmapEditorSettings")
        .map_err(|error| format!("could not inspect m_LightmapEditorSettings: {error}"))?
    else {
        return Ok(());
    };
    if !matches!(editor_settings, SerializedValue::Struct(_)) {
        return Err(format!(
            "m_LightmapEditorSettings has value {editor_settings:?}, expected a struct"
        ));
    }
    for (field, value) in DEFAULTS {
        set_scene_editor_format_default(editor_settings, field, *value)?;
    }
    set_scene_editor_color_default(editor_settings, "m_SkyLightColor", [0.86, 0.93, 1.0, 1.0])?;
    Ok(())
}

fn set_scene_editor_format_default(
    root: &mut SerializedValue,
    field_name: &str,
    default: SceneEditorFormatDefault,
) -> Result<(), String> {
    let value = match root.unique_field_in_base_chain_mut(field_name) {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(()),
        Err(error) => return Err(format!("could not inspect {field_name}: {error}")),
    };
    match (value, default) {
        (SerializedValue::Bool(value), SceneEditorFormatDefault::Bool(default)) => {
            *value = default;
        }
        (SerializedValue::I32(value), SceneEditorFormatDefault::I32(default)) => {
            *value = default;
        }
        (SerializedValue::F32(value), SceneEditorFormatDefault::F32(default)) => {
            *value = default;
        }
        (SerializedValue::StringBytes(value), SceneEditorFormatDefault::String(default)) => {
            value.clear();
            value
                .try_reserve_exact(default.len())
                .map_err(|_| format!("could not allocate editor default for {field_name}"))?;
            value.extend_from_slice(default.as_bytes());
        }
        (value, default) => {
            return Err(format!(
                "editor default for {field_name} has incompatible value shape {value:?} vs {default:?}"
            ));
        }
    }
    Ok(())
}

fn set_scene_editor_color_default(
    root: &mut SerializedValue,
    field_name: &str,
    components: [f32; 4],
) -> Result<(), String> {
    let Some(value) = root
        .unique_field_in_base_chain_mut(field_name)
        .map_err(|error| format!("could not inspect {field_name}: {error}"))?
    else {
        return Ok(());
    };
    let SerializedValue::Struct(fields) = value else {
        return Err(format!("editor default {field_name} is not a struct"));
    };
    for (name, component) in ["r", "g", "b", "a"].into_iter().zip(components) {
        let Some(value) = fields
            .iter_mut()
            .find(|field| field.name == name)
            .map(|field| &mut field.value)
        else {
            return Err(format!("editor default {field_name} lacks {name}"));
        };
        let SerializedValue::F32(value) = value else {
            return Err(format!("editor default {field_name}.{name} is not Float32"));
        };
        *value = component;
    }
    Ok(())
}

fn material_shader_pointer(mut pointer: UnityMetaPointer) -> UnityMetaPointer {
    // Project Shader assets use Meta pointers; predefined engine assets keep type 0.
    if pointer.asset_type != UnityProjectAssetType::Internal {
        pointer.asset_type = UnityProjectAssetType::Meta;
    }
    pointer
}

fn resolve_workspace_pointer(
    request: UnityYamlPPtrRequest<'_>,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    current_collection_index: usize,
    error: &mut Option<UnityYamlWorkspaceExportError>,
) -> ProjectPointerResolution {
    let ExportAssetCollectionIdentity::SerializedFile { file_index } = request.source.collection
    else {
        *error = Some(UnityYamlWorkspaceExportError::ProcessedObjectIdentity {
            source: request.source,
        });
        return ProjectPointerResolution::Null;
    };
    let mut matching_edges = reference_graph
        .references_from_object(file_index, request.source.object_index)
        .filter(|edge| {
            edge.field_path == request.field_path
                && edge.file_id == request.file_id
                && edge.path_id == request.path_id
        });
    let edge = if let Some(edge) = matching_edges.next() {
        if matching_edges.next().is_some() {
            *error = Some(UnityYamlWorkspaceExportError::ReferenceEdgeAmbiguous {
                file_index,
                object_index: request.source.object_index,
                field_path: request.field_path.to_owned(),
                file_id: request.file_id,
                path_id: request.path_id,
            });
            return ProjectPointerResolution::Null;
        }
        edge
    } else {
        // AnimationClip conversion appends PPtr keys whose paths did not exist in the scanned
        // source TypeTree. Resolve them only through an identical serialized pointer already
        // present in this same object, and only when every matching source edge agrees.
        let mut pair_matches = reference_graph
            .references_from_object(file_index, request.source.object_index)
            .filter(|edge| edge.file_id == request.file_id && edge.path_id == request.path_id);
        let Some(edge) = pair_matches.next() else {
            *error = Some(UnityYamlWorkspaceExportError::ReferenceEdgeMissing {
                file_index,
                object_index: request.source.object_index,
                field_path: request.field_path.to_owned(),
                file_id: request.file_id,
                path_id: request.path_id,
            });
            return ProjectPointerResolution::Null;
        };
        if pair_matches.any(|candidate| candidate.resolution != edge.resolution) {
            *error = Some(UnityYamlWorkspaceExportError::ReferenceEdgeAmbiguous {
                file_index,
                object_index: request.source.object_index,
                field_path: request.field_path.to_owned(),
                file_id: request.file_id,
                path_id: request.path_id,
            });
            return ProjectPointerResolution::Null;
        }
        edge
    };

    let pointer_request = match &edge.resolution {
        MultiFilePPtrResolution::Null => {
            *error = Some(
                UnityYamlWorkspaceExportError::NonNullPointerResolvedAsNull {
                    file_index,
                    object_index: request.source.object_index,
                    field_path: request.field_path.to_owned(),
                    path_id: request.path_id,
                },
            );
            return ProjectPointerResolution::Null;
        }
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => {
            let Some(identity) =
                reference_graph.object_identity(*target_file_index, *target_object_index)
            else {
                *error = Some(
                    UnityYamlWorkspaceExportError::ReferenceTargetIdentityMissing {
                        file_index: *target_file_index,
                        object_index: *target_object_index,
                    },
                );
                return ProjectPointerResolution::Null;
            };
            let Some(class_id) =
                reference_graph.object_class_id(*target_file_index, *target_object_index)
            else {
                *error = Some(
                    UnityYamlWorkspaceExportError::ReferenceTargetClassIdMissing {
                        file_index: *target_file_index,
                        object_index: *target_object_index,
                    },
                );
                return ProjectPointerResolution::Null;
            };
            ProjectPointerRequest::ResolvedAsset(ExportAssetIdentity {
                collection: ExportAssetCollectionIdentity::SerializedFile {
                    file_index: *target_file_index,
                },
                object_index: *target_object_index,
                path_id: identity.path_id,
                class_id,
            })
        }
        MultiFilePPtrResolution::PredefinedEngineAsset {
            resource, path_id, ..
        } => ProjectPointerRequest::PredefinedEngineAsset {
            path_id: *path_id,
            resource: *resource,
        },
        resolution => ProjectPointerRequest::UnresolvedInput(UnresolvedProjectPointer {
            source_file_index: file_index,
            source_object_index: request.source.object_index,
            source_path_id: request.source.path_id,
            field_path: request.field_path.to_owned(),
            serialized_file_id: request.file_id,
            serialized_path_id: request.path_id,
            expected_class_id: None,
            resolution: resolution.clone(),
        }),
    };
    match pointer_map.resolve(pointer_request, current_collection_index) {
        Ok(ProjectPointerResolution::Mapped(pointer))
            if request.source.class_id == 21
                && request.field_path.rsplit('.').next() == Some("m_Shader") =>
        {
            ProjectPointerResolution::Mapped(material_shader_pointer(pointer))
        }
        Ok(resolution) => resolution,
        Err(source) => {
            *error = Some(UnityYamlWorkspaceExportError::PointerMap(source));
            ProjectPointerResolution::Null
        }
    }
}

/// Convert one workspace object using the established positional export entry point.
#[allow(clippy::too_many_arguments)]
pub fn workspace_object_to_unity_yaml_document(
    workspace: &SerializedFileWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    object: &WorkspaceObject,
    source: ExportAssetIdentity,
    file_id: i64,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
) -> Result<UnityYamlDocument, UnityYamlWorkspaceExportError> {
    let context = WorkspaceYamlExportContext::new(
        workspace,
        reference_graph,
        pointer_map,
        current_collection_index,
        missing_reference_guid,
        limits,
    );
    let Some(document) = context.convert(WorkspaceYamlObjectConversionRequest {
        object,
        source,
        file_id,
        processed_animation_root: None,
        animation_clip_editor_format: false,
        generated_component_overlay: None,
        generated_prefab_internal: None,
        generated_lighting_data: None,
    })?
    else {
        return Err(UnityYamlWorkspaceExportError::ImageTextureSkippedWithoutContent { source });
    };
    Ok(document)
}

/// Convert one workspace object while resolving streamed data for Mesh and image-texture YAML.
/// Returns `None` when the baseline exporter skips an image texture with no usable data.
#[allow(clippy::too_many_arguments)]
pub fn workspace_object_to_unity_yaml_document_with_streamed_resources(
    workspace: &SerializedFileWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    object: &WorkspaceObject,
    source: ExportAssetIdentity,
    file_id: i64,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    streamed_resources: &WorkspaceStreamedYamlResourceContext<'_, '_>,
) -> Result<Option<UnityYamlDocument>, UnityYamlWorkspaceExportError> {
    workspace_object_to_unity_yaml_document_with_processed_animation_root(
        workspace,
        reference_graph,
        pointer_map,
        object,
        source,
        file_id,
        current_collection_index,
        missing_reference_guid,
        limits,
        None,
        streamed_resources,
        false,
    )
}

/// Convert one workspace object using an already processed AnimationClip root when present.
/// Other object families retain the same streamed-resource conversion behavior.
#[allow(clippy::too_many_arguments)]
pub fn workspace_object_to_unity_yaml_document_with_processed_animation_root(
    workspace: &SerializedFileWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    object: &WorkspaceObject,
    source: ExportAssetIdentity,
    file_id: i64,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    processed_animation_root: Option<&SerializedValue>,
    streamed_resources: &WorkspaceStreamedYamlResourceContext<'_, '_>,
    animation_clip_editor_format: bool,
) -> Result<Option<UnityYamlDocument>, UnityYamlWorkspaceExportError> {
    WorkspaceYamlExportContext::new(
        workspace,
        reference_graph,
        pointer_map,
        current_collection_index,
        missing_reference_guid,
        limits,
    )
    .with_streamed_resources(streamed_resources)
    .convert(WorkspaceYamlObjectConversionRequest {
        object,
        source,
        file_id,
        processed_animation_root,
        animation_clip_editor_format,
        generated_component_overlay: None,
        generated_prefab_internal: None,
        generated_lighting_data: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::pointer_map::ExportPointerGuid;

    #[test]
    fn material_shader_pointer_preserves_predefined_internal_asset_type() {
        let internal = UnityMetaPointer {
            file_id: 10755,
            guid: ExportPointerGuid::Collection([0xF0; 16]),
            asset_type: UnityProjectAssetType::Internal,
        };
        assert_eq!(material_shader_pointer(internal), internal);

        let project_asset = UnityMetaPointer {
            asset_type: UnityProjectAssetType::Serialized,
            ..internal
        };
        assert_eq!(
            material_shader_pointer(project_asset).asset_type,
            UnityProjectAssetType::Meta
        );
    }
}
