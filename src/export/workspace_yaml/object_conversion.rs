use std::str;

use crate::processing::asset_factory::{
    reference_class_layouts, ClassLayoutSource, FactoryAssetKind, FactoryUnityVersion,
};
use crate::processing::scene_naming::SceneUnityVersion;
use crate::resource::{resolve_and_read_streamed_resource_info, StreamedResourceContentResolution};
use crate::serialized_file::metadata::TypeTree;
use crate::serialized_file::object::{
    default_value_for_type_tree_node, ObjectDefaultValueLimits, SerializedFieldValue,
    SerializedValue,
};
use crate::serialized_file::object_workspace::{ObjectWorkspace, WorkspaceObject};
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
use super::super::unity_yaml::{
    UnityYamlDocument, UnityYamlNode, UnityYamlNodeKind, UnityYamlScalar,
};
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
use super::sprite_atlas::{
    field_path_in_base_chain, unique_reference_for_field, WorkspaceSpriteAtlasPointer,
    WorkspaceSpriteAtlasYamlOverlay,
};

fn should_use_scene_editor_type_tree(class_id: i32, unity_version_text: Option<&str>) -> bool {
    let Some(version) = unity_version_text.and_then(FactoryUnityVersion::parse) else {
        return false;
    };
    let supported_version =
        (version.major == 5 && version.minor == 6) || (version.major == 2022 && version.minor == 3);
    let supported_class = matches!(class_id, 1 | 4 | 23 | 28 | 33 | 108)
        || (version.major == 2022
            && version.minor == 3
            && matches!(
                class_id,
                20 | 43
                    | 86
                    | 121
                    | 64
                    | 65
                    | 28
                    | 72
                    | 81
                    | 136
                    | 193
                    | 196
                    | 205
                    | 222
                    | 223
                    | 224
                    | 320
                    | 90
                    | 82
                    | 95
                    | 213
                    | 135
                    | 137
                    | 319
                    | 198
                    | 199
                    // 云栖仙府 breadth: these carry full editor base fields on
                    // 2022.3 (probed TPK base for each) but exported release-shaped.
                    | 54
                    | 84
                    | 96
                    | 153
                    // Breadth probe: LineRenderer and SpriteRenderer are IRenderers
                    // with full editor base on 2022.3 (probed); same truncation.
                    | 120
                    | 212
                    // Breadth probe: the Animation component carries full
                    // editor base on 2022.3 (probed); same truncation.
                    | 111
                    // Breadth probe: LensFlare and FlareLayer carry full editor
                    // base on 2022.3 (probed); same truncation, no extra format.
                    | 123
                    | 124
                    | 850_595_691
                    | 895_512_359
                    | 1_773_428_102
                    | 1_818_360_608
                    | 1_818_360_609
                    | 258
                    // Breadth probe: Terrain carries full editor base plus its
                    // GI fields on 2022.3 (probed); same release truncation.
                    | 218
                    // Breadth probe: TerrainCollider, TerrainData, and the
                    // 220 light-probe group carry full editor base on 2022.3
                    // (probed); same release truncation, no extra format.
                    | 154
                    | 156
                    | 220
                    // Breadth probe: PhysicMaterial carries full editor base
                    // on 2022.3 (probed); same release truncation.
                    | 134
                    // Breadth probe: TextMesh, class 215, and CanvasGroup
                    // carry full editor base on 2022.3 (probed); same release
                    // truncation, no extra format. The 215 editor tree also
                    // omits the baked texture.
                    | 102
                    | 215
                    | 225
                    // Breadth probe (fourth world): these classes carry the
                    // full editor base on 2022.3 (4/4 of m_ObjectHideFlags,
                    // m_CorrespondingSourceObject, m_PrefabInstance and
                    // m_PrefabAsset present in the editor tree and absent from
                    // the release tree) but were exported release-shaped:
                    // OcclusionCullingSettings (whose editor tree also adds
                    // m_OcclusionBakeSettings), Projector, Font,
                    // WheelCollider, AudioLowPassFilter, WindZone,
                    // NavMeshAgent, AnimatorOverrideController, NavMeshData,
                    // OcclusionCullingData (whose editor tree also adds
                    // m_StaticRenderers and m_Portals), LookAtConstraint and
                    // ScaleConstraint (whose editor trees also add
                    // m_IsLocked).
                    | 29
                    | 119
                    | 128
                    | 146
                    | 169
                    | 182
                    | 195
                    | 221
                    | 238
                    | 363
                    // Breadth probe (seventh world): AudioReverbZone carries the
                    // full editor base on 2022.3 (probed) but was exported
                    // release-shaped, which costs the four base fields.
                    | 167
                    // Breadth probe (ninth world): AudioReverbFilter and
                    // LightProbeProxyVolume carry the full editor base on 2022.3
                    // (probed) but were exported release-shaped; the TerrainLayer
                    // custom class id behaves the same way.
                    | 164
                    | 259
                    | 1_953_259_897
                    | 1_183_024_399
                    | 1_818_360_610
            ));
    supported_version && supported_class
}

fn should_preserve_type_tree_field_names(
    class_id: i32,
    factory_kind: &FactoryAssetKind,
    editor_type_tree_used: bool,
    animation_clip_editor_format: bool,
    generated_prefab_internal: bool,
) -> bool {
    let generated = matches!(factory_kind, FactoryAssetKind::Generated(_));
    let generated_game_object_or_transform = matches!(class_id, 1 | 4) && generated;
    // The generated OcclusionCullingSettings_2017 class exposes the original
    // m_ObjectHideFlags field name through WalkEditor. The older recovered
    // TypeTree still names it that way, while the broad legacy YAML alias maps
    // it to m_HideFlags.
    let generated_occlusion_settings = class_id == 29
        && matches!(
            factory_kind,
            FactoryAssetKind::Generated(name) if name == "OcclusionCullingSettings_2017"
        );
    let generated_controller_member =
        matches!(class_id, 91 | 114 | 206 | 1101 | 1102 | 1107 | 1109) && generated;

    generated_game_object_or_transform
        || generated_occlusion_settings
        || generated_controller_member
        || (animation_clip_editor_format && class_id == 74)
        || editor_type_tree_used
        || (generated_prefab_internal && generated)
}

