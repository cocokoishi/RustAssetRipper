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
    lightmap_settings_has_lighting_data_asset, WorkspaceSceneHierarchies,
    WorkspaceSceneHierarchyBuilder, WorkspaceSceneHierarchyError, WorkspaceSceneHierarchyLimits,
    WorkspaceSceneHierarchyMembershipError, WorkspaceSceneHierarchyMembershipLimits,
    WorkspaceSceneObjectIdentity,
};
use crate::processing::scene_metadata::{
    extract_build_settings_scene_paths, BuildSettingsSceneLimits,
};
use crate::processing::scene_naming::SceneUnityVersion;

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
        objects,
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
            let class_id = object
                .class_id
                .map(i32::from)
                .or_else(|| {
                    let workspace_object = objects.get(file_index, object_index)?;
                    workspace_object
                        .resolved_type_index
                        .map(|_| workspace_object.raw_type_id)
                })
                .unwrap_or(0);
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
    objects: &ObjectWorkspace,
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
        let mut decoded_roots = Vec::new();
        let mut scene_object_count = 0usize;
        let mut all_scene_objects_decoded = true;
        for (object_index, metadata_object) in parsed.metadata.objects.iter().enumerate() {
            let class_id = metadata_object.class_id.map(i32::from).or_else(|| {
                metadata_object
                    .resolved_type_index
                    .map(|_| metadata_object.raw_type_id)
            });
            if !matches!(class_id, Some(1 | 4 | 224 | 1001)) {
                continue;
            }
            scene_object_count = scene_object_count.checked_add(1).ok_or_else(|| {
                WorkspaceSceneProcessingError::SceneGraph {
                    file_index,
                    message: "scene object count overflow".to_owned(),
                }
            })?;
            let Some(workspace_object) = objects.get(file_index, object_index) else {
                all_scene_objects_decoded = false;
                break;
            };
            if workspace_object.file_index != file_index
                || workspace_object.object_index != object_index
                || workspace_object.path_id != metadata_object.path_id
            {
                return Err(WorkspaceSceneProcessingError::SceneGraph {
                    file_index,
                    message: format!(
                        "ObjectWorkspace identity mismatch at object {object_index}: expected PathID {}, got file {} object {} PathID {}",
                        metadata_object.path_id,
                        workspace_object.file_index,
                        workspace_object.object_index,
                        workspace_object.path_id,
                    ),
                });
            }
            let Ok(payload) = &workspace_object.payload else {
                all_scene_objects_decoded = false;
                break;
            };
            decoded_roots.try_reserve(1).map_err(|_| {
                WorkspaceSceneProcessingError::SceneGraph {
                    file_index,
                    message: "could not allocate decoded scene roots".to_owned(),
                }
            })?;
            decoded_roots.push((object_index, payload.root.clone()));
        }
        let mut scene_graph =
            if all_scene_objects_decoded && decoded_roots.len() == scene_object_count {
                SceneGraph::from_decoded_roots(&parsed.metadata, &decoded_roots, limits.scene_graph)
            } else {
                SceneGraph::build(&parsed, limits.scene_graph)
            }
            .map_err(|error| WorkspaceSceneProcessingError::SceneGraph {
                file_index,
                message: error.to_string(),
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
        let has_lighting_data_asset = file
            .metadata
            .prefix
            .unity_version_text
            .as_deref()
            .and_then(SceneUnityVersion::parse)
            .map(lightmap_settings_has_lighting_data_asset);
        builder
            .add_collection_graph_with_lighting_data_asset_support(
                file_index,
                &scene_graph,
                roots,
                audit,
                has_lighting_data_asset,
                references,
            )
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
    include!("scene_wiring_tests_01.rs");
}
