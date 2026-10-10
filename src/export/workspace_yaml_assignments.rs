use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str;

use crate::export::scene_wiring::MaterializedGeneratedTransform;
use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::PlatformResourceProvider;
use crate::processing::asset_factory::{
    reference_class_layouts, ClassLayoutSource, FactoryAssetKind, FactoryUnityVersion,
};
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
use crate::serialized_file::metadata::{SerializedFileMetadata, SerializedType};
use crate::serialized_file::object::{
    default_value_for_type_tree_node, ObjectDefaultValueLimits, ObjectEncodeLimits,
    ParsedObjectPayload, SerializedFieldValue, SerializedValue,
};
use crate::serialized_file::object_workspace::WorkspaceObject;
use crate::serialized_file::reference_graph::{MultiFilePPtrResolution, MultiFileReferenceGraph};
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::pointer_map::{
    ExportPointerMapError, ProjectExportPointerMap, ProjectPointerRequest,
    ProjectPointerResolution, UnityProjectAssetType, UnresolvedProjectPointer,
};
use super::prefab_collection::{
    PrefabAssetRepresentation, PrefabExportIdAssignment, PrefabExportIdPlan,
};
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

/// Processed collection scope used by generated Transform YAML execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GeneratedTransformCollectionScope {
    Scene { scene_index: usize },
    Prefab,
}

impl GeneratedTransformCollectionScope {
    fn matches(self, kind: &WorkspaceProcessedCollectionKind) -> bool {
        match self {
            Self::Scene { scene_index } => {
                *kind == WorkspaceProcessedCollectionKind::SceneGeneratedAssets { scene_index }
            }
            Self::Prefab => *kind == WorkspaceProcessedCollectionKind::MissingPrefabTransforms,
        }
    }

    fn description(self) -> String {
        match self {
            Self::Scene { scene_index } => {
                format!("the generated collection for Scene {scene_index}")
            }
            Self::Prefab => "the generated Transform collection for Prefabs".to_owned(),
        }
    }
}

/// Type metadata and processed-asset plan needed to execute generated Transform assignments.
#[derive(Clone, Copy)]
pub struct WorkspaceGeneratedTransformYamlContext<'a> {
    pub processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
    collection_scope: GeneratedTransformCollectionScope,
    materialized_transforms: Option<&'a [MaterializedGeneratedTransform]>,
    metadata: Option<&'a SerializedFileMetadata>,
    transform_type_index: Option<usize>,
    legacy_prefab_export: Option<WorkspaceGeneratedLegacyPrefabYamlContext<'a>>,
    pub default_value_limits: ObjectDefaultValueLimits,
    pub encode_limits: ObjectEncodeLimits,
}

#[derive(Clone, Copy)]
struct WorkspaceGeneratedLegacyPrefabYamlContext<'a> {
    export_ids: &'a PrefabExportIdPlan,
    unity_version_text: &'a str,
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
            collection_scope: GeneratedTransformCollectionScope::Scene { scene_index },
            materialized_transforms: None,
            metadata: Some(metadata),
            transform_type_index: Some(transform_type_index),
            legacy_prefab_export: None,
            default_value_limits: ObjectDefaultValueLimits::default(),
            encode_limits: ObjectEncodeLimits::default(),
        }
    }

    pub fn for_prefabs(
        processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
        metadata: &'a SerializedFileMetadata,
        transform_type_index: usize,
    ) -> Self {
        Self {
            processed_assets,
            collection_scope: GeneratedTransformCollectionScope::Prefab,
            materialized_transforms: None,
            metadata: Some(metadata),
            transform_type_index: Some(transform_type_index),
            legacy_prefab_export: None,
            default_value_limits: ObjectDefaultValueLimits::default(),
            encode_limits: ObjectEncodeLimits::default(),
        }
    }

    /// Use the payloads already materialized by the workspace processing pass. Each generated
    /// Transform retains the source file and TypeTree that supplied its serialized layout, so a
    /// collection may safely contain generated objects from different input files or versions.
    pub fn with_materialized_scene_transforms(
        processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
        scene_index: usize,
        generated_transforms: &'a [MaterializedGeneratedTransform],
    ) -> Self {
        Self {
            processed_assets,
            collection_scope: GeneratedTransformCollectionScope::Scene { scene_index },
            materialized_transforms: Some(generated_transforms),
            metadata: None,
            transform_type_index: None,
            legacy_prefab_export: None,
            default_value_limits: ObjectDefaultValueLimits::default(),
            encode_limits: ObjectEncodeLimits::default(),
        }
    }

    /// Prefab counterpart to [`Self::with_materialized_scene_transforms`].
    pub fn with_materialized_prefab_transforms(
        processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
        generated_transforms: &'a [MaterializedGeneratedTransform],
    ) -> Self {
        Self {
            processed_assets,
            collection_scope: GeneratedTransformCollectionScope::Prefab,
            materialized_transforms: Some(generated_transforms),
            metadata: None,
            transform_type_index: None,
            legacy_prefab_export: None,
            default_value_limits: ObjectDefaultValueLimits::default(),
            encode_limits: ObjectEncodeLimits::default(),
        }
    }

    /// Attach the generated legacy Prefab identity and source version for one Prefab collection.
    pub fn with_legacy_prefab_export(
        mut self,
        export_ids: &'a PrefabExportIdPlan,
        unity_version_text: &'a str,
    ) -> Self {
        self.legacy_prefab_export = Some(WorkspaceGeneratedLegacyPrefabYamlContext {
            export_ids,
            unity_version_text,
        });
        self
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

/// Scene-manager field overlay for a generated LightingDataAsset collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceGeneratedLightingDataYamlPointer {
    pub source: ExportAssetIdentity,
    pub target: ExportAssetIdentity,
}