pub(super) struct WorkspaceYamlObjectConversionRequest<'object> {
    pub(super) object: &'object WorkspaceObject,
    pub(super) source: ExportAssetIdentity,
    pub(super) file_id: i64,
    pub(super) processed_animation_root: Option<&'object SerializedValue>,
    pub(super) animation_clip_editor_format: bool,
    pub(super) game_object_static_mark: bool,
    pub(super) generated_component_overlay: Option<&'object WorkspaceGeneratedComponentYamlOverlay>,
    pub(super) generated_prefab_internal: Option<&'object WorkspaceGeneratedPrefabInternalPointer>,
    pub(super) generated_lighting_data: Option<&'object WorkspaceGeneratedLightingDataYamlPointer>,
    pub(super) sprite_atlas_overlay: Option<&'object WorkspaceSpriteAtlasYamlOverlay>,
    pub(super) nav_mesh_build_settings_overlay:
        Option<&'object WorkspaceNavMeshBuildSettingsOverlay>,
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
            game_object_static_mark,
            generated_component_overlay,
            generated_prefab_internal,
            generated_lighting_data,
            sprite_atlas_overlay,
            nav_mesh_build_settings_overlay,
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
                || generated_lighting_data.is_some()
                || sprite_atlas_overlay.is_some())
        {
            return Err(UnityYamlWorkspaceExportError::AnimationClipEditorFormatInvariant {
                source,
                message: "processed AnimationClip root is attached to an incompatible object or another generated overlay".to_owned(),
            });
        }
        if let Some(overlay) = sprite_atlas_overlay {
            if source.class_id != 213 || overlay.source != source || object.factory.class_id != 213
            {
                return Err(UnityYamlWorkspaceExportError::SpriteAtlasInvariant {
                    source,
                    message: "SpriteAtlas overlay does not match the source Sprite identity"
                        .to_owned(),
                });
            }
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
            .or_else(|| generated_component_overlay.map(|overlay| overlay.payload.root.clone()))
            .or_else(|| sprite_atlas_overlay.map(|overlay| overlay.root.clone()));
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
        if ((animation_clip_editor_format && class_id == 74)
            || matches!(class_id, 104 | 157)
            || class_id == 213
            || class_id == 687078895
            || should_use_scene_editor_type_tree(class_id, unity_version_text))
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
            merge_editor_default_value(
                &mut root,
                export_root,
                &editor_tree,
                0,
                object.object_index,
                "",
            )
            .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            if matches!(class_id, 104 | 157 | 196) {
                ensure_scene_manager_hide_flags(&mut root)
                    .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            if class_id == 157 {
                apply_lightmap_settings_editor_defaults(&mut root)
                    .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            if class_id == 196 {
                if let Some(overlay) = nav_mesh_build_settings_overlay {
                    if overlay.source != source {
                        return Err(scene_editor_format_error(
                            source,
                            class_id,
                            "NavMesh build-settings overlay does not match the source NavMeshSettings identity"
                                .to_owned(),
                        ));
                    }
                }
                apply_nav_mesh_settings_editor_format(&mut root, nav_mesh_build_settings_overlay)
                    .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            if class_id == 850595691 {
                apply_lighting_settings_editor_defaults(&mut root)
                    .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            if class_id == 43 {
                apply_mesh_editor_format(&mut root)
                    .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            if class_id == 28 {
                apply_texture_editor_format(&mut root)
                    .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            if class_id == 213 {
                apply_sprite_geometry(export_root, &mut root)
                    .map_err(|message| scene_editor_format_error(source, class_id, message))?;
            }
            if class_id == 687078895 {
                apply_sprite_atlas_editor_format(&mut root)
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
            if matches!(
                class_id,
                1 | 4 | 23 | 96 | 120 | 137 | 199 | 212 | 218 | 224
            ) {
                apply_scene_object_editor_format(
                    &mut root,
                    export_root,
                    class_id,
                    source,
                    file_index,
                    object.object_index,
                    reference_graph,
                    game_object_static_mark,
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
        if should_preserve_type_tree_field_names(
            class_id,
            &object.factory.kind,
            editor_manager_type_tree.is_some(),
            animation_clip_editor_format,
            generated_prefab_internal.is_some(),
        ) {
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

        if matches!(&object.factory.kind, FactoryAssetKind::Generated(_)) {
            // Generated native assets expose ClassName independently of their
            // shared schema root. TypeTreeObject fallbacks keep that root name.
            hints.native_class_name =
                crate::serialized_file::object::class_id_type_name(class_id).map(str::to_owned);
        }

        let mut pointer_error = None;
        let mut generated_component_pointer_seen = false;
        let mut generated_prefab_internal_pointer_seen = false;
        let mut generated_lighting_data_pointer_seen = false;
        let mut generated_lighting_data_pointer_observations = Vec::new();
        let mut sprite_atlas_pointer_seen = sprite_atlas_overlay
            .map(|overlay| vec![false; overlay.pointers.len()])
            .unwrap_or_default();
        let document = serialized_object_value_to_unity_yaml_document(
            source,
            &payload.type_name,
            document_serialized_version(
                class_id,
                type_root.version,
                editor_manager_type_tree.is_some(),
                unity_version,
                unity_version_text,
            ),
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
                if let Some(overlay) = sprite_atlas_overlay {
                    if request.source == overlay.source {
                        for (index, pointer) in overlay.pointers.iter().enumerate() {
                            if request.field_path == pointer.field_path {
                                return resolve_sprite_atlas_pointer(
                                    request,
                                    overlay.source,
                                    pointer,
                                    pointer_map,
                                    current_collection_index,
                                    &mut sprite_atlas_pointer_seen[index],
                                    &mut pointer_error,
                                );
                            }
                        }
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
        let mut document = document.map_err(UnityYamlWorkspaceExportError::Conversion)?;
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
        if let Some(overlay) = sprite_atlas_overlay {
            if let Some(index) = sprite_atlas_pointer_seen.iter().position(|seen| !seen) {
                return Err(UnityYamlWorkspaceExportError::SpriteAtlasInvariant {
                    source,
                    message: format!(
                        "copied SpriteAtlas PPtr at {:?} was not visited by the YAML walker",
                        overlay.pointers[index].field_path
                    ),
                });
            }
        }
        let legacy_mesh_editor_header_backfill = class_id == 43
            && legacy_mesh_editor_header_backfill_version(unity_version.major, unity_version.minor);
        if editor_manager_type_tree.is_none()
            && !document.root.stripped
            && (prefab_header_backfill_version(unity_version.major, unity_version.minor)
                || legacy_mesh_editor_header_backfill)
        {
            let backfill = if class_id == 1 {
                apply_game_object_header_backfill(
                    &mut document,
                    export_root,
                    game_object_static_mark,
                )
            } else if class_id == 4 {
                apply_transform_header_backfill(
                    &mut document,
                    export_root,
                    source,
                    file_index,
                    object.object_index,
                    reference_graph,
                )
            } else if legacy_mesh_editor_header_backfill {
                apply_legacy_mesh_editor_header_backfill(&mut document)
            } else {
                apply_generic_header_backfill(&mut document, class_id)
            };
            backfill.map_err(|message| scene_editor_format_error(source, class_id, message))?;
            // MonoBehaviour curves and fields serialize through managed layouts
            // without these editor slots, so the editor defaults must not be
            // added there.
            if class_id != 114 {
                apply_keyframe_tangent_mode_backfill(&mut document);
                apply_custom_data_label_backfill(&mut document);
                // The frozen `EditorFormatConverter.Convert(IRenderer)` covers
                // every renderer; the backfill mirrors its class set.
                if matches!(class_id, 23 | 96 | 120 | 137 | 199 | 212) {
                    apply_renderer_gi_backfill(
                        &mut document,
                        unity_version.major,
                        unity_version.minor,
                    );
                }
                // NavMeshSettings carries a defaulted build-settings struct
                // the 2019.4 release layout lacks.
                if class_id == 196 {
                    apply_navmesh_settings_backfill(
                        &mut document,
                        unity_version.major,
                        unity_version.minor,
                    )
                    .map_err(|message| scene_editor_format_error(source, class_id, message))?;
                }
                apply_class_editor_field_backfill(
                    &mut document,
                    class_id,
                    unity_version.major,
                    unity_version.minor,
                );
            }
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

fn resolve_sprite_atlas_pointer(
    request: UnityYamlPPtrRequest<'_>,
    source: ExportAssetIdentity,
    planned: &WorkspaceSpriteAtlasPointer,
    pointer_map: &ProjectExportPointerMap,
    current_collection_index: usize,
    seen: &mut bool,
    error: &mut Option<UnityYamlWorkspaceExportError>,
) -> ProjectPointerResolution {
    let invariant_error =
        |message: String| UnityYamlWorkspaceExportError::SpriteAtlasInvariant { source, message };
    if request.file_id != 0 || request.path_id != planned.path_id {
        *error = Some(invariant_error(format!(
            "copied SpriteAtlas PPtr at {:?} has FileID/PathID ({}, {}), expected (0, {})",
            request.field_path, request.file_id, request.path_id, planned.path_id
        )));
        return ProjectPointerResolution::Null;
    }
    if *seen {
        *error = Some(invariant_error(format!(
            "copied SpriteAtlas PPtr at {:?} was visited more than once",
            request.field_path
        )));
        return ProjectPointerResolution::Null;
    }
    *seen = true;
    let planned_null = matches!(&planned.target, ProjectPointerRequest::Null);
    match pointer_map.resolve(planned.target.clone(), current_collection_index) {
        Ok(ProjectPointerResolution::Null) if planned_null => ProjectPointerResolution::Null,
        Ok(ProjectPointerResolution::Null) => {
            *error = Some(invariant_error(format!(
                "non-null SpriteAtlas PPtr at {:?} resolved as null",
                request.field_path
            )));
            ProjectPointerResolution::Null
        }
        Ok(resolution) => resolution,
        Err(source) => {
            *error = Some(UnityYamlWorkspaceExportError::PointerMap(source));
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
    type_tree: &TypeTree,
    type_node_index: usize,
    object_index: usize,
    field_path: &str,
) -> Result<(), String> {
    match (defaults, source) {
        (SerializedValue::Struct(default_fields), SerializedValue::Struct(source_fields)) => {
            let tree_children = direct_type_tree_children(type_tree, type_node_index)?;
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
                let matching_tree_children = tree_children
                    .iter()
                    .copied()
                    .filter(|&index| {
                        type_tree.nodes[index].name.as_deref() == Some(&source_field.name)
                    })
                    .collect::<Vec<_>>();
                let [child_node_index] = matching_tree_children.as_slice() else {
                    return Err(format!(
                        "editor TypeTree has {} schema nodes for field {child_path}",
                        matching_tree_children.len()
                    ));
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
                    type_tree,
                    *child_node_index,
                    object_index,
                    &child_path,
                )?;
            }
            Ok(())
        }
        (SerializedValue::Array(default_items), SerializedValue::Array(source_items)) => {
            let element_node_index = type_tree_array_element_node(type_tree, type_node_index)
                .map_err(|message| format!("{field_path}: {message}"))?;
            let template = default_value_for_type_tree_node(
                type_tree,
                object_index,
                element_node_index,
                ObjectDefaultValueLimits::default(),
            )
            .map_err(|error| {
                format!("{field_path}: could not construct editor array element: {error}")
            })?;
            let mut merged_items = Vec::new();
            merged_items
                .try_reserve_exact(source_items.len())
                .map_err(|_| format!("{field_path}: could not reserve editor array elements"))?;
            for (index, source_item) in source_items.iter().enumerate() {
                let item_path = format!("{field_path}[{index}]");
                let mut item = template.clone();
                merge_editor_default_value(
                    &mut item,
                    source_item,
                    type_tree,
                    element_node_index,
                    object_index,
                    &item_path,
                )?;
                merged_items.push(item);
            }
            *default_items = merged_items;
            Ok(())
        }
        (default, source) => {
            *default = source.clone();
            Ok(())
        }
    }
}

fn direct_type_tree_children(
    type_tree: &TypeTree,
    node_index: usize,
) -> Result<Vec<usize>, String> {
    let node = type_tree
        .nodes
        .get(node_index)
        .ok_or_else(|| format!("editor TypeTree node {node_index} is out of range"))?;
    let child_level = node
        .level
        .checked_add(1)
        .ok_or_else(|| format!("editor TypeTree node {node_index} has an invalid depth"))?;
    let mut children = Vec::new();
    for (index, candidate) in type_tree.nodes.iter().enumerate().skip(node_index + 1) {
        if candidate.level <= node.level {
            break;
        }
        if candidate.level == child_level {
            children.push(index);
        }
    }
    Ok(children)
}

fn type_tree_array_element_node(type_tree: &TypeTree, node_index: usize) -> Result<usize, String> {
    let node = type_tree
        .nodes
        .get(node_index)
        .ok_or_else(|| format!("editor TypeTree array node {node_index} is out of range"))?;
    let array_node_index = if node.type_name.as_deref() == Some("Array") {
        node_index
    } else {
        let array_children = direct_type_tree_children(type_tree, node_index)?
            .into_iter()
            .filter(|&child| {
                type_tree.nodes[child].type_name.as_deref() == Some("Array")
                    && type_tree.nodes[child].name.as_deref() == Some("Array")
            })
            .collect::<Vec<_>>();
        match array_children.as_slice() {
            [array_node_index] => *array_node_index,
            _ => {
                return Err(format!(
                    "editor TypeTree node {node_index} does not identify one array schema"
                ));
            }
        }
    };
    let children = direct_type_tree_children(type_tree, array_node_index)?;
    let mut data_node_index = None;
    let mut has_size = false;
    for child_index in children {
        match type_tree.nodes[child_index].name.as_deref() {
            Some("size") => has_size = true,
            Some("data") if data_node_index.replace(child_index).is_some() => {
                return Err(format!(
                    "editor TypeTree array node {array_node_index} has duplicate data nodes"
                ));
            }
            _ => {}
        }
    }
    if !has_size {
        return Err(format!(
            "editor TypeTree array node {array_node_index} has no size node"
        ));
    }
    data_node_index.ok_or_else(|| {
        format!("editor TypeTree array node {array_node_index} has no data element schema")
    })
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

#[allow(clippy::too_many_arguments)]
fn apply_scene_object_editor_format(
    editor_root: &mut SerializedValue,
    source_root: &SerializedValue,
    class_id: i32,
    source: ExportAssetIdentity,
    file_index: usize,
    object_index: usize,
    reference_graph: &MultiFileReferenceGraph<'_>,
    game_object_static_mark: bool,
) -> Result<(), String> {
    match class_id {
        1 => apply_game_object_editor_format(editor_root, source_root, game_object_static_mark),
        4 | 224 => apply_transform_editor_format(
            editor_root,
            source_root,
            source,
            file_index,
            object_index,
            reference_graph,
        ),
        // The frozen `EditorFormatConverter.Convert(IRenderer)` covers every
        // renderer; 23/137/199 were the first transcribed, 96/120/212 came from
        // renderer breadth probes (all probed on 2022.3).
        23 | 96 | 120 | 137 | 199 | 212 => apply_renderer_editor_format(editor_root),
        // The frozen `EditorFormatProcessor.ConvertAsync` stamps a fixed
        // `0.0512` terrain scale unconditionally.
        218 => apply_terrain_editor_format(editor_root),
        _ => Ok(()),
    }
}

fn apply_game_object_editor_format(
    editor_root: &mut SerializedValue,
    source_root: &SerializedValue,
    static_mark: bool,
) -> Result<(), String> {
    let tag_name = game_object_tag_name(source_root)?;
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
    if static_mark {
        // Mirrors the frozen `MarkGameObjectAsStatic`: when editor-format
        // conversion enables Everything, Unity sets all bits even though it
        // only uses the first 7. A missing field fails loudly like the other
        // GameObject editor invariants above.
        let flags = editor_root
            .unique_field_in_base_chain_mut("m_StaticEditorFlags")
            .map_err(|error| format!("could not inspect GameObject.m_StaticEditorFlags: {error}"))?
            .ok_or_else(|| "editor GameObject has no m_StaticEditorFlags field".to_owned())?;
        let SerializedValue::U32(value) = flags else {
            return Err(format!(
                "editor GameObject.m_StaticEditorFlags has value {flags:?}, expected U32"
            ));
        };
        *value = u32::MAX;
    }
    Ok(())
}

/// Renderer classes covered by the frozen `EditorFormatConverter.Convert(IRenderer)`.
fn editor_format_renderer_class(class_id: i32) -> bool {
    matches!(class_id, 23 | 96 | 120 | 137 | 199 | 212)
}

fn serialized_int_value(value: &SerializedValue) -> Option<i64> {
    match value {
        SerializedValue::I8(value) => Some(i64::from(*value)),
        SerializedValue::U8(value) => Some(i64::from(*value)),
        SerializedValue::I16(value) => Some(i64::from(*value)),
        SerializedValue::U16(value) => Some(i64::from(*value)),
        SerializedValue::I32(value) => Some(i64::from(*value)),
        SerializedValue::U32(value) => Some(i64::from(*value)),
        SerializedValue::I64(value) => Some(*value),
        SerializedValue::U64(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

/// Mirrors the frozen `EditorFormatConverter` static rule for one renderer
/// payload: a present `m_StaticBatchInfo` marks unless it is default
/// (`firstSubMesh == 0 && subMeshCount == 0`); otherwise a present
/// non-empty `m_SubsetIndices` marks. Missing or unreadable fields select no
/// mark: the renderer's own export path still fails loudly on truly broken
/// data, while the GameObject keeps its source flags.
fn renderer_static_batch_marks(root: &SerializedValue) -> bool {
    let batch_info = root.unique_field_in_base_chain("m_StaticBatchInfo");
    if let Ok(Some(batch_info)) = batch_info {
        let first = batch_info
            .unique_field_in_base_chain("firstSubMesh")
            .ok()
            .flatten()
            .and_then(serialized_int_value)
            .unwrap_or(0);
        let count = batch_info
            .unique_field_in_base_chain("subMeshCount")
            .ok()
            .flatten()
            .and_then(serialized_int_value)
            .unwrap_or(0);
        return first != 0 || count != 0;
    }
    if let Ok(Some(SerializedValue::Array(items))) =
        root.unique_field_in_base_chain("m_SubsetIndices")
    {
        return !items.is_empty();
    }
    false
}

/// Whether the frozen editor-format conversion would mark this GameObject
/// static: any attached renderer component meeting the static-batch rule.
/// Reference edges under `m_Component` resolve the attached components;
/// unresolvable renderers select no mark and keep the ordinary route.
pub(super) fn game_object_static_mark(
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    object: &WorkspaceObject,
) -> bool {
    reference_graph
        .references_from_object(object.file_index, object.object_index)
        .filter(|edge| edge.field_path.starts_with("m_Component"))
        .filter_map(|edge| match &edge.resolution {
            MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index,
            } => Some((*target_file_index, *target_object_index)),
            _ => None,
        })
        .any(|(target_file_index, target_object_index)| {
            let class_id = reference_graph.object_class_id(target_file_index, target_object_index);
            if !class_id.is_some_and(editor_format_renderer_class) {
                return false;
            }
            let Some(target) = objects.get(target_file_index, target_object_index) else {
                return false;
            };
            let Ok(payload) = target.payload.as_ref() else {
                return false;
            };
            renderer_static_batch_marks(&payload.root)
        })
}

/// Prefab/scene editor headers for versions without a ported editor TypeTree.
///
/// Reference output proves 2018.4 and 2019.4 share the 2018.3 GameObject and
/// Transform header sets; other versions stay on the fail-open plain route
/// until their sets have equivalent evidence.
pub(super) fn prefab_header_backfill_version(major: u32, minor: u32) -> bool {
    matches!((major, minor), (2018, 4) | (2019, 4))
}

/// Unity 5.6 uses legacy Mesh editor header names. Keep this separate from the
/// generic prefab-header versions so older classes do not receive unverified
/// editor fields.
pub(super) fn legacy_mesh_editor_header_backfill_version(major: u32, minor: u32) -> bool {
    matches!((major, minor), (5, 6))
}

fn yaml_body_entries(
    document: &mut UnityYamlDocument,
) -> Result<
    &mut Vec<(
        crate::export::unity_yaml::UnityYamlNode,
        crate::export::unity_yaml::UnityYamlNode,
    )>,
    String,
> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut document.root.kind else {
        return Err("Unity YAML document root is not a mapping".to_owned());
    };
    let (_, body) = entries
        .first_mut()
        .ok_or_else(|| "Unity YAML document root has no class body entry".to_owned())?;
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut body.kind else {
        return Err("Unity YAML class body is not a mapping".to_owned());
    };
    Ok(entries)
}

fn yaml_field_index(
    entries: &[(
        crate::export::unity_yaml::UnityYamlNode,
        crate::export::unity_yaml::UnityYamlNode,
    )],
    name: &str,
) -> Option<usize> {
    use crate::export::unity_yaml::{UnityYamlNodeKind, UnityYamlScalar};

    entries.iter().position(|(key, _)| match &key.kind {
        UnityYamlNodeKind::Scalar(UnityYamlScalar::PlainString(key)) => key == name,
        _ => false,
    })
}

fn yaml_plain_node(name: &str) -> crate::export::unity_yaml::UnityYamlNode {
    crate::export::unity_yaml::UnityYamlNode::plain_string(name)
}

fn yaml_null_pptr_node() -> crate::export::unity_yaml::UnityYamlNode {
    use crate::export::unity_yaml::{UnityYamlMappingStyle, UnityYamlNode, UnityYamlScalar};

    let mut pointer = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    pointer
        .insert_field("fileID", UnityYamlNode::scalar(UnityYamlScalar::Signed(0)))
        .expect("null PPtr mapping accepts fileID");
    pointer
}

fn game_object_tag_name(source_root: &SerializedValue) -> Result<String, String> {
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
    Ok(match tag_id {
        0 => "Untagged".to_owned(),
        1 => "Respawn".to_owned(),
        2 => "Finish".to_owned(),
        3 => "EditorOnly".to_owned(),
        5 => "MainCamera".to_owned(),
        6 => "Player".to_owned(),
        7 => "GameController".to_owned(),
        value => format!("unknown_{value}"),
    })
}

const OBJECT_EDITOR_HEADER_FIELDS: [&str; 1] = ["m_ObjectHideFlags"];
const LEGACY_MESH_EDITOR_HEADER_FIELDS: [&str; 3] = [
    "m_ObjectHideFlags",
    "m_PrefabParentObject",
    "m_PrefabInternal",
];
const PREFAB_EDITOR_HEADER_FIELDS: [&str; 4] = [
    "m_ObjectHideFlags",
    "m_CorrespondingSourceObject",
    "m_PrefabInstance",
    "m_PrefabAsset",
];

/// Inserts the editor header fields selected by the source class layout when
/// the decoded payload lacks them. Insertion preserves source order and skips
/// fields the conversion already wrote.
pub(super) fn insert_editor_headers(
    entries: &mut Vec<(UnityYamlNode, UnityYamlNode)>,
    fields: &[&str],
) {
    let mut insert_at = match yaml_field_index(entries, "serializedVersion") {
        Some(index) => index + 1,
        None => 0,
    };
    for &name in fields {
        if yaml_field_index(entries, name).is_none() {
            let value = if name == "m_ObjectHideFlags" {
                UnityYamlNode::scalar(UnityYamlScalar::Unsigned(0))
            } else {
                yaml_null_pptr_node()
            };
            entries.insert(insert_at, (yaml_plain_node(name), value));
            insert_at += 1;
        }
    }
}

/// Inserts the editor-default custom-data labels (`colorLabelN`,
/// `vectorLabelN_M`) after the particle CustomData stream fields they annotate.
///
/// The serialized CustomData layout carries the indexed `colorN`/`vectorN_M`
/// fields without labels; the reference editor layout carries empty-string
/// labels after each one. Objects converted through an editor TypeTree gain
/// them from editor defaults; versions without a ported editor tree need them
/// here. Insertion only follows fields already present and skips labels
/// already written, so other shapes select no completion.
pub(super) fn apply_custom_data_label_backfill(document: &mut UnityYamlDocument) {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut document.root.kind else {
        return;
    };
    let Some((_, body)) = entries.first_mut() else {
        return;
    };
    backfill_labels_in_node(body);
}

fn custom_data_label_for(field: &str) -> Option<String> {
    if let Some(stream) = field.strip_prefix("color") {
        if !stream.is_empty() && stream.bytes().all(|b| b.is_ascii_digit()) {
            return Some(format!("colorLabel{stream}"));
        }
    }
    if let Some(rest) = field.strip_prefix("vector") {
        if let Some((stream, index)) = rest.split_once('_') {
            if !stream.is_empty()
                && !index.is_empty()
                && stream.bytes().all(|b| b.is_ascii_digit())
                && index.bytes().all(|b| b.is_ascii_digit())
            {
                return Some(format!("vectorLabel{stream}_{index}"));
            }
        }
    }
    None
}

fn backfill_labels_in_node(node: &mut UnityYamlNode) {
    match &mut node.kind {
        UnityYamlNodeKind::Mapping { entries, .. } => {
            let mut index = 0;
            while index < entries.len() {
                let name = match &entries[index].0.kind {
                    UnityYamlNodeKind::Scalar(UnityYamlScalar::PlainString(name)) => name.clone(),
                    _ => {
                        index += 1;
                        continue;
                    }
                };
                match custom_data_label_for(&name) {
                    Some(label) if yaml_field_index(entries, &label).is_none() => {
                        entries.insert(
                            index + 1,
                            (
                                yaml_plain_node(&label),
                                UnityYamlNode::plain_string(String::new()),
                            ),
                        );
                        index += 2;
                    }
                    _ => {
                        index += 1;
                    }
                }
            }
            for (_, value) in entries.iter_mut() {
                backfill_labels_in_node(value);
            }
        }
        UnityYamlNodeKind::Sequence { items, .. } => {
            for item in items.iter_mut() {
                backfill_labels_in_node(item);
            }
        }
        UnityYamlNodeKind::Scalar(_) => {}
    }
}

/// Inserts the editor-default renderer lightmap fields when the serialized
/// layout lacks them.
///
/// The reference renderer editor layout carries ten lightmap fields with
/// canonical defaults; some serialized layouts omit them. Objects converted
/// through an editor TypeTree gain them from editor defaults; versions
/// without a ported editor tree need them here. Insertion targets the
/// `23|96|120|137|199|212` renderer family (mirroring the editor conversion's
/// class set), places absent fields after `m_LightProbeVolumeOverride` (else
/// before `m_SortingLayerID`, else appended), and never rewrites source
/// values. MonoBehaviour documents are excluded by the caller.
///
/// The same editor layout omits the baked lightmap indices and additional
/// vertex streams that some serialized layouts carry, so those release-only
/// fields are removed: Unity re-derives baked indices on import, and the
/// reference output never contains them.
pub(super) fn apply_renderer_gi_backfill(document: &mut UnityYamlDocument, major: u32, minor: u32) {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut document.root.kind else {
        return;
    };
    let Some((_, body)) = entries.first_mut() else {
        return;
    };
    let UnityYamlNodeKind::Mapping {
        entries: fields, ..
    } = &mut body.kind
    else {
        return;
    };
    let defaults = [
        (
            "m_ScaleInLightmap",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(1.0)),
        ),
        (
            "m_PreserveUVs",
            UnityYamlNode::scalar(UnityYamlScalar::Boolean(false)),
        ),
        (
            "m_IgnoreNormalsForChartDetection",
            UnityYamlNode::scalar(UnityYamlScalar::Boolean(false)),
        ),
        (
            "m_ImportantGI",
            UnityYamlNode::scalar(UnityYamlScalar::Boolean(false)),
        ),
        (
            "m_StitchLightmapSeams",
            UnityYamlNode::scalar(UnityYamlScalar::Boolean(false)),
        ),
        (
            "m_SelectedEditorRenderState",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(3)),
        ),
        (
            "m_MinimumChartSize",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(4)),
        ),
        (
            "m_AutoUVMaxDistance",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(0.5)),
        ),
        (
            "m_AutoUVMaxAngle",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(89.0)),
        ),
        ("m_LightmapParameters", yaml_null_pptr_node()),
    ];
    let mut insert_at = match yaml_field_index(fields, "m_LightProbeVolumeOverride") {
        Some(index) => index + 1,
        None => match yaml_field_index(fields, "m_SortingLayerID") {
            Some(index) => index,
            None => fields.len(),
        },
    };
    for (name, value) in defaults {
        if yaml_field_index(fields, name).is_none() {
            fields.insert(insert_at, (yaml_plain_node(name), value));
            insert_at += 1;
        }
    }
    // 2019.4 adds the receive-GI mode after the lightmap scale; 2018.4
    // reference output omits it, so it stays version-gated.
    if (major, minor) == (2019, 4) && yaml_field_index(fields, "m_ReceiveGI").is_none() {
        let position = match yaml_field_index(fields, "m_ScaleInLightmap") {
            Some(index) => index + 1,
            None => match yaml_field_index(fields, "m_PreserveUVs") {
                Some(index) => index,
                None => fields.len(),
            },
        };
        fields.insert(
            position,
            (
                yaml_plain_node("m_ReceiveGI"),
                UnityYamlNode::scalar(UnityYamlScalar::Signed(1)),
            ),
        );
    }
    fields.retain(|(key, _)| match &key.kind {
        UnityYamlNodeKind::Scalar(UnityYamlScalar::PlainString(name)) => !matches!(
            name.as_str(),
            "m_LightmapIndex"
                | "m_LightmapIndexDynamic"
                | "m_LightmapTilingOffset"
                | "m_LightmapTilingOffsetDynamic"
                | "m_AdditionalVertexStreams"
        ),
        _ => true,
    });
}

/// Inserts the default 2019.4 NavMesh build-settings struct into
/// NavMeshSettings documents whose serialized layout lacks it.
///
/// The 2019.4 release layout carries only `m_NavMeshData`; the editor layout
/// adds `m_BuildSettings` (TPK-probed member types) and omits the prefab
/// headers, and reference output fills Unity's default build settings.
/// Values match the reference document exactly (cell size is one sixth).
/// Present structs are kept, and unproven versions select no completion.
pub(super) fn apply_navmesh_settings_backfill(
    document: &mut UnityYamlDocument,
    major: u32,
    minor: u32,
) -> Result<(), String> {
    use crate::export::unity_yaml::UnityYamlMappingStyle;

    if (major, minor) != (2019, 4) {
        return Ok(());
    }
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut document.root.kind else {
        return Ok(());
    };
    let Some((_, body)) = entries.first_mut() else {
        return Ok(());
    };
    let UnityYamlNodeKind::Mapping {
        entries: fields, ..
    } = &mut body.kind
    else {
        return Ok(());
    };
    if yaml_field_index(fields, "m_BuildSettings").is_some() {
        return Ok(());
    }
    let failed = "could not allocate default NavMesh build settings";
    let mut debug = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    debug
        .insert_field("m_Flags", UnityYamlNode::scalar(UnityYamlScalar::Signed(0)))
        .map_err(|_| failed.to_owned())?;
    let mut settings = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    settings
        .insert_field(
            "serializedVersion",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(2)),
        )
        .map_err(|_| failed.to_owned())?;
    for (name, value) in [
        (
            "agentTypeID",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
        ),
        (
            "agentRadius",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(0.5)),
        ),
        (
            "agentHeight",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(2.0)),
        ),
        (
            "agentSlope",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(45.0)),
        ),
        (
            "agentClimb",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(0.4)),
        ),
        (
            "ledgeDropHeight",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(0.0)),
        ),
        (
            "maxJumpAcrossDistance",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(0.0)),
        ),
        (
            "minRegionArea",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(2.0)),
        ),
        (
            "manualCellSize",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
        ),
        (
            "cellSize",
            UnityYamlNode::scalar(UnityYamlScalar::Float32(1.0 / 6.0)),
        ),
        (
            "manualTileSize",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
        ),
        (
            "tileSize",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(256)),
        ),
        (
            "accuratePlacement",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
        ),
    ] {
        settings
            .insert_field(name, value)
            .map_err(|_| failed.to_owned())?;
    }
    settings
        .insert_field("debug", debug)
        .map_err(|_| failed.to_owned())?;
    let position = match yaml_field_index(fields, "m_NavMeshData") {
        Some(index) => index,
        None => fields.len(),
    };
    fields.insert(position, (yaml_plain_node("m_BuildSettings"), settings));
    // The 196 editor layout carries no prefab headers, so the universal
    // header insert is reverted here to match the reference field order.
    fields.retain(|(key, _)| match &key.kind {
        UnityYamlNodeKind::Scalar(UnityYamlScalar::PlainString(name)) => !matches!(
            name.as_str(),
            "m_CorrespondingSourceObject" | "m_PrefabInstance" | "m_PrefabAsset"
        ),
        _ => true,
    });
    Ok(())
}

/// A class-specific editor-layout delta: fields the editor layout carries
/// with defaults that some serialized layouts lack (inserted), plus
/// release-only fields the editor layout omits (removed).
struct ClassEditorFieldDelta {
    class_id: i32,
    inserts: &'static [ClassFieldInsert],
    removes: &'static [&'static str],
}

/// One defaulted field insertion: placed after `after` when present, else
/// before `before` when present, else appended. An empty `after` selects the
/// `before` anchor directly (used when the predecessor is an array item with
/// no plain mapping key). `versions` lists the backfill (major, minor) pairs
/// the insertion is proven for; empty applies to every backfill version.
struct ClassFieldInsert {
    name: &'static str,
    after: &'static str,
    before: &'static str,
    value: ClassFieldDefault,
    versions: &'static [(u32, u32)],
}

/// Constructible default values for inserted editor fields.
#[derive(Clone, Copy)]
enum ClassFieldDefault {
    EmptyString,
    Signed(i64),
    Float32(f32),
    Boolean(bool),
}

impl ClassFieldDefault {
    fn node(self) -> UnityYamlNode {
        match self {
            ClassFieldDefault::EmptyString => UnityYamlNode::plain_string(String::new()),
            ClassFieldDefault::Signed(value) => {
                UnityYamlNode::scalar(UnityYamlScalar::Signed(value))
            }
            ClassFieldDefault::Float32(value) => {
                UnityYamlNode::scalar(UnityYamlScalar::Float32(value))
            }
            ClassFieldDefault::Boolean(value) => {
                UnityYamlNode::scalar(UnityYamlScalar::Boolean(value))
            }
        }
    }
}

/// Class-specific editor deltas proven against reference output. The Animator
/// warning message, the Light shadow metrics, and the ParentConstraint lock
/// flag are editor defaults absent from the data; the Light baking output is
/// a release-only struct the editor layout omits. The Mesh optimizer flag is
/// an editor-only bool set to true by `EditorFormatProcessor` on Unity 5.6 and
/// 2018.4; 2019.4 renames it to an editor-only int set to -1. The Camera axis
/// mode is a 2019.4 editor-only int defaulting to 0.
const CLASS_EDITOR_FIELD_DELTAS: &[ClassEditorFieldDelta] = &[
    ClassEditorFieldDelta {
        class_id: 20,
        inserts: &[ClassFieldInsert {
            name: "m_FOVAxisMode",
            after: "m_GateFitMode",
            before: "m_SensorSize",
            value: ClassFieldDefault::Signed(0),
            versions: &[(2019, 4)],
        }],
        removes: &[],
    },
    ClassEditorFieldDelta {
        class_id: 43,
        inserts: &[
            ClassFieldInsert {
                name: "m_MeshOptimized",
                after: "",
                before: "m_StreamData",
                value: ClassFieldDefault::Boolean(true),
                versions: &[(5, 6), (2018, 4)],
            },
            ClassFieldInsert {
                name: "m_MeshOptimizationFlags",
                after: "",
                before: "m_StreamData",
                value: ClassFieldDefault::Signed(-1),
                versions: &[(2019, 4)],
            },
        ],
        removes: &[],
    },
    ClassEditorFieldDelta {
        class_id: 95,
        inserts: &[ClassFieldInsert {
            name: "m_WarningMessage",
            after: "m_LinearVelocityBlending",
            before: "m_HasTransformHierarchy",
            value: ClassFieldDefault::EmptyString,
            versions: &[],
        }],
        removes: &[],
    },
    ClassEditorFieldDelta {
        class_id: 108,
        inserts: &[
            ClassFieldInsert {
                name: "m_ShadowRadius",
                after: "m_UseColorTemperature",
                before: "m_Cookie",
                value: ClassFieldDefault::Float32(0.0),
                versions: &[],
            },
            ClassFieldInsert {
                name: "m_ShadowAngle",
                after: "m_ShadowRadius",
                before: "m_Cookie",
                value: ClassFieldDefault::Float32(0.0),
                versions: &[],
            },
        ],
        removes: &["m_BakingOutput"],
    },
    ClassEditorFieldDelta {
        class_id: 1773428102,
        inserts: &[ClassFieldInsert {
            name: "m_IsLocked",
            after: "m_IsContraintActive",
            before: "m_Sources",
            value: ClassFieldDefault::Boolean(false),
            versions: &[],
        }],
        removes: &[],
    },
];

/// Applies the class-specific editor-layout delta for the document's class, if
/// any is declared. Insertions skip fields already present and skip versions
/// outside their proven list. The Mesh optimization fields also take the value
/// forced by `EditorFormatProcessor`, even when serialized data had another
/// value; removals drop release-only fields the editor layout omits. Other
/// classes select no completion. MonoBehaviour documents are excluded by the
/// caller.
pub(super) fn apply_class_editor_field_backfill(
    document: &mut UnityYamlDocument,
    class_id: i32,
    major: u32,
    minor: u32,
) {
    let Some(delta) = CLASS_EDITOR_FIELD_DELTAS
        .iter()
        .find(|delta| delta.class_id == class_id)
    else {
        return;
    };
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut document.root.kind else {
        return;
    };
    let Some((_, body)) = entries.first_mut() else {
        return;
    };
    let UnityYamlNodeKind::Mapping {
        entries: fields, ..
    } = &mut body.kind
    else {
        return;
    };
    for insert in delta.inserts {
        if !insert.versions.is_empty() && !insert.versions.contains(&(major, minor)) {
            continue;
        }
        if let Some(index) = yaml_field_index(fields, insert.name) {
            if class_id == 43
                && matches!(insert.name, "m_MeshOptimized" | "m_MeshOptimizationFlags")
            {
                fields[index].1 = insert.value.node();
            }
            continue;
        }
        let position = match yaml_field_index(fields, insert.after) {
            Some(index) => index + 1,
            None => match yaml_field_index(fields, insert.before) {
                Some(index) => index,
                None => fields.len(),
            },
        };
        fields.insert(
            position,
            (yaml_plain_node(insert.name), insert.value.node()),
        );
    }
    if !delta.removes.is_empty() {
        fields.retain(|(key, _)| match &key.kind {
            UnityYamlNodeKind::Scalar(UnityYamlScalar::PlainString(name)) => {
                !delta.removes.contains(&name.as_str())
            }
            _ => true,
        });
    }
}

/// Inserts the editor-default keyframe `tangentMode` into decoded animation
/// curves whose serialized layout lacks it.
///
/// Some layouts (e.g. the 2018.4 Keyframe v3) serialize seven fields without
/// `tangentMode`; the reference editor layout carries the field and exports
/// its default 0. Objects converted through an editor TypeTree gain it from
/// editor defaults; versions without a ported editor tree need it here.
/// Insertion targets `m_Curve` keyframe items that carry `time` and
/// `weightedMode` but no `tangentMode`, placing the default before
/// `weightedMode` to match the reference field order. Other shapes select no
/// completion rather than guessing a position. MonoBehaviour documents are
/// excluded by the caller: their curves serialize through managed layouts
/// without a tangentMode slot.
pub(super) fn apply_keyframe_tangent_mode_backfill(document: &mut UnityYamlDocument) {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut document.root.kind else {
        return;
    };
    let Some((_, body)) = entries.first_mut() else {
        return;
    };
    backfill_tangent_mode_in_node(body);
}

fn backfill_tangent_mode_in_node(node: &mut UnityYamlNode) {
    match &mut node.kind {
        UnityYamlNodeKind::Mapping { entries, .. } => {
            if let Some(index) = yaml_field_index(entries, "m_Curve") {
                if let UnityYamlNodeKind::Sequence { items, .. } = &mut entries[index].1.kind {
                    for item in items.iter_mut() {
                        let UnityYamlNodeKind::Mapping {
                            entries: keyframe, ..
                        } = &mut item.kind
                        else {
                            continue;
                        };
                        if yaml_field_index(keyframe, "tangentMode").is_some()
                            || yaml_field_index(keyframe, "time").is_none()
                        {
                            continue;
                        }
                        if let Some(weighted) = yaml_field_index(keyframe, "weightedMode") {
                            keyframe.insert(
                                weighted,
                                (
                                    yaml_plain_node("tangentMode"),
                                    UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
                                ),
                            );
                        }
                    }
                }
            }
            for (_, value) in entries.iter_mut() {
                backfill_tangent_mode_in_node(value);
            }
        }
        UnityYamlNodeKind::Sequence { items, .. } => {
            for item in items.iter_mut() {
                backfill_tangent_mode_in_node(item);
            }
        }
        UnityYamlNodeKind::Scalar(_) => {}
    }
}

/// Completes a converted document with the editor header fields exposed by its
/// source class when no editor TypeTree was applied. Class-specific completions
/// (tag replacement, static marking, root order, euler hints) stay separate.
pub(super) fn apply_generic_header_backfill(
    document: &mut UnityYamlDocument,
    class_id: i32,
) -> Result<(), String> {
    let entries = yaml_body_entries(document)?;
    let fields = if class_id == 29 {
        // The source OcclusionCullingSettings editor tree carries only the
        // Object hide flag; this class is not an EditorExtension and has no
        // prefab PPtr headers.
        &OBJECT_EDITOR_HEADER_FIELDS[..]
    } else {
        &PREFAB_EDITOR_HEADER_FIELDS[..]
    };
    insert_editor_headers(entries, fields);
    Ok(())
}

/// Completes a Unity 5.6 Mesh document with its legacy editor header fields.
/// Newer prefab pointer names do not belong to this serialized layout.
pub(super) fn apply_legacy_mesh_editor_header_backfill(
    document: &mut UnityYamlDocument,
) -> Result<(), String> {
    let entries = yaml_body_entries(document)?;
    insert_editor_headers(entries, &LEGACY_MESH_EDITOR_HEADER_FIELDS);
    Ok(())
}

/// Selects the document `serializedVersion` stamp for the converted object.
///
/// Reference documents stamp the editor layout version. Objects converted
/// through an editor TypeTree already carry it; versions without a ported
/// editor tree would otherwise stamp the release/file tree version, which can
/// be newer (per-patch file trees) than the editor layout. The TPK editor
/// root version is the proven-correct stamp; lookup failures keep the existing
/// version instead of failing the export.
fn document_serialized_version(
    class_id: i32,
    type_version: i32,
    editor_tree_used: bool,
    unity_version: SceneUnityVersion,
    unity_version_text: Option<&str>,
) -> i32 {
    if editor_tree_used || !prefab_header_backfill_version(unity_version.major, unity_version.minor)
    {
        return type_version;
    }
    let Some(text) = unity_version_text else {
        return type_version;
    };
    let Some(factory_version) = FactoryUnityVersion::parse(text) else {
        return type_version;
    };
    let Ok(Some((_, editor))) = reference_class_layouts().tpk_trees(class_id, &factory_version)
    else {
        return type_version;
    };
    editor
        .nodes
        .first()
        .map(|node| node.version)
        .unwrap_or(type_version)
}

/// Completes a converted GameObject document with the 2018.4/2019.4 editor
/// headers when no editor TypeTree was applied. Missing source fields select
/// no completion for that field rather than failing the export; the core
/// conversion already succeeded and Unity defaults the absent metadata.
pub(super) fn apply_game_object_header_backfill(
    document: &mut UnityYamlDocument,
    source_root: &SerializedValue,
    static_mark: bool,
) -> Result<(), String> {
    use crate::export::unity_yaml::{UnityYamlNode, UnityYamlScalar};

    let entries = yaml_body_entries(document)?;
    insert_editor_headers(entries, &PREFAB_EDITOR_HEADER_FIELDS);
    if let Some(index) = yaml_field_index(entries, "m_Tag") {
        entries.remove(index);
        entries.insert(
            index,
            (
                yaml_plain_node("m_TagString"),
                UnityYamlNode::string(game_object_tag_name(source_root)?),
            ),
        );
    }
    let tag_string_at = yaml_field_index(entries, "m_TagString");
    if tag_string_at.is_none() {
        if let (Some(name_at), Ok(tag_name)) = (
            yaml_field_index(entries, "m_Name"),
            game_object_tag_name(source_root),
        ) {
            entries.insert(
                name_at + 1,
                (
                    yaml_plain_node("m_TagString"),
                    UnityYamlNode::string(tag_name),
                ),
            );
        }
    }
    if let Some(index) = yaml_field_index(entries, "m_TagString") {
        let mut at = index + 1;
        for (name, value) in [
            ("m_Icon", yaml_null_pptr_node()),
            (
                "m_NavMeshLayer",
                UnityYamlNode::scalar(UnityYamlScalar::Unsigned(0)),
            ),
        ] {
            if yaml_field_index(entries, name).is_none() {
                entries.insert(at, (yaml_plain_node(name), value));
                at += 1;
            }
        }
    }
    match yaml_field_index(entries, "m_StaticEditorFlags") {
        Some(index) => {
            if static_mark {
                let (_, value) = &mut entries[index];
                if matches!(
                    value.kind,
                    crate::export::unity_yaml::UnityYamlNodeKind::Scalar(
                        UnityYamlScalar::Unsigned(_)
                    )
                ) {
                    *value = UnityYamlNode::scalar(UnityYamlScalar::Unsigned(u32::MAX as u64));
                }
            }
        }
        None => {
            entries.insert(
                yaml_field_index(entries, "m_IsActive").unwrap_or(entries.len()),
                (
                    yaml_plain_node("m_StaticEditorFlags"),
                    UnityYamlNode::scalar(UnityYamlScalar::Unsigned(if static_mark {
                        u32::MAX as u64
                    } else {
                        0
                    })),
                ),
            );
        }
    }
    Ok(())
}

/// Completes a converted Transform document with the 2018.4/2019.4 editor
/// headers when no editor TypeTree was applied. Unresolvable fields select no
/// completion, mirroring the GameObject backfill above.
pub(super) fn apply_transform_header_backfill(
    document: &mut UnityYamlDocument,
    source_root: &SerializedValue,
    source: ExportAssetIdentity,
    file_index: usize,
    object_index: usize,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<(), String> {
    use crate::export::unity_yaml::{UnityYamlMappingStyle, UnityYamlNode, UnityYamlScalar};

    let entries = yaml_body_entries(document)?;
    insert_editor_headers(entries, &PREFAB_EDITOR_HEADER_FIELDS);
    if yaml_field_index(entries, "m_RootOrder").is_none() {
        if let Ok(root_order) = calculate_scene_transform_root_order(
            source_root,
            source,
            file_index,
            object_index,
            reference_graph,
        ) {
            entries.push((
                yaml_plain_node("m_RootOrder"),
                UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(root_order))),
            ));
        }
    }
    if yaml_field_index(entries, "m_LocalEulerAnglesHint").is_none() {
        if let Ok(Some(rotation)) = source_root.unique_field_in_base_chain("m_LocalRotation") {
            if let Ok(rotation) = read_scene_quaternion(rotation, "Transform.m_LocalRotation") {
                let euler = crate::resources::quaternion::euler_hint_from_rotation(rotation);
                let mut hint = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
                for (axis, value) in [("x", euler[0]), ("y", euler[1]), ("z", euler[2])] {
                    hint.insert_field(axis, UnityYamlNode::scalar(UnityYamlScalar::Float32(value)))
                        .expect("euler hint mapping accepts axis fields");
                }
                entries.push((yaml_plain_node("m_LocalEulerAnglesHint"), hint));
            }
        }
    }
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

fn apply_terrain_editor_format(root: &mut SerializedValue) -> Result<(), String> {
    set_scene_editor_format_default(
        root,
        "m_ScaleInLightmap",
        SceneEditorFormatDefault::F32(0.0512),
    )
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
    U32(u32),
    F32(f32),
    String(&'static str),
}

fn apply_mesh_editor_format(root: &mut SerializedValue) -> Result<(), String> {
    // Unity's project YAML expands a TypelessData member into its byte count and payload,
    // even though AssetRipper's TPK tree represents both as one `Bytes` value.
    let vertex_data = root
        .unique_field_in_base_chain_mut("m_VertexData")
        .map_err(|error| format!("could not inspect m_VertexData: {error}"))?
        .ok_or_else(|| "Mesh editor TypeTree has no m_VertexData field".to_owned())?;
    let SerializedValue::Struct(vertex_fields) = vertex_data else {
        return Err(format!(
            "m_VertexData has value {vertex_data:?}, expected a struct"
        ));
    };
    let mut data_fields = vertex_fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.name == "m_DataSize");
    let (data_index, data_field) = data_fields
        .next()
        .ok_or_else(|| "m_VertexData has no m_DataSize TypelessData field".to_owned())?;
    if data_fields.next().is_some() {
        return Err("m_VertexData has duplicate m_DataSize fields".to_owned());
    }
    if vertex_fields
        .iter()
        .any(|field| field.name == "_typelessdata")
    {
        return Err("m_VertexData already has a _typelessdata field".to_owned());
    }
    let data_length = match &data_field.value {
        SerializedValue::Bytes(bytes) => bytes.len(),
        value => {
            return Err(format!(
                "m_VertexData.m_DataSize has value {value:?}, expected TypelessData bytes"
            ));
        }
    };
    let serialized_length = i32::try_from(data_length)
        .map_err(|_| "m_VertexData TypelessData exceeds Unity's signed length range".to_owned())?;
    vertex_fields
        .try_reserve(1)
        .map_err(|_| "could not reserve the Unity TypelessData YAML payload field".to_owned())?;
    let payload = match std::mem::replace(
        &mut vertex_fields[data_index].value,
        SerializedValue::I32(serialized_length),
    ) {
        SerializedValue::Bytes(bytes) => bytes,
        value => {
            vertex_fields[data_index].value = value;
            return Err("m_VertexData.m_DataSize changed while preparing YAML".to_owned());
        }
    };
    vertex_fields.insert(
        data_index + 1,
        SerializedFieldValue {
            name: "_typelessdata".to_owned(),
            value: SerializedValue::Bytes(payload),
        },
    );

    // Frozen AssetRipper's EditorFormatProcessor calls
    // SetMeshOptimizationFlags(MeshOptimizationFlags.Everything); Everything is -1.
    set_scene_editor_format_default(
        root,
        "m_MeshOptimizationFlags",
        SceneEditorFormatDefault::I32(-1),
    )
}

/// Applies the frozen `SpriteProcessor.ProcessSprite` geometry recomputation for
/// atlas-less sprites: Rect/Pivot/Border are rederived from the render-data
/// rectangle, then Offset and TextureRectOffset are recomputed from the new
/// values, in source order with `f32` IEEE semantics.
///
/// The atlas-copy branch (non-null `m_SpriteAtlas` with a `RenderDataKey` map hit)
/// is resolved upstream by the sprite-atlas overlay, which copies the atlas render
/// data into the source payload and clears the atlas pointer and tags. Geometry
/// then proceeds on those resolved values. A non-null atlas pointer surviving in
/// either the source or the merged root means no overlay resolved it, which stays
/// a loud error instead of silent raw values.
/// `Has_Pivot`/`Has_Border` follow bundle field presence on the source payload.
fn apply_sprite_geometry(
    source_payload: &SerializedValue,
    root: &mut SerializedValue,
) -> Result<(), String> {
    let containers: [(&SerializedValue, &str); 2] = [(source_payload, "source"), (root, "merged")];
    for (container, container_name) in containers {
        let atlas = container
            .unique_field_in_base_chain("m_SpriteAtlas")
            .map_err(|error| {
                format!("could not inspect {container_name} Sprite.m_SpriteAtlas: {error}")
            })?;
        match atlas {
            Some(SerializedValue::PPtr { path_id: 0, .. }) | None => {}
            Some(SerializedValue::PPtr { file_id, path_id }) => {
                return Err(format!(
                    "Sprite references SpriteAtlas (fileID {file_id}, PathID {path_id}); atlas-packed sprite RenderDataMap resolution is not implemented"
                ));
            }
            Some(value) => {
                return Err(format!(
                    "Sprite.m_SpriteAtlas has value {value:?}, expected a PPtr"
                ));
            }
        }
    }
    let rect = read_rect_field(source_payload, "m_Rect")?;
    let render_data = source_payload
        .unique_field_in_base_chain("m_RD")
        .map_err(|error| format!("could not inspect Sprite.m_RD: {error}"))?
        .ok_or_else(|| "Sprite source payload has no m_RD field".to_owned())?;
    let texture_rect = read_rect_field(render_data, "textureRect")?;
    let crop_bottom_left = read_vec2_field(render_data, "textureRectOffset")?;
    let atlas_rect = (
        texture_rect.0,
        texture_rect.1,
        texture_rect.2,
        texture_rect.3,
    );
    let size_delta = (rect.2 - atlas_rect.2, rect.3 - atlas_rect.3);
    let crop_top_right = (
        size_delta.0 - crop_bottom_left.0,
        size_delta.1 - crop_bottom_left.1,
    );
    let pivot = if source_payload
        .unique_field_in_base_chain("m_Pivot")
        .map_err(|error| format!("could not inspect Sprite.m_Pivot: {error}"))?
        .is_some()
    {
        read_vec2_field(source_payload, "m_Pivot")?
    } else {
        let center = (rect.2 / 2.0, rect.3 / 2.0);
        let offset = read_vec2_field(source_payload, "m_Offset")?;
        let pivot_offset = (center.0 + offset.0, center.1 + offset.1);
        (pivot_offset.0 / rect.2, pivot_offset.1 / rect.3)
    };
    let pivot_position = (pivot.0 * rect.2, pivot.1 * rect.3);
    let atlas_pivot_position = (
        pivot_position.0 - crop_bottom_left.0,
        pivot_position.1 - crop_bottom_left.1,
    );
    let atlas_pivot = (
        atlas_pivot_position.0 / atlas_rect.2,
        atlas_pivot_position.1 / atlas_rect.3,
    );
    let has_border = source_payload
        .unique_field_in_base_chain("m_Border")
        .map_err(|error| format!("could not inspect Sprite.m_Border: {error}"))?
        .is_some();
    if has_border {
        let border = read_vec4_field(source_payload, "m_Border")?;
        let adjusted = (
            adjust_sprite_border(border.0, crop_bottom_left.0),
            adjust_sprite_border(border.1, crop_bottom_left.1),
            adjust_sprite_border(border.2, crop_top_right.0),
            adjust_sprite_border(border.3, crop_top_right.1),
        );
        write_vec4_field(root, "m_Border", adjusted)?;
    }
    write_rect_field(root, "m_Rect", atlas_rect)?;
    if root
        .unique_field_in_base_chain("m_Pivot")
        .map_err(|error| format!("could not inspect Sprite.m_Pivot: {error}"))?
        .is_some()
    {
        write_vec2_field(root, "m_Pivot", atlas_pivot)?;
    }
    write_vec2_field(
        root,
        "m_Offset",
        (
            (atlas_pivot.0 - 0.5) * atlas_rect.2,
            (atlas_pivot.1 - 0.5) * atlas_rect.3,
        ),
    )?;
    write_vec2_field(
        render_data_output(root)?,
        "textureRectOffset",
        (texture_rect.0 - atlas_rect.0, texture_rect.1 - atlas_rect.1),
    )?;
    Ok(())
}

fn adjust_sprite_border(value: f32, crop: f32) -> f32 {
    if value == 0.0 {
        0.0
    } else {
        value - crop
    }
}

fn render_data_output(root: &mut SerializedValue) -> Result<&mut SerializedValue, String> {
    root.unique_field_in_base_chain_mut("m_RD")
        .map_err(|error| format!("could not inspect Sprite.m_RD: {error}"))?
        .ok_or_else(|| "Sprite editor root has no m_RD field".to_owned())
}

fn sprite_struct_field<'a>(
    value: &'a SerializedValue,
    container: &str,
    name: &str,
) -> Result<&'a SerializedValue, String> {
    let SerializedValue::Struct(fields) = value else {
        return Err(format!("{container} is not a struct"));
    };
    fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| &field.value)
        .ok_or_else(|| format!("{container} has no {name} field"))
}

