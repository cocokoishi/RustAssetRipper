//! Unity YAML export bridge for objects decoded from a SerializedFile workspace.

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str;

use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::PlatformResourceProvider;
use crate::processing::scene_naming::SceneUnityVersion;
use crate::resource::{
    resolve_and_read_streamed_resource_info, RootResourceFileCatalog, StreamedResourceContentError,
    StreamedResourceContentLimits, StreamedResourceContentResolution,
};
use crate::serialized_file::object_workspace::WorkspaceObject;
use crate::serialized_file::reference_graph::{MultiFilePPtrResolution, MultiFileReferenceGraph};
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::pointer_map::{
    ExportPointerMapError, ProjectExportPointerMap, ProjectPointerRequest,
    ProjectPointerResolution, UnresolvedProjectPointer,
};
use super::prefab_collection::PrefabExportIdAssignment;
use super::scene_collection::{
    order_scene_yaml_assignments, ExportAssetCollectionIdentity, ExportAssetIdentity,
    SceneExportIdAssignment, SceneYamlAssetOrderError, SceneYamlAssetOrderLimits,
};
use super::streamed_assets::{
    has_streamed_yaml_asset_field, prepare_streamed_yaml_asset, StreamedYamlAssetError,
    StreamedYamlAssetKind, StreamedYamlAssetLimits, StreamedYamlAssetPreparation,
};
use super::unity_yaml::{
    write_unity_yaml_documents, UnityYamlDocument, UnityYamlWriteError, UnityYamlWriteLimits,
};
use super::value_yaml::{
    serialized_object_value_to_unity_yaml_document, UnityYamlObjectConversionError,
    UnityYamlObjectConversionHints, UnityYamlObjectConversionLimits, UnityYamlObjectFieldMode,
    UnityYamlPPtrRequest,
};

/// A typed Scene or Prefab export-ID assignment usable by the YAML collection executor.
pub trait UnityYamlCollectionAssignment {
    fn asset(&self) -> ExportAssetIdentity;
    fn file_id(&self) -> i64;
}

impl UnityYamlCollectionAssignment for SceneExportIdAssignment {
    fn asset(&self) -> ExportAssetIdentity {
        self.asset
    }

    fn file_id(&self) -> i64 {
        self.file_id
    }
}

impl UnityYamlCollectionAssignment for PrefabExportIdAssignment {
    fn asset(&self) -> ExportAssetIdentity {
        self.asset
    }

    fn file_id(&self) -> i64 {
        self.file_id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnityYamlWorkspaceCollectionLimits {
    pub max_documents: usize,
    pub object_conversion: UnityYamlObjectConversionLimits,
    pub yaml_write: UnityYamlWriteLimits,
}

impl Default for UnityYamlWorkspaceCollectionLimits {
    fn default() -> Self {
        Self {
            max_documents: 1_000_000,
            object_conversion: UnityYamlObjectConversionLimits::default(),
            yaml_write: UnityYamlWriteLimits::default(),
        }
    }
}

/// Resource lookup inputs for streamed Mesh and image-texture YAML export.
#[derive(Clone, Copy)]
pub struct WorkspaceStreamedYamlResourceContext<'a, 'provider> {
    pub filesystem: &'a VirtualFileSystem,
    pub root_catalog: &'a RootResourceFileCatalog,
    pub provider: &'a PlatformResourceProvider<'provider>,
    pub content_limits: StreamedResourceContentLimits,
    pub staging_limits: StreamedYamlAssetLimits,
}

/// Execute one ordered Scene or Prefab YAML collection using workspace-decoded source objects.
///
/// Assignment order is preserved exactly. Callers provide the baseline `ExportableAssets` order;
/// processed objects without a workspace payload fail explicitly until their generated YAML
/// walkers are connected.
#[allow(clippy::too_many_arguments)]
pub fn workspace_assignments_to_unity_yaml<A: UnityYamlCollectionAssignment>(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_yaml_order: &[A],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    workspace_assignments_to_unity_yaml_with_context(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        assignments_in_yaml_order,
        current_collection_index,
        missing_reference_guid,
        limits,
        None,
    )
}

/// Execute a YAML collection while resolving and embedding streamed Mesh/texture content.
#[allow(clippy::too_many_arguments)]
pub fn workspace_assignments_to_unity_yaml_with_streamed_resources<
    A: UnityYamlCollectionAssignment,
>(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_yaml_order: &[A],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    streamed_resources: &WorkspaceStreamedYamlResourceContext<'_, '_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    workspace_assignments_to_unity_yaml_with_context(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        assignments_in_yaml_order,
        current_collection_index,
        missing_reference_guid,
        limits,
        Some(streamed_resources),
    )
}

#[allow(clippy::too_many_arguments)]
fn workspace_assignments_to_unity_yaml_with_context<A: UnityYamlCollectionAssignment>(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_yaml_order: &[A],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    streamed_resources: Option<&WorkspaceStreamedYamlResourceContext<'_, '_>>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let document_count = assignments_in_yaml_order.len();
    if document_count > limits.max_documents {
        return Err(UnityYamlWorkspaceCollectionError::DocumentLimitExceeded {
            count: document_count,
            limit: limits.max_documents,
        });
    }

    let mut seen_assets = HashSet::new();
    seen_assets.try_reserve(document_count).map_err(|_| {
        UnityYamlWorkspaceCollectionError::AllocationFailed {
            resource: "workspace YAML asset identity index",
            requested: document_count,
        }
    })?;
    let mut seen_file_ids = HashSet::new();
    seen_file_ids.try_reserve(document_count).map_err(|_| {
        UnityYamlWorkspaceCollectionError::AllocationFailed {
            resource: "workspace YAML document fileID index",
            requested: document_count,
        }
    })?;
    let mut documents = Vec::new();
    documents.try_reserve_exact(document_count).map_err(|_| {
        UnityYamlWorkspaceCollectionError::AllocationFailed {
            resource: "workspace YAML documents",
            requested: document_count,
        }
    })?;

    for (assignment_index, assignment) in assignments_in_yaml_order.iter().enumerate() {
        let asset = assignment.asset();
        if !seen_assets.insert(asset.object_key()) {
            return Err(UnityYamlWorkspaceCollectionError::DuplicateAssetIdentity {
                assignment_index,
                asset,
            });
        }
        let file_id = assignment.file_id();
        if !seen_file_ids.insert(file_id) {
            return Err(UnityYamlWorkspaceCollectionError::DuplicateDocumentFileId {
                assignment_index,
                file_id,
            });
        }
        let ExportAssetCollectionIdentity::SerializedFile { file_index } = asset.collection else {
            return Err(
                UnityYamlWorkspaceCollectionError::ProcessedAssetRequiresGeneratedWalker {
                    assignment_index,
                    asset,
                },
            );
        };
        let object = objects.get(file_index, asset.object_index).ok_or(
            UnityYamlWorkspaceCollectionError::WorkspaceObjectMissing {
                assignment_index,
                asset,
            },
        )?;
        let document = workspace_object_to_unity_yaml_document_with_context(
            workspace,
            reference_graph,
            pointer_map,
            object,
            asset,
            file_id,
            current_collection_index,
            missing_reference_guid,
            limits.object_conversion,
            streamed_resources,
        )
        .map_err(|source| UnityYamlWorkspaceCollectionError::ObjectExport {
            assignment_index,
            source: Box::new(source),
        })?;
        documents.push(document);
    }

    write_unity_yaml_documents(&documents, limits.yaml_write)
        .map_err(UnityYamlWorkspaceCollectionError::YamlWrite)
}

/// Order hierarchy-listed Scene assignments using the baseline manager-first stable comparator,
/// then execute them through the workspace YAML collection writer.
#[allow(clippy::too_many_arguments)]
pub fn workspace_scene_assignments_to_unity_yaml(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let ordered_assignments = order_scene_yaml_assignments(
        assignments_in_hierarchy_order,
        SceneYamlAssetOrderLimits {
            max_assets: limits.max_documents,
        },
    )
    .map_err(UnityYamlWorkspaceCollectionError::SceneOrder)?;
    workspace_assignments_to_unity_yaml(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        &ordered_assignments,
        current_collection_index,
        missing_reference_guid,
        limits,
    )
}

/// Order and export Scene assignments with bounded streamed Mesh/texture resolution.
#[allow(clippy::too_many_arguments)]
pub fn workspace_scene_assignments_to_unity_yaml_with_streamed_resources(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    streamed_resources: &WorkspaceStreamedYamlResourceContext<'_, '_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let ordered_assignments = order_scene_yaml_assignments(
        assignments_in_hierarchy_order,
        SceneYamlAssetOrderLimits {
            max_assets: limits.max_documents,
        },
    )
    .map_err(UnityYamlWorkspaceCollectionError::SceneOrder)?;
    workspace_assignments_to_unity_yaml_with_streamed_resources(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        &ordered_assignments,
        current_collection_index,
        missing_reference_guid,
        limits,
        streamed_resources,
    )
}

#[derive(Debug)]
pub enum UnityYamlWorkspaceCollectionError {
    DocumentLimitExceeded {
        count: usize,
        limit: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    SceneOrder(SceneYamlAssetOrderError),
    DuplicateAssetIdentity {
        assignment_index: usize,
        asset: ExportAssetIdentity,
    },
    DuplicateDocumentFileId {
        assignment_index: usize,
        file_id: i64,
    },
    ProcessedAssetRequiresGeneratedWalker {
        assignment_index: usize,
        asset: ExportAssetIdentity,
    },
    WorkspaceObjectMissing {
        assignment_index: usize,
        asset: ExportAssetIdentity,
    },
    ObjectExport {
        assignment_index: usize,
        source: Box<UnityYamlWorkspaceExportError>,
    },
    YamlWrite(UnityYamlWriteError),
}

impl Display for UnityYamlWorkspaceCollectionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DocumentLimitExceeded { count, limit } => write!(
                f,
                "workspace YAML collection has {count} documents, exceeding limit {limit}"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(f, "could not allocate {requested} entries for {resource}"),
            Self::SceneOrder(source) => Display::fmt(source, f),
            Self::DuplicateAssetIdentity {
                assignment_index,
                asset,
            } => write!(
                f,
                "workspace YAML assignment {assignment_index} duplicates asset identity {asset:?}"
            ),
            Self::DuplicateDocumentFileId {
                assignment_index,
                file_id,
            } => write!(
                f,
                "workspace YAML assignment {assignment_index} duplicates document fileID {file_id}"
            ),
            Self::ProcessedAssetRequiresGeneratedWalker {
                assignment_index,
                asset,
            } => write!(
                f,
                "workspace YAML assignment {assignment_index} is a processed asset without a generated YAML walker: {asset:?}"
            ),
            Self::WorkspaceObjectMissing {
                assignment_index,
                asset,
            } => write!(
                f,
                "workspace YAML assignment {assignment_index} has no decoded source object: {asset:?}"
            ),
            Self::ObjectExport {
                assignment_index,
                source,
            } => write!(
                f,
                "workspace YAML assignment {assignment_index} failed object conversion: {source}"
            ),
            Self::YamlWrite(source) => Display::fmt(source, f),
        }
    }
}

impl Error for UnityYamlWorkspaceCollectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ObjectExport { source, .. } => Some(source),
            Self::SceneOrder(source) => Some(source),
            Self::YamlWrite(source) => Some(source),
            _ => None,
        }
    }
}

