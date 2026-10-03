//! Unity YAML export bridge for objects decoded from a SerializedFile workspace.

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str;

use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::PlatformResourceProvider;
use crate::processing::prefab_processing::{
    materialize_planned_game_object_component_append,
    materialize_planned_generated_transform_payload, WorkspaceGeneratedConstructorPointer,
    WorkspaceGeneratedPointerField, WorkspaceGeneratedTransformPayload,
    WorkspaceProcessedCollectionKind, WorkspaceProcessedDependencyTarget,
    WorkspaceProcessedPrefabAssetKind, WorkspaceProcessedPrefabAssetPlan,
    WorkspaceSourceDependencyAppendTarget, WorkspaceSourceDependencyPointerField,
};
use crate::processing::scene_naming::SceneUnityVersion;
use crate::resource::{
    resolve_and_read_streamed_resource_info, RootResourceFileCatalog, StreamedResourceContentError,
    StreamedResourceContentLimits, StreamedResourceContentResolution,
};
use crate::serialized_file::metadata::SerializedFileMetadata;
use crate::serialized_file::object::{
    ObjectDefaultValueLimits, ObjectEncodeLimits, ParsedObjectPayload, SerializedValue,
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

/// Type metadata and processed-asset plan needed to execute generated Transform assignments.
#[derive(Clone, Copy)]
pub struct WorkspaceGeneratedTransformYamlContext<'a> {
    pub processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
    pub scene_index: usize,
    pub metadata: &'a SerializedFileMetadata,
    pub transform_type_index: usize,
    pub default_value_limits: ObjectDefaultValueLimits,
    pub encode_limits: ObjectEncodeLimits,
}

impl<'a> WorkspaceGeneratedTransformYamlContext<'a> {
    pub fn new(
        processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
        scene_index: usize,
        metadata: &'a SerializedFileMetadata,
        transform_type_index: usize,
    ) -> Self {
        Self {
            processed_assets,
            scene_index,
            metadata,
            transform_type_index,
            default_value_limits: ObjectDefaultValueLimits::default(),
            encode_limits: ObjectEncodeLimits::default(),
        }
    }
}

struct WorkspaceGeneratedComponentYamlPointer {
    source: ExportAssetIdentity,
    field_path: String,
    file_id: i32,
    path_id: i64,
    target: ExportAssetIdentity,
}

struct WorkspaceGeneratedComponentYamlOverlay {
    payload: ParsedObjectPayload,
    pointer: WorkspaceGeneratedComponentYamlPointer,
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
        None,
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
    generated_transforms: Option<&WorkspaceGeneratedTransformYamlContext<'_>>,
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
        let document = match asset.collection {
            ExportAssetCollectionIdentity::SerializedFile { file_index } => {
                let object = objects.get(file_index, asset.object_index).ok_or(
                    UnityYamlWorkspaceCollectionError::WorkspaceObjectMissing {
                        assignment_index,
                        asset,
                    },
                )?;
                let generated_component_overlay = generated_transforms
                    .map(|context| {
                        workspace_generated_component_yaml_overlay(
                            workspace, object, asset, context,
                        )
                    })
                    .transpose()
                    .map_err(|source| UnityYamlWorkspaceCollectionError::ObjectExport {
                        assignment_index,
                        source: Box::new(source),
                    })?
                    .flatten();
                workspace_object_to_unity_yaml_document_with_context(
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
                    generated_component_overlay.as_ref(),
                )
                .map_err(|source| {
                    UnityYamlWorkspaceCollectionError::ObjectExport {
                        assignment_index,
                        source: Box::new(source),
                    }
                })?
            }
            ExportAssetCollectionIdentity::ProcessedCollection { .. } => {
                let Some(generated_transforms) = generated_transforms else {
                    return Err(
                        UnityYamlWorkspaceCollectionError::ProcessedAssetRequiresGeneratedWalker {
                            assignment_index,
                            asset,
                        },
                    );
                };
                let document = workspace_generated_transform_assignment_to_unity_yaml_document(
                    workspace,
                    reference_graph,
                    pointer_map,
                    asset,
                    file_id,
                    current_collection_index,
                    missing_reference_guid,
                    limits.object_conversion,
                    generated_transforms,
                )
                .map_err(|source| {
                    UnityYamlWorkspaceCollectionError::ObjectExport {
                        assignment_index,
                        source: Box::new(source),
                    }
                })?;
                Some(document)
            }
        };
        if let Some(document) = document {
            documents.push(document);
        }
    }

    write_unity_yaml_documents(&documents, limits.yaml_write)
        .map_err(UnityYamlWorkspaceCollectionError::YamlWrite)
}

