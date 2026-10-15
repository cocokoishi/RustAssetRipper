//! Executable Unity project export composed from collection dispatch, staged planning, and
//! the `ExportHandler.Export` write sequence.
//!
//! This is the product-level project path: every decoded workspace object is routed through the
//! frozen `ProjectExporter.CreateCollection` stack walk ([`crate::export::project_exporter`]),
//! every route is materialized into a staged [`ProjectExportPlan`], and only then does
//! `execute_project_export` touch the disk. Objects whose baseline collection is not exportable
//! are reported as explicit notices. Some unported families retain an explicit generic YAML
//! fallback; AudioClip uses the source `YamlAudioExportCollection` path only when that collection
//! wins and no unresolved audio data would be dropped. TextAssets with non-empty `m_Script` bytes
//! use their binary exporter, settings-based extension, and TextScriptImporter.
//!
//! Scene definitions are processed into workspace hierarchies and emitted as `.unity` files
//! through the scene collection route, with scene IDs, pointers, and `DefaultImporter` metadata.
//! Modern PrefabProcessor hierarchies are emitted as `.prefab` collections with planned IDs,
//! generated Transforms, pointer mapping, and `PrefabImporter` metadata. A source-matched legacy
//! generated Prefab route covers a synthetic Unity 2017.4.40f1 hierarchy; other legacy branches,
//! full AssetBundle-derived importer metadata, and the remaining class-specific exporters remain
//! required integrations. A manager-backed `DllExportWithoutRenaming` route saves catalog-backed
//! assemblies to `Assets/Plugins` with source GUID/fileID mapping and a source-pinned Unity 2019.4
//! `PluginImporter`; assembly reserialization and additional manager-resolved assemblies remain
//! unresolved.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::hash::BuildHasher;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::PlatformResourceProvider;
use crate::processing::asset_bundle::{
    AssetBundleContainerTarget, resolve_asset_bundle_container_reference,
};
use crate::processing::asset_factory::{
    ClassLayoutSource, FactoryAssetKind, FactoryUnityVersion, FactoryVersionType,
    reference_class_layouts,
};
use crate::processing::managed_assembly::{
    ManagedAssembly, ManagedAssemblyCatalog, ManagedAssemblyLimits, mono_script_export_identity,
};
use crate::processing::scene_collections::{
    AssetBundleContainerDecodeError, SceneBundleMappingLimits, SceneDefinitionGuidPlan,
    SceneDefinitionPathSource, SceneGuidWords, SceneManagerKind,
    extract_asset_bundle_container_references,
};
use crate::processing::scene_hierarchy::{
    WorkspaceSceneAssetCategory, WorkspaceSceneHierarchyScene, WorkspaceSceneObjectIdentity,
};
use crate::processing::scene_naming::SceneUnityVersion;
use crate::processing::script_identity::ScriptIdentifier;
use crate::resource::{
    RootResourceFileCatalog, StreamedResourceContentLimits, StreamedResourceContentResolution,
    StreamedResourceRange, resolve_and_read_streamed_resource_info,
    resolve_and_read_streamed_resource_range,
};
use crate::resources::shader::{SHADER_CLASS_ID, ShaderView, script_has_decompiled_text};
use crate::serialized_file::object::{
    ObjectDefaultValueLimits, ParsedObjectPayload, SerializedFieldValue, SerializedValue,
    default_value_for_type_tree_node,
};
use crate::serialized_file::object_workspace::{ObjectWorkspace, WorkspaceObject};
use crate::serialized_file::reference_graph::{MultiFilePPtrResolution, MultiFileReferenceGraph};
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::anim_wiring::process_workspace_animation_clips;
use super::asset_path::ExportAssetPathIdentity;
use super::collection_claims::ClaimMainKind;
use super::collection_kind::ExportCollectionKind;
use super::collection_paths::{
    plan_script_export_paths, remove_assembly_file_extension, scripts_folder_name,
};
use super::empty_script::{MonoScriptIdentity, empty_script_content, is_predefined_assembly};
use super::export_extension::DEFAULT_EXPORT_EXTENSION;
use super::path_id_map::{
    PathIdMapAssetFacts, SerializedGameInfo, build_serialized_file, generated_type_interfaces,
    has_path_id_map_generated_types, serialize_path_id_map,
};
use super::pointer_map::{
    ExportCollectionPointerMetadata, ExportPointerGuid, ExportPointerMapLimits,
    ExportedAssetPointerEntry, ProjectExportPointerMap, ProjectPointerRequest,
    ProjectPointerResolution, UnityMetaPointer, UnityProjectAssetType, UnresolvedProjectPointer,
};
use super::post_export::PATH_ID_MAP_FILE;
use super::prefab_collection::{
    PrefabAssetRepresentation, PrefabExportIdPlan, PrefabExportPlanLimits,
    PrefabHierarchyAssetGroups, VersionedPrefabAsset, generate_prefab_source_pointer,
    plan_prefab_export_ids,
};
use super::project_exporter::{
    ObjectClassFacts, PlannedProjectFile, PlannedProjectMeta, PlannedProjectPostExportFile,
    PlannedProjectStreamFile, PlannedProjectSupportFile, ProjectExportError, ProjectExportNotice,
    ProjectExportPlan, ProjectExportSettings, ProjectPathPlanner, ProjectStreamPostExports,
    claim_facts_for_shader_text, execute_project_export_with_streams, export_extension_for,
    lighting_data_native_format_importer_for_version, mesh_native_format_importer_for_version,
    native_format_importer_for_version, object_class_name, package_manifest_json,
    project_version_file, select_collection_with_claim_facts,
    shader_importer_for_version_with_non_modifiable_textures,
};
use super::project_plan::plan_scene_export_paths;
use super::reference_assemblies::ReferenceAssemblyMaps;
use super::resource_extensions::text_export_extension;
use super::scene_collection::{
    ExportAssetCollectionIdentity, ExportAssetIdentity, ExportAssetObjectKey,
    SceneExportIdAssignment, SceneExportIdPlanLimits, SceneHierarchyAssetOrderLimits,
    plan_workspace_scene_export_ids_with_processed_assets,
};
use super::scene_wiring::{
    WorkspaceSceneProcessing, WorkspaceSceneProcessingLimits, process_workspace_scenes_and_prefabs,
};
use super::script_export_flow::{
    AssemblyExportType, add_assembly_file_extension, assembly_definition_file_name,
    assembly_definition_json, assembly_export_type, emit_assembly_definitions,
};
use super::script_hashing::{
    ScriptHashLimits, calculate_assembly_guid, calculate_script_file_id_str,
    calculate_script_guid_str,
};
use super::settings::StreamingAssetsMode;
use super::settings_file::{ExportSettingsFile, SerializedSettingsFile};
use super::shader_export::{
    YAML_SHADER_EXTENSION, export_shader_text, unity_patches, yaml_shader_patch_texts,
    yaml_shader_patches,
};
use super::streamed_assets::StreamedYamlAssetLimits;
use super::texture_project::{
    plan_cubemap_array_project_export_for_source_version,
    plan_cubemap_project_export_for_source_version,
    plan_lightmap_texture_project_export_for_source_version,
    plan_texture2d_array_project_export_for_source_version,
    plan_texture3d_project_export_for_source_version,
    plan_texture_project_export_for_source_version, TextureImporterLayout, TextureProjectPlan,
};
use super::unity_yaml::{
    UnityYamlMappingStyle, UnityYamlNode, UnityYamlSequenceStyle, UnityYamlWriteLimits,
    project_pointer_to_unity_yaml, write_unity_yaml_documents,
};
use super::value_yaml::{
    UnityYamlObjectConversionHints, UnityYamlObjectConversionLimits, UnityYamlObjectFieldMode,
    UnityYamlPPtrRequest, serialized_object_value_to_unity_yaml_document,
};
use super::workspace_yaml::sprite_atlas::workspace_sprite_atlas_yaml_overlay;
use super::workspace_yaml::{
    workspace_object_to_unity_yaml_document_with_atlas_overlay,
    workspace_prefab_hierarchy_to_unity_yaml_with_generated_transforms,
    workspace_scene_assignments_to_unity_yaml_with_project_generated_assets,
    UnityYamlWorkspaceCollectionLimits, WorkspaceGeneratedLightingDataYamlPointer,
    WorkspaceGeneratedTransformYamlContext, WorkspaceStreamedYamlResourceContext,
};

