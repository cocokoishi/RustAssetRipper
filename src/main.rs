#[cfg(test)]
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use assetripper::export::audio_export::try_decode_audio;
use assetripper::export::audio_export::{
    convert_ogg_to_wav_with_limit, converts_ogg_to_wav, plan_audio_clip_collection,
    AudioDecodeFact, MAX_DECODED_AUDIO_BYTES,
};
use assetripper::export::pointer_map::{
    ExportCollectionPointerMetadata, ExportPointerMapLimits, ExportedAssetPointerEntry,
    ProjectExportPointerMap, UnityProjectAssetType,
};
use assetripper::export::project_workflow::{
    export_workspace_yaml_project_with_serialized_settings, WorkspaceProjectExportLimits,
};
use assetripper::export::scene_collection::{
    ExportAssetCollectionIdentity, ExportAssetIdentity, SceneExportIdAssignment,
};
use assetripper::export::settings::AudioExportFormat as SettingsAudioFormat;
use assetripper::export::settings::ShaderExportMode;
use assetripper::export::settings::{ExportDefaults, ImageExportFormat as SettingsImageFormat};
use assetripper::export::settings_file::ExportSettingsFile;
use assetripper::export::shader_export::export_shader_text;
use assetripper::export::streamed_assets::StreamedYamlAssetLimits;
use assetripper::export::unity_yaml::{write_unity_yaml_documents, UnityYamlWriteLimits};
use assetripper::export::value_yaml::UnityYamlObjectConversionLimits;
use assetripper::export::workspace_yaml::{
    workspace_assignments_to_unity_yaml_with_streamed_resources,
    workspace_nav_mesh_build_settings_overlay,
    workspace_object_to_unity_yaml_document_with_streamed_resources,
    UnityYamlWorkspaceCollectionLimits, WorkspaceStreamedYamlResourceContext,
};
use assetripper::io::binary::Endian;
use assetripper::io::dependency::DependencyIndexLimits;
use assetripper::io::filesystem::VirtualFileSystem;
use assetripper::io::game_structure::{MixedGameStructure, MixedGameStructureLimits};
use assetripper::io::platform_file_inventory::{
    select_scripting_backend, PlatformFileInventoryLimits, PlatformFileInventoryOptions,
    PlatformWorkspaceLoadOptions, ScriptingBackend as PlatformScriptingBackend,
};
use assetripper::io::platform_structure::{
    detect_platform_structure, PlatformResourceProvider, PlatformStructureLimits,
};
use assetripper::io::stream::logical_file_path;
use assetripper::processing::asset_bundle::get_asset_bundle_name;
use assetripper::processing::asset_factory::ClassLayoutSource;
#[cfg(test)]
use assetripper::processing::asset_factory::{
    FactoryUnityVersion, SourceFactoryLookupError, SourceTpkLookupError, SourceTpkTrees,
};
#[cfg(test)]
use assetripper::processing::managed_assembly::{ManagedAssembly, ManagedAssemblyLimits};
use assetripper::processing::managed_assembly::{
    ManagedAssemblyCatalog, ManagedAssemblyCatalogLimits, ManagedScriptResolutionOutcome,
    ManagedScriptResolutionReport,
};
use assetripper::processing::managed_assembly_schema::ManagedSchemaLimits;
use assetripper::processing::prefab_processing::{
    animator_controller_source_dependency_inventory, plan_workspace_prefab_processing,
    plan_workspace_processed_prefab_assets, plan_workspace_scene_hierarchy_bundle_names,
    WorkspaceBundleAssetNameFact, WorkspacePrefabProcessingEvent, WorkspacePrefabProcessingLimits,
    WorkspaceProcessedPrefabAssetPlanLimits, WorkspaceSceneBundleSource,
    WorkspaceSceneHierarchyBundlePlanLimits,
};
use assetripper::processing::project_settings::{
    EditorBuildSettingsLimits, EditorBuildSettingsModel,
};
use assetripper::processing::scene::{
    SceneGraph, SceneGraphLimits, SceneHierarchyAuditLimits, SceneRootDiscoveryLimits,
};
use assetripper::processing::scene_collections::{
    extract_occlusion_scene_guid, extract_streamed_scene_bundle_metadata,
    map_streamed_scene_bundle_paths, plan_scene_definition, resolve_scene_collection_path,
    scene_manager_kind, BuildSettingsScenePathSet, SceneBundleMappingLimits,
    SceneBundlePathMappingSource, SceneDefinitionPlanLimits, SceneGuidWords, SceneManagerKind,
    StreamedSceneBundleMetadata, WorkspaceSceneDefinitionLimits, WorkspaceSceneDefinitions,
};
use assetripper::processing::scene_hierarchy::{
    WorkspaceGeneratedTransformPlanBuilder, WorkspaceGeneratedTransformPlanLimits,
    WorkspacePrefabInstanceRootResolution, WorkspaceSceneAssetCategory,
    WorkspaceSceneComponentEvidence, WorkspaceSceneHierarchyBuilder, WorkspaceSceneHierarchyLimits,
};
use assetripper::processing::scene_metadata::{
    decode_build_settings_scene_paths_from_parts, BuildSettingsSceneLimits,
};
use assetripper::resource::{
    resolve_and_read_streamed_resource_info, resolve_and_read_streamed_resource_range,
    resolve_streamed_resource_info, scan_streamed_resource_infos, ResolvedResourceFile,
    RootResourceCatalogLimits, RootResourceFileCatalog, StreamedResourceContentLimits,
    StreamedResourceContentResolution, StreamedResourceInfo, StreamedResourceRange,
    StreamedResourceResolution, StreamedResourceScanLimits,
};
use assetripper::resources::audio::{AudioClipView, AudioDataSource, AUDIO_CLIP_CLASS_ID};
use assetripper::resources::glb::build_glb;
use assetripper::resources::image_export::ImageExportFormat;
use assetripper::resources::mesh::MeshView;
use assetripper::resources::packed_bit_vector::PackedBitVectorLimits;
use assetripper::resources::shader::{ShaderView, SHADER_CLASS_ID};
use assetripper::resources::texture::{
    export_cubemap_array_image, export_texture2d_array_image,
    export_texture2d_image_with_unity_version, export_texture3d_image, Texture2DView,
    CUBEMAP_CLASS_ID,
};
use assetripper::resources::texture_array::{
    CubemapArrayView, Texture2DArrayView, Texture3DView, CUBEMAPARRAY_CLASS_ID,
    TEXTURE2DARRAY_CLASS_ID, TEXTURE3D_CLASS_ID,
};
use assetripper::resources::vertex_data_blob::{EndianType, UnityVersion};
use assetripper::scheme::{
    BatchMountLimits, BatchMountReport, ContainerMountLimits, PathMountInput, SchemeReader,
    SchemeReaderLimits,
};
use assetripper::serialized_file::collection_workspace::{
    WorkspaceAssetCollections, WorkspaceCollectionLimits,
};
use assetripper::serialized_file::file::SerializedFile;
use assetripper::serialized_file::metadata::ObjectInfo;
use assetripper::serialized_file::object::ObjectDecodeLimits;
use assetripper::serialized_file::object::SerializedValue;
use assetripper::serialized_file::object_workspace::{
    ObjectWorkspace, ObjectWorkspaceLimits, WorkspaceObject,
};
use assetripper::serialized_file::reference_graph::{
    MultiFileObjectReference, MultiFilePPtrResolution, MultiFileReferenceGraph,
    MultiFileReferenceGraphLimits, ReferenceGraphLimits,
};
use assetripper::serialized_file::workspace::{
    LoadedSerializedFile, PreExpandedContainer, SerializedFileDependencyReport,
    SerializedFileWorkspace, SerializedFileWorkspaceLimits, SerializedFileWorkspaceLoadOptions,
    WorkspaceSerializedBundle,
};

