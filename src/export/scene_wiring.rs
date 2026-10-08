//! Production wiring for the Scene and Prefab processing workstream.
//!
//! This module turns the ported Scene/prefab planners into a real processing pipeline over a
//! loaded workspace. It mirrors the baseline processor order from
//! `AssetRipper.Export.UnityProjects/ExportHandler.cs` lines 82 and 90:
//!
//! 1. `SceneDefinitionProcessor.Process` (`Processing/Scenes/SceneDefinitionProcessor.cs`):
//!    one pass over the source collections/objects collects scene collections
//!    (`ILevelGameManager`), the selected `BuildSettings`, and streamed-scene `AssetBundle`
//!    objects; `SceneHelpers.TryGetScenePath` recovers BuildSettings paths and
//!    `SceneDefinition.FromPath`/`FromName` make the scene definitions; the generated settings
//!    collection supplies `EditorBuildSettings`.
//! 2. `PrefabProcessor.Process` (`Processing/Prefabs/PrefabProcessor.cs`):
//!    `AddMissingTransforms` first, then one `SceneHierarchyObject` per scene definition, then
//!    PrefabInstance hierarchies, then a generated prefab for every remaining root GameObject.
//!
//! Unlike the planner-only stages, the outcome here owns a materialized typed object graph
//! ([`MaterializedSceneGraph`]): every GameObject carries its decoded name, active state, and
//! ordered component slots; every Transform carries its local values, attached GameObject,
//! resolved father, and ordered children. Generated objects are materialized too: the
//! PrefabProcessor's synthesized Transforms are built from the selected class-ID-4 TypeTree
//! (`TransformExtensions.InitializeDefault` values plus the `m_GameObject` constructor pointer)
//! and appended to the owning GameObject's component list, and every shortfall is reported as an
//! explicit diagnostic rather than silently skipped.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use crate::io::filesystem::VirtualFileSystem;
use crate::serialized_file::collection_workspace::{
    WorkspaceAssetCollections, WorkspaceCollectionLimits,
};
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::object::{ObjectDefaultValueLimits, ObjectEncodeLimits};
use crate::serialized_file::object_workspace::ObjectWorkspace;
use crate::serialized_file::reference_graph::MultiFileReferenceGraph;
use crate::serialized_file::workspace::SerializedFileWorkspace;

use crate::processing::asset_bundle::get_asset_bundle_name;
use crate::processing::prefab_processing::{
    animator_controller_source_dependency_inventory,
    materialize_planned_generated_transform_payload, plan_workspace_prefab_processing,
    plan_workspace_processed_prefab_assets, WorkspaceGeneratedComponentAddition,
    WorkspaceGeneratedTransformPayload, WorkspacePrefabProcessingLimits,
    WorkspacePrefabProcessingPlan, WorkspaceProcessedAssetIdentity,
    WorkspaceProcessedPrefabAssetKind, WorkspaceProcessedPrefabAssetPlan,
    WorkspaceProcessedPrefabAssetPlanLimits,
};
use crate::processing::project_settings::{EditorBuildSettingsLimits, EditorBuildSettingsModel};
use crate::processing::scene::{
    SceneGraph, SceneGraphIssue, SceneGraphLimits, SceneHierarchyAuditLimits, SceneObjectRecord,
    SceneRectTransformValues, SceneRelationship, SceneRelationshipKind, SceneRelationshipTarget,
    SceneRootDiscoveryLimits, SceneTransformValues,
};
use crate::processing::scene_collections::{
    extract_occlusion_scene_guid, extract_streamed_scene_bundle_metadata,
    map_streamed_scene_bundle_paths, plan_scene_definition, resolve_scene_collection_path,
    scene_manager_kind, BuildSettingsScenePathSet, SceneBundleMappingLimits,
    SceneBundlePathMappingSource, SceneCollectionPathStatus, SceneDefinitionPlanLimits,
    SceneGuidWords, WorkspaceSceneDefinitionError, WorkspaceSceneDefinitionLimits,
    WorkspaceSceneDefinitions,
};
use crate::processing::scene_hierarchy::{
    WorkspaceSceneHierarchies, WorkspaceSceneHierarchyBuilder, WorkspaceSceneHierarchyError,
    WorkspaceSceneHierarchyLimits, WorkspaceSceneHierarchyMembershipError,
    WorkspaceSceneHierarchyMembershipLimits, WorkspaceSceneObjectIdentity,
};
use crate::processing::scene_metadata::{
    extract_build_settings_scene_paths, BuildSettingsSceneLimits,
};

const GAME_OBJECT_CLASS_ID: i32 = 1;
const TRANSFORM_CLASS_ID: i32 = 4;
const RECT_TRANSFORM_CLASS_ID: i32 = 224;
const BUILD_SETTINGS_CLASS_ID: i32 = 141;
const ASSET_BUNDLE_CLASS_ID: i32 = 142;
const OCCLUSION_CULLING_SETTINGS_CLASS_ID: i32 = 29;

/// Bounds for one workspace scene/prefab processing pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkspaceSceneProcessingLimits {
    pub scene_graph: SceneGraphLimits,
    pub root_discovery: SceneRootDiscoveryLimits,
    pub hierarchy_audit: SceneHierarchyAuditLimits,
    pub hierarchy: WorkspaceSceneHierarchyLimits,
    pub membership: WorkspaceSceneHierarchyMembershipLimits,
    pub definitions: WorkspaceSceneDefinitionLimits,
    pub definition_plan: SceneDefinitionPlanLimits,
    pub build_settings: BuildSettingsSceneLimits,
    pub bundle_mapping: SceneBundleMappingLimits,
    pub collections: WorkspaceCollectionLimits,
    pub prefab_processing: WorkspacePrefabProcessingLimits,
    pub processed_assets: WorkspaceProcessedPrefabAssetPlanLimits,
    pub editor_build_settings: EditorBuildSettingsLimits,
    pub max_generated_transforms: usize,
    pub max_diagnostics: usize,
}

impl Default for WorkspaceSceneProcessingLimits {
    fn default() -> Self {
        Self {
            scene_graph: SceneGraphLimits::default(),
            root_discovery: SceneRootDiscoveryLimits::default(),
            hierarchy_audit: SceneHierarchyAuditLimits::default(),
            hierarchy: WorkspaceSceneHierarchyLimits::default(),
            membership: WorkspaceSceneHierarchyMembershipLimits::default(),
            definitions: WorkspaceSceneDefinitionLimits::default(),
            definition_plan: SceneDefinitionPlanLimits::default(),
            build_settings: BuildSettingsSceneLimits::default(),
            bundle_mapping: SceneBundleMappingLimits::default(),
            collections: WorkspaceCollectionLimits::default(),
            prefab_processing: WorkspacePrefabProcessingLimits::default(),
            processed_assets: WorkspaceProcessedPrefabAssetPlanLimits::default(),
            editor_build_settings: EditorBuildSettingsLimits::default(),
            max_generated_transforms: 4_000_000,
            max_diagnostics: 100_000,
        }
    }
}

/// A processing stage that failed. Every variant keeps the source file/object context available.
#[derive(Debug)]
pub enum WorkspaceSceneProcessingError {
    FileSystem {
        file_index: usize,
        virtual_path: String,
        source: io::Error,
    },
    SerializedFile {
        file_index: usize,
        virtual_path: String,
        message: String,
    },
    SceneGraph {
        file_index: usize,
        message: String,
    },
    SceneHierarchies(String),
    SceneDefinitions(String),
    BuildSettings(String),
    CollectionModel(String),
    PrefabProcessing(String),
    ProcessedAssets(String),
    ResourceLimitExceeded {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for WorkspaceSceneProcessingError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileSystem {
                file_index,
                virtual_path,
                source,
            } => write!(
                f,
                "scene processing could not read workspace file {file_index} {virtual_path:?}: {source}"
            ),
            Self::SerializedFile {
                file_index,
                virtual_path,
                message,
            } => write!(
                f,
                "scene processing could not parse workspace file {file_index} {virtual_path:?}: {message}"
            ),
            Self::SceneGraph {
                file_index,
                message,
            } => write!(f, "scene graph for workspace file {file_index} failed: {message}"),
            Self::SceneHierarchies(message) => write!(f, "scene hierarchy build failed: {message}"),
            Self::SceneDefinitions(message) => write!(f, "scene definition build failed: {message}"),
            Self::BuildSettings(message) => write!(f, "BuildSettings selection failed: {message}"),
            Self::CollectionModel(message) => {
                write!(f, "workspace collection model build failed: {message}")
            }
            Self::PrefabProcessing(message) => write!(f, "PrefabProcessor replay failed: {message}"),
            Self::ProcessedAssets(message) => {
                write!(f, "processed prefab asset planning failed: {message}")
            }
            Self::ResourceLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(
                f,
                "{resource} count {requested} exceeds the scene processing limit {limit}"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(f, "could not allocate {requested} entries for {resource}"),
        }
    }
}

impl Error for WorkspaceSceneProcessingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FileSystem { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// One GameObject component slot in serialized `m_Component` order.
#[derive(Clone, Debug, PartialEq)]
pub enum MaterializedComponentTarget {
    /// A slot decoded from the source component array.
    Source {
        relationship_index: usize,
        file_id: i32,
        path_id: i64,
        target: SceneRelationshipTarget,
    },
    /// A slot appended by the PrefabProcessor's generated-transform pass.
    Generated {
        asset: WorkspaceProcessedAssetIdentity,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaterializedComponentSlot {
    pub source_array_index: usize,
    /// Resolved class ID of the referenced object when it is known.
    pub class_id: Option<i32>,
    pub target: MaterializedComponentTarget,
}

/// A typed GameObject with its decoded name, active state, and ordered components.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterializedGameObject {
    pub identity: WorkspaceSceneObjectIdentity,
    /// Decoded name when the serialized bytes are valid UTF-8.
    pub name: Option<String>,
    /// Original serialized name bytes; malformed input is never silently rewritten.
    pub name_bytes: Option<Vec<u8>>,
    pub is_active: Option<bool>,
    pub components: Vec<MaterializedComponentSlot>,
    /// Identity of the Transform synthesized by `PrefabProcessor.AddMissingTransforms`.
    pub generated_transform: Option<WorkspaceProcessedAssetIdentity>,
}

/// A typed Transform or RectTransform with its resolved hierarchy edges.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterializedTransform {
    pub identity: WorkspaceSceneObjectIdentity,
    pub game_object: Option<SceneRelationship>,
    pub father: Option<SceneRelationship>,
    pub children: Vec<SceneRelationship>,
    pub values: Option<SceneTransformValues>,
    pub rect_values: Option<SceneRectTransformValues>,
    /// True when this Transform was synthesized by the missing-transform pass.
    pub generated: bool,
}

/// All typed objects materialized from one workspace SerializedFile.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterializedSceneCollection {
    pub file_index: usize,
    pub file_name: String,
    pub objects: Vec<SceneObjectRecord>,
    pub game_objects: Vec<MaterializedGameObject>,
    pub transforms: Vec<MaterializedTransform>,
    pub issues: Vec<SceneGraphIssue>,
}

impl MaterializedSceneCollection {
    pub fn game_object(
        &self,
        identity: WorkspaceSceneObjectIdentity,
    ) -> Option<&MaterializedGameObject> {
        self.game_objects
            .iter()
            .find(|object| object.identity == identity)
    }

    pub fn game_object_mut(
        &mut self,
        identity: WorkspaceSceneObjectIdentity,
    ) -> Option<&mut MaterializedGameObject> {
        self.game_objects
            .iter_mut()
            .find(|object| object.identity == identity)
    }

    /// One GameObject's Transform component, matching `GameObjectExtensions.TryGetComponent<ITransform>`.
    pub fn transform_of(
        &self,
        game_object: WorkspaceSceneObjectIdentity,
    ) -> Option<&MaterializedTransform> {
        let object = self.game_object(game_object)?;
        self.transforms.iter().find(|transform| {
            transform.game_object.as_ref().is_some_and(|relationship| {
                matches!(
                    relationship.target,
                    SceneRelationshipTarget::LocalObject { object_index }
                        if object.identity.object_index == object_index
                )
            }) && object.components.iter().any(|slot| {
                matches!(
                    &slot.target,
                    MaterializedComponentTarget::Source { path_id, .. }
                        if *path_id == transform.identity.path_id
                )
            })
        })
    }