const MAX_PROJECT_OBJECTS: usize = 20_000_000;
const MAX_PROJECT_ASSET_PAYLOAD_BYTES: usize = 4 * 1024 * 1024 * 1024;

/// Resource and allocation bounds for one workspace project export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceProjectExportLimits {
    pub max_objects: usize,
    /// Original StreamingAssets files/directories and saved auxiliary assemblies.
    pub max_post_export_entries: usize,
    pub max_total_asset_payload_bytes: usize,
    pub pointer_map: ExportPointerMapLimits,
    pub yaml_collection: UnityYamlWorkspaceCollectionLimits,
    pub project_output: super::project_output::UnityProjectOutputLimits,
    pub yaml_write: UnityYamlWriteLimits,
    pub streamed_content: StreamedResourceContentLimits,
}

impl Default for WorkspaceProjectExportLimits {
    fn default() -> Self {
        Self {
            max_objects: MAX_PROJECT_OBJECTS,
            max_post_export_entries: 1_000_000,
            max_total_asset_payload_bytes: MAX_PROJECT_ASSET_PAYLOAD_BYTES,
            pointer_map: ExportPointerMapLimits::default(),
            yaml_collection: UnityYamlWorkspaceCollectionLimits::default(),
            project_output: super::project_output::UnityProjectOutputLimits::default(),
            yaml_write: UnityYamlWriteLimits::default(),
            streamed_content: StreamedResourceContentLimits::default(),
        }
    }
}