/// Convert one workspace-decoded SerializedFile object into the project YAML form.
///
/// The object identity, its TypeTree and version, reference scan, and project pointer map are all
/// checked against the same workspace indices. The reference graph must have completed scanning
/// the source object; undecoded references and trailing object bytes remain explicit errors.
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
    workspace_object_to_unity_yaml_document_with_context(
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
    )
}

/// Convert one workspace object while resolving streamed data for Mesh and image-texture YAML.
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
) -> Result<UnityYamlDocument, UnityYamlWorkspaceExportError> {
    workspace_object_to_unity_yaml_document_with_context(
        workspace,
        reference_graph,
        pointer_map,
        object,
        source,
        file_id,
        current_collection_index,
        missing_reference_guid,
        limits,
        Some(streamed_resources),
    )
}

#[allow(clippy::too_many_arguments)]
fn workspace_object_to_unity_yaml_document_with_context(
    workspace: &SerializedFileWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    object: &WorkspaceObject,
    source: ExportAssetIdentity,
    file_id: i64,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    streamed_resources: Option<&WorkspaceStreamedYamlResourceContext<'_, '_>>,
) -> Result<UnityYamlDocument, UnityYamlWorkspaceExportError> {
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
        || metadata_object.is_stripped != object.is_stripped
    {
        return Err(UnityYamlWorkspaceExportError::MetadataIdentityMismatch {
            file_index,
            object_index: object.object_index,
        });
    }
    let class_id =
        object
            .class_id
            .map(i32::from)
            .ok_or(UnityYamlWorkspaceExportError::MissingClassId {
                file_index,
                object_index: object.object_index,
            })?;
    if source.class_id != class_id {
        return Err(UnityYamlWorkspaceExportError::ClassIdMismatch {
            source_class_id: source.class_id,
            object_class_id: class_id,
        });
    }
    let payload =
        object
            .payload
            .as_ref()
            .map_err(|source| UnityYamlWorkspaceExportError::ObjectDecode {
                file_index,
                object_index: object.object_index,
                message: source.to_string(),
            })?;
    if payload.object_index != object.object_index || payload.path_id != source.path_id {
        return Err(UnityYamlWorkspaceExportError::PayloadIdentityMismatch {
            file_index,
            object_index: object.object_index,
            payload_object_index: payload.object_index,
            payload_path_id: payload.path_id,
        });
    }
    if !payload.trailing_bytes.is_empty() {
        return Err(UnityYamlWorkspaceExportError::UnparsedTrailingPayload {
            file_index,
            object_index: object.object_index,
            byte_count: payload.trailing_bytes.len(),
        });
    }

    let type_index =
        object
            .resolved_type_index
            .ok_or(UnityYamlWorkspaceExportError::MissingResolvedType {
                file_index,
                object_index: object.object_index,
            })?;
    let serialized_type = file.metadata.types.get(type_index).ok_or(
        UnityYamlWorkspaceExportError::InvalidResolvedType {
            file_index,
            object_index: object.object_index,
            type_index,
            type_count: file.metadata.types.len(),
        },
    )?;
    let type_tree = serialized_type.type_tree.as_ref().ok_or(
        UnityYamlWorkspaceExportError::MissingTypeTree {
            file_index,
            object_index: object.object_index,
            type_index,
        },
    )?;
    let type_root =
        type_tree
            .nodes
            .first()
            .ok_or(UnityYamlWorkspaceExportError::EmptyTypeTree {
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
    let hints = UnityYamlObjectConversionHints::from_type_tree(type_tree, unity_version, limits)
        .map_err(UnityYamlWorkspaceExportError::Conversion)?;

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
                graph_class_id: reference_graph.object_class_id(file_index, object.object_index),
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
    if let Some(asset_kind) = match class_id {
        43 => Some(StreamedYamlAssetKind::Mesh),
        28 | 189 => Some(StreamedYamlAssetKind::ImageTexture),
        _ => None,
    } {
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
            match prepare_streamed_yaml_asset(&mut export_root, asset_kind, None, staging_limits) {
                Ok(
                    StreamedYamlAssetPreparation::NoStreamField
                    | StreamedYamlAssetPreparation::StreamUnset
                    | StreamedYamlAssetPreparation::ExistingInlineData { .. },
                ) => {}
                Ok(StreamedYamlAssetPreparation::Materialized { .. }) => {
                    return Err(UnityYamlWorkspaceExportError::StreamedAssetInvariant {
                        file_index,
                        object_index: object.object_index,
                        message: "staging reported materialization without supplied bytes"
                            .to_owned(),
                    });
                }
                Err(StreamedYamlAssetError::StreamContentRequired { resource }) => {
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
                            return Err(UnityYamlWorkspaceExportError::StreamedResourceMissing {
                                file_index,
                                object_index: object.object_index,
                                path: resource.path,
                                unavailable_data_paths,
                            });
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
                    prepare_streamed_yaml_asset(
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
    let export_root = streamed_export_root.as_ref().unwrap_or(&payload.root);

    let mut pointer_error = None;
    let document = serialized_object_value_to_unity_yaml_document(
        source,
        &payload.type_name,
        type_root.version,
        file_id,
        // SerializedFile's stripped bit selects a serialized type layout; the baseline YAML
        // marker comes from the owning hierarchy's StrippedAssets set, which is not wired here.
        false,
        export_root,
        &hints,
        UnityYamlObjectFieldMode::ProjectAsset,
        unity_version,
        missing_reference_guid,
        limits,
        |request| {
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
    document.map_err(UnityYamlWorkspaceExportError::Conversion)
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
    let Some(edge) = matching_edges.next() else {
        *error = Some(UnityYamlWorkspaceExportError::ReferenceEdgeMissing {
            file_index,
            object_index: request.source.object_index,
            field_path: request.field_path.to_owned(),
            file_id: request.file_id,
            path_id: request.path_id,
        });
        return ProjectPointerResolution::Null;
    };
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
        Ok(resolution) => resolution,
        Err(source) => {
            *error = Some(UnityYamlWorkspaceExportError::PointerMap(source));
            ProjectPointerResolution::Null
        }
    }
}

#[derive(Debug)]
pub enum UnityYamlWorkspaceExportError {
    ProcessedObjectIdentity {
        source: ExportAssetIdentity,
    },
    WorkspaceIdentityMismatch {
        source: ExportAssetIdentity,
        object_file_index: usize,
        object_index: usize,
        path_id: i64,
    },
    MissingWorkspaceFile {
        file_index: usize,
    },
    MissingMetadataObject {
        file_index: usize,
        object_index: usize,
    },
    MetadataIdentityMismatch {
        file_index: usize,
        object_index: usize,
    },
    MissingClassId {
        file_index: usize,
        object_index: usize,
    },
    ClassIdMismatch {
        source_class_id: i32,
        object_class_id: i32,
    },
    ObjectDecode {
        file_index: usize,
        object_index: usize,
        message: String,
    },
    PayloadIdentityMismatch {
        file_index: usize,
        object_index: usize,
        payload_object_index: usize,
        payload_path_id: i64,
    },
    UnparsedTrailingPayload {
        file_index: usize,
        object_index: usize,
        byte_count: usize,
    },
    MissingResolvedType {
        file_index: usize,
        object_index: usize,
    },
    InvalidResolvedType {
        file_index: usize,
        object_index: usize,
        type_index: usize,
        type_count: usize,
    },
    MissingTypeTree {
        file_index: usize,
        object_index: usize,
        type_index: usize,
    },
    EmptyTypeTree {
        file_index: usize,
        object_index: usize,
        type_index: usize,
    },
    UnityVersionUnavailable {
        file_index: usize,
        unity_version_text: Option<String>,
    },
    ReferenceGraphIdentityMissing {
        file_index: usize,
        object_index: usize,
    },
    ReferenceGraphIdentityMismatch {
        file_index: usize,
        object_index: usize,
        expected_path_id: i64,
        graph_path_id: i64,
    },
    ReferenceGraphClassIdMismatch {
        file_index: usize,
        object_index: usize,
        expected_class_id: i32,
        graph_class_id: Option<i32>,
    },
    ReferenceScanIncomplete {
        file_index: usize,
        object_index: usize,
    },
    ReferenceEdgeMissing {
        file_index: usize,
        object_index: usize,
        field_path: String,
        file_id: i32,
        path_id: i64,
    },
    ReferenceEdgeAmbiguous {
        file_index: usize,
        object_index: usize,
        field_path: String,
        file_id: i32,
        path_id: i64,
    },
    NonNullPointerResolvedAsNull {
        file_index: usize,
        object_index: usize,
        field_path: String,
        path_id: i64,
    },
    ReferenceTargetIdentityMissing {
        file_index: usize,
        object_index: usize,
    },
    ReferenceTargetClassIdMissing {
        file_index: usize,
        object_index: usize,
    },
    StreamedAssetPreparation {
        file_index: usize,
        object_index: usize,
        source: Box<StreamedYamlAssetError>,
    },
    StreamedResourceResolution {
        file_index: usize,
        object_index: usize,
        path: String,
        source: Box<StreamedResourceContentError>,
    },
    StreamedResourceMissing {
        file_index: usize,
        object_index: usize,
        path: String,
        unavailable_data_paths: Vec<String>,
    },
    StreamedResourceUnexpectedUnset {
        file_index: usize,
        object_index: usize,
        path: String,
    },
    StreamedAssetInvariant {
        file_index: usize,
        object_index: usize,
        message: String,
    },
    PointerMap(ExportPointerMapError),
    Conversion(UnityYamlObjectConversionError),
}

impl Display for UnityYamlWorkspaceExportError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProcessedObjectIdentity { source } => {
                write!(f, "workspace YAML source is not a SerializedFile object: {source:?}")
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::export::pointer_map::{
        ExportCollectionPointerMetadata, ExportPointerMapLimits, ExportedAssetPointerEntry,
        ProjectExportPointerMapBuilder, UnityProjectAssetType,
    };
    use crate::export::scene_collection::ExportAssetIdentity;
    use crate::io::dependency::DependencyFilePath;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::resource::{ExplicitRootResourceInput, RootResourceCatalogLimits};
    use crate::scheme::{SchemeReader, SchemeReaderLimits};
    use crate::serialized_file::file::SerializedFile;
    use crate::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
    use crate::serialized_file::reference_graph::{
        MultiFileReferenceGraph, MultiFileReferenceGraphLimits,
    };
    use crate::serialized_file::workspace::{
        SerializedFileWorkspace, SerializedFileWorkspaceLimits,
    };

    const COLLECTION_INDEX: usize = 7;
    const GAME_OBJECT_PATH_ID: i64 = 42;
    const TRANSFORM_PATH_ID: i64 = 43;

    fn asset(object_index: usize, path_id: i64, class_id: i32) -> ExportAssetIdentity {
        ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 0 },
            object_index,
            path_id,
            class_id,
        }
    }

    fn pointer_map() -> ProjectExportPointerMap {
        let assets = [
            ExportedAssetPointerEntry {
                asset: asset(0, GAME_OBJECT_PATH_ID, 1),
                file_id: GAME_OBJECT_PATH_ID,
                collection_index: COLLECTION_INDEX,
            },
            ExportedAssetPointerEntry {
                asset: asset(1, TRANSFORM_PATH_ID, 4),
                file_id: TRANSFORM_PATH_ID,
                collection_index: COLLECTION_INDEX,
            },
        ];
        let collections = [ExportCollectionPointerMetadata {
            collection_index: COLLECTION_INDEX,
            guid: [0x71; 16],
            asset_type: UnityProjectAssetType::Serialized,
        }];
        ProjectExportPointerMap::new(&assets, &collections, ExportPointerMapLimits::default())
            .unwrap()
    }

    fn pointer_map_for_asset(asset: ExportAssetIdentity) -> ProjectExportPointerMap {
        let assets = [ExportedAssetPointerEntry {
            asset,
            file_id: asset.path_id,
            collection_index: COLLECTION_INDEX,
        }];
        let collections = [ExportCollectionPointerMetadata {
            collection_index: COLLECTION_INDEX,
            guid: [0x72; 16],
            asset_type: UnityProjectAssetType::Serialized,
        }];
        ProjectExportPointerMap::new(&assets, &collections, ExportPointerMapLimits::default())
            .unwrap()
    }

    fn load_workspaces(target_path_id: i64) -> (VirtualFileSystem, SerializedFileWorkspace) {
        let bytes = make_game_object_and_transform_file(target_path_id);
        load_workspace_bytes(&bytes)
    }

    fn load_workspace_bytes(bytes: &[u8]) -> (VirtualFileSystem, SerializedFileWorkspace) {
        load_workspace_files(&[("main.assets", bytes)])
    }

    fn load_workspace_files(
        files: &[(&str, &[u8])],
    ) -> (VirtualFileSystem, SerializedFileWorkspace) {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        let mut input = Vec::new();
        for &(file_name, bytes) in files {
            let virtual_path = format!("/game/{file_name}");
            filesystem.write_all_bytes(&virtual_path, bytes).unwrap();
            input.push(DependencyFilePath {
                file_name: file_name.to_owned(),
                virtual_path,
            });
        }
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &input,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        (filesystem, workspace)
    }

    fn explicit_root_resource_catalog(
        filesystem: &VirtualFileSystem,
        file_name: &str,
        virtual_path: &str,
    ) -> RootResourceFileCatalog {
        let dispatched =
            SchemeReader::read_with_limits(b"root resource", SchemeReaderLimits::default())
                .unwrap();
        let input = [ExplicitRootResourceInput {
            file_name,
            virtual_path,
            dispatched_file: &dispatched,
        }];
        RootResourceFileCatalog::from_explicit_resource_inputs(
            filesystem,
            &input,
            RootResourceCatalogLimits::default(),
        )
        .unwrap()
    }

    fn make_streamed_asset_file(class_id: i32, offset_is_u64: bool) -> Vec<u8> {
        let (root_type, stream_field, inline_parent_type, inline_parent_name, inline_field) =
            match class_id {
                43 => (
                    "Mesh",
                    "m_StreamData",
                    "VertexData",
                    "m_VertexData",
                    "m_DataSize",
                ),
                189 => (
                    "ImageTexture",
                    "m_StreamData_C189",
                    "TypelessData",
                    "m_ImageData_C189",
                    "",
                ),
                _ => panic!("fixture supports Mesh and ImageTexture only"),
            };
        let mut tree = vec![
            (0, 1, root_type, "", -1),
            (1, 1, "StreamingInfo", stream_field, -1),
            (2, 1, "string", "path", -1),
            (3, 1, "Array", "Array", -1),
            (4, 1, "int", "size", 4),
            (4, 1, "char", "data", 1),
            (
                2,
                1,
                if offset_is_u64 { "UInt64" } else { "UInt32" },
                "offset",
                if offset_is_u64 { 8 } else { 4 },
            ),
            (2, 1, "UInt32", "size", 4),
        ];
        if class_id == 43 {
            tree.extend([
                (1, 1, inline_parent_type, inline_parent_name, -1),
                (2, 1, "TypelessData", inline_field, -1),
                (3, 1, "int", "size", 4),
                (3, 1, "UInt8", "data", 1),
            ]);
        } else {
            tree.extend([
                (1, 1, inline_parent_type, inline_parent_name, -1),
                (2, 1, "int", "size", 4),
                (2, 1, "UInt8", "data", 1),
            ]);
        }

        let mut payload = Vec::new();
        payload.extend_from_slice(&i32::try_from("texture.resS".len()).unwrap().to_le_bytes());
        payload.extend_from_slice(b"texture.resS");
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        if offset_is_u64 {
            payload.extend_from_slice(&2u64.to_le_bytes());
        } else {
            payload.extend_from_slice(&2u32.to_le_bytes());
        }
        payload.extend_from_slice(&3u32.to_le_bytes());
        payload.extend_from_slice(&0i32.to_le_bytes()); // empty inline TypelessData

        let mut metadata = b"2019.4.0f1\0".to_vec();
        metadata.extend_from_slice(&0u32.to_le_bytes()); // generation-10 long FileID flag
        metadata.extend_from_slice(&1i32.to_le_bytes());
        write_type_tree(&mut metadata, class_id, &tree);
        metadata.extend_from_slice(&0u32.to_le_bytes()); // long FileID flag
        metadata.extend_from_slice(&1i32.to_le_bytes());
        write_object_record(
            &mut metadata,
            77,
            0,
            payload.len(),
            class_id,
            class_id as i16,
        );
        metadata.extend_from_slice(&0i32.to_le_bytes()); // external files
        metadata.push(0); // user information string

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn plan_single_file_scene_assignments(
        bytes: &[u8],
        reference_graph: &MultiFileReferenceGraph<'_>,
    ) -> Vec<SceneExportIdAssignment> {
        plan_scene_assignments_for_workspace(&[("main.assets", bytes)], reference_graph)
    }

    fn plan_scene_assignments_for_workspace(
        files: &[(&str, &[u8])],
        reference_graph: &MultiFileReferenceGraph<'_>,
    ) -> Vec<SceneExportIdAssignment> {
        let (first_file_name, _) = files.first().unwrap();
        let plan = crate::processing::scene_collections::plan_scene_definition(
            0,
            first_file_name,
            &crate::processing::scene_collections::SceneCollectionPathStatus::BuildSettingsMissing,
            None,
            None,
            crate::processing::scene_collections::SceneDefinitionPlanLimits::default(),
        )
        .unwrap();
        let definitions =
            crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
                vec![plan],
                files.len(),
                crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
            )
            .unwrap();
        let mut definitions = definitions;
        for file_index in 1..files.len() {
            definitions.add_collection(0, file_index).unwrap();
        }
        let mut hierarchy_builder =
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
                files.len(),
                crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
            )
            .unwrap();
        for (file_index, &(file_name, bytes)) in files.iter().enumerate() {
            let serialized_file = SerializedFile::parse(bytes).unwrap();
            let mut scene_graph = crate::processing::scene::SceneGraph::build(
                &serialized_file,
                crate::processing::scene::SceneGraphLimits::default(),
            )
            .unwrap();
            scene_graph
                .resolve_with_multi_file_graph(file_index, file_name, reference_graph)
                .unwrap();
            let roots = scene_graph
                .discover_workspace_roots(
                    file_index,
                    file_name,
                    reference_graph,
                    crate::processing::scene::SceneRootDiscoveryLimits::default(),
                )
                .unwrap();
            let audit = scene_graph
                .audit_workspace_transform_links(
                    file_index,
                    file_name,
                    reference_graph,
                    crate::processing::scene::SceneHierarchyAuditLimits::default(),
                )
                .unwrap();
            hierarchy_builder
                .add_collection_graph(file_index, &scene_graph, roots, audit, reference_graph)
                .unwrap();
        }
        let hierarchies = hierarchy_builder.finish(&definitions).unwrap();
        crate::export::scene_collection::plan_workspace_scene_export_ids(
            &hierarchies.scenes()[0],
            SceneUnityVersion {
                major: 2019,
                minor: 4,
                patch: 0,
            },
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyMembershipLimits::default(),
            crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
            crate::export::scene_collection::SceneExportIdPlanLimits::default(),
        )
        .unwrap()
    }

    fn make_game_object_and_transform_file(component_path_id: i64) -> Vec<u8> {
        let game_object_tree = [
            (0, 6, "GameObject", "", -1),
            (1, 1, "vector", "m_Component", -1),
            (2, 1, "Array", "Array", -1),
            (3, 1, "int", "size", 4),
            (3, 1, "ComponentPair", "data", -1),
            (4, 1, "PPtr<Transform>", "component", 12),
            (5, 1, "SInt32", "m_FileID", 4),
            (5, 1, "SInt64", "m_PathID", 8),
        ];
        let transform_tree = [
            (0, 2, "Transform", "", -1),
            (1, 1, "PPtr<GameObject>", "m_GameObject", 12),
            (2, 1, "SInt32", "m_FileID", 4),
            (2, 1, "SInt64", "m_PathID", 8),
        ];
        let mut metadata = b"2019.4.0f1\0".to_vec();
        metadata.extend_from_slice(&0u32.to_le_bytes());
        metadata.extend_from_slice(&2i32.to_le_bytes());
        write_type_tree(&mut metadata, 1, &game_object_tree);
        write_type_tree(&mut metadata, 4, &transform_tree);

        metadata.extend_from_slice(&0u32.to_le_bytes()); // generation-10 long FileID flag
        metadata.extend_from_slice(&2i32.to_le_bytes());
        let mut game_object_payload = Vec::new();
        game_object_payload.extend_from_slice(&1i32.to_le_bytes()); // m_Component size
        game_object_payload.extend_from_slice(&0i32.to_le_bytes()); // component FileID
        game_object_payload.extend_from_slice(&component_path_id.to_le_bytes());
        let mut transform_payload = Vec::new();
        transform_payload.extend_from_slice(&0i32.to_le_bytes()); // m_GameObject FileID
        transform_payload.extend_from_slice(&GAME_OBJECT_PATH_ID.to_le_bytes());

        write_object_record(
            &mut metadata,
            GAME_OBJECT_PATH_ID,
            0,
            game_object_payload.len(),
            1,
            1,
        );
        write_object_record(
            &mut metadata,
            TRANSFORM_PATH_ID,
            game_object_payload.len(),
            transform_payload.len(),
            4,
            4,
        );
        metadata.extend_from_slice(&0i32.to_le_bytes()); // external files
        metadata.push(0); // user information string

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + game_object_payload.len() + transform_payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&game_object_payload);
        file.extend_from_slice(&transform_payload);
        file
    }

    fn make_mesh_renderer_component_file() -> Vec<u8> {
        const COMPONENT_CLASS_ID: i32 = 23;
        const COMPONENT_PATH_ID: i64 = 9001;

        let component_tree = [
            (0, 1, "MeshRenderer", "", -1),
            (1, 1, "Renderer", "Base", -1),
            (2, 1, "Component", "Base", -1),
            (3, 1, "PPtr<GameObject>", "m_GameObject", 8),
            (4, 1, "SInt32", "m_FileID", 4),
            (4, 1, "SInt32", "m_PathID", 4),
        ];
        let payload = [0_u8; 8];
        let mut metadata = b"2019.4.0f1\0".to_vec();
        metadata.extend_from_slice(&0_u32.to_le_bytes());
        metadata.extend_from_slice(&1_i32.to_le_bytes());
        write_type_tree(&mut metadata, COMPONENT_CLASS_ID, &component_tree);
        metadata.extend_from_slice(&0_u32.to_le_bytes()); // generation-10 long FileID flag
        metadata.extend_from_slice(&1_i32.to_le_bytes());
        write_object_record(
            &mut metadata,
            COMPONENT_PATH_ID,
            0,
            payload.len(),
            COMPONENT_CLASS_ID,
            COMPONENT_CLASS_ID as i16,
        );
        metadata.extend_from_slice(&0_i32.to_le_bytes()); // external files
        metadata.push(0); // user information string

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&10_i32.to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_game_object_mesh_renderer_file() -> Vec<u8> {
        const GAME_OBJECT_CLASS_ID: i32 = 1;
        const MESH_RENDERER_CLASS_ID: i32 = 23;

        let game_object_tree = [
            (0, 1, "GameObject", "", -1),
            (1, 1, "string", "m_Name", -1),
            (2, 1, "Array", "Array", -1),
            (3, 1, "int", "size", 4),
            (3, 1, "char", "data", 1),
            (1, 1, "bool", "m_IsActive", 1),
            (1, 1, "vector", "m_Component", -1),
            (2, 1, "Array", "Array", -1),
            (3, 1, "int", "size", 4),
            (3, 1, "ComponentPair", "data", -1),
            (4, 1, "PPtr<Component>", "component", 8),
            (5, 1, "SInt32", "m_FileID", 4),
            (5, 1, "SInt32", "m_PathID", 4),
        ];
        let mesh_renderer_tree = [
            (0, 1, "MeshRenderer", "", -1),
            (1, 1, "Renderer", "Base", -1),
            (2, 1, "Component", "Base", -1),
            (3, 1, "PPtr<GameObject>", "m_GameObject", 8),
            (4, 1, "SInt32", "m_FileID", 4),
            (4, 1, "SInt32", "m_PathID", 4),
        ];

        let mut metadata = b"2019.4.0f1\0".to_vec();
        metadata.extend_from_slice(&0_u32.to_le_bytes());
        metadata.extend_from_slice(&2_i32.to_le_bytes());
        write_type_tree(&mut metadata, GAME_OBJECT_CLASS_ID, &game_object_tree);
        write_type_tree(&mut metadata, MESH_RENDERER_CLASS_ID, &mesh_renderer_tree);

        let mut game_object_payload = Vec::new();
        game_object_payload.extend_from_slice(&4_i32.to_le_bytes());
        game_object_payload.extend_from_slice(b"Root");
        game_object_payload.push(1);
        game_object_payload.extend_from_slice(&1_i32.to_le_bytes());
        game_object_payload.extend_from_slice(&0_i32.to_le_bytes());
        game_object_payload.extend_from_slice(&9001_i32.to_le_bytes());
        let mut mesh_renderer_payload = Vec::new();
        mesh_renderer_payload.extend_from_slice(&0_i32.to_le_bytes());
        mesh_renderer_payload.extend_from_slice(&42_i32.to_le_bytes());

        metadata.extend_from_slice(&0_u32.to_le_bytes()); // generation-10 long FileID flag
        metadata.extend_from_slice(&2_i32.to_le_bytes());
        write_object_record(
            &mut metadata,
            42,
            0,
            game_object_payload.len(),
            GAME_OBJECT_CLASS_ID,
            GAME_OBJECT_CLASS_ID as i16,
        );
        write_object_record(
            &mut metadata,
            9001,
            game_object_payload.len(),
            mesh_renderer_payload.len(),
            MESH_RENDERER_CLASS_ID,
            MESH_RENDERER_CLASS_ID as i16,
        );
        metadata.extend_from_slice(&0_i32.to_le_bytes()); // external files
        metadata.push(0); // user information string

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + game_object_payload.len() + mesh_renderer_payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&10_i32.to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&game_object_payload);
        file.extend_from_slice(&mesh_renderer_payload);
        file
    }

    fn make_generation_10_object_file_with_external(
        class_id: i32,
        path_id: i64,
        type_tree: &[(u8, i32, &str, &str, i32)],
        payload: &[u8],
        external_file_name: &str,
    ) -> Vec<u8> {
        let mut metadata = b"2019.4.0f1\0".to_vec();
        metadata.extend_from_slice(&0_u32.to_le_bytes());
        metadata.extend_from_slice(&1_i32.to_le_bytes());
        write_type_tree(&mut metadata, class_id, type_tree);
        metadata.extend_from_slice(&0_u32.to_le_bytes()); // generation-10 long FileID flag
        metadata.extend_from_slice(&1_i32.to_le_bytes());
        write_object_record(
            &mut metadata,
            path_id,
            0,
            payload.len(),
            class_id,
            class_id as i16,
        );
        metadata.extend_from_slice(&1_i32.to_le_bytes()); // external files
        metadata.push(0); // empty asset path
        metadata.extend_from_slice(&[0; 16]); // GUID
        metadata.extend_from_slice(&0_i32.to_le_bytes()); // asset type
        metadata.extend_from_slice(external_file_name.as_bytes());
        metadata.push(0);
        metadata.push(0); // user information string

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&10_i32.to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(payload);
        file
    }

    fn make_game_object_with_external_component() -> Vec<u8> {
        let tree = [
            (0, 1, "GameObject", "", -1),
            (1, 1, "string", "m_Name", -1),
            (2, 1, "Array", "Array", -1),
            (3, 1, "int", "size", 4),
            (3, 1, "char", "data", 1),
            (1, 1, "bool", "m_IsActive", 1),
            (1, 1, "vector", "m_Component", -1),
            (2, 1, "Array", "Array", -1),
            (3, 1, "int", "size", 4),
            (3, 1, "ComponentPair", "data", -1),
            (4, 1, "PPtr<Component>", "component", 8),
            (5, 1, "SInt32", "m_FileID", 4),
            (5, 1, "SInt32", "m_PathID", 4),
        ];
        let mut payload = Vec::new();
        payload.extend_from_slice(&4_i32.to_le_bytes());
        payload.extend_from_slice(b"Root");
        payload.push(1);
        payload.extend_from_slice(&1_i32.to_le_bytes());
        payload.extend_from_slice(&1_i32.to_le_bytes());
        payload.extend_from_slice(&9001_i32.to_le_bytes());
        make_generation_10_object_file_with_external(1, 42, &tree, &payload, "shared.assets")
    }

    fn make_game_object_with_external_pptr_dictionary() -> Vec<u8> {
        let tree = [
            (0, 1, "GameObject", "", -1),
            (1, 1, "map", "m_Targets", -1),
            (2, 1, "Array", "Array", -1),
            (3, 1, "SInt32", "size", 4),
            (3, 1, "pair", "data", -1),
            (4, 1, "string", "first", -1),
            (5, 1, "Array", "Array", -1),
            (6, 1, "SInt32", "size", 4),
            (6, 1, "char", "data", 2),
            (4, 1, "PPtr<GameObject>", "second", 12),
            (5, 1, "SInt32", "m_FileID", 4),
            (5, 1, "SInt64", "m_PathID", 8),
        ];
        let mut payload = Vec::new();
        payload.extend_from_slice(&1_i32.to_le_bytes()); // one dictionary entry
        payload.extend_from_slice(&6_i32.to_le_bytes()); // string length
        payload.extend_from_slice(b"target");
        payload.extend_from_slice(&[0; 2]); // align string data to four bytes
        payload.extend_from_slice(&1_i32.to_le_bytes()); // first external file
        payload.extend_from_slice(&GAME_OBJECT_PATH_ID.to_le_bytes());
        make_generation_10_object_file_with_external(1, 51, &tree, &payload, "target.assets")
    }

    fn make_mesh_renderer_with_external_game_object() -> Vec<u8> {
        let tree = [
            (0, 1, "MeshRenderer", "", -1),
            (1, 1, "Renderer", "Base", -1),
            (2, 1, "Component", "Base", -1),
            (3, 1, "PPtr<GameObject>", "m_GameObject", 8),
            (4, 1, "SInt32", "m_FileID", 4),
            (4, 1, "SInt32", "m_PathID", 4),
        ];
        let mut payload = Vec::new();
        payload.extend_from_slice(&1_i32.to_le_bytes());
        payload.extend_from_slice(&42_i32.to_le_bytes());
        make_generation_10_object_file_with_external(23, 9001, &tree, &payload, "main.assets")
    }

    fn write_type_tree(output: &mut Vec<u8>, class_id: i32, nodes: &[(u8, i32, &str, &str, i32)]) {
        let mut string_buffer = Vec::new();
        let mut offsets = Vec::new();
        for (_, _, type_name, name, _) in nodes {
            let type_offset = push_type_tree_string(&mut string_buffer, type_name);
            let name_offset = push_type_tree_string(&mut string_buffer, name);
            offsets.push((type_offset, name_offset));
        }
        output.extend_from_slice(&class_id.to_le_bytes());
        output.extend_from_slice(&(nodes.len() as i32).to_le_bytes());
        output.extend_from_slice(&(string_buffer.len() as i32).to_le_bytes());
        for ((level, version, _, _, byte_size), (type_offset, name_offset)) in
            nodes.iter().zip(offsets)
        {
            output.extend_from_slice(&(*version as u16).to_le_bytes());
            output.push(*level);
            output.push(0);
            output.extend_from_slice(&type_offset.to_le_bytes());
            output.extend_from_slice(&name_offset.to_le_bytes());
            output.extend_from_slice(&byte_size.to_le_bytes());
            output.extend_from_slice(&0i32.to_le_bytes());
            output.extend_from_slice(&0u32.to_le_bytes());
        }
        output.extend_from_slice(&string_buffer);
    }

    fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn write_object_record(
        output: &mut Vec<u8>,
        path_id: i64,
        data_start: usize,
        data_size: usize,
        raw_type_id: i32,
        class_id: i16,
    ) {
        output.extend_from_slice(&i32::try_from(path_id).unwrap().to_le_bytes());
        output.extend_from_slice(&u32::try_from(data_start).unwrap().to_le_bytes());
        output.extend_from_slice(&i32::try_from(data_size).unwrap().to_le_bytes());
        output.extend_from_slice(&raw_type_id.to_le_bytes());
        output.extend_from_slice(&class_id.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
    }

    #[test]
    fn workspace_yaml_export_resolves_and_embeds_streamed_image_texture_bytes() {
        let source_bytes = make_streamed_asset_file(189, false);
        let (mut filesystem, workspace) = load_workspace_bytes(&source_bytes);
        filesystem
            .write_all_bytes("/game/texture.resS", b"0123456789")
            .unwrap();
        let catalog =
            explicit_root_resource_catalog(&filesystem, "texture.resS", "/game/texture.resS");
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let identity = asset(0, 77, 189);
        let pointer_map = pointer_map_for_asset(identity);
        let provider = PlatformResourceProvider::new(None, None);
        let context = WorkspaceStreamedYamlResourceContext {
            filesystem: &filesystem,
            root_catalog: &catalog,
            provider: &provider,
            content_limits: StreamedResourceContentLimits::default(),
            staging_limits: StreamedYamlAssetLimits::default(),
        };

        let document = workspace_object_to_unity_yaml_document_with_streamed_resources(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            identity,
            77,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
            &context,
        )
        .unwrap();
        let yaml =
            write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default()).unwrap();

        assert!(yaml.contains("m_ImageData_C189: 323334"));
        assert!(yaml.contains("offset: 0"));
        assert!(yaml.contains("size: 0"));
        assert!(!yaml.contains("texture.resS"));

        let assignment = SceneExportIdAssignment {
            asset: identity,
            file_id: 77,
        };
        let collection_yaml = workspace_assignments_to_unity_yaml_with_streamed_resources(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &[assignment],
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
            &context,
        )
        .unwrap();
        assert!(collection_yaml.contains("m_ImageData_C189: 323334"));
        let scene_yaml = workspace_scene_assignments_to_unity_yaml_with_streamed_resources(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &[assignment],
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
            &context,
        )
        .unwrap();
        assert!(scene_yaml.contains("m_ImageData_C189: 323334"));

        let payload = objects.objects()[0].payload.as_ref().unwrap();
        assert_eq!(
            crate::resource::extract_streamed_resource_info(
                payload
                    .root
                    .unique_field_in_base_chain("m_StreamData_C189")
                    .unwrap()
                    .unwrap(),
            )
            .unwrap()
            .path,
            "texture.resS"
        );
        assert_eq!(
            payload
                .root
                .unique_field_in_base_chain("m_ImageData_C189")
                .unwrap()
                .unwrap(),
            &crate::serialized_file::object::SerializedValue::Bytes(Vec::new())
        );
    }

    #[test]
    fn workspace_yaml_export_resolves_and_embeds_streamed_mesh_bytes() {
        let source_bytes = make_streamed_asset_file(43, true);
        let (mut filesystem, workspace) = load_workspace_bytes(&source_bytes);
        filesystem
            .write_all_bytes("/game/texture.resS", b"0123456789")
            .unwrap();
        let catalog =
            explicit_root_resource_catalog(&filesystem, "texture.resS", "/game/texture.resS");
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let identity = asset(0, 77, 43);
        let pointer_map = pointer_map_for_asset(identity);
        let provider = PlatformResourceProvider::new(None, None);
        let context = WorkspaceStreamedYamlResourceContext {
            filesystem: &filesystem,
            root_catalog: &catalog,
            provider: &provider,
            content_limits: StreamedResourceContentLimits::default(),
            staging_limits: StreamedYamlAssetLimits::default(),
        };

        let document = workspace_object_to_unity_yaml_document_with_streamed_resources(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            identity,
            77,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
            &context,
        )
        .unwrap();
        let yaml =
            write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default()).unwrap();

        assert!(yaml.contains("m_Data: 323334"));
        assert!(yaml.contains("offset: 0"));
        assert!(yaml.contains("size: 0"));
        assert!(!yaml.contains("texture.resS"));
        let payload = objects.objects()[0].payload.as_ref().unwrap();
        assert_eq!(
            crate::resource::extract_streamed_resource_info(
                payload
                    .root
                    .unique_field_in_base_chain("m_StreamData")
                    .unwrap()
                    .unwrap(),
            )
            .unwrap()
            .path,
            "texture.resS"
        );
        assert_eq!(
            payload
                .root
                .unique_field_in_base_chain("m_VertexData")
                .unwrap()
                .unwrap()
                .unique_field_in_base_chain("m_DataSize")
                .unwrap()
                .unwrap(),
            &crate::serialized_file::object::SerializedValue::Bytes(Vec::new())
        );
    }

    #[test]
    fn workspace_yaml_export_reports_missing_streamed_resource_without_null_or_empty_data() {
        let source_bytes = make_streamed_asset_file(189, true);
        let (filesystem, workspace) = load_workspace_bytes(&source_bytes);
        let empty_inputs: [ExplicitRootResourceInput<'_, '_>; 0] = [];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &empty_inputs,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let identity = asset(0, 77, 189);
        let pointer_map = pointer_map_for_asset(identity);
        let provider = PlatformResourceProvider::new(None, None);
        let context = WorkspaceStreamedYamlResourceContext {
            filesystem: &filesystem,
            root_catalog: &catalog,
            provider: &provider,
            content_limits: StreamedResourceContentLimits::default(),
            staging_limits: StreamedYamlAssetLimits::default(),
        };

        let no_context_error = workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            identity,
            77,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap_err();
        match no_context_error {
            UnityYamlWorkspaceExportError::StreamedAssetPreparation { source, .. } => {
                assert!(matches!(
                    *source,
                    StreamedYamlAssetError::StreamContentRequired { resource }
                        if resource.path == "texture.resS" && resource.offset == 2 && resource.size == 3
                ))
            }
            other => panic!("expected missing resource context error, got {other}"),
        }

        let copy_limited_context = WorkspaceStreamedYamlResourceContext {
            staging_limits: StreamedYamlAssetLimits {
                max_source_value_copy_bytes: 1,
                ..StreamedYamlAssetLimits::default()
            },
            ..context
        };
        let copy_limit_error = workspace_object_to_unity_yaml_document_with_streamed_resources(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            identity,
            77,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
            &copy_limited_context,
        )
        .unwrap_err();
        match copy_limit_error {
            UnityYamlWorkspaceExportError::StreamedAssetPreparation { source, .. } => {
                assert!(matches!(
                    *source,
                    StreamedYamlAssetError::SourceValueCopyLimitExceeded {
                        size,
                        limit: 1
                    } if size > 1
                ))
            }
            other => panic!("expected source-copy bound, got {other}"),
        }

        assert!(matches!(
            workspace_object_to_unity_yaml_document_with_streamed_resources(
                &workspace,
                &reference_graph,
                &pointer_map,
                &objects.objects()[0],
                identity,
                77,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
                &context,
            ),
            Err(UnityYamlWorkspaceExportError::StreamedResourceMissing {
                path,
                unavailable_data_paths,
                ..
            }) if path == "texture.resS" && unavailable_data_paths.is_empty()
        ));
        assert_eq!(
            objects.objects()[0]
                .payload
                .as_ref()
                .unwrap()
                .root
                .unique_field_in_base_chain("m_ImageData_C189")
                .unwrap()
                .unwrap(),
            &crate::serialized_file::object::SerializedValue::Bytes(Vec::new())
        );
    }

    #[test]
    fn exports_workspace_gameobject_component_and_transform_back_reference() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        assert_eq!(
            reference_graph.references()[0].field_path,
            "m_Component[0].component"
        );
        assert_eq!(
            reference_graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 1,
            }
        );

        let pointer_map = pointer_map();
        let game_object = workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            asset(0, GAME_OBJECT_PATH_ID, 1),
            GAME_OBJECT_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let transform = workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[1],
            asset(1, TRANSFORM_PATH_ID, 4),
            TRANSFORM_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[game_object, transform],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("--- !u!1 &42"));
        assert!(output.contains("serializedVersion: 6"));
        assert!(output.contains("component: {fileID: 43}"));
        assert!(output.contains("m_GameObject: {fileID: 42}"));
    }

    #[test]
    fn collection_executor_writes_documents_in_supplied_yaml_order() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert_eq!(objects.get(0, 0).unwrap().path_id, GAME_OBJECT_PATH_ID);
        assert_eq!(objects.get(0, 1).unwrap().path_id, TRANSFORM_PATH_ID);
        assert!(objects.get(1, 0).is_none());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let assignments = [
            SceneExportIdAssignment {
                asset: asset(1, TRANSFORM_PATH_ID, 4),
                file_id: TRANSFORM_PATH_ID,
            },
            SceneExportIdAssignment {
                asset: asset(0, GAME_OBJECT_PATH_ID, 1),
                file_id: GAME_OBJECT_PATH_ID,
            },
        ];
        let output = workspace_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map(),
            &assignments,
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let transform_position = output.find("--- !u!4 &43").unwrap();
        let game_object_position = output.find("--- !u!1 &42").unwrap();
        assert!(transform_position < game_object_position);
        assert!(output.contains("m_GameObject: {fileID: 42}"));
        assert!(output.contains("component: {fileID: 43}"));
    }

    #[test]
    fn scene_executor_applies_scene_ordering_before_writing() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let assignments = [
            SceneExportIdAssignment {
                asset: asset(0, GAME_OBJECT_PATH_ID, 1),
                file_id: GAME_OBJECT_PATH_ID,
            },
            SceneExportIdAssignment {
                asset: asset(1, TRANSFORM_PATH_ID, 4),
                file_id: TRANSFORM_PATH_ID,
            },
        ];
        let output = workspace_scene_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map(),
            &assignments,
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();
        assert!(output.find("--- !u!1 &42").unwrap() < output.find("--- !u!4 &43").unwrap());
    }

    #[test]
    fn scene_component_membership_ids_flow_through_workspace_yaml_export() {
        const COMPONENT_CLASS_ID: i32 = 23;
        const COMPONENT_PATH_ID: i64 = 9001;

        let bytes = make_mesh_renderer_component_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let assignments = plan_single_file_scene_assignments(&bytes, &reference_graph);
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].file_id, COMPONENT_PATH_ID);
        assert_eq!(assignments[0].asset.class_id, COMPONENT_CLASS_ID);

        let mut pointer_map_builder =
            ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments)
            .unwrap();
        let pointer_map = pointer_map_builder.build().unwrap();
        let output = workspace_scene_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments,
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();

        assert!(output.contains("--- !u!23 &9001"));
        assert!(output.contains("MeshRenderer:"));
        assert!(output.contains("m_GameObject: {fileID: 0}"));
    }

    #[test]
    fn scene_gameobject_component_nonnull_pptrs_follow_ids_into_yaml() {
        let bytes = make_game_object_mesh_renderer_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        assert_eq!(reference_graph.file_object_count(0), Some(2));
        assert_eq!(
            reference_graph.references()[0].resolution,
            crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 1,
            }
        );
        assert_eq!(
            reference_graph.references()[1].resolution,
            crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 0,
            }
        );

        let assignments = plan_single_file_scene_assignments(&bytes, &reference_graph);
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| (assignment.asset.object_index, assignment.file_id))
                .collect::<Vec<_>>(),
            [(0, GAME_OBJECT_PATH_ID), (1, 9001)]
        );
        let mut pointer_map_builder =
            ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments)
            .unwrap();
        let pointer_map = pointer_map_builder.build().unwrap();
        let output = workspace_scene_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments,
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();

        assert!(output.find("--- !u!1 &42").unwrap() < output.find("--- !u!23 &9001").unwrap());
        assert!(output.contains("component: {fileID: 9001}"));
        assert!(output.contains("m_GameObject: {fileID: 42}"));
    }

    #[test]
    fn scene_component_pointers_to_other_collections_emit_guid_and_type() {
        let bytes = make_game_object_mesh_renderer_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let assignments = plan_single_file_scene_assignments(&bytes, &reference_graph);
        assert_eq!(assignments.len(), 2);

        let mut pointer_map_builder =
            ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments[..1])
            .unwrap();
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX + 1, [0x82; 16], &assignments[1..])
            .unwrap();
        let pointer_map = pointer_map_builder.build().unwrap();
        let game_object_yaml = workspace_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments[..1],
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let mesh_renderer_yaml = workspace_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments[1..],
            COLLECTION_INDEX + 1,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();

        assert!(
            game_object_yaml.contains(
                "component: {fileID: 9001, guid: 82828282828282828282828282828282, type: 2}"
            ),
            "{game_object_yaml}"
        );
        assert!(
            mesh_renderer_yaml.contains(
                "m_GameObject: {fileID: 42, guid: 71717171717171717171717171717171, type: 2}"
            ),
            "{mesh_renderer_yaml}"
        );
    }

    #[test]
    fn cross_file_component_pptrs_reach_separate_scene_yaml_collections() {
        let game_object_bytes = make_game_object_with_external_component();
        let mesh_renderer_bytes = make_mesh_renderer_with_external_game_object();
        let source_files = [
            ("main.assets", game_object_bytes.as_slice()),
            ("shared.assets", mesh_renderer_bytes.as_slice()),
        ];
        let (filesystem, workspace) = load_workspace_files(&source_files);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        assert_eq!(reference_graph.file_object_count(0), Some(1));
        assert_eq!(reference_graph.file_object_count(1), Some(1));
        assert_eq!(reference_graph.references().len(), 2);
        assert_eq!(
            reference_graph.references()[0].resolution,
            crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
        assert_eq!(
            reference_graph.references()[1].resolution,
            crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 0,
            }
        );

        let assignments = plan_scene_assignments_for_workspace(&source_files, &reference_graph);
        assert_eq!(assignments.len(), 2);
        assert_eq!(assignments[0].asset.path_id, 42);
        assert_eq!(assignments[1].asset.path_id, 9001);
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| assignment.file_id)
                .collect::<Vec<_>>(),
            [42, 9001]
        );

        let mut pointer_map_builder =
            ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments[..1])
            .unwrap();
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX + 1, [0x82; 16], &assignments[1..])
            .unwrap();
        let pointer_map = pointer_map_builder.build().unwrap();
        let game_object_yaml = workspace_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments[..1],
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let mesh_renderer_yaml = workspace_assignments_to_unity_yaml(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments[1..],
            COLLECTION_INDEX + 1,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
        )
        .unwrap();

        assert!(game_object_yaml.contains("--- !u!1 &42"));
        assert!(mesh_renderer_yaml.contains("--- !u!23 &9001"));
        assert!(game_object_yaml.contains(
            "component: {fileID: 9001, guid: 82828282828282828282828282828282, type: 2}"
        ));
        assert!(mesh_renderer_yaml.contains(
            "m_GameObject: {fileID: 42, guid: 71717171717171717171717171717171, type: 2}"
        ));
    }

    #[test]
    fn pptr_dictionary_values_resolve_and_emit_exported_collection_pointer() {
        const SOURCE_PATH_ID: i64 = 51;
        const TARGET_COLLECTION_INDEX: usize = COLLECTION_INDEX + 1;
        const TARGET_FILE_ID: i64 = 142;

        let dictionary_bytes = make_game_object_with_external_pptr_dictionary();
        let target_bytes = make_game_object_and_transform_file(TRANSFORM_PATH_ID);
        let source_files = [
            ("main.assets", dictionary_bytes.as_slice()),
            ("target.assets", target_bytes.as_slice()),
        ];
        let (filesystem, workspace) = load_workspace_files(&source_files);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete());

        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let dictionary_edges: Vec<_> = reference_graph.references_from_object(0, 0).collect();
        assert_eq!(dictionary_edges.len(), 1);
        assert_eq!(dictionary_edges[0].field_path, "m_Targets[0].second");
        assert_eq!(dictionary_edges[0].file_id, 1);
        assert_eq!(dictionary_edges[0].path_id, GAME_OBJECT_PATH_ID);
        assert_eq!(
            dictionary_edges[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );

        let target_asset = ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 1 },
            object_index: 0,
            path_id: GAME_OBJECT_PATH_ID,
            class_id: 1,
        };
        let pointer_map = ProjectExportPointerMap::new(
            &[ExportedAssetPointerEntry {
                asset: target_asset,
                file_id: TARGET_FILE_ID,
                collection_index: TARGET_COLLECTION_INDEX,
            }],
            &[ExportCollectionPointerMetadata {
                collection_index: TARGET_COLLECTION_INDEX,
                guid: [0x82; 16],
                asset_type: UnityProjectAssetType::Serialized,
            }],
            ExportPointerMapLimits::default(),
        )
        .unwrap();
        let document = workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            asset(0, SOURCE_PATH_ID, 1),
            SOURCE_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let yaml = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();

        assert!(yaml.contains("--- !u!1 &51"), "{yaml}");
        assert!(
            yaml.contains("target: {fileID: 142, guid: 82828282828282828282828282828282, type: 2}"),
            "{yaml}"
        );
    }

    #[test]
    fn collection_executor_reports_processed_assets_without_a_generated_walker() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let processed = SceneExportIdAssignment {
            asset: ExportAssetIdentity {
                collection: ExportAssetCollectionIdentity::ProcessedCollection {
                    collection_index: 5,
                },
                object_index: 0,
                path_id: 1,
                class_id: 4,
            },
            file_id: 123,
        };
        assert!(matches!(
            workspace_assignments_to_unity_yaml(
                &workspace,
                &objects,
                &reference_graph,
                &pointer_map(),
                &[processed],
                COLLECTION_INDEX,
                None,
                UnityYamlWorkspaceCollectionLimits::default(),
            ),
            Err(
                UnityYamlWorkspaceCollectionError::ProcessedAssetRequiresGeneratedWalker {
                    assignment_index: 0,
                    ..
                }
            )
        ));
    }

    #[test]
    fn refuses_workspace_payload_bytes_or_reference_edges_that_do_not_match() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let pointer_map = pointer_map();

        let mut with_trailing_bytes = objects.objects()[0].clone();
        with_trailing_bytes
            .payload
            .as_mut()
            .unwrap()
            .trailing_bytes
            .push(0xaa);
        assert!(matches!(
            workspace_object_to_unity_yaml_document(
                &workspace,
                &reference_graph,
                &pointer_map,
                &with_trailing_bytes,
                asset(0, GAME_OBJECT_PATH_ID, 1),
                GAME_OBJECT_PATH_ID,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlWorkspaceExportError::UnparsedTrailingPayload { byte_count: 1, .. })
        ));

        let mut with_wrong_pointer = objects.objects()[0].clone();
        let payload = with_wrong_pointer.payload.as_mut().unwrap();
        let crate::serialized_file::object::SerializedValue::Struct(fields) = &mut payload.root
        else {
            panic!("fixture root is a struct")
        };
        let crate::serialized_file::object::SerializedValue::Array(components) =
            &mut fields[0].value
        else {
            panic!("fixture component field is an array")
        };
        let crate::serialized_file::object::SerializedValue::Struct(pair_fields) =
            &mut components[0]
        else {
            panic!("fixture component value is a struct")
        };
        pair_fields[0].value = crate::serialized_file::object::SerializedValue::PPtr {
            file_id: 0,
            path_id: 999,
        };
        assert!(matches!(
            workspace_object_to_unity_yaml_document(
                &workspace,
                &reference_graph,
                &pointer_map,
                &with_wrong_pointer,
                asset(0, GAME_OBJECT_PATH_ID, 1),
                GAME_OBJECT_PATH_ID,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlWorkspaceExportError::ReferenceEdgeMissing { path_id: 999, .. })
        ));
    }

    #[test]
    fn missing_local_component_reference_remains_an_export_error() {
        let (filesystem, workspace) = load_workspaces(999);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let pointer_map = pointer_map();
        assert!(matches!(
            workspace_object_to_unity_yaml_document(
                &workspace,
                &reference_graph,
                &pointer_map,
                &objects.objects()[0],
                asset(0, GAME_OBJECT_PATH_ID, 1),
                GAME_OBJECT_PATH_ID,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlWorkspaceExportError::Conversion(
                UnityYamlObjectConversionError::PointerConversion { .. }
            ))
        ));
    }
}
