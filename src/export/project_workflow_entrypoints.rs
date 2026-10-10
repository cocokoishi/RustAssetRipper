use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::hash::BuildHasher;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::PlatformResourceProvider;
use crate::processing::asset_factory::{
    reference_class_layouts, ClassLayoutSource, FactoryAssetKind, FactoryUnityVersion,
    FactoryVersionType,
};
use crate::processing::managed_assembly::{
    mono_script_export_identity, ManagedAssembly, ManagedAssemblyCatalog, ManagedAssemblyLimits,
};
use crate::processing::scene_collections::{
    SceneDefinitionGuidPlan, SceneDefinitionPathSource, SceneGuidWords, SceneManagerKind,
};
use crate::processing::scene_hierarchy::{
    WorkspaceSceneAssetCategory, WorkspaceSceneHierarchyScene, WorkspaceSceneObjectIdentity,
};
use crate::processing::scene_naming::SceneUnityVersion;
use crate::processing::script_identity::ScriptIdentifier;
use crate::resource::{
    resolve_and_read_streamed_resource_info, resolve_and_read_streamed_resource_range,
    RootResourceFileCatalog, StreamedResourceContentLimits, StreamedResourceContentResolution,
    StreamedResourceRange,
};
use crate::resources::shader::{script_has_decompiled_text, ShaderView, SHADER_CLASS_ID};
use crate::serialized_file::object::{
    default_value_for_type_tree_node, ObjectDefaultValueLimits, ParsedObjectPayload,
    SerializedFieldValue, SerializedValue,
};
use crate::serialized_file::object_workspace::{ObjectWorkspace, WorkspaceObject};
use crate::serialized_file::reference_graph::{MultiFilePPtrResolution, MultiFileReferenceGraph};
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::asset_path::ExportAssetPathIdentity;
use super::collection_claims::ClaimMainKind;
use super::collection_kind::ExportCollectionKind;
use super::collection_paths::{
    plan_script_export_paths, remove_assembly_file_extension, scripts_folder_name,
};
use super::empty_script::{empty_script_content, is_predefined_assembly, MonoScriptIdentity};
use super::export_extension::DEFAULT_EXPORT_EXTENSION;
use super::path_id_map::{
    build_serialized_file, generated_type_interfaces, has_path_id_map_generated_types,
    serialize_path_id_map, PathIdMapAssetFacts, SerializedGameInfo,
};
use super::pointer_map::{
    ExportCollectionPointerMetadata, ExportPointerGuid, ExportPointerMapLimits,
    ExportedAssetPointerEntry, ProjectExportPointerMap, ProjectPointerRequest,
    ProjectPointerResolution, UnityMetaPointer, UnityProjectAssetType, UnresolvedProjectPointer,
};
use super::post_export::PATH_ID_MAP_FILE;
use super::prefab_collection::{
    generate_prefab_source_pointer, plan_prefab_export_ids, PrefabAssetRepresentation,
    PrefabExportIdPlan, PrefabExportPlanLimits, PrefabHierarchyAssetGroups, VersionedPrefabAsset,
};
use super::project_exporter::{
    claim_facts_for_shader_text, execute_project_export, export_extension_for,
    native_format_importer_for_version, object_class_name, package_manifest_json,
    project_version_file, select_collection_with_claim_facts,
    shader_importer_for_version_with_non_modifiable_textures, ObjectClassFacts, PlannedProjectFile,
    PlannedProjectMeta, PlannedProjectPostExportFile, PlannedProjectSupportFile,
    ProjectExportError, ProjectExportNotice, ProjectExportPlan, ProjectExportSettings,
    ProjectPathPlanner,
};
use super::project_plan::plan_scene_export_paths;
use super::reference_assemblies::ReferenceAssemblyMaps;
use super::resource_extensions::text_export_extension;
use super::scene_collection::{
    plan_workspace_scene_export_ids_with_processed_assets, ExportAssetCollectionIdentity,
    ExportAssetIdentity, ExportAssetObjectKey, SceneExportIdAssignment, SceneExportIdPlanLimits,
    SceneHierarchyAssetOrderLimits,
};
use super::scene_wiring::{
    process_workspace_scenes_and_prefabs, WorkspaceSceneProcessing, WorkspaceSceneProcessingLimits,
};
use super::script_export_flow::{
    add_assembly_file_extension, assembly_definition_file_name, assembly_definition_json,
    assembly_export_type, emit_assembly_definitions, AssemblyExportType,
};
use super::script_hashing::{
    calculate_assembly_guid, calculate_script_file_id_str, calculate_script_guid_str,
    ScriptHashLimits,
};
use super::settings_file::ExportSettingsFile;
use super::shader_export::{
    export_shader_text, unity_patches, yaml_shader_patch_texts, yaml_shader_patches,
    YAML_SHADER_EXTENSION,
};
use super::streamed_assets::StreamedYamlAssetLimits;
use super::texture_project::{
    plan_lightmap_texture_project_export, plan_texture_project_export, TextureImporterLayout,
    TextureProjectPlan,
};
use super::unity_yaml::{
    project_pointer_to_unity_yaml, write_unity_yaml_documents, UnityYamlMappingStyle,
    UnityYamlNode, UnityYamlSequenceStyle, UnityYamlWriteLimits,
};
use super::value_yaml::{
    serialized_object_value_to_unity_yaml_document, UnityYamlObjectConversionHints,
    UnityYamlObjectConversionLimits, UnityYamlObjectFieldMode, UnityYamlPPtrRequest,
};
use super::workspace_yaml::{
    workspace_object_to_unity_yaml_document_with_streamed_resources,
    workspace_prefab_hierarchy_to_unity_yaml_with_generated_transforms,
    workspace_scene_assignments_to_unity_yaml_with_project_generated_assets,
    UnityYamlWorkspaceCollectionLimits, WorkspaceGeneratedLightingDataYamlPointer,
    WorkspaceGeneratedTransformYamlContext, WorkspaceStreamedYamlResourceContext,
};