fn sprite_f32_field(value: &SerializedValue, container: &str, name: &str) -> Result<f32, String> {
    match sprite_struct_field(value, container, name)? {
        SerializedValue::F32(v) => Ok(*v),
        other => Err(format!("{container}.{name} is {other:?}, expected F32")),
    }
}

fn read_vec2_field(value: &SerializedValue, name: &str) -> Result<(f32, f32), String> {
    let field = sprite_struct_field(value, "Sprite", name)?;
    Ok((
        sprite_f32_field(field, name, "x")?,
        sprite_f32_field(field, name, "y")?,
    ))
}

fn read_vec4_field(value: &SerializedValue, name: &str) -> Result<(f32, f32, f32, f32), String> {
    let field = sprite_struct_field(value, "Sprite", name)?;
    Ok((
        sprite_f32_field(field, name, "x")?,
        sprite_f32_field(field, name, "y")?,
        sprite_f32_field(field, name, "z")?,
        sprite_f32_field(field, name, "w")?,
    ))
}

fn read_rect_field(value: &SerializedValue, name: &str) -> Result<(f32, f32, f32, f32), String> {
    let field = sprite_struct_field(value, "Sprite", name)?;
    Ok((
        sprite_f32_field(field, name, "x")?,
        sprite_f32_field(field, name, "y")?,
        sprite_f32_field(field, name, "width")?,
        sprite_f32_field(field, name, "height")?,
    ))
}