    /// `GameObjectExtensions.IsRoot`: no Transform, or a Transform whose father pointer is null.
    pub fn is_root(&self, game_object: WorkspaceSceneObjectIdentity) -> bool {
        match self.transform_of(game_object) {
            None => true,
            Some(transform) => transform
                .father
                .as_ref()
                .is_none_or(|father| matches!(father.target, SceneRelationshipTarget::Null)),
        }
    }
}

/// The full materialized typed graph over the workspace.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterializedSceneGraph {
    collections: Vec<MaterializedSceneCollection>,
    collection_by_file: Vec<Option<usize>>,
}

impl MaterializedSceneGraph {
    pub fn collections(&self) -> &[MaterializedSceneCollection] {
        &self.collections
    }

    pub fn collection(&self, file_index: usize) -> Option<&MaterializedSceneCollection> {
        let index = (*self.collection_by_file.get(file_index)?)?;
        self.collections.get(index)
    }

    pub fn game_object(
        &self,
        identity: WorkspaceSceneObjectIdentity,
    ) -> Option<&MaterializedGameObject> {
        self.collection(identity.file_index)?.game_object(identity)
    }

    pub fn game_object_count(&self) -> usize {
        self.collections
            .iter()
            .map(|collection| collection.game_objects.len())
            .sum()
    }

    pub fn transform_count(&self) -> usize {
        self.collections
            .iter()
            .map(|collection| collection.transforms.len())
            .sum()
    }
}

/// One materialized `SceneHierarchyObject` (`Prefabs/SceneHierarchyObject.cs` lines 73-113).
#[derive(Clone, Debug, PartialEq)]
pub struct MaterializedSceneHierarchyAsset {
    pub scene_index: usize,
    /// `SceneHierarchyObject.Name`, which reads `SceneDefinition.Name`.
    pub name: String,
    /// `GetOrCreateSceneCollection`'s `scene.Name + " (Generated Assets)"` collection.
    pub generated_collection_name: String,
    /// `SceneDefinition.Collections` in order.
    pub source_collection_file_indices: Vec<usize>,
    pub game_objects: Vec<WorkspaceSceneObjectIdentity>,
    pub components: Vec<WorkspaceSceneObjectIdentity>,
    /// Generated Transform components from this scene's `(Generated Assets)` collection.
    ///
    /// `PrefabProcessor.AddMissingTransforms` creates the scene collection before
    /// `SceneHierarchyObject.Create`, so those generated assets are part of
    /// `SceneHierarchyObject.Assets` after the source objects. They have no workspace file or
    /// object-table index, so they are listed by processed-asset identity.
    pub generated_components: Vec<WorkspaceProcessedAssetIdentity>,
    pub managers: Vec<WorkspaceSceneObjectIdentity>,
    pub prefab_instances: Vec<WorkspaceSceneObjectIdentity>,
    pub scene_roots: Option<WorkspaceSceneObjectIdentity>,
    /// `SceneHierarchyObject.GetRoots()` in `GameObjects` order.
    pub roots: Vec<WorkspaceSceneObjectIdentity>,
    /// Read-back value of `SceneHierarchyObject.AssetBundleName` after processing.
    ///
    /// AssetRipper's `SceneHierarchyObject` inherits the no-op setter from
    /// `GeneratedObjectBase`, so this remains `None` even when the processor computes a name.
    pub asset_bundle_name: Option<String>,
    /// Name `PrefabProcessor` attempts to assign before the generated-object no-op setter drops it.
    pub requested_asset_bundle_name: Option<String>,
    /// True when the hierarchy's asset membership or root discovery reported issues.
    pub incomplete: bool,
}

/// A generated Transform materialized from a real class-ID-4 TypeTree.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterializedGeneratedTransform {
    pub asset: WorkspaceProcessedAssetIdentity,
    pub game_object: WorkspaceSceneObjectIdentity,
    /// `ProcessedAssetCollection.Name` that owns the generated asset.
    pub collection_name: String,
    /// Workspace file supplying the class-ID-4 TypeTree used to materialize this payload.
    pub type_file_index: usize,
    pub payload: WorkspaceGeneratedTransformPayload,
}

/// Explicit processing diagnostics. Every shortfall is recorded here instead of being skipped.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkspaceSceneProcessingDiagnostic {
    UndecodedObject {
        file_index: usize,
        object_index: usize,
        path_id: i64,
        class_id: Option<i32>,
        message: String,
    },
    SceneGraphIssue {
        file_index: usize,
        issue: SceneGraphIssue,
    },
    SceneCollectionWithoutBuildSettingsPath {
        file_index: usize,
        status: String,
    },
    BuildSettingsSelected {
        file_index: usize,
        object_index: usize,
        path_id: i64,
        scene_count: usize,
    },
    BuildSettingsUndecodable {
        file_index: usize,
        object_index: usize,
        message: String,
    },
    OcclusionSceneGuid {
        file_index: usize,
        object_index: usize,
        guid: Option<SceneGuidWords>,
    },
    OcclusionSceneGuidUndecodable {
        file_index: usize,
        object_index: usize,
        message: String,
    },
    StreamedSceneBundleMapped {
        file_index: usize,
        source: SceneBundlePathMappingSource,
        mappings: Vec<(usize, String)>,
    },
    StreamedSceneBundleUndecodable {
        file_index: usize,
        object_index: usize,
        message: String,
    },
    StreamedSceneBundleUnsupported {
        file_index: usize,
    },
    MissingTransformGenerated {
        game_object: WorkspaceSceneObjectIdentity,
        collection_name: String,
        asset: WorkspaceProcessedAssetIdentity,
    },
    GeneratedTransformMaterialized {
        game_object: WorkspaceSceneObjectIdentity,
        asset: WorkspaceProcessedAssetIdentity,
        byte_len: usize,
    },
    GeneratedTransformUnmaterialized {
        game_object: WorkspaceSceneObjectIdentity,
        asset: WorkspaceProcessedAssetIdentity,
        message: String,
    },
    GeneratedTransformTypeUnavailable {
        game_object: WorkspaceSceneObjectIdentity,
        message: String,
    },
    PrefabProcessingIncomplete {
        message: String,
    },
    ProcessedAssetPlanIncomplete {
        message: String,
    },
}