const MAX_INPUT_PATHS: usize = 100_000;
const MAX_STREAMED_SCENE_BUNDLE_OBJECTS: usize = 1_000_000;
const MAX_TOTAL_STREAMED_SCENE_BUNDLE_ENTRIES: usize = 1_000_000;
const MAX_TOTAL_STREAMED_SCENE_BUNDLE_STRING_BYTES: usize = 256 * 1024 * 1024;
const MAX_TOTAL_STREAMED_SCENE_PATH_MAPPINGS: usize = 1_000_000;
const MAX_OBJECT_DECODE_DIAGNOSTICS: usize = 128;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
struct MainFileTypeTreeTestLayouts;

#[cfg(test)]
impl ClassLayoutSource for MainFileTypeTreeTestLayouts {
    fn generated_class_name(
        &self,
        class_id: i32,
        version: &FactoryUnityVersion,
    ) -> Result<Option<&str>, SourceFactoryLookupError> {
        if assetripper::export::path_id_map::has_path_id_map_generated_types(class_id) {
            return assetripper::processing::asset_factory::reference_class_layouts()
                .generated_class_name(class_id, version);
        }
        Ok(Some("SyntheticTypeTreeTestObject"))
    }

    fn tpk_trees(
        &self,
        _class_id: i32,
        _version: &FactoryUnityVersion,
    ) -> Result<Option<SourceTpkTrees>, SourceTpkLookupError> {
        Ok(None)
    }
}