fn sprite_struct_field_mut<'a>(
    value: &'a mut SerializedValue,
    container: &str,
    name: &str,
) -> Result<&'a mut SerializedValue, String> {
    let SerializedValue::Struct(fields) = value else {
        return Err(format!("{container} is not a struct"));
    };
    fields
        .iter_mut()
        .find(|field| field.name == name)
        .map(|field| &mut field.value)
        .ok_or_else(|| format!("{container} has no {name} field"))
}

fn write_vec2_field(
    value: &mut SerializedValue,
    name: &str,
    vec: (f32, f32),
) -> Result<(), String> {
    let field = sprite_struct_field_mut(value, "Sprite", name)?;
    let SerializedValue::Struct(components) = field else {
        return Err(format!("Sprite.{name} is not a struct"));
    };
    for (component_name, component_value) in [("x", vec.0), ("y", vec.1)] {
        let component = components
            .iter_mut()
            .find(|field| field.name == component_name)
            .ok_or_else(|| format!("Sprite.{name} has no {component_name} field"))?;
        component.value = SerializedValue::F32(component_value);
    }
    Ok(())
}

fn write_vec4_field(
    value: &mut SerializedValue,
    name: &str,
    vec: (f32, f32, f32, f32),
) -> Result<(), String> {
    let field = sprite_struct_field_mut(value, "Sprite", name)?;
    let SerializedValue::Struct(components) = field else {
        return Err(format!("Sprite.{name} is not a struct"));
    };
    for (component_name, component_value) in
        [("x", vec.0), ("y", vec.1), ("z", vec.2), ("w", vec.3)]
    {
        let component = components
            .iter_mut()
            .find(|field| field.name == component_name)
            .ok_or_else(|| format!("Sprite.{name} has no {component_name} field"))?;
        component.value = SerializedValue::F32(component_value);
    }
    Ok(())
}