/// Summary of the files written by the workspace project path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceProjectExportReport {
    pub serialized_file_count: usize,
    pub exported_object_count: usize,
    pub asset_file_count: usize,
    pub support_file_count: usize,
    pub post_export_file_count: usize,
    /// Asset files that received an importer sidecar.
    pub meta_file_count: usize,
    pub total_asset_payload_bytes: usize,
    pub project_version: SceneUnityVersion,
    /// Exportable source-object collections, processed Scenes, and processed Prefabs written.
    pub collection_count: usize,
    /// Objects whose baseline collection writes nothing, with the reason.
    pub skipped_objects: Vec<ProjectExportNotice>,
    /// MonoScript pointers retained when the source has no matching type or assembly content.
    pub unresolved_script_content: Vec<ProjectExportNotice>,
    /// Files written through the generic asset-family renderer instead of their own.
    pub fallbacks: Vec<ProjectExportNotice>,
    /// Unknown or unreadable objects preserved through the source raw-data exporter. Their
    /// payload bytes are retained, but their internal fields and references could not be read.
    pub opaque_fallbacks: Vec<ProjectExportNotice>,
    /// Scene/Prefab processing diagnostics that did not prevent generic source-object export.
    pub processing_warnings: Vec<String>,
}

/// One object's export route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObjectRoute {
    /// `AssetExportCollection` and its subclasses: unique asset path plus `.meta`.
    Asset,
    /// `TextAssetExportCollection`: raw `m_Script` bytes, mode-based extension, and TextScriptImporter.
    TextAsset,
    /// `FontAssetExportCollection`: raw font bytes with a TrueTypeFontImporter and generated subasset IDs.
    FontBinary,
    /// `YamlAudioExportCollection`: AudioClip YAML with the source `audioclip` extension.
    AudioYaml,
    /// `AudioClipExportCollection`: decoded audio bytes with AudioImporter metadata.
    AudioBinary,
    /// `TextureExportCollection`: a converted image plus the generated TextureImporter sidecar.
    TextureImage,
    /// `ShaderExportCollection` using `DummyShaderTextExporter` or `SimpleShaderExporter`.
    ShaderText,
    /// `YamlShaderExportCollection` plus its version-gated Unity editor patches.
    ShaderYaml,
    /// `ManagerExportCollection`/`EditorBuildSettingsExportCollection`: fixed path, no `.meta`.
    Manager,
    /// `EngineAssetsExporter` redirects explicit default-resource/builtin-extra collections to
    /// their predefined EGUID/FGUID and preserves the source PathID without writing an asset.
    EngineRedirect,
    /// `ScriptExportCollection`/`EmptyScriptExportCollection`: deterministic `.cs` and MonoImporter metadata.
    Script,
    /// A manager-backed MonoScript whose source collection only registers a pointer; saved DLL
    /// bytes are staged once per loaded assembly when present.
    ScriptPointerOnly,
    /// `UnknownExportCollection`: raw payload bytes, no `.meta`.
    Unknown,
    /// `UnreadableExportCollection`: raw payload bytes, no `.meta`.
    Unreadable,
    /// A non-exportable collection: the baseline writes nothing.
    Skipped,
}