#[cfg(test)]
thread_local! {
    // Main's broad command tests use small hand-authored TypeTrees to exercise command wiring.
    // Dedicated source-factory tests switch this off so production layouts remain authoritative.
    static USE_FILE_TYPE_TREE_TEST_LAYOUTS: Cell<bool> = const { Cell::new(true) };
}

#[cfg(test)]
struct SourceFactoryLayoutGuard {
    previous: bool,
}

#[cfg(test)]
impl SourceFactoryLayoutGuard {
    fn use_source_factory() -> Self {
        let previous = USE_FILE_TYPE_TREE_TEST_LAYOUTS.with(|flag| flag.replace(false));
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for SourceFactoryLayoutGuard {
    fn drop(&mut self) {
        USE_FILE_TYPE_TREE_TEST_LAYOUTS.with(|flag| flag.set(self.previous));
    }
}

fn active_class_layout_source() -> &'static dyn ClassLayoutSource {
    #[cfg(test)]
    {
        static TEST_LAYOUTS: MainFileTypeTreeTestLayouts = MainFileTypeTreeTestLayouts;
        if USE_FILE_TYPE_TREE_TEST_LAYOUTS.with(Cell::get) {
            return &TEST_LAYOUTS;
        }
    }
    assetripper::processing::asset_factory::reference_class_layouts()
}

#[cfg(test)]
fn load_workspace_with_dependencies(
    filesystem: &mut VirtualFileSystem,
    inputs: &[assetripper::io::dependency::DependencyFilePath],
    registered_files: &[assetripper::io::dependency::DependencyFilePath],
    data_paths: &[String],
    limits: WorkspaceLoadLimits,
) -> Result<
    (SerializedFileWorkspace, SerializedFileDependencyReport),
    Box<assetripper::serialized_file::workspace::SerializedFileWorkspaceError>,
> {
    load_workspace_with_preexpanded_containers(
        filesystem,
        inputs,
        registered_files,
        data_paths,
        limits,
        &[],
    )
}

#[derive(Clone, Copy, Debug, Default)]
struct WorkspaceLoadLimits {
    serialized_files: SerializedFileWorkspaceLimits,
    dependency_index: DependencyIndexLimits,
    containers: ContainerMountLimits,
}

fn load_workspace_with_mount_report(
    filesystem: &mut VirtualFileSystem,
    inputs: &[assetripper::io::dependency::DependencyFilePath],
    registered_files: &[assetripper::io::dependency::DependencyFilePath],
    data_paths: &[String],
    limits: WorkspaceLoadLimits,
    mount_report: &mut BatchMountReport,
) -> Result<(SerializedFileWorkspace, SerializedFileDependencyReport), Box<dyn Error>> {
    let preexpanded_containers = take_preexpanded_containers(mount_report)?;
    load_workspace_with_preexpanded_containers(
        filesystem,
        inputs,
        registered_files,
        data_paths,
        limits,
        &preexpanded_containers,
    )
    .map_err(Into::into)
}

fn load_workspace_with_preexpanded_containers(
    filesystem: &mut VirtualFileSystem,
    inputs: &[assetripper::io::dependency::DependencyFilePath],
    registered_files: &[assetripper::io::dependency::DependencyFilePath],
    data_paths: &[String],
    limits: WorkspaceLoadLimits,
    preexpanded_containers: &[PreExpandedContainer],
) -> Result<
    (SerializedFileWorkspace, SerializedFileDependencyReport),
    Box<assetripper::serialized_file::workspace::SerializedFileWorkspaceError>,
> {
    SerializedFileWorkspace::load_from_vfs_with_dependencies_and_layout_source(
        filesystem,
        inputs,
        registered_files,
        SerializedFileWorkspaceLoadOptions {
            data_paths,
            limits: limits.serialized_files,
            index_limits: limits.dependency_index,
            container_limits: limits.containers,
            preexpanded_containers,
            layouts: active_class_layout_source(),
        },
    )
}

fn take_preexpanded_containers(
    mount_report: &mut BatchMountReport,
) -> Result<Vec<PreExpandedContainer>, io::Error> {
    let trees = std::mem::take(&mut mount_report.container_trees);
    let mut preexpanded = Vec::new();
    preexpanded
        .try_reserve_exact(trees.len())
        .map_err(|_| invalid_data("pre-expanded container list allocation failed"))?;
    for tree in trees {
        let tree = std::sync::Arc::try_unwrap(tree).unwrap_or_else(|shared| (*shared).clone());
        let serialized_files_loaded = tree.serialized_files.len();
        preexpanded.push(PreExpandedContainer {
            virtual_path: tree.virtual_path,
            serialized_files_loaded,
            container_scopes: tree.container_scopes,
            container_bundles: tree.container_bundles,
            serialized_files: tree.serialized_files,
            resource_files: tree.resource_files,
            failed_entries: tree.failed_entries,
        });
    }
    Ok(preexpanded)
}

#[cfg(test)]
fn load_workspace_from_vfs(
    filesystem: &VirtualFileSystem,
    inputs: &[assetripper::io::dependency::DependencyFilePath],
    limits: SerializedFileWorkspaceLimits,
) -> Result<
    SerializedFileWorkspace,
    Box<assetripper::serialized_file::workspace::SerializedFileWorkspaceError>,
> {
    SerializedFileWorkspace::load_from_vfs_with_layout_source(
        filesystem,
        inputs,
        limits,
        active_class_layout_source(),
    )
}

fn load_object_workspace(
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    limits: ObjectWorkspaceLimits,
) -> Result<
    ObjectWorkspace,
    Box<assetripper::serialized_file::object_workspace::ObjectWorkspaceError>,
> {
    ObjectWorkspace::load_from_vfs_with_layout_source(
        filesystem,
        workspace,
        limits,
        active_class_layout_source(),
    )
}

fn build_workspace_scene_graph(
    filesystem: &VirtualFileSystem,
    file_index: usize,
    file: &LoadedSerializedFile,
    decoded_objects: &ObjectWorkspace,
) -> Result<SceneGraph, Box<dyn Error>> {
    let workspace_objects = decoded_objects.objects_for_file(file_index);
    let all_scene_objects_materialized = file
        .metadata
        .objects
        .iter()
        .enumerate()
        .filter(|(_, object)| matches!(workspace_object_class_id(object), Some(1 | 4 | 224 | 1001)))
        .all(|(object_index, _)| {
            workspace_objects
                .get(object_index)
                .is_some_and(|object| object.payload.is_ok())
        });
    if all_scene_objects_materialized {
        return Ok(SceneGraph::from_materialized_workspace(
            &file.metadata,
            file_index,
            workspace_objects,
            SceneGraphLimits::default(),
        )?);
    }

    // Preserve the existing file-TypeTree diagnostic path when ObjectWorkspace could not
    // materialize a scene object; a failed generated read must not erase a detectable edge.
    let maximum_bytes = usize::try_from(SerializedFileWorkspaceLimits::default().max_file_bytes)
        .unwrap_or(usize::MAX);
    let bytes = filesystem.read_all_bytes(&file.virtual_path, maximum_bytes)?;
    let parsed_file = SerializedFile::parse(&bytes)?;
    Ok(SceneGraph::build(
        &parsed_file,
        SceneGraphLimits::default(),
    )?)
}
const MAX_REFERENCE_GRAPH_DIAGNOSTICS: usize = 128;
const MAX_GENERATED_TRANSFORM_DIAGNOSTICS: usize = 128;
const MAX_STREAMED_RESOURCE_DIAGNOSTICS: usize = 128;
const MAX_TOTAL_STREAMED_RESOURCE_VALUE_NODES: usize = 50_000_000;
const MAX_TOTAL_STREAMED_RESOURCE_CANDIDATES: usize = 1_000_000;
const MAX_TOTAL_STREAMED_RESOURCE_PATH_BYTES: usize = 64 * 1024 * 1024;

struct StreamedResourceSummaryContext<'a> {
    root_catalog: &'a RootResourceFileCatalog,
    provider: PlatformResourceProvider<'a>,
}