fn write_rect_field(
    value: &mut SerializedValue,
    name: &str,
    rect: (f32, f32, f32, f32),
) -> Result<(), String> {
    let field = sprite_struct_field_mut(value, "Sprite", name)?;
    let SerializedValue::Struct(components) = field else {
        return Err(format!("Sprite.{name} is not a struct"));
    };
    for (component_name, component_value) in [
        ("x", rect.0),
        ("y", rect.1),
        ("width", rect.2),
        ("height", rect.3),
    ] {
        let component = components
            .iter_mut()
            .find(|field| field.name == component_name)
            .ok_or_else(|| format!("Sprite.{name} has no {component_name} field"))?;
        component.value = SerializedValue::F32(component_value);
    }
    Ok(())
}
/// shape (`image data: <byte count>` plus a `_typelessdata` payload), mirroring
/// the frozen mesh `m_DataSize` expansion: the TPK tree represents both as one
/// `Bytes` value. A missing field (streamed textures without inline bytes) is left
/// untouched; any other shape is an explicit error.
fn apply_texture_editor_format(root: &mut SerializedValue) -> Result<(), String> {
    let SerializedValue::Struct(fields) = root else {
        return Err("Texture2D editor root is not a struct".to_owned());
    };
    let Some(data_index) = fields.iter().position(|field| field.name == "image data") else {
        return Ok(());
    };
    if fields.iter().any(|field| field.name == "_typelessdata") {
        return Err("Texture2D already has a _typelessdata field".to_owned());
    }
    let data_length = match &fields[data_index].value {
        SerializedValue::Bytes(bytes) => bytes.len(),
        value => {
            return Err(format!(
                "Texture2D image data has value {value:?}, expected TypelessData bytes"
            ));
        }
    };
    let serialized_length = i32::try_from(data_length)
        .map_err(|_| "Texture2D TypelessData exceeds Unity's signed length range".to_owned())?;
    fields
        .try_reserve(1)
        .map_err(|_| "could not reserve the Unity TypelessData YAML payload field".to_owned())?;
    let payload = match std::mem::replace(
        &mut fields[data_index].value,
        SerializedValue::I32(serialized_length),
    ) {
        SerializedValue::Bytes(bytes) => bytes,
        value => {
            fields[data_index].value = value;
            return Err("Texture2D image data changed while preparing YAML".to_owned());
        }
    };
    fields.insert(
        data_index + 1,
        SerializedFieldValue {
            name: "_typelessdata".to_owned(),
            value: SerializedValue::Bytes(payload),
        },
    );
    Ok(())
}

/// Editor `m_BuildSettings` values sourced from the referenced class-238 `NavMeshData`,
/// mirroring the frozen `NavMeshSettingsExtensions.ConvertToEditorFormat` branches:
/// a null (or dangling) `m_NavMeshData` keeps the standalone defaults, `m_NavMeshParams`
/// maps five fields over those defaults, and `m_NavMeshBuildSettings` copies field-for-field
/// like the generated `CopyValues`.
pub struct WorkspaceNavMeshBuildSettingsOverlay {
    pub source: ExportAssetIdentity,
    pub settings: WorkspaceNavMeshBuildSettingsSource,
}

pub enum WorkspaceNavMeshBuildSettingsSource {
    /// Pre-5.6 `m_NavMeshParams` floats, applied over the standalone defaults exactly like
    /// the frozen `SetValues(INavMeshParams)`.
    NavMeshParams {
        walkable_radius: f32,
        walkable_height: f32,
        walkable_climb: f32,
        tile_size: f32,
        cell_size: f32,
    },
    /// 5.6+ `m_NavMeshBuildSettings` struct, copied by field name.
    BuildSettings(SerializedValue),
}

/// Resolve the class-238 `NavMeshData` behind a class-196 `NavMeshSettings.m_NavMeshData`
/// pointer through the reference graph and the decoded object workspace.
///
/// Returns `None` when the pointer is absent, null, or dangles (the frozen `NavMeshDataP`
/// is null in all three cases, so the standalone defaults apply). Any other failure to
/// determine the source values is an explicit error rather than silently writing defaults.
pub fn workspace_nav_mesh_build_settings_overlay(
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    object: &WorkspaceObject,
    source: ExportAssetIdentity,
) -> Result<Option<WorkspaceNavMeshBuildSettingsOverlay>, String> {
    if source.class_id != 196 || object.factory.class_id != 196 {
        return Err(format!(
            "NavMesh build-settings overlay received source class {} and workspace class {}",
            source.class_id, object.factory.class_id
        ));
    }
    let payload = object
        .payload
        .as_ref()
        .map_err(|error| format!("NavMeshSettings payload could not be decoded: {error}"))?;
    let nav_mesh_data = payload
        .root
        .unique_field_in_base_chain("m_NavMeshData")
        .map_err(|error| format!("could not inspect NavMeshSettings.m_NavMeshData: {error}"))?;
    let Some(nav_mesh_data) = nav_mesh_data else {
        return Ok(None);
    };
    let SerializedValue::PPtr { file_id, path_id } = nav_mesh_data else {
        return Err(format!(
            "NavMeshSettings.m_NavMeshData has value {nav_mesh_data:?}, expected a PPtr"
        ));
    };
    if *path_id == 0 {
        return Ok(None);
    }
    let field_path = field_path_in_base_chain(&payload.root, "m_NavMeshData")?
        .ok_or_else(|| "NavMeshSettings.m_NavMeshData has no serialized field path".to_owned())?;
    let reference = unique_reference_for_field(
        reference_graph,
        object.file_index,
        object.object_index,
        &field_path,
        *file_id,
        *path_id,
    )?;
    let (data_file_index, data_object_index) = match &reference.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => (*target_file_index, *target_object_index),
        MultiFilePPtrResolution::Null => return Ok(None),
        resolution => {
            return Err(format!(
                "NavMeshSettings.m_NavMeshData at {field_path:?} did not resolve to one loaded object: {resolution:?}"
            ));
        }
    };
    if reference_graph.object_class_id(data_file_index, data_object_index) != Some(238) {
        return Err(format!(
            "NavMeshSettings.m_NavMeshData resolves to class {:?}, expected NavMeshData class 238",
            reference_graph.object_class_id(data_file_index, data_object_index)
        ));
    }
    if reference_graph.object_scan_is_complete(data_file_index, data_object_index) != Some(true) {
        return Err(format!(
            "NavMeshData file {data_file_index} object {data_object_index} has an incomplete reference scan"
        ));
    }
    let data = objects.get(data_file_index, data_object_index).ok_or_else(|| {
        format!(
            "NavMeshData file {data_file_index} object {data_object_index} is absent from the decoded object workspace"
        )
    })?;
    let data_payload = data
        .payload
        .as_ref()
        .map_err(|error| format!("NavMeshData payload could not be decoded: {error}"))?;
    if let Some(params) = data_payload
        .root
        .unique_field_in_base_chain("m_NavMeshParams")
        .map_err(|error| format!("could not inspect NavMeshData.m_NavMeshParams: {error}"))?
    {
        let SerializedValue::Struct(_) = params else {
            return Err(format!(
                "NavMeshData.m_NavMeshParams has value {params:?}, expected a struct"
            ));
        };
        return Ok(Some(WorkspaceNavMeshBuildSettingsOverlay {
            source,
            settings: WorkspaceNavMeshBuildSettingsSource::NavMeshParams {
                walkable_radius: nav_mesh_param(params, "walkableRadius")?,
                walkable_height: nav_mesh_param(params, "walkableHeight")?,
                walkable_climb: nav_mesh_param(params, "walkableClimb")?,
                tile_size: nav_mesh_param(params, "tileSize")?,
                cell_size: nav_mesh_param(params, "cellSize")?,
            },
        }));
    }
    let build_settings = data_payload
        .root
        .unique_field_in_base_chain("m_NavMeshBuildSettings")
        .map_err(|error| format!("could not inspect NavMeshData.m_NavMeshBuildSettings: {error}"))?
        .ok_or_else(|| {
            "NavMeshData has neither m_NavMeshParams nor m_NavMeshBuildSettings".to_owned()
        })?;
    let SerializedValue::Struct(_) = build_settings else {
        return Err(format!(
            "NavMeshData.m_NavMeshBuildSettings has value {build_settings:?}, expected a struct"
        ));
    };
    Ok(Some(WorkspaceNavMeshBuildSettingsOverlay {
        source,
        settings: WorkspaceNavMeshBuildSettingsSource::BuildSettings(build_settings.clone()),
    }))
}

fn nav_mesh_param(params: &SerializedValue, name: &str) -> Result<f32, String> {
    let SerializedValue::Struct(fields) = params else {
        return Err(format!(
            "NavMeshData.m_NavMeshParams has value {params:?}, expected a struct"
        ));
    };
    let field = fields
        .iter()
        .find(|field| field.name == name)
        .ok_or_else(|| format!("NavMeshData.m_NavMeshParams has no {name} field"))?;
    match field.value {
        SerializedValue::F32(value) => Ok(value),
        ref value => Err(format!(
            "NavMeshData.m_NavMeshParams.{name} has value {value:?}, expected float"
        )),
    }
}

/// Applies the frozen `NavMeshSettingsExtensions.ConvertToEditorFormat` behavior for the
/// serialized null-`m_NavMeshData` case: `BuildSettings.SetToDefault()`.
///
/// A non-null `m_NavMeshData` requires reading the referenced class-238 `NavMeshData` payload
/// (`NavMeshParams` or `NavMeshBuildSettings`) and is deliberately an explicit error rather than
/// silently writing the standalone defaults.
/// Applies the frozen `NavMeshSettingsExtensions.ConvertToEditorFormat` behavior.
///
/// A null (or dangling) `m_NavMeshData` writes the standalone `BuildSettings.SetToDefault()`
/// values; a resolved overlay carries the referenced class-238 `NavMeshData` values instead
/// (`NavMeshParams` mapped over those defaults, `NavMeshBuildSettings` copied field-for-field).
/// A non-null pointer with no resolved overlay is an explicit error rather than silently
/// writing the standalone defaults.
fn apply_nav_mesh_settings_editor_format(
    root: &mut SerializedValue,
    overlay: Option<&WorkspaceNavMeshBuildSettingsOverlay>,
) -> Result<(), String> {
    let non_null_data = match root
        .unique_field_in_base_chain("m_NavMeshData")
        .map_err(|error| format!("could not inspect NavMeshSettings.m_NavMeshData: {error}"))?
    {
        Some(SerializedValue::PPtr { path_id: 0, .. }) | None => false,
        Some(SerializedValue::PPtr { file_id, path_id }) => {
            if overlay.is_none() {
                return Err(format!(
                    "NavMeshSettings.m_NavMeshData is non-null (fileID {file_id}, PathID {path_id}); the referenced NavMeshData payload is not yet read"
                ));
            }
            true
        }
        Some(value) => {
            return Err(format!(
                "NavMeshSettings.m_NavMeshData has value {value:?}, expected a PPtr"
            ));
        }
    };
    let _ = non_null_data;

    let build_settings = root
        .unique_field_in_base_chain_mut("m_BuildSettings")
        .map_err(|error| format!("could not inspect NavMeshSettings.m_BuildSettings: {error}"))?
        .ok_or_else(|| "NavMeshSettings editor TypeTree has no m_BuildSettings field".to_owned())?;
    let SerializedValue::Struct(fields) = build_settings else {
        return Err(format!(
            "NavMeshSettings.m_BuildSettings has value {build_settings:?}, expected a struct"
        ));
    };
    match overlay {
        None => write_nav_mesh_build_settings_defaults(fields)?,
        Some(overlay) => match &overlay.settings {
            WorkspaceNavMeshBuildSettingsSource::NavMeshParams {
                walkable_radius,
                walkable_height,
                walkable_climb,
                tile_size,
                cell_size,
            } => {
                write_nav_mesh_build_settings_defaults(fields)?;
                set_nav_mesh_build_settings_float(fields, "agentRadius", *walkable_radius)?;
                set_nav_mesh_build_settings_float(fields, "agentHeight", *walkable_height)?;
                set_nav_mesh_build_settings_float(fields, "agentClimb", *walkable_climb)?;
                // The frozen `(int)navParams.TileSize` truncates toward zero; real tile
                // sizes are small positives where Rust `as` matches exactly.
                set_nav_mesh_build_settings_int(fields, "tileSize", *tile_size as i32)?;
                set_nav_mesh_build_settings_float(fields, "cellSize", *cell_size)?;
            }
            WorkspaceNavMeshBuildSettingsSource::BuildSettings(source) => {
                copy_nav_mesh_build_settings_fields(fields, source, "m_BuildSettings")?;
            }
        },
    }
    Ok(())
}

/// Copies a 5.6+ `m_NavMeshBuildSettings` struct field-for-field like the generated
/// `CopyValues`: every editor field takes the same-named source value, recursing into
/// nested structs such as `debug`. Source-only fields have no generated assignment and
/// stay untouched, exactly as the frozen conversion leaves them.
fn copy_nav_mesh_build_settings_fields(
    target: &mut [SerializedFieldValue],
    source: &SerializedValue,
    context: &str,
) -> Result<(), String> {
    let SerializedValue::Struct(source_fields) = source else {
        return Err(format!(
            "NavMeshData {context} has value {source:?}, expected a struct"
        ));
    };
    for target_field in target.iter_mut() {
        let Some(source_field) = source_fields
            .iter()
            .find(|field| field.name == target_field.name)
        else {
            continue;
        };
        let field_context = format!("{context}.{}", target_field.name);
        copy_nav_mesh_build_settings_value(
            &mut target_field.value,
            &source_field.value,
            &field_context,
        )?;
    }
    Ok(())
}

fn copy_nav_mesh_build_settings_value(
    target: &mut SerializedValue,
    source: &SerializedValue,
    context: &str,
) -> Result<(), String> {
    match (&mut *target, source) {
        (SerializedValue::Struct(target_fields), SerializedValue::Struct(source_fields)) => {
            for target_field in target_fields.iter_mut() {
                let Some(source_field) = source_fields
                    .iter()
                    .find(|field| field.name == target_field.name)
                else {
                    continue;
                };
                let field_context = format!("{context}.{}", target_field.name);
                copy_nav_mesh_build_settings_value(
                    &mut target_field.value,
                    &source_field.value,
                    &field_context,
                )?;
            }
            Ok(())
        }
        (SerializedValue::Bool(target), SerializedValue::Bool(source)) => {
            *target = *source;
            Ok(())
        }
        (SerializedValue::I32(target), SerializedValue::I32(source)) => {
            *target = *source;
            Ok(())
        }
        (SerializedValue::U32(target), SerializedValue::U32(source)) => {
            *target = *source;
            Ok(())
        }
        (SerializedValue::U8(target), SerializedValue::U8(source)) => {
            *target = *source;
            Ok(())
        }
        (SerializedValue::F32(target), SerializedValue::F32(source)) => {
            *target = *source;
            Ok(())
        }
        _ => Err(format!(
            "NavMeshData {context} has value {source:?}, incompatible with editor value {target:?}"
        )),
    }
}

/// Writes the frozen `NavMeshBuildSettingsExtensions.SetToDefault` values into the
/// editor `m_BuildSettings` struct: every listed field must exist with a matching scalar
/// shape, and `debug.m_Flags` is zeroed.
fn write_nav_mesh_build_settings_defaults(
    fields: &mut [SerializedFieldValue],
) -> Result<(), String> {
    // Field names follow the source editor TypeTree node names; `buildHeightMesh` is the
    // Unity 2022.3 spelling of the source `AccuratePlacement` property.
    const DEFAULTS: &[(&str, SceneEditorFormatDefault)] = &[
        ("agentTypeID", SceneEditorFormatDefault::I32(0)),
        ("agentRadius", SceneEditorFormatDefault::F32(0.5)),
        ("agentHeight", SceneEditorFormatDefault::F32(2.0)),
        ("agentSlope", SceneEditorFormatDefault::F32(45.0)),
        ("agentClimb", SceneEditorFormatDefault::F32(0.4)),
        ("ledgeDropHeight", SceneEditorFormatDefault::F32(0.0)),
        ("maxJumpAcrossDistance", SceneEditorFormatDefault::F32(0.0)),
        ("minRegionArea", SceneEditorFormatDefault::F32(2.0)),
        ("manualCellSize", SceneEditorFormatDefault::I32(0)),
        ("cellSize", SceneEditorFormatDefault::F32(1.0 / 6.0)),
        ("manualTileSize", SceneEditorFormatDefault::I32(0)),
        ("tileSize", SceneEditorFormatDefault::I32(256)),
        ("buildHeightMesh", SceneEditorFormatDefault::I32(0)),
        ("maxJobWorkers", SceneEditorFormatDefault::U32(0)),
        (
            "preserveTilesOutsideBounds",
            SceneEditorFormatDefault::I32(0),
        ),
    ];

    for (name, default) in DEFAULTS {
        let field = fields
            .iter_mut()
            .find(|field| field.name == *name)
            .ok_or_else(|| format!("NavMeshSettings.m_BuildSettings has no {name} field"))?;
        set_scene_editor_scalar_default(&mut field.value, *default, name)?;
    }

    let debug = fields
        .iter_mut()
        .find(|field| field.name == "debug")
        .ok_or_else(|| "NavMeshSettings.m_BuildSettings has no debug field".to_owned())?;
    let SerializedValue::Struct(debug_fields) = &mut debug.value else {
        return Err(format!(
            "NavMeshSettings.m_BuildSettings.debug has value {:?}, expected a struct",
            debug.value
        ));
    };
    let flags = debug_fields
        .iter_mut()
        .find(|field| field.name == "m_Flags")
        .ok_or_else(|| "NavMeshSettings.m_BuildSettings.debug has no m_Flags field".to_owned())?;
    let SerializedValue::U8(value) = &mut flags.value else {
        return Err(format!(
            "NavMeshSettings.m_BuildSettings.debug.m_Flags has value {:?}, expected UInt8",
            flags.value
        ));
    };
    *value = 0;
    Ok(())
}