impl WorkspaceSceneProcessingDiagnostic {
    /// True when this diagnostic reports a real shortfall rather than an informational fact.
    pub fn is_problem(&self) -> bool {
        match self {
            Self::UndecodedObject { .. }
            | Self::SceneGraphIssue { .. }
            | Self::BuildSettingsUndecodable { .. }
            | Self::OcclusionSceneGuidUndecodable { .. }
            | Self::StreamedSceneBundleUndecodable { .. }
            | Self::StreamedSceneBundleUnsupported { .. }
            | Self::GeneratedTransformUnmaterialized { .. }
            | Self::GeneratedTransformTypeUnavailable { .. }
            | Self::PrefabProcessingIncomplete { .. }
            | Self::ProcessedAssetPlanIncomplete { .. } => true,
            Self::SceneCollectionWithoutBuildSettingsPath { .. }
            | Self::BuildSettingsSelected { .. }
            | Self::OcclusionSceneGuid { .. }
            | Self::StreamedSceneBundleMapped { .. }
            | Self::MissingTransformGenerated { .. }
            | Self::GeneratedTransformMaterialized { .. } => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceSceneProcessingReport {
    pub workspace_files: usize,
    pub source_collections: usize,
    pub scene_definitions: usize,
    pub scene_hierarchy_assets: usize,
    pub prefab_processing_events: usize,
    pub generated_collections: usize,
    pub generated_assets: usize,
    pub generated_transforms_materialized: usize,
    pub generated_transforms_requested: usize,
    pub materialized_game_objects: usize,
    pub materialized_transforms: usize,
    pub diagnostics: usize,
    pub diagnostics_truncated: bool,
    pub complete: bool,
}

impl WorkspaceSceneProcessingReport {
    pub fn is_complete(self) -> bool {
        self.complete
    }
}

/// The materialized outcome of the SceneDefinitionProcessor + PrefabProcessor sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSceneProcessing {
    /// Materialized scene definitions with their collection membership.
    pub definitions: WorkspaceSceneDefinitions,
    /// The generated `EditorBuildSettings` content when a BuildSettings asset was selected.
    pub editor_build_settings: Option<EditorBuildSettingsModel>,
    /// The typed GameObject/component/Transform graph, including generated components.
    pub graph: MaterializedSceneGraph,
    /// The hierarchy graphs the PrefabProcessor consumed.
    pub hierarchies: WorkspaceSceneHierarchies,
    /// One materialized hierarchy asset per scene definition.
    pub scene_hierarchies: Vec<MaterializedSceneHierarchyAsset>,
    /// The replayed PrefabProcessor identity passes.
    pub prefab_processing: WorkspacePrefabProcessingPlan,
    /// The generated collection/asset allocation plan.
    pub processed_assets: WorkspaceProcessedPrefabAssetPlan,
    /// Generated Transform objects with their materialized values and encoded bytes.
    pub generated_transforms: Vec<MaterializedGeneratedTransform>,
    pub diagnostics: Vec<WorkspaceSceneProcessingDiagnostic>,
    pub source_collection_file_indices: Vec<usize>,
    pub report: WorkspaceSceneProcessingReport,
}

/// Run the baseline SceneDefinitionProcessor and PrefabProcessor sequence over a loaded workspace.
///
/// The pass order, asset classification, path resolution, generated collection names, and
/// missing-transform behavior follow the frozen baseline sources named in the module docs. The
/// result owns real typed objects and generated objects; no step is a detached planner.
pub fn process_workspace_scenes_and_prefabs(
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    references: &MultiFileReferenceGraph<'_>,
    limits: WorkspaceSceneProcessingLimits,
) -> Result<WorkspaceSceneProcessing, WorkspaceSceneProcessingError> {
    let source_collection_file_indices = workspace
        .source_collection_file_indices()
        .map_err(|error| WorkspaceSceneProcessingError::SceneDefinitions(error.to_string()))?;
    let file_count = workspace.files().len();

    let definitions_pass =
        run_scene_definition_pass(workspace, objects, &source_collection_file_indices, limits)?;
    let definitions = definitions_pass.definitions;

    let mut diagnostics = definitions_pass.diagnostics;
    for object in objects.objects() {
        if let Err(error) = &object.payload {
            push_diagnostic(
                &mut diagnostics,
                limits,
                WorkspaceSceneProcessingDiagnostic::UndecodedObject {
                    file_index: object.file_index,
                    object_index: object.object_index,
                    path_id: object.path_id,
                    class_id: object.class_id.map(i32::from),
                    message: error.to_string(),
                },
            );
        }
    }
    let editor_build_settings = match definitions_pass.build_settings_paths {
        Some(paths) => Some(
            EditorBuildSettingsModel::from_build_settings_paths(
                &paths,
                limits.editor_build_settings,
            )
            .map_err(|error| WorkspaceSceneProcessingError::BuildSettings(error.to_string()))?,
        ),
        None => None,
    };

    let (graph, hierarchies) = build_typed_graph_and_hierarchies(
        filesystem,
        workspace,
        &definitions,
        references,
        limits,
        &mut diagnostics,
    )?;

    let collection_models =
        WorkspaceAssetCollections::build(workspace, objects, limits.collections)
            .map_err(|error| WorkspaceSceneProcessingError::CollectionModel(error.to_string()))?;
    let mut external_identifier_counts = vec![0usize; file_count];
    for collection in collection_models.collections() {
        if let Some(slot) = external_identifier_counts.get_mut(collection.source_file_index) {
            *slot = collection.external_identifier_count;
        }
    }
    let animator_controller_inventory =
        animator_controller_source_dependency_inventory(objects, limits.processed_assets)
            .map_err(|error| WorkspaceSceneProcessingError::ProcessedAssets(error.to_string()))?;

    let prefab_processing = plan_workspace_prefab_processing(
        &hierarchies,
        &source_collection_file_indices,
        limits.prefab_processing,
    )
    .map_err(|error| WorkspaceSceneProcessingError::PrefabProcessing(error.to_string()))?;
    if !prefab_processing.is_complete() {
        push_diagnostic(
            &mut diagnostics,
            limits,
            WorkspaceSceneProcessingDiagnostic::PrefabProcessingIncomplete {
                message: format!(
                    "{} processed GameObjects, {} hierarchy entries, stopped_on_incomplete_hierarchy={}",
                    prefab_processing.processed_game_objects().len(),
                    prefab_processing.hierarchy_entry_count(),
                    prefab_processing.stopped_on_incomplete_hierarchy()
                ),
            },
        );
    }
    let processed_assets = plan_workspace_processed_prefab_assets(
        &hierarchies,
        &prefab_processing,
        &external_identifier_counts,
        &animator_controller_inventory,
        limits.processed_assets,
    )
    .map_err(|error| WorkspaceSceneProcessingError::ProcessedAssets(error.to_string()))?;
    if !processed_assets.is_complete() {
        push_diagnostic(
            &mut diagnostics,
            limits,
            WorkspaceSceneProcessingDiagnostic::ProcessedAssetPlanIncomplete {
                message: processed_assets
                    .incomplete_reason()
                    .unwrap_or("unspecified")
                    .to_owned(),
            },
        );
    }

    let (graph, generated_transforms) = materialize_generated_transforms(
        workspace,
        objects,
        graph,
        &processed_assets,
        limits,
        &mut diagnostics,
    )?;
    let scene_hierarchies =
        materialize_scene_hierarchies(workspace, objects, &hierarchies, limits, &mut diagnostics)?;
    let mut scene_hierarchies = scene_hierarchies;
    for transform in &generated_transforms {
        let Some(collection) = processed_assets
            .collections()
            .get(transform.asset.collection_index)
        else {
            continue;
        };
        if let crate::processing::prefab_processing::WorkspaceProcessedCollectionKind::
            SceneGeneratedAssets { scene_index } = &collection.kind
        {
            if let Some(hierarchy) = scene_hierarchies.get_mut(*scene_index) {
                hierarchy.generated_components.push(transform.asset);
            }
        }
    }

    let generated_transforms_requested = processed_assets
        .component_additions()
        .iter()
        .filter(|addition| addition.component_class_id == TRANSFORM_CLASS_ID)
        .count();
    let complete = prefab_processing.is_complete()
        && processed_assets.is_complete()
        && scene_hierarchies.iter().all(|asset| !asset.incomplete)
        && generated_transforms.len() == generated_transforms_requested
        && graph
            .collections()
            .iter()
            .all(|collection| collection.issues.is_empty())
        && !diagnostics.iter().any(|diagnostic| diagnostic.is_problem());
    let report = WorkspaceSceneProcessingReport {
        workspace_files: file_count,
        source_collections: source_collection_file_indices.len(),
        scene_definitions: definitions.definitions().len(),
        scene_hierarchy_assets: scene_hierarchies.len(),
        prefab_processing_events: prefab_processing.events().len(),
        generated_collections: processed_assets.collections().len(),
        generated_assets: processed_assets.asset_count(),
        generated_transforms_materialized: generated_transforms.len(),
        generated_transforms_requested,
        materialized_game_objects: graph.game_object_count(),
        materialized_transforms: graph.transform_count(),
        diagnostics: diagnostics.len(),
        diagnostics_truncated: diagnostics.len() >= limits.max_diagnostics,
        complete,
    };

    Ok(WorkspaceSceneProcessing {
        definitions,
        editor_build_settings,
        graph,
        hierarchies,
        scene_hierarchies,
        prefab_processing,
        processed_assets,
        generated_transforms,
        diagnostics,
        source_collection_file_indices,
        report,
    })
}

struct SceneDefinitionPass {
    definitions: WorkspaceSceneDefinitions,
    build_settings_paths: Option<Vec<String>>,
    diagnostics: Vec<WorkspaceSceneProcessingDiagnostic>,
}

fn run_scene_definition_pass(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    source_collection_file_indices: &[usize],
    limits: WorkspaceSceneProcessingLimits,
) -> Result<SceneDefinitionPass, WorkspaceSceneProcessingError> {
    let mut diagnostics = Vec::new();
    // `SceneDefinitionProcessor` scans every collection and object once. A Rust HashSet keeps
    // insertion order explicit so the resulting definitions are deterministic.
    let mut scene_collections: Vec<usize> = Vec::new();
    let mut scene_collection_set = HashSet::new();
    let mut scene_guids: HashMap<usize, SceneGuidWords> = HashMap::new();
    let mut build_settings: Option<(usize, usize)> = None;
    let mut streamed_bundles: Vec<(usize, usize)> = Vec::new();

    for &file_index in source_collection_file_indices {
        let file = workspace.files().get(file_index).ok_or_else(|| {
            WorkspaceSceneProcessingError::SceneDefinitions(format!(
                "source collection traversal returned invalid file index {file_index}"
            ))
        })?;
        for (object_index, object) in file.metadata.objects.iter().enumerate() {
            let class_id = i32::from(object.class_id.unwrap_or(0));
            if scene_manager_kind(Some(class_id)).is_some() {
                if scene_collection_set.insert(file_index) {
                    scene_collections.push(file_index);
                }
                if class_id == OCCLUSION_CULLING_SETTINGS_CLASS_ID {
                    if let Some(workspace_object) = objects.get(file_index, object_index) {
                        match &workspace_object.payload {
                            Ok(payload) => match extract_occlusion_scene_guid(&payload.root) {
                                Ok(Some(guid)) => {
                                    scene_guids.insert(file_index, guid);
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        WorkspaceSceneProcessingDiagnostic::OcclusionSceneGuid {
                                            file_index,
                                            object_index,
                                            guid: Some(guid),
                                        },
                                    );
                                }
                                Ok(None) => push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    WorkspaceSceneProcessingDiagnostic::OcclusionSceneGuid {
                                        file_index,
                                        object_index,
                                        guid: None,
                                    },
                                ),
                                Err(error) => push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    WorkspaceSceneProcessingDiagnostic::OcclusionSceneGuidUndecodable {
                                        file_index,
                                        object_index,
                                        message: error.to_string(),
                                    },
                                ),
                            },
                            Err(error) => push_diagnostic(
                                &mut diagnostics,
                                limits,
                                WorkspaceSceneProcessingDiagnostic::OcclusionSceneGuidUndecodable {
                                    file_index,
                                    object_index,
                                    message: error.to_string(),
                                },
                            ),
                        }
                    }
                }
            } else if class_id == BUILD_SETTINGS_CLASS_ID {
                // The source overwrites `buildSettings` on every match, so the final
                // source-collection-order object wins.
                build_settings = Some((file_index, object_index));
            } else if class_id == ASSET_BUNDLE_CLASS_ID {
                streamed_bundles.push((file_index, object_index));
            }
        }
    }

    let mut build_settings_paths = None;
    if let Some((file_index, object_index)) = build_settings {
        if let Some(workspace_object) = objects.get(file_index, object_index) {
            match &workspace_object.payload {
                Ok(payload) => {
                    match extract_build_settings_scene_paths(&payload.root, limits.build_settings) {
                        Ok(paths) => {
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                WorkspaceSceneProcessingDiagnostic::BuildSettingsSelected {
                                    file_index,
                                    object_index,
                                    path_id: workspace_object.path_id,
                                    scene_count: paths.len(),
                                },
                            );
                            build_settings_paths = Some(paths);
                        }
                        Err(error) => push_diagnostic(
                            &mut diagnostics,
                            limits,
                            WorkspaceSceneProcessingDiagnostic::BuildSettingsUndecodable {
                                file_index,
                                object_index,
                                message: error.to_string(),
                            },
                        ),
                    }
                }
                Err(error) => push_diagnostic(
                    &mut diagnostics,
                    limits,
                    WorkspaceSceneProcessingDiagnostic::BuildSettingsUndecodable {
                        file_index,
                        object_index,
                        message: error.to_string(),
                    },
                ),
            }
        }
    }

    let mut streamed_paths: HashMap<usize, String> = HashMap::new();
    for (file_index, object_index) in streamed_bundles {
        let Some(workspace_object) = objects.get(file_index, object_index) else {
            continue;
        };
        let Ok(payload) = &workspace_object.payload else {
            continue;
        };
        let metadata =
            match extract_streamed_scene_bundle_metadata(&payload.root, limits.bundle_mapping) {
                Ok(metadata) => metadata,
                Err(error) => {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        WorkspaceSceneProcessingDiagnostic::StreamedSceneBundleUndecodable {
                            file_index,
                            object_index,
                            message: error.to_string(),
                        },
                    );
                    continue;
                }
            };
        let direct_collection_file_indices =
            match workspace.files()[file_index].container_bundle_index {
                Some(bundle_index) => {
                    let Some(bundle) = workspace.container_bundles().get(bundle_index) else {
                        continue;
                    };
                    bundle.direct_collection_file_indices.clone()
                }
                None => {
                    // A directly loaded standalone SerializedFile belongs to the root
                    // `GameBundle`, which is itself a `SerializedBundle`; its `Collections` are the
                    // directly loaded files in load order.
                    (0..workspace.files().len())
                        .filter(|index| workspace.files()[*index].container_bundle_index.is_none())
                        .collect()
                }
            };
        let mut names = Vec::new();
        let mut is_scene = Vec::new();
        for &collection_file_index in &direct_collection_file_indices {
            let Some(collection_file) = workspace.files().get(collection_file_index) else {
                continue;
            };
            names.push(crate::io::special_names::fix_file_identifier(
                &collection_file.file_name,
            ));
            is_scene.push(scene_collection_set.contains(&collection_file_index));
        }
        match map_streamed_scene_bundle_paths(&metadata, &names, &is_scene, limits.bundle_mapping) {
            Ok(Some(mapped)) => {
                let mut rows = Vec::new();
                for mapping in &mapped.mappings {
                    let Some(&collection_file_index) =
                        direct_collection_file_indices.get(mapping.collection_index)
                    else {
                        continue;
                    };
                    if scene_collection_set.insert(collection_file_index) {
                        scene_collections.push(collection_file_index);
                    }
                    streamed_paths.insert(collection_file_index, mapping.path.clone());
                    rows.push((collection_file_index, mapping.path.clone()));
                }
                push_diagnostic(
                    &mut diagnostics,
                    limits,
                    WorkspaceSceneProcessingDiagnostic::StreamedSceneBundleMapped {
                        file_index,
                        source: mapped.source,
                        mappings: rows,
                    },
                );
            }
            Ok(None) => {}
            Err(error) => push_diagnostic(
                &mut diagnostics,
                limits,
                WorkspaceSceneProcessingDiagnostic::StreamedSceneBundleUndecodable {
                    file_index,
                    object_index,
                    message: error.to_string(),
                },
            ),
        }
    }

    let build_settings_set = build_settings_paths
        .as_deref()
        .map(BuildSettingsScenePathSet::Unique);
    let mut plans = Vec::new();
    for &file_index in &scene_collections {
        let file = &workspace.files()[file_index];
        let path_set = build_settings_set.unwrap_or(BuildSettingsScenePathSet::Missing);
        let status = resolve_scene_collection_path(
            &file.file_name,
            file.metadata.prefix.unity_version_text.as_deref(),
            path_set,
        );
        if !matches!(status, SceneCollectionPathStatus::Resolved { .. }) {
            push_diagnostic(
                &mut diagnostics,
                limits,
                WorkspaceSceneProcessingDiagnostic::SceneCollectionWithoutBuildSettingsPath {
                    file_index,
                    status: status.to_string(),
                },
            );
        }
        let plan = plan_scene_definition(
            file_index,
            &file.file_name,
            &status,
            streamed_paths.get(&file_index).map(String::as_str),
            scene_guids.get(&file_index).copied(),
            limits.definition_plan,
        )
        .map_err(|error| WorkspaceSceneProcessingError::SceneDefinitions(error.to_string()))?;
        plans.push(plan);
    }

    let definitions =
        WorkspaceSceneDefinitions::from_plans(plans, workspace.files().len(), limits.definitions)
            .map_err(|error: WorkspaceSceneDefinitionError| {
            WorkspaceSceneProcessingError::SceneDefinitions(error.to_string())
        })?;
    Ok(SceneDefinitionPass {
        definitions,
        build_settings_paths,
        diagnostics,
    })
}