struct StreamedSceneBundleWorkItem {
    owner_file_index: usize,
    object_index: usize,
    path_id: i64,
    source_collection_order: usize,
    metadata: StreamedSceneBundleMetadata,
}

struct StreamedScenePathAssignment {
    path: String,
    source: SceneBundlePathMappingSource,
    asset_bundle_file_index: usize,
    asset_bundle_path_id: i64,
}

fn main() {
    if let Err(error) = run(std::env::args_os().skip(1)) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

/// The top-level CLI command selected by the first argument.
///
/// The baseline product surface is the GUI/Web UI rather than a command line, so these
/// command names are this CLI's own contract; the classifier pins the exact accepted
/// spellings (including `-h`/`help` and `-V`) so dispatch and usage cannot drift apart
/// silently. `serve` selects the baseline's own entry (the browser interface); a bare
/// invocation and a leading unrecognized option select the same entry, so the product's
/// launch behaviour matches the frozen application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CliCommand {
    Help,
    Version,
    Serve,
    ExportObjectYaml,
    ExportFileYaml,
    ExportTexture,
    ExportMesh,
    ExportShader,
    ExportAudio,
    ExportProject,
    Inspect,
    InspectFiles,
    SaveDefaultSettings,
    DeleteDefaultSettings,
    Unknown,
}