fn set_nav_mesh_build_settings_float(
    fields: &mut [SerializedFieldValue],
    name: &str,
    value: f32,
) -> Result<(), String> {
    let field = fields
        .iter_mut()
        .find(|field| field.name == name)
        .ok_or_else(|| format!("NavMeshSettings.m_BuildSettings has no {name} field"))?;
    let SerializedValue::F32(target) = &mut field.value else {
        return Err(format!(
            "NavMeshSettings.m_BuildSettings.{name} has value {:?}, expected float",
            field.value
        ));
    };
    *target = value;
    Ok(())
}

fn set_nav_mesh_build_settings_int(
    fields: &mut [SerializedFieldValue],
    name: &str,
    value: i32,
) -> Result<(), String> {
    let field = fields
        .iter_mut()
        .find(|field| field.name == name)
        .ok_or_else(|| format!("NavMeshSettings.m_BuildSettings has no {name} field"))?;
    let SerializedValue::I32(target) = &mut field.value else {
        return Err(format!(
            "NavMeshSettings.m_BuildSettings.{name} has value {:?}, expected int",
            field.value
        ));
    };
    *target = value;
    Ok(())
}

/// Ports `SpriteAtlasExtensions.ConvertToEditorFormat` for the editor TypeTree.
/// The editor `packables` list is rebuilt from packed sprites, and the nested settings use the
/// generated `Initialize` defaults instead of zero-filled TypeTree defaults.
fn apply_sprite_atlas_editor_format(root: &mut SerializedValue) -> Result<(), String> {
    let packed_sprites = root
        .unique_field_in_base_chain("m_PackedSprites")
        .map_err(|error| format!("could not inspect SpriteAtlas.m_PackedSprites: {error}"))?
        .cloned();
    let editor_data = root
        .unique_field_in_base_chain_mut("m_EditorData")
        .map_err(|error| format!("could not inspect SpriteAtlas.m_EditorData: {error}"))?
        .ok_or_else(|| "SpriteAtlas editor TypeTree has no m_EditorData field".to_owned())?;

    if let Some(texture_settings) = editor_data
        .unique_field_in_base_chain_mut("textureSettings")
        .map_err(|error| format!("could not inspect SpriteAtlas textureSettings: {error}"))?
    {
        for (name, default) in [
            ("anisoLevel", SceneEditorFormatDefault::I32(1)),
            ("compressionQuality", SceneEditorFormatDefault::I32(50)),
            ("maxTextureSize", SceneEditorFormatDefault::I32(2048)),
            ("textureCompression", SceneEditorFormatDefault::I32(0)),
            ("filterMode", SceneEditorFormatDefault::I32(1)),
            ("generateMipMaps", SceneEditorFormatDefault::Bool(false)),
            ("readable", SceneEditorFormatDefault::Bool(false)),
            ("crunchedCompression", SceneEditorFormatDefault::Bool(false)),
            ("sRGB", SceneEditorFormatDefault::Bool(true)),
        ] {
            set_scene_editor_format_default(texture_settings, name, default)?;
        }
    }

    if let Some(packing_parameters) = editor_data
        .unique_field_in_base_chain_mut("packingParameters")
        .map_err(|error| format!("could not inspect SpriteAtlas packingParameters: {error}"))?
    {
        for (name, default) in [
            ("padding", SceneEditorFormatDefault::I32(2)),
            ("paddingPower", SceneEditorFormatDefault::I32(2)),
            ("blockOffset", SceneEditorFormatDefault::I32(1)),
            ("allowAlphaSplitting", SceneEditorFormatDefault::I32(0)),
            ("enableRotation", SceneEditorFormatDefault::I32(1)),
            ("enableTightPacking", SceneEditorFormatDefault::I32(1)),
        ] {
            set_scene_editor_format_default(packing_parameters, name, default)?;
        }
    }

    if let Some(packing_settings) = editor_data
        .unique_field_in_base_chain_mut("packingSettings")
        .map_err(|error| format!("could not inspect SpriteAtlas packingSettings: {error}"))?
    {
        for (name, default) in [
            ("padding", SceneEditorFormatDefault::I32(2)),
            ("blockOffset", SceneEditorFormatDefault::I32(1)),
            ("allowAlphaSplitting", SceneEditorFormatDefault::Bool(false)),
            ("enableRotation", SceneEditorFormatDefault::Bool(true)),
            ("enableTightPacking", SceneEditorFormatDefault::Bool(true)),
            ("enableAlphaDilation", SceneEditorFormatDefault::Bool(true)),
        ] {
            set_scene_editor_format_default(packing_settings, name, default)?;
        }
    }

    set_scene_editor_format_default(
        editor_data,
        "variantMultiplier",
        SceneEditorFormatDefault::F32(1.0),
    )?;
    set_scene_editor_format_default(
        editor_data,
        "bindAsDefault",
        SceneEditorFormatDefault::Bool(true),
    )?;
    if let Some(packed_sprites) = packed_sprites {
        if let Some(packables) = editor_data
            .unique_field_in_base_chain_mut("packables")
            .map_err(|error| format!("could not inspect SpriteAtlas packables: {error}"))?
        {
            *packables = packed_sprites;
        }
    }
    Ok(())
}

/// Applies the frozen `LightmapEditorSettingsExtensions.ConvertToEditorFormat`
/// behavior for `ILightingSettings`: every editor-only field receives its Unity
/// default, mirroring the source's unconditional assignments.
///
/// Field names and value shapes follow the 2022.3 editor TypeTree node names; the
/// numeric values reproduce the frozen method exactly (verified against a frozen
/// scene-settings export, which carries these defaults verbatim). Fields absent from the active
/// editor layout are left untouched, matching the versioned generated-class behavior of the source.
fn apply_lighting_settings_editor_defaults(root: &mut SerializedValue) -> Result<(), String> {
    const DEFAULTS: &[(&str, SceneEditorFormatDefault)] = &[
        ("m_BakeBackend", SceneEditorFormatDefault::I32(0)),
        ("m_LightmapMaxSize", SceneEditorFormatDefault::I32(4096)),
        ("m_BakeResolution", SceneEditorFormatDefault::F32(40.0)),
        ("m_Padding", SceneEditorFormatDefault::I32(2)),
        ("m_LightmapCompression", SceneEditorFormatDefault::I32(3)),
        ("m_AO", SceneEditorFormatDefault::Bool(false)),
        ("m_AOMaxDistance", SceneEditorFormatDefault::F32(1.0)),
        ("m_CompAOExponent", SceneEditorFormatDefault::F32(1.0)),
        ("m_CompAOExponentDirect", SceneEditorFormatDefault::F32(0.0)),
        ("m_ExtractAO", SceneEditorFormatDefault::Bool(false)),
        ("m_MixedBakeMode", SceneEditorFormatDefault::I32(2)),
        ("m_LightmapsBakeMode", SceneEditorFormatDefault::I32(1)),
        ("m_FilterMode", SceneEditorFormatDefault::I32(1)),
        (
            "m_ExportTrainingData",
            SceneEditorFormatDefault::Bool(false),
        ),
        (
            "m_TrainingDataDestination",
            SceneEditorFormatDefault::String("TrainingData"),
        ),
        ("m_RealtimeResolution", SceneEditorFormatDefault::F32(2.0)),
        ("m_ForceWhiteAlbedo", SceneEditorFormatDefault::Bool(false)),
        ("m_ForceUpdates", SceneEditorFormatDefault::Bool(false)),
        ("m_FinalGather", SceneEditorFormatDefault::Bool(false)),
        ("m_FinalGatherRayCount", SceneEditorFormatDefault::I32(256)),
        (
            "m_FinalGatherFiltering",
            SceneEditorFormatDefault::Bool(true),
        ),
        ("m_PVRCulling", SceneEditorFormatDefault::Bool(true)),
        ("m_PVRSampling", SceneEditorFormatDefault::I32(1)),
        ("m_PVRDirectSampleCount", SceneEditorFormatDefault::I32(32)),
        ("m_PVRSampleCount", SceneEditorFormatDefault::I32(500)),
        (
            "m_PVREnvironmentSampleCount",
            SceneEditorFormatDefault::I32(256),
        ),
        (
            "m_PVREnvironmentReferencePointCount",
            SceneEditorFormatDefault::I32(2048),
        ),
        (
            "m_LightProbeSampleCountMultiplier",
            SceneEditorFormatDefault::F32(4.0),
        ),
        ("m_PVRBounces", SceneEditorFormatDefault::I32(2)),
        ("m_PVRMinBounces", SceneEditorFormatDefault::I32(2)),
        (
            "m_PVREnvironmentImportanceSampling",
            SceneEditorFormatDefault::Bool(true),
        ),
        ("m_PVRFilteringMode", SceneEditorFormatDefault::I32(1)),
        ("m_PVRDenoiserTypeDirect", SceneEditorFormatDefault::I32(1)),
        (
            "m_PVRDenoiserTypeIndirect",
            SceneEditorFormatDefault::I32(1),
        ),
        ("m_PVRDenoiserTypeAO", SceneEditorFormatDefault::I32(1)),
        ("m_PVRFilterTypeDirect", SceneEditorFormatDefault::I32(0)),
        ("m_PVRFilterTypeIndirect", SceneEditorFormatDefault::I32(0)),
        ("m_PVRFilterTypeAO", SceneEditorFormatDefault::I32(0)),
        (
            "m_PVRFilteringGaussRadiusDirect",
            SceneEditorFormatDefault::F32(1.0),
        ),
        (
            "m_PVRFilteringGaussRadiusIndirect",
            SceneEditorFormatDefault::F32(5.0),
        ),
        (
            "m_PVRFilteringGaussRadiusAO",
            SceneEditorFormatDefault::F32(2.0),
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
            "m_PVRFilteringAtrousPositionSigmaAO",
            SceneEditorFormatDefault::F32(1.0),
        ),
        ("m_PVRTiledBaking", SceneEditorFormatDefault::I32(0)),
        (
            "m_NumRaysToShootPerTexel",
            SceneEditorFormatDefault::I32(-1),
        ),
        (
            "m_RespectSceneVisibilityWhenBakingGI",
            SceneEditorFormatDefault::Bool(false),
        ),
    ];
    for (name, default) in DEFAULTS {
        set_scene_editor_format_default(root, name, *default)?;
    }
    Ok(())
}

fn set_scene_editor_scalar_default(
    value: &mut SerializedValue,
    default: SceneEditorFormatDefault,
    field_name: &str,
) -> Result<(), String> {
    match (value, default) {
        (SerializedValue::Bool(value), SceneEditorFormatDefault::Bool(default)) => *value = default,
        (SerializedValue::I32(value), SceneEditorFormatDefault::I32(default)) => *value = default,
        (SerializedValue::U32(value), SceneEditorFormatDefault::U32(default)) => *value = default,
        (SerializedValue::F32(value), SceneEditorFormatDefault::F32(default)) => *value = default,
        (value, default) => {
            return Err(format!(
                "editor default for {field_name} has incompatible value shape {value:?} vs {default:?}"
            ));
        }
    }
    Ok(())
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
            expected_class_id: request.expected_class_id,
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
        game_object_static_mark: false,
        generated_component_overlay: None,
        generated_prefab_internal: None,
        generated_lighting_data: None,
        sprite_atlas_overlay: None,
        nav_mesh_build_settings_overlay: None,
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
    nav_mesh_build_settings_overlay: Option<WorkspaceNavMeshBuildSettingsOverlay>,
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
        nav_mesh_build_settings_overlay,
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
    nav_mesh_build_settings_overlay: Option<WorkspaceNavMeshBuildSettingsOverlay>,
) -> Result<Option<UnityYamlDocument>, UnityYamlWorkspaceExportError> {
    workspace_object_to_unity_yaml_document_with_atlas_overlay(
        workspace,
        reference_graph,
        pointer_map,
        object,
        source,
        file_id,
        current_collection_index,
        missing_reference_guid,
        limits,
        processed_animation_root,
        streamed_resources,
        animation_clip_editor_format,
        None,
        nav_mesh_build_settings_overlay,
    )
}