#[derive(Clone, Copy)]
struct WorkspaceGeneratedPrefabInternalPointer {
    source: ExportAssetIdentity,
    target: ExportAssetIdentity,
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
        None,
    )
}

/// Execute the YAML members of a planned Prefab collection in baseline hierarchy order.
///
/// `PrefabExportCollection.ExportableAssets` excludes hidden modern PrefabInstance markers but
/// includes the legacy Prefab object. This context-free helper fails explicitly for a generated
/// legacy main object; the generated-aware project route supplies its TPK payload schema and
/// `m_PrefabInternal` overlays. This function writes the `.prefab` YAML body only; importer
/// `.meta` output is a separate export path.
#[allow(clippy::too_many_arguments)]
pub fn workspace_prefab_hierarchy_to_unity_yaml(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    plan: &PrefabExportIdPlan,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    workspace_prefab_hierarchy_to_unity_yaml_with_context(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        plan,
        current_collection_index,
        missing_reference_guid,
        limits,
        None,
    )
}

/// Execute a Prefab hierarchy that includes PrefabProcessor-generated Transform objects.
#[allow(clippy::too_many_arguments)]
pub fn workspace_prefab_hierarchy_to_unity_yaml_with_generated_transforms(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    plan: &PrefabExportIdPlan,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    generated_transforms: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    workspace_prefab_hierarchy_to_unity_yaml_with_context(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        plan,
        current_collection_index,
        missing_reference_guid,
        limits,
        Some(generated_transforms),
    )
}