/// Map the first command-line word to its [`CliCommand`]. Matching is exact and case
/// sensitive: `Inspect`, `-v`, and `--he` are all [`CliCommand::Unknown`].
fn classify_cli_command(name: &str) -> CliCommand {
    match name {
        "--help" | "-h" | "help" => CliCommand::Help,
        "--version" | "-V" => CliCommand::Version,
        "serve" => CliCommand::Serve,
        "export-object-yaml" => CliCommand::ExportObjectYaml,
        "export-file-yaml" => CliCommand::ExportFileYaml,
        "export-texture" => CliCommand::ExportTexture,
        "export-mesh" => CliCommand::ExportMesh,
        "export-shader" => CliCommand::ExportShader,
        "export-audio" => CliCommand::ExportAudio,
        "export-project" => CliCommand::ExportProject,
        "inspect" => CliCommand::Inspect,
        "inspect-files" => CliCommand::InspectFiles,
        "save-default-settings" => CliCommand::SaveDefaultSettings,
        "delete-default-settings" => CliCommand::DeleteDefaultSettings,
        _ => CliCommand::Unknown,
    }
}

/// Whether the first command-line word selects the baseline's browser entry.
///
/// No word at all, the port's `serve` spelling, and any leading option the CLI does not define
/// (`--port 5000`, `--headless`, `-?`) all start the server, which is how the frozen
/// application behaves. The CLI's own `--help`/`--version` spellings keep their CLI meaning, so
/// `serve --help` and `serve --version` reach the browser entry's help and version text.
fn selects_web_entry(first: Option<&str>) -> bool {
    let Some(text) = first else {
        return true;
    };
    match classify_cli_command(text) {
        CliCommand::Serve => true,
        CliCommand::Help | CliCommand::Version => false,
        _ => text.starts_with('-'),
    }
}