/// Convert one workspace object with an optional prebuilt sprite-atlas overlay.
///
/// Crate-internal planning routes resolve atlas-packed sprites through
/// [`workspace_sprite_atlas_yaml_overlay`](super::sprite_atlas::workspace_sprite_atlas_yaml_overlay)
/// before conversion; the overlay type stays crate-internal so the public
/// conversion surface does not expose atlas-internals to external callers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn workspace_object_to_unity_yaml_document_with_atlas_overlay(
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
    sprite_atlas_overlay: Option<WorkspaceSpriteAtlasYamlOverlay>,
    nav_mesh_build_settings_overlay: Option<WorkspaceNavMeshBuildSettingsOverlay>,
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
        game_object_static_mark: false,
        generated_component_overlay: None,
        generated_prefab_internal: None,
        generated_lighting_data: None,
        sprite_atlas_overlay: sprite_atlas_overlay.as_ref(),
        nav_mesh_build_settings_overlay: nav_mesh_build_settings_overlay.as_ref(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::pointer_map::ExportPointerGuid;

    #[test]
    fn unity_2022_3_render_texture_and_flare_require_editor_layouts() {
        let version = FactoryUnityVersion::parse("2022.3.22f1").unwrap();
        for (class_id, expected_name) in [(86, "CustomRenderTexture"), (121, "Flare")] {
            let (release, editor) = reference_class_layouts()
                .tpk_trees(class_id, &version)
                .unwrap()
                .expect("frozen TPK must contain the resource layout");
            assert_eq!(editor.nodes[0].type_name.as_deref(), Some(expected_name));
            for field in [
                "m_ObjectHideFlags",
                "m_CorrespondingSourceObject",
                "m_PrefabInstance",
                "m_PrefabAsset",
            ] {
                assert!(editor
                    .nodes
                    .iter()
                    .any(|node| node.name.as_deref() == Some(field)));
                assert!(!release
                    .nodes
                    .iter()
                    .any(|node| node.name.as_deref() == Some(field)));
            }
            if class_id == 86 {
                assert!(editor.nodes.iter().any(|node| {
                    node.name.as_deref() == Some("m_ImageContentsHash")
                        && node.type_name.as_deref() == Some("Hash128")
                }));
                assert!(!release
                    .nodes
                    .iter()
                    .any(|node| { node.name.as_deref() == Some("m_ImageContentsHash") }));
                // Hash128's exported serializedVersion comes from its Unity
                // version contract, independently of the TPK node version.
                let hints = UnityYamlObjectConversionHints::from_type_tree(
                    &editor,
                    SceneUnityVersion::parse("2022.3.22f1").unwrap(),
                    UnityYamlObjectConversionLimits::default(),
                )
                .unwrap();
                assert_eq!(
                    hints
                        .hash128_schemas
                        .get("m_ImageContentsHash")
                        .map(|schema| schema.serialized_version),
                    Some(2)
                );
            }
            assert!(should_use_scene_editor_type_tree(
                class_id,
                Some("2022.3.22f1")
            ));
        }
    }
    #[test]
    fn unity_2022_3_scene_classes_use_editor_type_trees() {
        for class_id in [
            1,
            4,
            20,
            43,
            86,
            121,
            64,
            65,
            28,
            72,
            136,
            205,
            222,
            223,
            224,
            320,
            28,
            72,
            81,
            193,
            196,
            90,
            23,
            33,
            82,
            95,
            213,
            108,
            135,
            319,
            137,
            198,
            199,
            54,
            84,
            96,
            153,
            120,
            212,
            111,
            123,
            124,
            850_595_691,
            895_512_359,
            1_773_428_102,
            1_818_360_608,
            1_818_360_609,
            258,
            218,
            134,
            102,
            215,
            225,
            29,
            119,
            128,
            146,
            169,
            182,
            195,
            221,
            238,
            363,
            167,
            164,
            259,
            1_953_259_897,
            1_183_024_399,
            1_818_360_610,
        ] {
            assert!(should_use_scene_editor_type_tree(
                class_id,
                Some("2022.3.22f1")
            ));
        }
        assert!(!should_use_scene_editor_type_tree(1, Some("2022.2.0f1")));
        assert!(!should_use_scene_editor_type_tree(24, Some("2022.3.22f1")));
        assert!(!should_use_scene_editor_type_tree(20, Some("2022.2.0f1")));
        assert!(!should_use_scene_editor_type_tree(137, Some("5.6.0f1")));
        assert!(!should_use_scene_editor_type_tree(43, Some("5.6.0f1")));
        assert!(!should_use_scene_editor_type_tree(90, Some("5.6.0f1")));
        assert!(should_use_scene_editor_type_tree(1, Some("5.6.0f1")));
    }

    #[test]
    fn unity_5_6_texture2d_uses_editor_tree_fields_and_hash_schema() {
        let version = FactoryUnityVersion::parse("5.6.0f1").unwrap();
        let (release, editor) = reference_class_layouts()
            .tpk_trees(28, &version)
            .unwrap()
            .expect("Unity 5.6 source TPK must contain the Texture2D layout");

        assert!(should_use_scene_editor_type_tree(28, Some("5.6.0f1")));
        assert_eq!(editor.nodes[0].type_name.as_deref(), Some("Texture2D"));
        for field in [
            "m_ObjectHideFlags",
            "m_PrefabParentObject",
            "m_PrefabInternal",
            "m_ImageContentsHash",
            "m_AlphaIsTransparency",
        ] {
            assert!(
                editor
                    .nodes
                    .iter()
                    .any(|node| node.name.as_deref() == Some(field)),
                "Unity 5.6 Texture2D editor tree must contain {field}"
            );
            assert!(
                !release
                    .nodes
                    .iter()
                    .any(|node| node.name.as_deref() == Some(field)),
                "Unity 5.6 Texture2D release tree must omit {field}"
            );
        }

        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &editor,
            SceneUnityVersion::parse("5.6.0f1").unwrap(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            hints
                .hash128_schemas
                .get("m_ImageContentsHash")
                .map(|schema| schema.serialized_version),
            Some(2)
        );
        assert!(should_preserve_type_tree_field_names(
            28,
            &FactoryAssetKind::Generated("Texture2D_5_5".to_owned()),
            true,
            false,
            false,
        ));
    }

    /// Evidence for the fourth-world editor-route additions: every class added
    /// there must carry the editor base the reference output shows in its
    /// editor tree and lack all four base fields in the release tree, which is
    /// the segment's route criterion. `OcclusionCullingSettings` is the one
    /// class whose editor tree carries only `m_ObjectHideFlags` (matching the
    /// frozen scene document, which has no prefab fields at all).
    #[test]
    fn fourth_world_editor_route_additions_carry_the_full_editor_base() {
        let version = FactoryUnityVersion::parse("2022.3.22f1").unwrap();
        const BASE: [&str; 4] = [
            "m_ObjectHideFlags",
            "m_CorrespondingSourceObject",
            "m_PrefabInstance",
            "m_PrefabAsset",
        ];
        for class_id in [
            29,
            119,
            128,
            146,
            169,
            182,
            195,
            221,
            238,
            363,
            167,
            164,
            259,
            1_953_259_897,
            1_183_024_399,
            1_818_360_610,
        ] {
            let (release, editor) = reference_class_layouts()
                .tpk_trees(class_id, &version)
                .unwrap()
                .unwrap_or_else(|| panic!("class {class_id} must have 2022.3 TPK trees"));
            for field in BASE {
                assert!(
                    !release
                        .nodes
                        .iter()
                        .any(|node| node.name.as_deref() == Some(field)),
                    "class {class_id} release tree must omit {field}"
                );
            }
            assert!(
                editor
                    .nodes
                    .iter()
                    .any(|node| node.name.as_deref() == Some("m_ObjectHideFlags")),
                "class {class_id} editor tree must carry m_ObjectHideFlags"
            );
            if class_id != 29 {
                for field in &BASE[1..] {
                    assert!(
                        editor
                            .nodes
                            .iter()
                            .any(|node| node.name.as_deref() == Some(field)),
                        "class {class_id} editor tree must carry {field}"
                    );
                }
            }
            assert!(should_use_scene_editor_type_tree(
                class_id,
                Some("2022.3.22f1")
            ));
        }
    }

    #[test]
    fn unity_2022_3_skinned_mesh_renderer_has_editor_tree_fields() {
        let version = FactoryUnityVersion::parse("2022.3.22f1").unwrap();
        let (_, editor_tree) = reference_class_layouts()
            .tpk_trees(137, &version)
            .unwrap()
            .expect("Unity 2022.3 source TPK must contain the SkinnedMeshRenderer layout");

        for field in [
            "m_ObjectHideFlags",
            "m_CorrespondingSourceObject",
            "m_PrefabInstance",
            "m_PrefabAsset",
        ] {
            assert!(
                editor_tree
                    .nodes
                    .iter()
                    .any(|node| node.name.as_deref() == Some(field)),
                "SkinnedMeshRenderer editor TypeTree has no {field} field"
            );
        }
        assert!(
            !editor_tree
                .nodes
                .iter()
                .any(|node| node.name.as_deref() == Some("m_HideFlags")),
            "SkinnedMeshRenderer editor TypeTree unexpectedly has release m_HideFlags"
        );
    }

    #[test]
    fn unity_2022_3_editor_tree_classes_have_editor_base_fields() {
        let version = FactoryUnityVersion::parse("2022.3.22f1").unwrap();
        let expected_fields = [
            "m_ObjectHideFlags",
            "m_CorrespondingSourceObject",
            "m_PrefabInstance",
            "m_PrefabAsset",
        ];

        for class_id in [
            20,
            43,
            86,
            121,
            64,
            65,
            28,
            72,
            136,
            205,
            222,
            223,
            224,
            320,
            81,
            193,
            90,
            82,
            95,
            213,
            135,
            319,
            1_773_428_102,
            198,
            199,
            54,
            84,
            96,
            153,
            120,
            212,
            123,
            124,
            850_595_691,
            895_512_359,
            1_818_360_608,
            1_818_360_609,
            258,
            218,
            134,
            102,
            215,
            225,
        ] {
            let (_, editor_tree) = reference_class_layouts()
                .tpk_trees(class_id, &version)
                .unwrap()
                .unwrap_or_else(|| {
                    panic!("Unity 2022.3 source TPK has no editor tree for class {class_id}")
                });
            for field in expected_fields {
                assert!(
                    editor_tree
                        .nodes
                        .iter()
                        .any(|node| node.name.as_deref() == Some(field)),
                    "class {class_id} editor TypeTree has no {field} field"
                );
            }
            assert!(
                !editor_tree
                    .nodes
                    .iter()
                    .any(|node| node.name.as_deref() == Some("m_HideFlags")),
                "class {class_id} editor TypeTree unexpectedly has release m_HideFlags"
            );
        }
    }

    #[test]
    fn unity_2022_3_manager_classes_have_object_hide_flags_only() {
        let version = FactoryUnityVersion::parse("2022.3.22f1").unwrap();
        for class_id in [104, 157, 196] {
            let (_, editor_tree) = reference_class_layouts()
                .tpk_trees(class_id, &version)
                .unwrap()
                .unwrap_or_else(|| {
                    panic!("Unity 2022.3 source TPK has no editor tree for class {class_id}")
                });
            assert!(
                editor_tree
                    .nodes
                    .iter()
                    .any(|node| node.name.as_deref() == Some("m_ObjectHideFlags")),
                "manager class {class_id} editor TypeTree has no m_ObjectHideFlags field"
            );
            for field in [
                "m_CorrespondingSourceObject",
                "m_PrefabInstance",
                "m_PrefabAsset",
            ] {
                assert!(
                    !editor_tree
                        .nodes
                        .iter()
                        .any(|node| node.name.as_deref() == Some(field)),
                    "manager class {class_id} editor TypeTree unexpectedly has {field}"
                );
            }
        }
    }

    #[test]
    fn unity_2022_3_mesh_editor_tree_has_optimization_field() {
        let version = FactoryUnityVersion::parse("2022.3.22f1").unwrap();
        let (_, editor_tree) = reference_class_layouts()
            .tpk_trees(43, &version)
            .unwrap()
            .expect("Unity 2022.3 source TPK must contain the Mesh editor layout");
        assert!(editor_tree
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some("m_MeshOptimizationFlags")));
        assert!(editor_tree.nodes.iter().enumerate().any(|(index, node)| {
            node.type_name.as_deref() == Some("TypelessData")
                && node.name.as_deref() == Some("m_DataSize")
                && editor_tree.nodes.get(index + 1).is_some_and(|child| {
                    child.name.as_deref() == Some("size")
                        && child.type_name.as_deref() == Some("int")
                })
        }));

        let payload = vec![0x6e, 0x12, 0x03, 0x3b];
        let mut root = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "m_VertexData".to_owned(),
                value: SerializedValue::Struct(vec![SerializedFieldValue {
                    name: "m_DataSize".to_owned(),
                    value: SerializedValue::Bytes(payload.clone()),
                }]),
            },
            SerializedFieldValue {
                name: "m_MeshOptimizationFlags".to_owned(),
                value: SerializedValue::I32(0),
            },
        ]);
        apply_mesh_editor_format(&mut root).unwrap();
        assert!(matches!(
            root.unique_field("m_MeshOptimizationFlags").unwrap(),
            Some(SerializedValue::I32(-1))
        ));
        let vertex_data = root
            .unique_field("m_VertexData")
            .unwrap()
            .expect("Mesh has vertex data");
        assert!(matches!(
            vertex_data.unique_field("m_DataSize").unwrap(),
            Some(SerializedValue::I32(4))
        ));
        assert_eq!(
            vertex_data.unique_field("_typelessdata").unwrap(),
            Some(&SerializedValue::Bytes(payload))
        );
    }

    #[test]
    fn editor_type_tree_fields_are_preserved_inside_source_arrays() {
        use crate::serialized_file::metadata::TypeTreeNode;

        let node = |level, type_name: &str, name: &str| TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size: -1,
            index: 0,
            meta_flags: 0,
            reference_type_hash: None,
        };
        let editor_tree = TypeTree {
            nodes: vec![
                node(0, "Root", "Base"),
                node(1, "vector", "curves"),
                node(2, "Array", "Array"),
                node(3, "int", "size"),
                node(3, "Keyframe", "data"),
                node(4, "float", "time"),
                node(4, "int", "tangentMode"),
                node(4, "int", "weightedMode"),
            ],
            string_buffer: Vec::new(),
        };
        let source = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "curves".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "time".to_owned(),
                    value: SerializedValue::F32(1.25),
                },
                SerializedFieldValue {
                    name: "weightedMode".to_owned(),
                    value: SerializedValue::I32(2),
                },
            ])]),
        }]);
        let mut defaults = default_value_for_type_tree_node(
            &editor_tree,
            0,
            0,
            ObjectDefaultValueLimits::default(),
        )
        .unwrap();

        merge_editor_default_value(&mut defaults, &source, &editor_tree, 0, 0, "").unwrap();

        let SerializedValue::Struct(root_fields) = defaults else {
            panic!("editor default root is not a struct");
        };
        let SerializedValue::Array(curves) = &root_fields[0].value else {
            panic!("curve field is not an array");
        };
        let SerializedValue::Struct(keyframe) = &curves[0] else {
            panic!("curve array element is not a struct");
        };
        let fields: Vec<_> = keyframe
            .iter()
            .map(|field| (field.name.as_str(), &field.value))
            .collect();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], ("time", &SerializedValue::F32(1.25)));
        assert_eq!(fields[1], ("tangentMode", &SerializedValue::I32(0)));
        assert_eq!(fields[2], ("weightedMode", &SerializedValue::I32(2)));
    }

    #[test]
    fn skinned_mesh_renderer_editor_tree_preserves_editor_field_names() {
        let generated = FactoryAssetKind::Generated("SkinnedMeshRenderer_2021".to_owned());
        assert!(should_preserve_type_tree_field_names(
            137, &generated, true, false, false
        ));
        assert!(!should_preserve_type_tree_field_names(
            137, &generated, false, false, false
        ));
    }

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

    #[test]
    fn renderer_editor_format_overwrites_gi_defaults_for_any_renderer() {
        // The frozen `EditorFormatConverter.Convert(IRenderer)` overwrites these
        // fields unconditionally on every renderer; 云栖仙府 TrailRenderer (96)
        // exports zeros without it.
        let mut root = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "m_ScaleInLightmap".to_owned(),
                value: SerializedValue::F32(0.0),
            },
            SerializedFieldValue {
                name: "m_ReceiveGI".to_owned(),
                value: SerializedValue::I32(0),
            },
            SerializedFieldValue {
                name: "m_SelectedEditorRenderState".to_owned(),
                value: SerializedValue::I32(0),
            },
            SerializedFieldValue {
                name: "m_MinimumChartSize".to_owned(),
                value: SerializedValue::I32(0),
            },
            SerializedFieldValue {
                name: "m_AutoUVMaxDistance".to_owned(),
                value: SerializedValue::F32(0.0),
            },
            SerializedFieldValue {
                name: "m_AutoUVMaxAngle".to_owned(),
                value: SerializedValue::F32(0.0),
            },
        ]);
        apply_renderer_editor_format(&mut root).unwrap();
        let field = |name: &str| {
            root.unique_field(name)
                .unwrap()
                .unwrap_or_else(|| panic!("renderer root has no {name}"))
                .clone()
        };
        assert_eq!(field("m_ScaleInLightmap"), SerializedValue::F32(1.0));
        assert_eq!(field("m_ReceiveGI"), SerializedValue::I32(1));
        assert_eq!(
            field("m_SelectedEditorRenderState"),
            SerializedValue::I32(3)
        );
        assert_eq!(field("m_MinimumChartSize"), SerializedValue::I32(4));
        assert_eq!(field("m_AutoUVMaxDistance"), SerializedValue::F32(0.5));
        assert_eq!(field("m_AutoUVMaxAngle"), SerializedValue::F32(89.0));
    }

    #[test]
    fn sprite_geometry_recomputes_rect_pivot_and_offsets_bit_exact() {
        fn vec2(x: f32, y: f32) -> SerializedValue {
            SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "x".to_owned(),
                    value: SerializedValue::F32(x),
                },
                SerializedFieldValue {
                    name: "y".to_owned(),
                    value: SerializedValue::F32(y),
                },
            ])
        }
        fn rect(x: f32, y: f32, width: f32, height: f32) -> SerializedValue {
            SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "x".to_owned(),
                    value: SerializedValue::F32(x),
                },
                SerializedFieldValue {
                    name: "y".to_owned(),
                    value: SerializedValue::F32(y),
                },
                SerializedFieldValue {
                    name: "width".to_owned(),
                    value: SerializedValue::F32(width),
                },
                SerializedFieldValue {
                    name: "height".to_owned(),
                    value: SerializedValue::F32(height),
                },
            ])
        }
        // Frozen `Cookie.asset` vectors: the bundle stores rect height one ulp above
        // 848 (`0x44540001`) with an exact 0.5 pivot, while the frozen export carries
        // rect height 848, pivot.y `0.50000006`, and offset.y `5.054474E-05`.
        let source = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "m_Rect".to_owned(),
                value: rect(0.0, 0.0, 1024.0, f32::from_bits(0x4454_0001)),
            },
            SerializedFieldValue {
                name: "m_Pivot".to_owned(),
                value: vec2(0.5, 0.5),
            },
            SerializedFieldValue {
                name: "m_Border".to_owned(),
                value: SerializedValue::Struct(vec![
                    SerializedFieldValue {
                        name: "x".to_owned(),
                        value: SerializedValue::F32(0.0),
                    },
                    SerializedFieldValue {
                        name: "y".to_owned(),
                        value: SerializedValue::F32(0.0),
                    },
                    SerializedFieldValue {
                        name: "z".to_owned(),
                        value: SerializedValue::F32(0.0),
                    },
                    SerializedFieldValue {
                        name: "w".to_owned(),
                        value: SerializedValue::F32(0.0),
                    },
                ]),
            },
            SerializedFieldValue {
                name: "m_Offset".to_owned(),
                value: vec2(0.0, 0.0),
            },
            SerializedFieldValue {
                name: "m_RD".to_owned(),
                value: SerializedValue::Struct(vec![
                    SerializedFieldValue {
                        name: "textureRect".to_owned(),
                        value: rect(0.0, 0.0, 1024.0, 848.0),
                    },
                    SerializedFieldValue {
                        name: "textureRectOffset".to_owned(),
                        value: vec2(0.0, 0.0),
                    },
                ]),
            },
        ]);
        let mut root = source.clone();
        // The merged editor root also carries the null atlas pointer.
        if let SerializedValue::Struct(fields) = &mut root {
            fields.push(SerializedFieldValue {
                name: "m_SpriteAtlas".to_owned(),
                value: SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                },
            });
        }
        apply_sprite_geometry(&source, &mut root).unwrap();
        let height = sprite_f32_field(
            sprite_struct_field(&root, "Sprite", "m_Rect").unwrap(),
            "m_Rect",
            "height",
        )
        .unwrap();
        assert_eq!(height.to_bits(), 0x4454_0000, "rect height");
        let pivot_y = sprite_f32_field(
            sprite_struct_field(&root, "Sprite", "m_Pivot").unwrap(),
            "m_Pivot",
            "y",
        )
        .unwrap();
        assert_eq!(pivot_y.to_bits(), 0x3F00_0001, "pivot.y");
        assert_eq!(pivot_y, 0.50000006f32);
        let offset_y = sprite_f32_field(
            sprite_struct_field(&root, "Sprite", "m_Offset").unwrap(),
            "m_Offset",
            "y",
        )
        .unwrap();
        assert_eq!(offset_y, 5.054474E-05f32, "offset.y");
        let texture_offset = read_vec2_field(
            sprite_struct_field(&root, "Sprite", "m_RD").unwrap(),
            "textureRectOffset",
        )
        .unwrap();
        assert_eq!(texture_offset, (0.0, 0.0));
    }

    #[test]
    fn sprite_geometry_uses_overlay_resolved_atlas_render_data() {
        fn vec2(x: f32, y: f32) -> SerializedValue {
            SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "x".to_owned(),
                    value: SerializedValue::F32(x),
                },
                SerializedFieldValue {
                    name: "y".to_owned(),
                    value: SerializedValue::F32(y),
                },
            ])
        }
        fn rect(x: f32, y: f32, width: f32, height: f32) -> SerializedValue {
            SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "x".to_owned(),
                    value: SerializedValue::F32(x),
                },
                SerializedFieldValue {
                    name: "y".to_owned(),
                    value: SerializedValue::F32(y),
                },
                SerializedFieldValue {
                    name: "width".to_owned(),
                    value: SerializedValue::F32(width),
                },
                SerializedFieldValue {
                    name: "height".to_owned(),
                    value: SerializedValue::F32(height),
                },
            ])
        }
        // An overlay-resolved source carries the atlas render data in `m_RD`
        // with a cleared atlas pointer, exactly as the sprite-atlas overlay
        // rebuilds it. The sprite's own `m_Rect` differs from the atlas
        // rectangle so the test proves the atlas values drive the output.
        let source = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "m_Rect".to_owned(),
                value: rect(0.0, 0.0, 1024.0, 848.0),
            },
            SerializedFieldValue {
                name: "m_Pivot".to_owned(),
                value: vec2(0.5, 0.5),
            },
            SerializedFieldValue {
                name: "m_Border".to_owned(),
                value: SerializedValue::Struct(vec![
                    SerializedFieldValue {
                        name: "x".to_owned(),
                        value: SerializedValue::F32(0.0),
                    },
                    SerializedFieldValue {
                        name: "y".to_owned(),
                        value: SerializedValue::F32(0.0),
                    },
                    SerializedFieldValue {
                        name: "z".to_owned(),
                        value: SerializedValue::F32(0.0),
                    },
                    SerializedFieldValue {
                        name: "w".to_owned(),
                        value: SerializedValue::F32(0.0),
                    },
                ]),
            },
            SerializedFieldValue {
                name: "m_Offset".to_owned(),
                value: vec2(0.0, 0.0),
            },
            SerializedFieldValue {
                name: "m_RD".to_owned(),
                value: SerializedValue::Struct(vec![
                    SerializedFieldValue {
                        name: "textureRect".to_owned(),
                        value: rect(10.0, 20.0, 256.0, 128.0),
                    },
                    SerializedFieldValue {
                        name: "textureRectOffset".to_owned(),
                        value: vec2(2.0, 3.0),
                    },
                ]),
            },
            SerializedFieldValue {
                name: "m_SpriteAtlas".to_owned(),
                value: SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                },
            },
        ]);
        let mut root = source.clone();
        apply_sprite_geometry(&source, &mut root).unwrap();
        let out_rect = read_rect_field(&root, "m_Rect").unwrap();
        assert_eq!(out_rect, (10.0, 20.0, 256.0, 128.0));
        let out_pivot = read_vec2_field(&root, "m_Pivot").unwrap();
        assert_eq!(out_pivot, (1.9921875, 3.2890625));
        let out_offset = read_vec2_field(&root, "m_Offset").unwrap();
        assert_eq!(out_offset, (382.0, 357.0));
        let out_texture_offset = read_vec2_field(
            sprite_struct_field(&root, "Sprite", "m_RD").unwrap(),
            "textureRectOffset",
        )
        .unwrap();
        assert_eq!(out_texture_offset, (0.0, 0.0));
    }

    #[test]
    fn sprite_geometry_rejects_unresolved_atlas_pointers_loudly() {
        let atlas_pointer = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_SpriteAtlas".to_owned(),
            value: SerializedValue::PPtr {
                file_id: 0,
                path_id: 793,
            },
        }]);
        let null_pointer = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_SpriteAtlas".to_owned(),
            value: SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            },
        }]);
        let source_error =
            apply_sprite_geometry(&atlas_pointer, &mut null_pointer.clone()).unwrap_err();
        assert!(
            source_error.contains("RenderDataMap resolution is not implemented"),
            "unexpected source-atlas error: {source_error}"
        );
        let root_error =
            apply_sprite_geometry(&null_pointer, &mut atlas_pointer.clone()).unwrap_err();
        assert!(
            root_error.contains("RenderDataMap resolution is not implemented"),
            "unexpected root-atlas error: {root_error}"
        );
    }

    #[test]
    fn terrain_editor_format_stamps_fixed_lightmap_scale() {
        let mut root = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_ScaleInLightmap".to_owned(),
            value: SerializedValue::F32(0.0),
        }]);
        apply_terrain_editor_format(&mut root).unwrap();
        let SerializedValue::Struct(fields) = &root else {
            panic!("terrain test root is not a struct");
        };
        assert_eq!(fields[0].value, SerializedValue::F32(0.0512));
    }

    fn batch_info_root(first: i32, count: i32) -> SerializedValue {
        SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_StaticBatchInfo".to_owned(),
            value: SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "firstSubMesh".to_owned(),
                    value: SerializedValue::I32(first),
                },
                SerializedFieldValue {
                    name: "subMeshCount".to_owned(),
                    value: SerializedValue::I32(count),
                },
            ]),
        }])
    }

    fn subset_root(count: usize) -> SerializedValue {
        SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_SubsetIndices".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::U32(0); count]),
        }])
    }

    #[test]
    fn static_batch_info_marks_unless_default() {
        // Mirrors the frozen `StaticBatchInfo.IsDefault`: only (0, 0) keeps
        // the GameObject unmarked.
        assert!(!renderer_static_batch_marks(&batch_info_root(0, 0)));
        assert!(renderer_static_batch_marks(&batch_info_root(0, 1)));
        assert!(renderer_static_batch_marks(&batch_info_root(1, 1)));
        assert!(renderer_static_batch_marks(&batch_info_root(1, 0)));
    }

    #[test]
    fn subset_indices_mark_without_batch_info() {
        // Without `m_StaticBatchInfo` (older layouts), a non-empty
        // `m_SubsetIndices` marks; an empty or missing one does not.
        assert!(renderer_static_batch_marks(&subset_root(3)));
        assert!(!renderer_static_batch_marks(&subset_root(0)));
        assert!(!renderer_static_batch_marks(&SerializedValue::Struct(
            vec![]
        )));
    }

    fn nav_mesh_editor_root(data_file_id: i32, data_path_id: i64) -> SerializedValue {
        fn scalar(name: &str, value: SerializedValue) -> SerializedFieldValue {
            SerializedFieldValue {
                name: name.to_owned(),
                value,
            }
        }
        SerializedValue::Struct(vec![
            scalar(
                "m_NavMeshData",
                SerializedValue::PPtr {
                    file_id: data_file_id,
                    path_id: data_path_id,
                },
            ),
            scalar(
                "m_BuildSettings",
                SerializedValue::Struct(vec![
                    scalar("agentTypeID", SerializedValue::I32(0)),
                    scalar("agentRadius", SerializedValue::F32(0.0)),
                    scalar("agentHeight", SerializedValue::F32(0.0)),
                    scalar("agentSlope", SerializedValue::F32(0.0)),
                    scalar("agentClimb", SerializedValue::F32(0.0)),
                    scalar("ledgeDropHeight", SerializedValue::F32(0.0)),
                    scalar("maxJumpAcrossDistance", SerializedValue::F32(0.0)),
                    scalar("minRegionArea", SerializedValue::F32(0.0)),
                    scalar("manualCellSize", SerializedValue::I32(0)),
                    scalar("cellSize", SerializedValue::F32(0.0)),
                    scalar("manualTileSize", SerializedValue::I32(0)),
                    scalar("tileSize", SerializedValue::I32(0)),
                    scalar("buildHeightMesh", SerializedValue::I32(0)),
                    scalar("maxJobWorkers", SerializedValue::U32(0)),
                    scalar("preserveTilesOutsideBounds", SerializedValue::I32(0)),
                    scalar(
                        "debug",
                        SerializedValue::Struct(vec![scalar("m_Flags", SerializedValue::U8(255))]),
                    ),
                ]),
            ),
        ])
    }

    fn nav_mesh_build_settings_field<'a>(
        root: &'a SerializedValue,
        name: &str,
    ) -> &'a SerializedValue {
        let SerializedValue::Struct(fields) = root else {
            panic!("navmesh test root is not a struct");
        };
        let settings = fields
            .iter()
            .find(|field| field.name == "m_BuildSettings")
            .expect("navmesh test root has m_BuildSettings");
        let SerializedValue::Struct(settings_fields) = &settings.value else {
            panic!("navmesh m_BuildSettings is not a struct");
        };
        &settings_fields
            .iter()
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("navmesh m_BuildSettings has no {name}"))
            .value
    }

    fn nav_mesh_test_source() -> ExportAssetIdentity {
        ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 0 },
            object_index: 3,
            path_id: 7,
            class_id: 196,
        }
    }

    #[test]
    fn nav_mesh_null_data_writes_standalone_defaults() {
        let mut root = nav_mesh_editor_root(0, 0);
        apply_nav_mesh_settings_editor_format(&mut root, None).unwrap();
        assert_eq!(
            nav_mesh_build_settings_field(&root, "agentRadius"),
            &SerializedValue::F32(0.5)
        );
        assert_eq!(
            nav_mesh_build_settings_field(&root, "tileSize"),
            &SerializedValue::I32(256)
        );
        assert_eq!(
            nav_mesh_build_settings_field(&root, "cellSize"),
            &SerializedValue::F32(1.0 / 6.0)
        );
    }

    #[test]
    fn nav_mesh_params_overlay_maps_five_fields_over_defaults() {
        let mut root = nav_mesh_editor_root(1, 4526);
        let overlay = WorkspaceNavMeshBuildSettingsOverlay {
            source: nav_mesh_test_source(),
            settings: WorkspaceNavMeshBuildSettingsSource::NavMeshParams {
                walkable_radius: 1.5,
                walkable_height: 2.5,
                walkable_climb: 0.75,
                tile_size: 64.9,
                cell_size: 0.125,
            },
        };
        apply_nav_mesh_settings_editor_format(&mut root, Some(&overlay)).unwrap();
        assert_eq!(
            nav_mesh_build_settings_field(&root, "agentRadius"),
            &SerializedValue::F32(1.5)
        );
        assert_eq!(
            nav_mesh_build_settings_field(&root, "agentHeight"),
            &SerializedValue::F32(2.5)
        );
        assert_eq!(
            nav_mesh_build_settings_field(&root, "agentClimb"),
            &SerializedValue::F32(0.75)
        );
        // The frozen `(int)TileSize` truncates toward zero.
        assert_eq!(
            nav_mesh_build_settings_field(&root, "tileSize"),
            &SerializedValue::I32(64)
        );
        assert_eq!(
            nav_mesh_build_settings_field(&root, "cellSize"),
            &SerializedValue::F32(0.125)
        );
        // Untouched fields keep the standalone defaults.
        assert_eq!(
            nav_mesh_build_settings_field(&root, "agentSlope"),
            &SerializedValue::F32(45.0)
        );
    }

    #[test]
    fn nav_mesh_build_settings_overlay_copies_source_fields() {
        let mut root = nav_mesh_editor_root(1, 4526);
        let source = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "agentRadius".to_owned(),
                value: SerializedValue::F32(0.2),
            },
            SerializedFieldValue {
                name: "tileSize".to_owned(),
                value: SerializedValue::I32(128),
            },
            SerializedFieldValue {
                name: "debug".to_owned(),
                value: SerializedValue::Struct(vec![SerializedFieldValue {
                    name: "m_Flags".to_owned(),
                    value: SerializedValue::U8(7),
                }]),
            },
            SerializedFieldValue {
                name: "sourceOnlyFutureField".to_owned(),
                value: SerializedValue::I32(9),
            },
        ]);
        // A target-only field keeps its merged value; a source-only field has
        // no generated assignment and must not appear in the output.
        if let SerializedValue::Struct(fields) = &mut root {
            let settings = fields
                .iter_mut()
                .find(|field| field.name == "m_BuildSettings")
                .unwrap();
            let SerializedValue::Struct(settings_fields) = &mut settings.value else {
                panic!("navmesh m_BuildSettings is not a struct");
            };
            settings_fields.push(SerializedFieldValue {
                name: "targetOnly".to_owned(),
                value: SerializedValue::I32(41),
            });
        }
        let overlay = WorkspaceNavMeshBuildSettingsOverlay {
            source: nav_mesh_test_source(),
            settings: WorkspaceNavMeshBuildSettingsSource::BuildSettings(source),
        };
        apply_nav_mesh_settings_editor_format(&mut root, Some(&overlay)).unwrap();
        assert_eq!(
            nav_mesh_build_settings_field(&root, "agentRadius"),
            &SerializedValue::F32(0.2)
        );
        assert_eq!(
            nav_mesh_build_settings_field(&root, "tileSize"),
            &SerializedValue::I32(128)
        );
        assert_eq!(
            nav_mesh_build_settings_field(&root, "targetOnly"),
            &SerializedValue::I32(41)
        );
        let SerializedValue::Struct(fields) = &root else {
            panic!("navmesh test root is not a struct");
        };
        let settings = fields
            .iter()
            .find(|field| field.name == "m_BuildSettings")
            .unwrap();
        let SerializedValue::Struct(settings_fields) = &settings.value else {
            panic!("navmesh m_BuildSettings is not a struct");
        };
        assert!(settings_fields
            .iter()
            .all(|field| field.name != "sourceOnlyFutureField"));
        let debug = settings_fields
            .iter()
            .find(|field| field.name == "debug")
            .unwrap();
        let SerializedValue::Struct(debug_fields) = &debug.value else {
            panic!("navmesh debug is not a struct");
        };
        assert_eq!(
            debug_fields
                .iter()
                .find(|field| field.name == "m_Flags")
                .unwrap()
                .value,
            SerializedValue::U8(7)
        );
    }

    #[test]
    fn nav_mesh_build_settings_copy_rejects_shape_mismatch() {
        let mut root = nav_mesh_editor_root(1, 4526);
        let source = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "agentRadius".to_owned(),
            value: SerializedValue::I32(1),
        }]);
        let overlay = WorkspaceNavMeshBuildSettingsOverlay {
            source: nav_mesh_test_source(),
            settings: WorkspaceNavMeshBuildSettingsSource::BuildSettings(source),
        };
        let error = apply_nav_mesh_settings_editor_format(&mut root, Some(&overlay))
            .expect_err("float/int shape mismatch must fail loudly");
        assert!(error.contains("incompatible"), "unexpected error: {error}");
    }

    #[test]
    fn nav_mesh_non_null_data_without_overlay_fails_loudly() {
        let mut root = nav_mesh_editor_root(1, 4526);
        let error = apply_nav_mesh_settings_editor_format(&mut root, None)
            .expect_err("unresolved non-null NavMeshData must fail loudly");
        assert!(error.contains("not yet read"), "unexpected error: {error}");
    }
}