/// A routed object with its resolved plan inputs.
struct RoutedObject<'a> {
    object: &'a WorkspaceObject,
    class_id: i32,
    class_name: String,
    named_name: Option<String>,
    original_directory: Option<String>,
    facts: ObjectClassFacts,
    shader_view: Option<ShaderView>,
    audio_yaml_resource: Option<crate::resources::audio::AudioClipResourceInfo>,
    audio_binary_export: Option<AudioBinaryExportDetails>,
    text_asset_export: Option<TextAssetExportDetails>,
    font_collection: Option<RoutedFontCollection<'a>>,
    texture_export: Option<TextureProjectPlan>,
    script_export: Option<ScriptExportDetails>,
    scriptable_object_group: Option<RoutedScriptableObjectGroup<'a>>,
    controller_collection: Option<RoutedControllerCollection<'a>>,
    selection: super::project_exporter::CollectionSelection,
    route: ObjectRoute,
    /// Planning-time prediction that a streamed generic-asset `Texture2D`
    /// will skip its file for lack of usable image data. The materialization
    /// check re-verifies; the flag only types cross-collection pointers
    /// (frozen texture collections report `Meta` even when the file is
    /// skipped, while written textures stay `Serialized`).
    texture_file_skipped: bool,
    fallback_reason: Option<&'static str>,
    collection_index: usize,
    /// GUID written into the collection's `.meta` file.
    guid: [u8; 16],
    /// GUID registered in the source project pointer map. A reference MonoScript uses the
    /// reference assembly GUID here while retaining its calculated script GUID in its `.meta`.
    pointer_guid: [u8; 16],
    file_id: i64,
}

/// A generated ScriptableObjectGroup produced by the source Timeline/PostProcessing processor.
struct RoutedScriptableObjectGroup<'a> {
    extension: &'static str,
    children: Vec<RoutedScriptableObjectGroupChild<'a>>,
}

struct RoutedScriptableObjectGroupChild<'a> {
    object: &'a WorkspaceObject,
    file_id: i64,
}

#[derive(Clone, Debug)]
struct ScriptExportDetails {
    identity: MonoScriptIdentity,
    execution_order: i16,
    guid: [u8; 16],
    pointer_guid: [u8; 16],
    pointer_file_id: i64,
    export_type: AssemblyExportType,
    missing_assembly: bool,
    source_script_present: Option<bool>,
}

#[derive(Clone, Debug)]
struct SavedAssemblyExport {
    assembly_name: String,
    source_virtual_path: String,
    relative_path: String,
    guid: [u8; 16],
    generated_forwarder: Option<String>,
}

#[derive(Clone, Debug)]
struct TextAssetExportDetails {
    script: Vec<u8>,
    extension: String,
}

#[derive(Clone, Debug)]
struct AudioBinaryExportDetails {
    bytes: Vec<u8>,
    collection_extension: String,
    importer: UnityYamlNode,
}

struct RoutedFontCollection<'a> {
    children: Vec<RoutedFontCollectionChild<'a>>,
}

struct RoutedFontCollectionChild<'a> {
    object: &'a WorkspaceObject,
    file_id: i64,
}

/// One processed Scene collection staged for the Unity project exporter.
struct RoutedSceneCollection {
    scene_index: usize,
    name: String,
    path: String,
    path_source: SceneDefinitionPathSource,
    guid_plan: SceneDefinitionGuidPlan,
    asset_bundle_name: Option<String>,
    assignments: Vec<SceneExportIdAssignment>,
    exportable_assignments: Vec<SceneExportIdAssignment>,
    collection_index: usize,
    guid: [u8; 16],
}