#[allow(clippy::too_many_arguments)]
fn workspace_generated_transform_assignment_to_unity_yaml_document(
    workspace: &SerializedFileWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    source: ExportAssetIdentity,
    file_id: i64,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    context: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<UnityYamlDocument, UnityYamlWorkspaceExportError> {
    let invariant_error = |message: String| {
        UnityYamlWorkspaceExportError::GeneratedTransformInvariant { source, message }
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
    let ExportAssetCollectionIdentity::ProcessedCollection { collection_index } = source.collection
    else {
        return Err(invariant_error(
            "generated Transform assignment has SerializedFile collection scope".to_owned(),
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
        || collection.kind
            != (WorkspaceProcessedCollectionKind::SceneGeneratedAssets {
                scene_index: context.scene_index,
            })
    {
        return Err(invariant_error(format!(
            "processed collection {collection_index} is not the generated collection for Scene {}",
            context.scene_index
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
    {
        return Err(invariant_error(
            "processed asset identity disagrees with the YAML assignment".to_owned(),
        ));
    }
    let WorkspaceProcessedPrefabAssetKind::GeneratedTransform { game_object, .. } = &asset.kind
    else {
        return Err(invariant_error(format!(
            "processed asset {}:{} is not a generated Transform",
            collection_index, source.object_index
        )));
    };
    let mut constructor_pointers = context
        .processed_assets
        .constructor_pointers()
        .iter()
        .filter(|pointer| {
            pointer.source_asset == asset.identity
                && pointer.field == WorkspaceGeneratedPointerField::TransformGameObject
        });
    let constructor_pointer = constructor_pointers.next().copied().ok_or_else(|| {
        invariant_error("planned Transform has no constructor GameObject PPtr".to_owned())
    })?;
    if constructor_pointers.next().is_some() {
        return Err(invariant_error(
            "planned Transform has duplicate constructor GameObject PPtrs".to_owned(),
        ));
    }
    if constructor_pointer.target != *game_object {
        return Err(invariant_error(
            "constructor PPtr target disagrees with the generated Transform plan".to_owned(),
        ));
    }

    let target_file = workspace
        .files()
        .get(game_object.file_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "planned GameObject file {} is absent from the workspace",
                game_object.file_index
            ))
        })?;
    let target_object = target_file
        .metadata
        .objects
        .get(game_object.object_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "planned GameObject object {} is absent from file {}",
                game_object.object_index, game_object.file_index
            ))
        })?;
    if target_object.path_id != game_object.path_id || target_object.raw_type_id != 1 {
        return Err(invariant_error(
            "planned GameObject identity disagrees with its SerializedFile object record"
                .to_owned(),
        ));
    }
    let reference_identity = reference_graph
        .object_identity(game_object.file_index, game_object.object_index)
        .ok_or_else(|| {
            invariant_error("reference graph has no planned GameObject identity".to_owned())
        })?;
    if reference_identity.path_id != game_object.path_id
        || reference_graph.object_class_id(game_object.file_index, game_object.object_index)
            != Some(1)
    {
        return Err(invariant_error(
            "planned GameObject identity disagrees with the reference graph".to_owned(),
        ));
    }

    let payload = materialize_planned_generated_transform_payload(
        context.processed_assets,
        asset.identity,
        context.metadata,
        context.transform_type_index,
        context.default_value_limits,
        context.encode_limits,
    )
    .map_err(|error| invariant_error(error.to_string()))?;
    generated_transform_payload_to_unity_yaml_document(
        &payload,
        context.metadata,
        source,
        file_id,
        constructor_pointer,
        pointer_map,
        current_collection_index,
        missing_reference_guid,
        limits,
    )
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

/// Order and export Scene YAML assignments with the planned generated Transform walker.
#[allow(clippy::too_many_arguments)]
pub fn workspace_scene_assignments_to_unity_yaml_with_generated_transforms(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    generated_transforms: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let ordered_assignments = order_scene_yaml_assignments(
        assignments_in_hierarchy_order,
        SceneYamlAssetOrderLimits {
            max_assets: limits.max_documents,
        },
    )
    .map_err(UnityYamlWorkspaceCollectionError::SceneOrder)?;
    workspace_assignments_to_unity_yaml_with_context(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        &ordered_assignments,
        current_collection_index,
        missing_reference_guid,
        limits,
        None,
        Some(generated_transforms),
    )
}

/// Scene YAML export with both streamed asset staging and generated Transform support.
#[allow(clippy::too_many_arguments)]
pub fn workspace_scene_assignments_to_unity_yaml_with_streamed_resources_and_generated_transforms(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    streamed_resources: &WorkspaceStreamedYamlResourceContext<'_, '_>,
    generated_transforms: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let ordered_assignments = order_scene_yaml_assignments(
        assignments_in_hierarchy_order,
        SceneYamlAssetOrderLimits {
            max_assets: limits.max_documents,
        },
    )
    .map_err(UnityYamlWorkspaceCollectionError::SceneOrder)?;
    workspace_assignments_to_unity_yaml_with_context(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        &ordered_assignments,
        current_collection_index,
        missing_reference_guid,
        limits,
        Some(streamed_resources),
        Some(generated_transforms),
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

fn workspace_generated_component_yaml_overlay(
    workspace: &SerializedFileWorkspace,
    object: &WorkspaceObject,
    source: ExportAssetIdentity,
    context: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<Option<WorkspaceGeneratedComponentYamlOverlay>, UnityYamlWorkspaceExportError> {
    if source.class_id != 1 {
        return Ok(None);
    }
    let invariant_error = |message: String| {
        UnityYamlWorkspaceExportError::GeneratedComponentOverlayInvariant { source, message }
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
    if object.file_index
        != match source.collection {
            ExportAssetCollectionIdentity::SerializedFile { file_index } => file_index,
            ExportAssetCollectionIdentity::ProcessedCollection { .. } => {
                return Err(invariant_error(
                    "GameObject overlay source has processed-collection scope".to_owned(),
                ));
            }
        }
        || object.object_index != source.object_index
        || object.path_id != source.path_id
    {
        return Err(invariant_error(
            "GameObject overlay source identity differs from its workspace object".to_owned(),
        ));
    }

    let mut generated_collections =
        context
            .processed_assets
            .collections()
            .iter()
            .filter(|collection| {
                collection.kind
                    == (WorkspaceProcessedCollectionKind::SceneGeneratedAssets {
                        scene_index: context.scene_index,
                    })
            });
    let scene_generated_collection = generated_collections.next().ok_or_else(|| {
        invariant_error(format!(
            "processed plan has no generated collection for Scene {}",
            context.scene_index
        ))
    })?;
    if generated_collections.next().is_some() {
        return Err(invariant_error(format!(
            "processed plan has multiple generated collections for Scene {}",
            context.scene_index
        )));
    }
    let generated_collection_index = scene_generated_collection.collection_index;
    let mut matching_overlays = context
        .processed_assets
        .component_list_overlays()
        .iter()
        .filter(|overlay| {
            overlay.game_object.file_index == object.file_index
                && overlay.game_object.object_index == object.object_index
                && overlay.game_object.path_id == source.path_id
                && overlay.game_object.class_id == Some(1)
                && overlay.generated_component.component_asset.collection_index
                    == generated_collection_index
        });
    let Some(overlay) = matching_overlays.next() else {
        return Ok(None);
    };
    if matching_overlays.next().is_some() {
        return Err(invariant_error(
            "source GameObject has duplicate generated-component overlays for the selected Scene"
                .to_owned(),
        ));
    }
    if overlay.generated_component.game_object != overlay.game_object
        || overlay.generated_component.component_class_id != 4
        || overlay.generated_component.component_asset.class_id != 4
    {
        return Err(invariant_error(
            "generated component overlay identity or class is not a Transform".to_owned(),
        ));
    }

    let generated_collection = context
        .processed_assets
        .collections()
        .get(generated_collection_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "generated collection {generated_collection_index} is outside the processed plan"
            ))
        })?;
    let mut matching_generated_assets = generated_collection
        .assets
        .iter()
        .enumerate()
        .filter(|(_, asset)| asset.identity == overlay.generated_component.component_asset);
    let (generated_object_index, generated_asset) =
        matching_generated_assets.next().ok_or_else(|| {
            invariant_error("overlay Transform is absent from its generated collection".to_owned())
        })?;
    if matching_generated_assets.next().is_some()
        || !matches!(
            generated_asset.kind,
            WorkspaceProcessedPrefabAssetKind::GeneratedTransform { game_object, .. }
                if game_object == overlay.game_object
        )
    {
        return Err(invariant_error(
            "overlay target is not the uniquely planned Transform for this GameObject".to_owned(),
        ));
    }
    let target_asset = ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::ProcessedCollection {
            collection_index: generated_collection_index,
        },
        object_index: generated_object_index,
        path_id: generated_asset.identity.path_id,
        class_id: generated_asset.identity.class_id,
    };
    let mut matching_constructor_pointers = context
        .processed_assets
        .constructor_pointers()
        .iter()
        .filter(|pointer| {
            pointer.source_asset == generated_asset.identity
                && pointer.field == WorkspaceGeneratedPointerField::TransformGameObject
        });
    let constructor_pointer = matching_constructor_pointers.next().ok_or_else(|| {
        invariant_error("overlay Transform has no constructor GameObject PPtr".to_owned())
    })?;
    if matching_constructor_pointers.next().is_some()
        || constructor_pointer.target != overlay.game_object
        || constructor_pointer.path_id != overlay.game_object.path_id
    {
        return Err(invariant_error(
            "overlay Transform constructor PPtr does not target its GameObject".to_owned(),
        ));
    }
    let mut matching_dependencies = generated_collection
        .dependencies
        .iter()
        .filter(|dependency| dependency.file_id == constructor_pointer.file_id);
    let dependency = matching_dependencies.next().ok_or_else(|| {
        invariant_error("overlay Transform source dependency is absent".to_owned())
    })?;
    if matching_dependencies.next().is_some()
        || dependency.target
            != (WorkspaceProcessedDependencyTarget::SerializedFileCollection {
                file_index: overlay.game_object.file_index,
            })
    {
        return Err(invariant_error(
            "overlay Transform dependency does not uniquely target its GameObject file".to_owned(),
        ));
    }

    let mut matching_dependency_appends = context
        .processed_assets
        .source_dependency_appends()
        .iter()
        .filter(|append| {
            append.source_file_index == overlay.game_object.file_index
                && append.pointer_field
                    == WorkspaceSourceDependencyPointerField::GameObjectComponent
                && append.target_collection
                    == (WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index: generated_collection_index,
                    })
        });
    let dependency_append = matching_dependency_appends.next().ok_or_else(|| {
        invariant_error("source collection has no generated-Transform dependency append".to_owned())
    })?;
    if matching_dependency_appends.next().is_some() {
        return Err(invariant_error(
            "source collection has duplicate generated-Transform dependency appends".to_owned(),
        ));
    }
    let dependency_file_id = dependency_append.file_id.ok_or_else(|| {
        invariant_error("generated-Transform dependency FileID is deferred".to_owned())
    })?;
    if dependency_file_id <= 0 {
        return Err(invariant_error(format!(
            "generated-Transform dependency FileID {dependency_file_id} is not positive"
        )));
    }

    let file = workspace.files().get(object.file_index).ok_or(
        UnityYamlWorkspaceExportError::MissingWorkspaceFile {
            file_index: object.file_index,
        },
    )?;
    let type_index =
        object
            .resolved_type_index
            .ok_or(UnityYamlWorkspaceExportError::MissingResolvedType {
                file_index: object.file_index,
                object_index: object.object_index,
            })?;
    let serialized_type = file.metadata.types.get(type_index).ok_or(
        UnityYamlWorkspaceExportError::InvalidResolvedType {
            file_index: object.file_index,
            object_index: object.object_index,
            type_index,
            type_count: file.metadata.types.len(),
        },
    )?;
    if serialized_type.raw_type_id != 1 {
        return Err(invariant_error(format!(
            "GameObject object resolves to class ID {}, expected 1",
            serialized_type.raw_type_id
        )));
    }
    let type_tree = serialized_type.type_tree.as_ref().ok_or(
        UnityYamlWorkspaceExportError::MissingTypeTree {
            file_index: object.file_index,
            object_index: object.object_index,
            type_index,
        },
    )?;
    let element_node_index =
        game_object_component_element_node_index(type_tree).map_err(invariant_error)?;
    let empty_element_template = file
        .metadata
        .default_value_for_type_tree_node(
            object.object_index,
            type_index,
            element_node_index,
            context.default_value_limits,
        )
        .map_err(|error| invariant_error(error.to_string()))?;
    let field_path = generated_component_pointer_field_path(
        &empty_element_template,
        overlay.generated_component.appended_index,
    )
    .map_err(invariant_error)?;
    let source_payload =
        object
            .payload
            .as_ref()
            .map_err(|error| UnityYamlWorkspaceExportError::ObjectDecode {
                file_index: object.file_index,
                object_index: object.object_index,
                message: error.to_string(),
            })?;
    let edited_payload = materialize_planned_game_object_component_append(
        context.processed_assets,
        source_payload,
        overlay,
        Some(&empty_element_template),
        context.encode_limits.max_array_elements,
    )
    .map_err(|error| invariant_error(error.to_string()))?;
    Ok(Some(WorkspaceGeneratedComponentYamlOverlay {
        payload: edited_payload,
        pointer: WorkspaceGeneratedComponentYamlPointer {
            source,
            field_path,
            file_id: dependency_file_id,
            path_id: generated_asset.identity.path_id,
            target: target_asset,
        },
    }))
}

fn game_object_component_element_node_index(
    type_tree: &crate::serialized_file::metadata::TypeTree,
) -> Result<usize, String> {
    let mut component_nodes = type_tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.name.as_deref() == Some("m_Component"));
    let (component_node_index, component_node) = component_nodes
        .next()
        .ok_or_else(|| "GameObject TypeTree has no m_Component field".to_owned())?;
    if component_nodes.next().is_some() {
        return Err("GameObject TypeTree has duplicate m_Component fields".to_owned());
    }
    if component_node.type_name.as_deref() != Some("vector") {
        return Err(format!(
            "GameObject m_Component has TypeTree type {:?}, expected vector",
            component_node.type_name
        ));
    }
    let array_node_index = type_tree_unique_direct_child(type_tree, component_node_index, "Array")?;
    let array_node = &type_tree.nodes[array_node_index];
    if array_node.type_name.as_deref() != Some("Array") {
        return Err("GameObject m_Component Array node has an unexpected type".to_owned());
    }
    let size_node_index = type_tree_unique_direct_child(type_tree, array_node_index, "size")?;
    let element_node_index = type_tree_unique_direct_child(type_tree, array_node_index, "data")?;
    if size_node_index >= element_node_index {
        return Err("GameObject m_Component TypeTree places data before size".to_owned());
    }
    Ok(element_node_index)
}

fn type_tree_unique_direct_child(
    type_tree: &crate::serialized_file::metadata::TypeTree,
    parent_index: usize,
    child_name: &str,
) -> Result<usize, String> {
    let parent = type_tree
        .nodes
        .get(parent_index)
        .ok_or_else(|| format!("TypeTree parent node {parent_index} is unavailable"))?;
    let child_level = parent
        .level
        .checked_add(1)
        .ok_or_else(|| format!("TypeTree node {parent_index} cannot have children"))?;
    let mut found = None;
    for (node_index, node) in type_tree.nodes.iter().enumerate().skip(parent_index + 1) {
        if node.level <= parent.level {
            break;
        }
        if node.level == child_level
            && node.name.as_deref() == Some(child_name)
            && found.replace(node_index).is_some()
        {
            return Err(format!(
                "TypeTree node {parent_index} has duplicate direct children named {child_name:?}"
            ));
        }
    }
    found.ok_or_else(|| {
        format!("TypeTree node {parent_index} has no direct child named {child_name:?}")
    })
}

fn generated_component_pointer_field_path(
    template: &SerializedValue,
    appended_index: usize,
) -> Result<String, String> {
    let suffix = if template.pptr().is_some() {
        String::new()
    } else if template
        .struct_fields()
        .and_then(|fields| {
            (fields.len() == 1 && fields[0].name == "component").then_some(&fields[0].value)
        })
        .and_then(SerializedValue::pptr)
        .is_some()
    {
        ".component".to_owned()
    } else {
        return Err(
            "GameObject m_Component element TypeTree is neither a PPtr nor a ComponentPair"
                .to_owned(),
        );
    };
    Ok(format!("m_Component[{appended_index}]{suffix}"))
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
    let Some(document) = workspace_object_to_unity_yaml_document_with_context(
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
        None,
    )?
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
        None,
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
    generated_component_overlay: Option<&WorkspaceGeneratedComponentYamlOverlay>,
) -> Result<Option<UnityYamlDocument>, UnityYamlWorkspaceExportError> {
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
        189 => Some(StreamedYamlAssetKind::ImageTexture),
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
    let source_export_root = generated_component_overlay
        .map(|overlay| &overlay.payload.root)
        .unwrap_or(&payload.root);
    let export_root = streamed_export_root.as_ref().unwrap_or(source_export_root);

    let mut pointer_error = None;
    let mut generated_component_pointer_seen = false;
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
    if generated_component_overlay.is_some() && !generated_component_pointer_seen {
        return Err(
            UnityYamlWorkspaceExportError::GeneratedComponentOverlayInvariant {
                source,
                message: "generated component PPtr was not visited by the YAML walker".to_owned(),
            },
        );
    }
    document
        .map(Some)
        .map_err(UnityYamlWorkspaceExportError::Conversion)
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
    GeneratedTransformInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    GeneratedComponentOverlayInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    ImageTextureSkippedWithoutContent {
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
            Self::GeneratedTransformInvariant { source, message } => write!(
                f,
                "generated Transform YAML export failed for {source:?}: {message}"
            ),
            Self::GeneratedComponentOverlayInvariant { source, message } => write!(
                f,
                "generated GameObject component overlay export failed for {source:?}: {message}"
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
    use crate::serialized_file::metadata::{SerializedType, TypeTree, TypeTreeNode};
    use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
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
        pointer_map_for_assets(&[asset])
    }

    fn pointer_map_for_assets(assets: &[ExportAssetIdentity]) -> ProjectExportPointerMap {
        let entries = assets
            .iter()
            .map(|&asset| ExportedAssetPointerEntry {
                asset,
                file_id: asset.path_id,
                collection_index: COLLECTION_INDEX,
            })
            .collect::<Vec<_>>();
        let collections = [ExportCollectionPointerMetadata {
            collection_index: COLLECTION_INDEX,
            guid: [0x72; 16],
            asset_type: UnityProjectAssetType::Serialized,
        }];
        ProjectExportPointerMap::new(&entries, &collections, ExportPointerMapLimits::default())
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
        explicit_root_resource_catalog_for_files(filesystem, &[(file_name, virtual_path)])
    }

    fn explicit_root_resource_catalog_for_files(
        filesystem: &VirtualFileSystem,
        resources: &[(&str, &str)],
    ) -> RootResourceFileCatalog {
        let dispatched = resources
            .iter()
            .map(|_| {
                SchemeReader::read_with_limits(b"root resource", SchemeReaderLimits::default())
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let inputs = resources
            .iter()
            .zip(&dispatched)
            .map(
                |(&(file_name, virtual_path), dispatched_file)| ExplicitRootResourceInput {
                    file_name,
                    virtual_path,
                    dispatched_file,
                },
            )
            .collect::<Vec<_>>();
        RootResourceFileCatalog::from_explicit_resource_inputs(
            filesystem,
            &inputs,
            RootResourceCatalogLimits::default(),
        )
        .unwrap()
    }

    fn make_streamed_asset_file(class_id: i32, offset_is_u64: bool) -> Vec<u8> {
        make_streamed_asset_file_with_stream_path(class_id, offset_is_u64, "texture.resS")
    }

    fn make_streamed_asset_file_with_stream_path(
        class_id: i32,
        offset_is_u64: bool,
        stream_path: &str,
    ) -> Vec<u8> {
        make_streamed_asset_file_with_stream_info(class_id, offset_is_u64, stream_path, 3)
    }

    fn make_streamed_asset_file_with_stream_info(
        class_id: i32,
        offset_is_u64: bool,
        stream_path: &str,
        stream_size: u32,
    ) -> Vec<u8> {
        make_streamed_asset_file_with_inline_data(
            class_id,
            offset_is_u64,
            stream_path,
            stream_size,
            &[],
        )
    }

    fn make_streamed_asset_file_with_inline_data(
        class_id: i32,
        offset_is_u64: bool,
        stream_path: &str,
        stream_size: u32,
        inline_data: &[u8],
    ) -> Vec<u8> {
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
        payload.extend_from_slice(&i32::try_from(stream_path.len()).unwrap().to_le_bytes());
        payload.extend_from_slice(stream_path.as_bytes());
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        if offset_is_u64 {
            payload.extend_from_slice(&2u64.to_le_bytes());
        } else {
            payload.extend_from_slice(&2u32.to_le_bytes());
        }
        payload.extend_from_slice(&stream_size.to_le_bytes());
        payload.extend_from_slice(&i32::try_from(inline_data.len()).unwrap().to_le_bytes());
        payload.extend_from_slice(inline_data);

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
        let hierarchies = scene_hierarchies_for_workspace(files, reference_graph);
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

    fn scene_hierarchies_for_workspace(
        files: &[(&str, &[u8])],
        reference_graph: &MultiFileReferenceGraph<'_>,
    ) -> crate::processing::scene_hierarchy::WorkspaceSceneHierarchies {
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
        hierarchy_builder.finish(&definitions).unwrap()
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

    fn make_game_object_without_transform_file() -> Vec<u8> {
        let game_object_tree = [
            (0, 6, "GameObject", "", -1),
            (1, 1, "string", "m_Name", -1),
            (2, 1, "Array", "Array", -1),
            (3, 1, "int", "size", 4),
            (3, 1, "char", "data", 1),
            (1, 1, "bool", "m_IsActive", 1),
            (1, 1, "vector", "m_Component", -1),
            (2, 1, "Array", "Array", -1),
            (3, 1, "int", "size", 4),
            (3, 1, "ComponentPair", "data", -1),
            (4, 1, "PPtr<Transform>", "component", 12),
            (5, 1, "SInt32", "m_FileID", 4),
            (5, 1, "SInt64", "m_PathID", 8),
        ];
        let mut metadata = b"2019.4.0f1\0".to_vec();
        metadata.extend_from_slice(&0u32.to_le_bytes());
        metadata.extend_from_slice(&1i32.to_le_bytes());
        write_type_tree(&mut metadata, 1, &game_object_tree);
        metadata.extend_from_slice(&0u32.to_le_bytes()); // generation-10 long FileID flag
        metadata.extend_from_slice(&1i32.to_le_bytes());
        let mut payload = Vec::new();
        payload.extend_from_slice(&4i32.to_le_bytes());
        payload.extend_from_slice(b"Root");
        payload.push(1); // active GameObject
        payload.extend_from_slice(&0i32.to_le_bytes()); // empty m_Component array
        write_object_record(&mut metadata, GAME_OBJECT_PATH_ID, 0, payload.len(), 1, 1);
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

    fn generated_transform_metadata() -> SerializedFileMetadata {
        let node = |level, name: &str, type_name: &str, byte_size| TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size,
            index: 0,
            meta_flags: 0,
            reference_type_hash: None,
        };
        let nodes = vec![
            node(0, "", "Transform", -1),
            node(1, "Base", "Component", -1),
            node(2, "m_GameObject", "PPtr<GameObject>", 12),
            node(3, "m_FileID", "SInt32", 4),
            node(3, "m_PathID", "SInt64", 8),
            node(1, "m_Father", "PPtr<Transform>", 12),
            node(2, "m_FileID", "SInt32", 4),
            node(2, "m_PathID", "SInt64", 8),
            node(1, "m_Children", "vector", -1),
            node(2, "Array", "Array", -1),
            node(3, "size", "int", 4),
            node(3, "data", "PPtr<Transform>", 12),
            node(4, "m_FileID", "SInt32", 4),
            node(4, "m_PathID", "SInt64", 8),
            node(1, "m_RootOrder", "int", 4),
            node(1, "m_LocalEulerAnglesHint", "Vector3f", -1),
            node(2, "x", "float", 4),
            node(2, "y", "float", 4),
            node(2, "z", "float", 4),
            node(1, "m_LocalPosition", "Vector3f", -1),
            node(2, "x", "float", 4),
            node(2, "y", "float", 4),
            node(2, "z", "float", 4),
            node(1, "m_LocalRotation", "Quaternionf", -1),
            node(2, "x", "float", 4),
            node(2, "y", "float", 4),
            node(2, "z", "float", 4),
            node(2, "w", "float", 4),
            node(1, "m_LocalScale", "Vector3f", -1),
            node(2, "x", "float", 4),
            node(2, "y", "float", 4),
            node(2, "z", "float", 4),
        ];
        SerializedFileMetadata {
            prefix: SerializedFileMetadataPrefix {
                metadata_offset: 0,
                metadata_size: 0,
                body_endian: crate::io::binary::Endian::Little,
                unity_version_text: Some("2019.4.31f1".to_owned()),
                target_platform: Some(0),
                has_type_tree: true,
                consumed_bytes: 0,
            },
            types: vec![SerializedType {
                raw_type_id: 4,
                type_id_for_hash: 4,
                is_stripped_type: false,
                script_type_index: -1,
                script_id: None,
                old_type_hash: None,
                type_tree: Some(TypeTree {
                    nodes,
                    string_buffer: Vec::new(),
                }),
                type_dependencies: Vec::new(),
            }],
            long_file_id: None,
            objects: Vec::new(),
            script_types: Vec::new(),
            externals: Vec::new(),
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        }
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
        .unwrap()
        .expect("streamed image texture should be exported");
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
    fn workspace_yaml_collection_skips_image_texture_with_unset_or_zero_length_stream() {
        for (stream_path, stream_size) in [("", 3), ("texture.resS", 0)] {
            let source_bytes =
                make_streamed_asset_file_with_stream_info(189, false, stream_path, stream_size);
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
            assert!(document.is_none());

            let assignment = SceneExportIdAssignment {
                asset: identity,
                file_id: 77,
            };
            let yaml = workspace_assignments_to_unity_yaml_with_streamed_resources(
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
            assert!(!yaml.contains("--- !u!189"));
            assert!(!yaml.contains("ImageTexture:"));
        }
    }

    #[test]
    fn workspace_yaml_collection_preserves_multi_asset_order_and_streamed_bytes() {
        let first_bytes =
            make_streamed_asset_file_with_inline_data(189, false, "first.resS", 3, b"ORIG");
        let second_bytes = make_streamed_asset_file_with_stream_info(189, false, "second.resS", 3);
        let source_files = [
            ("first.assets", first_bytes.as_slice()),
            ("second.assets", second_bytes.as_slice()),
        ];
        let (mut filesystem, workspace) = load_workspace_files(&source_files);
        filesystem
            .write_all_bytes("/game/second.resS", b"yyXYZ!")
            .unwrap();
        let catalog =
            explicit_root_resource_catalog(&filesystem, "second.resS", "/game/second.resS");
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

        let first = asset(0, 77, 189);
        let second = ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 1 },
            object_index: 0,
            path_id: 77,
            class_id: 189,
        };
        let pointer_map = pointer_map_for_assets(&[first, second]);
        let provider = PlatformResourceProvider::new(None, None);
        let context = WorkspaceStreamedYamlResourceContext {
            filesystem: &filesystem,
            root_catalog: &catalog,
            provider: &provider,
            content_limits: StreamedResourceContentLimits::default(),
            staging_limits: StreamedYamlAssetLimits::default(),
        };
        let assignments = [
            SceneExportIdAssignment {
                asset: second,
                file_id: 202,
            },
            SceneExportIdAssignment {
                asset: first,
                file_id: 101,
            },
        ];

        let collection_yaml = workspace_assignments_to_unity_yaml_with_streamed_resources(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments,
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
            &context,
        )
        .unwrap();
        assert_yaml_asset_order_and_streamed_bytes(&collection_yaml);

        let scene_yaml = workspace_scene_assignments_to_unity_yaml_with_streamed_resources(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments,
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
            &context,
        )
        .unwrap();
        assert_yaml_asset_order_and_streamed_bytes(&scene_yaml);

        let first_payload = objects.objects()[0].payload.as_ref().unwrap();
        assert_eq!(
            crate::resource::extract_streamed_resource_info(
                first_payload
                    .root
                    .unique_field_in_base_chain("m_StreamData_C189")
                    .unwrap()
                    .unwrap(),
            )
            .unwrap()
            .path,
            "first.resS"
        );
        assert_eq!(
            first_payload
                .root
                .unique_field_in_base_chain("m_ImageData_C189")
                .unwrap()
                .unwrap(),
            &crate::serialized_file::object::SerializedValue::Bytes(b"ORIG".to_vec())
        );

        let second_payload = objects.objects()[1].payload.as_ref().unwrap();
        assert_eq!(
            crate::resource::extract_streamed_resource_info(
                second_payload
                    .root
                    .unique_field_in_base_chain("m_StreamData_C189")
                    .unwrap()
                    .unwrap(),
            )
            .unwrap()
            .path,
            "second.resS"
        );
        assert_eq!(
            second_payload
                .root
                .unique_field_in_base_chain("m_ImageData_C189")
                .unwrap()
                .unwrap(),
            &crate::serialized_file::object::SerializedValue::Bytes(Vec::new())
        );
    }

    fn assert_yaml_asset_order_and_streamed_bytes(yaml: &str) {
        let second_start = yaml.find("--- !u!189 &202").unwrap();
        let first_start = yaml.find("--- !u!189 &101").unwrap();
        assert!(second_start < first_start);
        assert_eq!(yaml.matches("--- !u!189").count(), 2);
        assert!(yaml[second_start..first_start].contains("m_ImageData_C189: 58595a"));
        assert!(yaml[first_start..].contains("m_ImageData_C189: 4f524947"));
        assert!(!yaml.contains("first.resS"));
        assert!(!yaml.contains("second.resS"));
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
        .unwrap()
        .expect("streamed Mesh should be exported");
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
    fn scene_executor_writes_a_planned_generated_transform_document() {
        let bytes = make_game_object_without_transform_file();
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

        let hierarchies =
            scene_hierarchies_for_workspace(&[("main.assets", &bytes)], &reference_graph);
        let processing = crate::processing::prefab_processing::plan_workspace_prefab_processing(
            &hierarchies,
            &[0],
            Default::default(),
        )
        .unwrap();
        assert!(processing.is_complete());
        let processed_assets =
            crate::processing::prefab_processing::plan_workspace_processed_prefab_assets(
                &hierarchies,
                &processing,
                &[0],
                &Default::default(),
                crate::processing::prefab_processing::WorkspaceProcessedPrefabAssetPlanLimits::default(),
            )
            .unwrap();
        assert!(
            processed_assets.is_complete(),
            "{:?}",
            processed_assets.incomplete_reason()
        );

        let assignments = crate::export::scene_collection::
            plan_workspace_scene_export_ids_with_processed_assets(
                &hierarchies.scenes()[0],
                0,
                &processed_assets,
                SceneUnityVersion {
                    major: 2019,
                    minor: 4,
                    patch: 0,
                },
                crate::processing::scene_hierarchy::WorkspaceSceneHierarchyMembershipLimits::default(),
                crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
                crate::export::scene_collection::SceneExportIdPlanLimits::default(),
            )
            .unwrap();
        let generated_transform = assignments
            .iter()
            .find(|assignment| {
                assignment.asset.class_id == 4
                    && matches!(
                        assignment.asset.collection,
                        ExportAssetCollectionIdentity::ProcessedCollection { .. }
                    )
            })
            .copied()
            .expect("scene export planner should include the synthesized Transform");

        let mut pointer_map_builder =
            ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        pointer_map_builder
            .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments)
            .unwrap();
        let pointer_map = pointer_map_builder.build().unwrap();
        let transform_metadata = generated_transform_metadata();
        let generated_context = WorkspaceGeneratedTransformYamlContext::new(
            &processed_assets,
            0,
            &transform_metadata,
            0,
        );
        let output = workspace_scene_assignments_to_unity_yaml_with_generated_transforms(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &assignments,
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
            &generated_context,
        )
        .unwrap();

        assert!(output.contains(&format!("--- !u!4 &{}", generated_transform.file_id)));
        assert!(output.contains("m_GameObject: {fileID: 42}"), "{output}");
        assert!(
            output.contains(&format!(
                "component: {{fileID: {}}}",
                generated_transform.file_id
            )),
            "{output}"
        );
        assert!(
            !output.contains("m_GameObject: {fileID: 42, guid:"),
            "{output}"
        );
        assert!(
            output.contains("m_LocalScale:\n    x: 1\n    y: 1\n    z: 1"),
            "{output}"
        );
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