fn build_typed_graph_and_hierarchies(
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    definitions: &WorkspaceSceneDefinitions,
    references: &MultiFileReferenceGraph<'_>,
    limits: WorkspaceSceneProcessingLimits,
    diagnostics: &mut Vec<WorkspaceSceneProcessingDiagnostic>,
) -> Result<(MaterializedSceneGraph, WorkspaceSceneHierarchies), WorkspaceSceneProcessingError> {
    let file_count = workspace.files().len();
    let mut builder = WorkspaceSceneHierarchyBuilder::new(file_count, limits.hierarchy).map_err(
        |error: WorkspaceSceneHierarchyError| {
            WorkspaceSceneProcessingError::SceneHierarchies(error.to_string())
        },
    )?;
    let mut collections = Vec::new();
    let mut collection_by_file = vec![None; file_count];
    let maximum_bytes = usize::try_from(
        crate::serialized_file::workspace::SerializedFileWorkspaceLimits::default().max_file_bytes,
    )
    .unwrap_or(usize::MAX);
    for (file_index, file) in workspace.files().iter().enumerate() {
        let bytes = filesystem
            .read_all_bytes(&file.virtual_path, maximum_bytes)
            .map_err(|source| WorkspaceSceneProcessingError::FileSystem {
                file_index,
                virtual_path: file.virtual_path.clone(),
                source,
            })?;
        let parsed = SerializedFile::parse(&bytes).map_err(|error| {
            WorkspaceSceneProcessingError::SerializedFile {
                file_index,
                virtual_path: file.virtual_path.clone(),
                message: error.to_string(),
            }
        })?;
        let mut scene_graph = SceneGraph::build(&parsed, limits.scene_graph).map_err(|error| {
            WorkspaceSceneProcessingError::SceneGraph {
                file_index,
                message: error.to_string(),
            }
        })?;
        let roots = scene_graph
            .discover_workspace_roots(
                file_index,
                &file.file_name,
                references,
                limits.root_discovery,
            )
            .map_err(|error| WorkspaceSceneProcessingError::SceneGraph {
                file_index,
                message: error.to_string(),
            })?;
        let audit = scene_graph
            .audit_workspace_transform_links(
                file_index,
                &file.file_name,
                references,
                limits.hierarchy_audit,
            )
            .map_err(|error| WorkspaceSceneProcessingError::SceneGraph {
                file_index,
                message: error.to_string(),
            })?;
        let collection = materialize_collection(file_index, &file.file_name, &scene_graph);
        for issue in &collection.issues {
            push_diagnostic(
                diagnostics,
                limits,
                WorkspaceSceneProcessingDiagnostic::SceneGraphIssue {
                    file_index,
                    issue: issue.clone(),
                },
            );
        }
        builder
            .add_collection_graph(file_index, &scene_graph, roots, audit, references)
            .map_err(|error: WorkspaceSceneHierarchyError| {
                WorkspaceSceneProcessingError::SceneHierarchies(error.to_string())
            })?;
        collection_by_file[file_index] = Some(collections.len());
        collections.push(collection);
    }
    let hierarchies = builder
        .finish(definitions)
        .map_err(|error| WorkspaceSceneProcessingError::SceneHierarchies(error.to_string()))?;
    Ok((
        MaterializedSceneGraph {
            collections,
            collection_by_file,
        },
        hierarchies,
    ))
}

fn materialize_collection(
    file_index: usize,
    file_name: &str,
    graph: &SceneGraph,
) -> MaterializedSceneCollection {
    let objects = graph.objects().to_vec();
    let mut game_objects = Vec::new();
    let mut transforms = Vec::new();
    for record in &objects {
        match record.class_id {
            Some(GAME_OBJECT_CLASS_ID) => {
                let mut components = Vec::new();
                for (relationship_index, relationship) in graph.relationships().iter().enumerate() {
                    if relationship.source_object_index != record.object_index
                        || relationship.kind != SceneRelationshipKind::GameObjectComponent
                    {
                        continue;
                    }
                    let class_id = target_class_id(graph, &relationship.target);
                    components.push(MaterializedComponentSlot {
                        source_array_index: components.len(),
                        class_id,
                        target: MaterializedComponentTarget::Source {
                            relationship_index,
                            file_id: relationship.file_id,
                            path_id: relationship.path_id,
                            target: relationship.target.clone(),
                        },
                    });
                }
                game_objects.push(MaterializedGameObject {
                    identity: identity_of(file_index, record),
                    name: record
                        .name_bytes
                        .as_deref()
                        .and_then(|bytes| std::str::from_utf8(bytes).ok())
                        .map(str::to_owned),
                    name_bytes: record.name_bytes.clone(),
                    is_active: record.is_active,
                    components,
                    generated_transform: None,
                });
            }
            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {
                let mut game_object = None;
                let mut father = None;
                let mut children = Vec::new();
                for relationship in graph.relationships() {
                    if relationship.source_object_index != record.object_index {
                        continue;
                    }
                    match relationship.kind {
                        SceneRelationshipKind::ComponentGameObject => {
                            if game_object.is_none() {
                                game_object = Some(relationship.clone());
                            }
                        }
                        SceneRelationshipKind::TransformParent => {
                            if father.is_none() {
                                father = Some(relationship.clone());
                            }
                        }
                        SceneRelationshipKind::TransformChild => {
                            children.push(relationship.clone())
                        }
                        SceneRelationshipKind::GameObjectComponent
                        | SceneRelationshipKind::PrefabInstanceRootGameObject => {}
                    }
                }
                transforms.push(MaterializedTransform {
                    identity: identity_of(file_index, record),
                    game_object,
                    father,
                    children,
                    values: record.transform.clone(),
                    rect_values: record
                        .transform
                        .as_ref()
                        .and_then(|values| values.rect_transform.clone()),
                    generated: false,
                });
            }
            _ => {}
        }
    }
    MaterializedSceneCollection {
        file_index,
        file_name: file_name.to_owned(),
        objects,
        game_objects,
        transforms,
        issues: graph.issues().to_vec(),
    }
}

fn target_class_id(graph: &SceneGraph, target: &SceneRelationshipTarget) -> Option<i32> {
    match target {
        SceneRelationshipTarget::LocalObject { object_index }
        | SceneRelationshipTarget::WorkspaceObject { object_index, .. } => graph
            .objects()
            .get(*object_index)
            .and_then(|object| object.class_id),
        _ => None,
    }
}

fn identity_of(file_index: usize, record: &SceneObjectRecord) -> WorkspaceSceneObjectIdentity {
    WorkspaceSceneObjectIdentity {
        file_index,
        object_index: record.object_index,
        path_id: record.path_id,
        class_id: record.class_id,
    }
}

fn materialize_generated_transforms(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    mut graph: MaterializedSceneGraph,
    processed_assets: &WorkspaceProcessedPrefabAssetPlan,
    limits: WorkspaceSceneProcessingLimits,
    diagnostics: &mut Vec<WorkspaceSceneProcessingDiagnostic>,
) -> Result<
    (MaterializedSceneGraph, Vec<MaterializedGeneratedTransform>),
    WorkspaceSceneProcessingError,
> {
    let mut generated = Vec::new();
    if !processed_assets.is_complete() {
        return Ok((graph, generated));
    }
    let mut requested = 0usize;
    for addition in processed_assets.component_additions() {
        if addition.component_class_id != TRANSFORM_CLASS_ID {
            continue;
        }
        requested = requested.saturating_add(1);
        if generated.len() >= limits.max_generated_transforms {
            return Err(WorkspaceSceneProcessingError::ResourceLimitExceeded {
                resource: "generated Transform materializations",
                requested,
                limit: limits.max_generated_transforms,
            });
        }
        let Some(collection_name) = processed_assets
            .collections()
            .get(addition.component_asset.collection_index)
            .map(|collection| collection.name.clone())
        else {
            continue;
        };
        push_diagnostic(
            diagnostics,
            limits,
            WorkspaceSceneProcessingDiagnostic::MissingTransformGenerated {
                game_object: addition.game_object,
                collection_name: collection_name.clone(),
                asset: addition.component_asset,
            },
        );
        let kind = processed_assets
            .collections()
            .get(addition.component_asset.collection_index)
            .and_then(|collection| {
                collection
                    .assets
                    .iter()
                    .find(|asset| asset.identity == addition.component_asset)
            })
            .map(|asset| asset.kind.clone());
        let Some(WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
            game_object,
            default_values: _,
        }) = kind
        else {
            continue;
        };
        let Some((type_file_index, type_index)) =
            select_generated_transform_type(workspace, game_object.file_index)
        else {
            push_diagnostic(
                diagnostics,
                limits,
                WorkspaceSceneProcessingDiagnostic::GeneratedTransformTypeUnavailable {
                    game_object,
                    message: "no workspace SerializedFile exposes a class-ID-4 type entry"
                        .to_owned(),
                },
            );
            continue;
        };
        let metadata = &workspace.files()[type_file_index].metadata;
        match materialize_planned_generated_transform_payload(
            processed_assets,
            addition.component_asset,
            metadata,
            type_index,
            ObjectDefaultValueLimits::default(),
            ObjectEncodeLimits::default(),
        ) {
            Ok(payload) => {
                append_generated_component(&mut graph, addition, payload.asset);
                push_diagnostic(
                    diagnostics,
                    limits,
                    WorkspaceSceneProcessingDiagnostic::GeneratedTransformMaterialized {
                        game_object,
                        asset: payload.asset,
                        byte_len: payload.bytes.len(),
                    },
                );
                generated.push(MaterializedGeneratedTransform {
                    asset: payload.asset,
                    game_object,
                    collection_name,
                    type_file_index,
                    payload,
                });
            }
            Err(error) => push_diagnostic(
                diagnostics,
                limits,
                WorkspaceSceneProcessingDiagnostic::GeneratedTransformUnmaterialized {
                    game_object,
                    asset: addition.component_asset,
                    message: error.to_string(),
                },
            ),
        }
    }
    let _ = objects;
    Ok((graph, generated))
}

fn append_generated_component(
    graph: &mut MaterializedSceneGraph,
    addition: &WorkspaceGeneratedComponentAddition,
    asset: WorkspaceProcessedAssetIdentity,
) {
    let Some(collection_index) = graph
        .collection_by_file
        .get(addition.game_object.file_index)
        .copied()
        .flatten()
    else {
        return;
    };
    let Some(collection) = graph.collections.get_mut(collection_index) else {
        return;
    };
    let Some(game_object) = collection.game_object_mut(addition.game_object) else {
        return;
    };
    let source_array_index = addition.appended_index.max(game_object.components.len());
    game_object.components.push(MaterializedComponentSlot {
        source_array_index,
        class_id: Some(TRANSFORM_CLASS_ID),
        target: MaterializedComponentTarget::Generated { asset },
    });
    game_object.generated_transform = Some(asset);
}

fn select_generated_transform_type(
    workspace: &SerializedFileWorkspace,
    preferred_file_index: usize,
) -> Option<(usize, usize)> {
    let order = std::iter::once(preferred_file_index)
        .chain((0..workspace.files().len()).filter(|index| *index != preferred_file_index));
    let mut fallback = None;
    for file_index in order {
        let metadata = &workspace.files()[file_index].metadata;
        for (type_index, serialized_type) in metadata.types.iter().enumerate() {
            if serialized_type.raw_type_id != TRANSFORM_CLASS_ID {
                continue;
            }
            if serialized_type.type_tree.is_some() {
                return Some((file_index, type_index));
            }
            fallback.get_or_insert((file_index, type_index));
        }
    }
    fallback
}

fn materialize_scene_hierarchies(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    hierarchies: &WorkspaceSceneHierarchies,
    limits: WorkspaceSceneProcessingLimits,
    diagnostics: &mut Vec<WorkspaceSceneProcessingDiagnostic>,
) -> Result<Vec<MaterializedSceneHierarchyAsset>, WorkspaceSceneProcessingError> {
    let mut result = Vec::new();
    for (scene_index, scene) in hierarchies.scenes().iter().enumerate() {
        let membership = scene.asset_membership(limits.membership).map_err(
            |error: WorkspaceSceneHierarchyMembershipError| {
                WorkspaceSceneProcessingError::SceneHierarchies(error.to_string())
            },
        )?;
        let mut root_ids = HashSet::new();
        let mut collection_file_indices = Vec::new();
        for collection in scene.collections() {
            collection_file_indices.push(collection.collection_file_index());
            for candidate in collection.roots().roots() {
                if let Some(record) = collection.scene_objects().get(candidate.game_object_index) {
                    root_ids.insert(WorkspaceSceneObjectIdentity {
                        file_index: collection.collection_file_index(),
                        object_index: candidate.game_object_index,
                        path_id: record.path_id,
                        class_id: Some(GAME_OBJECT_CLASS_ID),
                    });
                }
            }
        }
        let roots = membership
            .game_objects()
            .iter()
            .copied()
            .filter(|game_object| root_ids.contains(game_object))
            .collect();
        let requested_asset_bundle_name =
            requested_scene_bundle_name(workspace, objects, &collection_file_indices);
        let incomplete = !membership.is_complete()
            || scene
                .collections()
                .iter()
                .any(|collection| !collection.roots().issues().is_empty());
        if incomplete {
            push_diagnostic(
                diagnostics,
                limits,
                WorkspaceSceneProcessingDiagnostic::PrefabProcessingIncomplete {
                    message: format!(
                        "scene {scene_index} hierarchy membership or root discovery is incomplete"
                    ),
                },
            );
        }
        result.push(MaterializedSceneHierarchyAsset {
            scene_index,
            name: scene.plan.name.clone(),
            generated_collection_name: format!("{} (Generated Assets)", scene.plan.name),
            source_collection_file_indices: collection_file_indices,
            game_objects: membership.game_objects().to_vec(),
            components: membership.components().to_vec(),
            generated_components: Vec::new(),
            managers: membership.managers().to_vec(),
            prefab_instances: membership.prefab_instances().to_vec(),
            scene_roots: membership.scene_roots(),
            roots,
            asset_bundle_name: None,
            requested_asset_bundle_name,
            incomplete,
        });
    }
    Ok(result)
}