const MAX_PROJECT_OBJECTS: usize = 20_000_000;
const MAX_PROJECT_ASSET_PAYLOAD_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// Resource and allocation bounds for one workspace project export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceProjectExportLimits {
    pub max_objects: usize,
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
    /// `YamlAudioExportCollection`: AudioClip YAML with the source `audioclip` extension.
    AudioYaml,
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
    text_asset_export: Option<TextAssetExportDetails>,
    texture_export: Option<TextureProjectPlan>,
    script_export: Option<ScriptExportDetails>,
    selection: super::project_exporter::CollectionSelection,
    route: ObjectRoute,
    collection_index: usize,
    /// GUID written into the collection's `.meta` file.
    guid: [u8; 16],
    /// GUID registered in the source project pointer map. A reference MonoScript uses the
    /// reference assembly GUID here while retaining its calculated script GUID in its `.meta`.
    pointer_guid: [u8; 16],
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
    scene_guid: [u8; 16],
    collection_index: usize,
    identity: ExportAssetIdentity,
    guid: [u8; 16],
    original_directory: String,
    source_version: SceneUnityVersion,
    type_tree: crate::serialized_file::metadata::TypeTree,
    root: SerializedValue,
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
    asset_bundle_name: Option<String>,
    representation: PrefabAssetRepresentation,
    export_ids: PrefabExportIdPlan,
    collection_index: usize,
    guid: [u8; 16],
}

/// Execute a workspace-backed Unity project export with explicit export settings.
#[allow(clippy::too_many_arguments)]
pub fn export_workspace_yaml_project_with_settings(
    output_root: &Path,
    delete_existing: bool,
    filesystem: &VirtualFileSystem,
    root_catalog: &RootResourceFileCatalog,
    resource_provider: &PlatformResourceProvider<'_>,
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    settings: &ExportSettingsFile,
    limits: WorkspaceProjectExportLimits,
) -> Result<WorkspaceProjectExportReport, Box<dyn Error>> {
    export_workspace_yaml_project_with_script_manager(
        output_root,
        delete_existing,
        filesystem,
        root_catalog,
        resource_provider,
        workspace,
        objects,
        reference_graph,
        settings,
        false,
        limits,
    )
}

/// Execute project export with the source Mono manager availability fact supplied by import.
#[allow(clippy::too_many_arguments)]
pub fn export_workspace_yaml_project_with_script_manager(
    output_root: &Path,
    delete_existing: bool,
    filesystem: &VirtualFileSystem,
    root_catalog: &RootResourceFileCatalog,
    resource_provider: &PlatformResourceProvider<'_>,
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    settings: &ExportSettingsFile,
    assembly_manager_is_set: bool,
    limits: WorkspaceProjectExportLimits,
) -> Result<WorkspaceProjectExportReport, Box<dyn Error>> {
    export_workspace_yaml_project_with_assembly_catalog(
        output_root,
        delete_existing,
        filesystem,
        root_catalog,
        resource_provider,
        workspace,
        objects,
        reference_graph,
        settings,
        assembly_manager_is_set,
        None,
        limits,
    )
}