#[allow(clippy::too_many_arguments)]
fn workspace_prefab_hierarchy_to_unity_yaml_with_context(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    plan: &PrefabExportIdPlan,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    generated_transforms: Option<&WorkspaceGeneratedTransformYamlContext<'_>>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let expected_emit_prefab_asset =
        plan.representation == PrefabAssetRepresentation::LegacyPrefabMarker;
    if plan.emit_prefab_asset != expected_emit_prefab_asset {
        return Err(
            UnityYamlWorkspaceCollectionError::PrefabPlanEmissionMismatch {
                emit_prefab_asset: plan.emit_prefab_asset,
                representation: plan.representation,
            },
        );
    }
    if plan.exportable_assets.len() > limits.max_documents {
        return Err(UnityYamlWorkspaceCollectionError::DocumentLimitExceeded {
            count: plan.exportable_assets.len(),
            limit: limits.max_documents,
        });
    }

    let mut assignments = Vec::new();
    assignments
        .try_reserve_exact(plan.exportable_assets.len())
        .map_err(|_| UnityYamlWorkspaceCollectionError::AllocationFailed {
            resource: "Prefab YAML assignments",
            requested: plan.exportable_assets.len(),
        })?;
    for (assignment_index, asset) in plan.exportable_assets.iter().copied().enumerate() {
        let mut matching = plan
            .registered_assets
            .iter()
            .filter(|registered| registered.asset == asset);
        let Some(assignment) = matching.next().copied() else {
            return Err(
                UnityYamlWorkspaceCollectionError::PrefabAssignmentNotRegistered {
                    assignment_index,
                    asset,
                },
            );
        };
        if matching.next().is_some() {
            return Err(
                UnityYamlWorkspaceCollectionError::PrefabAssignmentRegisteredMoreThanOnce {
                    assignment_index,
                    asset,
                },
            );
        }
        assignments.push(assignment);
    }

    workspace_assignments_to_unity_yaml_with_context(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        &assignments,
        current_collection_index,
        missing_reference_guid,
        limits,
        None,
        generated_transforms,
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
    generated_lighting_data: Option<&WorkspaceGeneratedLightingDataYamlPointer>,
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
                let generated_prefab_internal = if let Some(context) = generated_transforms {
                    workspace_generated_prefab_internal_pointer(asset, context).map_err(
                        |source| UnityYamlWorkspaceCollectionError::ObjectExport {
                            assignment_index,
                            source: Box::new(source),
                        },
                    )?
                } else {
                    None
                };
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
                    generated_prefab_internal.as_ref(),
                    generated_lighting_data,
                )
                .map_err(|source| {
                    UnityYamlWorkspaceCollectionError::ObjectExport {
                        assignment_index,
                        source: Box::new(source),
                    }
                })?
            }
            ExportAssetCollectionIdentity::ProcessedCollection { .. } => {
                let Some(generated_context) = generated_transforms else {
                    return Err(
                        UnityYamlWorkspaceCollectionError::ProcessedAssetRequiresGeneratedWalker {
                            assignment_index,
                            asset,
                        },
                    );
                };
                let document = match asset.class_id {
                    4 => workspace_generated_transform_assignment_to_unity_yaml_document(
                        workspace,
                        reference_graph,
                        pointer_map,
                        asset,
                        file_id,
                        current_collection_index,
                        missing_reference_guid,
                        limits.object_conversion,
                        generated_context,
                    ),
                    1001 => workspace_generated_prefab_assignment_to_unity_yaml_document(
                        workspace,
                        pointer_map,
                        asset,
                        file_id,
                        current_collection_index,
                        missing_reference_guid,
                        limits.object_conversion,
                        generated_context,
                    ),
                    _ => {
                        return Err(
                            UnityYamlWorkspaceCollectionError::ProcessedAssetRequiresGeneratedWalker {
                                assignment_index,
                                asset,
                            },
                        );
                    }
                }
                .map_err(|source| UnityYamlWorkspaceCollectionError::ObjectExport {
                    assignment_index,
                    source: Box::new(source),
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
        || !context.collection_scope.matches(&collection.kind)
    {
        return Err(invariant_error(format!(
            "processed collection {collection_index} is not {}",
            context.collection_scope.description()
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

    if let Some(materialized_transforms) = context.materialized_transforms {
        let mut matches = materialized_transforms
            .iter()
            .filter(|transform| transform.asset == asset.identity);
        let transform = matches.next().ok_or_else(|| {
            invariant_error(format!(
                "generated Transform {:?} was not materialized by workspace processing",
                asset.identity
            ))
        })?;
        if matches.next().is_some() {
            return Err(invariant_error(format!(
                "generated Transform {:?} has multiple materialized payloads",
                asset.identity
            )));
        }
        if transform.game_object != *game_object || transform.payload.asset != asset.identity {
            return Err(invariant_error(
                "materialized Transform identity disagrees with its processed asset plan"
                    .to_owned(),
            ));
        }
        let metadata = &workspace
            .files()
            .get(transform.type_file_index)
            .ok_or_else(|| {
                invariant_error(format!(
                    "generated Transform type file {} is absent from the workspace",
                    transform.type_file_index
                ))
            })?
            .metadata;
        generated_transform_payload_to_unity_yaml_document(
            &transform.payload,
            metadata,
            source,
            file_id,
            constructor_pointer,
            pointer_map,
            current_collection_index,
            missing_reference_guid,
            limits,
        )
    } else {
        let metadata = context.metadata.ok_or_else(|| {
            invariant_error("generated Transform YAML has no TypeTree metadata context".to_owned())
        })?;
        let type_index = context.transform_type_index.ok_or_else(|| {
            invariant_error("generated Transform YAML has no TypeTree index".to_owned())
        })?;
        let payload = materialize_planned_generated_transform_payload(
            context.processed_assets,
            asset.identity,
            metadata,
            type_index,
            context.default_value_limits,
            context.encode_limits,
        )
        .map_err(|error| invariant_error(error.to_string()))?;
        generated_transform_payload_to_unity_yaml_document(
            &payload,
            metadata,
            source,
            file_id,
            constructor_pointer,
            pointer_map,
            current_collection_index,
            missing_reference_guid,
            limits,
        )
    }
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
        None,
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
    workspace_scene_assignments_to_unity_yaml_with_project_generated_assets(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        assignments_in_hierarchy_order,
        current_collection_index,
        missing_reference_guid,
        limits,
        streamed_resources,
        Some(generated_transforms),
        None,
    )
}

/// Scene YAML export with streamed assets and the generated collections owned by that Scene.
#[allow(clippy::too_many_arguments)]
pub fn workspace_scene_assignments_to_unity_yaml_with_project_generated_assets(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    streamed_resources: &WorkspaceStreamedYamlResourceContext<'_, '_>,
    generated_transforms: Option<&WorkspaceGeneratedTransformYamlContext<'_>>,
    generated_lighting_data: Option<&WorkspaceGeneratedLightingDataYamlPointer>,
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
        generated_transforms,
        generated_lighting_data,
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
    PrefabAssignmentNotRegistered {
        assignment_index: usize,
        asset: ExportAssetIdentity,
    },
    PrefabAssignmentRegisteredMoreThanOnce {
        assignment_index: usize,
        asset: ExportAssetIdentity,
    },
    PrefabPlanEmissionMismatch {
        emit_prefab_asset: bool,
        representation: PrefabAssetRepresentation,
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
            Self::PrefabAssignmentNotRegistered {
                assignment_index,
                asset,
            } => write!(
                f,
                "Prefab YAML assignment {assignment_index} has no registered export ID: {asset:?}"
            ),
            Self::PrefabAssignmentRegisteredMoreThanOnce {
                assignment_index,
                asset,
            } => write!(
                f,
                "Prefab YAML assignment {assignment_index} has duplicate registered export IDs: {asset:?}"
            ),
            Self::PrefabPlanEmissionMismatch {
                emit_prefab_asset,
                representation,
            } => write!(
                f,
                "Prefab export plan says emit_prefab_asset={emit_prefab_asset} for representation {representation:?}"
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

fn workspace_generated_prefab_internal_pointer(
    source: ExportAssetIdentity,
    context: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<Option<WorkspaceGeneratedPrefabInternalPointer>, UnityYamlWorkspaceExportError> {
    let Some(legacy) = context.legacy_prefab_export else {
        return Ok(None);
    };
    let plan = legacy.export_ids;
    if plan.representation != PrefabAssetRepresentation::LegacyPrefabMarker
        || !plan.emit_prefab_asset
        || !plan.exportable_assets.contains(&source)
    {
        return Ok(None);
    }
    let mut main_assets = plan
        .registered_assets
        .iter()
        .filter(|asset| asset.is_main_asset);
    let Some(main) = main_assets.next() else {
        return Err(UnityYamlWorkspaceExportError::GeneratedPrefabInvariant {
            source,
            message: "legacy Prefab plan has no main asset identity".to_owned(),
        });
    };
    if main_assets.next().is_some()
        || main.asset.class_id != 1001
        || !matches!(
            main.asset.collection,
            ExportAssetCollectionIdentity::ProcessedCollection { .. }
        )
    {
        return Err(UnityYamlWorkspaceExportError::GeneratedPrefabInvariant {
            source,
            message: "legacy Prefab main asset is not one generated class-1001 asset".to_owned(),
        });
    }
    Ok(Some(WorkspaceGeneratedPrefabInternalPointer {
        source,
        target: main.asset,
    }))
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

    let mut generated_collections = context
        .processed_assets
        .collections()
        .iter()
        .filter(|collection| context.collection_scope.matches(&collection.kind));
    let scene_generated_collection = generated_collections.next().ok_or_else(|| {
        invariant_error(format!(
            "processed plan has no {}",
            context.collection_scope.description()
        ))
    })?;
    if generated_collections.next().is_some() {
        return Err(invariant_error(format!(
            "processed plan has multiple collections for {}",
            context.collection_scope.description()
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
            "source GameObject has duplicate generated-component overlays for the selected collection"
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
    let type_tree = object
        .recovered_type_tree
        .as_deref()
        .or(serialized_type.type_tree.as_ref())
        .ok_or(UnityYamlWorkspaceExportError::MissingTypeTree {
            file_index: object.file_index,
            object_index: object.object_index,
            type_index,
        })?;
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

/// `StrippedAssetExtensions.IsStripped` over caller facts: the asset's main asset is a
/// hierarchy object tracking a stripped set, and the asset is in that set
/// (`AssetRipper/Source/AssetRipper.Export.UnityProjects/StrippedAssetExtensions.cs`
/// lines 10-18). `YamlWalker.ExportYamlDocument` strips the walked fields to the
/// per-class allowlist and marks the document `stripped` exactly on this decision
/// (`AssetRipper/Source/AssetRipper.Export.UnityProjects/YamlWalker.cs` lines 80-85).
///
/// The workspace executor passes `false` today because neither fact is wired through the
/// workspace flow; this kernel records the decision shape so a caller supplying
/// hierarchy facts can replace the constant without re-deriving it.
pub const fn is_stripped_document(
    main_asset_is_hierarchy_object: bool,
    asset_in_stripped_set: bool,
) -> bool {
    main_asset_is_hierarchy_object && asset_in_stripped_set
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
        None,
        None,
    )
}