struct RoutedLightingDataAsset {
    scene_index: usize,
    source_owner: ExportAssetIdentity,
    render_settings_owner: ExportAssetIdentity,
    scene_guid: [u8; 16],
    collection_index: usize,
    identity: ExportAssetIdentity,
    guid: [u8; 16],
    original_directory: String,
    source_version: SceneUnityVersion,
    type_tree: crate::serialized_file::metadata::TypeTree,
    root: SerializedValue,
    renderer_uv_mesh_targets: Vec<Option<ExportAssetIdentity>>,
}

#[derive(Clone, Copy)]
struct LightingDataRendererInput<'object> {
    identity: ExportAssetIdentity,
    file_id: i64,
    value: &'object SerializedValue,
    uv_mesh_target: Option<ExportAssetIdentity>,
}

#[derive(Clone, Copy)]
struct LightingDataTerrainInput<'object> {
    identity: ExportAssetIdentity,
    file_id: i64,
    value: &'object SerializedValue,
}

#[derive(Clone, Copy)]
struct LightingDataLightInput<'object> {
    identity: ExportAssetIdentity,
    file_id: i64,
    value: &'object SerializedValue,
}

struct PreparedSceneCollections {
    processing: WorkspaceSceneProcessing,
    routed: Vec<RoutedSceneCollection>,
    member_keys: HashSet<ExportAssetObjectKey>,
    prefabs: Vec<RoutedPrefabCollection>,
    prefab_member_keys: HashSet<ExportAssetObjectKey>,
    processing_warnings: Vec<String>,
    used_guids: HashSet<[u8; 16]>,
}

/// One PrefabExportCollection reconstructed from a PrefabProcessor hierarchy.
struct RoutedPrefabCollection {
    name: String,
    class_name: String,
    named_name: Option<String>,
    /// Bundle-container `OriginalDirectory` of the prefab root (generated prefabs) or the
    /// prefab instance itself (existing prefabs); mirrors `CreatePrefabForRoot` copying
    /// `root.GetBestDirectory()` into `OverrideDirectory`. Falls back to the class
    /// directory when the bundle container assigns no path.
    original_directory: Option<String>,
    asset_bundle_name: Option<String>,
    representation: PrefabAssetRepresentation,
    export_ids: PrefabExportIdPlan,
    collection_index: usize,
    guid: [u8; 16],
}

struct WorkspaceProjectExportInputs<'inputs, 'provider, 'graph> {
    output_root: &'inputs Path,
    delete_existing: bool,
    filesystem: &'inputs VirtualFileSystem,
    root_catalog: &'inputs RootResourceFileCatalog,
    resource_provider: &'inputs PlatformResourceProvider<'provider>,
    workspace: &'inputs SerializedFileWorkspace,
    objects: &'inputs ObjectWorkspace,
    reference_graph: &'inputs MultiFileReferenceGraph<'graph>,
    settings: &'inputs ExportSettingsFile,
    streaming_assets_mode: StreamingAssetsMode,
    assembly_manager_is_set: bool,
    managed_assemblies: Option<&'inputs ManagedAssemblyCatalog>,
    limits: WorkspaceProjectExportLimits,
}

struct WorkspaceProjectExportContext<'inputs, 'provider, 'graph> {
    inputs: WorkspaceProjectExportInputs<'inputs, 'provider, 'graph>,
    editor_version_text: &'inputs str,
    project_version: SceneUnityVersion,
    has_mono_scripts: bool,
    reference_assembly_maps: Option<ReferenceAssemblyMaps>,
    settings_facts: ProjectExportSettings,
    opaque_objects: HashSet<(usize, usize)>,
    scene_processing: WorkspaceSceneProcessing,
    processed_animation_roots: HashMap<(usize, usize), SerializedValue>,
    scene_collections: Vec<RoutedSceneCollection>,
    scene_member_keys: HashSet<ExportAssetObjectKey>,
    prefab_collections: Vec<RoutedPrefabCollection>,
    prefab_member_keys: HashSet<ExportAssetObjectKey>,
    scriptable_object_groups: PreparedScriptableObjectGroups,
    controller_collections: PreparedControllerCollections,
    processing_warnings: Vec<String>,
    used_guids: HashSet<[u8; 16]>,
    lightmap_texture_directories: HashMap<(usize, usize), String>,
    scene_lighting_subasset_directories: HashMap<(usize, usize), String>,
    bundle_container_original_directories: HashMap<(usize, usize), String>,
}