/// `PrefabProcessor.Process` lines 34-47: computes the candidate name that it passes to
/// `SceneHierarchyObject.AssetBundleName`. The generated hierarchy inherits a no-op setter from
/// `GeneratedObjectBase`, so this candidate is not the effective property value.
fn requested_scene_bundle_name(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    collection_file_indices: &[usize],
) -> Option<String> {
    let bundle_index = collection_file_indices.iter().find_map(|file_index| {
        workspace
            .files()
            .get(*file_index)
            .and_then(|file| file.container_bundle_index)
    })?;
    let bundle = workspace.container_bundles().get(bundle_index)?;
    let bundle_name = bundle.name_fixed.clone().unwrap_or_default();
    for &collection_file_index in &bundle.direct_collection_file_indices {
        let Some(file) = workspace.files().get(collection_file_index) else {
            continue;
        };
        for (object_index, object) in file.metadata.objects.iter().enumerate() {
            if i32::from(object.class_id.unwrap_or(0)) != ASSET_BUNDLE_CLASS_ID {
                continue;
            }
            let Some(workspace_object) = objects.get(collection_file_index, object_index) else {
                continue;
            };
            let Ok(payload) = &workspace_object.payload else {
                continue;
            };
            if let Ok(name) = get_asset_bundle_name(&payload.root, &bundle_name) {
                return Some(name);
            }
        }
    }
    Some(bundle_name)
}