fn run(arguments: impl Iterator<Item = OsString>) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    let first = arguments.next();
    let first_text = match &first {
        Some(word) => Some(
            word.to_str()
                .ok_or_else(|| invalid_input("the command name must be valid UTF-8"))?
                .to_owned(),
        ),
        None => None,
    };
    if selects_web_entry(first_text.as_deref()) {
        let mut launch_arguments: Vec<OsString> = Vec::new();
        if let Some(word) = first_text.filter(|word| word != "serve") {
            launch_arguments.push(OsString::from(word));
        }
        launch_arguments.extend(arguments);
        return web::run(&launch_arguments);
    }
    let command = first_text.expect("a first word was classified above");
    match classify_cli_command(&command) {
        CliCommand::Help => {
            print_usage();
            Ok(())
        }
        CliCommand::Version => {
            println!("assetripper-rs {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CliCommand::Serve => {
            let launch_arguments: Vec<OsString> = arguments.collect();
            web::run(&launch_arguments)
        }
        CliCommand::ExportObjectYaml => export_object_yaml_from_arguments(arguments),
        CliCommand::ExportFileYaml => export_file_yaml_from_arguments(arguments),
        CliCommand::ExportTexture => export_texture_from_arguments(arguments),
        CliCommand::ExportMesh => export_mesh_from_arguments(arguments),
        CliCommand::ExportShader => export_shader_from_arguments(arguments),
        CliCommand::ExportAudio => export_audio_from_arguments(arguments),
        CliCommand::ExportProject => export_project_from_arguments(arguments),
        CliCommand::Inspect | CliCommand::InspectFiles => {
            let mut input_paths = Vec::new();
            for argument in arguments {
                if input_paths.len() >= MAX_INPUT_PATHS {
                    return Err(invalid_input(format!(
                        "input path count exceeds {MAX_INPUT_PATHS}"
                    )));
                }
                input_paths.push(PathBuf::from(argument));
            }
            if input_paths.is_empty() {
                print_usage();
                return Err(invalid_input(format!(
                    "{command} requires at least one input"
                )));
            }

            if command == "inspect-files" {
                inspect_explicit_files(&input_paths)
            } else {
                inspect_directories(&input_paths)
            }
        }
        CliCommand::SaveDefaultSettings => save_default_settings_from_arguments(arguments),
        CliCommand::DeleteDefaultSettings => delete_default_settings_from_arguments(arguments),
        CliCommand::Unknown => {
            print_usage();
            Err(invalid_input(format!("unknown command '{command}'")))
        }
    }
}

#[path = "main_cli_export.rs"]
mod export_commands;
use export_commands::*;
#[path = "main_cli_inspection.rs"]
mod inspection;
#[path = "main_cli_primary_content.rs"]
mod primary_content_commands;
use inspection::*;
#[path = "main_cli_summary.rs"]
mod summary;
use summary::*;
#[path = "web/mod.rs"]
mod web;

#[cfg(test)]
#[path = "test_fixtures.rs"]
mod test_fixtures;

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