struct RoutedProjectObjects<'inputs> {
    routed: Vec<RoutedObject<'inputs>>,
    notices: Vec<ProjectExportNotice>,
    unresolved_script_content: Vec<ProjectExportNotice>,
    fallback_notices: Vec<ProjectExportNotice>,
    opaque_fallback_notices: Vec<ProjectExportNotice>,
    next_collection_index: usize,
    next_guid_sequence: u64,
    used_guids: HashSet<[u8; 16]>,
}

struct ProjectRoutingContext<'context, 'provider, 'graph> {
    filesystem: &'context VirtualFileSystem,
    root_catalog: &'context RootResourceFileCatalog,
    resource_provider: &'context PlatformResourceProvider<'provider>,
    workspace: &'context SerializedFileWorkspace,
    objects: &'context ObjectWorkspace,
    reference_graph: &'context MultiFileReferenceGraph<'graph>,
    settings: &'context ExportSettingsFile,
    project_version: SceneUnityVersion,
    editor_version_text: &'context str,
    settings_facts: ProjectExportSettings,
    assembly_manager_is_set: bool,
    managed_assemblies: Option<&'context ManagedAssemblyCatalog>,
    reference_assembly_maps: Option<&'context ReferenceAssemblyMaps>,
    opaque_objects: &'context HashSet<(usize, usize)>,
    scene_member_keys: &'context HashSet<ExportAssetObjectKey>,
    prefab_member_keys: &'context HashSet<ExportAssetObjectKey>,
    scriptable_object_groups: &'context PreparedScriptableObjectGroups,
    controller_collections: &'context PreparedControllerCollections,
    lightmap_texture_directories: &'context HashMap<(usize, usize), String>,
    scene_lighting_subasset_directories: &'context HashMap<(usize, usize), String>,
    bundle_container_original_directories: &'context HashMap<(usize, usize), String>,
    existing_guids: &'context HashSet<[u8; 16]>,
    limits: WorkspaceProjectExportLimits,
}

/// The collection type name that would wrap the object, for diagnostics.
fn identity_of(entry: &RoutedObject<'_>) -> ExportAssetIdentity {
    ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: entry.object.file_index,
        },
        object_index: entry.object.object_index,
        path_id: entry.object.path_id,
        class_id: entry.class_id,
    }
}

mod animator_collections;
mod animator_controller;
mod asset_support;
mod audio_asset;
mod collection_planning;
mod entrypoints;
mod export;
mod font_asset;
mod importer_metadata;
mod lighting_asset;
mod object_routing;
mod planning;
mod post_export_outputs;
mod project_settings;
mod scene_collections;
mod script_exports;
mod scriptable_object_groups;
mod workspace_identity;

use self::animator_collections::{
    PreparedControllerCollections, RoutedControllerCollection, RoutedControllerMember,
    hide_controller_behaviour, prepare_controller_collections,
};
use self::animator_controller::{
    normalize_controller_collection_documents, rebuild_animator_controller_documents,
    RebuiltControllerDocuments,
};
use self::asset_support::*;
use self::audio_asset::*;
use self::collection_planning::{
    GeneratedCollectionPreparationContext, PreparedGeneratedCollections, ProjectPointerPlan,
    ProjectPointerPlanningContext, build_project_pointer_map, prepare_generated_collections,
};
use self::font_asset::*;
use self::importer_metadata::*;
use self::lighting_asset::*;
use self::object_routing::route_project_objects;
use self::planning::{
    LightingDataPlanningContext, PrefabPlanningContext, ProjectExportPlanBuilder,
    RoutedAssetPlanningContext, ScenePlanningContext, StagedProjectFiles, plan_lighting_data_files,
    plan_prefab_files, plan_routed_asset_files, plan_scene_files,
};
use self::scene_collections::*;
use self::script_exports::*;
use self::scriptable_object_groups::*;
use self::workspace_identity::*;

pub use self::entrypoints::{
    export_workspace_yaml_project, export_workspace_yaml_project_with_script_manager,
    export_workspace_yaml_project_with_settings,
};
pub use self::export::{
    export_workspace_yaml_project_with_assembly_catalog,
    export_workspace_yaml_project_with_serialized_settings,
};

#[cfg(test)]
#[path = "project_workflow/tests/mod.rs"]
mod tests;