fn push_diagnostic(
    diagnostics: &mut Vec<WorkspaceSceneProcessingDiagnostic>,
    limits: WorkspaceSceneProcessingLimits,
    diagnostic: WorkspaceSceneProcessingDiagnostic,
) {
    if diagnostics.len() < limits.max_diagnostics {
        diagnostics.push(diagnostic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::dependency::DependencyFilePath;
    use crate::processing::asset_factory::FileTypeTreeTestLayouts;
    use crate::processing::scene_collections::{
        SceneDefinitionGuidPlan, SceneDefinitionPathSource, SceneGuidGenerationReason,
    };
    use crate::serialized_file::object_workspace::ObjectWorkspaceLimits;
    use crate::serialized_file::reference_graph::MultiFileReferenceGraphLimits;
    use crate::serialized_file::workspace::SerializedFileWorkspaceLimits;

    const GAME_OBJECT_PATH_ID: i64 = 100;
    const TRANSFORM_PATH_ID: i64 = 200;
    const ORPHAN_GAME_OBJECT_PATH_ID: i64 = 101;
    const OCCLUSION_PATH_ID: i64 = 300;
    const SECOND_GAME_OBJECT_PATH_ID: i64 = 110;
    const SECOND_TRANSFORM_PATH_ID: i64 = 210;
    const SECOND_OCCLUSION_PATH_ID: i64 = 301;
    const BUILD_SETTINGS_PATH_ID: i64 = 400;
    const ASSET_BUNDLE_PATH_ID: i64 = 500;

    /// The frozen 2019.4.40f1 class-4 TPK layout (release tree): no `m_RootOrder` and no
    /// `m_LocalEulerAnglesHint`.
    type Tree = &'static [(u8, i32, &'static str, &'static str, i32, u32)];

    const GAME_OBJECT_TREE: Tree = &[
        (0, 6, "GameObject", "Base", -1, 32768),
        (1, 1, "vector", "m_Component", -1, 268468289),
        (2, 1, "Array", "Array", -1, 268451905),
        (3, 1, "int", "size", 4, 268435521),
        (3, 1, "ComponentPair", "data", -1, 268435521),
        (4, 1, "PPtr<Component>", "component", 12, 268435521),
        (5, 1, "int", "m_FileID", 4, 268435521),
        (5, 1, "SInt64", "m_PathID", 8, 268435521),
        (1, 1, "unsigned int", "m_Layer", 4, 0),
        (1, 1, "string", "m_Name", -1, 32768),
        (2, 1, "Array", "Array", -1, 16385),
        (3, 1, "int", "size", 4, 1),
        (3, 1, "char", "data", 1, 1),
        (1, 1, "UInt16", "m_Tag", 2, 0),
        (1, 1, "bool", "m_IsActive", 1, 0),
    ];

    const TRANSFORM_TREE: Tree = &[
        (0, 1, "Transform", "Base", -1, 32768),
        (1, 1, "PPtr<GameObject>", "m_GameObject", 12, 65),
        (2, 1, "int", "m_FileID", 4, 65),
        (2, 1, "SInt64", "m_PathID", 8, 65),
        (1, 1, "Quaternionf", "m_LocalRotation", -1, 2097152),
        (2, 1, "float", "x", 4, 2097152),
        (2, 1, "float", "y", 4, 2097152),
        (2, 1, "float", "z", 4, 2097152),
        (2, 1, "float", "w", 4, 2097152),
        (1, 1, "Vector3f", "m_LocalPosition", -1, 2097152),
        (2, 1, "float", "x", 4, 2097152),
        (2, 1, "float", "y", 4, 2097152),
        (2, 1, "float", "z", 4, 2097152),
        (1, 1, "Vector3f", "m_LocalScale", -1, 2097152),
        (2, 1, "float", "x", 4, 2097152),
        (2, 1, "float", "y", 4, 2097152),
        (2, 1, "float", "z", 4, 2097152),
        (1, 1, "vector", "m_Children", -1, 32833),
        (2, 1, "Array", "Array", -1, 16449),
        (3, 1, "int", "size", 4, 65),
        (3, 1, "PPtr<Transform>", "data", 12, 65),
        (4, 1, "int", "m_FileID", 4, 65),
        (4, 1, "SInt64", "m_PathID", 8, 65),
        (1, 1, "PPtr<Transform>", "m_Father", 12, 1),
        (2, 1, "int", "m_FileID", 4, 1),
        (2, 1, "SInt64", "m_PathID", 8, 1),
    ];

    const OCCLUSION_TREE: Tree = &[
        (0, 2, "OcclusionCullingSettings", "Base", -1, 32768),
        (1, 1, "GUID", "m_SceneGUID", -1, 0),
        (2, 1, "unsigned int", "data[0]", 4, 0),
        (2, 1, "unsigned int", "data[1]", 4, 0),
        (2, 1, "unsigned int", "data[2]", 4, 0),
        (2, 1, "unsigned int", "data[3]", 4, 0),
        (
            1,
            1,
            "PPtr<OcclusionCullingData>",
            "m_OcclusionCullingData",
            12,
            0,
        ),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "vector", "m_StaticRenderers", -1, 32768),
        (2, 1, "Array", "Array", -1, 16384),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "PPtr<Renderer>", "data", 12, 0),
        (4, 1, "int", "m_FileID", 4, 8388609),
        (4, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "vector", "m_Portals", -1, 32768),
        (2, 1, "Array", "Array", -1, 16384),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "PPtr<OcclusionPortal>", "data", 12, 0),
        (4, 1, "int", "m_FileID", 4, 8388609),
        (4, 1, "SInt64", "m_PathID", 8, 8388609),
    ];

    const BUILD_SETTINGS_TREE: Tree = &[
        (0, 1, "BuildSettings", "", -1, 0),
        (1, 1, "vector", "m_Scenes", -1, 0),
        (2, 1, "Array", "Array", -1, 0),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "string", "data", -1, 0),
        (4, 1, "Array", "Array", -1, 0),
        (5, 1, "int", "size", 4, 0),
        (5, 1, "char", "data", 1, 0),
    ];

    const ASSET_BUNDLE_TREE: Tree = &[
        (0, 1, "AssetBundle", "", -1, 0),
        (1, 1, "map", "m_Container", -1, 0),
        (2, 1, "Array", "Array", -1, 0),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "pair", "data", -1, 0),
        (4, 1, "string", "first", -1, 0),
        (5, 1, "Array", "Array", -1, 0),
        (6, 1, "int", "size", 4, 0),
        (6, 1, "char", "data", 1, 0),
        (4, 1, "AssetInfo", "second", -1, 0),
        (5, 1, "int", "preloadIndex", 4, 0),
        (5, 1, "int", "preloadSize", 4, 0),
        (5, 1, "PPtr<Object>", "asset", 12, 0),
        (6, 1, "int", "m_FileID", 4, 0),
        (6, 1, "SInt64", "m_PathID", 8, 0),
        (1, 1, "bool", "m_IsStreamedSceneAssetBundle", 1, 0),
        (1, 1, "map", "m_SceneHashes", -1, 0),
        (2, 1, "Array", "Array", -1, 0),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "pair", "data", -1, 0),
        (4, 1, "string", "first", -1, 0),
        (5, 1, "Array", "Array", -1, 0),
        (6, 1, "int", "size", 4, 0),
        (6, 1, "char", "data", 1, 0),
        (4, 1, "string", "second", -1, 0),
        (5, 1, "Array", "Array", -1, 0),
        (6, 1, "int", "size", 4, 0),
        (6, 1, "char", "data", 1, 0),
    ];

    /// Release TypeTree nodes copied from AssetRipper's TPK for class 142 at Unity 2019.4.40f1.
    /// Unlike `ASSET_BUNDLE_TREE`, this includes the complete serialized object layout and its
    /// alignment flags so the frozen source factory can materialize the streamed-scene fixture.
    const ASSET_BUNDLE_FULL_TREE: Tree = &[
        (0, 1, "AssetBundle", "Base", -1, 32768),
        (1, 1, "string", "m_Name", -1, 557057),
        (2, 1, "Array", "Array", -1, 540673),
        (3, 1, "int", "size", 4, 524289),
        (3, 1, "char", "data", 1, 524289),
        (1, 1, "vector", "m_PreloadTable", -1, 32768),
        (2, 1, "Array", "Array", -1, 16384),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "PPtr<Object>", "data", -1, 0),
        (4, 1, "int", "m_FileID", 4, 8388609),
        (4, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "map", "m_Container", -1, 32768),
        (2, 1, "Array", "Array", -1, 32768),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "pair", "data", -1, 32768),
        (4, 1, "string", "first", -1, 32768),
        (5, 1, "Array", "Array", -1, 16385),
        (6, 1, "int", "size", 4, 1),
        (6, 1, "char", "data", 1, 1),
        (4, 1, "AssetInfo", "second", -1, 0),
        (5, 1, "int", "preloadIndex", 4, 0),
        (5, 1, "int", "preloadSize", 4, 0),
        (5, 1, "PPtr<Object>", "asset", -1, 0),
        (6, 1, "int", "m_FileID", 4, 8388609),
        (6, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "AssetInfo", "m_MainAsset", -1, 0),
        (2, 1, "int", "preloadIndex", 4, 0),
        (2, 1, "int", "preloadSize", 4, 0),
        (2, 1, "PPtr<Object>", "asset", -1, 0),
        (3, 1, "int", "m_FileID", 4, 8388609),
        (3, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "unsigned int", "m_RuntimeCompatibility", 4, 0),
        (1, 1, "string", "m_AssetBundleName", -1, 32768),
        (2, 1, "Array", "Array", -1, 16385),
        (3, 1, "int", "size", 4, 1),
        (3, 1, "char", "data", 1, 1),
        (1, 1, "vector", "m_Dependencies", -1, 32768),
        (2, 1, "Array", "Array", -1, 49152),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "string", "data", -1, 32768),
        (4, 1, "Array", "Array", -1, 16385),
        (5, 1, "int", "size", 4, 1),
        (5, 1, "char", "data", 1, 1),
        (1, 1, "bool", "m_IsStreamedSceneAssetBundle", 1, 16384),
        (1, 1, "int", "m_ExplicitDataLayout", 4, 0),
        (1, 1, "int", "m_PathFlags", 4, 0),
        (1, 1, "map", "m_SceneHashes", -1, 32768),
        (2, 1, "Array", "Array", -1, 32768),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "pair", "data", -1, 32768),
        (4, 1, "string", "first", -1, 32768),
        (5, 1, "Array", "Array", -1, 16385),
        (6, 1, "int", "size", 4, 1),
        (6, 1, "char", "data", 1, 1),
        (4, 1, "string", "second", -1, 32768),
        (5, 1, "Array", "Array", -1, 16385),
        (6, 1, "int", "size", 4, 1),
        (6, 1, "char", "data", 1, 1),
    ];

    struct FixtureType {
        class_id: i32,
        tree: Tree,
    }

    struct FixtureObject {
        path_id: i64,
        type_index: usize,
        payload: Vec<u8>,
    }

    fn push_string(output: &mut Vec<u8>, value: &str) {
        output.extend_from_slice(&i32::try_from(value.len()).unwrap().to_le_bytes());
        output.extend_from_slice(value.as_bytes());
        while !output.len().is_multiple_of(4) {
            output.push(0);
        }
    }

    fn push_pptr(output: &mut Vec<u8>, file_id: i32, path_id: i64) {
        output.extend_from_slice(&file_id.to_le_bytes());
        output.extend_from_slice(&path_id.to_le_bytes());
    }

    fn push_vector3(output: &mut Vec<u8>, value: [f32; 3]) {
        for component in value {
            output.extend_from_slice(&component.to_le_bytes());
        }
    }

    fn push_quaternion(output: &mut Vec<u8>, value: [f32; 4]) {
        for component in value {
            output.extend_from_slice(&component.to_le_bytes());
        }
    }

    fn game_object_payload(name: &str, is_active: bool, components: &[(i32, i64)]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&i32::try_from(components.len()).unwrap().to_le_bytes());
        for (file_id, path_id) in components {
            push_pptr(&mut payload, *file_id, *path_id);
        }
        payload.extend_from_slice(&0u32.to_le_bytes()); // m_Layer
        push_string(&mut payload, name);
        payload.extend_from_slice(&0u16.to_le_bytes()); // m_Tag
        payload.push(u8::from(is_active));
        payload
    }

    #[allow(clippy::too_many_arguments)]
    fn transform_payload(
        game_object_path_id: i64,
        position: [f32; 3],
        rotation: [f32; 4],
        scale: [f32; 3],
        children: &[i64],
        father: Option<i64>,
        father_file_id: i32,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        push_pptr(&mut payload, 0, game_object_path_id);
        push_quaternion(&mut payload, rotation);
        push_vector3(&mut payload, position);
        push_vector3(&mut payload, scale);
        payload.extend_from_slice(&i32::try_from(children.len()).unwrap().to_le_bytes());
        for child in children {
            push_pptr(&mut payload, 0, *child);
        }
        match father {
            Some(path_id) => push_pptr(&mut payload, father_file_id, path_id),
            None => push_pptr(&mut payload, 0, 0),
        }
        payload
    }

    fn occlusion_payload(guid: [u32; 4]) -> Vec<u8> {
        let mut payload = Vec::new();
        for word in guid {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        push_pptr(&mut payload, 0, 0);
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_StaticRenderers
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_Portals
        payload
    }

    fn build_settings_payload(scenes: &[&str]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&i32::try_from(scenes.len()).unwrap().to_le_bytes());
        for scene in scenes {
            push_string(&mut payload, scene);
        }
        payload
    }

    fn asset_bundle_payload(
        container: &str,
        streamed: bool,
        scene_hashes: &[(&str, &str)],
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1i32.to_le_bytes()); // m_Container size
        push_string(&mut payload, container);
        payload.extend_from_slice(&0i32.to_le_bytes()); // preloadIndex
        payload.extend_from_slice(&0i32.to_le_bytes()); // preloadSize
        push_pptr(&mut payload, 0, 0); // asset
        payload.push(u8::from(streamed));
        payload.extend_from_slice(&i32::try_from(scene_hashes.len()).unwrap().to_le_bytes());
        for (path, name) in scene_hashes {
            push_string(&mut payload, path);
            push_string(&mut payload, name);
        }
        payload
    }

    fn full_asset_bundle_payload(bundle_name: &str, scene_hashes: &[(&str, &str)]) -> Vec<u8> {
        let mut payload = Vec::new();
        push_string(&mut payload, "CAB-scene"); // m_Name
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_PreloadTable size
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_Container size
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_MainAsset.preloadIndex
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_MainAsset.preloadSize
        push_pptr(&mut payload, 0, 0); // m_MainAsset.asset
        payload.extend_from_slice(&0u32.to_le_bytes()); // m_RuntimeCompatibility
        push_string(&mut payload, bundle_name); // m_AssetBundleName
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_Dependencies size
        payload.push(1); // m_IsStreamedSceneAssetBundle
        while !payload.len().is_multiple_of(4) {
            payload.push(0); // TypeTree AlignBytes flag after streamed-scene bool
        }
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_ExplicitDataLayout
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_PathFlags
        payload.extend_from_slice(&i32::try_from(scene_hashes.len()).unwrap().to_le_bytes());
        for (scene_path, collection_name) in scene_hashes {
            push_string(&mut payload, scene_path);
            push_string(&mut payload, collection_name);
        }
        payload
    }

    fn full_asset_bundle_file(scene_hashes: &[(&str, &str)]) -> Vec<u8> {
        build_serialized_file(
            "2019.4.40f1",
            &[FixtureType {
                class_id: ASSET_BUNDLE_CLASS_ID,
                tree: ASSET_BUNDLE_FULL_TREE,
            }],
            &[FixtureObject {
                path_id: ASSET_BUNDLE_PATH_ID,
                type_index: 0,
                payload: full_asset_bundle_payload("streamed-scene-bundle", scene_hashes),
            }],
        )
    }

    fn write_type_tree(output: &mut Vec<u8>, class_id: i32, nodes: Tree) {
        let mut string_buffer = Vec::new();
        let mut offsets = Vec::new();
        for (_, _, type_name, name, _, _) in nodes {
            let type_offset = push_tree_string(&mut string_buffer, type_name);
            let name_offset = push_tree_string(&mut string_buffer, name);
            offsets.push((type_offset, name_offset));
        }
        output.extend_from_slice(&class_id.to_le_bytes());
        output.extend_from_slice(&i32::try_from(nodes.len()).unwrap().to_le_bytes());
        output.extend_from_slice(&i32::try_from(string_buffer.len()).unwrap().to_le_bytes());
        for (index, ((level, version, _, _, byte_size, meta), (type_offset, name_offset))) in
            nodes.iter().zip(offsets).enumerate()
        {
            output.extend_from_slice(&u16::try_from(*version).unwrap().to_le_bytes());
            output.push(*level);
            output.push(0);
            output.extend_from_slice(&type_offset.to_le_bytes());
            output.extend_from_slice(&name_offset.to_le_bytes());
            output.extend_from_slice(&byte_size.to_le_bytes());
            output.extend_from_slice(&i32::try_from(index).unwrap().to_le_bytes());
            output.extend_from_slice(&meta.to_le_bytes());
        }
        output.extend_from_slice(&string_buffer);
    }

    fn push_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn build_serialized_file(
        version: &str,
        types: &[FixtureType],
        objects: &[FixtureObject],
    ) -> Vec<u8> {
        let mut metadata = version.as_bytes().to_vec();
        metadata.push(0);
        metadata.extend_from_slice(&0u32.to_le_bytes());
        metadata.extend_from_slice(&i32::try_from(types.len()).unwrap().to_le_bytes());
        for fixture_type in types {
            write_type_tree(&mut metadata, fixture_type.class_id, fixture_type.tree);
        }
        metadata.extend_from_slice(&0u32.to_le_bytes()); // generation-10 long FileID flag
        metadata.extend_from_slice(&i32::try_from(objects.len()).unwrap().to_le_bytes());
        let mut data_start = 0usize;
        for object in objects {
            let class_id = types[object.type_index].class_id;
            metadata.extend_from_slice(&i32::try_from(object.path_id).unwrap().to_le_bytes());
            metadata.extend_from_slice(&u32::try_from(data_start).unwrap().to_le_bytes());
            metadata.extend_from_slice(&i32::try_from(object.payload.len()).unwrap().to_le_bytes());
            metadata.extend_from_slice(&class_id.to_le_bytes());
            metadata.extend_from_slice(&i16::try_from(class_id).unwrap().to_le_bytes());
            metadata.extend_from_slice(&0u16.to_le_bytes());
            data_start += object.payload.len();
        }
        metadata.extend_from_slice(&0i32.to_le_bytes()); // external files
        metadata.push(0); // user information
        let data_offset = 20 + metadata.len();
        let payload_bytes: usize = objects.iter().map(|object| object.payload.len()).sum();
        let file_size = data_offset + payload_bytes;
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&u32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        for object in objects {
            file.extend_from_slice(&object.payload);
        }
        file
    }

    fn scene_types() -> Vec<FixtureType> {
        vec![
            FixtureType {
                class_id: GAME_OBJECT_CLASS_ID,
                tree: GAME_OBJECT_TREE,
            },
            FixtureType {
                class_id: TRANSFORM_CLASS_ID,
                tree: TRANSFORM_TREE,
            },
            FixtureType {
                class_id: OCCLUSION_CULLING_SETTINGS_CLASS_ID,
                tree: OCCLUSION_TREE,
            },
        ]
    }

    fn level0_file() -> Vec<u8> {
        build_serialized_file(
            "2019.4.40f1",
            &scene_types(),
            &[
                FixtureObject {
                    path_id: GAME_OBJECT_PATH_ID,
                    type_index: 0,
                    payload: game_object_payload("Root", true, &[(0, TRANSFORM_PATH_ID)]),
                },
                FixtureObject {
                    path_id: TRANSFORM_PATH_ID,
                    type_index: 1,
                    payload: transform_payload(
                        GAME_OBJECT_PATH_ID,
                        [1.0, 2.0, 3.0],
                        [0.0, 0.0, 0.0, 1.0],
                        [2.0, 3.0, 4.0],
                        &[],
                        None,
                        0,
                    ),
                },
                FixtureObject {
                    path_id: ORPHAN_GAME_OBJECT_PATH_ID,
                    type_index: 0,
                    payload: game_object_payload("Orphan", false, &[]),
                },
                FixtureObject {
                    path_id: OCCLUSION_PATH_ID,
                    type_index: 2,
                    payload: occlusion_payload([
                        0x1122_3344,
                        0x5566_7788,
                        0x99aa_bbcc,
                        0xddee_ff00,
                    ]),
                },
            ],
        )
    }

    fn level1_file() -> Vec<u8> {
        build_serialized_file(
            "2019.4.40f1",
            &scene_types(),
            &[
                FixtureObject {
                    path_id: SECOND_GAME_OBJECT_PATH_ID,
                    type_index: 0,
                    payload: game_object_payload("Second", true, &[(0, SECOND_TRANSFORM_PATH_ID)]),
                },
                FixtureObject {
                    path_id: SECOND_TRANSFORM_PATH_ID,
                    type_index: 1,
                    payload: transform_payload(
                        SECOND_GAME_OBJECT_PATH_ID,
                        [0.0; 3],
                        [0.0, 0.0, 0.0, 1.0],
                        [1.0; 3],
                        &[],
                        None,
                        0,
                    ),
                },
                FixtureObject {
                    path_id: SECOND_OCCLUSION_PATH_ID,
                    type_index: 2,
                    payload: occlusion_payload([0; 4]),
                },
            ],
        )
    }

    fn build_settings_file(scenes: &[&str]) -> Vec<u8> {
        build_serialized_file(
            "2019.4.40f1",
            &[FixtureType {
                class_id: BUILD_SETTINGS_CLASS_ID,
                tree: BUILD_SETTINGS_TREE,
            }],
            &[FixtureObject {
                path_id: BUILD_SETTINGS_PATH_ID,
                type_index: 0,
                payload: build_settings_payload(scenes),
            }],
        )
    }

    fn asset_bundle_file(container: &str, streamed: bool, hashes: &[(&str, &str)]) -> Vec<u8> {
        build_serialized_file(
            "2019.4.40f1",
            &[FixtureType {
                class_id: ASSET_BUNDLE_CLASS_ID,
                tree: ASSET_BUNDLE_TREE,
            }],
            &[FixtureObject {
                path_id: ASSET_BUNDLE_PATH_ID,
                type_index: 0,
                payload: asset_bundle_payload(container, streamed, hashes),
            }],
        )
    }

    /// The fixture workspace used by the regression and probe-replay tests.
    ///
    /// Names carry no extension because the baseline `SceneHelpers` scene-name regex requires an
    /// exact `levelN`/`maindata` collection name. `globalgamemanagers` supplies the BuildSettings
    /// scene list and `CAB-scene` a streamed-scene AssetBundle whose SceneHashes map
    /// `Assets/Scenes/Streamed.unity` onto the `level0` collection.
    fn fixture_files() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("level0", level0_file()),
            ("level1", level1_file()),
            (
                "globalgamemanagers",
                build_settings_file(&[
                    "Assets/Scenes/FromBuild.unity",
                    "Assets/Scenes/Dupe.unity",
                    "Assets/Scenes/Dupe.unity",
                ]),
            ),
            (
                "CAB-scene",
                asset_bundle_file(
                    "Assets/Scenes/Streamed.unity",
                    true,
                    &[("Assets/Scenes/Streamed.unity", "level0")],
                ),
            ),
        ]
    }

    fn run_pipeline(
        files: &[(&str, Vec<u8>)],
    ) -> (WorkspaceSceneProcessing, SerializedFileWorkspace) {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        let mut input = Vec::new();
        for (file_name, bytes) in files {
            let virtual_path = format!("/game/{file_name}");
            filesystem.write_all_bytes(&virtual_path, bytes).unwrap();
            input.push(DependencyFilePath {
                file_name: (*file_name).to_owned(),
                virtual_path,
            });
        }
        let workspace = SerializedFileWorkspace::load_from_vfs_with_layout_source(
            &filesystem,
            &input,
            SerializedFileWorkspaceLimits::default(),
            &FileTypeTreeTestLayouts,
        )
        .unwrap();
        let objects = ObjectWorkspace::load_from_vfs_with_layout_source(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
            &FileTypeTreeTestLayouts,
        )
        .unwrap();
        let references = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let processing = process_workspace_scenes_and_prefabs(
            &filesystem,
            &workspace,
            &objects,
            &references,
            WorkspaceSceneProcessingLimits::default(),
        )
        .unwrap();
        (processing, workspace)
    }

    fn file_index(workspace: &SerializedFileWorkspace, name: &str) -> usize {
        workspace
            .files()
            .iter()
            .position(|file| file.file_name == name)
            .unwrap_or_else(|| panic!("workspace has no file {name}"))
    }

    #[test]
    fn materializes_scene_definitions_from_build_settings_guid_and_streamed_hashes() {
        let (processing, workspace) = run_pipeline(&fixture_files());
        let level0 = file_index(&workspace, "level0");
        let level1 = file_index(&workspace, "level1");

        let definition0 = processing
            .definitions
            .definitions()
            .iter()
            .find(|definition| definition.plan.collection_file_index == level0)
            .expect("level0 must produce a scene definition");
        assert_eq!(definition0.plan.collection_name, "level0");
        assert_eq!(definition0.plan.path, "Assets/Scenes/Streamed");
        assert_eq!(definition0.plan.name, "Streamed");
        assert_eq!(
            definition0.plan.path_source,
            SceneDefinitionPathSource::StreamedSceneBundle
        );
        assert_eq!(
            definition0.plan.guid,
            SceneDefinitionGuidPlan::Preserve(SceneGuidWords {
                data: [0x1122_3344, 0x5566_7788, 0x99aa_bbcc, 0xddee_ff00],
            })
        );
        assert_eq!(definition0.collection_file_indices().to_vec(), vec![level0]);

        let definition1 = processing
            .definitions
            .definitions()
            .iter()
            .find(|definition| definition.plan.collection_file_index == level1)
            .expect("level1 must produce a scene definition");
        assert_eq!(definition1.plan.path, "Assets/Scenes/Dupe");
        assert_eq!(definition1.plan.name, "Dupe");
        assert_eq!(
            definition1.plan.path_source,
            SceneDefinitionPathSource::BuildSettings { scene_index: 1 }
        );
        assert_eq!(
            definition1.plan.guid,
            SceneDefinitionGuidPlan::GenerateAtExport(SceneGuidGenerationReason::ZeroInInput)
        );

        let settings = processing
            .editor_build_settings
            .as_ref()
            .expect("BuildSettings must produce EditorBuildSettings");
        let paths: Vec<&str> = settings
            .scenes()
            .iter()
            .map(|scene| scene.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "Assets/Scenes/FromBuild.unity",
                "Assets/Scenes/Dupe.unity",
                "Assets/Scenes/Dupe.unity"
            ]
        );
        assert!(settings.scenes().iter().all(|scene| scene.enabled));
        assert_eq!(processing.report.scene_definitions, 2);
    }

    #[test]
    fn materializes_typed_game_object_transform_graph_with_ordered_components() {
        let (processing, workspace) = run_pipeline(&fixture_files());
        let level0 = file_index(&workspace, "level0");
        let collection = processing
            .graph
            .collection(level0)
            .expect("level0 has a materialized collection");

        let root = collection
            .game_object(WorkspaceSceneObjectIdentity {
                file_index: level0,
                object_index: 0,
                path_id: GAME_OBJECT_PATH_ID,
                class_id: Some(GAME_OBJECT_CLASS_ID),
            })
            .expect("Root must be materialized");
        assert_eq!(root.name.as_deref(), Some("Root"));
        assert_eq!(root.is_active, Some(true));
        assert_eq!(root.components.len(), 1);
        let MaterializedComponentTarget::Source { path_id, .. } = &root.components[0].target else {
            panic!("Root's first component must be the source Transform slot");
        };
        assert_eq!(*path_id, TRANSFORM_PATH_ID);
        assert_eq!(root.components[0].class_id, Some(TRANSFORM_CLASS_ID));

        let orphan = collection
            .game_object(WorkspaceSceneObjectIdentity {
                file_index: level0,
                object_index: 2,
                path_id: ORPHAN_GAME_OBJECT_PATH_ID,
                class_id: Some(GAME_OBJECT_CLASS_ID),
            })
            .expect("Orphan must be materialized");
        assert_eq!(orphan.name.as_deref(), Some("Orphan"));
        assert_eq!(orphan.is_active, Some(false));

        let transform = collection
            .transforms
            .iter()
            .find(|transform| transform.identity.path_id == TRANSFORM_PATH_ID)
            .expect("the Root Transform must be materialized");
        let values = transform.values.as_ref().expect("Transform values");
        assert_eq!(
            values
                .local_position
                .map(|value| (value.x, value.y, value.z)),
            Some((1.0, 2.0, 3.0))
        );
        assert_eq!(
            values.local_scale.map(|value| (value.x, value.y, value.z)),
            Some((2.0, 3.0, 4.0))
        );
        assert_eq!(
            values
                .local_rotation
                .map(|value| (value.x, value.y, value.z, value.w)),
            Some((0.0, 0.0, 0.0, 1.0))
        );
        assert!(matches!(
            transform.father.as_ref().map(|father| &father.target),
            Some(SceneRelationshipTarget::Null)
        ));
        assert_eq!(collection.game_objects.len(), 2);
        assert_eq!(collection.transforms.len(), 1);
    }

    #[test]
    fn prefab_processor_generates_and_appends_a_missing_transform() {
        let (processing, workspace) = run_pipeline(&fixture_files());
        let level0 = file_index(&workspace, "level0");
        assert_eq!(processing.report.generated_transforms_requested, 1);
        assert_eq!(processing.report.generated_transforms_materialized, 1);
        assert_eq!(processing.generated_transforms.len(), 1);

        let generated = &processing.generated_transforms[0];
        assert_eq!(generated.game_object.path_id, ORPHAN_GAME_OBJECT_PATH_ID);
        assert_eq!(generated.payload.asset.class_id, TRANSFORM_CLASS_ID);
        assert!(!generated.payload.bytes.is_empty());

        // `TransformExtensions.InitializeDefault` against a real 2019.4 class-4 layout, which has
        // no `m_RootOrder`/`m_LocalEulerAnglesHint`.
        let root = &generated.payload.root;
        let position = root
            .unique_field_in_base_chain("m_LocalPosition")
            .unwrap()
            .unwrap();
        assert_eq!(vector3_of(position), [0.0, 0.0, 0.0]);
        let scale = root
            .unique_field_in_base_chain("m_LocalScale")
            .unwrap()
            .unwrap();
        assert_eq!(vector3_of(scale), [1.0, 1.0, 1.0]);
        let rotation = root
            .unique_field_in_base_chain("m_LocalRotation")
            .unwrap()
            .unwrap();
        assert_eq!(quaternion_of(rotation), [0.0, 0.0, 0.0, 1.0]);
        assert!(root
            .unique_field_in_base_chain("m_RootOrder")
            .unwrap()
            .is_none());
        match root
            .unique_field_in_base_chain("m_GameObject")
            .unwrap()
            .unwrap()
        {
            crate::serialized_file::object::SerializedValue::PPtr { path_id, .. } => {
                assert_eq!(*path_id, ORPHAN_GAME_OBJECT_PATH_ID)
            }
            other => panic!("m_GameObject is {other:?}"),
        }

        // The processor mutation appended the generated Transform to Orphan's component list.
        let collection = processing.graph.collection(level0).unwrap();
        let orphan = collection
            .game_object(WorkspaceSceneObjectIdentity {
                file_index: level0,
                object_index: 2,
                path_id: ORPHAN_GAME_OBJECT_PATH_ID,
                class_id: Some(GAME_OBJECT_CLASS_ID),
            })
            .unwrap();
        assert_eq!(orphan.generated_transform, Some(generated.payload.asset));
        assert_eq!(orphan.components.len(), 1);
        match &orphan.components[0].target {
            MaterializedComponentTarget::Generated { asset } => {
                assert_eq!(*asset, generated.payload.asset)
            }
            other => panic!("expected a generated component slot, got {other:?}"),
        }
        assert!(processing.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            WorkspaceSceneProcessingDiagnostic::MissingTransformGenerated { .. }
        )));
        assert!(processing.report.complete, "{:?}", processing.diagnostics);
    }

    #[test]
    fn materializes_scene_hierarchy_assets_and_roots() {
        let (processing, _workspace) = run_pipeline(&fixture_files());
        let hierarchy = processing
            .scene_hierarchies
            .iter()
            .find(|hierarchy| hierarchy.name == "Streamed")
            .expect("the streamed scene must produce a hierarchy asset");
        assert_eq!(
            hierarchy.generated_collection_name,
            "Streamed (Generated Assets)"
        );
        assert_eq!(
            hierarchy
                .game_objects
                .iter()
                .map(|game_object| game_object.path_id)
                .collect::<Vec<_>>(),
            [GAME_OBJECT_PATH_ID, ORPHAN_GAME_OBJECT_PATH_ID]
        );
        assert_eq!(
            hierarchy
                .roots
                .iter()
                .map(|game_object| game_object.path_id)
                .collect::<Vec<_>>(),
            [GAME_OBJECT_PATH_ID, ORPHAN_GAME_OBJECT_PATH_ID]
        );
        assert_eq!(hierarchy.managers.len(), 1);
        assert_eq!(hierarchy.managers[0].path_id, OCCLUSION_PATH_ID);
        assert_eq!(hierarchy.generated_components.len(), 1);
        assert!(!hierarchy.incomplete);
        assert!(processing.prefab_processing.is_complete());
    }

    #[test]
    fn export_project_writes_processed_scene_collections_and_skips_duplicate_build_paths() {
        // Use complete 2019.4 TPK layouts so the frozen generated factory materializes both
        // BuildSettings and the streamed-scene AssetBundle instead of classifying the shortened
        // unit-test shapes as unreadable. `level1` and `level2` map to duplicate paths; the
        // first export wins. The streamed bundle maps `level3` through SceneHashes.
        let files = vec![
            ("level0", level0_file()),
            ("level1", level1_file()),
            ("level2", level1_file()),
            ("level3", level1_file()),
            (
                "globalgamemanagers",
                crate::processing::scene_metadata::tests::make_reference_build_settings_file(&[
                    "Assets/Scenes/FromBuild.unity",
                    "Assets/Scenes/Dupe.unity",
                    "Assets/Scenes/Dupe.unity",
                ]),
            ),
            (
                "CAB-scene",
                full_asset_bundle_file(&[("Assets/Scenes/Streamed.unity", "level3")]),
            ),
        ];

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        let mut inputs = Vec::new();
        for (file_name, bytes) in &files {
            let virtual_path = format!("/game/{file_name}");
            filesystem.write_all_bytes(&virtual_path, bytes).unwrap();
            inputs.push(DependencyFilePath {
                file_name: (*file_name).to_owned(),
                virtual_path,
            });
        }
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let references = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let scene_processing = process_workspace_scenes_and_prefabs(
            &filesystem,
            &workspace,
            &objects,
            &references,
            WorkspaceSceneProcessingLimits::default(),
        )
        .unwrap();
        assert!(
            scene_processing.report.is_complete(),
            "{:?}",
            scene_processing.diagnostics
        );
        assert!(scene_processing
            .scene_hierarchies
            .iter()
            .all(|hierarchy| hierarchy.asset_bundle_name.is_none()));
        let reference_rows: Vec<_> = include_str!("scene_processor_fixture.txt")
            .lines()
            .filter(|line| {
                matches!(
                    line.split_once('|').map(|(record, _)| record),
                    Some(
                        "HIER"
                            | "HIER-GUID"
                            | "HIER-GO"
                            | "HIER-COMPONENTS"
                            | "HIER-MANAGERS"
                            | "HIER-PREFABS"
                            | "HIER-SCENEROOTS"
                            | "HIER-ROOTS"
                    )
                )
            })
            .collect();
        assert_eq!(
            reference_rows.len(),
            scene_processing.scene_hierarchies.len() * 8
        );

        fn reference_field<'a>(row: &'a str, name: &str) -> &'a str {
            row.strip_prefix(name)
                .or_else(|| row.split('|').find_map(|part| part.strip_prefix(name)))
                .unwrap_or_else(|| panic!("reference row {row:?} has no {name:?} field"))
        }

        let mut reference_scenes: Vec<_> = reference_rows.chunks_exact(8).collect();
        reference_scenes.sort_by_key(|rows| reference_field(rows[0], "path="));
        let mut rust_scenes: Vec<_> = scene_processing
            .hierarchies
            .scenes()
            .iter()
            .zip(&scene_processing.scene_hierarchies)
            .collect();
        rust_scenes.sort_by_key(|(scene, _)| scene.plan.path.as_str());
        for (rows, (scene, hierarchy)) in reference_scenes.into_iter().zip(rust_scenes) {
            assert_eq!(reference_field(rows[0], "scene="), scene.plan.name);
            assert_eq!(reference_field(rows[0], "path="), scene.plan.path);
            assert_eq!(reference_field(rows[0], "name="), scene.plan.name);
            assert_eq!(
                reference_field(rows[0], "bundle="),
                hierarchy.asset_bundle_name.as_deref().unwrap_or("null")
            );
            match scene.plan.guid {
                SceneDefinitionGuidPlan::Preserve(words) => {
                    let mut guid_bytes = [0; 16];
                    for (index, word) in words.data.iter().enumerate() {
                        guid_bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
                    }
                    assert_eq!(
                        reference_field(rows[1], "HIER-GUID|"),
                        crate::export::unity_yaml::format_unity_guid_words(guid_bytes)
                    );
                }
                SceneDefinitionGuidPlan::GenerateAtExport(_) => {
                    // AssetRipper generates these GUIDs randomly in its in-memory scene
                    // definitions, so compare the rest of the scene semantics and validate the
                    // Rust-emitted GUID in the project export below instead of freezing a random
                    // source-runtime value in this fixture.
                }
            }

            let source_ids = |row: &str| {
                row.split_once('|')
                    .unwrap()
                    .1
                    .split(',')
                    .filter_map(|asset| {
                        asset
                            .split_once('#')
                            .and_then(|(_, tail)| tail.split_once(':'))
                            .map(|(path_id, _)| path_id.parse::<i64>().unwrap())
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                hierarchy
                    .game_objects
                    .iter()
                    .map(|identity| identity.path_id)
                    .collect::<Vec<_>>(),
                source_ids(rows[2])
            );
            assert_eq!(
                hierarchy.components.len() + hierarchy.generated_components.len(),
                reference_field(rows[3], "HIER-COMPONENTS|")
                    .parse::<usize>()
                    .unwrap()
            );
            assert_eq!(
                hierarchy
                    .managers
                    .iter()
                    .map(|identity| identity.path_id)
                    .collect::<Vec<_>>(),
                source_ids(rows[4])
            );
            assert_eq!(
                hierarchy.prefab_instances.len(),
                reference_field(rows[5], "HIER-PREFABS|")
                    .parse::<usize>()
                    .unwrap()
            );
            assert_eq!(
                hierarchy.scene_roots.is_some(),
                reference_field(rows[6], "HIER-SCENEROOTS|") != "null"
            );
            assert_eq!(
                hierarchy
                    .roots
                    .iter()
                    .map(|identity| identity.path_id)
                    .collect::<Vec<_>>(),
                source_ids(rows[7])
            );
        }
        let output_root = crate::test_support::unique_test_directory("scene-project-export");
        let root_catalog = crate::resource::RootResourceFileCatalog::default();
        let provider = crate::io::platform_structure::PlatformResourceProvider::new(None, None);

        let report = crate::export::project_workflow::export_workspace_yaml_project(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &references,
            crate::export::project_workflow::WorkspaceProjectExportLimits::default(),
        )
        .unwrap();

        let first_scene =
            std::fs::read_to_string(output_root.join("Assets/Scenes/FromBuild.unity")).unwrap();
        assert!(first_scene.contains("--- !u!1 &100"));
        assert!(first_scene.contains("--- !u!4 &"));
        assert!(first_scene.contains("m_Components:"));
        assert!(first_scene.contains("m_GameObject:"));

        let first_meta =
            std::fs::read_to_string(output_root.join("Assets/Scenes/FromBuild.unity.meta"))
                .unwrap();
        let importer_reference = include_str!("default_importer_probe_fixture.txt");
        assert!(importer_reference.contains("VALUE|ExternalObjects|Count = 0"));
        for property in ["AssetBundleName_R", "AssetBundleVariant", "UserData"] {
            assert!(importer_reference
                .lines()
                .any(|line| line == format!("VALUE|{property}|")));
        }
        assert!(first_meta.contains("DefaultImporter:"));
        assert!(first_meta.contains("externalObjects: {}"));
        assert!(first_meta.contains("assetBundleName:"));
        assert!(first_meta
            .lines()
            .any(|line| line.trim() == "assetBundleName:"));
        assert!(first_meta
            .lines()
            .any(|line| line.trim() == "assetBundleVariant:"));
        assert!(!first_meta.contains("mainObjectFileID:"));
        let first_guid = first_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .unwrap();
        let from_build_rows = reference_rows
            .chunks_exact(8)
            .find(|rows| reference_field(rows[0], "path=") == "Assets/Scenes/FromBuild")
            .unwrap();
        assert_eq!(
            first_guid,
            reference_field(from_build_rows[1], "HIER-GUID|")
        );

        let duplicate_scene = output_root.join("Assets/Scenes/Dupe.unity");
        assert!(duplicate_scene.is_file());
        let duplicate_meta =
            std::fs::read_to_string(duplicate_scene.with_extension("unity.meta")).unwrap();
        let duplicate_guid = duplicate_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .unwrap();
        assert_eq!(duplicate_guid.len(), 32);
        assert!(duplicate_guid
            .chars()
            .all(|character| character.is_ascii_hexdigit()));
        assert_ne!(duplicate_guid, "00000000000000000000000000000000");
        assert_ne!(duplicate_guid, first_guid);
        let streamed_scene = output_root.join("Assets/Scenes/Streamed.unity");
        assert!(streamed_scene.is_file());
        let streamed_meta =
            std::fs::read_to_string(streamed_scene.with_extension("unity.meta")).unwrap();
        assert!(streamed_meta.contains("assetBundleName:"));
        let streamed_guid = streamed_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .unwrap();
        assert_eq!(streamed_guid.len(), 32);
        assert!(streamed_guid
            .chars()
            .all(|character| character.is_ascii_hexdigit()));
        assert_ne!(streamed_guid, "00000000000000000000000000000000");
        assert_ne!(streamed_guid, first_guid);
        assert_ne!(streamed_guid, duplicate_guid);
        assert!(report.skipped_objects.iter().any(|notice| {
            notice.reason == "a previous duplicate BuildSettings scene already wrote this path"
        }));
        assert!(!output_root.join("Assets/GameObject/RootA.asset").exists());
        assert!(report.exported_object_count >= 5);

        std::fs::remove_dir_all(output_root).unwrap();
    }

    fn vector3_of(value: &crate::serialized_file::object::SerializedValue) -> [f32; 3] {
        let fields = value.struct_fields().expect("Vector3f struct");
        let mut result = [0.0; 3];
        for (index, name) in ["x", "y", "z"].iter().enumerate() {
            let field = fields
                .iter()
                .find(|field| field.name == *name)
                .unwrap_or_else(|| panic!("Vector3f has no {name}"));
            match &field.value {
                crate::serialized_file::object::SerializedValue::F32(component) => {
                    result[index] = *component
                }
                other => panic!("Vector3f.{name} is {other:?}"),
            }
        }
        result
    }

    fn quaternion_of(value: &crate::serialized_file::object::SerializedValue) -> [f32; 4] {
        let fields = value.struct_fields().expect("Quaternionf struct");
        let mut result = [0.0; 4];
        for (index, name) in ["x", "y", "z", "w"].iter().enumerate() {
            let field = fields
                .iter()
                .find(|field| field.name == *name)
                .unwrap_or_else(|| panic!("Quaternionf has no {name}"));
            match &field.value {
                crate::serialized_file::object::SerializedValue::F32(component) => {
                    result[index] = *component
                }
                other => panic!("Quaternionf.{name} is {other:?}"),
            }
        }
        result
    }

    /// Emit the fixture inputs consumed by `tools/probes/scene-processor` (the isolated reference
    /// runtime probe). Run with:
    /// `cargo test --lib scene_wiring::tests::emit_scene_processor_probe_inputs -- --ignored`.
    #[test]
    #[ignore = "writes probe inputs under target/scene-scratch"]
    fn emit_scene_processor_probe_inputs() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("scene-scratch");
        std::fs::create_dir_all(&directory).unwrap();
        let files = [
            ("level0", level0_file()),
            ("level1", level1_file()),
            ("level2", level1_file()),
            ("level3", level1_file()),
            (
                "globalgamemanagers",
                crate::processing::scene_metadata::tests::make_reference_build_settings_file(&[
                    "Assets/Scenes/FromBuild.unity",
                    "Assets/Scenes/Dupe.unity",
                    "Assets/Scenes/Dupe.unity",
                ]),
            ),
            (
                "CAB-scene",
                full_asset_bundle_file(&[("Assets/Scenes/Streamed.unity", "level3")]),
            ),
        ];
        for (name, bytes) in files {
            std::fs::write(directory.join(name), bytes).unwrap();
        }
    }

    #[test]
    fn fixture_bytes_are_hand_built_images_not_rust_encoder_output() {
        // The fixtures are hand-assembled SerializedFile images (header + metadata + TypeTree +
        // payload) with literal offsets. This guards the probe inputs against silently becoming
        // self-generated encoder round trips: the production loader must replay them unchanged.
        let level0 = level0_file();
        let parsed = SerializedFile::parse(&level0).unwrap();
        assert_eq!(parsed.metadata.objects.len(), 4);
        assert_eq!(
            parsed
                .metadata
                .objects
                .iter()
                .map(|object| object.class_id)
                .collect::<Vec<_>>(),
            [Some(1), Some(4), Some(1), Some(29)]
        );
        assert_eq!(
            parsed
                .metadata
                .objects
                .iter()
                .map(|object| object.path_id)
                .collect::<Vec<_>>(),
            [
                GAME_OBJECT_PATH_ID,
                TRANSFORM_PATH_ID,
                ORPHAN_GAME_OBJECT_PATH_ID,
                OCCLUSION_PATH_ID
            ]
        );
        let payload = parsed.decode_object_payload(0).unwrap();
        assert_eq!(payload.type_name, "GameObject");
    }
}
