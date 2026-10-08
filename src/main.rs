use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use assetripper::export::audio_export::try_decode_audio;
use assetripper::export::audio_export::{
    converts_ogg_to_wav, plan_audio_clip_collection, AudioDecodeFact,
};
use assetripper::export::pointer_map::{
    ExportCollectionPointerMetadata, ExportPointerMapLimits, ExportedAssetPointerEntry,
    ProjectExportPointerMap, UnityProjectAssetType,
};
use assetripper::export::project_workflow::{
    export_workspace_yaml_project_with_settings, WorkspaceProjectExportLimits,
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
    workspace_object_to_unity_yaml_document_with_streamed_resources,
    UnityYamlWorkspaceCollectionLimits, WorkspaceStreamedYamlResourceContext,
};
use assetripper::io::binary::Endian;
use assetripper::io::dependency::DependencyIndexLimits;
use assetripper::io::filesystem::VirtualFileSystem;
use assetripper::io::game_structure::{MixedGameStructure, MixedGameStructureLimits};
use assetripper::io::platform_file_inventory::{
    PlatformFileInventoryLimits, PlatformFileInventoryOptions, PlatformWorkspaceLoadOptions,
};
use assetripper::io::platform_structure::{
    detect_platform_structure, PlatformResourceProvider, PlatformStructureLimits,
};
use assetripper::io::stream::logical_file_path;
use assetripper::processing::asset_bundle::get_asset_bundle_name;
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
    resolve_and_read_streamed_resource_info, resolve_streamed_resource_info,
    scan_streamed_resource_infos, ResolvedResourceFile, RootResourceCatalogLimits,
    RootResourceFileCatalog, StreamedResourceContentLimits, StreamedResourceContentResolution,
    StreamedResourceInfo, StreamedResourceResolution, StreamedResourceScanLimits,
};
use assetripper::resources::audio::{AudioClipView, AudioDataSource, AUDIO_CLIP_CLASS_ID};
use assetripper::resources::glb::build_glb;
use assetripper::resources::image_export::ImageExportFormat;
use assetripper::resources::mesh::MeshView;
use assetripper::resources::packed_bit_vector::PackedBitVectorLimits;
use assetripper::resources::shader::{ShaderView, SHADER_CLASS_ID};
use assetripper::resources::texture::{
    export_cubemap_array_image, export_texture2d_array_image, export_texture2d_image,
    export_texture3d_image, Texture2DView, CUBEMAP_CLASS_ID,
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
    MultiFileReferenceGraphLimits,
};
use assetripper::serialized_file::workspace::{
    SerializedFileDependencyReport, SerializedFileWorkspace, SerializedFileWorkspaceLimits,
    WorkspaceSerializedBundle,
};

const MAX_INPUT_PATHS: usize = 100_000;
const MAX_STREAMED_SCENE_BUNDLE_OBJECTS: usize = 1_000_000;
const MAX_TOTAL_STREAMED_SCENE_BUNDLE_ENTRIES: usize = 1_000_000;
const MAX_TOTAL_STREAMED_SCENE_BUNDLE_STRING_BYTES: usize = 256 * 1024 * 1024;
const MAX_TOTAL_STREAMED_SCENE_PATH_MAPPINGS: usize = 1_000_000;
const MAX_OBJECT_DECODE_DIAGNOSTICS: usize = 128;
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
/// silently. Servant commands needing a web server stay out of scope: there is no
/// `serve` arm because no server ships in this binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CliCommand {
    Help,
    Version,
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

fn run(arguments: impl Iterator<Item = OsString>) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    let Some(command) = arguments.next() else {
        print_usage();
        return Err(invalid_input("a command is required"));
    };
    let command = command
        .into_string()
        .map_err(|_| invalid_input("the command name must be valid UTF-8"))?;
    match classify_cli_command(&command) {
        CliCommand::Help => {
            print_usage();
            Ok(())
        }
        CliCommand::Version => {
            println!("assetripper-rs {}", env!("CARGO_PKG_VERSION"));
            Ok(())
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

fn export_file_yaml_from_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    let serialized_file_index =
        parse_required_argument::<usize>(arguments.next(), "SerializedFile index")?;
    let output_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_input("export-file-yaml requires an output YAML path"))?;
    let mut input_files = Vec::new();
    for argument in arguments {
        if input_files.len() >= MAX_INPUT_PATHS {
            return Err(invalid_input(format!(
                "input path count exceeds {MAX_INPUT_PATHS}"
            )));
        }
        input_files.push(PathBuf::from(argument));
    }
    if input_files.is_empty() {
        return Err(invalid_input(
            "export-file-yaml requires at least one explicitly selected input file",
        ));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    export_file_yaml_to(
        &input_files,
        serialized_file_index,
        &output_path,
        &mut output,
    )
}

fn export_object_yaml_from_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    let serialized_file_index =
        parse_required_argument::<usize>(arguments.next(), "SerializedFile index")?;
    let path_id = parse_required_argument::<i64>(arguments.next(), "object PathID")?;
    let output_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_input("export-object-yaml requires an output YAML path"))?;
    let mut input_files = Vec::new();
    for argument in arguments {
        if input_files.len() >= MAX_INPUT_PATHS {
            return Err(invalid_input(format!(
                "input path count exceeds {MAX_INPUT_PATHS}"
            )));
        }
        input_files.push(PathBuf::from(argument));
    }
    if input_files.is_empty() {
        return Err(invalid_input(
            "export-object-yaml requires at least one explicitly selected input file",
        ));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    export_object_yaml_to(
        &input_files,
        serialized_file_index,
        path_id,
        &output_path,
        &mut output,
    )
}

fn export_texture_from_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    // The configured fallback defaults to `ExportSettings.ImageExportFormat` (`Png`),
    // with original-extension preference defaulting to true.
    let mut configured = ExportDefaults::default().image_export_format;
    let mut prefer_original = ExportDefaults::default().prefer_original_texture_extension;
    // Optional leading flags precede the positional arguments.
    let mut positional: Vec<OsString> = Vec::new();
    let mut explicit_settings = false;
    while let Some(next) = arguments.next() {
        if next == "--settings" {
            let path = arguments.next().ok_or_else(|| {
                invalid_input("export-texture --settings requires a settings file path")
            })?;
            let export = load_cli_export_settings(Path::new(&path))?;
            configured = export.image_export_format;
            prefer_original = export.prefer_original_texture_extension;
            explicit_settings = true;
        } else {
            positional.push(next);
        }
    }
    // An explicit flag beats the default-path file; the default-path file beats
    // built-in defaults, mirroring the startup `LoadFromDefaultPath` step.
    if !explicit_settings {
        if let Some(export) = try_load_default_export_settings()? {
            configured = export.image_export_format;
            prefer_original = export.prefer_original_texture_extension;
        }
    }
    let mut positional = positional.into_iter();
    let serialized_file_index =
        parse_required_argument::<usize>(positional.next(), "SerializedFile index")?;
    let path_id = parse_required_argument::<i64>(positional.next(), "object PathID")?;
    let output_path = positional
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_input("export-texture requires an output image path"))?;
    let mut input_files = Vec::new();
    for argument in positional {
        if input_files.len() >= MAX_INPUT_PATHS {
            return Err(invalid_input(format!(
                "input path count exceeds {MAX_INPUT_PATHS}"
            )));
        }
        input_files.push(PathBuf::from(argument));
    }
    if input_files.is_empty() {
        return Err(invalid_input(
            "export-texture requires at least one explicitly selected input file",
        ));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    export_texture_to(
        &input_files,
        serialized_file_index,
        path_id,
        configured,
        prefer_original,
        &output_path,
        &mut output,
    )
}

fn export_texture_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    path_id: i64,
    configured: SettingsImageFormat,
    prefer_original_texture_extension: bool,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mount_report) = mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    let root_resources = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = SerializedFileWorkspace::load_from_vfs_with_dependencies(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        SerializedFileWorkspaceLimits::default(),
        DependencyIndexLimits::default(),
        ContainerMountLimits::default(),
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let file = workspace
        .files()
        .get(serialized_file_index)
        .ok_or_else(|| {
            invalid_input(format!(
                "SerializedFile index {serialized_file_index} is out of range for {} loaded files",
                workspace.files().len()
            ))
        })?;
    let container_bundle_index = file.container_bundle_index;
    let mut matching_objects = file
        .metadata
        .objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.path_id == path_id);
    let (object_index, _) = matching_objects.next().ok_or_else(|| {
        invalid_input(format!(
            "SerializedFile {serialized_file_index} has no object with PathID {path_id}"
        ))
    })?;
    if matching_objects.next().is_some() {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} has multiple objects with PathID {path_id}"
        ))
        .into());
    }

    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let object = objects
        .get(serialized_file_index, object_index)
        .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
    let class_id = object.class_id.map(i32::from);
    let payload = object.payload.as_ref().map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} could not be decoded: {error}"
        ))
    })?;
    enum TextureExportTarget {
        Texture2D { view: Texture2DView, flip_y: bool },
        Texture3D { view: Texture3DView },
        Texture2DArray { view: Texture2DArrayView },
        CubemapArray { view: CubemapArrayView },
    }
    let target = match class_id {
        Some(28) | Some(CUBEMAP_CLASS_ID) => {
            let view = Texture2DView::from_value(&payload.root).map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable Texture2D: {error}"
                ))
            })?;
            // Cubemaps (class 89) share the `ITexture2D` fields but skip the
            // vertical flip (`texture is not ICubemap`).
            let flip_y = !matches!(class_id, Some(CUBEMAP_CLASS_ID));
            TextureExportTarget::Texture2D { view, flip_y }
        }
        Some(TEXTURE3D_CLASS_ID) => {
            let view = Texture3DView::from_value(&payload.root).map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable Texture3D: {error}"
                ))
            })?;
            TextureExportTarget::Texture3D { view }
        }
        Some(TEXTURE2DARRAY_CLASS_ID) => {
            let view = Texture2DArrayView::from_value(&payload.root).map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable Texture2DArray: {error}"
                ))
            })?;
            TextureExportTarget::Texture2DArray { view }
        }
        Some(CUBEMAPARRAY_CLASS_ID) => {
            let view = CubemapArrayView::from_value(&payload.root).map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable CubemapArray: {error}"
                ))
            })?;
            TextureExportTarget::CubemapArray { view }
        }
        _ => {
            return Err(invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} PathID {path_id} has class {:?}, not a class-28 Texture2D, class-89 Cubemap, class-117 Texture3D, class-187 Texture2DArray, or class-188 CubemapArray",
                object.class_id
            ))
            .into());
        }
    };
    let (image_is_empty, stream_data) = match &target {
        TextureExportTarget::Texture2D { view, .. } => {
            (view.image_data.is_empty(), view.stream_data.as_ref())
        }
        TextureExportTarget::Texture3D { view } => {
            (view.image_data.is_empty(), view.stream_data.as_ref())
        }
        TextureExportTarget::Texture2DArray { view } => {
            (view.image_data.is_empty(), view.stream_data.as_ref())
        }
        TextureExportTarget::CubemapArray { view } => {
            (view.image_data.is_empty(), view.stream_data.as_ref())
        }
    };
    let stream_bytes: Option<Vec<u8>> = match stream_data {
        Some(info) if image_is_empty && info.is_set() => {
            let provider =
                PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed.data_paths));
            let resolution = resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                container_bundle_index,
                info,
                &root_resources,
                &provider,
                StreamedResourceContentLimits::default(),
            )
            .map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} streamed resource '{}' could not be read: {error}",
                    info.path
                ))
            })?;
            match resolution {
                StreamedResourceContentResolution::Resolved { bytes, .. } => Some(bytes),
                StreamedResourceContentResolution::Missing {
                    unavailable_data_paths,
                } => {
                    return Err(invalid_data_owned(format!(
                        "SerializedFile {serialized_file_index} PathID {path_id} streamed resource '{}' is missing; unavailable data paths: {unavailable_data_paths:?}",
                        info.path
                    ))
                    .into());
                }
                StreamedResourceContentResolution::Unset => None,
            }
        }
        _ => None,
    };

    // The output extension selects the format; otherwise `GetTextureExportFormat`
    // picks the original extension's format when preferred and recognized, else the
    // configured settings format. Bundle-decoded assets carry no original path, so the
    // best extension is `None` and the fallback reduces to the configured format.
    // With default settings (`Png`) this matches the previous hardcoded fallback.
    let fallback = SettingsImageFormat::texture_export_format(
        prefer_original_texture_extension,
        None,
        configured,
    );
    let format = ImageExportFormat::from_settings_format(
        output_path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(SettingsImageFormat::from_extension)
            .unwrap_or(fallback),
    );
    // The collection platform and endianness drive the `GetImageData` transforms
    // (Xbox360 swap, Switch unswizzle); the file's own metadata prefix is the
    // honest available equivalent. Discriminant 0 matches no `BuildTarget` and
    // transforms nothing.
    // Cubemaps (class 89) share the `ITexture2D` fields but skip the vertical
    // flip (`texture is not ICubemap`); the flag travels with the target below.
    let platform = file
        .metadata
        .prefix
        .target_platform
        .and_then(|platform| i32::try_from(platform).ok())
        .unwrap_or(0);
    let platform_blob_little_endian =
        file.metadata.prefix.body_endian == assetripper::io::binary::Endian::Little;
    let image = match &target {
        TextureExportTarget::Texture2D { view, flip_y } => export_texture2d_image(
            view,
            stream_bytes.as_deref(),
            platform,
            platform_blob_little_endian,
            *flip_y,
            format,
        ),
        TextureExportTarget::Texture3D { view } => {
            export_texture3d_image(view, stream_bytes.as_deref(), format)
        }
        TextureExportTarget::Texture2DArray { view } => {
            export_texture2d_array_image(view, stream_bytes.as_deref(), format)
        }
        TextureExportTarget::CubemapArray { view } => {
            export_cubemap_array_image(view, stream_bytes.as_deref(), format)
        }
    }
    .map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} texture export failed: {error}"
        ))
    })?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(&image)?;
    writeln!(
        status_output,
        "Wrote a {} texture from SerializedFile {serialized_file_index} PathID {path_id} to {:?}.",
        format.file_extension(),
        output_path
    )?;
    Ok(())
}

fn export_mesh_from_arguments<I>(arguments: I) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = OsString>,
{
    let mut arguments = arguments;
    let serialized_file_index =
        parse_required_argument::<usize>(arguments.next(), "SerializedFile index")?;
    let path_id = parse_required_argument::<i64>(arguments.next(), "object PathID")?;
    let output_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_input("export-mesh requires an output .glb path"))?;
    let mut input_files = Vec::new();
    for argument in arguments {
        if input_files.len() >= MAX_INPUT_PATHS {
            return Err(invalid_input(format!(
                "input path count exceeds {MAX_INPUT_PATHS}"
            )));
        }
        input_files.push(PathBuf::from(argument));
    }
    if input_files.is_empty() {
        return Err(invalid_input(
            "export-mesh requires at least one explicitly selected input file",
        ));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    export_mesh_to(
        &input_files,
        serialized_file_index,
        path_id,
        &output_path,
        &mut output,
    )
}

fn export_mesh_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    path_id: i64,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, _mount_report) =
        mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = SerializedFileWorkspace::load_from_vfs_with_dependencies(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        SerializedFileWorkspaceLimits::default(),
        DependencyIndexLimits::default(),
        ContainerMountLimits::default(),
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let (object_index, _) = select_unique_object(&workspace, serialized_file_index, path_id)?;

    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let object = objects
        .get(serialized_file_index, object_index)
        .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
    if object.class_id != Some(43) {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} has class {:?}, not a class-43 Mesh",
            object.class_id
        ))
        .into());
    }
    let payload = object.payload.as_ref().map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} could not be decoded: {error}"
        ))
    })?;
    let view = MeshView::from_value(
        &payload.root,
        file_unity_version(&workspace, serialized_file_index)?,
        file_endian_type(&workspace, serialized_file_index),
    )
    .map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable Mesh: {error}"
        ))
    })?;
    let mesh_data = view
        .read_mesh_data(PackedBitVectorLimits::default())
        .map_err(|error| {
            invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} PathID {path_id} mesh data could not be read: {error}"
            ))
        })?
        .ok_or_else(|| {
            invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} PathID {path_id} has no mesh data to export"
            ))
        })?;
    let name = mesh_export_name(&payload.root)
        .unwrap_or_else(|| format!("Mesh_{serialized_file_index}_{path_id}"));
    let exported = build_glb(&name, &mesh_data).map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} GLB export failed: {error}"
        ))
    })?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(&exported.bytes)?;
    for warning in &exported.warnings {
        writeln!(status_output, "warning: {warning}")?;
    }
    let vertex_count = mesh_data.vertices.len();
    let sub_mesh_count = exported.emitted_source_indices.len();
    let emitted_indices: usize = exported.emitted_source_indices.iter().map(Vec::len).sum();
    writeln!(
        status_output,
        "Wrote a GLB with {vertex_count} vertices, {sub_mesh_count} sub-meshes, and \
         {emitted_indices} emitted indices from SerializedFile {serialized_file_index} \
         PathID {path_id} to {output_path:?}."
    )?;
    Ok(())
}

/// Loads an `AssetRipper.Settings.json` file for a `--settings` CLI flag.
///
/// Only the export record is consumed today; the remaining records stay available for
/// future commands. Missing files, invalid UTF-8, oversized files, and schema violations
/// are explicit input errors carrying the path.
fn load_cli_export_settings(path: &Path) -> Result<ExportSettingsFile, Box<dyn Error>> {
    read_serialized_settings_file(path).map(|settings| settings.export)
}

/// Reads one `AssetRipper.Settings.json` file, shared by the explicit `--settings` flag
/// and the default-path fallback. Every failure mode names the file.
fn read_export_settings_file(path: &Path) -> Result<ExportSettingsFile, Box<dyn Error>> {
    read_serialized_settings_file(path).map(|settings| settings.export)
}

/// `SerializedSettings.TryLoadFromDefaultPath` for one caller-supplied directory: an
/// absent file yields `None` (the caller keeps built-in defaults), while a present file
/// loads or fails with a path-carrying error, mirroring the source throw on corrupt
/// content. The directory is a parameter so tests stay hermetic; product code passes
/// the executable directory (see `try_load_default_export_settings`).
fn load_default_export_settings_from_dir(
    directory: &Path,
) -> Result<Option<ExportSettingsFile>, Box<dyn Error>> {
    use assetripper::export::asset_path::HostPathPlatform;
    use assetripper::export::settings_file::default_settings_file_path;

    let platform = if cfg!(windows) {
        HostPathPlatform::Windows
    } else {
        HostPathPlatform::Unix
    };
    let directory_text = directory
        .to_str()
        .ok_or_else(|| invalid_input("the executable directory is not valid UTF-8"))?;
    let path = PathBuf::from(default_settings_file_path(directory_text, platform));
    if !path.exists() {
        return Ok(None);
    }
    read_export_settings_file(&path).map(Some)
}

/// Resolve the executable directory (`AppContext.BaseDirectory` in the source).
/// The process query stays in product code; tests pass directories explicitly.
fn executable_directory() -> Result<PathBuf, Box<dyn Error>> {
    let executable = std::env::current_exe()
        .map_err(|source| invalid_input(format!("could not locate the executable: {source}")))?;
    executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| invalid_input("the executable path has no parent directory"))
}

/// `SerializedSettings.SaveToDefaultPath` for one caller-supplied directory: the
/// record is rendered byte-exactly (same writer as the load path) and overwritten
/// unconditionally, mirroring `File.Create`. Every failure mode names the file.
/// Returns the written path. The directory is a parameter so tests stay hermetic;
/// product code passes the executable directory.
fn save_serialized_settings_to_dir(
    settings: &assetripper::export::settings_file::SerializedSettingsFile,
    directory: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    use assetripper::export::asset_path::HostPathPlatform;
    use assetripper::export::settings_file::{default_settings_file_path, write_settings_file};

    let platform = if cfg!(windows) {
        HostPathPlatform::Windows
    } else {
        HostPathPlatform::Unix
    };
    let directory_text = directory
        .to_str()
        .ok_or_else(|| invalid_input("the executable directory is not valid UTF-8"))?;
    let path = PathBuf::from(default_settings_file_path(directory_text, platform));
    let text = write_settings_file(settings).map_err(|error| {
        invalid_input(format!(
            "settings for {} could not be rendered: {error}",
            path.display()
        ))
    })?;
    fs::write(&path, text).map_err(|source| {
        invalid_input(format!(
            "could not write settings file {}: {source}",
            path.display()
        ))
    })?;
    Ok(path)
}

/// `SerializedSettings.DeleteDefaultPath` for one caller-supplied directory: an
/// absent file is a success without effect (mirroring the source existence check),
/// a present file is removed. Returns whether a file was removed. Every failure
/// mode names the file.
fn delete_serialized_settings_in_dir(directory: &Path) -> Result<bool, Box<dyn Error>> {
    use assetripper::export::asset_path::HostPathPlatform;
    use assetripper::export::settings_file::default_settings_file_path;

    let platform = if cfg!(windows) {
        HostPathPlatform::Windows
    } else {
        HostPathPlatform::Unix
    };
    let directory_text = directory
        .to_str()
        .ok_or_else(|| invalid_input("the executable directory is not valid UTF-8"))?;
    let path = PathBuf::from(default_settings_file_path(directory_text, platform));
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(&path).map_err(|source| {
        invalid_input(format!(
            "could not delete settings file {}: {source}",
            path.display()
        ))
    })?;
    Ok(true)
}

/// Read one full `AssetRipper.Settings.json` record, shared by `--settings` inputs
/// that need more than the export section. Every failure mode names the file.
fn read_serialized_settings_file(
    path: &Path,
) -> Result<assetripper::export::settings_file::SerializedSettingsFile, Box<dyn Error>> {
    use assetripper::export::settings_file::{parse_settings_file, SettingsFileLimits};

    let limits = SettingsFileLimits::default();
    let bytes = fs::read(path).map_err(|source| {
        invalid_input(format!(
            "could not read settings file {}: {source}",
            path.display()
        ))
    })?;
    if bytes.len() > limits.max_file_bytes {
        return Err(invalid_input(format!(
            "settings file {} uses {} bytes, exceeding limit {}",
            path.display(),
            bytes.len(),
            limits.max_file_bytes
        )));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        invalid_input(format!(
            "settings file {} is not valid UTF-8",
            path.display()
        ))
    })?;
    parse_settings_file(&text, limits).map_err(|error| {
        invalid_input(format!(
            "settings file {} could not be parsed: {error}",
            path.display()
        ))
    })
}

/// `SerializedSettings.TryLoadFromDefaultPath` plus the `LoadFromDefaultPath` startup
/// step for the CLI: settings stored next to the executable apply when the caller gave
/// no explicit `--settings` flag. `AppContext.BaseDirectory` arrives here as the
/// executable directory, keeping the process query in product code.
fn try_load_default_export_settings() -> Result<Option<ExportSettingsFile>, Box<dyn Error>> {
    let directory = executable_directory()?;
    load_default_export_settings_from_dir(&directory)
}

/// `save-default-settings [--settings <path>]`: persist settings next to the
/// executable (`SerializedSettings.SaveToDefaultPath`). With `--settings`, the
/// named file is validated and stored; without it, built-in defaults are stored.
/// The file is overwritten unconditionally, mirroring `File.Create`.
fn save_default_settings_from_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    use assetripper::export::settings::{ExportDefaults, ImportDefaults, ProcessingDefaults};
    use assetripper::export::settings_file::SerializedSettingsFile;

    let mut arguments = arguments;
    let mut source: Option<PathBuf> = None;
    while let Some(next) = arguments.next() {
        let text = next
            .into_string()
            .map_err(|_| invalid_input("the --settings value must be valid UTF-8"))?;
        if text == "--settings" {
            if source.is_some() {
                return Err(invalid_input(
                    "save-default-settings accepts --settings at most once",
                ));
            }
            let path = arguments.next().ok_or_else(|| {
                invalid_input("save-default-settings --settings requires a settings file path")
            })?;
            source = Some(PathBuf::from(path));
        } else {
            return Err(invalid_input(format!(
                "unexpected save-default-settings argument '{text}'; expected --settings <path>"
            )));
        }
    }
    let settings = match source {
        Some(path) => read_serialized_settings_file(&path)?,
        None => SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        ),
    };
    let directory = executable_directory()?;
    let path = save_serialized_settings_to_dir(&settings, &directory)?;
    println!("saved settings to {}", path.display());
    Ok(())
}

/// `delete-default-settings`: remove the executable-adjacent settings file
/// (`SerializedSettings.DeleteDefaultPath`). An absent file is a success without
/// effect, mirroring the source existence check.
fn delete_default_settings_from_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    if let Some(next) = arguments.next() {
        let text = next
            .into_string()
            .unwrap_or_else(|_| "<invalid UTF-8>".to_owned());
        return Err(invalid_input(format!(
            "unexpected delete-default-settings argument '{text}'; this command takes no arguments"
        )));
    }
    let directory = executable_directory()?;
    // Recompute the display path through the same helper so the report cannot drift.
    let removed = delete_serialized_settings_in_dir(&directory)?;
    if removed {
        println!("deleted the default settings file");
    } else {
        println!("no default settings file exists");
    }
    Ok(())
}

/// Parses the optional leading `--shader-export-mode <dummy|yaml|decompile>` flag of
/// `export-shader`; the default matches `ExportSettings.ShaderExportMode` (`Dummy`).
fn parse_shader_export_mode(
    argument: Option<OsString>,
) -> Result<ShaderExportMode, Box<dyn Error>> {
    let Some(argument) = argument else {
        return Ok(ShaderExportMode::Dummy);
    };
    let text = argument
        .into_string()
        .map_err(|_| invalid_input("the --shader-export-mode value must be valid UTF-8"))?;
    match text.as_str() {
        "dummy" => Ok(ShaderExportMode::Dummy),
        "yaml" => Ok(ShaderExportMode::Yaml),
        "decompile" => Ok(ShaderExportMode::Decompile),
        _ => Err(invalid_input(format!(
            "unknown --shader-export-mode '{text}'; expected dummy, yaml, or decompile"
        ))),
    }
}

fn export_shader_from_arguments<I>(arguments: I) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = OsString>,
{
    let mut arguments = arguments;
    let mut flag_mode: Option<ShaderExportMode> = None;
    // The settings-file fallback defaults to the `ExportSettings` default (`Dummy`).
    let mut settings_mode = ShaderExportMode::Dummy;
    let mut explicit_settings = false;
    // Optional leading flags precede the positional arguments.
    loop {
        let Some(next) = arguments.next() else {
            return Err(invalid_input(
                "export-shader requires a SerializedFile index",
            ));
        };
        if next == "--shader-export-mode" {
            flag_mode = Some(parse_shader_export_mode(arguments.next())?);
        } else if next == "--settings" {
            let path = arguments.next().ok_or_else(|| {
                invalid_input("export-shader --settings requires a settings file path")
            })?;
            settings_mode = load_cli_export_settings(Path::new(&path))?.shader_export_mode;
            explicit_settings = true;
        } else {
            let serialized_file_index = next
                .into_string()
                .ok()
                .and_then(|text| text.parse::<usize>().ok())
                .ok_or_else(|| invalid_input("export-shader requires a SerializedFile index"))?;
            let path_id = parse_required_argument::<i64>(arguments.next(), "object PathID")?;
            let output_path = arguments
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| invalid_input("export-shader requires an output .shader path"))?;
            let mut input_files = Vec::new();
            for argument in arguments {
                if input_files.len() >= MAX_INPUT_PATHS {
                    return Err(invalid_input(format!(
                        "input path count exceeds {MAX_INPUT_PATHS}"
                    )));
                }
                input_files.push(PathBuf::from(argument));
            }
            if input_files.is_empty() {
                return Err(invalid_input(
                    "export-shader requires at least one explicitly selected input file",
                ));
            }
            let stdout = io::stdout();
            let mut output = stdout.lock();
            // An explicit flag beats the settings file on any order; the
            // default-path file fills the settings fallback when no `--settings`
            // flag was given.
            if !explicit_settings {
                if let Some(export) = try_load_default_export_settings()? {
                    settings_mode = export.shader_export_mode;
                }
            }
            let mode = flag_mode.unwrap_or(settings_mode);
            return export_shader_to(
                &input_files,
                serialized_file_index,
                path_id,
                mode,
                &output_path,
                &mut output,
            );
        }
    }
}

fn export_shader_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    path_id: i64,
    mode: ShaderExportMode,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, _mount_report) =
        mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = SerializedFileWorkspace::load_from_vfs_with_dependencies(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        SerializedFileWorkspaceLimits::default(),
        DependencyIndexLimits::default(),
        ContainerMountLimits::default(),
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let (object_index, _) = select_unique_object(&workspace, serialized_file_index, path_id)?;

    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let object = objects
        .get(serialized_file_index, object_index)
        .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
    if object.class_id != Some(SHADER_CLASS_ID as i16) {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} has class {:?}, not a class-48 Shader",
            object.class_id
        ))
        .into());
    }
    let payload = object.payload.as_ref().map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} could not be decoded: {error}"
        ))
    })?;
    let view = ShaderView::from_value(&payload.root).map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable Shader: {error}"
        ))
    })?;
    let text = export_shader_text(&view, mode).map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} shader export failed: {error}"
        ))
    })?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(&text)?;
    writeln!(
        status_output,
        "Wrote a shader from SerializedFile {serialized_file_index} PathID {path_id} to {output_path:?}."
    )?;
    Ok(())
}

fn export_audio_from_arguments<I>(arguments: I) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = OsString>,
{
    let mut arguments = arguments;
    // The configured format defaults to `ExportSettings.AudioExportFormat` (`Default`).
    let mut configured = ExportDefaults::default().audio_export_format;
    // Optional leading flags precede the positional arguments.
    let mut positional: Vec<OsString> = Vec::new();
    let mut explicit_settings = false;
    while let Some(next) = arguments.next() {
        if next == "--settings" {
            let path = arguments.next().ok_or_else(|| {
                invalid_input("export-audio --settings requires a settings file path")
            })?;
            configured = load_cli_export_settings(Path::new(&path))?.audio_export_format;
            explicit_settings = true;
        } else {
            positional.push(next);
        }
    }
    // An explicit flag beats the default-path file; the default-path file beats
    // built-in defaults, mirroring the startup `LoadFromDefaultPath` step.
    if !explicit_settings {
        if let Some(export) = try_load_default_export_settings()? {
            configured = export.audio_export_format;
        }
    }
    let mut positional = positional.into_iter();
    let serialized_file_index =
        parse_required_argument::<usize>(positional.next(), "SerializedFile index")?;
    let path_id = parse_required_argument::<i64>(positional.next(), "object PathID")?;
    let output_path = positional
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_input("export-audio requires an output audio path"))?;
    let mut input_files = Vec::new();
    for argument in positional {
        if input_files.len() >= MAX_INPUT_PATHS {
            return Err(invalid_input(format!(
                "input path count exceeds {MAX_INPUT_PATHS}"
            )));
        }
        input_files.push(PathBuf::from(argument));
    }
    if input_files.is_empty() {
        return Err(invalid_input(
            "export-audio requires at least one explicitly selected input file",
        ));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    export_audio_to(
        &input_files,
        serialized_file_index,
        path_id,
        configured,
        &output_path,
        &mut output,
    )
}

fn export_project_from_arguments<I>(arguments: I) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = OsString>,
{
    let mut output_root: Option<PathBuf> = None;
    let mut delete_existing = false;
    let mut settings_path: Option<PathBuf> = None;
    let mut inputs = Vec::new();
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        if argument == "--delete-existing" {
            delete_existing = true;
            continue;
        }
        if argument == "--settings" {
            let value = arguments
                .next()
                .ok_or_else(|| invalid_input("--settings requires a settings file path"))?;
            settings_path = Some(PathBuf::from(value));
            continue;
        }
        match &output_root {
            None => output_root = Some(PathBuf::from(argument)),
            Some(_) => {
                if inputs.len() >= MAX_INPUT_PATHS {
                    return Err(invalid_input(format!(
                        "input path count exceeds {MAX_INPUT_PATHS}"
                    )));
                }
                inputs.push(PathBuf::from(argument));
            }
        }
    }
    let output_root = output_root
        .ok_or_else(|| invalid_input("export-project requires a new output directory"))?;
    if inputs.is_empty() {
        return Err(invalid_input(
            "export-project requires at least one input file or directory",
        ));
    }
    let settings = match settings_path {
        Some(path) => read_export_settings_file(&path)?,
        None => try_load_default_export_settings()?
            .unwrap_or_else(|| ExportSettingsFile::from(ExportDefaults::default())),
    };
    let stdout = io::stdout();
    let mut status_output = stdout.lock();
    export_project_to(
        &inputs,
        &output_root,
        delete_existing,
        &settings,
        &mut status_output,
    )
}

fn export_project_to<W: io::Write>(
    input_paths: &[PathBuf],
    output_root: &Path,
    delete_existing: bool,
    settings: &ExportSettingsFile,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mount_report) = mount_project_inputs(input_paths)?;
    ensure_no_batch_container_failures(&mount_report)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected paths contain no recognized SerializedFile inputs",
        ));
    }
    let root_catalog = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    let (workspace, _) = SerializedFileWorkspace::load_from_vfs_with_dependencies(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        SerializedFileWorkspaceLimits::default(),
        DependencyIndexLimits::default(),
        ContainerMountLimits::default(),
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let reference_graph = workspace.reference_graph(MultiFileReferenceGraphLimits::default())?;
    let platform = detect_platform_structure(
        &filesystem,
        &selected_paths,
        PlatformStructureLimits::default(),
    )?;
    let provider =
        PlatformResourceProvider::with_mixed_data_paths(platform.as_ref(), Some(&mixed.data_paths));
    let report = export_workspace_yaml_project_with_settings(
        output_root,
        delete_existing,
        &filesystem,
        &root_catalog,
        &provider,
        &workspace,
        &objects,
        &reference_graph,
        settings,
        WorkspaceProjectExportLimits::default(),
    )?;
    writeln!(
        status_output,
        "Wrote a Unity project to {:?}: {} SerializedFiles, {} objects in {} asset files ({} with importer metadata; support-file count {}), {} bytes (project version {}).",
        output_root,
        report.serialized_file_count,
        report.exported_object_count,
        report.asset_file_count,
        report.meta_file_count,
        report.support_file_count,
        report.total_yaml_bytes,
        report.project_version.major
    )?;
    if !report.skipped_objects.is_empty() {
        writeln!(
            status_output,
            "{} objects claimed a non-exportable baseline collection and were not written (the baseline writes nothing for them):",
            report.skipped_objects.len()
        )?;
        for notice in report.skipped_objects.iter().take(10) {
            writeln!(status_output, "  - {}: {}", notice.subject, notice.reason)?;
        }
    }
    if !report.processing_warnings.is_empty() {
        writeln!(
            status_output,
            "{} Scene/Prefab processing warnings:",
            report.processing_warnings.len()
        )?;
        for warning in report.processing_warnings.iter().take(10) {
            writeln!(status_output, "  - {warning}")?;
        }
    }
    if !report.fallbacks.is_empty() {
        writeln!(
            status_output,
            "{} files used the generic asset-family renderer because their specialized exporter is not wired yet:",
            report.fallbacks.len()
        )?;
        for notice in report.fallbacks.iter().take(10) {
            writeln!(status_output, "  - {}: {}", notice.subject, notice.reason)?;
        }
    }
    writeln!(
        status_output,
        "Mounted {} logical inputs ({} bytes). Processed Scene and modern Prefab collection routes are wired; legacy generated Prefabs and the remaining class-specific exporters are still incomplete.",
        mount_report.discovered_inputs,
        mount_report.discovered_source_bytes
    )?;
    Ok(())
}

fn export_audio_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    path_id: i64,
    format: SettingsAudioFormat,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mount_report) = mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    let root_resources = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = SerializedFileWorkspace::load_from_vfs_with_dependencies(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        SerializedFileWorkspaceLimits::default(),
        DependencyIndexLimits::default(),
        ContainerMountLimits::default(),
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let file = workspace
        .files()
        .get(serialized_file_index)
        .ok_or_else(|| {
            invalid_input(format!(
                "SerializedFile index {serialized_file_index} is out of range for {} loaded files",
                workspace.files().len()
            ))
        })?;
    let container_bundle_index = file.container_bundle_index;
    let (object_index, _) = select_unique_object(&workspace, serialized_file_index, path_id)?;

    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let object = objects
        .get(serialized_file_index, object_index)
        .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
    if object.class_id != Some(AUDIO_CLIP_CLASS_ID as i16) {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} has class {:?}, not a class-83 AudioClip",
            object.class_id
        ))
        .into());
    }
    let payload = object.payload.as_ref().map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} could not be decoded: {error}"
        ))
    })?;
    let view = AudioClipView::from_value(&payload.root).map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable AudioClip: {error}"
        ))
    })?;
    let raw_data: Vec<u8> = match view.audio_data_source() {
        AudioDataSource::Inline(bytes) => bytes.to_vec(),
        AudioDataSource::Resource(info) => {
            if !info.is_set() {
                return Err(invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} has an unset audio resource"
                ))
                .into());
            }
            let provider =
                PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed.data_paths));
            let resolution = resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                container_bundle_index,
                info,
                &root_resources,
                &provider,
                StreamedResourceContentLimits::default(),
            )
            .map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} streamed resource '{}' could not be read: {error}",
                    info.path
                ))
            })?;
            match resolution {
                StreamedResourceContentResolution::Resolved { bytes, .. } => bytes,
                StreamedResourceContentResolution::Missing {
                    unavailable_data_paths,
                } => {
                    return Err(invalid_data_owned(format!(
                        "SerializedFile {serialized_file_index} PathID {path_id} streamed resource '{}' is missing; unavailable data paths: {unavailable_data_paths:?}",
                        info.path
                    ))
                    .into());
                }
                StreamedResourceContentResolution::Unset => {
                    return Err(invalid_data_owned(format!(
                        "SerializedFile {serialized_file_index} PathID {path_id} has an unset audio resource"
                    ))
                    .into());
                }
            }
        }
        AudioDataSource::Empty => Vec::new(),
    };
    let name = view
        .name
        .as_deref()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .unwrap_or_default();
    let (decoded, extension) =
        try_decode_audio(name, view.sound_type, &raw_data).map_err(|error| {
            invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} audio export failed: {error}"
        ))
        })?;
    // `AudioClipExporter.TryCreateCollection` picks the collection extension from the
    // decode outcome plus the configured `AudioExportFormat`; only `PreferWav` meeting
    // an `ogg` decode changes anything. The class guard above already established the
    // clip, and a successful decode is never empty.
    let collection_extension = plan_audio_clip_collection(AudioDecodeFact {
        is_audio_clip: true,
        decode_succeeded: true,
        decoded_is_empty: decoded.is_empty(),
        decoded_extension: extension,
        format,
    })
    .map_err(|refusal| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} audio export refused: {refusal:?}"
        ))
    })?;
    if converts_ogg_to_wav(extension, &collection_extension) {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} requires an Ogg-to-WAV conversion, which needs a Vorbis decoder that is not available"
        ))
        .into());
    }
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(&decoded)?;
    writeln!(
        status_output,
        "Wrote a {collection_extension} audio from SerializedFile {serialized_file_index} PathID {path_id} to {output_path:?}."
    )?;
    Ok(())
}

/// `Mesh.Name`, when the payload carries a readable UTF-8 name.
fn mesh_export_name(root: &SerializedValue) -> Option<String> {
    root.unique_field_in_base_chain("m_Name")
        .ok()
        .flatten()
        .and_then(|value| match value {
            SerializedValue::StringBytes(bytes) => {
                std::str::from_utf8(bytes).ok().map(str::to_owned)
            }
            SerializedValue::Bytes(bytes) => std::str::from_utf8(bytes).ok().map(str::to_owned),
            _ => None,
        })
        .filter(|name| !name.is_empty())
}

/// Finds the single object with `path_id`, rejecting a missing or duplicated PathID.
fn select_unique_object(
    workspace: &SerializedFileWorkspace,
    serialized_file_index: usize,
    path_id: i64,
) -> Result<(usize, &ObjectInfo), Box<dyn Error>> {
    let file = workspace
        .files()
        .get(serialized_file_index)
        .ok_or_else(|| {
            invalid_input(format!(
                "SerializedFile index {serialized_file_index} is out of range for {} loaded files",
                workspace.files().len()
            ))
        })?;
    let mut matching = file
        .metadata
        .objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.path_id == path_id);
    let found = matching.next().ok_or_else(|| {
        invalid_input(format!(
            "SerializedFile {serialized_file_index} has no object with PathID {path_id}"
        ))
    })?;
    if matching.next().is_some() {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} has multiple objects with PathID {path_id}"
        ))
        .into());
    }
    Ok(found)
}

/// The numeric Unity version used by the vertex-data branches.
///
/// An absent or unparseable version string is reported rather than silently defaulted, because the
/// channel and stream layouts differ across versions.
fn file_unity_version(
    workspace: &SerializedFileWorkspace,
    serialized_file_index: usize,
) -> Result<UnityVersion, Box<dyn Error>> {
    let text = workspace
        .files()
        .get(serialized_file_index)
        .and_then(|file| file.metadata.prefix.unity_version_text.as_deref())
        .ok_or_else(|| {
            invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} has no Unity version string, so its mesh \
                 channel layout cannot be resolved"
            ))
        })?;
    UnityVersion::parse(text).ok_or_else(|| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} Unity version '{text}' is unparseable, so its \
             mesh channel layout cannot be resolved"
        ))
        .into()
    })
}

/// The file's body byte order.
///
/// Pre-generation-9 files keep the flag in the metadata; a file whose header does not declare one
/// is little endian, matching `EndianBinaryReader`'s default.
fn file_endian_type(
    workspace: &SerializedFileWorkspace,
    serialized_file_index: usize,
) -> EndianType {
    match workspace
        .files()
        .get(serialized_file_index)
        .and_then(|file| file.header.data_endian)
    {
        Some(Endian::Big) => EndianType::Big,
        Some(Endian::Little) | None => EndianType::Little,
    }
}

fn parse_required_argument<T>(
    argument: Option<OsString>,
    description: &'static str,
) -> Result<T, Box<dyn Error>>
where
    T: std::str::FromStr,
{
    let argument = argument
        .ok_or_else(|| invalid_input(format!("export-object-yaml requires {description}")))?;
    let argument = argument
        .into_string()
        .map_err(|_| invalid_input(format!("{description} must be valid UTF-8")))?;
    argument
        .parse()
        .map_err(|_| invalid_input(format!("invalid {description} '{argument}'")))
}

fn export_object_yaml_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    path_id: i64,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mount_report) = mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    let root_resources = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = SerializedFileWorkspace::load_from_vfs_with_dependencies(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        SerializedFileWorkspaceLimits::default(),
        DependencyIndexLimits::default(),
        ContainerMountLimits::default(),
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let file = workspace
        .files()
        .get(serialized_file_index)
        .ok_or_else(|| {
            invalid_input(format!(
                "SerializedFile index {serialized_file_index} is out of range for {} loaded files",
                workspace.files().len()
            ))
        })?;
    let mut matching_objects = file
        .metadata
        .objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.path_id == path_id);
    let (object_index, _) = matching_objects.next().ok_or_else(|| {
        invalid_input(format!(
            "SerializedFile {serialized_file_index} has no object with PathID {path_id}"
        ))
    })?;
    if matching_objects.next().is_some() {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} has multiple objects with PathID {path_id}"
        ))
        .into());
    }

    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let object = objects
        .get(serialized_file_index, object_index)
        .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
    let class_id = object
        .class_id
        .map(i32::from)
        .ok_or_else(|| invalid_data("selected object has no class ID"))?;
    let source = ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: serialized_file_index,
        },
        object_index,
        path_id,
        class_id,
    };
    let const_collection_index = 0;
    let const_file_id = 1;
    let pointer_map = ProjectExportPointerMap::new(
        &[ExportedAssetPointerEntry {
            asset: source,
            file_id: const_file_id,
            collection_index: const_collection_index,
        }],
        &[ExportCollectionPointerMetadata {
            collection_index: const_collection_index,
            // This one-object YAML probe writes no Unity .meta file or foreign collection GUID.
            guid: [0; 16],
            asset_type: UnityProjectAssetType::Serialized,
        }],
        ExportPointerMapLimits::default(),
    )?;
    let reference_graph = workspace.reference_graph(MultiFileReferenceGraphLimits::default())?;
    let provider = PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed.data_paths));
    let resource_context = WorkspaceStreamedYamlResourceContext {
        filesystem: &filesystem,
        root_catalog: &root_resources,
        provider: &provider,
        content_limits: StreamedResourceContentLimits::default(),
        staging_limits: StreamedYamlAssetLimits::default(),
    };
    let Some(document) = workspace_object_to_unity_yaml_document_with_streamed_resources(
        &workspace,
        &reference_graph,
        &pointer_map,
        object,
        source,
        const_file_id,
        const_collection_index,
        None,
        UnityYamlObjectConversionLimits::default(),
        &resource_context,
    )?
    else {
        writeln!(
            status_output,
            "Skipped SerializedFile {serialized_file_index} PathID {path_id}: the baseline YAML exporter omits image textures without usable inline or streamed data."
        )?;
        return Ok(());
    };
    let yaml = write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default())?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(yaml.as_bytes())?;
    writeln!(
        status_output,
        "Wrote one YAML object from SerializedFile {serialized_file_index} PathID {path_id} to {:?}.",
        output_path
    )?;
    Ok(())
}

fn export_file_yaml_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mount_report) = mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    let root_resources = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = SerializedFileWorkspace::load_from_vfs_with_dependencies(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        SerializedFileWorkspaceLimits::default(),
        DependencyIndexLimits::default(),
        ContainerMountLimits::default(),
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let file = workspace
        .files()
        .get(serialized_file_index)
        .ok_or_else(|| {
            invalid_input(format!(
                "SerializedFile index {serialized_file_index} is out of range for {} loaded files",
                workspace.files().len()
            ))
        })?;
    let object_count = file.metadata.objects.len();
    let collection_limits = UnityYamlWorkspaceCollectionLimits::default();
    if object_count > collection_limits.max_documents {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} has {object_count} objects, exceeding the YAML document limit {}",
            collection_limits.max_documents
        ))
        .into());
    }

    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let reference_graph = workspace.reference_graph(MultiFileReferenceGraphLimits::default())?;
    let mut assignments = Vec::new();
    assignments.try_reserve_exact(object_count).map_err(|_| {
        invalid_data_owned(format!("unable to reserve {object_count} YAML assignments"))
    })?;
    let mut pointer_entries = Vec::new();
    pointer_entries
        .try_reserve_exact(object_count)
        .map_err(|_| {
            invalid_data_owned(format!("unable to reserve {object_count} YAML pointers"))
        })?;

    for (object_index, object_info) in file.metadata.objects.iter().enumerate() {
        if object_info.path_id == 0 {
            return Err(invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} object {object_index} has PathID 0, which cannot be assigned as a Unity YAML document ID"
            ))
            .into());
        }
        if reference_graph.object_scan_is_complete(serialized_file_index, object_index)
            != Some(true)
        {
            return Err(invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} object {object_index} PathID {} has no complete TypeTree reference scan",
                object_info.path_id
            ))
            .into());
        }
        for reference in reference_graph.references_from_object(serialized_file_index, object_index)
        {
            match &reference.resolution {
                MultiFilePPtrResolution::Null => {}
                MultiFilePPtrResolution::Object {
                    target_file_index, ..
                } if *target_file_index == serialized_file_index => {}
                MultiFilePPtrResolution::Object {
                    target_file_index,
                    target_object_index,
                } => {
                    return Err(invalid_data_owned(format!(
                        "SerializedFile {serialized_file_index} object {object_index} PathID {} references object {target_object_index} in SerializedFile {target_file_index}; one-file YAML export cannot assign a foreign collection GUID",
                        object_info.path_id
                    ))
                    .into());
                }
                unresolved => {
                    return Err(invalid_data_owned(format!(
                        "SerializedFile {serialized_file_index} object {object_index} PathID {} has unresolved reference at {:?}: {unresolved:?}",
                        object_info.path_id, reference.field_path
                    ))
                    .into());
                }
            }
        }

        let object = objects
            .get(serialized_file_index, object_index)
            .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
        let class_id = object
            .class_id
            .map(i32::from)
            .ok_or_else(|| invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} object {object_index} PathID {} has no class ID",
                object_info.path_id
            )))?;
        let asset = ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile {
                file_index: serialized_file_index,
            },
            object_index,
            path_id: object_info.path_id,
            class_id,
        };
        assignments.push(SceneExportIdAssignment {
            asset,
            file_id: object_info.path_id,
        });
        pointer_entries.push(ExportedAssetPointerEntry {
            asset,
            file_id: object_info.path_id,
            collection_index: 0,
        });
    }

    let pointer_map = ProjectExportPointerMap::new(
        &pointer_entries,
        &[ExportCollectionPointerMetadata {
            collection_index: 0,
            guid: [0; 16],
            asset_type: UnityProjectAssetType::Serialized,
        }],
        ExportPointerMapLimits::default(),
    )?;
    let provider = PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed.data_paths));
    let resource_context = WorkspaceStreamedYamlResourceContext {
        filesystem: &filesystem,
        root_catalog: &root_resources,
        provider: &provider,
        content_limits: StreamedResourceContentLimits::default(),
        staging_limits: StreamedYamlAssetLimits::default(),
    };
    let yaml = workspace_assignments_to_unity_yaml_with_streamed_resources(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments,
        0,
        None,
        collection_limits,
        &resource_context,
    )?;
    let exported_document_count = yaml.match_indices("--- !u!").count();
    let omitted_object_count = object_count
        .checked_sub(exported_document_count)
        .ok_or_else(|| invalid_data("YAML exporter wrote more documents than source objects"))?;
    if object_count > 0 && exported_document_count == 0 {
        writeln!(
            status_output,
            "Skipped SerializedFile {serialized_file_index}: all {object_count} objects were omitted because the baseline YAML exporter found ImageTexture objects without usable inline or streamed data."
        )?;
        return Ok(());
    }
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(yaml.as_bytes())?;
    if omitted_object_count == 0 {
        writeln!(
            status_output,
            "Wrote {exported_document_count} ordered YAML objects from SerializedFile {serialized_file_index} to {:?}.",
            output_path
        )?;
    } else {
        writeln!(
            status_output,
            "Wrote {exported_document_count} ordered YAML objects from {object_count} objects in SerializedFile {serialized_file_index} to {:?}; the baseline YAML exporter omitted {omitted_object_count} ImageTexture objects without usable inline or streamed data.",
            output_path
        )?;
    }
    Ok(())
}

/// How a loaded selection proceeds through load-and-process.
///
/// Mirrors the `ExportHandler.LoadAndProcess` branching
/// (`AssetRipper/Source/AssetRipper.Export.UnityProjects/ExportHandler.cs`, lines 142-150)
/// together with the empty-bundle warning in the `GameStructure` constructor
/// (`AssetRipper/Source/AssetRipper.Import/Structure/GameStructure.cs`, lines 42-45),
/// decided over the caller-counted asset collections (`GameBundle.HasAnyAssetCollections()`,
/// i.e. `FetchAssetCollections().Any()`). The load and process phases themselves need the
/// VFS, collections, and processor pipeline and stay with their callers, as does the
/// `GameFileLoader.Reset` state clearing (a fresh CLI process starts empty).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoadProcessPlan {
    /// The bundle holds asset collections: quiet load, then run processing.
    Process,
    /// The bundle holds none: warn that no valid assets were found, skip
    /// processing, and return the bundle unprocessed.
    ReturnUnprocessed,
}

/// Decides whether a loaded bundle with `asset_collection_count` collections runs
/// the processing phase.
const fn plan_load_process(asset_collection_count: usize) -> LoadProcessPlan {
    if asset_collection_count > 0 {
        LoadProcessPlan::Process
    } else {
        LoadProcessPlan::ReturnUnprocessed
    }
}

/// What `inspect-files` does with its recognized inputs, over caller facts.
///
/// Serialized inputs always load the workspace. With none, explicit root resources
/// still report: baseline `GameBundle.FromPaths` keeps `ResourceFile` inputs without
/// any `SerializedFile` (`GameBundle.FromPaths.cs`, lines 49-51). With neither, the
/// selection names nothing to inspect and the command fails.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExplicitInspectPlan {
    /// No recognized inputs: fail the command.
    FailNoRecognizedInputs,
    /// Roots only: print the catalog plus the no-SerializedFile notice.
    ReportRootsOnly,
    /// Serialized inputs present: load the workspace.
    LoadWorkspace,
}

/// Selects the `inspect-files` outcome from the recognized SerializedFile and
/// explicit root resource counts.
const fn plan_explicit_inspect(
    serialized_input_count: usize,
    root_resource_count: usize,
) -> ExplicitInspectPlan {
    if serialized_input_count > 0 {
        ExplicitInspectPlan::LoadWorkspace
    } else if root_resource_count > 0 {
        ExplicitInspectPlan::ReportRootsOnly
    } else {
        ExplicitInspectPlan::FailNoRecognizedInputs
    }
}

fn inspect_explicit_files(input_files: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    inspect_explicit_files_to(input_files, &mut output)
}

fn inspect_explicit_files_to<W: io::Write>(
    input_files: &[PathBuf],
    output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mount_report) = mount_input_files(input_files)?;
    print_batch_container_failures_to(output, &mount_report)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    let root_resources = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    print_explicit_root_resource_catalog_to(output, &root_resources)?;

    match plan_explicit_inspect(
        mixed.serialized_file_inputs.len(),
        root_resources.files().len(),
    ) {
        ExplicitInspectPlan::FailNoRecognizedInputs => {
            return Err(invalid_input(
                "the selected files contain no recognized SerializedFile or root ResourceFile inputs",
            ));
        }
        ExplicitInspectPlan::ReportRootsOnly => {
            writeln!(output, "No SerializedFile inputs were selected.")?;
            return Ok(());
        }
        ExplicitInspectPlan::LoadWorkspace => {}
    }

    let (workspace, dependencies) = SerializedFileWorkspace::load_from_vfs_with_dependencies(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        SerializedFileWorkspaceLimits::default(),
        DependencyIndexLimits::default(),
        ContainerMountLimits::default(),
    )?;
    writeln!(
        output,
        "explicit file import: {} logical inputs, {} mounted leaves, {} output bytes",
        mount_report.discovered_inputs, mount_report.mounted_entries, mount_report.mounted_bytes
    )?;
    let resource_context = StreamedResourceSummaryContext {
        root_catalog: &root_resources,
        provider: PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed.data_paths)),
    };
    print_workspace_summary_with_streamed_resources_to(
        output,
        &filesystem,
        &workspace,
        &dependencies,
        &resource_context,
    )?;
    Ok(())
}

fn print_explicit_root_resource_catalog_to<W: io::Write>(
    output: &mut W,
    catalog: &RootResourceFileCatalog,
) -> Result<(), io::Error> {
    writeln!(
        output,
        "Explicit GameBundle root ResourceFiles: {}",
        catalog.files().len()
    )?;
    for (index, resource) in catalog.files().iter().enumerate() {
        writeln!(
            output,
            "  root resource[{index}]: name={:?}, virtual_path={:?}",
            resource.name, resource.virtual_path
        )?;
    }
    Ok(())
}

/// `MixedGameStructure.SelectUniquePaths` over caller facts: explicit inputs naming
/// the same logical file load once, mirroring the source `Distinct()` after
/// `MultiFileStream.GetFilePath` (split-part spellings collapse to their base path).
/// Comparison is exact on the logical spelling without filesystem canonicalization,
/// and the first occurrence wins so CLI order is preserved. This runs before mounting
/// because per-input destination roots would otherwise defeat the mount-level
/// duplicate-input gate and load the same file twice.
fn deduplicate_logical_input_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for path in paths {
        if seen.insert(logical_file_path(path)) {
            unique.push(path.clone());
        }
    }
    unique
}

fn mount_input_files(
    input_files: &[PathBuf],
) -> Result<(VirtualFileSystem, Vec<String>, BatchMountReport), Box<dyn Error>> {
    let input_files = deduplicate_logical_input_paths(input_files);
    let mut filesystem = VirtualFileSystem::new();
    let mut mount_inputs = Vec::new();
    mount_inputs
        .try_reserve_exact(input_files.len())
        .map_err(|_| invalid_input("could not allocate the explicit input path list"))?;
    let mut selected_paths = Vec::new();
    selected_paths
        .try_reserve_exact(input_files.len())
        .map_err(|_| invalid_input("could not allocate the selected VFS path list"))?;
    for (index, path) in input_files.iter().enumerate() {
        let destination_root = format!("/explicit/{index}");
        mount_inputs.push(PathMountInput {
            path: path.clone(),
            destination_root: Some(destination_root.clone()),
        });
        selected_paths.push(destination_root);
    }
    let mount_report = SchemeReader::mount_paths(
        &mount_inputs,
        &mut filesystem,
        SchemeReaderLimits::default(),
        ContainerMountLimits::default(),
        BatchMountLimits::default(),
    )?;
    Ok((filesystem, selected_paths, mount_report))
}

fn mount_input_files_for_export(
    input_files: &[PathBuf],
) -> Result<(VirtualFileSystem, Vec<String>, BatchMountReport), Box<dyn Error>> {
    let mounted = mount_input_files(input_files)?;
    ensure_no_batch_container_failures(&mounted.2)?;
    Ok(mounted)
}

fn ensure_no_batch_container_failures(report: &BatchMountReport) -> Result<(), Box<dyn Error>> {
    let Some(failure) = report.failed_entries.first() else {
        return Ok(());
    };
    Err(invalid_input(format!(
        "refusing to export an incomplete container mount: {} UnityFS entries failed; first failure is input {:?}, entry {} {:?}: {}",
        report.failed_entries.len(),
        failure.input_path,
        failure.entry.entry_index,
        failure.entry.source_name,
        failure.entry.error
    )))
}

fn ensure_no_workspace_container_failures(
    workspace: &SerializedFileWorkspace,
) -> Result<(), Box<dyn Error>> {
    let Some(failure) = workspace.failed_container_entries().first() else {
        return Ok(());
    };
    Err(invalid_input(format!(
        "refusing to export an incomplete dependency-container mount: {} UnityFS entries failed; first failure is container {:?}, entry {} {:?}: {}",
        workspace.failed_container_entries().len(),
        failure.source_container_path,
        failure.entry.entry_index,
        failure.entry.source_name,
        failure.entry.error
    )))
}

fn print_batch_container_failures_to<W: io::Write>(
    output: &mut W,
    report: &BatchMountReport,
) -> io::Result<()> {
    let failures = &report.failed_entries;
    if failures.is_empty() {
        return Ok(());
    }
    writeln!(
        output,
        "UnityFS entries recorded as failed: {}",
        failures.len()
    )?;
    for failed in failures {
        writeln!(
            output,
            "  input {:?}; entry {} {:?}; VFS path {:?}: {}",
            failed.input_path,
            failed.entry.entry_index,
            failed.entry.source_name,
            failed.entry.virtual_path,
            failed.entry.error
        )?;
    }
    Ok(())
}

fn print_workspace_container_failures_to<W: io::Write>(
    output: &mut W,
    workspace: &SerializedFileWorkspace,
) -> io::Result<()> {
    let failures = workspace.failed_container_entries();
    if failures.is_empty() {
        return Ok(());
    }
    writeln!(
        output,
        "UnityFS entries recorded as failed during workspace expansion: {}",
        failures.len()
    )?;
    for failed in failures {
        writeln!(
            output,
            "  container {:?}; entry {} {:?}; VFS path {:?}: {}",
            failed.source_container_path,
            failed.entry.entry_index,
            failed.entry.source_name,
            failed.entry.virtual_path,
            failed.entry.error
        )?;
    }
    Ok(())
}

fn mount_project_inputs(
    input_paths: &[PathBuf],
) -> Result<(VirtualFileSystem, Vec<String>, BatchMountReport), Box<dyn Error>> {
    let input_paths = deduplicate_logical_input_paths(input_paths);
    if input_paths.is_empty() {
        return Err(invalid_input(
            "project export needs at least one input path",
        ));
    }
    let mut filesystem = VirtualFileSystem::new();
    let limits = BatchMountLimits::default();
    let mut total = BatchMountReport::default();
    let mut selected_paths = Vec::new();
    selected_paths
        .try_reserve_exact(input_paths.len())
        .map_err(|_| invalid_input("could not allocate project input roots"))?;

    for (index, input_path) in input_paths.iter().enumerate() {
        let metadata = fs::symlink_metadata(input_path).map_err(|source| {
            io::Error::new(
                source.kind(),
                format!("could not inspect project input {:?}: {source}", input_path),
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(invalid_input(format!(
                "project input {:?} is a symbolic link",
                input_path
            )));
        }
        let destination_root = format!("/project-input/{index}");
        let remaining = BatchMountLimits {
            max_inputs: remaining_budget_usize(
                limits.max_inputs,
                total.discovered_inputs,
                "logical input count",
            )?,
            max_directory_entries: remaining_budget_usize(
                limits.max_directory_entries,
                total.directory_entries_scanned,
                "directory entry count",
            )?,
            max_directory_depth: limits.max_directory_depth,
            max_total_input_bytes: remaining_budget_u64(
                limits.max_total_input_bytes,
                total.discovered_source_bytes,
                "source input byte count",
            )?,
            max_total_output_bytes: remaining_budget_u64(
                limits.max_total_output_bytes,
                total.mounted_bytes,
                "mounted output byte count",
            )?,
            max_failed_entries: remaining_budget_usize(
                limits.max_failed_entries,
                total.failed_entries.len(),
                "failed container entry count",
            )?,
            max_total_failed_entry_metadata_bytes: remaining_budget_u64(
                limits.max_total_failed_entry_metadata_bytes,
                total.failed_entry_metadata_bytes,
                "failed container entry metadata byte count",
            )?,
        };
        let next = if metadata.is_dir() {
            SchemeReader::mount_directory(
                input_path,
                &mut filesystem,
                Some(&destination_root),
                SchemeReaderLimits::default(),
                ContainerMountLimits::default(),
                remaining,
            )?
        } else if metadata.is_file() {
            SchemeReader::mount_paths(
                &[PathMountInput {
                    path: input_path.clone(),
                    destination_root: Some(destination_root.clone()),
                }],
                &mut filesystem,
                SchemeReaderLimits::default(),
                ContainerMountLimits::default(),
                remaining,
            )?
        } else {
            return Err(invalid_input(format!(
                "project input {:?} is not a regular file or directory",
                input_path
            )));
        };
        merge_mount_report(&mut total, &next)?;
        selected_paths.push(destination_root);
    }
    Ok((filesystem, selected_paths, total))
}

fn inspect_directories(input_directories: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    inspect_directories_to(input_directories, &mut output)
}

fn inspect_directories_to<W: io::Write>(
    input_directories: &[PathBuf],
    output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, total_mount_report) =
        mount_input_directories(input_directories, true)?;
    print_batch_container_failures_to(output, &total_mount_report)?;
    let structure = detect_platform_structure(
        &filesystem,
        &selected_paths,
        PlatformStructureLimits::default(),
    )?;
    if let Some(structure) = structure {
        let mixed = MixedGameStructure::from_vfs_paths_with_consumed_paths(
            &filesystem,
            &selected_paths,
            &structure.consumed_paths,
            MixedGameStructureLimits::default(),
        )?;
        let inventory = structure.collect_game_files(
            &filesystem,
            PlatformFileInventoryOptions::default(),
            PlatformFileInventoryLimits::default(),
        )?;
        let loaded = structure.load_primary_serialized_workspace_with_additional_inputs(
            &mut filesystem,
            PlatformWorkspaceLoadOptions::default(),
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            &mixed.data_paths,
        )?;
        writeln!(
            output,
            "platform: {:?}; source registrations: {}; mixed registrations: {}; scripting backend: {:?}; assemblies: {}; initial containers: {}",
            structure.platform,
            inventory.registered_files.len(),
            mixed.registered_files.len(),
            inventory.scripting_backend,
            inventory.assemblies.len(),
            loaded.initial_container_expansions.len()
        )?;
        writeln!(
            output,
            "platform directory import: {} logical files, {} source bytes",
            total_mount_report.discovered_inputs, total_mount_report.discovered_source_bytes
        )?;
        let root_catalog = RootResourceFileCatalog::default();
        let resource_context = StreamedResourceSummaryContext {
            root_catalog: &root_catalog,
            provider: PlatformResourceProvider::with_mixed_data_paths(
                Some(&structure),
                Some(&mixed.data_paths),
            ),
        };
        print_workspace_summary_with_streamed_resources_to(
            output,
            &filesystem,
            &loaded.workspace,
            &loaded.dependencies,
            &resource_context,
        )?;
        return Ok(());
    }

    drop(filesystem);
    let (mut filesystem, selected_paths, total_mount_report) =
        mount_input_directories(input_directories, false)?;
    print_batch_container_failures_to(output, &total_mount_report)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    if plan_load_process(mixed.serialized_file_inputs.len()) == LoadProcessPlan::ReturnUnprocessed {
        writeln!(
            output,
            "No valid assets were found in the selected directories; processing was skipped."
        )?;
        return Ok(());
    }
    let (workspace, dependencies) = SerializedFileWorkspace::load_from_vfs_with_dependencies(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        SerializedFileWorkspaceLimits::default(),
        DependencyIndexLimits::default(),
        ContainerMountLimits::default(),
    )?;
    writeln!(
        output,
        "platform: mixed; registered files: {}; assemblies: {}",
        mixed.registered_files.len(),
        mixed.assemblies.len()
    )?;
    writeln!(
        output,
        "recursively mounted directory import: {} logical inputs, {} leaf files, {} output bytes",
        total_mount_report.discovered_inputs,
        total_mount_report.mounted_entries,
        total_mount_report.mounted_bytes
    )?;
    let root_catalog = RootResourceFileCatalog::default();
    let resource_context = StreamedResourceSummaryContext {
        root_catalog: &root_catalog,
        provider: PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed.data_paths)),
    };
    print_workspace_summary_with_streamed_resources_to(
        output,
        &filesystem,
        &workspace,
        &dependencies,
        &resource_context,
    )?;
    Ok(())
}

fn mount_input_directories(
    input_directories: &[PathBuf],
    preserve_containers: bool,
) -> Result<(VirtualFileSystem, Vec<String>, BatchMountReport), Box<dyn Error>> {
    let input_directories = deduplicate_logical_input_paths(input_directories);
    let mut filesystem = VirtualFileSystem::new();
    let batch_limits = BatchMountLimits::default();
    let mut total_mount_report = BatchMountReport::default();
    let mut selected_paths = Vec::new();
    selected_paths
        .try_reserve_exact(input_directories.len())
        .map_err(|_| invalid_input("could not allocate the selected input list"))?;

    for (index, input_directory) in input_directories.iter().enumerate() {
        if !input_directory.is_dir() {
            return Err(invalid_input(format!(
                "input '{}' is not a directory",
                input_directory.display()
            )));
        }

        let destination_root = format!("/inputs/{index}");
        filesystem.create_directory(Some(&destination_root))?;
        let remaining_limits = BatchMountLimits {
            max_inputs: remaining_budget_usize(
                batch_limits.max_inputs,
                total_mount_report.discovered_inputs,
                "logical input count",
            )?,
            max_directory_entries: remaining_budget_usize(
                batch_limits.max_directory_entries,
                total_mount_report.directory_entries_scanned,
                "directory entry count",
            )?,
            max_directory_depth: batch_limits.max_directory_depth,
            max_total_input_bytes: remaining_budget_u64(
                batch_limits.max_total_input_bytes,
                total_mount_report.discovered_source_bytes,
                "source input byte count",
            )?,
            max_total_output_bytes: if preserve_containers {
                batch_limits.max_total_output_bytes
            } else {
                remaining_budget_u64(
                    batch_limits.max_total_output_bytes,
                    total_mount_report.mounted_bytes,
                    "mounted output byte count",
                )?
            },
            max_failed_entries: remaining_budget_usize(
                batch_limits.max_failed_entries,
                total_mount_report.failed_entries.len(),
                "failed container entry count",
            )?,
            max_total_failed_entry_metadata_bytes: remaining_budget_u64(
                batch_limits.max_total_failed_entry_metadata_bytes,
                total_mount_report.failed_entry_metadata_bytes,
                "failed container entry metadata byte count",
            )?,
        };
        let report = if preserve_containers {
            SchemeReader::mount_directory_preserving_containers(
                input_directory,
                &mut filesystem,
                Some(&destination_root),
                SchemeReaderLimits::default(),
                ContainerMountLimits::default(),
                remaining_limits,
            )?
        } else {
            SchemeReader::mount_directory(
                input_directory,
                &mut filesystem,
                Some(&destination_root),
                SchemeReaderLimits::default(),
                ContainerMountLimits::default(),
                remaining_limits,
            )?
        };
        merge_mount_report(&mut total_mount_report, &report)?;
        println!(
            "mounted {}: {} logical inputs, {} VFS files, {} bytes",
            input_directory.display(),
            report.discovered_inputs,
            report.mounted_entries,
            report.mounted_bytes
        );
        selected_paths.push(destination_root);
    }

    Ok((filesystem, selected_paths, total_mount_report))
}

fn print_reference_graph_summary_to<W: io::Write>(
    output: &mut W,
    workspace: &SerializedFileWorkspace,
    graph: &MultiFileReferenceGraph<'_>,
) -> Result<(), Box<dyn Error>> {
    let mut null_references = 0usize;
    let mut resolved_references = 0usize;
    let mut unresolved_references = 0usize;
    for reference in graph.references() {
        let count = match &reference.resolution {
            MultiFilePPtrResolution::Null => &mut null_references,
            MultiFilePPtrResolution::Object { .. } => &mut resolved_references,
            _ => &mut unresolved_references,
        };
        *count = count
            .checked_add(1)
            .ok_or_else(|| invalid_data("reference resolution count overflows"))?;
    }
    let scan_issue_count = workspace
        .files()
        .iter()
        .try_fold(0usize, |total, file| {
            total.checked_add(file.reference_graph.issues().len())
        })
        .ok_or_else(|| invalid_data("reference graph issue count overflows"))?;
    let diagnostic_count = unresolved_references
        .checked_add(scan_issue_count)
        .ok_or_else(|| invalid_data("reference graph diagnostic count overflows"))?;

    writeln!(
        output,
        "TypeTree reference graph: {} recognized PPtrs, {null_references} null, {resolved_references} uniquely resolved, {unresolved_references} unresolved; {scan_issue_count} per-file scan issues; complete={}",
        graph.references().len(),
        graph.is_complete()
    )?;

    let mut diagnostics = 0usize;
    for reference in graph.references().iter().filter(|reference| {
        !matches!(
            &reference.resolution,
            MultiFilePPtrResolution::Null | MultiFilePPtrResolution::Object { .. }
        )
    }) {
        if diagnostics >= MAX_REFERENCE_GRAPH_DIAGNOSTICS {
            break;
        }
        let source_file = workspace
            .files()
            .get(reference.source_file_index)
            .ok_or_else(|| invalid_data("reference graph source file index is invalid"))?;
        write_unresolved_reference_diagnostic(output, source_file, reference)?;
        diagnostics += 1;
    }
    if diagnostics < MAX_REFERENCE_GRAPH_DIAGNOSTICS {
        'file_issues: for (file_index, file) in workspace.files().iter().enumerate() {
            for issue in file.reference_graph.issues() {
                if diagnostics >= MAX_REFERENCE_GRAPH_DIAGNOSTICS {
                    break 'file_issues;
                }
                writeln!(
                    output,
                    "  Reference scan issue: file {file_index} {:?} (name {:?}): {issue:?}",
                    file.virtual_path, file.file_name
                )?;
                diagnostics += 1;
            }
        }
    }
    if diagnostic_count > diagnostics {
        writeln!(
            output,
            "  {} additional reference diagnostics are not listed; summary counts include them",
            diagnostic_count - diagnostics
        )?;
    }
    Ok(())
}

fn write_unresolved_reference_diagnostic<W: io::Write>(
    output: &mut W,
    source_file: &assetripper::serialized_file::workspace::LoadedSerializedFile,
    reference: &MultiFileObjectReference<'_>,
) -> io::Result<()> {
    writeln!(
        output,
        "  Unresolved PPtr: file {} {:?} (name {:?}), object {}, PathID {}, field {:?}: {:?}",
        reference.source_file_index,
        source_file.virtual_path,
        source_file.file_name,
        reference.source_object_index,
        reference.source_path_id,
        reference.field_path,
        reference.resolution
    )
}

fn print_streamed_resource_summary_to<W: io::Write>(
    output: &mut W,
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    decoded_objects: &ObjectWorkspace,
    context: &StreamedResourceSummaryContext<'_>,
) -> Result<(), Box<dyn Error>> {
    let mut total_value_nodes = 0usize;
    let mut total_candidates = 0usize;
    let mut total_resource_path_bytes = 0usize;
    let mut unscannable_objects = 0usize;
    let mut scan_errors = 0usize;
    let mut unset = 0usize;
    let mut missing = 0usize;
    let mut resolved_bundle = 0usize;
    let mut resolved_root = 0usize;
    let mut resolved_external = 0usize;
    let mut invalid_ranges = 0usize;
    let mut resolution_errors = 0usize;
    let mut diagnostic_count = 0usize;
    let mut listed_diagnostics = 0usize;

    for object in decoded_objects.objects() {
        let source_file = workspace
            .files()
            .get(object.file_index)
            .ok_or_else(|| invalid_data("decoded object refers to an invalid workspace file"))?;
        let payload = match &object.payload {
            Ok(payload) => payload,
            Err(_) => {
                increment_streamed_resource_count(
                    &mut unscannable_objects,
                    "unscannable StreamingInfo object count",
                )?;
                continue;
            }
        };
        let scan = match scan_streamed_resource_infos(
            &payload.root,
            StreamedResourceScanLimits::default(),
        ) {
            Ok(scan) => scan,
            Err(error) => {
                increment_streamed_resource_count(
                    &mut scan_errors,
                    "StreamingInfo scan error count",
                )?;
                increment_streamed_resource_count(
                    &mut diagnostic_count,
                    "StreamingInfo diagnostic count",
                )?;
                if listed_diagnostics < MAX_STREAMED_RESOURCE_DIAGNOSTICS {
                    write_streamed_resource_diagnostic(
                        output,
                        source_file,
                        object,
                        None,
                        None,
                        &error.to_string(),
                    )?;
                    listed_diagnostics += 1;
                }
                continue;
            }
        };
        total_value_nodes = total_value_nodes
            .checked_add(scan.value_nodes_scanned)
            .ok_or_else(|| invalid_data("aggregate StreamingInfo value-node count overflows"))?;
        if total_value_nodes > MAX_TOTAL_STREAMED_RESOURCE_VALUE_NODES {
            return Err(invalid_data_owned(format!(
                "aggregate StreamingInfo scan visits {total_value_nodes} value nodes, exceeding limit {MAX_TOTAL_STREAMED_RESOURCE_VALUE_NODES}"
            ))
            .into());
        }
        total_candidates = total_candidates
            .checked_add(scan.values.len())
            .ok_or_else(|| invalid_data("aggregate StreamingInfo candidate count overflows"))?;
        if total_candidates > MAX_TOTAL_STREAMED_RESOURCE_CANDIDATES {
            return Err(invalid_data_owned(format!(
                "aggregate StreamingInfo scan found {total_candidates} candidates, exceeding limit {MAX_TOTAL_STREAMED_RESOURCE_CANDIDATES}"
            ))
            .into());
        }
        let object_path_bytes = scan.values.iter().try_fold(0usize, |total, located| {
            total.checked_add(located.info.path.len())
        });
        let object_path_bytes = object_path_bytes.ok_or_else(|| {
            invalid_data("aggregate StreamingInfo resource-path byte count overflows")
        })?;
        total_resource_path_bytes = total_resource_path_bytes
            .checked_add(object_path_bytes)
            .ok_or_else(|| {
                invalid_data("aggregate StreamingInfo resource-path byte count overflows")
            })?;
        if total_resource_path_bytes > MAX_TOTAL_STREAMED_RESOURCE_PATH_BYTES {
            return Err(invalid_data_owned(format!(
                "aggregate StreamingInfo resource paths use {total_resource_path_bytes} bytes, exceeding limit {MAX_TOTAL_STREAMED_RESOURCE_PATH_BYTES}"
            ))
            .into());
        }

        for located in &scan.values {
            let resolution = resolve_streamed_resource_info(
                filesystem,
                workspace,
                source_file.container_bundle_index,
                &located.info,
                context.root_catalog,
                &context.provider,
                PlatformStructureLimits::default(),
            );
            match resolution {
                Ok(StreamedResourceResolution::Unset) => {
                    increment_streamed_resource_count(&mut unset, "unset StreamingInfo count")?;
                }
                Ok(StreamedResourceResolution::Missing {
                    unavailable_data_paths,
                }) => {
                    increment_streamed_resource_count(
                        &mut missing,
                        "missing StreamingInfo resource count",
                    )?;
                    increment_streamed_resource_count(
                        &mut diagnostic_count,
                        "StreamingInfo diagnostic count",
                    )?;
                    if listed_diagnostics < MAX_STREAMED_RESOURCE_DIAGNOSTICS {
                        let outcome = format!(
                            "resource missing; {} DataPaths were unavailable",
                            unavailable_data_paths.len()
                        );
                        write_streamed_resource_diagnostic(
                            output,
                            source_file,
                            object,
                            Some(&located.field_path),
                            Some(&located.info),
                            &outcome,
                        )?;
                        listed_diagnostics += 1;
                    }
                }
                Ok(StreamedResourceResolution::Resolved {
                    resource,
                    range_is_valid: true,
                }) => {
                    let count = match resource {
                        ResolvedResourceFile::Bundled { .. } => &mut resolved_bundle,
                        ResolvedResourceFile::ExplicitRoot { .. } => &mut resolved_root,
                        ResolvedResourceFile::External { .. } => &mut resolved_external,
                        ResolvedResourceFile::Missing { .. } => {
                            return Err(invalid_data(
                                "resolved StreamingInfo contains a missing resource result",
                            )
                            .into());
                        }
                    };
                    increment_streamed_resource_count(count, "resolved StreamingInfo range count")?;
                }
                Ok(StreamedResourceResolution::Resolved {
                    resource: _,
                    range_is_valid: false,
                }) => {
                    increment_streamed_resource_count(
                        &mut invalid_ranges,
                        "invalid StreamingInfo range count",
                    )?;
                    increment_streamed_resource_count(
                        &mut diagnostic_count,
                        "StreamingInfo diagnostic count",
                    )?;
                    if listed_diagnostics < MAX_STREAMED_RESOURCE_DIAGNOSTICS {
                        write_streamed_resource_diagnostic(
                            output,
                            source_file,
                            object,
                            Some(&located.field_path),
                            Some(&located.info),
                            "resource resolved but offset/size exceeds its byte length",
                        )?;
                        listed_diagnostics += 1;
                    }
                }
                Err(error) => {
                    increment_streamed_resource_count(
                        &mut resolution_errors,
                        "StreamingInfo resolution error count",
                    )?;
                    increment_streamed_resource_count(
                        &mut diagnostic_count,
                        "StreamingInfo diagnostic count",
                    )?;
                    if listed_diagnostics < MAX_STREAMED_RESOURCE_DIAGNOSTICS {
                        let outcome = error.to_string();
                        write_streamed_resource_diagnostic(
                            output,
                            source_file,
                            object,
                            Some(&located.field_path),
                            Some(&located.info),
                            &outcome,
                        )?;
                        listed_diagnostics += 1;
                    }
                }
            }
        }
    }

    let complete = unscannable_objects == 0
        && scan_errors == 0
        && missing == 0
        && invalid_ranges == 0
        && resolution_errors == 0;
    writeln!(
        output,
        "StreamingInfo resources: {total_candidates} candidates, {unset} unset, {resolved_bundle} bundle ranges, {resolved_root} explicit root ranges, {resolved_external} external ranges, {missing} missing, {invalid_ranges} invalid ranges, {resolution_errors} resolution errors, {scan_errors} scan errors, {unscannable_objects} undecoded objects; scanned {total_value_nodes} value nodes and {total_resource_path_bytes} path bytes; complete={complete}"
    )?;
    if diagnostic_count > listed_diagnostics {
        writeln!(
            output,
            "  {} additional StreamingInfo diagnostics are not listed; summary counts include them",
            diagnostic_count - listed_diagnostics
        )?;
    }
    Ok(())
}

fn increment_streamed_resource_count(
    count: &mut usize,
    description: &str,
) -> Result<(), Box<dyn Error>> {
    *count = count
        .checked_add(1)
        .ok_or_else(|| invalid_data_owned(format!("{description} overflows")))?;
    Ok(())
}

fn write_streamed_resource_diagnostic<W: io::Write>(
    output: &mut W,
    source_file: &assetripper::serialized_file::workspace::LoadedSerializedFile,
    object: &WorkspaceObject,
    field_path: Option<&str>,
    info: Option<&StreamedResourceInfo>,
    outcome: &str,
) -> io::Result<()> {
    let file_path = bounded_diagnostic_text(&source_file.virtual_path, 256);
    let file_name = bounded_diagnostic_text(&source_file.file_name, 256);
    match (field_path, info) {
        (Some(field_path), Some(info)) => writeln!(
            output,
            "  StreamingInfo diagnostic: file {} {:?} (name {:?}), object {}, PathID {}, field {:?}, path {:?} (truncated={}), offset={}, size={}: {}",
            object.file_index,
            file_path.text,
            file_name.text,
            object.object_index,
            object.path_id,
            bounded_diagnostic_text(field_path, 256).text,
            bounded_diagnostic_text(&info.path, 256).text,
            bounded_diagnostic_text(&info.path, 256).truncated,
            info.offset,
            info.size,
            outcome
        ),
        _ => writeln!(
            output,
            "  StreamingInfo scan diagnostic: file {} {:?} (name {:?}), object {}, PathID {}: {}",
            object.file_index,
            file_path.text,
            file_name.text,
            object.object_index,
            object.path_id,
            outcome
        ),
    }
}

struct BoundedDiagnosticText<'a> {
    text: &'a str,
    truncated: bool,
}

fn bounded_diagnostic_text(value: &str, max_bytes: usize) -> BoundedDiagnosticText<'_> {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    BoundedDiagnosticText {
        text: &value[..end],
        truncated: end < value.len(),
    }
}

#[cfg(test)]
fn print_workspace_summary_to<W: io::Write>(
    output: &mut W,
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    dependencies: &SerializedFileDependencyReport,
) -> Result<(), Box<dyn Error>> {
    print_workspace_summary_with_optional_resources_to(
        output,
        filesystem,
        workspace,
        dependencies,
        None,
    )
}

fn print_workspace_summary_with_streamed_resources_to<W: io::Write>(
    output: &mut W,
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    dependencies: &SerializedFileDependencyReport,
    context: &StreamedResourceSummaryContext<'_>,
) -> Result<(), Box<dyn Error>> {
    print_workspace_summary_with_optional_resources_to(
        output,
        filesystem,
        workspace,
        dependencies,
        Some(context),
    )
}

fn print_workspace_summary_with_optional_resources_to<W: io::Write>(
    output: &mut W,
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    dependencies: &SerializedFileDependencyReport,
    resource_context: Option<&StreamedResourceSummaryContext<'_>>,
) -> Result<(), Box<dyn Error>> {
    print_workspace_container_failures_to(output, workspace)?;
    let source_collection_file_indices = workspace.source_collection_file_indices()?;
    let mut source_collection_order_by_file = Vec::new();
    source_collection_order_by_file
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_data("could not reserve source collection order index"))?;
    source_collection_order_by_file.resize(workspace.files().len(), usize::MAX);
    for (order, &file_index) in source_collection_file_indices.iter().enumerate() {
        let Some(source_order) = source_collection_order_by_file.get_mut(file_index) else {
            return Err(
                invalid_data("source collection traversal returned an invalid file index").into(),
            );
        };
        *source_order = order;
    }

    let multi_file_graph = workspace.reference_graph(MultiFileReferenceGraphLimits::default())?;
    let mut scene_collection_flags = Vec::new();
    scene_collection_flags
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_data("could not reserve scene collection flags"))?;
    scene_collection_flags.extend(workspace.files().iter().map(|file| {
        file.metadata
            .objects
            .iter()
            .any(|object| scene_manager_kind(workspace_object_class_id(object)).is_some())
    }));
    let mut scene_hierarchy_builder = WorkspaceSceneHierarchyBuilder::new(
        workspace.files().len(),
        WorkspaceSceneHierarchyLimits::default(),
    )?;
    let mut generated_transform_plan_builder = WorkspaceGeneratedTransformPlanBuilder::new(
        workspace.files().len(),
        WorkspaceGeneratedTransformPlanLimits::default(),
    )?;
    let mut scene_guid_words_by_file = Vec::new();
    scene_guid_words_by_file
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_data("could not reserve scene GUID index"))?;
    scene_guid_words_by_file.resize(workspace.files().len(), None::<SceneGuidWords>);
    let mut streamed_scene_bundle_work = Vec::new();
    let mut total_streamed_scene_bundle_objects = 0usize;
    let mut total_streamed_scene_bundle_entries = 0usize;
    let mut total_streamed_scene_bundle_string_bytes = 0usize;
    // (bundle_index, source_collection_order, object_index, resolved GetAssetBundleName,
    //  has_streamed_flag, is_streamed_value) for the PrefabProcessor scene-pass planner.
    // Standalone files use usize::MAX as the bundle index so they never match a
    // SerializedBundle lookup; their facts are retained only for diagnostics.
    let mut scene_bundle_name_facts: Vec<(usize, usize, usize, String, bool, bool)> = Vec::new();

    writeln!(output, "SerializedFiles: {}", workspace.files().len())?;
    let decoded_objects =
        ObjectWorkspace::load_from_vfs(filesystem, workspace, ObjectWorkspaceLimits::default())?;
    if let Some(context) = resource_context {
        print_streamed_resource_summary_to(
            output,
            filesystem,
            workspace,
            &decoded_objects,
            context,
        )?;
    }
    let animator_controller_inventory = animator_controller_source_dependency_inventory(
        &decoded_objects,
        WorkspaceProcessedPrefabAssetPlanLimits::default(),
    )?;
    let collection_models = WorkspaceAssetCollections::build(
        workspace,
        &decoded_objects,
        WorkspaceCollectionLimits::default(),
    )?;
    let object_report = decoded_objects.report();
    let collection_report = collection_models.report();
    writeln!(
        output,
        "Workspace collections: {} ({} objects, {} external identifiers)",
        collection_report.collection_count,
        collection_report.object_count,
        collection_report.external_identifier_count
    )?;
    writeln!(
        output,
        "AnimatorController source dependencies: {} generated-collection appends modeled, {} source files deferred by incomplete TypeTree data",
        animator_controller_inventory.modeled_append_count(),
        animator_controller_inventory.deferred_file_count()
    )?;
    writeln!(
        output,
        "Object payload decode: {}/{} decoded, {} undecoded, {} raw fallback bytes",
        object_report.decoded_objects,
        object_report.object_count,
        object_report.undecoded_objects,
        object_report.total_undecoded_raw_bytes
    )?;
    print_reference_graph_summary_to(output, workspace, &multi_file_graph)?;
    let mut decode_diagnostics = 0usize;
    for object in decoded_objects
        .objects()
        .iter()
        .filter(|object| object.payload.is_err())
        .take(MAX_OBJECT_DECODE_DIAGNOSTICS)
    {
        let file = workspace
            .files()
            .get(object.file_index)
            .ok_or_else(|| invalid_data("decoded object refers to an invalid workspace file"))?;
        let Err(error) = &object.payload else {
            return Err(
                invalid_data("undecoded-object diagnostics lost their decode error").into(),
            );
        };
        writeln!(
            output,
            "  Undecoded object: file {} {:?} (name {:?}), object {}, PathID {}, class {:?}, bytes {:?}: {error}",
            object.file_index,
            file.virtual_path,
            file.file_name,
            object.object_index,
            object.path_id,
            object.class_id,
            object.raw_payload_range
        )?;
        decode_diagnostics += 1;
    }
    if object_report.undecoded_objects > decode_diagnostics {
        writeln!(
            output,
            "  {} additional object decode errors are not listed; summary counts include them",
            object_report.undecoded_objects - decode_diagnostics,
        )?;
    }
    drop(decoded_objects);

    let mut build_settings_paths = None;
    let mut selected_build_settings = None;
    'select_build_settings: for &file_index in source_collection_file_indices.iter().rev() {
        let file = workspace
            .files()
            .get(file_index)
            .ok_or_else(|| invalid_data("source collection file index is invalid"))?;
        for (object_index, object) in file.metadata.objects.iter().enumerate().rev() {
            if workspace_object_class_id(object) != Some(141) {
                continue;
            }
            let maximum_bytes =
                usize::try_from(SerializedFileWorkspaceLimits::default().max_file_bytes)
                    .unwrap_or(usize::MAX);
            let file_bytes = filesystem.read_all_bytes(&file.virtual_path, maximum_bytes)?;
            let scenes = decode_build_settings_scene_paths_from_parts(
                &file.metadata,
                &file.header,
                &file_bytes,
                object_index,
                BuildSettingsSceneLimits::default(),
                &ObjectDecodeLimits::default(),
            )?;
            build_settings_paths = Some(scenes);
            selected_build_settings = Some((file_index, object.path_id));
            break 'select_build_settings;
        }
    }

    let editor_build_settings = build_settings_paths
        .as_deref()
        .map(|paths| {
            EditorBuildSettingsModel::from_build_settings_paths(
                paths,
                EditorBuildSettingsLimits::default(),
            )
        })
        .transpose()?;
    if let Some(settings) = &editor_build_settings {
        writeln!(
            output,
            "Generated EditorBuildSettings: {} scenes",
            settings.scenes().len()
        )?;
        for (scene_index, scene) in settings.scenes().iter().enumerate() {
            writeln!(
                output,
                "  scene[{scene_index}]: enabled={}, path={:?}",
                scene.enabled, scene.path
            )?;
        }
    }

    for (file_index, file) in workspace.files().iter().enumerate() {
        let collection_index = collection_models
            .collection_index_for_file(file_index)
            .ok_or_else(|| invalid_data("workspace file has no collection model"))?;
        let collection = collection_models
            .collections()
            .get(collection_index)
            .ok_or_else(|| invalid_data("workspace collection model index is invalid"))?;
        writeln!(
            output,
            "  Collection {collection_index} {:?}: file {:?}, {:?}, {} objects at {:?}, {} external identifiers",
            collection.fixed_name,
            file.virtual_path,
            file.header.format_version,
            file.metadata.objects.len(),
            collection.object_range,
            collection.external_identifier_count
        )?;

        let mut file_bytes = None;
        let scene_manager_count = file
            .metadata
            .objects
            .iter()
            .filter(|object| scene_manager_kind(workspace_object_class_id(object)).is_some())
            .count();
        if scene_manager_count > 0 {
            writeln!(
                output,
                "    Scene collection candidate: {scene_manager_count} LevelGameManager objects"
            )?;
            let contains_occlusion_settings = file.metadata.objects.iter().any(|object| {
                scene_manager_kind(workspace_object_class_id(object))
                    == Some(SceneManagerKind::OcclusionCullingSettings)
            });
            let parsed_scene_guid_file = if contains_occlusion_settings {
                if file_bytes.is_none() {
                    let maximum_bytes =
                        usize::try_from(SerializedFileWorkspaceLimits::default().max_file_bytes)
                            .unwrap_or(usize::MAX);
                    file_bytes =
                        Some(filesystem.read_all_bytes(&file.virtual_path, maximum_bytes)?);
                }
                let Some(bytes) = file_bytes.as_deref() else {
                    return Err(invalid_data("scene manager input bytes were not retained").into());
                };
                Some(SerializedFile::parse(bytes)?)
            } else {
                None
            };
            for (object_index, object) in file.metadata.objects.iter().enumerate() {
                let Some(kind) = scene_manager_kind(workspace_object_class_id(object)) else {
                    continue;
                };
                writeln!(
                    output,
                    "      manager PathID {}: {:?} (class ID {})",
                    object.path_id,
                    kind,
                    kind.class_id()
                )?;
                if kind == SceneManagerKind::OcclusionCullingSettings {
                    let parsed_file = parsed_scene_guid_file.as_ref().ok_or_else(|| {
                        invalid_data("OcclusionCullingSettings file was not parsed")
                    })?;
                    let payload = parsed_file.decode_object_payload(object_index)?;
                    match extract_occlusion_scene_guid(&payload.root)? {
                        Some(scene_guid) => {
                            scene_guid_words_by_file[file_index] = Some(scene_guid);
                            writeln!(
                                output,
                                "        SceneGUID words: [0x{:08x}, 0x{:08x}, 0x{:08x}, 0x{:08x}]",
                                scene_guid.data[0],
                                scene_guid.data[1],
                                scene_guid.data[2],
                                scene_guid.data[3]
                            )?;
                        }
                        None => writeln!(output, "        SceneGUID: absent from this TypeTree")?,
                    }
                }
            }
        }

        let contains_scene_objects =
            file.metadata.objects.iter().any(|object| {
                matches!(workspace_object_class_id(object), Some(1 | 4 | 224 | 1001))
            });
        for (object_index, object) in file.metadata.objects.iter().enumerate() {
            let class_id = workspace_object_class_id(object);
            if class_id != Some(141) {
                continue;
            }
            if file_bytes.is_none() {
                let maximum_bytes =
                    usize::try_from(SerializedFileWorkspaceLimits::default().max_file_bytes)
                        .unwrap_or(usize::MAX);
                file_bytes = Some(filesystem.read_all_bytes(&file.virtual_path, maximum_bytes)?);
            }
            let Some(file_bytes) = file_bytes.as_deref() else {
                return Err(invalid_data("BuildSettings file bytes were not retained").into());
            };
            let scenes = decode_build_settings_scene_paths_from_parts(
                &file.metadata,
                &file.header,
                file_bytes,
                object_index,
                BuildSettingsSceneLimits::default(),
                &ObjectDecodeLimits::default(),
            )?;
            writeln!(
                output,
                "    BuildSettings object {}: {} scene paths{}",
                object.path_id,
                scenes.len(),
                if selected_build_settings == Some((file_index, object.path_id)) {
                    " (selected by source collection traversal)"
                } else {
                    ""
                }
            )?;
            for (scene_index, path) in scenes.iter().enumerate() {
                writeln!(output, "      scene[{scene_index}]: {path}")?;
            }
        }

        if contains_scene_objects || scene_collection_flags[file_index] {
            if file_bytes.is_none() {
                let maximum_bytes =
                    usize::try_from(SerializedFileWorkspaceLimits::default().max_file_bytes)
                        .unwrap_or(usize::MAX);
                file_bytes = Some(filesystem.read_all_bytes(&file.virtual_path, maximum_bytes)?);
            }
            let Some(file_bytes) = file_bytes.as_deref() else {
                return Err(invalid_data("scene input bytes were not retained").into());
            };
            let parsed_file = SerializedFile::parse(file_bytes)?;
            let mut scene_graph = SceneGraph::build(&parsed_file, SceneGraphLimits::default())?;
            scene_graph.resolve_with_multi_file_graph(
                file_index,
                &file.file_name,
                &multi_file_graph,
            )?;
            let root_discovery = scene_graph.discover_workspace_roots(
                file_index,
                &file.file_name,
                &multi_file_graph,
                SceneRootDiscoveryLimits::default(),
            )?;
            generated_transform_plan_builder.add_file(
                file_index,
                &root_discovery,
                &multi_file_graph,
            )?;
            let hierarchy_audit = scene_graph.audit_workspace_transform_links(
                file_index,
                &file.file_name,
                &multi_file_graph,
                SceneHierarchyAuditLimits::default(),
            )?;
            let game_object_count = scene_graph
                .objects()
                .iter()
                .filter(|object| object.class_id == Some(1))
                .count();
            let transform_count = scene_graph
                .objects()
                .iter()
                .filter(|object| matches!(object.class_id, Some(4 | 224)))
                .count();
            writeln!(
                output,
                "    Scene graph: {game_object_count} GameObjects, {transform_count} Transforms, {} relationships, {} decode issues, complete={}",
                scene_graph.relationships().len(),
                scene_graph.issues().len(),
                scene_graph.is_complete()
            )?;
            writeln!(
                output,
                "    Workspace root candidates: {}, root issues: {}, Transform links checked: {}, workspace hierarchy issues: {}, audit complete={}",
                root_discovery.roots().len(),
                root_discovery.issues().len(),
                hierarchy_audit.relationships_checked(),
                hierarchy_audit.issues().len(),
                hierarchy_audit.is_complete()
            )?;
            for root in root_discovery.roots() {
                if let Some(object) = scene_graph.objects().get(root.game_object_index) {
                    let transform = match (root.transform_file_index, root.transform_index) {
                        (Some(transform_file_index), Some(transform_index)) => {
                            let path_id = multi_file_graph
                                .object_identity(transform_file_index, transform_index)
                                .map(|identity| identity.path_id);
                            format!(
                                "Transform file {transform_file_index} object {transform_index} PathID {path_id:?}"
                            )
                        }
                        _ => "no Transform component".to_owned(),
                    };
                    writeln!(
                        output,
                        "      root candidate file {} PathID {} ({transform}; {:?})",
                        root.source_file_index, object.path_id, root.reason
                    )?;
                }
            }
            scene_hierarchy_builder.add_collection_graph(
                file_index,
                &scene_graph,
                root_discovery,
                hierarchy_audit,
                &multi_file_graph,
            )?;
        }

        if file
            .metadata
            .objects
            .iter()
            .any(|object| workspace_object_class_id(object) == Some(142))
        {
            if file_bytes.is_none() {
                let maximum_bytes =
                    usize::try_from(SerializedFileWorkspaceLimits::default().max_file_bytes)
                        .unwrap_or(usize::MAX);
                file_bytes = Some(filesystem.read_all_bytes(&file.virtual_path, maximum_bytes)?);
            }
            let Some(file_bytes) = file_bytes.as_deref() else {
                return Err(invalid_data("AssetBundle input bytes were not retained").into());
            };
            let parsed_file = SerializedFile::parse(file_bytes)?;
            for (object_index, object) in file.metadata.objects.iter().enumerate() {
                if workspace_object_class_id(object) != Some(142) {
                    continue;
                }
                total_streamed_scene_bundle_objects = total_streamed_scene_bundle_objects
                    .checked_add(1)
                    .ok_or_else(|| invalid_data("AssetBundle object count overflows"))?;
                if total_streamed_scene_bundle_objects > MAX_STREAMED_SCENE_BUNDLE_OBJECTS {
                    return Err(invalid_data(
                        "AssetBundle object count exceeds the inspect scene-mapping limit",
                    )
                    .into());
                }
                let payload = parsed_file.decode_object_payload(object_index)?;
                let metadata = extract_streamed_scene_bundle_metadata(
                    &payload.root,
                    SceneBundleMappingLimits::default(),
                )?;
                let object_entries = metadata
                    .scene_hashes
                    .as_ref()
                    .map_or(0, Vec::len)
                    .checked_add(metadata.container_paths.as_ref().map_or(0, Vec::len))
                    .ok_or_else(|| invalid_data("AssetBundle metadata entry count overflows"))?;
                total_streamed_scene_bundle_entries = total_streamed_scene_bundle_entries
                    .checked_add(object_entries)
                    .ok_or_else(|| invalid_data("AssetBundle metadata entry count overflows"))?;
                if total_streamed_scene_bundle_entries > MAX_TOTAL_STREAMED_SCENE_BUNDLE_ENTRIES {
                    return Err(invalid_data(
                        "AssetBundle metadata entries exceed the aggregate inspect limit",
                    )
                    .into());
                }
                let object_string_bytes = metadata
                    .scene_hashes
                    .as_ref()
                    .into_iter()
                    .flat_map(|entries| entries.iter())
                    .flat_map(|(scene_path, collection_name)| {
                        [scene_path.len(), collection_name.len()]
                    })
                    .chain(
                        metadata
                            .container_paths
                            .as_ref()
                            .into_iter()
                            .flat_map(|paths| paths.iter().map(String::len)),
                    )
                    .try_fold(0usize, |total, size| total.checked_add(size))
                    .ok_or_else(|| invalid_data("AssetBundle metadata byte count overflows"))?;
                total_streamed_scene_bundle_string_bytes = total_streamed_scene_bundle_string_bytes
                    .checked_add(object_string_bytes)
                    .ok_or_else(|| invalid_data("AssetBundle metadata byte count overflows"))?;
                if total_streamed_scene_bundle_string_bytes
                    > MAX_TOTAL_STREAMED_SCENE_BUNDLE_STRING_BYTES
                {
                    return Err(invalid_data(
                        "AssetBundle metadata strings exceed the aggregate inspect limit",
                    )
                    .into());
                }

                let owner_bundle_name = match file.container_bundle_index {
                    Some(bundle_index) => workspace
                        .container_bundles()
                        .get(bundle_index)
                        .and_then(|bundle| bundle.name_fixed.clone())
                        .unwrap_or_else(|| file.file_name.clone()),
                    None => "GameBundle".to_owned(),
                };
                let resolved_bundle_name = get_asset_bundle_name(&payload.root, &owner_bundle_name)
                    .map_err(|error| {
                        invalid_data_owned(format!(
                            "AssetBundle PathID {} name resolution failed: {error}",
                            object.path_id
                        ))
                    })?;
                let bundle_fact_index = file.container_bundle_index.unwrap_or(usize::MAX);
                let bundle_fact_order = *source_collection_order_by_file
                    .get(file_index)
                    .ok_or_else(|| {
                        invalid_data("scene bundle name collection order is unavailable")
                    })?;
                scene_bundle_name_facts.push((
                    bundle_fact_index,
                    bundle_fact_order,
                    object_index,
                    resolved_bundle_name,
                    metadata.is_streamed_scene_asset_bundle.is_some(),
                    metadata.is_streamed_scene_asset_bundle.unwrap_or(false),
                ));

                if metadata.is_streamed_scene_asset_bundle == Some(true) {
                    if file.container_bundle_index.is_none() {
                        writeln!(
                            output,
                            "    Streamed-scene AssetBundle PathID {}: scene name recovery is unavailable outside a SerializedBundle",
                            object.path_id
                        )?;
                        continue;
                    }
                    let Some(_owner_collection_index) = file.container_collection_index else {
                        return Err(invalid_data(
                            "streamed-scene AssetBundle is missing its source collection index",
                        )
                        .into());
                    };
                    streamed_scene_bundle_work.try_reserve(1).map_err(|_| {
                        invalid_data("could not reserve streamed-scene AssetBundle work items")
                    })?;
                    streamed_scene_bundle_work.push(StreamedSceneBundleWorkItem {
                        owner_file_index: file_index,
                        object_index,
                        path_id: object.path_id,
                        source_collection_order: *source_collection_order_by_file
                            .get(file_index)
                            .ok_or_else(|| {
                            invalid_data("streamed-scene collection order is unavailable")
                        })?,
                        metadata,
                    });
                } else {
                    writeln!(
                        output,
                        "    AssetBundle PathID {}: streamed-scene flag {:?}",
                        object.path_id, metadata.is_streamed_scene_asset_bundle
                    )?;
                }
            }
        }
    }

    streamed_scene_bundle_work
        .sort_unstable_by_key(|bundle| (bundle.source_collection_order, bundle.object_index));
    let scene_bundle_limits = SceneBundleMappingLimits::default();
    let mut streamed_scene_path_assignments =
        HashMap::<usize, Vec<StreamedScenePathAssignment>>::new();
    let mut total_streamed_scene_path_mappings = 0usize;
    for bundle in streamed_scene_bundle_work {
        let owner_file = workspace
            .files()
            .get(bundle.owner_file_index)
            .ok_or_else(|| invalid_data("streamed-scene AssetBundle file index is invalid"))?;
        let (owner_bundle_index, owner_bundle) = source_bundle_for_collection(
            owner_file.container_bundle_index,
            workspace.container_bundles(),
        )?;
        let collection_file_indices = &owner_bundle.direct_collection_file_indices;
        if collection_file_indices.len() > scene_bundle_limits.max_entries {
            return Err(assetripper::processing::scene_collections::SceneBundleMappingError::EntryCountLimitExceeded {
                resource: "bundle collection",
                count: collection_file_indices.len(),
                limit: scene_bundle_limits.max_entries,
            }
            .into());
        }
        let mut collection_names = Vec::new();
        collection_names
            .try_reserve_exact(collection_file_indices.len())
            .map_err(|_| invalid_data("could not reserve streamed-scene collection names"))?;
        let mut collection_is_scene = Vec::new();
        collection_is_scene
            .try_reserve_exact(collection_file_indices.len())
            .map_err(|_| invalid_data("could not reserve streamed-scene collection flags"))?;
        let mut collection_name_bytes = 0usize;
        for (collection_index, &collection_file_index) in collection_file_indices.iter().enumerate()
        {
            let collection_file =
                workspace
                    .files()
                    .get(collection_file_index)
                    .ok_or_else(|| {
                        invalid_data("container collection file index is outside the workspace")
                    })?;
            if collection_file.container_bundle_index != Some(owner_bundle_index)
                || collection_file.container_collection_index != Some(collection_index)
                || collection_file.container_scope.as_deref() != owner_bundle.scope_path.as_deref()
            {
                return Err(invalid_data(
                    "streamed-scene bundle collection links do not match their source node",
                )
                .into());
            }
            let collection_name = match owner_bundle.scope_path.as_deref() {
                Some(scope) => {
                    relative_container_collection_name(&collection_file.virtual_path, scope)
                        .ok_or_else(|| {
                            invalid_data("container collection path is outside its scope")
                        })?
                }
                None => collection_file.file_name.as_str(),
            };
            collection_name_bytes = collection_name_bytes
                .checked_add(collection_name.len())
                .ok_or_else(|| invalid_data("streamed-scene collection-name bytes overflow"))?;
            if collection_name_bytes > scene_bundle_limits.max_total_name_and_path_bytes {
                return Err(assetripper::processing::scene_collections::SceneBundleMappingError::ByteLimitExceeded {
                    requested: collection_name_bytes,
                    limit: scene_bundle_limits.max_total_name_and_path_bytes,
                }
                .into());
            }
            collection_names.push(collection_name.to_owned());
            collection_is_scene.push(scene_collection_flags[collection_file_index]);
        }

        let Some(path_mappings) = map_streamed_scene_bundle_paths(
            &bundle.metadata,
            &collection_names,
            &collection_is_scene,
            scene_bundle_limits,
        )?
        else {
            return Err(invalid_data(
                "streamed-scene AssetBundle was not selected for scene path mapping",
            )
            .into());
        };
        writeln!(
            output,
            "    Streamed-scene AssetBundle PathID {}: {:?}, {} path mappings",
            bundle.path_id,
            path_mappings.source,
            path_mappings.mappings.len()
        )?;
        for mapping in path_mappings.mappings {
            total_streamed_scene_path_mappings = total_streamed_scene_path_mappings
                .checked_add(1)
                .ok_or_else(|| invalid_data("streamed-scene path mapping count overflows"))?;
            if total_streamed_scene_path_mappings > MAX_TOTAL_STREAMED_SCENE_PATH_MAPPINGS {
                return Err(invalid_data(
                    "streamed-scene path mappings exceed the aggregate inspect limit",
                )
                .into());
            }
            let target_file_index = *collection_file_indices
                .get(mapping.collection_index)
                .ok_or_else(|| invalid_data("scene mapping collection index is out of bounds"))?;
            let collection_name = collection_names
                .get(mapping.collection_index)
                .ok_or_else(|| invalid_data("scene mapping collection name is unavailable"))?;
            scene_collection_flags[target_file_index] = true;
            writeln!(
                output,
                "      collection {:?} (workspace file {}): {}",
                collection_name, target_file_index, mapping.path
            )?;
            if !streamed_scene_path_assignments.contains_key(&target_file_index) {
                streamed_scene_path_assignments
                    .try_reserve(1)
                    .map_err(|_| {
                        invalid_data("could not reserve streamed-scene path assignments")
                    })?;
                streamed_scene_path_assignments.insert(target_file_index, Vec::new());
            }
            let assignments = streamed_scene_path_assignments
                .get_mut(&target_file_index)
                .ok_or_else(|| invalid_data("streamed-scene assignment index disappeared"))?;
            assignments
                .try_reserve(1)
                .map_err(|_| invalid_data("could not reserve streamed-scene assignment"))?;
            assignments.push(StreamedScenePathAssignment {
                path: mapping.path,
                source: path_mappings.source,
                asset_bundle_file_index: bundle.owner_file_index,
                asset_bundle_path_id: bundle.path_id,
            });
        }
    }

    for (file_index, &is_scene_collection) in scene_collection_flags.iter().enumerate() {
        if !is_scene_collection || scene_hierarchy_builder.contains_collection_graph(file_index) {
            continue;
        }
        let file = workspace
            .files()
            .get(file_index)
            .ok_or_else(|| invalid_data("scene collection file index is invalid"))?;
        let maximum_bytes =
            usize::try_from(SerializedFileWorkspaceLimits::default().max_file_bytes)
                .unwrap_or(usize::MAX);
        let bytes = filesystem.read_all_bytes(&file.virtual_path, maximum_bytes)?;
        let parsed_file = SerializedFile::parse(&bytes)?;
        let mut scene_graph = SceneGraph::build(&parsed_file, SceneGraphLimits::default())?;
        scene_graph.resolve_with_multi_file_graph(
            file_index,
            &file.file_name,
            &multi_file_graph,
        )?;
        let roots = scene_graph.discover_workspace_roots(
            file_index,
            &file.file_name,
            &multi_file_graph,
            SceneRootDiscoveryLimits::default(),
        )?;
        if !generated_transform_plan_builder.contains_file(file_index) {
            generated_transform_plan_builder.add_file(file_index, &roots, &multi_file_graph)?;
        }
        let audit = scene_graph.audit_workspace_transform_links(
            file_index,
            &file.file_name,
            &multi_file_graph,
            SceneHierarchyAuditLimits::default(),
        )?;
        scene_hierarchy_builder.add_collection_graph(
            file_index,
            &scene_graph,
            roots,
            audit,
            &multi_file_graph,
        )?;
    }

    let scene_path_set = if let Some(paths) = build_settings_paths.as_deref() {
        BuildSettingsScenePathSet::Unique(paths)
    } else {
        BuildSettingsScenePathSet::Missing
    };
    let scene_count = scene_collection_flags
        .iter()
        .filter(|&&is_scene| is_scene)
        .count();
    let mut scene_definition_plans = Vec::new();
    scene_definition_plans
        .try_reserve_exact(scene_count)
        .map_err(|_| invalid_data("could not reserve workspace scene definitions"))?;
    for &file_index in &source_collection_file_indices {
        if !scene_collection_flags[file_index] {
            continue;
        }
        let file = workspace.files().get(file_index).ok_or_else(|| {
            invalid_data("source collection traversal returned an invalid file index")
        })?;
        let collection_name = file
            .container_scope
            .as_deref()
            .and_then(|scope| relative_container_collection_name(&file.virtual_path, scope))
            .unwrap_or(&file.file_name);
        let status = resolve_scene_collection_path(
            collection_name,
            file.metadata.prefix.unity_version_text.as_deref(),
            scene_path_set,
        );
        writeln!(
            output,
            "    Scene path mapping for collection {:?}: {status}",
            collection_name
        )?;
        let streamed_scene_path = if let Some(assignments) =
            streamed_scene_path_assignments.get(&file_index)
        {
            for assignment in assignments {
                writeln!(
                    output,
                    "      streamed-scene override {:?} via AssetBundle file {} PathID {} ({:?})",
                    assignment.path,
                    assignment.asset_bundle_file_index,
                    assignment.asset_bundle_path_id,
                    assignment.source
                )?;
            }
            if let Some(effective) = assignments.last() {
                let conflicting = assignments
                    .iter()
                    .any(|assignment| assignment.path != effective.path);
                writeln!(
                    output,
                    "      effective scene path: {} (streamed-scene override; assignments={}, conflicting={})",
                    effective.path,
                    assignments.len(),
                    conflicting
                )?;
            }
            assignments
                .last()
                .map(|assignment| assignment.path.as_str())
        } else {
            None
        };
        let scene_plan = plan_scene_definition(
            file_index,
            collection_name,
            &status,
            streamed_scene_path,
            scene_guid_words_by_file[file_index],
            SceneDefinitionPlanLimits::default(),
        )?;
        scene_definition_plans.push(scene_plan);
    }

    let scene_definitions = WorkspaceSceneDefinitions::from_plans(
        scene_definition_plans,
        workspace.files().len(),
        WorkspaceSceneDefinitionLimits::default(),
    )?;
    let scene_hierarchies = scene_hierarchy_builder.finish(&scene_definitions)?;
    // PrefabProcessor scene pass: hierarchy bundle names in source order with
    // FetchAssets()-ordered AssetBundle facts and the streamed-scene assert.
    scene_bundle_name_facts.sort_unstable_by_key(|left| (left.1, left.2));
    let mut file_bundle_sources: Vec<Option<WorkspaceSceneBundleSource>> = Vec::new();
    file_bundle_sources
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_data("could not reserve scene bundle source index"))?;
    for file in workspace.files() {
        if let Some(bundle_index) = file.container_bundle_index {
            let bundle_name = workspace
                .container_bundles()
                .get(bundle_index)
                .and_then(|bundle| bundle.name_fixed.clone())
                .unwrap_or_else(|| file.file_name.clone());
            file_bundle_sources.push(Some(WorkspaceSceneBundleSource {
                bundle_index,
                bundle_name,
                is_serialized_bundle: true,
            }));
        } else {
            file_bundle_sources.push(None);
        }
    }
    let mut bundle_assets_in_fetch_order: HashMap<usize, Vec<WorkspaceBundleAssetNameFact>> =
        HashMap::new();
    for (bundle_index, _, _, resolved_name, has_flag, is_value) in scene_bundle_name_facts {
        if bundle_index == usize::MAX {
            continue;
        }
        bundle_assets_in_fetch_order
            .entry(bundle_index)
            .or_default()
            .push(WorkspaceBundleAssetNameFact {
                resolved_name,
                has_streamed_scene_flag: has_flag,
                is_streamed_scene_asset_bundle: is_value,
            });
    }
    let scene_bundle_collections: Vec<Vec<usize>> = scene_definitions
        .definitions()
        .iter()
        .map(|definition| definition.collection_file_indices().to_vec())
        .collect();
    let scene_bundle_assignments = plan_workspace_scene_hierarchy_bundle_names(
        &scene_bundle_collections,
        &file_bundle_sources,
        &bundle_assets_in_fetch_order,
        WorkspaceSceneHierarchyBundlePlanLimits::default(),
    )?;
    let non_scene_transform_plans = generated_transform_plan_builder.finish(&scene_definitions)?;
    writeln!(
        output,
        "Generated Prefab Transforms planned for non-scene GameObjects: {}",
        non_scene_transform_plans.plans().len()
    )?;
    for plan in non_scene_transform_plans
        .plans()
        .iter()
        .take(MAX_GENERATED_TRANSFORM_DIAGNOSTICS)
    {
        writeln!(
            output,
            "  missing Transform: file {} object {} PathID {}",
            plan.game_object.file_index, plan.game_object.object_index, plan.game_object.path_id
        )?;
    }
    if non_scene_transform_plans.plans().len() > MAX_GENERATED_TRANSFORM_DIAGNOSTICS {
        writeln!(
            output,
            "  {} additional missing-Transform plans are not listed",
            non_scene_transform_plans.plans().len() - MAX_GENERATED_TRANSFORM_DIAGNOSTICS
        )?;
    }
    writeln!(
        output,
        "Non-scene hierarchy collection graphs: {}",
        scene_hierarchies.non_scene_collections().len()
    )?;
    for collection in scene_hierarchies
        .non_scene_collections()
        .iter()
        .take(MAX_GENERATED_TRANSFORM_DIAGNOSTICS)
    {
        writeln!(
            output,
            "  hierarchy file {}: {} GameObjects, {} Transforms, {} component candidates, {} roots, {} root issues, {} relationships, complete={}",
            collection.collection_file_index(),
            collection.game_object_count(),
            collection.transform_count(),
            collection.component_candidates().len(),
            collection.roots().roots().len(),
            collection.roots().issues().len(),
            collection.relationships().len(),
            collection.is_complete()
        )?;
    }
    if scene_hierarchies.non_scene_collections().len() > MAX_GENERATED_TRANSFORM_DIAGNOSTICS {
        writeln!(
            output,
            "  {} additional non-scene hierarchy graphs are not listed",
            scene_hierarchies.non_scene_collections().len() - MAX_GENERATED_TRANSFORM_DIAGNOSTICS
        )?;
    }
    let mut prefab_root_counts = [0usize; 4];
    let mut prefab_root_diagnostics = 0usize;
    for prefab_instance in scene_hierarchies
        .prefab_instance_identities_in_collection_order(&source_collection_file_indices)
    {
        let resolution = scene_hierarchies.resolve_prefab_instance_root(prefab_instance)?;
        let count_index = match &resolution {
            WorkspacePrefabInstanceRootResolution::Resolved(_) => 0,
            WorkspacePrefabInstanceRootResolution::Null => 1,
            WorkspacePrefabInstanceRootResolution::FieldUnavailable => 2,
            _ => 3,
        };
        prefab_root_counts[count_index] = prefab_root_counts[count_index]
            .checked_add(1)
            .ok_or_else(|| invalid_data("PrefabInstance root count overflows usize"))?;
        if count_index >= 2 && prefab_root_diagnostics < MAX_GENERATED_TRANSFORM_DIAGNOSTICS {
            writeln!(
                output,
                "  PrefabInstance root file {} object {} PathID {}: {:?}",
                prefab_instance.file_index,
                prefab_instance.object_index,
                prefab_instance.path_id,
                resolution
            )?;
            prefab_root_diagnostics += 1;
        }
    }
    writeln!(
        output,
        "PrefabInstance RootGameObject references: {} total, {} resolved, {} null, {} field unavailable, {} unresolved or invalid",
        prefab_root_counts.iter().sum::<usize>(),
        prefab_root_counts[0],
        prefab_root_counts[1],
        prefab_root_counts[2],
        prefab_root_counts[3]
    )?;
    let prefab_processing_plan = plan_workspace_prefab_processing(
        &scene_hierarchies,
        &source_collection_file_indices,
        WorkspacePrefabProcessingLimits::default(),
    )?;
    let mut source_external_identifier_counts = Vec::new();
    source_external_identifier_counts
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_data("could not reserve source external identifier counts"))?;
    source_external_identifier_counts.extend(
        workspace
            .files()
            .iter()
            .map(|file| file.metadata.externals.len()),
    );
    let processed_prefab_assets = plan_workspace_processed_prefab_assets(
        &scene_hierarchies,
        &prefab_processing_plan,
        &source_external_identifier_counts,
        &animator_controller_inventory,
        WorkspaceProcessedPrefabAssetPlanLimits::default(),
    )?;
    let prefab_processing_report = prefab_processing_plan.report();
    let incomplete_hierarchy_count = prefab_processing_report
        .existing_hierarchy_failures
        .checked_add(prefab_processing_report.generated_hierarchy_failures)
        .ok_or_else(|| invalid_data("Prefab hierarchy failure count overflows usize"))?;
    writeln!(
        output,
        "PrefabProcessor identity plan: {} scene GameObjects, {} existing PrefabInstances, {} existing hierarchies, {} existing instances skipped, {} generated hierarchies, {} virtual Transform entries in planned hierarchies, {} remaining GameObjects skipped, {} processed GameObjects, {} hierarchy entries, {} root issues, {} incomplete hierarchies, complete={}, stopped={}",
        prefab_processing_report.scene_game_object_records,
        prefab_processing_report.existing_prefab_instances,
        prefab_processing_report.existing_prefab_hierarchies,
        prefab_processing_report.existing_prefab_instances_skipped,
        prefab_processing_report.generated_prefab_hierarchies,
        prefab_processing_report.generated_transform_components,
        prefab_processing_report.remaining_game_objects_skipped,
        prefab_processing_report.processed_game_objects,
        prefab_processing_report.hierarchy_entries,
        prefab_processing_report.root_resolution_issues,
        incomplete_hierarchy_count,
        prefab_processing_report.complete,
        prefab_processing_report.stopped_on_incomplete_hierarchy
    )?;
    writeln!(
        output,
        "PrefabProcessor generated-asset allocation plan: complete={}, {} collections, {} assets, {} GameObject component-pair appends, {} constructor PPtrs, {} source dependency appends ({} FileIDs assigned, {} deferred across {} source files; {} AnimatorController collection appends modeled){}",
        processed_prefab_assets.is_complete(),
        processed_prefab_assets.collections().len(),
        processed_prefab_assets.asset_count(),
        processed_prefab_assets.component_additions().len(),
        processed_prefab_assets.constructor_pointers().len(),
        processed_prefab_assets.source_dependency_appends().len(),
        processed_prefab_assets
            .source_dependency_appends()
            .iter()
            .filter(|append| append.file_id.is_some())
            .count(),
        processed_prefab_assets
            .source_dependency_appends()
            .iter()
            .filter(|append| append.file_id.is_none())
            .count(),
        processed_prefab_assets.deferred_source_dependency_file_count(),
        processed_prefab_assets.modeled_animator_controller_dependency_append_count(),
        processed_prefab_assets
            .incomplete_reason()
            .map(|reason| format!(", incomplete: {reason}"))
            .unwrap_or_default()
    )?;
    if processed_prefab_assets.is_complete() {
        for collection in processed_prefab_assets.collections() {
            let path_id_range = match (collection.assets.first(), collection.assets.last()) {
                (Some(first), Some(last)) => {
                    format!(
                        "PathID {}..={}",
                        first.identity.path_id, last.identity.path_id
                    )
                }
                _ => "no assigned PathIDs".to_owned(),
            };
            writeln!(
                output,
                "  generated collection {} {:?} {:?}: {} assets, {path_id_range}, {} dependency entries (indexes complete={})",
                collection.collection_index,
                collection.kind,
                collection.name,
                collection.assets.len(),
                collection.dependencies.len(),
                collection.dependency_indices_complete
            )?;
        }
    }
    let mut prefab_processing_diagnostics = 0usize;
    for event in prefab_processing_plan.events() {
        if prefab_processing_diagnostics >= MAX_GENERATED_TRANSFORM_DIAGNOSTICS {
            break;
        }
        let diagnostic = match event {
            WorkspacePrefabProcessingEvent::ExistingPrefabInstanceRootUnavailable {
                prefab_instance,
                resolution,
            } => Some(format!(
                "PrefabInstance {}:{} PathID {} root is indeterminate: {:?}",
                prefab_instance.file_index,
                prefab_instance.object_index,
                prefab_instance.path_id,
                resolution
            )),
            WorkspacePrefabProcessingEvent::RemainingGameObjectRootUnavailable {
                game_object,
                root_walk,
            } => Some(format!(
                "GameObject {}:{} PathID {} GetRoot is indeterminate: {:?}",
                game_object.file_index,
                game_object.object_index,
                game_object.path_id,
                root_walk.issue
            )),
            WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyIncomplete {
                prefab_instance,
                root_game_object,
                hierarchy,
                ..
            } => Some(format!(
                "PrefabInstance {}:{} root {}:{} hierarchy walk is incomplete: {:?}",
                prefab_instance.file_index,
                prefab_instance.object_index,
                root_game_object.file_index,
                root_game_object.object_index,
                hierarchy.issues()
            )),
            WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyIncomplete {
                source_game_object,
                root_game_object,
                hierarchy,
                ..
            } => Some(format!(
                "GameObject {}:{} generated root {}:{} hierarchy walk is incomplete: {:?}",
                source_game_object.file_index,
                source_game_object.object_index,
                root_game_object.file_index,
                root_game_object.object_index,
                hierarchy.issues()
            )),
            _ => None,
        };
        if let Some(diagnostic) = diagnostic {
            writeln!(output, "  {diagnostic}")?;
            prefab_processing_diagnostics += 1;
        }
    }
    writeln!(
        output,
        "SceneDefinitions: {}",
        scene_definitions.definitions().len()
    )?;
    for (scene_index, scene_definition) in scene_definitions.definitions().iter().enumerate() {
        let scene_plan = &scene_definition.plan;
        writeln!(
            output,
            "      SceneDefinition model {scene_index}: name={:?}, path={:?} ({})",
            scene_plan.name, scene_plan.path, scene_plan.path_source
        )?;
        writeln!(
            output,
            "        collection files: {:?}",
            scene_definition.collection_file_indices()
        )?;
        writeln!(
            output,
            "        collection identity: {:?} (workspace file {})",
            scene_plan.collection_name, scene_plan.collection_file_index
        )?;
        writeln!(output, "        scene GUID: {}", scene_plan.guid)?;
        let bundle_assignment = scene_bundle_assignments
            .get(scene_index)
            .ok_or_else(|| invalid_data("scene hierarchy bundle assignment is unavailable"))?;
        writeln!(
            output,
            "        hierarchy AssetBundleName: {}",
            bundle_assignment
                .effective_asset_bundle_name
                .as_deref()
                .unwrap_or("null")
        )?;
        match &bundle_assignment.name {
            assetripper::processing::prefab_processing::WorkspaceSceneHierarchyBundleName::Unset => {
                writeln!(
                    output,
                    "        hierarchy AssetBundleName candidate: unset (no SerializedBundle)"
                )?;
            }
            assetripper::processing::prefab_processing::WorkspaceSceneHierarchyBundleName::BundleName(
                name,
            ) => {
                writeln!(
                    output,
                    "        hierarchy AssetBundleName candidate: bundle {name:?} (no IAssetBundle asset)"
                )?;
            }
            assetripper::processing::prefab_processing::WorkspaceSceneHierarchyBundleName::AssetName(
                name,
            ) => {
                writeln!(
                    output,
                    "        hierarchy AssetBundleName candidate: asset {name:?} (first IAssetBundle in FetchAssets order)"
                )?;
            }
        }
        writeln!(
            output,
            "        GeneratedObjectBase.AssetBundleName setter is a no-op"
        )?;
        if let Some(warning) = &bundle_assignment.streamed_scene_assert_warning {
            writeln!(output, "        hierarchy AssetBundle warning: {warning}")?;
        }
        let scene_hierarchy = scene_hierarchies
            .scenes()
            .get(scene_index)
            .ok_or_else(|| invalid_data("SceneDefinition hierarchy model is unavailable"))?;
        for collection_hierarchy in scene_hierarchy.collections() {
            let scene_asset_count = collection_hierarchy.scene_assets().len();
            let manager_asset_count = collection_hierarchy
                .scene_assets()
                .iter()
                .filter(|asset| {
                    matches!(
                        asset.category,
                        WorkspaceSceneAssetCategory::LevelGameManager(_)
                    )
                })
                .count();
            let prefab_instance_count = collection_hierarchy
                .scene_assets()
                .iter()
                .filter(|asset| asset.category == WorkspaceSceneAssetCategory::PrefabInstance)
                .count();
            let scene_roots_count = collection_hierarchy
                .scene_assets()
                .iter()
                .filter(|asset| asset.category == WorkspaceSceneAssetCategory::SceneRoots)
                .count();
            let transform_candidate_count = collection_hierarchy
                .component_candidates()
                .iter()
                .filter(|candidate| {
                    candidate.evidence == WorkspaceSceneComponentEvidence::TransformClassId
                })
                .count();
            let component_edge_candidate_count = collection_hierarchy
                .component_candidates()
                .iter()
                .filter(|candidate| {
                    candidate.evidence
                        == WorkspaceSceneComponentEvidence::GameObjectComponentReference
                })
                .count();
            let game_object_field_candidate_count = collection_hierarchy
                .component_candidates()
                .iter()
                .filter(|candidate| {
                    candidate.evidence == WorkspaceSceneComponentEvidence::GameObjectPointerField
                })
                .count();
            writeln!(
                output,
                "        hierarchy file {}: {} GameObjects, {} Transforms, {} scene assets ({} managers, {} PrefabInstances, {} SceneRoots), {} component candidates ({} Transform IDs, {} GameObject edges, {} m_GameObject fields), {} missing Transform plans, {} relationships, {} roots, {} graph issues, {} root issues, {} reciprocal issues, scene graph complete={}, reference scans complete={}, overall complete={}",
                collection_hierarchy.collection_file_index(),
                collection_hierarchy.game_object_count(),
                collection_hierarchy.transform_count(),
                scene_asset_count,
                manager_asset_count,
                prefab_instance_count,
                scene_roots_count,
                collection_hierarchy.component_candidates().len(),
                transform_candidate_count,
                component_edge_candidate_count,
                game_object_field_candidate_count,
                collection_hierarchy.missing_transform_plans().len(),
                collection_hierarchy.relationships().len(),
                collection_hierarchy.roots().roots().len(),
                collection_hierarchy.graph_issues().len(),
                collection_hierarchy.roots().issues().len(),
                collection_hierarchy.hierarchy_audit().issues().len(),
                collection_hierarchy.source_graph_complete(),
                collection_hierarchy.source_reference_scans_complete(),
                collection_hierarchy.is_complete()
            )?;
        }
    }
    writeln!(
        output,
        "external requests: {}; newly loaded dependencies: {}; resolved: {}",
        dependencies.requests.len(),
        dependencies.loaded_serialized_files,
        dependencies.is_complete()
    )?;
    Ok(())
}

fn workspace_object_class_id(object: &ObjectInfo) -> Option<i32> {
    object
        .class_id
        .map(i32::from)
        .or_else(|| object.resolved_type_index.map(|_| object.raw_type_id))
}

fn relative_container_collection_name<'a>(virtual_path: &'a str, scope: &str) -> Option<&'a str> {
    let relative_path = virtual_path.strip_prefix(scope)?.strip_prefix('/')?;
    (!relative_path.is_empty()).then_some(relative_path)
}

fn source_bundle_for_collection(
    bundle_index: Option<usize>,
    bundles: &[WorkspaceSerializedBundle],
) -> Result<(usize, &WorkspaceSerializedBundle), io::Error> {
    let index = bundle_index.ok_or_else(|| {
        invalid_data("serialized collection is not attached to a source SerializedBundle")
    })?;
    let bundle = bundles.get(index).ok_or_else(|| {
        invalid_data("serialized collection bundle index is outside the workspace")
    })?;
    Ok((index, bundle))
}

fn remaining_budget_u64(limit: u64, consumed: u64, description: &str) -> Result<u64, io::Error> {
    limit.checked_sub(consumed).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("aggregate {description} exceeds its configured limit"),
        )
    })
}

fn remaining_budget_usize(
    limit: usize,
    consumed: usize,
    description: &str,
) -> Result<usize, io::Error> {
    limit.checked_sub(consumed).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("aggregate {description} exceeds its configured limit"),
        )
    })
}

fn merge_mount_report(
    total: &mut BatchMountReport,
    next: &BatchMountReport,
) -> Result<(), io::Error> {
    total
        .explicit_root_resource_files
        .try_reserve(next.explicit_root_resource_files.len())
        .map_err(|_| invalid_data("explicit ResourceFile metadata allocation failed"))?;
    total.directory_entries_scanned = total
        .directory_entries_scanned
        .checked_add(next.directory_entries_scanned)
        .ok_or_else(|| invalid_data("directory entry count overflows"))?;
    total.discovered_inputs = total
        .discovered_inputs
        .checked_add(next.discovered_inputs)
        .ok_or_else(|| invalid_data("logical input count overflows"))?;
    total.discovered_source_bytes = total
        .discovered_source_bytes
        .checked_add(next.discovered_source_bytes)
        .ok_or_else(|| invalid_data("source input byte count overflows"))?;
    total.completed_inputs = total
        .completed_inputs
        .checked_add(next.completed_inputs)
        .ok_or_else(|| invalid_data("completed input count overflows"))?;
    total.mounted_entries = total
        .mounted_entries
        .checked_add(next.mounted_entries)
        .ok_or_else(|| invalid_data("mounted file count overflows"))?;
    total.mounted_bytes = total
        .mounted_bytes
        .checked_add(next.mounted_bytes)
        .ok_or_else(|| invalid_data("mounted output byte count overflows"))?;
    total.failed_entry_metadata_bytes = total
        .failed_entry_metadata_bytes
        .checked_add(next.failed_entry_metadata_bytes)
        .ok_or_else(|| invalid_data("failed-entry metadata byte count overflows"))?;
    total
        .failed_entries
        .try_reserve(next.failed_entries.len())
        .map_err(|_| invalid_data("failed container entry metadata allocation failed"))?;
    total
        .failed_entries
        .extend(next.failed_entries.iter().cloned());
    total
        .explicit_root_resource_files
        .extend_from_slice(&next.explicit_root_resource_files);
    Ok(())
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn invalid_data_owned(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn print_usage() {
    println!(
        "Usage:\n  assetripper-rs inspect <input-directory> [input-directory ...]\n  assetripper-rs inspect-files <input-file> [input-file ...]\n  assetripper-rs export-project [--delete-existing] [--settings <path>] <output-directory> <input-file-or-directory> [input-file-or-directory ...]\n  assetripper-rs export-object-yaml <serialized-file-index> <path-id> <new-output-yaml> <input-file> [input-file ...]\n  assetripper-rs export-file-yaml <serialized-file-index> <new-output-yaml> <input-file> [input-file ...]\n  assetripper-rs export-texture [--settings <path>] <serialized-file-index> <path-id> <new-output-image> <input-file> [input-file ...]\n  assetripper-rs export-mesh <serialized-file-index> <path-id> <new-output-glb> <input-file> [input-file ...]\n  assetripper-rs export-shader [--shader-export-mode <dummy|yaml|decompile>] [--settings <path>] <serialized-file-index> <path-id> <new-output-shader> <input-file> [input-file ...]\n  assetripper-rs export-audio [--settings <path>] <serialized-file-index> <path-id> <new-output-audio> <input-file> [input-file ...]\n  assetripper-rs save-default-settings [--settings <path>]\n  assetripper-rs delete-default-settings\n  assetripper-rs --help\n  assetripper-rs --version\n\n`inspect` mounts directories into isolated VFS roots. `inspect-files` dispatches explicitly selected file paths, registers top-level ResourceFiles as GameBundle root inputs, and reports the parsed SerializedFiles. `export-project` loads mixed file and directory inputs, resolves cross-file references and streamed resources, and writes processed Scene collections as `.unity` YAML with `DefaultImporter` `.meta`, Material `.mat` YAML with cross-file Shader pointer mapping; Shader collections write `.shader` plus `ShaderImporter` in Dummy/Decompile modes (raw source text wins when present) or `.asset` plus `NativeFormatImporter` and version-gated patches in YAML mode; other routed objects use generic YAML `.asset` files, `ProjectSettings/ProjectVersion.txt`, and the default package manifest. It stages all project outputs before writing and refuses undecoded objects, unresolved pointers, or objects omitted by YAML conversion. The generated Scene hierarchy's effective `AssetBundleName` remains null because its `GeneratedObjectBase` setter is a no-op; its `.meta` bundle-name field is blank. Duplicate BuildSettings paths keep the first Scene output. The output directory must not be empty or named `Desktop`, `Documents`, or `Downloads`; an existing non-empty directory aborts the export unless `--delete-existing` is given, which deletes and recreates it exactly like the reference exporter does after deletion consent. This is still an initial workspace workflow: legacy generated Prefabs and overlays, remaining class-specific exporter/importer families (including complete Material/Shader field coverage), settings application, and remaining post-exporters are open parity work. `export-object-yaml` writes one decoded object as YAML. `export-file-yaml` writes YAML-exportable objects from one SerializedFile in object-table order, resolves streamed Mesh and image-texture bytes, preserves intra-file references through source PathIDs, and refuses unresolved or cross-file references. It reports ImageTexture objects omitted by the baseline when they have no usable image data. `export-texture` decodes one class-28 Texture2D, resolves its inline or streamed image data, and writes a PNG/BMP/TGA image; the output extension selects the format (otherwise the configured settings format, default PNG). `export-mesh` decodes one class-43 Mesh, resolves its vertex data and compressed mesh, and writes a glTF 2.0 binary file with one node per sub-mesh, converting Unity's left-handed coordinates to glTF's right-handed space and rewriting the sub-mesh topology. `export-shader` decodes one class-48 Shader and writes `.shader` text: decompiled script text is written verbatim under every mode, otherwise `dummy` (the default) and `decompile` render the fallback dummy body and `yaml` reports that YAML mode writes a YAML asset instead. `export-audio` decodes one class-83 AudioClip, resolves its inline or streamed audio data, and writes the decoded bytes; tracker audio passes through, `FSB5` reports the external-decoder requirement, and unknown bytes fail with the reference diagnostic. None of the commands replaces an existing output file."
    );
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{
        merge_mount_report, remaining_budget_u64, remaining_budget_usize,
        source_bundle_for_collection, WorkspaceSerializedBundle,
    };
    use assetripper::export::settings::{ExportDefaults, ShaderExportMode};
    use assetripper::export::settings_file::ExportSettingsFile;
    use assetripper::io::dependency::DependencyFilePath;
    use assetripper::io::filesystem::VirtualFileSystem;
    use assetripper::io::platform_file_inventory::PlatformWorkspaceLoadOptions;
    use assetripper::io::platform_structure::{
        detect_platform_structure, PlatformKind, PlatformStructure, PlatformStructureLimits,
    };
    use assetripper::scheme::BatchMountReport;
    use assetripper::scheme::FileKind;
    use assetripper::serialized_file::collection_workspace::{
        WorkspaceAssetCollections, WorkspaceCollectionLimits,
    };
    use assetripper::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
    use assetripper::serialized_file::workspace::{
        SerializedFileDependencyOutcome, SerializedFileWorkspace, SerializedFileWorkspaceLimits,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    /// The built-in `ExportSettings` record, used by project-export tests without a settings file.
    fn default_export_settings() -> ExportSettingsFile {
        ExportSettingsFile::from(ExportDefaults::default())
    }

    /// The `guid:` line of one importer sidecar.
    fn meta_guid(path: &std::path::Path) -> String {
        fs::read_to_string(path)
            .expect("sidecar is readable")
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("sidecar has a GUID")
            .to_owned()
    }

    /// Every file below one directory as portable, sorted project-relative paths.
    fn project_tree(root: &std::path::Path) -> Vec<String> {
        fn walk(root: &std::path::Path, current: &std::path::Path, output: &mut Vec<String>) {
            for entry in fs::read_dir(current).expect("output directory is readable") {
                let entry = entry.expect("directory entry is readable");
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, output);
                } else {
                    let relative = path
                        .strip_prefix(root)
                        .expect("output path is below the export root");
                    output.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut output = Vec::new();
        walk(root, root, &mut output);
        output.sort();
        output
    }

    /// A class-129 `PlayerSettings` object: the `TypeTreeObject { IsPlayerSettings }` manager path.
    fn make_player_settings_file() -> Vec<u8> {
        let mut strings = Vec::new();
        let root_type = push_type_tree_string(&mut strings, "PlayerSettings");
        let empty_name = push_type_tree_string(&mut strings, "");
        let int_type = push_type_tree_string(&mut strings, "int");
        let value_name = push_type_tree_string(&mut strings, "m_Value");
        let nodes = [
            (0u8, root_type, empty_name, -1i32),
            (1, int_type, value_name, 4),
        ];
        let mut payload = Vec::new();
        put_i32_le(&mut payload, 7);
        finish_single_object_file(b"2019.4.0f1\0", 129, &nodes, &strings, &payload, 42)
    }

    #[test]
    fn streamed_scene_bundle_lookup_uses_direct_collections_without_a_scope_path() {
        let bundles = [WorkspaceSerializedBundle {
            scope_path: None,
            name_fixed: None,
            parent_bundle_index: None,
            parent_entry_index: None,
            direct_collection_file_indices: vec![3, 1],
            direct_resource_file_indices: Vec::new(),
        }];

        let (bundle_index, bundle) = source_bundle_for_collection(Some(0), &bundles).unwrap();
        assert_eq!(bundle_index, 0);
        assert_eq!(bundle.scope_path, None);
        assert_eq!(bundle.direct_collection_file_indices, [3, 1]);

        let error = source_bundle_for_collection(None, &bundles).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    struct TestInputDirectory {
        directory: PathBuf,
        files: Vec<PathBuf>,
        subdirectories: Vec<PathBuf>,
    }

    impl TestInputDirectory {
        fn new_root() -> PathBuf {
            loop {
                let index = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let directory = std::env::current_dir()
                    .unwrap()
                    .join("target")
                    .join(format!("cli-input-{}-{index}", std::process::id()));
                match fs::create_dir(&directory) {
                    Ok(()) => return directory,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        panic!(
                            "failed to create test input root {}: {error}",
                            directory.display()
                        )
                    }
                }
            }
        }

        fn new() -> Self {
            let directory = Self::new_root();
            let file = directory.join("main.assets");
            fs::write(&file, make_generation_10_file()).unwrap();
            Self {
                directory,
                files: vec![file],
                subdirectories: Vec::new(),
            }
        }

        fn new_windows_platform() -> Self {
            let directory = Self::new_root();
            let game_name = directory.file_name().unwrap().to_str().unwrap();
            let executable = directory.join(format!("{game_name}.exe"));
            let data_directory = directory.join(format!("{game_name}_Data"));
            fs::create_dir(&data_directory).unwrap();
            fs::write(&executable, b"synthetic executable marker").unwrap();

            let primary = data_directory.join("globalgamemanagers");
            fs::write(
                &primary,
                make_generation_10_file_with_externals(&["shared.assets", "data.unity3d"]),
            )
            .unwrap();
            let container_path = data_directory.join("data.unity3d");
            fs::write(
                &container_path,
                make_webdata_fixture(&[("shared.assets", &make_generation_10_file())]),
            )
            .unwrap();

            Self {
                directory,
                files: vec![executable, primary, container_path],
                subdirectories: vec![data_directory],
            }
        }
    }

    impl Drop for TestInputDirectory {
        fn drop(&mut self) {
            for file in &self.files {
                let _ = fs::remove_file(file);
            }
            for directory in self.subdirectories.iter().rev() {
                let _ = fs::remove_dir(directory);
            }
            let _ = fs::remove_dir(&self.directory);
        }
    }

    fn put_u16_le(output: &mut Vec<u8>, value: u16) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_i32_le(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32_le(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32_be(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn put_i32_le_as_be(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn put_f32_le(output: &mut Vec<u8>, value: f32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn put_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        put_node_with_meta(output, level, type_offset, name_offset, byte_size, index, 0);
    }

    fn put_node_with_meta(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
        meta_flags: u32,
    ) {
        put_node_with_version_and_meta(
            output,
            1,
            level,
            type_offset,
            name_offset,
            byte_size,
            index,
            meta_flags,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn put_node_with_version_and_meta(
        output: &mut Vec<u8>,
        version: u16,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
        meta_flags: u32,
    ) {
        put_u16_le(output, version);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, meta_flags);
    }

    fn insert_vfs_file(filesystem: &mut VirtualFileSystem, path: &str, bytes: &[u8]) {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, bytes).unwrap();
    }

    fn push_asset_bundle_node(
        strings: &mut Vec<u8>,
        nodes: &mut Vec<(u8, u32, u32, i32, i32)>,
        level: u8,
        type_name: &str,
        field_name: &str,
        byte_size: i32,
    ) {
        let type_offset = push_type_tree_string(strings, type_name);
        let name_offset = push_type_tree_string(strings, field_name);
        let index = i32::try_from(nodes.len()).unwrap();
        nodes.push((level, type_offset, name_offset, byte_size, index));
    }

    fn put_asset_bundle_string_payload(output: &mut Vec<u8>, value: &str) {
        let bytes = value.as_bytes();
        put_i32_le(output, i32::try_from(bytes.len()).unwrap());
        output.extend_from_slice(bytes);
        let padding = (4 - output.len() % 4) % 4;
        output.resize(output.len() + padding, 0);
    }

    fn make_asset_bundle_file() -> Vec<u8> {
        make_asset_bundle_file_fixture(false)
    }

    fn make_asset_bundle_file_with_local_reference() -> Vec<u8> {
        make_asset_bundle_file_fixture(true)
    }

    fn make_asset_bundle_file_fixture(include_local_target: bool) -> Vec<u8> {
        let mut strings = Vec::new();
        let mut nodes = Vec::new();
        push_asset_bundle_node(&mut strings, &mut nodes, 0, "AssetBundle", "", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 1, "map", "m_Container", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 2, "Array", "Array", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 3, "int", "size", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 3, "pair", "data", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 4, "string", "first", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 5, "Array", "Array", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 6, "int", "size", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 6, "char", "data", 1);
        push_asset_bundle_node(&mut strings, &mut nodes, 4, "AssetInfo", "second", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 5, "int", "preloadIndex", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 5, "int", "preloadSize", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 5, "PPtr<Object>", "asset", 12);
        push_asset_bundle_node(&mut strings, &mut nodes, 6, "int", "m_FileID", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 6, "SInt64", "m_PathID", 8);
        push_asset_bundle_node(
            &mut strings,
            &mut nodes,
            1,
            "bool",
            "m_IsStreamedSceneAssetBundle",
            1,
        );
        push_asset_bundle_node(&mut strings, &mut nodes, 1, "map", "m_SceneHashes", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 2, "Array", "Array", -1);
        push_asset_bundle_node(&mut strings, &mut nodes, 3, "int", "size", 4);
        push_asset_bundle_node(&mut strings, &mut nodes, 3, "pair", "data", -1);
        for member in ["first", "second"] {
            push_asset_bundle_node(&mut strings, &mut nodes, 4, "string", member, -1);
            push_asset_bundle_node(&mut strings, &mut nodes, 5, "Array", "Array", -1);
            push_asset_bundle_node(&mut strings, &mut nodes, 6, "int", "size", 4);
            push_asset_bundle_node(&mut strings, &mut nodes, 6, "char", "data", 1);
        }

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 142);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in nodes {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, if include_local_target { 2 } else { 1 });
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, 1);
        put_asset_bundle_string_payload(&mut payload, "Assets/Scenes/Other.unity");
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        let container_path_id = if include_local_target { 43_i64 } else { 0 };
        payload.extend_from_slice(&container_path_id.to_le_bytes());
        payload.push(1);
        put_i32_le(&mut payload, 1);
        put_asset_bundle_string_payload(&mut payload, "Assets/Scenes/BundleScene.unity");
        put_asset_bundle_string_payload(&mut payload, "CAB-Scene");

        let mut target_payload = Vec::new();
        if include_local_target {
            put_i32_le(&mut target_payload, 0);
            target_payload.push(0);
            put_i32_le(&mut target_payload, 0);
        }

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 142);
        put_u16_le(&mut metadata, 142);
        put_u16_le(&mut metadata, 0);
        if include_local_target {
            put_i32_le(&mut metadata, 43);
            put_u32_le(&mut metadata, u32::try_from(payload.len()).unwrap());
            put_i32_le(&mut metadata, i32::try_from(target_payload.len()).unwrap());
            put_i32_le(&mut metadata, 142);
            put_u16_le(&mut metadata, 142);
            put_u16_le(&mut metadata, 0);
        }
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let file_size = data_offset + payload.len() + target_payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file.extend_from_slice(&target_payload);
        file
    }

    fn make_resource_manager_file_with_local_reference() -> Vec<u8> {
        let mut manager_strings = Vec::new();
        let mut manager_nodes = Vec::new();
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            0,
            "ResourceManager",
            "",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            1,
            "map",
            "m_Container",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            2,
            "Array",
            "Array",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            3,
            "int",
            "size",
            4,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            3,
            "pair",
            "data",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            4,
            "string",
            "first",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            5,
            "Array",
            "Array",
            -1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            6,
            "int",
            "size",
            4,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            6,
            "char",
            "data",
            1,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            4,
            "PPtr<Object>",
            "second",
            12,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            5,
            "int",
            "m_FileID",
            4,
        );
        push_asset_bundle_node(
            &mut manager_strings,
            &mut manager_nodes,
            5,
            "SInt64",
            "m_PathID",
            8,
        );

        let mut target_strings = Vec::new();
        let mut target_nodes = Vec::new();
        push_asset_bundle_node(&mut target_strings, &mut target_nodes, 0, "Shader", "", -1);
        push_asset_bundle_node(
            &mut target_strings,
            &mut target_nodes,
            1,
            "string",
            "m_Name",
            -1,
        );
        push_asset_bundle_node(
            &mut target_strings,
            &mut target_nodes,
            2,
            "Array",
            "Array",
            -1,
        );
        push_asset_bundle_node(&mut target_strings, &mut target_nodes, 3, "int", "size", 4);
        push_asset_bundle_node(&mut target_strings, &mut target_nodes, 3, "char", "data", 1);

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 2);
        for (class_id, strings, nodes) in [
            (147, manager_strings, manager_nodes),
            (48, target_strings, target_nodes),
        ] {
            put_i32_le(&mut metadata, class_id);
            put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
            put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
            for (level, type_offset, name_offset, byte_size, index) in nodes {
                put_node(
                    &mut metadata,
                    level,
                    type_offset,
                    name_offset,
                    byte_size,
                    index,
                );
            }
            metadata.extend_from_slice(&strings);
        }

        let mut manager_payload = Vec::new();
        put_i32_le(&mut manager_payload, 2);
        for path in ["Textures/inner/wood.shader", "wood.shader"] {
            put_asset_bundle_string_payload(&mut manager_payload, path);
            put_i32_le(&mut manager_payload, 0);
            manager_payload.extend_from_slice(&43_i64.to_le_bytes());
        }
        let mut target_payload = Vec::new();
        put_asset_bundle_string_payload(&mut target_payload, "Wood");

        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 2);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(manager_payload.len()).unwrap());
        put_i32_le(&mut metadata, 147);
        put_u16_le(&mut metadata, 147);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 43);
        put_u32_le(&mut metadata, u32::try_from(manager_payload.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(target_payload.len()).unwrap());
        put_i32_le(&mut metadata, 48);
        put_u16_le(&mut metadata, 48);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let file_size = data_offset + manager_payload.len() + target_payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&manager_payload);
        file.extend_from_slice(&target_payload);
        file
    }

    fn make_generation_10_file() -> Vec<u8> {
        make_generation_10_file_with_externals(&[])
    }

    fn make_generation_10_file_with_externals(external_names: &[&str]) -> Vec<u8> {
        make_generation_10_file_with_pointer_path_id(external_names, 42)
    }

    fn make_generation_10_file_with_pointer_path_id(
        external_names: &[&str],
        pointer_path_id: i32,
    ) -> Vec<u8> {
        make_generation_10_file_with_pointer(external_names, 0, pointer_path_id)
    }

    fn make_generation_10_file_with_pointer(
        external_names: &[&str],
        pointer_file_id: i32,
        pointer_path_id: i32,
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, "Root");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let value_name = push_type_tree_string(&mut string_buffer, "m_Value");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let target_name = push_type_tree_string(&mut string_buffer, "m_Target");
        let pptr_type = push_type_tree_string(&mut string_buffer, "PPtr<GameObject>");
        let file_id_name = push_type_tree_string(&mut string_buffer, "m_FileID");
        let int32_type = push_type_tree_string(&mut string_buffer, "SInt32");
        let path_id_name = push_type_tree_string(&mut string_buffer, "m_PathID");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 5);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        put_node(&mut metadata, 0, root_type, empty_name, -1, 0);
        put_node(&mut metadata, 1, int_type, value_name, 4, 1);
        put_node(&mut metadata, 1, pptr_type, target_name, 8, 2);
        put_node(&mut metadata, 2, int32_type, file_id_name, 4, 3);
        put_node(&mut metadata, 2, int32_type, path_id_name, 4, 4);
        metadata.extend_from_slice(&string_buffer);

        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 12);
        put_i32_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
        for external_name in external_names {
            metadata.push(0);
            for word in [0, 0, 0, 0] {
                put_u32_le(&mut metadata, word);
            }
            put_i32_le(&mut metadata, 0);
            metadata.extend_from_slice(external_name.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let mut payload = Vec::new();
        put_i32_le(&mut payload, 0x1234_5678);
        put_i32_le(&mut payload, pointer_file_id);
        put_i32_le(&mut payload, pointer_path_id);
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_streaming_info_file(path: &str, offset: u64, size: u32) -> Vec<u8> {
        make_streaming_info_file_with_offset_width(path, offset, size, true)
    }

    fn make_streaming_info_file_with_offset_width(
        path: &str,
        offset: u64,
        size: u32,
        offset_is_u64: bool,
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, "Texture2D");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let streaming_info_type = push_type_tree_string(&mut string_buffer, "StreamingInfo");
        let stream_data_name = push_type_tree_string(&mut string_buffer, "m_StreamData");
        let string_type = push_type_tree_string(&mut string_buffer, "string");
        let path_name = push_type_tree_string(&mut string_buffer, "path");
        let array_type = push_type_tree_string(&mut string_buffer, "Array");
        let size_name = push_type_tree_string(&mut string_buffer, "size");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let data_name = push_type_tree_string(&mut string_buffer, "data");
        let char_type = push_type_tree_string(&mut string_buffer, "char");
        let offset_type_name = if offset_is_u64 { "UInt64" } else { "UInt32" };
        let offset_type = push_type_tree_string(&mut string_buffer, offset_type_name);
        let offset_name = push_type_tree_string(&mut string_buffer, "offset");
        let uint32_type = push_type_tree_string(&mut string_buffer, "UInt32");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 28);
        put_i32_le(&mut metadata, 8);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty_name, -1, 0),
            (1, streaming_info_type, stream_data_name, -1, 1),
            (2, string_type, path_name, -1, 2),
            (3, array_type, array_type, -1, 3),
            (4, int_type, size_name, 4, 4),
            (4, char_type, data_name, 1, 5),
            (
                2,
                offset_type,
                offset_name,
                if offset_is_u64 { 8 } else { 4 },
                6,
            ),
            (2, uint32_type, size_name, 4, 7),
        ] {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, i32::try_from(path.len()).unwrap());
        payload.extend_from_slice(path.as_bytes());
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        if offset_is_u64 {
            payload.extend_from_slice(&offset.to_le_bytes());
        } else {
            put_u32_le(&mut payload, u32::try_from(offset).unwrap());
        }
        put_u32_le(&mut payload, size);

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 28);
        put_u16_le(&mut metadata, 28);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_streamed_image_texture_file() -> Vec<u8> {
        make_streamed_image_texture_file_with_stream("texture.resS", 3)
    }

    fn make_streamed_image_texture_file_with_stream(
        stream_path: &str,
        stream_size: u32,
    ) -> Vec<u8> {
        make_streamed_image_texture_file_with_streams(&[(stream_path, 2, stream_size)])
    }

    fn make_streamed_image_texture_file_with_streams(streams: &[(&str, u64, u32)]) -> Vec<u8> {
        make_streamed_texture_file_for_class(
            189,
            "ImageTexture",
            "m_StreamData_C189",
            "m_ImageData_C189",
            "2019.4.0f1",
            true,
            streams,
        )
    }

    fn make_streamed_texture2d_file(stream_path: &str, offset: u64, size: u32) -> Vec<u8> {
        make_streamed_texture_file_for_class(
            28,
            "Texture2D",
            "m_StreamData",
            "image data",
            "2022.3.22f1",
            true,
            &[(stream_path, offset, size)],
        )
    }

    fn make_streamed_texture_file_for_class(
        class_id: i32,
        root_type_name: &str,
        stream_field_name: &str,
        inline_field_name: &str,
        unity_version: &str,
        offset_is_u64: bool,
        streams: &[(&str, u64, u32)],
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, root_type_name);
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let streaming_info_type = push_type_tree_string(&mut string_buffer, "StreamingInfo");
        let stream_data_name = push_type_tree_string(&mut string_buffer, stream_field_name);
        let string_type = push_type_tree_string(&mut string_buffer, "string");
        let path_name = push_type_tree_string(&mut string_buffer, "path");
        let array_type = push_type_tree_string(&mut string_buffer, "Array");
        let size_name = push_type_tree_string(&mut string_buffer, "size");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let data_name = push_type_tree_string(&mut string_buffer, "data");
        let char_type = push_type_tree_string(&mut string_buffer, "char");
        let offset_type_name = if offset_is_u64 { "UInt64" } else { "UInt32" };
        let offset_type = push_type_tree_string(&mut string_buffer, offset_type_name);
        let offset_name = push_type_tree_string(&mut string_buffer, "offset");
        let uint32_type = push_type_tree_string(&mut string_buffer, "UInt32");
        let typeless_data_type = push_type_tree_string(&mut string_buffer, "TypelessData");
        let inline_data_name = push_type_tree_string(&mut string_buffer, inline_field_name);
        let uint8_type = push_type_tree_string(&mut string_buffer, "UInt8");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(unity_version.as_bytes());
        metadata.push(0);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, class_id);
        put_i32_le(&mut metadata, 11);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty_name, -1, 0),
            (1, streaming_info_type, stream_data_name, -1, 1),
            (2, string_type, path_name, -1, 2),
            (3, array_type, array_type, -1, 3),
            (4, int_type, size_name, 4, 4),
            (4, char_type, data_name, 1, 5),
            (
                2,
                offset_type,
                offset_name,
                if offset_is_u64 { 8 } else { 4 },
                6,
            ),
            (2, uint32_type, size_name, 4, 7),
            (1, typeless_data_type, inline_data_name, -1, 8),
            (2, int_type, size_name, 4, 9),
            (2, uint8_type, data_name, 1, 10),
        ] {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(streams.len()).unwrap());

        let mut payload = Vec::new();
        for (object_index, (stream_path, stream_offset, stream_size)) in streams.iter().enumerate()
        {
            let padding = (4 - payload.len() % 4) % 4;
            payload.resize(payload.len() + padding, 0);
            let byte_start = payload.len();
            let mut object_payload = Vec::new();
            put_i32_le(
                &mut object_payload,
                i32::try_from(stream_path.len()).unwrap(),
            );
            object_payload.extend_from_slice(stream_path.as_bytes());
            let padding = (4 - object_payload.len() % 4) % 4;
            object_payload.resize(object_payload.len() + padding, 0);
            if offset_is_u64 {
                object_payload.extend_from_slice(&stream_offset.to_le_bytes());
            } else {
                put_u32_le(
                    &mut object_payload,
                    u32::try_from(*stream_offset).expect("fixture offset fits UInt32"),
                );
            }
            put_u32_le(&mut object_payload, *stream_size);
            put_i32_le(&mut object_payload, 0);

            put_i32_le(&mut metadata, 77 + i32::try_from(object_index).unwrap());
            put_i32_le(&mut metadata, i32::try_from(byte_start).unwrap());
            put_i32_le(&mut metadata, i32::try_from(object_payload.len()).unwrap());
            put_i32_le(&mut metadata, class_id);
            put_u16_le(&mut metadata, class_id as u16);
            put_u16_le(&mut metadata, 0);
            payload.extend_from_slice(&object_payload);
        }
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_texture2d_file() -> Vec<u8> {
        make_texture_file(28, "Texture2D", 2, 1, &[10, 20, 30, 255, 40, 50, 60, 255])
    }

    fn make_texture_file(
        class_id: i32,
        root_type_name: &str,
        width: i32,
        height: i32,
        data: &[u8],
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, root_type_name);
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let width_name = push_type_tree_string(&mut string_buffer, "m_Width");
        let height_name = push_type_tree_string(&mut string_buffer, "m_Height");
        let format_name = push_type_tree_string(&mut string_buffer, "m_TextureFormat");
        let complete_size_name = push_type_tree_string(&mut string_buffer, "m_CompleteImageSize");
        let image_count_name = push_type_tree_string(&mut string_buffer, "m_ImageCount");
        let uint8_type = push_type_tree_string(&mut string_buffer, "UInt8");
        let lightmap_name = push_type_tree_string(&mut string_buffer, "m_LightmapFormat");
        let typeless_data_type = push_type_tree_string(&mut string_buffer, "TypelessData");
        let image_data_name = push_type_tree_string(&mut string_buffer, "image data");
        let size_name = push_type_tree_string(&mut string_buffer, "size");
        let data_name = push_type_tree_string(&mut string_buffer, "data");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, class_id);
        put_i32_le(&mut metadata, 10);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty_name, -1, 0),
            (1, int_type, width_name, 4, 1),
            (1, int_type, height_name, 4, 2),
            (1, int_type, format_name, 4, 3),
            (1, int_type, complete_size_name, 4, 4),
            (1, int_type, image_count_name, 4, 5),
            (1, uint8_type, lightmap_name, 1, 6),
            (1, typeless_data_type, image_data_name, -1, 7),
            (2, int_type, size_name, 4, 8),
            (2, uint8_type, data_name, 1, 9),
        ] {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, width); // m_Width
        put_i32_le(&mut payload, height); // m_Height
        put_i32_le(&mut payload, 4); // m_TextureFormat (RGBA32)
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap()); // m_CompleteImageSize
        put_i32_le(&mut payload, 1); // m_ImageCount
        payload.push(0); // m_LightmapFormat
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap());
        payload.extend_from_slice(data);
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, class_id);
        put_u16_le(&mut metadata, class_id as u16);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_build_settings_file(scene_paths: &[&str]) -> Vec<u8> {
        make_build_settings_file_with_path_id(scene_paths, 42)
    }

    /// Synthetic class-117/187/188 texture file: 1x1(x1) RGBA pixels behind a
    /// `GraphicsFormat` discriminant of 4 (RGBA32 decode). `height` is absent
    /// on class 188 (its height equals its width); `third` is `m_Depth` on 117
    /// and 187, `m_CubemapCount` on 188; `size` is `m_DataSize` (u32) on
    /// 187/188 and `m_CompleteImageSize` (u32 spelling) on 117.
    #[allow(clippy::too_many_arguments)]
    fn make_array_texture_file(
        class_id: i32,
        root_type_name: &str,
        height: Option<i32>,
        third: Option<(&str, i32)>,
        size_name: &str,
        size_is_u32: bool,
        size_value: u32,
        data: &[u8],
    ) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, root_type_name);
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let uint_type = push_type_tree_string(&mut string_buffer, "UInt32");
        let width_name = push_type_tree_string(&mut string_buffer, "m_Width");
        let height_name = push_type_tree_string(&mut string_buffer, "m_Height");
        let third_name = third.map(|(name, _)| push_type_tree_string(&mut string_buffer, name));
        let format_name = push_type_tree_string(&mut string_buffer, "m_Format");
        let size_name = push_type_tree_string(&mut string_buffer, size_name);
        let typeless_data_type = push_type_tree_string(&mut string_buffer, "TypelessData");
        let image_data_name = push_type_tree_string(&mut string_buffer, "m_ImageData");
        let size_field_name = push_type_tree_string(&mut string_buffer, "size");
        let data_field_name = push_type_tree_string(&mut string_buffer, "data");
        let uint8_type = push_type_tree_string(&mut string_buffer, "UInt8");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, class_id);
        let size_type = if size_is_u32 { uint_type } else { int_type };
        let mut array_nodes = vec![(0, root_type, empty_name, -1, 0)];
        array_nodes.push((1, int_type, width_name, 4, 1));
        let mut array_index = 2;
        if height.is_some() {
            array_nodes.push((1, int_type, height_name, 4, array_index));
            array_index += 1;
        }
        if let Some(third) = third_name {
            array_nodes.push((1, int_type, third, 4, array_index));
            array_index += 1;
        }
        array_nodes.push((1, int_type, format_name, 4, array_index));
        array_index += 1;
        array_nodes.push((1, size_type, size_name, 4, array_index));
        array_index += 1;
        array_nodes.push((1, typeless_data_type, image_data_name, -1, array_index));
        array_index += 1;
        array_nodes.push((2, int_type, size_field_name, 4, array_index));
        array_index += 1;
        array_nodes.push((2, uint8_type, data_field_name, 1, array_index));
        put_i32_le(&mut metadata, i32::try_from(array_nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in array_nodes {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, 1); // m_Width
        if let Some(height) = height {
            put_i32_le(&mut payload, height); // m_Height
        }
        if let Some((_, third)) = third {
            put_i32_le(&mut payload, third); // m_Depth or m_CubemapCount
        }
        put_i32_le(&mut payload, 4); // m_Format (GraphicsFormat RGBA32 route)
        if size_is_u32 {
            put_u32_le(&mut payload, size_value);
        } else {
            put_i32_le(&mut payload, size_value as i32);
        }
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap());
        payload.extend_from_slice(data);
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, class_id);
        put_u16_le(&mut metadata, class_id as u16);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }
    fn make_build_settings_file_with_path_id(scene_paths: &[&str], object_path_id: i32) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let build_settings_type = push_type_tree_string(&mut string_buffer, "BuildSettings");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let vector_type = push_type_tree_string(&mut string_buffer, "vector");
        let scenes_field = push_type_tree_string(&mut string_buffer, "m_Scenes");
        let array_type = push_type_tree_string(&mut string_buffer, "Array");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let size_field = push_type_tree_string(&mut string_buffer, "size");
        let string_type = push_type_tree_string(&mut string_buffer, "string");
        let data_field = push_type_tree_string(&mut string_buffer, "data");
        let char_type = push_type_tree_string(&mut string_buffer, "char");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 141);
        put_i32_le(&mut metadata, 8);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, build_settings_type, empty_name, -1, 0),
            (1, vector_type, scenes_field, -1, 1),
            (2, array_type, array_type, -1, 2),
            (3, int_type, size_field, 4, 3),
            (3, string_type, data_field, -1, 4),
            (4, array_type, array_type, -1, 5),
            (5, int_type, size_field, 4, 6),
            (5, char_type, data_field, 1, 7),
        ] {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, object_path_id);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, i32::try_from(scene_paths.len()).unwrap());
        for scene_path in scene_paths {
            let scene_path = scene_path.as_bytes();
            put_i32_le(&mut payload, i32::try_from(scene_path.len()).unwrap());
            payload.extend_from_slice(scene_path);
            let padding = (4 - scene_path.len() % 4) % 4;
            payload.resize(payload.len() + padding, 0);
        }
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 141);
        put_u16_le(&mut metadata, 141);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_occlusion_settings_file(scene_guid: [u32; 4]) -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, "OcclusionCullingSettings");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let guid_type = push_type_tree_string(&mut string_buffer, "GUID");
        let scene_guid_name = push_type_tree_string(&mut string_buffer, "m_SceneGUID");
        let uint32_type = push_type_tree_string(&mut string_buffer, "UInt32");
        let word_names = [
            push_type_tree_string(&mut string_buffer, "m_Data_0_"),
            push_type_tree_string(&mut string_buffer, "m_Data_1_"),
            push_type_tree_string(&mut string_buffer, "m_Data_2_"),
            push_type_tree_string(&mut string_buffer, "m_Data_3_"),
        ];

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 29);
        put_i32_le(&mut metadata, 6);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        put_node(&mut metadata, 0, root_type, empty_name, -1, 0);
        put_node(&mut metadata, 1, guid_type, scene_guid_name, 16, 1);
        for (index, name) in word_names.into_iter().enumerate() {
            put_node(
                &mut metadata,
                2,
                uint32_type,
                name,
                4,
                i32::try_from(index + 2).unwrap(),
            );
        }
        metadata.extend_from_slice(&string_buffer);

        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 16);
        put_i32_le(&mut metadata, 29);
        put_u16_le(&mut metadata, 29);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let mut payload = Vec::new();
        for word in scene_guid {
            put_u32_le(&mut payload, word);
        }
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn put_i64_le(output: &mut Vec<u8>, value: i64) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    /// Single-`GameObject` SerializedFile mirroring the proven layout in
    /// `processing::scene` tests: `m_Name`, `m_IsActive`, and an
    /// `m_Component` PPtr vector whose entries address `(file_id, path_id)`
    /// with `file_id` indexing this file's own external table.
    fn make_inspect_game_object_file(
        name: &str,
        path_id: i32,
        components: &[(i32, i32)],
        external_path: Option<&str>,
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let root_type = push_type_tree_string(&mut strings, "GameObject");
        let empty = push_type_tree_string(&mut strings, "");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let string_type = push_type_tree_string(&mut strings, "string");
        let array = push_type_tree_string(&mut strings, "Array");
        let size = push_type_tree_string(&mut strings, "size");
        let int_type = push_type_tree_string(&mut strings, "int");
        let data = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let active_field = push_type_tree_string(&mut strings, "m_IsActive");
        let bool_type = push_type_tree_string(&mut strings, "bool");
        let component_field = push_type_tree_string(&mut strings, "m_Component");
        let vector_type = push_type_tree_string(&mut strings, "vector");
        let pptr_type = push_type_tree_string(&mut strings, "PPtr<Component>");
        let file_id_field = push_type_tree_string(&mut strings, "m_FileID");
        let path_id_field = push_type_tree_string(&mut strings, "m_PathID");
        let int32_type = push_type_tree_string(&mut strings, "SInt32");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 12);
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty, -1, 0),
            (1, string_type, name_field, -1, 1),
            (2, array, array, -1, 2),
            (3, int_type, size, 4, 3),
            (3, char_type, data, 1, 4),
            (1, bool_type, active_field, 1, 5),
            (1, vector_type, component_field, -1, 6),
            (2, array, array, -1, 7),
            (3, int_type, size, 4, 8),
            (3, pptr_type, data, -1, 9),
            (4, int32_type, file_id_field, 4, 10),
            (4, int32_type, path_id_field, 4, 11),
        ] {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, path_id);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, i32::try_from(name.len()).unwrap());
        payload.extend_from_slice(name.as_bytes());
        let name_padding = (4 - name.len() % 4) % 4;
        payload.resize(payload.len() + name_padding, 0);
        payload.push(1);
        put_i32_le(&mut payload, i32::try_from(components.len()).unwrap());
        for &(file_id, target_path_id) in components {
            put_i32_le(&mut payload, file_id);
            put_i32_le(&mut payload, target_path_id);
        }
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::from(external_path.is_some()));
        if let Some(external_path) = external_path {
            metadata.push(0);
            metadata.extend_from_slice(&[0; 16]);
            put_i32_le(&mut metadata, 0);
            metadata.extend_from_slice(external_path.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        finish_inspect_serialized_file(metadata, payload)
    }

    /// Single-`Transform` SerializedFile with the same node layout as the
    /// `processing::scene` fixture: `m_GameObject`, rotation/position/scale,
    /// `m_Children`, `m_Father`, `m_RootOrder`, and the euler hint.
    #[allow(clippy::too_many_arguments)]
    fn make_inspect_transform_file(
        path_id: i32,
        game_object: (i32, i64),
        children: &[(i32, i64)],
        father: (i32, i64),
        root_order: i32,
        external_names: &[&str],
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let root_type = push_type_tree_string(&mut strings, "Transform");
        let empty = push_type_tree_string(&mut strings, "");
        let pptr_game_object = push_type_tree_string(&mut strings, "PPtr<GameObject>");
        let pptr_transform = push_type_tree_string(&mut strings, "PPtr<Transform>");
        let quaternion_type = push_type_tree_string(&mut strings, "Quaternion");
        let vector3_type = push_type_tree_string(&mut strings, "Vector3");
        let float_type = push_type_tree_string(&mut strings, "float");
        let int_type = push_type_tree_string(&mut strings, "int");
        let vector_type = push_type_tree_string(&mut strings, "vector");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let file_id_field = push_type_tree_string(&mut strings, "m_FileID");
        let file_id_type = push_type_tree_string(&mut strings, "SInt32");
        let path_id_field = push_type_tree_string(&mut strings, "m_PathID");
        let path_id_type = push_type_tree_string(&mut strings, "SInt64");
        let game_object_field = push_type_tree_string(&mut strings, "m_GameObject");
        let local_rotation_field = push_type_tree_string(&mut strings, "m_LocalRotation");
        let local_position_field = push_type_tree_string(&mut strings, "m_LocalPosition");
        let local_scale_field = push_type_tree_string(&mut strings, "m_LocalScale");
        let children_field = push_type_tree_string(&mut strings, "m_Children");
        let father_field = push_type_tree_string(&mut strings, "m_Father");
        let root_order_field = push_type_tree_string(&mut strings, "m_RootOrder");
        let euler_hint_field = push_type_tree_string(&mut strings, "m_LocalEulerAnglesHint");
        let x_field = push_type_tree_string(&mut strings, "x");
        let y_field = push_type_tree_string(&mut strings, "y");
        let z_field = push_type_tree_string(&mut strings, "z");
        let w_field = push_type_tree_string(&mut strings, "w");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");

        let mut nodes = Vec::new();
        let mut add_node = |level, type_offset, name_offset, byte_size: i32| {
            let index = i32::try_from(nodes.len()).unwrap();
            nodes.push((level, type_offset, name_offset, byte_size, index));
        };
        add_node(0, root_type, empty, -1);
        add_node(1, pptr_game_object, game_object_field, -1);
        add_node(2, file_id_type, file_id_field, 4);
        add_node(2, path_id_type, path_id_field, 8);
        add_node(1, quaternion_type, local_rotation_field, 16);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);
        add_node(2, float_type, w_field, 4);
        add_node(1, vector3_type, local_position_field, 12);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);
        add_node(1, vector3_type, local_scale_field, 12);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);
        add_node(1, vector_type, children_field, -1);
        add_node(2, array_type, array_type, -1);
        add_node(3, int_type, size_field, 4);
        add_node(3, pptr_transform, data_field, -1);
        add_node(4, file_id_type, file_id_field, 4);
        add_node(4, path_id_type, path_id_field, 8);
        add_node(1, pptr_transform, father_field, -1);
        add_node(2, file_id_type, file_id_field, 4);
        add_node(2, path_id_type, path_id_field, 8);
        add_node(1, int_type, root_order_field, 4);
        add_node(1, vector3_type, euler_hint_field, 12);
        add_node(2, float_type, x_field, 4);
        add_node(2, float_type, y_field, 4);
        add_node(2, float_type, z_field, 4);

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 4);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in nodes {
            put_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, path_id);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, game_object.0);
        put_i64_le(&mut payload, game_object.1);
        for value in [0.0, 0.0, 0.0, 1.0] {
            put_f32_le(&mut payload, value);
        }
        for value in [0.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
            put_f32_le(&mut payload, value);
        }
        put_i32_le(&mut payload, i32::try_from(children.len()).unwrap());
        for &(file_id, child_path_id) in children {
            put_i32_le(&mut payload, file_id);
            put_i64_le(&mut payload, child_path_id);
        }
        put_i32_le(&mut payload, father.0);
        put_i64_le(&mut payload, father.1);
        put_i32_le(&mut payload, root_order);
        for value in [0.0, 0.0, 0.0] {
            put_f32_le(&mut payload, value);
        }

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 4);
        put_u16_le(&mut metadata, 4);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
        for external_name in external_names {
            metadata.push(0);
            metadata.extend_from_slice(&[0; 16]);
            put_i32_le(&mut metadata, 0);
            metadata.extend_from_slice(external_name.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        finish_inspect_serialized_file(metadata, payload)
    }

    fn finish_inspect_serialized_file(metadata: Vec<u8>, payload: Vec<u8>) -> Vec<u8> {
        let data_offset = 20usize + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn make_webdata_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let header_size = 20usize
            + entries
                .iter()
                .map(|(name, _)| 12usize + name.len())
                .sum::<usize>();
        let mut container = b"UnityWebData1.0\0".to_vec();
        container.extend_from_slice(&i32::try_from(header_size).unwrap().to_le_bytes());
        let mut data_offset = header_size;
        for (name, bytes) in entries {
            container.extend_from_slice(&i32::try_from(data_offset).unwrap().to_le_bytes());
            container.extend_from_slice(&i32::try_from(bytes.len()).unwrap().to_le_bytes());
            container.extend_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
            container.extend_from_slice(name.as_bytes());
            data_offset += bytes.len();
        }
        for (_, bytes) in entries {
            container.extend_from_slice(bytes);
        }
        container
    }

    #[test]
    fn aggregate_remaining_budget_rejects_a_consumed_overrun() {
        assert_eq!(remaining_budget_usize(10, 4, "input count").unwrap(), 6);
        assert!(remaining_budget_usize(10, 11, "input count").is_err());
        assert_eq!(remaining_budget_u64(20, 7, "input bytes").unwrap(), 13);
        assert!(remaining_budget_u64(20, 21, "input bytes").is_err());
    }

    #[test]
    fn aggregate_mount_report_adds_all_counters_with_checked_arithmetic() {
        let mut total = BatchMountReport {
            directory_entries_scanned: 1,
            discovered_inputs: 2,
            discovered_source_bytes: 10,
            completed_inputs: 1,
            mounted_entries: 3,
            mounted_bytes: 12,
            ..BatchMountReport::default()
        };
        let next = BatchMountReport {
            directory_entries_scanned: 4,
            discovered_inputs: 4,
            discovered_source_bytes: 20,
            completed_inputs: 3,
            mounted_entries: 5,
            mounted_bytes: 30,
            ..BatchMountReport::default()
        };
        merge_mount_report(&mut total, &next).unwrap();
        assert_eq!(total.directory_entries_scanned, 5);
        assert_eq!(total.discovered_inputs, 6);
        assert_eq!(total.discovered_source_bytes, 30);
        assert_eq!(total.completed_inputs, 4);
        assert_eq!(total.mounted_entries, 8);
        assert_eq!(total.mounted_bytes, 42);
    }

    #[test]
    fn logical_input_dedup_keeps_first_occurrences_like_select_unique_paths() {
        let inputs = [
            PathBuf::from("game/main.assets"),
            PathBuf::from("game/main.assets"),
            PathBuf::from("game/main.assets.split0"),
            PathBuf::from("other/shared.assets"),
            PathBuf::from("other/shared.assets.split1"),
        ];
        let unique = super::deduplicate_logical_input_paths(&inputs);
        assert_eq!(
            unique,
            [
                PathBuf::from("game/main.assets"),
                PathBuf::from("other/shared.assets"),
            ]
        );
        // Distinct inputs keep their order; exact spellings still compare unequal.
        let distinct = [PathBuf::from("a/x.assets"), PathBuf::from("b/x.assets")];
        assert_eq!(super::deduplicate_logical_input_paths(&distinct), distinct);
    }

    #[test]
    fn directory_inspection_loads_a_synthetic_serialized_file() {
        let input = TestInputDirectory::new();
        super::inspect_directories(std::slice::from_ref(&input.directory)).unwrap();
    }

    #[test]
    fn empty_directory_inspection_reports_unprocessed_empty_bundle() {
        let directory = TestInputDirectory::new_root();
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: Vec::new(),
            subdirectories: Vec::new(),
        };
        let mut output = Vec::new();
        super::inspect_directories_to(std::slice::from_ref(&directory), &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();

        let reference = include_str!("io/cli_inspect_fixture.txt")
            .lines()
            .find(|line| line.starts_with("EMPTY_DIRECTORY|"))
            .expect("source runtime fixture contains the empty directory outcome");
        assert_eq!(
            reference,
            "EMPTY_DIRECTORY|collections=0|processors_requested=0"
        );
        assert!(
            summary.contains(
                "No valid assets were found in the selected directories; processing was skipped."
            ),
            "{summary}"
        );
        drop(input);
    }

    #[test]
    fn resource_only_directory_matches_the_source_no_asset_process_gate() {
        let directory = TestInputDirectory::new_root();
        let resource_path = directory.join("notes.bytes");
        fs::write(&resource_path, b"plain resource bytes").unwrap();
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![resource_path],
            subdirectories: Vec::new(),
        };

        let reference = include_str!("io/cli_inspect_fixture.txt")
            .lines()
            .find(|line| line.starts_with("LOAD|resource_only_directory|"))
            .expect("source runtime fixture contains the resource-only outcome");
        assert_eq!(
            reference,
            "LOAD|resource_only_directory|collections=0|names=|processors_requested=0"
        );
        let mut output = Vec::new();
        super::inspect_directories_to(std::slice::from_ref(&directory), &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(
            summary.contains(
                "No valid assets were found in the selected directories; processing was skipped."
            ),
            "{summary}"
        );
        drop(input);
    }

    #[test]
    fn inspect_cli_rejects_empty_and_missing_directory_inputs() {
        use std::ffi::OsString;

        let source_empty = include_str!("io/cli_inspect_fixture.txt")
            .lines()
            .find(|line| line.starts_with("LOAD|no_paths|"))
            .expect("source runtime fixture contains the no-path error");
        assert_eq!(
            source_empty,
            "LOAD|no_paths|exception=ArgumentException|message=Game files not found (Parameter 'paths')"
        );
        let error = super::run([OsString::from("inspect")].into_iter()).unwrap_err();
        assert_eq!(error.to_string(), "inspect requires at least one input");

        let parent = TestInputDirectory::new_root();
        let missing = parent.join("missing");
        assert!(!missing.exists());
        let source_missing = include_str!("io/cli_inspect_fixture.txt")
            .lines()
            .find(|line| line.starts_with("LOAD|missing_directory|"))
            .expect("source runtime fixture contains the missing-directory error");
        assert_eq!(
            source_missing,
            "LOAD|missing_directory|exception=Exception|message=Neither file nor directory at '$ROOT\\missing' exists"
        );
        let mut output = Vec::new();
        let error =
            super::inspect_directories_to(std::slice::from_ref(&missing), &mut output).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("input '{}' is not a directory", missing.display())
        );
        drop(TestInputDirectory {
            directory: parent,
            files: Vec::new(),
            subdirectories: Vec::new(),
        });
    }

    /// End-to-end discovery differential against the reference `GameStructure`
    /// dump committed in `bundle_discovery_fixture.txt` (probe
    /// `Temp/opencode/bundleprobe2`): the same two C#-constructed UnityFS
    /// bundles are written to a temp dir, inspected, and the summary must show
    /// the reference file/collection/object inventory (both entry namings
    /// discovered, both SerializedFiles parsed, both objects with PathID 8001
    /// decoded). The reference-only injected engine collections stay outside:
    /// the Rust workspace holds exactly the two file collections.
    #[test]
    fn directory_inspection_reproduces_the_reference_bundle_inventory() {
        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "odd hex length");
            let bytes: Vec<u8> = hex
                .as_bytes()
                .chunks(2)
                .map(|pair| {
                    let text = std::str::from_utf8(pair).unwrap();
                    u8::from_str_radix(text, 16).unwrap()
                })
                .collect();
            assert_eq!(bytes.len(), hex.len() / 2);
            bytes
        }

        let fixture = include_str!("io/bundle_discovery_fixture.txt");
        let directory = TestInputDirectory::new_root();
        let mut bundle_names = Vec::new();
        let mut reference_assets = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            match parts[0] {
                "BUNDLE_HEX" => {
                    assert_eq!(parts.len(), 3, "{line}");
                    fs::write(directory.join(parts[1]), decode_hex(parts[2])).unwrap();
                    bundle_names.push(parts[1].to_owned());
                }
                "REF" => {
                    if parts[1] == "ASSET" {
                        reference_assets.push(parts.join("|"));
                    }
                }
                kind => panic!("unknown fixture row {kind}: {line}"),
            }
        }
        assert_eq!(bundle_names.len(), 2, "two reference bundles pin the input");
        assert_eq!(
            reference_assets,
            vec![
                "REF|ASSET|1|8001|GameObject".to_owned(),
                "REF|ASSET|1|8001|GameObject".to_owned(),
            ]
        );
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: bundle_names
                .iter()
                .map(|name| directory.join(name))
                .collect(),
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_directories_to(std::slice::from_ref(&directory), &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();

        assert!(
            summary.contains("platform: mixed; registered files: 2; assemblies: 0"),
            "{summary}"
        );
        assert!(summary.contains("SerializedFiles: 2"), "{summary}");
        assert!(
            summary.contains("Workspace collections: 2 (2 objects, 0 external identifiers)"),
            "{summary}"
        );
        assert!(
            summary.contains("Object payload decode: 2/2 decoded"),
            "{summary}"
        );
        // Collections are named by UnityFS entry name on both sides; indices
        // follow discovery order, so only the names are pinned.
        assert!(summary.contains("\"cab-test\""), "{summary}");
        assert!(summary.contains("\"main.assets\""), "{summary}");
        assert!(summary.contains("PathID 8001"), "{summary}");
        drop(input);
    }

    #[test]
    fn explicit_file_inspection_registers_only_selected_resource_inputs_as_roots() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let second_resource_path = directory.join("second.resS");
        let selected_resource_path = directory.join("direct.resS");
        let unselected_resource_path = directory.join("unselected.resS");
        fs::write(&serialized_path, make_generation_10_file()).unwrap();
        fs::write(&second_resource_path, b"second explicit root stream").unwrap();
        fs::write(&selected_resource_path, b"explicit root stream").unwrap();
        fs::write(&unselected_resource_path, b"not an explicit input").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                second_resource_path.clone(),
                selected_resource_path.clone(),
                unselected_resource_path,
            ],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_explicit_files_to(
            &[
                serialized_path,
                second_resource_path,
                selected_resource_path,
            ],
            &mut output,
        )
        .unwrap();
        let summary = String::from_utf8(output).unwrap();

        assert!(summary.contains("Explicit GameBundle root ResourceFiles: 2"));
        // Root resources enumerate in file-stack pop order (last input first),
        // matching baseline `GameBundle.FromPaths` — proven by the round-60
        // runtime differential on `IO-RESOURCE-RESOLUTION-001`.
        let first_resource_index = summary
            .find("root resource[0]: name=\"direct.resS\"")
            .unwrap();
        let second_resource_index = summary
            .find("root resource[1]: name=\"second.resS\"")
            .unwrap();
        assert!(first_resource_index < second_resource_index);
        assert!(!summary.contains("unselected.resS"));
        assert!(summary.contains("explicit file import: 3 logical inputs"));
        drop(input);
    }

    #[test]
    fn inspection_reports_failed_bundle_entries_and_project_export_refuses_partial_mounts() {
        fn decode_hex(hex: &str) -> Vec<u8> {
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        let row = include_str!("io/io_differential_fixture.txt")
            .lines()
            .find(|line| line.starts_with("SCHEME|ufs_v6_lzham_block|"))
            .expect("reference fixture contains the failed-entry continuation case");
        let input_hex = row
            .split('|')
            .find_map(|field| field.strip_prefix("in="))
            .expect("fixture row contains input bytes");
        let directory = TestInputDirectory::new_root();
        let bundle_path = directory.join("partial.bundle");
        fs::write(&bundle_path, decode_hex(input_hex)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![bundle_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut inspection = Vec::new();
        let inspect_error =
            super::inspect_explicit_files_to(std::slice::from_ref(&bundle_path), &mut inspection)
                .expect_err("the fixture has no SerializedFile or explicit root resource input");
        let inspection = String::from_utf8(inspection).unwrap();
        assert!(
            inspection.contains("UnityFS entries recorded as failed: 1"),
            "{inspection}\n{inspect_error}"
        );
        assert!(inspection.contains("unsupported.bin"), "{inspection}");

        let output_root = input.directory.join("partial-project");
        let mut status = Vec::new();
        let export_error = super::export_project_to(
            std::slice::from_ref(&bundle_path),
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .expect_err("project export must refuse a failed UnityFS entry");
        assert!(
            export_error
                .to_string()
                .contains("refusing to export an incomplete container mount"),
            "{export_error}"
        );
        assert!(!output_root.exists(), "partial project output was created");
        drop(input);
    }

    #[test]
    fn explicit_file_inspection_resolves_decoded_streaming_info_from_selected_root_resource() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("texture.resS");
        fs::write(
            &serialized_path,
            make_streaming_info_file("texture.resS", 2, 3),
        )
        .unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), resource_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_explicit_files_to(&[serialized_path, resource_path], &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();

        assert!(summary.contains(
            "StreamingInfo resources: 1 candidates, 0 unset, 0 bundle ranges, 1 explicit root ranges, 0 external ranges, 0 missing, 0 invalid ranges, 0 resolution errors, 0 scan errors, 0 undecoded objects"
        ));
        assert!(summary.contains("complete=true"));
        assert!(!summary.contains("StreamingInfo diagnostic:"));
        drop(input);
    }

    #[test]
    fn export_object_yaml_writes_streamed_image_texture_and_refuses_overwrite() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("texture.resS");
        let output_path = directory.join("image-texture.yaml");
        fs::write(&serialized_path, make_streamed_image_texture_file()).unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                resource_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_object_yaml_to(
            &[serialized_path.clone(), resource_path.clone()],
            0,
            77,
            &output_path,
            &mut status,
        )
        .unwrap();
        let yaml = fs::read_to_string(&output_path).unwrap();
        assert!(yaml.contains("--- !u!189 &1"));
        assert!(yaml.contains("m_ImageData_C189: 323334"));
        assert!(yaml.contains("offset: 0"));
        assert!(yaml.contains("size: 0"));
        assert!(!yaml.contains("texture.resS"));
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote one YAML object from SerializedFile 0 PathID 77"));

        let original_yaml = yaml;
        let error = super::export_object_yaml_to(
            &[serialized_path, resource_path],
            0,
            77,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_to_string(&output_path).unwrap(), original_yaml);
        drop(input);
    }

    #[test]
    fn export_object_yaml_reports_missing_streamed_resource_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("image-texture.yaml");
        fs::write(&serialized_path, make_streamed_image_texture_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let result =
            super::export_object_yaml_to(&[serialized_path], 0, 77, &output_path, &mut Vec::new());
        assert!(result.is_err());
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_object_yaml_skips_image_texture_without_data_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("image-texture.yaml");
        fs::write(
            &serialized_path,
            make_streamed_image_texture_file_with_stream("", 3),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_object_yaml_to(&[serialized_path], 0, 77, &output_path, &mut status).unwrap();
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Skipped SerializedFile 0 PathID 77"));
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_texture_writes_png_and_refuses_overwrite() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("texture.png");
        fs::write(&serialized_path, make_texture2d_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_texture_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            assetripper::export::settings::ImageExportFormat::Png,
            true,
            &output_path,
            &mut status,
        )
        .unwrap();
        let png = fs::read(&output_path).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a png texture from SerializedFile 0 PathID 42"));

        let original = png;
        let error = super::export_texture_to(
            &[serialized_path],
            0,
            42,
            assetripper::export::settings::ImageExportFormat::Png,
            true,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&output_path).unwrap(), original);
        drop(input);
    }

    #[test]
    fn export_cubemap_skips_the_vertical_flip() {
        // The class-89 payload shares the class-28 fields (1x2 RGBA rows
        // [10,20,30,255] and [40,50,60,255]); as a cubemap the rows keep file
        // order, so the bottom-up BMP tail holds row 1 first as BGRA. A flipped
        // class-28 export of the same bytes would end with row 0 instead.
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("cubemap.bmp");
        fs::write(
            &serialized_path,
            make_texture_file(89, "Cubemap", 1, 2, &[10, 20, 30, 255, 40, 50, 60, 255]),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_texture_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            assetripper::export::settings::ImageExportFormat::Bmp,
            true,
            &output_path,
            &mut status,
        )
        .unwrap();
        let bmp = fs::read(&output_path).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(&bmp[122..], &[60, 50, 40, 255, 30, 20, 10, 255]);
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a bmp texture from SerializedFile 0 PathID 42"));
        drop(input);
    }

    /// Class-117/187/188 textures export through the `GraphicsFormat` route:
    /// each synthetic file carries one RGBA pixel (format discriminant 4) and
    /// must yield a one-pixel PNG.
    #[test]
    fn export_array_textures_write_pngs() {
        let face_pixels: Vec<u8> = (0..6)
            .flat_map(|face| [10 + face, 20 + face, 30 + face, 255])
            .collect();
        for (class_id, root_name, height, third, size_name, size_value, data) in [
            (
                117,
                "Texture3D",
                Some(1),
                Some(("m_Depth", 1)),
                "m_CompleteImageSize",
                4,
                vec![10, 20, 30, 255],
            ),
            (
                187,
                "Texture2DArray",
                Some(1),
                Some(("m_Depth", 1)),
                "m_DataSize",
                4,
                vec![10, 20, 30, 255],
            ),
            (
                188,
                "CubemapArray",
                None,
                Some(("m_CubemapCount", 1)),
                "m_DataSize",
                24,
                face_pixels.clone(),
            ),
        ] {
            let directory = TestInputDirectory::new_root();
            let serialized_path = directory.join("main.assets");
            let output_path = directory.join("texture.png");
            fs::write(
                &serialized_path,
                make_array_texture_file(
                    class_id, root_name, height, third, size_name, true, size_value, &data,
                ),
            )
            .unwrap();
            let input = TestInputDirectory {
                directory,
                files: vec![serialized_path.clone(), output_path.clone()],
                subdirectories: Vec::new(),
            };

            let mut status = Vec::new();
            super::export_texture_to(
                std::slice::from_ref(&serialized_path),
                0,
                42,
                assetripper::export::settings::ImageExportFormat::Png,
                true,
                &output_path,
                &mut status,
            )
            .unwrap();
            let png = fs::read(&output_path).unwrap();
            assert_eq!(
                &png[..8],
                &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
                "class {class_id}"
            );
            drop(input);
        }
    }

    #[test]
    fn export_texture_reports_a_non_texture_object_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("texture.png");
        fs::write(
            &serialized_path,
            make_build_settings_file(&["Assets/A.unity"]),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let result = super::export_texture_to(
            &[serialized_path],
            0,
            42,
            assetripper::export::settings::ImageExportFormat::Png,
            true,
            &output_path,
            &mut Vec::new(),
        );
        assert!(result.is_err());
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_texture_settings_file_selects_the_fallback_format() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let settings_path = directory.join("AssetRipper.Settings.json");
        // The committed modified probe fixture configures BMP output.
        fs::write(
            &settings_path,
            include_str!("export/settings_file_modified.json"),
        )
        .unwrap();
        let output_path = directory.join("texture.bin");
        fs::write(&serialized_path, make_texture2d_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                settings_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        // An unrecognized extension falls back to the configured BMP format.
        let arguments = vec![
            OsString::from("--settings"),
            settings_path.clone().into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        super::export_texture_from_arguments(arguments.into_iter()).unwrap();
        assert_eq!(&fs::read(&output_path).unwrap()[..2], b"BM");
        drop(input);
    }

    #[test]
    fn export_texture_settings_file_prefer_original_falls_back_to_configured() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let settings_path = directory.join("AssetRipper.Settings.json");
        // PNG configured with original-extension preference: bundle-decoded assets
        // carry no original path, so the `.bin` output falls back to PNG.
        fs::write(
            &settings_path,
            "{\"Import\": {\"ScriptContentLevel\": 2, \"StreamingAssetsMode\": 1, \"DefaultVersion\": \"0.0.0a0\", \"TargetVersion\": \"0.0.0a0\"}, \"Processing\": {\"EnablePrefabOutlining\": false, \"EnableStaticMeshSeparation\": true, \"EnableAssetDeduplication\": false, \"RemoveNullableAttributes\": false, \"PublicizeAssemblies\": false, \"BundledAssetsExportMode\": 2}, \"Export\": {\"AudioExportFormat\": 2, \"ImageExportFormat\": 4, \"LightmapTextureExportFormat\": 2, \"ScriptExportMode\": 1, \"ScriptLanguageVersion\": -1, \"ScriptTypesFullyQualified\": false, \"ShaderExportMode\": 0, \"SpriteExportMode\": 0, \"TextExportMode\": 2, \"ExportUnreadableAssets\": false, \"PreferOriginalTextureExtension\": true, \"SaveSettingsToDisk\": false, \"LanguageCode\": null}}",
        )
        .unwrap();
        let output_path = directory.join("texture.bin");
        fs::write(&serialized_path, make_texture2d_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                settings_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        let arguments = vec![
            OsString::from("--settings"),
            settings_path.into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.into_os_string(),
        ];
        super::export_texture_from_arguments(arguments.into_iter()).unwrap();
        assert_eq!(
            &fs::read(&output_path).unwrap()[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
        drop(input);
    }

    #[test]
    fn export_texture_settings_file_problems_are_input_errors() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("texture.bin");
        let bad_path = directory.join("bad.json");
        fs::write(&serialized_path, make_texture2d_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                output_path.clone(),
                bad_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        // A missing settings file names its path.
        let arguments = vec![
            OsString::from("--settings"),
            OsString::from("no-such-file.json"),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        let error = super::export_texture_from_arguments(arguments.into_iter()).unwrap_err();
        assert!(error.to_string().contains("no-such-file.json"), "{error}");
        assert!(!output_path.exists());

        // A malformed settings file is rejected before any export work.
        let settings_path = bad_path;
        fs::write(&settings_path, b"{\"Import\": {}}").unwrap();
        let arguments = vec![
            OsString::from("--settings"),
            settings_path.into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        let error = super::export_texture_from_arguments(arguments.into_iter()).unwrap_err();
        assert!(error.to_string().contains("bad.json"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_shader_settings_file_selects_the_fallback_mode() {
        use std::ffi::OsString;

        // Compiled text (`Program` present): `Simple` does not admit it, so the mode
        // decides between the dummy bytes and the YAML-asset redirect.
        let script = "Shader \"Test/Compiled\" {\n\tSubShader {\n\t\tPass {\n\t\t\tProgram \"vp\" {\n\t\t\t\tSubProgram \"d3d11\" {\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}";
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let settings_path = directory.join("AssetRipper.Settings.json");
        // YAML mode with default versions and language settings.
        fs::write(
            &settings_path,
            "{\"Import\": {\"ScriptContentLevel\": 2, \"StreamingAssetsMode\": 1, \"DefaultVersion\": \"0.0.0a0\", \"TargetVersion\": \"0.0.0a0\"}, \"Processing\": {\"EnablePrefabOutlining\": false, \"EnableStaticMeshSeparation\": true, \"EnableAssetDeduplication\": false, \"RemoveNullableAttributes\": false, \"PublicizeAssemblies\": false, \"BundledAssetsExportMode\": 2}, \"Export\": {\"AudioExportFormat\": 2, \"ImageExportFormat\": 4, \"LightmapTextureExportFormat\": 2, \"ScriptExportMode\": 1, \"ScriptLanguageVersion\": -1, \"ScriptTypesFullyQualified\": false, \"ShaderExportMode\": 1, \"SpriteExportMode\": 0, \"TextExportMode\": 2, \"ExportUnreadableAssets\": false, \"PreferOriginalTextureExtension\": true, \"SaveSettingsToDisk\": false, \"LanguageCode\": null}}",
        )
        .unwrap();
        let output_path = directory.join("shader.shader");
        fs::write(&serialized_path, make_shader_file(script)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                settings_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        // The settings file selects YAML mode, whose text path reports the asset
        // redirect instead of writing dummy bytes.
        let arguments = vec![
            OsString::from("--settings"),
            settings_path.clone().into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        let error = super::export_shader_from_arguments(arguments.into_iter()).unwrap_err();
        assert!(error.to_string().contains("writes a YAML asset"), "{error}");
        assert!(!output_path.exists());

        // An explicit flag beats the settings file on any order.
        let arguments = vec![
            OsString::from("--shader-export-mode"),
            OsString::from("dummy"),
            OsString::from("--settings"),
            settings_path.into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        super::export_shader_from_arguments(arguments.into_iter()).unwrap();
        assert!(output_path.exists());
        drop(input);
    }

    #[test]
    fn export_mesh_writes_a_glb_and_refuses_overwrite() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("mesh.glb");
        fs::write(&serialized_path, make_mesh_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_mesh_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            &output_path,
            &mut status,
        )
        .unwrap();
        let glb = fs::read(&output_path).unwrap();
        assert_eq!(&glb[..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize,
            glb.len(),
            "the declared total length matches the file"
        );
        let status = String::from_utf8(status).unwrap();
        assert!(status.contains("Wrote a GLB with 3 vertices"), "{status}");
        assert!(status.contains("1 sub-meshes"), "{status}");
        assert!(status.contains("3 emitted indices"), "{status}");
        // The JSON chunk carries the mesh name and one SubMesh_0 node.
        let json_length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&glb[20..20 + json_length]).unwrap();
        assert!(json.contains("\"Tri\""), "{json}");
        assert!(json.contains("SubMesh_0"), "{json}");

        let original = glb;
        let error = super::export_mesh_to(&[serialized_path], 0, 42, &output_path, &mut Vec::new())
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&output_path).unwrap(), original);
        drop(input);
    }

    #[test]
    fn export_mesh_reports_a_non_mesh_object_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("mesh.glb");
        fs::write(&serialized_path, make_texture2d_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_mesh_to(&[serialized_path], 0, 42, &output_path, &mut Vec::new())
            .unwrap_err();
        assert!(error.to_string().contains("not a class-43 Mesh"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_mesh_requires_an_existing_path_id() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("mesh.glb");
        fs::write(&serialized_path, make_mesh_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_mesh_to(&[serialized_path], 0, 7, &output_path, &mut Vec::new())
            .unwrap_err();
        assert!(error.to_string().contains("PathID 7"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_shader_writes_simple_bytes_and_refuses_overwrite() {
        use assetripper::export::settings::ShaderExportMode;

        let script = "Shader \"Test/Simple\" {\n\tSubShader {\n\t\tPass {}\n\t}\n}";
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("shader.shader");
        fs::write(&serialized_path, make_shader_file(script)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_shader_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            ShaderExportMode::Dummy,
            &output_path,
            &mut status,
        )
        .unwrap();
        // Decompiled text wins under every mode with the raw script bytes.
        assert_eq!(fs::read(&output_path).unwrap(), script.as_bytes());
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a shader from SerializedFile 0 PathID 42"));

        let original = fs::read(&output_path).unwrap();
        let error = super::export_shader_to(
            &[serialized_path],
            0,
            42,
            ShaderExportMode::Dummy,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&output_path).unwrap(), original);
        drop(input);
    }

    #[test]
    fn export_shader_renders_dummy_for_compiled_scripts() {
        use assetripper::export::settings::ShaderExportMode;

        let script = "Shader \"Test/Prog\" {\n\tSubShader {\n\t\tProgram \"vp\" {\n\t\t}\n\t}\n}";
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("shader.shader");
        fs::write(&serialized_path, make_shader_file(script)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        super::export_shader_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            ShaderExportMode::Decompile,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap();
        let text = fs::read_to_string(&output_path).unwrap();
        // Header slice plus marker plus fallback body plus brace, like the probed rows.
        assert!(text.starts_with("Shader \"Test/Prog\" {\n\t\t//DummyShaderTextExporter\n"));
        assert!(text.contains("\n\tSubShader{\n"));
        assert!(text.ends_with("\t}\n}"));
        drop(input);
    }

    #[test]
    fn export_shader_yaml_mode_reports_asset_not_text() {
        use assetripper::export::settings::ShaderExportMode;

        let script = "Shader \"Test/Prog\" {\n\tSubShader {\n\t\tProgram \"vp\" {\n\t\t}\n\t}\n}";
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("shader.shader");
        fs::write(&serialized_path, make_shader_file(script)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_shader_to(
            &[serialized_path],
            0,
            42,
            ShaderExportMode::Yaml,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("writes a YAML asset"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_shader_reports_a_non_shader_object_without_creating_output() {
        use assetripper::export::settings::ShaderExportMode;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("shader.shader");
        fs::write(
            &serialized_path,
            make_build_settings_file(&["Assets/A.unity"]),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_shader_to(
            &[serialized_path],
            0,
            42,
            ShaderExportMode::Dummy,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("not a class-48 Shader"),
            "{error}"
        );
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_object_yaml_emits_material_fields() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("material.mat");
        fs::write(&serialized_path, make_material_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_object_yaml_to(&[serialized_path], 0, 42, &output_path, &mut status).unwrap();
        let yaml = fs::read_to_string(&output_path).unwrap();
        assert!(yaml.contains("--- !u!21 &1"), "{yaml}");
        assert!(yaml.contains("m_Name"), "{yaml}");
        assert!(yaml.contains("TestMat"), "{yaml}");
        assert!(yaml.contains("m_CustomRenderQueue"), "{yaml}");
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote one YAML object from SerializedFile 0 PathID 42"));
        drop(input);
    }

    #[test]
    fn export_audio_writes_typed_bytes_and_refuses_overwrite() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.wav");
        let data = b"RIFF....";
        fs::write(&serialized_path, make_audio_clip_file(data, Some(20))).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_audio_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut status,
        )
        .unwrap();
        assert_eq!(fs::read(&output_path).unwrap(), data);
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a wav audio from SerializedFile 0 PathID 42"));

        let original = fs::read(&output_path).unwrap();
        let error = super::export_audio_to(
            &[serialized_path],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&output_path).unwrap(), original);
        drop(input);
    }

    #[test]
    fn export_audio_passes_tracker_bytes_through_untouched() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("music.it");
        let data = b"IMPM....";
        fs::write(&serialized_path, make_audio_clip_file(data, None)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_audio_to(
            std::slice::from_ref(&serialized_path),
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut status,
        )
        .unwrap();
        assert_eq!(fs::read(&output_path).unwrap(), data);
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a it audio from SerializedFile 0 PathID 42"));
        drop(input);
    }

    #[test]
    fn export_audio_reports_undecodable_bytes_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.bin");
        fs::write(&serialized_path, make_audio_clip_file(b"ABCD", None)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_audio_to(
            &[serialized_path],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("Failed to convert audio"),
            "{error}"
        );
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_audio_reports_fsb5_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.fsb");
        fs::write(&serialized_path, make_audio_clip_file(b"FSB5....", None)).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_audio_to(
            &[serialized_path],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Fmod5Sharp"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_audio_reports_empty_data_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.wav");
        fs::write(&serialized_path, make_audio_clip_file(&[], Some(20))).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_audio_to(
            &[serialized_path],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("audio data could not be found"),
            "{error}"
        );
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_audio_resolves_streamed_resource_bytes() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("audio.resS");
        let output_path = directory.join("music.it");
        let content = b"IMPM\0\0\0\0";
        fs::write(
            &serialized_path,
            make_streamed_audio_clip_file("audio.resS", 0, 8),
        )
        .unwrap();
        fs::write(&resource_path, content).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                resource_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_audio_to(
            &[serialized_path.clone(), resource_path.clone()],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut status,
        )
        .unwrap();
        assert_eq!(fs::read(&output_path).unwrap(), content);
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote a it audio from SerializedFile 0 PathID 42"));
        drop(input);
    }

    #[test]
    fn export_audio_reports_a_non_audio_object_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.wav");
        fs::write(
            &serialized_path,
            make_build_settings_file(&["Assets/A.unity"]),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::export_audio_to(
            &[serialized_path],
            0,
            42,
            super::SettingsAudioFormat::Default,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("not a class-83 AudioClip"),
            "{error}"
        );
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_audio_settings_file_prefer_wav_reports_conversion_without_creating_output() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let settings_path = directory.join("AssetRipper.Settings.json");
        // The committed modified probe fixture configures `PreferWav` audio output.
        fs::write(
            &settings_path,
            include_str!("export/settings_file_modified.json"),
        )
        .unwrap();
        // Sound type 14 decodes as `ogg`, which `PreferWav` redirects to `wav`.
        fs::write(
            &serialized_path,
            make_audio_clip_file(b"OggS....", Some(14)),
        )
        .unwrap();
        let output_path = directory.join("clip.wav");
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                settings_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        let arguments = vec![
            OsString::from("--settings"),
            settings_path.into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.into_os_string(),
        ];
        let error = super::export_audio_from_arguments(arguments.into_iter()).unwrap_err();
        assert!(error.to_string().contains("Vorbis"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn export_audio_settings_file_prefer_wav_keeps_non_ogg_bytes() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let settings_path = directory.join("AssetRipper.Settings.json");
        fs::write(
            &settings_path,
            include_str!("export/settings_file_modified.json"),
        )
        .unwrap();
        // Sound type 20 decodes as `wav`, so `PreferWav` changes nothing.
        let data = b"RIFF....";
        fs::write(&serialized_path, make_audio_clip_file(data, Some(20))).unwrap();
        let output_path = directory.join("clip.wav");
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                settings_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        let arguments = vec![
            OsString::from("--settings"),
            settings_path.into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.into_os_string(),
        ];
        // `export_audio_from_arguments` reports to stdout internally; the byte-level
        // assertion below carries this test.
        super::export_audio_from_arguments(arguments.into_iter()).unwrap();
        assert_eq!(fs::read(&output_path).unwrap(), data);
        drop(input);
    }

    #[test]
    fn export_audio_settings_file_problems_are_input_errors() {
        use std::ffi::OsString;

        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("clip.wav");
        let bad_path = directory.join("bad.json");
        fs::write(
            &serialized_path,
            make_audio_clip_file(b"RIFF....", Some(20)),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                output_path.clone(),
                bad_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        // A missing settings file names its path.
        let arguments = vec![
            OsString::from("--settings"),
            OsString::from("no-such-file.json"),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        let error = super::export_audio_from_arguments(arguments.into_iter()).unwrap_err();
        assert!(error.to_string().contains("no-such-file.json"), "{error}");
        assert!(!output_path.exists());

        // A malformed settings file is rejected before any export work.
        let settings_path = bad_path;
        fs::write(&settings_path, b"{\"Import\": {}}").unwrap();
        let arguments = vec![
            OsString::from("--settings"),
            settings_path.into_os_string(),
            OsString::from("0"),
            OsString::from("42"),
            output_path.clone().into_os_string(),
            serialized_path.clone().into_os_string(),
        ];
        let error = super::export_audio_from_arguments(arguments.into_iter()).unwrap_err();
        assert!(error.to_string().contains("bad.json"), "{error}");
        assert!(!output_path.exists());
        drop(input);
    }

    #[test]
    fn default_path_settings_absent_file_keeps_builtin_defaults() {
        let directory = TestInputDirectory::new_root();
        let input = TestInputDirectory {
            directory: directory.clone(),
            files: Vec::new(),
            subdirectories: Vec::new(),
        };

        assert!(
            super::load_default_export_settings_from_dir(&directory.join("empty"))
                .unwrap()
                .is_none()
        );
        drop(input);
    }

    #[test]
    fn default_path_settings_present_file_loads_the_export_record() {
        use assetripper::export::settings::AudioExportFormat;
        use assetripper::export::settings::ImageExportFormat;
        use assetripper::export::settings::ShaderExportMode;

        let directory = TestInputDirectory::new_root();
        let subdir = directory.join("exe");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(
            subdir.join("AssetRipper.Settings.json"),
            "{\"Import\": {\"ScriptContentLevel\": 2, \"StreamingAssetsMode\": 1, \"DefaultVersion\": \"0.0.0a0\", \"TargetVersion\": \"0.0.0a0\"}, \"Processing\": {\"EnablePrefabOutlining\": false, \"EnableStaticMeshSeparation\": true, \"EnableAssetDeduplication\": false, \"RemoveNullableAttributes\": false, \"PublicizeAssemblies\": false, \"BundledAssetsExportMode\": 2}, \"Export\": {\"AudioExportFormat\": 3, \"ImageExportFormat\": 0, \"LightmapTextureExportFormat\": 2, \"ScriptExportMode\": 1, \"ScriptLanguageVersion\": -1, \"ScriptTypesFullyQualified\": false, \"ShaderExportMode\": 2, \"SpriteExportMode\": 0, \"TextExportMode\": 2, \"ExportUnreadableAssets\": false, \"PreferOriginalTextureExtension\": true, \"SaveSettingsToDisk\": false, \"LanguageCode\": null}}",
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![subdir.join("AssetRipper.Settings.json")],
            subdirectories: vec![subdir.clone()],
        };

        let export = super::load_default_export_settings_from_dir(&subdir)
            .unwrap()
            .expect("the stored file must load");
        assert_eq!(export.audio_export_format, AudioExportFormat::PreferWav);
        assert_eq!(export.image_export_format, ImageExportFormat::Bmp);
        assert_eq!(export.shader_export_mode, ShaderExportMode::Decompile);
        drop(input);
    }

    #[test]
    fn default_path_settings_broken_file_is_a_path_carrying_error() {
        let directory = TestInputDirectory::new_root();
        let subdir = directory.join("exe");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(
            subdir.join("AssetRipper.Settings.json"),
            b"{\"Import\": {}}",
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![subdir.join("AssetRipper.Settings.json")],
            subdirectories: vec![subdir.clone()],
        };

        let error = super::load_default_export_settings_from_dir(&subdir).unwrap_err();
        assert!(
            error.to_string().contains("AssetRipper.Settings.json"),
            "{error}"
        );
        drop(input);
    }

    #[test]
    fn default_path_save_then_load_round_trips_builtin_defaults() {
        use assetripper::export::settings::{
            ExportDefaults, ImageExportFormat, ImportDefaults, ProcessingDefaults,
        };
        use assetripper::export::settings_file::SerializedSettingsFile;

        let directory = TestInputDirectory::new_root();
        let subdir = directory.join("exe");
        std::fs::create_dir(&subdir).unwrap();
        let input = TestInputDirectory {
            directory,
            files: Vec::new(),
            subdirectories: vec![subdir.clone()],
        };

        let defaults = SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        );
        let path = super::save_serialized_settings_to_dir(&defaults, &subdir).unwrap();
        assert!(path.exists());
        let loaded = super::load_default_export_settings_from_dir(&subdir)
            .unwrap()
            .expect("the stored file must load");
        assert_eq!(loaded.image_export_format, ImageExportFormat::Png);
        // Overwriting is unconditional, mirroring `File.Create`.
        let path_again = super::save_serialized_settings_to_dir(&defaults, &subdir).unwrap();
        assert_eq!(path, path_again);
        drop(input);
    }

    #[test]
    fn default_path_save_explicit_source_stores_the_validated_record() {
        use assetripper::export::settings::ImageExportFormat;

        let directory = TestInputDirectory::new_root();
        let subdir = directory.join("exe");
        std::fs::create_dir(&subdir).unwrap();
        let source = subdir.join("source.json");
        std::fs::write(
            &source,
            "{\"Import\": {\"ScriptContentLevel\": 2, \"StreamingAssetsMode\": 1, \"DefaultVersion\": \"0.0.0a0\", \"TargetVersion\": \"0.0.0a0\"}, \"Processing\": {\"EnablePrefabOutlining\": false, \"EnableStaticMeshSeparation\": true, \"EnableAssetDeduplication\": false, \"RemoveNullableAttributes\": false, \"PublicizeAssemblies\": false, \"BundledAssetsExportMode\": 2}, \"Export\": {\"AudioExportFormat\": 3, \"ImageExportFormat\": 0, \"LightmapTextureExportFormat\": 2, \"ScriptExportMode\": 1, \"ScriptLanguageVersion\": -1, \"ScriptTypesFullyQualified\": false, \"ShaderExportMode\": 2, \"SpriteExportMode\": 0, \"TextExportMode\": 2, \"ExportUnreadableAssets\": false, \"PreferOriginalTextureExtension\": true, \"SaveSettingsToDisk\": false, \"LanguageCode\": null}}",
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![source.clone()],
            subdirectories: vec![subdir.clone()],
        };

        let settings = super::read_serialized_settings_file(&source).unwrap();
        super::save_serialized_settings_to_dir(&settings, &subdir).unwrap();
        let loaded = super::load_default_export_settings_from_dir(&subdir)
            .unwrap()
            .expect("the stored file must load");
        assert_eq!(loaded.image_export_format, ImageExportFormat::Bmp);

        let broken = subdir.join("broken.json");
        std::fs::write(&broken, b"{\"Import\": {}}").unwrap();
        let error = super::read_serialized_settings_file(&broken).unwrap_err();
        assert!(error.to_string().contains("broken.json"), "{error}");
        drop(input);
    }

    #[test]
    fn default_path_delete_removes_present_and_ignores_absent() {
        use assetripper::export::settings::{ExportDefaults, ImportDefaults, ProcessingDefaults};
        use assetripper::export::settings_file::SerializedSettingsFile;

        let directory = TestInputDirectory::new_root();
        let subdir = directory.join("exe");
        std::fs::create_dir(&subdir).unwrap();
        let input = TestInputDirectory {
            directory,
            files: Vec::new(),
            subdirectories: vec![subdir.clone()],
        };

        // Absent is a success without effect, mirroring the source check.
        assert!(!super::delete_serialized_settings_in_dir(&subdir).unwrap());
        let defaults = SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        );
        super::save_serialized_settings_to_dir(&defaults, &subdir).unwrap();
        assert!(super::delete_serialized_settings_in_dir(&subdir).unwrap());
        assert!(super::load_default_export_settings_from_dir(&subdir)
            .unwrap()
            .is_none());
        assert!(!super::delete_serialized_settings_in_dir(&subdir).unwrap());
        drop(input);
    }

    #[test]
    fn cli_classifier_pins_the_settings_persistence_spellings() {
        use super::{classify_cli_command, CliCommand};

        assert_eq!(
            classify_cli_command("save-default-settings"),
            CliCommand::SaveDefaultSettings
        );
        assert_eq!(
            classify_cli_command("delete-default-settings"),
            CliCommand::DeleteDefaultSettings
        );
        assert_eq!(
            classify_cli_command("Save-Default-Settings"),
            CliCommand::Unknown
        );
    }

    /// A class-43 `Mesh` whose vertex data is a single Float32 position channel and whose index
    /// buffer holds one 16-bit triangle.
    fn make_mesh_file() -> Vec<u8> {
        let mut strings = Vec::new();
        let mesh_type = push_type_tree_string(&mut strings, "Mesh");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let vector_type = push_type_tree_string(&mut strings, "vector");
        let sub_meshes_field = push_type_tree_string(&mut strings, "m_SubMeshes");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let sub_mesh_type = push_type_tree_string(&mut strings, "SubMesh");
        let uint32_type = push_type_tree_string(&mut strings, "UInt32");
        let first_byte_field = push_type_tree_string(&mut strings, "firstByte");
        let index_count_field = push_type_tree_string(&mut strings, "indexCount");
        let topology_field = push_type_tree_string(&mut strings, "topology");
        let base_vertex_field = push_type_tree_string(&mut strings, "baseVertex");
        let first_vertex_field = push_type_tree_string(&mut strings, "firstVertex");
        let vertex_count_field = push_type_tree_string(&mut strings, "vertexCount");
        let local_aabb_field = push_type_tree_string(&mut strings, "localAABB");
        let aabb_type = push_type_tree_string(&mut strings, "AABB");
        let center_field = push_type_tree_string(&mut strings, "m_Center");
        let extent_field = push_type_tree_string(&mut strings, "m_Extent");
        let vector3_type = push_type_tree_string(&mut strings, "Vector3");
        let x_field = push_type_tree_string(&mut strings, "x");
        let y_field = push_type_tree_string(&mut strings, "y");
        let z_field = push_type_tree_string(&mut strings, "z");
        let float_type = push_type_tree_string(&mut strings, "float");
        let typeless_data_type = push_type_tree_string(&mut strings, "TypelessData");
        let uint8_type = push_type_tree_string(&mut strings, "UInt8");
        let char_type = push_type_tree_string(&mut strings, "char");
        let index_buffer_field = push_type_tree_string(&mut strings, "m_IndexBuffer");
        let index_format_field = push_type_tree_string(&mut strings, "m_IndexFormat");
        let vertex_data_field = push_type_tree_string(&mut strings, "m_VertexData");
        let vertex_data_type = push_type_tree_string(&mut strings, "VertexData");
        let vertex_count_name = push_type_tree_string(&mut strings, "m_VertexCount");
        let channels_field = push_type_tree_string(&mut strings, "m_Channels");
        let channel_info_type = push_type_tree_string(&mut strings, "ChannelInfo");
        let dimension_field = push_type_tree_string(&mut strings, "dimension");
        let offset_field = push_type_tree_string(&mut strings, "offset");
        let format_field = push_type_tree_string(&mut strings, "format");
        let stream_field = push_type_tree_string(&mut strings, "stream");
        let streams_field = push_type_tree_string(&mut strings, "m_Streams");
        let stream_info_type = push_type_tree_string(&mut strings, "StreamInfo");
        let channel_mask_field = push_type_tree_string(&mut strings, "channelMask");
        let stream_offset_field = push_type_tree_string(&mut strings, "offset");
        let stride_field = push_type_tree_string(&mut strings, "stride");
        let divider_op_field = push_type_tree_string(&mut strings, "dividerOp");
        let uint16_type = push_type_tree_string(&mut strings, "UInt16");
        let frequency_field = push_type_tree_string(&mut strings, "frequency");
        let data_size_field = push_type_tree_string(&mut strings, "m_DataSize");

        let mut nodes: Vec<(u8, u32, u32, i32)> = Vec::new();
        fn push_node(
            nodes: &mut Vec<(u8, u32, u32, i32)>,
            level: u8,
            type_offset: u32,
            name_offset: u32,
            byte_size: i32,
        ) {
            nodes.push((level, type_offset, name_offset, byte_size));
        }

        // Level 0: the Mesh root.
        push_node(&mut nodes, 0, mesh_type, empty_name, -1);
        // Level 1: m_Name (string).
        push_node(&mut nodes, 1, string_type, name_field, -1);
        push_node(&mut nodes, 2, array_type, array_type, -1);
        push_node(&mut nodes, 3, int_type, size_field, 4);
        push_node(&mut nodes, 3, char_type, data_field, 1);
        // Level 1: m_SubMeshes (vector of SubMesh).
        push_node(&mut nodes, 1, vector_type, sub_meshes_field, -1);
        push_node(&mut nodes, 2, array_type, array_type, -1);
        push_node(&mut nodes, 3, int_type, size_field, 4);
        push_node(&mut nodes, 3, sub_mesh_type, data_field, -1);
        push_node(&mut nodes, 4, uint32_type, first_byte_field, 4);
        push_node(&mut nodes, 4, uint32_type, index_count_field, 4);
        push_node(&mut nodes, 4, int_type, topology_field, 4);
        push_node(&mut nodes, 4, uint32_type, base_vertex_field, 4);
        push_node(&mut nodes, 4, uint32_type, first_vertex_field, 4);
        push_node(&mut nodes, 4, uint32_type, vertex_count_field, 4);
        push_node(&mut nodes, 4, aabb_type, local_aabb_field, -1);
        for field in [center_field, extent_field] {
            push_node(&mut nodes, 5, vector3_type, field, -1);
            for component in [x_field, y_field, z_field] {
                push_node(&mut nodes, 6, float_type, component, 4);
            }
        }
        // Level 1: m_IndexBuffer (TypelessData).
        push_node(&mut nodes, 1, typeless_data_type, index_buffer_field, -1);
        push_node(&mut nodes, 2, int_type, size_field, 4);
        push_node(&mut nodes, 2, uint8_type, data_field, 1);
        // Level 1: m_IndexFormat (int).
        push_node(&mut nodes, 1, int_type, index_format_field, 4);
        // Level 1: m_VertexData (VertexData).
        push_node(&mut nodes, 1, vertex_data_type, vertex_data_field, -1);
        push_node(&mut nodes, 2, uint32_type, vertex_count_name, 4);
        push_node(&mut nodes, 2, vector_type, channels_field, -1);
        push_node(&mut nodes, 3, array_type, array_type, -1);
        push_node(&mut nodes, 4, int_type, size_field, 4);
        push_node(&mut nodes, 4, channel_info_type, data_field, -1);
        for field in [dimension_field, offset_field, format_field, stream_field] {
            push_node(&mut nodes, 5, uint8_type, field, 1);
        }
        push_node(&mut nodes, 2, vector_type, streams_field, -1);
        push_node(&mut nodes, 3, array_type, array_type, -1);
        push_node(&mut nodes, 4, int_type, size_field, 4);
        push_node(&mut nodes, 4, stream_info_type, data_field, -1);
        push_node(&mut nodes, 5, uint32_type, channel_mask_field, 4);
        push_node(&mut nodes, 5, uint32_type, stream_offset_field, 4);
        push_node(&mut nodes, 5, uint8_type, stride_field, 1);
        push_node(&mut nodes, 5, uint8_type, divider_op_field, 1);
        push_node(&mut nodes, 5, uint16_type, frequency_field, 2);
        push_node(&mut nodes, 2, typeless_data_type, data_size_field, -1);
        push_node(&mut nodes, 3, int_type, size_field, 4);
        push_node(&mut nodes, 3, uint8_type, data_field, 1);

        let vertex_data: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let mut vertex_bytes = Vec::with_capacity(36);
        for value in vertex_data {
            vertex_bytes.extend_from_slice(&value.to_le_bytes());
        }

        let mut payload = Vec::new();
        // m_Name
        put_i32_le(&mut payload, 3);
        payload.extend_from_slice(b"Tri");
        payload.resize(payload.len() + 1, 0);
        // m_SubMeshes: one sub-mesh with three indices.
        put_i32_le(&mut payload, 1);
        put_u32_le(&mut payload, 0); // firstByte
        put_u32_le(&mut payload, 3); // indexCount
        put_i32_le(&mut payload, 0); // topology (Triangles)
        put_u32_le(&mut payload, 0); // baseVertex
        put_u32_le(&mut payload, 0); // firstVertex
        put_u32_le(&mut payload, 3); // vertexCount
        for _ in 0..6 {
            put_f32_le(&mut payload, 0.0); // m_Center then m_Extent
        }
        // m_IndexBuffer: 0, 1, 2 as 16-bit indices.
        put_i32_le(&mut payload, 6);
        for value in [0u16, 1, 2] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.resize(payload.len() + 2, 0);
        // m_IndexFormat: 16-bit.
        put_i32_le(&mut payload, 0);
        // m_VertexData
        put_u32_le(&mut payload, 3); // m_VertexCount
        put_i32_le(&mut payload, 1); // m_Channels size
        payload.push(3); // dimension
        payload.push(0); // offset
        payload.push(0); // format (Float32)
        payload.push(0); // stream
        put_i32_le(&mut payload, 1); // m_Streams size
        put_u32_le(&mut payload, 1); // channelMask
        put_u32_le(&mut payload, 0); // offset
        payload.push(12); // stride
        payload.push(0); // dividerOp
        put_u16_le(&mut payload, 0); // frequency
        put_i32_le(&mut payload, i32::try_from(vertex_bytes.len()).unwrap());
        payload.extend_from_slice(&vertex_bytes);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 43);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size) in &nodes {
            put_node(
                &mut metadata,
                *level,
                *type_offset,
                *name_offset,
                *byte_size,
                0,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 43);
        put_u16_le(&mut metadata, 43);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut bytes = Vec::with_capacity(file_size);
        put_i32_le_as_be(&mut bytes, i32::try_from(metadata.len()).unwrap());
        put_u32_be(&mut bytes, u32::try_from(file_size).unwrap());
        put_u32_be(&mut bytes, 10);
        put_u32_be(&mut bytes, u32::try_from(data_offset).unwrap());
        bytes.push(0);
        bytes.extend_from_slice(&[0; 3]);
        bytes.extend_from_slice(&metadata);
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn put_typetree_string_value(payload: &mut Vec<u8>, text: &str) {
        put_i32_le(payload, i32::try_from(text.len()).unwrap());
        payload.extend_from_slice(text.as_bytes());
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
    }

    fn finish_single_object_file(
        version: &[u8],
        class_id: i32,
        nodes: &[(u8, u32, u32, i32)],
        strings: &[u8],
        payload: &[u8],
        path_id: i32,
    ) -> Vec<u8> {
        finish_single_object_file_with_externals(
            version,
            class_id,
            nodes,
            strings,
            payload,
            path_id,
            &[],
        )
    }

    fn finish_single_object_file_with_externals(
        version: &[u8],
        class_id: i32,
        nodes: &[(u8, u32, u32, i32)],
        strings: &[u8],
        payload: &[u8],
        path_id: i32,
        external_names: &[&str],
    ) -> Vec<u8> {
        let mut metadata = Vec::new();
        metadata.extend_from_slice(version);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, class_id);
        put_i32_le(&mut metadata, i32::try_from(nodes.len()).unwrap());
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size) in nodes {
            put_node(
                &mut metadata,
                *level,
                *type_offset,
                *name_offset,
                *byte_size,
                0,
            );
        }
        metadata.extend_from_slice(strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, path_id);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, class_id);
        put_u16_le(&mut metadata, u16::try_from(class_id).unwrap());
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
        for external_name in external_names {
            metadata.push(0);
            metadata.extend_from_slice(&[0; 16]);
            put_i32_le(&mut metadata, 0);
            metadata.extend_from_slice(external_name.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut bytes = Vec::with_capacity(file_size);
        put_i32_le_as_be(&mut bytes, i32::try_from(metadata.len()).unwrap());
        put_u32_be(&mut bytes, u32::try_from(file_size).unwrap());
        put_u32_be(&mut bytes, 10);
        put_u32_be(&mut bytes, u32::try_from(data_offset).unwrap());
        bytes.push(0);
        bytes.extend_from_slice(&[0; 3]);
        bytes.extend_from_slice(&metadata);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn finish_single_object_file_with_tpk_tree(
        version: &[u8],
        class_id: i32,
        type_tree: &assetripper::serialized_file::metadata::TypeTree,
        payload: &[u8],
        path_id: i32,
        external_names: &[&str],
    ) -> Vec<u8> {
        let mut metadata = Vec::new();
        metadata.extend_from_slice(version);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, class_id);
        put_i32_le(&mut metadata, i32::try_from(type_tree.nodes.len()).unwrap());
        put_i32_le(
            &mut metadata,
            i32::try_from(type_tree.string_buffer.len()).unwrap(),
        );
        for node in &type_tree.nodes {
            put_node_with_version_and_meta(
                &mut metadata,
                u16::try_from(node.version).unwrap(),
                node.level,
                node.type_string_offset.unwrap(),
                node.name_string_offset.unwrap(),
                node.byte_size,
                node.index,
                node.meta_flags,
            );
        }
        metadata.extend_from_slice(&type_tree.string_buffer);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, path_id);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, class_id);
        put_u16_le(&mut metadata, u16::try_from(class_id).unwrap());
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, i32::try_from(external_names.len()).unwrap());
        for external_name in external_names {
            metadata.push(0);
            metadata.extend_from_slice(&[0; 16]);
            put_i32_le(&mut metadata, 0);
            metadata.extend_from_slice(external_name.as_bytes());
            metadata.push(0);
        }
        metadata.push(0);

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + payload.len();
        let mut bytes = Vec::with_capacity(file_size);
        put_i32_le_as_be(&mut bytes, i32::try_from(metadata.len()).unwrap());
        put_u32_be(&mut bytes, u32::try_from(file_size).unwrap());
        put_u32_be(&mut bytes, 10);
        put_u32_be(&mut bytes, u32::try_from(data_offset).unwrap());
        bytes.push(0);
        bytes.extend_from_slice(&[0; 3]);
        bytes.extend_from_slice(&metadata);
        bytes.extend_from_slice(payload);
        bytes
    }

    /// Offsets into the synthetic TypeTree string buffer for `string` nodes.
    struct StringTreeTable {
        string_type: u32,
        array_type: u32,
        int_type: u32,
        size_field: u32,
        char_type: u32,
        data_field: u32,
    }

    fn string_tree_nodes(
        nodes: &mut Vec<(u8, u32, u32, i32)>,
        level: u8,
        table: &StringTreeTable,
        name: u32,
    ) {
        nodes.push((level, table.string_type, name, -1));
        nodes.push((level + 1, table.array_type, table.array_type, -1));
        nodes.push((level + 2, table.int_type, table.size_field, 4));
        nodes.push((level + 2, table.char_type, table.data_field, 1));
    }

    fn make_shader_file(script: &str) -> Vec<u8> {
        make_shader_file_with_path_id(script, 42)
    }

    fn make_shader_file_with_path_id(script: &str, path_id: i32) -> Vec<u8> {
        let mut strings = Vec::new();
        let shader_type = push_type_tree_string(&mut strings, "Shader");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let script_field = push_type_tree_string(&mut strings, "m_Script");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");

        let mut nodes = Vec::new();
        nodes.push((0, shader_type, empty_name, -1));
        let table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &table, name_field);
        string_tree_nodes(&mut nodes, 1, &table, script_field);

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "Test/Simple");
        put_typetree_string_value(&mut payload, script);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file(b"5.0.0f1\0", 48, &nodes, &strings, &payload, path_id)
    }

    fn make_shader_file_with_non_modifiable_texture_null() -> Vec<u8> {
        make_shader_file_with_non_modifiable_texture_pointer(0, 0, &[])
    }

    fn make_shader_file_with_non_modifiable_texture_pointer(
        file_id: i32,
        path_id: i64,
        external_names: &[&str],
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let shader_type = push_type_tree_string(&mut strings, "Shader");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let script_field = push_type_tree_string(&mut strings, "m_Script");
        let non_modifiable_field = push_type_tree_string(&mut strings, "m_NonModifiableTextures");
        let map_type = push_type_tree_string(&mut strings, "map");
        let pair_type = push_type_tree_string(&mut strings, "pair");
        let first_field = push_type_tree_string(&mut strings, "first");
        let second_field = push_type_tree_string(&mut strings, "second");
        let pptr_texture_type = push_type_tree_string(&mut strings, "PPtr<Texture>");
        let file_id_field = push_type_tree_string(&mut strings, "m_FileID");
        let path_id_type = push_type_tree_string(&mut strings, "SInt64");
        let path_id_field = push_type_tree_string(&mut strings, "m_PathID");

        let mut nodes = vec![(0, shader_type, empty_name, -1)];
        let string_table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &string_table, name_field);
        string_tree_nodes(&mut nodes, 1, &string_table, script_field);
        nodes.extend([
            (1, map_type, non_modifiable_field, -1),
            (2, array_type, array_type, -1),
            (3, int_type, size_field, 4),
            (3, pair_type, data_field, -1),
        ]);
        string_tree_nodes(&mut nodes, 4, &string_table, first_field);
        nodes.extend([
            (4, pptr_texture_type, second_field, -1),
            (5, int_type, file_id_field, 4),
            (5, path_id_type, path_id_field, 8),
        ]);

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "Test/Simple");
        put_typetree_string_value(&mut payload, "Shader \"Test/Simple\" { }\n");
        put_i32_le(&mut payload, 1);
        put_typetree_string_value(&mut payload, "_MainTex");
        put_i32_le(&mut payload, file_id);
        put_i64_le(&mut payload, path_id);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file_with_externals(
            b"2018.2.21f1\0",
            48,
            &nodes,
            &strings,
            &payload,
            42,
            external_names,
        )
    }

    fn make_minimal_texture_file(name: &str, path_id: i32) -> Vec<u8> {
        let mut strings = Vec::new();
        let texture_type = push_type_tree_string(&mut strings, "Texture2D");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let char_type = push_type_tree_string(&mut strings, "char");
        let data_field = push_type_tree_string(&mut strings, "data");
        let mut nodes = vec![(0, texture_type, empty_name, -1)];
        string_tree_nodes(
            &mut nodes,
            1,
            &StringTreeTable {
                string_type,
                array_type,
                int_type,
                size_field,
                char_type,
                data_field,
            },
            name_field,
        );
        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, name);
        finish_single_object_file(b"2018.2.21f1\0", 28, &nodes, &strings, &payload, path_id)
    }

    fn make_material_file() -> Vec<u8> {
        let mut strings = Vec::new();
        let material_type = push_type_tree_string(&mut strings, "Material");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let queue_field = push_type_tree_string(&mut strings, "m_CustomRenderQueue");

        let mut nodes = Vec::new();
        nodes.push((0, material_type, empty_name, -1));
        let table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &table, name_field);
        nodes.push((1, int_type, queue_field, 4));

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestMat");
        put_i32_le(&mut payload, -1);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file(b"2019.4.0f1\0", 21, &nodes, &strings, &payload, 42)
    }

    fn make_material_file_with_shader(shader_path_id: i32, external_name: &str) -> Vec<u8> {
        let mut strings = Vec::new();
        let material_type = push_type_tree_string(&mut strings, "Material");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let pptr_shader_type = push_type_tree_string(&mut strings, "PPtr<Shader>");
        let shader_field = push_type_tree_string(&mut strings, "m_Shader");
        let int_type = push_type_tree_string(&mut strings, "int");
        let file_id_field = push_type_tree_string(&mut strings, "m_FileID");
        let path_id_type = push_type_tree_string(&mut strings, "SInt32");
        let path_id_field = push_type_tree_string(&mut strings, "m_PathID");
        let queue_field = push_type_tree_string(&mut strings, "m_CustomRenderQueue");

        let mut nodes = vec![(0, material_type, empty_name, -1)];
        let string_table = StringTreeTable {
            string_type,
            array_type: push_type_tree_string(&mut strings, "Array"),
            int_type,
            size_field: push_type_tree_string(&mut strings, "size"),
            char_type: push_type_tree_string(&mut strings, "char"),
            data_field: push_type_tree_string(&mut strings, "data"),
        };
        string_tree_nodes(&mut nodes, 1, &string_table, name_field);
        nodes.extend([
            (1, pptr_shader_type, shader_field, -1),
            (2, int_type, file_id_field, 4),
            (2, path_id_type, path_id_field, 4),
            (1, int_type, queue_field, 4),
        ]);

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestMat");
        put_i32_le(&mut payload, 1);
        put_i32_le(&mut payload, shader_path_id);
        put_i32_le(&mut payload, -1);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file_with_externals(
            b"2019.4.0f1\0",
            21,
            &nodes,
            &strings,
            &payload,
            42,
            &[external_name],
        )
    }

    fn material_2018_tpk_release_tree() -> assetripper::serialized_file::metadata::TypeTree {
        material_tpk_release_tree(
            include_str!("export/material_tpk_2018_fixture.txt"),
            "TPK|21|2018.2.21f1|release",
        )
    }

    fn material_2018_3_tpk_release_tree() -> assetripper::serialized_file::metadata::TypeTree {
        material_tpk_release_tree(
            include_str!("export/material_tpk_2018_3_fixture.txt"),
            "TPK|21|2018.3.0f1|release",
        )
    }

    fn material_2019_4_tpk_release_tree() -> assetripper::serialized_file::metadata::TypeTree {
        material_tpk_release_tree(
            include_str!("export/material_tpk_2019_4_fixture.txt"),
            "TPK|21|2019.4.40f1|release",
        )
    }

    fn material_2017_4_tpk_release_tree() -> assetripper::serialized_file::metadata::TypeTree {
        material_tpk_release_tree(
            include_str!("export/material_tpk_2017_4_fixture.txt"),
            "TPK|21|2017.4.40f1|release",
        )
    }

    fn material_tpk_release_tree(
        fixture: &str,
        release_header: &str,
    ) -> assetripper::serialized_file::metadata::TypeTree {
        let mut nodes = Vec::new();
        let mut string_buffer = Vec::new();
        let mut in_release_tree = false;
        for line in fixture.lines() {
            if line.starts_with(release_header) {
                in_release_tree = true;
                continue;
            }
            if line.starts_with("TPK|") {
                in_release_tree = false;
                continue;
            }
            if !in_release_tree {
                continue;
            }
            let Some(row) = line.strip_prefix("NODE|") else {
                continue;
            };
            let fields = row.split('|').collect::<Vec<_>>();
            assert_eq!(
                fields.len(),
                5,
                "invalid committed Material TPK row: {line}"
            );
            let level = fields[0].parse::<u8>().unwrap();
            let type_name = fields[1];
            let name = fields[2];
            let version = fields[3].strip_prefix('v').unwrap().parse::<i32>().unwrap();
            let meta_flags = fields[4]
                .strip_prefix("meta")
                .unwrap()
                .parse::<u32>()
                .unwrap();
            let type_string_offset = u32::try_from(string_buffer.len()).unwrap();
            string_buffer.extend_from_slice(type_name.as_bytes());
            string_buffer.push(0);
            let name_string_offset = u32::try_from(string_buffer.len()).unwrap();
            string_buffer.extend_from_slice(name.as_bytes());
            string_buffer.push(0);
            let byte_size = match type_name {
                "char" | "UInt8" => 1,
                "int" | "unsigned int" | "UInt32" | "SInt32" | "float" => 4,
                "SInt64" | "UInt64" => 8,
                _ => -1,
            };
            nodes.push(assetripper::serialized_file::metadata::TypeTreeNode {
                level,
                version,
                type_flags: 0,
                type_string_offset: Some(type_string_offset),
                name_string_offset: Some(name_string_offset),
                type_name: Some(type_name.to_owned()),
                name: Some(name.to_owned()),
                byte_size,
                index: -1,
                meta_flags,
                reference_type_hash: None,
            });
        }
        assert_eq!(
            nodes.first().and_then(|node| node.type_name.as_deref()),
            Some("Material")
        );
        assetripper::serialized_file::metadata::TypeTree {
            nodes,
            string_buffer,
        }
    }

    fn make_material_file_with_reference_2018_tpk() -> Vec<u8> {
        make_material_file_with_reference_2018_tpk_and_properties(false)
    }

    fn make_material_file_with_reference_2018_tpk_and_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_2018_tpk_and_properties(true)
    }

    fn make_material_file_with_reference_2018_tpk_and_properties(
        include_saved_properties: bool,
    ) -> Vec<u8> {
        make_material_file_with_reference_2018_tpk_and_property_count(
            usize::from(include_saved_properties),
            false,
        )
    }

    fn make_material_file_with_reference_2018_tpk_with_multiple_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_2018_tpk_and_property_count(2, false)
    }

    fn make_material_file_with_reference_2018_tpk_with_duplicate_saved_property_keys() -> Vec<u8> {
        make_material_file_with_reference_2018_tpk_and_property_count(2, true)
    }

    fn make_material_file_with_reference_2018_tpk_and_property_count(
        saved_property_count: usize,
        duplicate_keys: bool,
    ) -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties(
            b"2018.2.21f1\0",
            material_2018_tpk_release_tree(),
            saved_property_count,
            duplicate_keys,
        )
    }

    fn make_material_file_with_reference_2018_3_tpk_and_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties(
            b"2018.3.0f1\0",
            material_2018_3_tpk_release_tree(),
            1,
            false,
        )
    }

    fn make_material_file_with_reference_2019_4_tpk_and_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties(
            b"2019.4.40f1\0",
            material_2019_4_tpk_release_tree(),
            1,
            false,
        )
    }

    fn make_material_file_with_reference_2017_4_tpk_and_saved_properties() -> Vec<u8> {
        make_material_file_with_reference_tpk_and_properties(
            b"2017.4.40f1\0",
            material_2017_4_tpk_release_tree(),
            1,
            false,
        )
    }

    fn make_material_file_with_reference_tpk_and_properties(
        version: &[u8],
        type_tree: assetripper::serialized_file::metadata::TypeTree,
        saved_property_count: usize,
        duplicate_keys: bool,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "MaterialFixture");
        put_i32_le(&mut payload, 1);
        put_i64_le(&mut payload, 42);
        put_typetree_string_value(&mut payload, "_FEATURE_A _FEATURE_B");
        put_u32_le(&mut payload, 4);
        payload.push(1);
        payload.push(1);
        while !payload.len().is_multiple_of(4) {
            payload.push(0);
        }
        put_i32_le(&mut payload, 2450);
        put_i32_le(&mut payload, 1);
        put_typetree_string_value(&mut payload, "RenderType");
        put_typetree_string_value(&mut payload, "TransparentCutout");
        put_i32_le(&mut payload, 0); // disabledShaderPasses
        if saved_property_count > 0 {
            let map_length = i32::try_from(saved_property_count).unwrap();
            put_i32_le(&mut payload, map_length);
            put_typetree_string_value(&mut payload, "_MainTex");
            put_i32_le(&mut payload, 0);
            put_i64_le(&mut payload, 0);
            put_f32_le(&mut payload, 1.25);
            put_f32_le(&mut payload, -2.5);
            put_f32_le(&mut payload, 0.125);
            put_f32_le(&mut payload, 0.75);
            if saved_property_count > 1 {
                put_typetree_string_value(
                    &mut payload,
                    if duplicate_keys {
                        "_MainTex"
                    } else {
                        "_ATexture"
                    },
                );
                put_i32_le(&mut payload, 0);
                put_i64_le(&mut payload, 0);
                put_f32_le(&mut payload, 2.0);
                put_f32_le(&mut payload, 0.5);
                put_f32_le(&mut payload, -0.25);
                put_f32_le(&mut payload, 0.125);
            }

            put_i32_le(&mut payload, map_length);
            put_typetree_string_value(
                &mut payload,
                if saved_property_count > 1 && !duplicate_keys {
                    "_ZFloat"
                } else {
                    "_Glossiness"
                },
            );
            put_f32_le(&mut payload, 0.375);
            if saved_property_count > 1 {
                put_typetree_string_value(
                    &mut payload,
                    if duplicate_keys {
                        "_Glossiness"
                    } else {
                        "_AFloat"
                    },
                );
                put_f32_le(&mut payload, -0.125);
            }

            put_i32_le(&mut payload, map_length);
            put_typetree_string_value(
                &mut payload,
                if saved_property_count > 1 && !duplicate_keys {
                    "_ZColor"
                } else {
                    "_Color"
                },
            );
            put_f32_le(&mut payload, 0.125);
            put_f32_le(&mut payload, 0.25);
            put_f32_le(&mut payload, 0.5);
            put_f32_le(&mut payload, 0.75);
            if saved_property_count > 1 {
                put_typetree_string_value(
                    &mut payload,
                    if duplicate_keys { "_Color" } else { "_AColor" },
                );
                put_f32_le(&mut payload, 1.0);
                put_f32_le(&mut payload, 0.5);
                put_f32_le(&mut payload, 0.25);
                put_f32_le(&mut payload, 1.0);
            }
        } else {
            put_i32_le(&mut payload, 0);
            put_i32_le(&mut payload, 0);
            put_i32_le(&mut payload, 0);
        }
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file_with_tpk_tree(
            version,
            21,
            &type_tree,
            &payload,
            42,
            &["shader.assets"],
        )
    }

    fn make_audio_clip_file(data: &[u8], sound_type: Option<i32>) -> Vec<u8> {
        let mut strings = Vec::new();
        let clip_type = push_type_tree_string(&mut strings, "AudioClip");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let typeless_type = push_type_tree_string(&mut strings, "TypelessData");
        let audio_data_field = push_type_tree_string(&mut strings, "m_AudioData");
        let uint8_type = push_type_tree_string(&mut strings, "UInt8");
        let type_field = push_type_tree_string(&mut strings, "m_Type");

        let mut nodes = Vec::new();
        nodes.push((0, clip_type, empty_name, -1));
        let table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &table, name_field);
        nodes.push((1, typeless_type, audio_data_field, -1));
        nodes.push((2, int_type, size_field, 4));
        nodes.push((2, uint8_type, data_field, 1));
        if sound_type.is_some() {
            nodes.push((1, int_type, type_field, 4));
        }

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestClip");
        put_i32_le(&mut payload, i32::try_from(data.len()).unwrap());
        payload.extend_from_slice(data);
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        if let Some(sound_type) = sound_type {
            put_i32_le(&mut payload, sound_type);
        }
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file(b"3.5.0f1\0", 83, &nodes, &strings, &payload, 42)
    }

    fn make_streamed_audio_clip_file(
        stream_path: &str,
        stream_offset: u64,
        stream_size: u32,
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let clip_type = push_type_tree_string(&mut strings, "AudioClip");
        let empty_name = push_type_tree_string(&mut strings, "");
        let string_type = push_type_tree_string(&mut strings, "string");
        let name_field = push_type_tree_string(&mut strings, "m_Name");
        let array_type = push_type_tree_string(&mut strings, "Array");
        let int_type = push_type_tree_string(&mut strings, "int");
        let size_field = push_type_tree_string(&mut strings, "size");
        let data_field = push_type_tree_string(&mut strings, "data");
        let char_type = push_type_tree_string(&mut strings, "char");
        let resource_type = push_type_tree_string(&mut strings, "StreamingInfo");
        let resource_field = push_type_tree_string(&mut strings, "m_Resource");
        let path_field = push_type_tree_string(&mut strings, "path");
        let offset_type = push_type_tree_string(&mut strings, "UInt64");
        let offset_field = push_type_tree_string(&mut strings, "offset");
        let uint32_type = push_type_tree_string(&mut strings, "UInt32");

        let mut nodes = Vec::new();
        nodes.push((0, clip_type, empty_name, -1));
        let table = StringTreeTable {
            string_type,
            array_type,
            int_type,
            size_field,
            char_type,
            data_field,
        };
        string_tree_nodes(&mut nodes, 1, &table, name_field);
        nodes.push((1, resource_type, resource_field, -1));
        string_tree_nodes(&mut nodes, 2, &table, path_field);
        nodes.push((2, offset_type, offset_field, 8));
        nodes.push((2, uint32_type, size_field, 4));

        let mut payload = Vec::new();
        put_typetree_string_value(&mut payload, "TestClip");
        put_typetree_string_value(&mut payload, stream_path);
        payload.extend_from_slice(&stream_offset.to_le_bytes());
        put_u32_le(&mut payload, stream_size);
        assert!(payload.len().is_multiple_of(4), "payload is 4-byte aligned");
        finish_single_object_file(b"2019.4.0f1\0", 83, &nodes, &strings, &payload, 42)
    }

    #[test]
    fn export_file_yaml_preserves_object_order_and_streamed_resources() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("texture.resS");
        let output_path = directory.join("serialized-file.yaml");
        let serialized_bytes = make_streamed_image_texture_file_with_streams(&[
            ("texture.resS", 1, 2),
            ("texture.resS", 6, 2),
        ]);
        fs::write(&serialized_path, &serialized_bytes).unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![
                serialized_path.clone(),
                resource_path.clone(),
                output_path.clone(),
            ],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_file_yaml_to(
            &[serialized_path.clone(), resource_path.clone()],
            0,
            &output_path,
            &mut status,
        )
        .unwrap();
        let yaml = fs::read_to_string(&output_path).unwrap();
        let first_object = yaml.find("--- !u!189 &77").unwrap();
        let second_object = yaml.find("--- !u!189 &78").unwrap();
        assert!(first_object < second_object);
        assert!(yaml.contains("m_ImageData_C189: 3132"));
        assert!(yaml.contains("m_ImageData_C189: 3637"));
        assert!(!yaml.contains("texture.resS"));
        assert_eq!(fs::read(&serialized_path).unwrap(), serialized_bytes);
        assert_eq!(fs::read(&resource_path).unwrap(), b"0123456789");
        assert!(String::from_utf8(status)
            .unwrap()
            .contains("Wrote 2 ordered YAML objects from SerializedFile 0"));

        let original_yaml = yaml;
        let error = super::export_file_yaml_to(
            &[serialized_path, resource_path],
            0,
            &output_path,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_to_string(&output_path).unwrap(), original_yaml);
        drop(input);
    }

    #[test]
    fn export_file_yaml_maps_intra_file_references_to_exported_path_ids() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("serialized-file.yaml");
        fs::write(&serialized_path, make_generation_10_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        super::run(
            [
                std::ffi::OsString::from("export-file-yaml"),
                std::ffi::OsString::from("0"),
                output_path.clone().into_os_string(),
                serialized_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();
        let yaml = fs::read_to_string(&output_path).unwrap();
        assert!(yaml.contains("--- !u!1 &42"));
        assert!(yaml.contains("m_Target: {fileID: 42}"));
        drop(input);
    }

    #[test]
    fn export_project_writes_a_cross_file_yaml_workspace() {
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("main.assets");
        let target_directory = directory.join("dependencies");
        fs::create_dir(&target_directory).unwrap();
        let target_path = target_directory.join("other.assets");
        let output_root = directory.join("project");
        fs::write(
            &source_path,
            make_generation_10_file_with_pointer(&["other.assets"], 1, 42),
        )
        .unwrap();
        fs::write(&target_path, make_generation_10_file()).unwrap();
        fs::create_dir(&output_root).unwrap();

        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![source_path.clone(), target_path.clone()],
            // Drop removes directories in reverse order, so keep parents before children.
            subdirectories: vec![
                target_directory.clone(),
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/GameObject"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
                target_directory.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        let source_asset = output_root.join("Assets/GameObject/GameObject.asset");
        let source_meta = output_root.join("Assets/GameObject/GameObject.asset.meta");
        let target_asset = output_root.join("Assets/GameObject/GameObject_0.asset");
        let target_meta = output_root.join("Assets/GameObject/GameObject_0.asset.meta");
        let source_yaml = fs::read_to_string(&source_asset).unwrap();
        let target_meta_text = fs::read_to_string(&target_meta).unwrap();
        let target_guid = target_meta_text
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("target asset sidecar has a GUID")
            .to_owned();

        assert!(source_yaml.contains("--- !u!1 &100000"));
        assert!(source_yaml.contains("m_Target: {fileID: 100000,"));
        assert!(source_yaml.contains(&format!("guid: {target_guid}")));
        assert!(fs::read_to_string(&target_asset)
            .unwrap()
            .contains("--- !u!1 &100000"));
        assert!(fs::read_to_string(&source_meta)
            .unwrap()
            .contains("NativeFormatImporter:"));
        assert_ne!(
            meta_guid(&source_meta),
            target_guid,
            "each exported object is its own baseline collection with its own GUID"
        );
        assert!(
            fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt"))
                .unwrap()
                .contains("2019.4.0f1")
        );
        assert!(
            fs::read_to_string(output_root.join("Packages/manifest.json"))
                .unwrap()
                .contains("\"dependencies\"")
        );
        input.files.extend([
            source_asset,
            source_meta,
            target_asset,
            target_meta,
            output_root.join("ProjectSettings/ProjectVersion.txt"),
            output_root.join("Packages/manifest.json"),
        ]);
    }

    #[test]
    fn export_project_recovers_generated_textasset_without_usable_file_typetree() {
        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        fn collect_output_tree(
            path: &std::path::Path,
            files: &mut Vec<PathBuf>,
            directories: &mut Vec<PathBuf>,
        ) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    directories.push(path.clone());
                    collect_output_tree(&path, files, directories);
                } else {
                    files.push(path);
                }
            }
        }

        let fixture = assetripper::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|textasset_notree|"))
            .expect(
                "source-factory fixture includes a generated TextAsset without a file TypeTree",
            );
        let source_bytes = decode_hex(hex);
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("textasset_notree.assets");
        let output_root = directory.join("project-no-typetree");
        fs::write(&source_path, &source_bytes).unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![source_path.clone()],
            subdirectories: Vec::new(),
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect("export-project uses the source-generated TPK fallback");

        let mut exported_files = Vec::new();
        let mut exported_directories = Vec::new();
        collect_output_tree(&output_root, &mut exported_files, &mut exported_directories);
        let assets = exported_files
            .iter()
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "asset")
            })
            .collect::<Vec<_>>();
        assert_eq!(assets.len(), 1, "exported files: {exported_files:?}");
        let yaml = fs::read_to_string(assets[0]).unwrap();
        assert!(yaml.contains("--- !u!49 &4900000"), "{yaml}");
        assert!(yaml.contains("m_Name: Hero"), "{yaml}");
        assert!(yaml.contains("m_Script: print(1)"), "{yaml}");
        assert_eq!(fs::read(&source_path).unwrap(), source_bytes);

        input.files.extend(exported_files);
        input.subdirectories.extend(exported_directories);
    }

    #[test]
    fn export_project_refuses_unknown_object_without_a_usable_type_tree() {
        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        let fixture = assetripper::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|unknown_class|"))
            .expect("source-factory fixture includes an unknown class");
        let bytes = decode_hex(hex);
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("unknown_class.assets");
        let output_root = directory.join("project-unknown-class");
        fs::write(&source_path, &bytes).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![source_path.clone()],
            subdirectories: Vec::new(),
        };

        let error = super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .expect_err("an unknown class without a tree must not report a complete export");
        assert!(
            error.to_string().contains("complete workspace PPtr scan"),
            "{error}"
        );
        assert!(
            !output_root.exists(),
            "failed export must not create a project root"
        );
        assert_eq!(fs::read(&source_path).unwrap(), bytes);
        drop(input);
    }

    #[test]
    fn export_project_materializes_streamed_texture2d_and_fails_closed_when_resource_is_missing() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("texture.resS");
        let output_root = directory.join("project");
        let missing_output_root = directory.join("project-missing-resource");
        let serialized_bytes = make_streamed_texture2d_file("texture.resS", 2, 3);
        fs::write(&serialized_path, &serialized_bytes).unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        fs::create_dir(&output_root).unwrap();
        fs::create_dir(&missing_output_root).unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), resource_path.clone()],
            subdirectories: vec![
                output_root.clone(),
                missing_output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Texture2D"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                serialized_path.clone().into_os_string(),
                resource_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        let asset_path = output_root.join("Assets/Texture2D/Texture2D.texture2D");
        let meta_path = output_root.join("Assets/Texture2D/Texture2D.texture2D.meta");
        let yaml = fs::read_to_string(&asset_path).unwrap();
        assert!(yaml.contains("--- !u!28 &2800000"));
        assert!(yaml.contains("image data: 323334"), "{yaml}");
        assert!(yaml.contains("offset: 0"), "{yaml}");
        assert!(yaml.contains("size: 0"), "{yaml}");
        assert!(!yaml.contains("texture.resS"), "{yaml}");
        assert!(fs::read_to_string(&meta_path)
            .unwrap()
            .contains("NativeFormatImporter:"));
        assert!(
            fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt"))
                .unwrap()
                .contains("2022.3.22f1")
        );
        assert_eq!(fs::read(&serialized_path).unwrap(), serialized_bytes);
        assert_eq!(fs::read(&resource_path).unwrap(), b"0123456789");

        let error = super::run(
            [
                std::ffi::OsString::from("export-project"),
                missing_output_root.clone().into_os_string(),
                serialized_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("texture.resS"), "{error}");
        assert_eq!(fs::read_dir(&missing_output_root).unwrap().count(), 0);
        assert_eq!(fs::read(&serialized_path).unwrap(), serialized_bytes);

        input.files.extend([
            asset_path,
            meta_path,
            output_root.join("ProjectSettings/ProjectVersion.txt"),
            output_root.join("Packages/manifest.json"),
        ]);
    }

    #[test]
    fn export_project_resolves_streamed_texture2d_from_platform_data_path() {
        let scratch = TestInputDirectory::new_root();
        let platform_root = scratch.join("game");
        let data_directory = platform_root.join("game_Data");
        let executable = platform_root.join("game.exe");
        let serialized_path = data_directory.join("globalgamemanagers");
        let resource_path = data_directory.join("texture.resS");
        let output_root = scratch.join("project");
        fs::create_dir(&platform_root).unwrap();
        fs::create_dir(&data_directory).unwrap();
        fs::write(&executable, b"synthetic executable marker").unwrap();
        let serialized_bytes = make_streamed_texture2d_file("texture.resS", 2, 3);
        fs::write(&serialized_path, &serialized_bytes).unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        fs::create_dir(&output_root).unwrap();

        let asset_path = output_root.join("Assets/Texture2D/Texture2D.texture2D");
        let meta_path = output_root.join("Assets/Texture2D/Texture2D.texture2D.meta");
        let input = TestInputDirectory {
            directory: scratch,
            files: vec![
                executable,
                serialized_path.clone(),
                resource_path.clone(),
                asset_path.clone(),
                meta_path.clone(),
                output_root.join("ProjectSettings/ProjectVersion.txt"),
                output_root.join("Packages/manifest.json"),
            ],
            subdirectories: vec![
                platform_root,
                data_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Texture2D"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                output_root.clone().into_os_string(),
                input.directory.join("game").into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        let source_rows = include_str!("export/streamed_image_resource_fixture.txt");
        assert!(source_rows
            .lines()
            .any(|line| { line == "SOURCE_PROVIDER|calls=1|name=texture.resS" }));
        assert!(source_rows.lines().any(|line| {
            line == "SOURCE_STREAMED_YAML|class=28|data=323334|offset=0|size=0|path_empty=true"
        }));
        let yaml = fs::read_to_string(&asset_path).unwrap();
        assert!(yaml.contains("--- !u!28 &2800000"), "{yaml}");
        assert!(yaml.contains("image data: 323334"), "{yaml}");
        assert!(yaml.contains("offset: 0"), "{yaml}");
        assert!(yaml.contains("size: 0"), "{yaml}");
        assert!(!yaml.contains("texture.resS"), "{yaml}");
        assert!(fs::read_to_string(&meta_path)
            .unwrap()
            .contains("NativeFormatImporter:"));
        assert!(
            fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt"))
                .unwrap()
                .contains("2022.3.22f1")
        );
        assert_eq!(fs::read(&serialized_path).unwrap(), serialized_bytes);
        assert_eq!(fs::read(&resource_path).unwrap(), b"0123456789");
    }

    #[test]
    fn export_project_dispatches_collections_and_writes_a_complete_unity_project() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();

        let main_path = directory.join("main.assets");
        fs::write(
            &main_path,
            make_generation_10_file_with_pointer(&["other.assets"], 1, 42),
        )
        .unwrap();
        let other_path = assets_directory.join("other.assets");
        fs::write(&other_path, make_generation_10_file()).unwrap();
        let texture_path = assets_directory.join("texture.assets");
        fs::write(&texture_path, make_texture2d_file()).unwrap();
        let material_path = assets_directory.join("material.assets");
        fs::write(
            &material_path,
            make_material_file_with_shader(42, "shader.assets"),
        )
        .unwrap();
        let mesh_path = assets_directory.join("mesh.assets");
        fs::write(&mesh_path, make_mesh_file()).unwrap();
        let player_path = assets_directory.join("player.assets");
        fs::write(&player_path, make_player_settings_file()).unwrap();
        let build_settings_path = assets_directory.join("settings.assets");
        fs::write(
            &build_settings_path,
            make_build_settings_file(&["Assets/Scenes/level0.unity"]),
        )
        .unwrap();
        let bundle_path = assets_directory.join("bundle.assets");
        fs::write(&bundle_path, make_asset_bundle_file()).unwrap();
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &shader_path,
            make_shader_file_with_path_id("Shader \"x\" { }\n", 42),
        )
        .unwrap();

        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![
                main_path.clone(),
                other_path.clone(),
                texture_path.clone(),
                material_path.clone(),
                mesh_path.clone(),
                player_path.clone(),
                build_settings_path.clone(),
                bundle_path.clone(),
                shader_path.clone(),
            ],
            // Drop removes directories in reverse order, so keep parents before children.
            subdirectories: vec![
                assets_directory.clone(),
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/GameObject"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Mesh"),
                output_root.join("Assets/Shader"),
                output_root.join("Assets/Texture2D"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        let mut status = Vec::new();
        super::export_project_to(
            &[main_path, assets_directory],
            &output_root,
            false,
            &default_export_settings(),
            &mut status,
        )
        .unwrap();
        let status = String::from_utf8(status).unwrap();

        let actual = project_tree(&output_root);
        assert_eq!(
            actual,
            vec![
                "Assets/GameObject/GameObject.asset".to_owned(),
                "Assets/GameObject/GameObject.asset.meta".to_owned(),
                "Assets/GameObject/GameObject_0.asset".to_owned(),
                "Assets/GameObject/GameObject_0.asset.meta".to_owned(),
                "Assets/Material/TestMat.mat".to_owned(),
                "Assets/Material/TestMat.mat.meta".to_owned(),
                "Assets/Mesh/Tri.asset".to_owned(),
                "Assets/Mesh/Tri.asset.meta".to_owned(),
                "Assets/Shader/Test_Simple.shader".to_owned(),
                "Assets/Shader/Test_Simple.shader.meta".to_owned(),
                "Assets/Texture2D/Texture2D.texture2D".to_owned(),
                "Assets/Texture2D/Texture2D.texture2D.meta".to_owned(),
                "Packages/manifest.json".to_owned(),
                "ProjectSettings/ProjectSettings.asset".to_owned(),
                "ProjectSettings/ProjectVersion.txt".to_owned(),
            ],
            "actual project tree: {actual:#?}\nstatus:\n{status}"
        );

        // The cross-file PPtr renders the target object's own collection GUID.
        let target_guid = meta_guid(&output_root.join("Assets/GameObject/GameObject_0.asset.meta"));
        let main_yaml = fs::read_to_string(output_root.join("Assets/GameObject/GameObject.asset"))
            .expect("cross-file asset is readable");
        assert!(main_yaml.contains("--- !u!1 &100000"), "{main_yaml}");
        assert!(
            main_yaml.contains(&format!("guid: {target_guid}")),
            "{main_yaml}"
        );

        // Native asset sidecars use AssetRipper's class-prefixed main export IDs.
        for (relative, main_id) in [
            ("Assets/GameObject/GameObject.asset.meta", 100_000),
            ("Assets/GameObject/GameObject_0.asset.meta", 100_000),
            ("Assets/Material/TestMat.mat.meta", 2_100_000),
            ("Assets/Mesh/Tri.asset.meta", 4_300_000),
            ("Assets/Texture2D/Texture2D.texture2D.meta", 2_800_000),
        ] {
            let meta = fs::read_to_string(output_root.join(relative)).unwrap();
            assert!(
                meta.contains("NativeFormatImporter:"),
                "{relative}:\n{meta}"
            );
            assert!(
                meta.contains(&format!("mainObjectFileID: {main_id}")),
                "{relative}:\n{meta}"
            );
        }

        // The source selects ShaderImporter for decompiled text and Material shader pointers
        // use the shader collection's exported file ID/GUID with Meta pointer type 3.
        let shader_meta_path = "Assets/Shader/Test_Simple.shader.meta";
        let shader_meta = fs::read_to_string(output_root.join(shader_meta_path)).unwrap();
        assert!(shader_meta.contains("ShaderImporter:"), "{shader_meta}");
        assert!(shader_meta.contains("externalObjects: {}"), "{shader_meta}");
        assert!(shader_meta.contains("defaultTextures: []"), "{shader_meta}");
        assert!(
            shader_meta.contains("nonModifiableTextures: []"),
            "{shader_meta}"
        );
        assert!(!shader_meta.contains("mainObjectFileID:"), "{shader_meta}");
        let shader_guid = meta_guid(&output_root.join(shader_meta_path));
        assert_eq!(
            fs::read(output_root.join("Assets/Shader/Test_Simple.shader")).unwrap(),
            b"Shader \"x\" { }\n"
        );
        let material_yaml = fs::read_to_string(output_root.join("Assets/Material/TestMat.mat"))
            .expect("material YAML is readable");
        assert!(
            material_yaml.contains(&format!(
                "m_Shader: {{fileID: 4800000, guid: {shader_guid}, type: 3}}"
            )),
            "material did not preserve its cross-file Shader reference:\n{material_yaml}"
        );

        // The settings-file family writes one file per object with unique GUIDs.
        let mut guids: Vec<String> = [
            "Assets/GameObject/GameObject.asset.meta",
            "Assets/GameObject/GameObject_0.asset.meta",
            "Assets/Material/TestMat.mat.meta",
            "Assets/Mesh/Tri.asset.meta",
            shader_meta_path,
            "Assets/Texture2D/Texture2D.texture2D.meta",
        ]
        .iter()
        .map(|relative| meta_guid(&output_root.join(relative)))
        .collect();
        guids.sort();
        let guid_count = guids.len();
        guids.dedup();
        assert_eq!(guids.len(), guid_count, "collection GUIDs must be unique");

        // The manager family has a fixed destination and no sidecar; the two non-exportable
        // collections wrote nothing and are reported explicitly.
        assert!(!output_root
            .join("ProjectSettings/ProjectSettings.asset.meta")
            .exists());
        assert!(!output_root.join("Assets/BuildSettings").exists());
        assert!(!output_root.join("Assets/AssetBundle").exists());
        assert!(
            status.contains("non-exportable baseline collection"),
            "{status}"
        );
        assert!(status.contains("EmptyExportCollection"), "{status}");
        assert!(
            !status.contains("generic asset-family renderer"),
            "this fixture's generic baseline routes should not be reported as specialized-renderer fallbacks:\n{status}"
        );
        assert!(
            !status.contains("ShaderExportCollection"),
            "shader used an unexpected generic fallback:\n{status}"
        );

        // Post-exporters: ProjectVersion and the default package manifest.
        assert!(
            fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt"))
                .unwrap()
                .contains("2019.4.0f1")
        );
        let manifest = fs::read_to_string(output_root.join("Packages/manifest.json")).unwrap();
        assert!(
            manifest.contains("\"com.unity.modules.androidjni\""),
            "{manifest}"
        );

        input.files.extend([
            output_root.join("Assets/GameObject/GameObject.asset"),
            output_root.join("Assets/GameObject/GameObject.asset.meta"),
            output_root.join("Assets/GameObject/GameObject_0.asset"),
            output_root.join("Assets/GameObject/GameObject_0.asset.meta"),
            output_root.join("Assets/Material/TestMat.mat"),
            output_root.join("Assets/Material/TestMat.mat.meta"),
            output_root.join("Assets/Mesh/Tri.asset"),
            output_root.join("Assets/Mesh/Tri.asset.meta"),
            output_root.join("Assets/Shader/Test_Simple.shader"),
            output_root.join("Assets/Shader/Test_Simple.shader.meta"),
            output_root.join("Assets/Texture2D/Texture2D.texture2D"),
            output_root.join("Assets/Texture2D/Texture2D.texture2D.meta"),
            output_root.join("ProjectSettings/ProjectSettings.asset"),
            output_root.join("ProjectSettings/ProjectVersion.txt"),
            output_root.join("Packages/manifest.json"),
        ]);
        input
            .subdirectories
            .push(output_root.join("Assets/Shader/Test"));
        drop(input);
    }

    #[test]
    fn export_project_yaml_shader_writes_source_versioned_patch_and_importer() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let shader_path = assets_directory.join("shader.assets");
        let script = "Shader \"x\" {\nSubShader {\nProgram \"vp\" {}\n}\n}\n";
        fs::write(&shader_path, make_shader_file_with_path_id(script, 42)).unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Shader"),
                output_root.join("Assets/Editor"),
                output_root.join("Assets/Editor/AssetRipperPatches"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };
        let mut settings = default_export_settings();
        settings.shader_export_mode = ShaderExportMode::Yaml;
        let mut status = Vec::new();
        super::export_project_to(&[shader_path], &output_root, false, &settings, &mut status)
            .unwrap();
        let status = String::from_utf8(status).unwrap();

        let asset_path = output_root.join("Assets/Shader/Test_Simple.asset");
        let meta_path = output_root.join("Assets/Shader/Test_Simple.asset.meta");
        assert!(asset_path.is_file());
        assert!(!output_root
            .join("Assets/Shader/Test_Simple.shader")
            .exists());
        assert!(
            fs::read_to_string(&asset_path)
                .unwrap()
                .contains("--- !u!48 &4800000"),
            "YAML Shader asset uses the Shader collection export ID"
        );
        let meta = fs::read_to_string(meta_path).unwrap();
        assert!(meta.contains("NativeFormatImporter:"), "{meta}");
        assert!(meta.contains("  name:\n"), "{meta}");
        assert!(!meta.contains("externalObjects:"), "{meta}");
        assert!(!meta.contains("mainObjectFileID:"), "{meta}");

        let patch_path =
            output_root.join("Assets/Editor/AssetRipperPatches/AvoidSavingYamlShaders.cs");
        assert!(patch_path.is_file());
        assert!(!output_root
            .join("Assets/Editor/AssetRipperPatches/YamlShaderPostprocessor.cs")
            .exists());
        assert!(status.contains("support-file count 1"), "{status}");

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_shader_importer_matches_the_reference_nonmodifiable_texture_map() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let expected = include_str!("export/shader_nonmodifiable_fixture.txt");
        let expected = expected
            .lines()
            .find_map(|line| line.strip_prefix("META|"))
            .expect("source fixture has a normalized importer row")
            .replace("\\n", "\n");
        let meta_path = output_root.join("Assets/Shader/Test_Simple.shader.meta");
        let meta = fs::read_to_string(meta_path).unwrap();
        let normalized = meta
            .lines()
            .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(normalized, expected);

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_shader_importer_maps_nonmodifiable_texture_pointers() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let shader_path = assets_directory.join("shader.assets");
        let texture_path = assets_directory.join("texture.assets");
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_pointer(1, 88, &["texture.assets"]),
        )
        .unwrap();
        fs::write(
            &texture_path,
            make_minimal_texture_file("ExternalTexture", 88),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![shader_path.clone(), texture_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Shader"),
                output_root.join("Assets/Texture2D"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[shader_path, texture_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let texture_meta =
            fs::read_to_string(output_root.join("Assets/Texture2D/ExternalTexture.texture2D.meta"))
                .unwrap();
        let texture_guid = texture_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Texture2D sidecar has a GUID");
        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        assert!(
            shader_meta.contains(&format!(
                "  - _MainTex: {{fileID: 2800000, guid: {texture_guid}, type: 2}}"
            )),
            "ShaderImporter texture entry points to the exported Texture2D sidecar: {shader_meta}"
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_yaml_matches_the_reference_2018_2_project_probe() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(&material_path, make_material_file_with_reference_2018_tpk()).unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_saved_property_maps_match_the_reference_2018_2_project_probe() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2018_tpk_and_saved_properties(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_properties_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_multiple_saved_property_entries_preserve_source_order() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2018_tpk_with_multiple_saved_properties(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_multi_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_duplicate_saved_property_keys_match_reference_yaml() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2018_tpk_with_duplicate_saved_property_keys(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_duplicate_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_yaml_matches_the_reference_2018_3_project_probe() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2018_3_tpk_and_saved_properties(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_2018_3_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_yaml_matches_the_reference_2019_4_project_probe() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2019_4_tpk_and_saved_properties(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_2019_4_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_yaml_matches_the_reference_2017_4_project_probe() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2017_4_tpk_and_saved_properties(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_2017_4_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_rejects_unresolved_references_before_writing_project_files() {
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("main.assets");
        let output_root = directory.join("project");
        fs::write(
            &source_path,
            make_generation_10_file_with_pointer_path_id(&[], 99),
        )
        .unwrap();
        fs::create_dir(&output_root).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![source_path.clone()],
            subdirectories: vec![output_root.clone()],
        };

        let error = super::export_project_to(
            &[source_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("unresolved")
                || error.to_string().contains("complete workspace PPtr scan"),
            "unexpected project export rejection: {error}"
        );
        assert_eq!(fs::read_dir(&output_root).unwrap().count(), 0);
        drop(input);
    }

    #[test]
    fn export_project_refuses_a_nonempty_output_directory_without_touching_it() {
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("main.assets");
        let output_root = directory.join("project");
        let existing_file = output_root.join("keep.txt");
        fs::write(&source_path, make_generation_10_file()).unwrap();
        fs::create_dir(&output_root).unwrap();
        fs::write(&existing_file, b"existing project file").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![source_path.clone(), existing_file.clone()],
            subdirectories: vec![output_root.clone()],
        };

        let error = super::export_project_to(
            &[source_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("User declined to delete existing export directory. Aborting export."));
        assert!(error.to_string().contains("--delete-existing"));
        assert_eq!(fs::read(&existing_file).unwrap(), b"existing project file");
        assert_eq!(fs::read_dir(&output_root).unwrap().count(), 1);
        drop(input);
    }

    #[test]
    fn export_project_replaces_a_nonempty_output_directory_when_delete_existing_leads() {
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("main.assets");
        let output_root = directory.join("project");
        let stale_file = output_root.join("keep.txt");
        fs::write(&source_path, make_generation_10_file()).unwrap();
        fs::create_dir(&output_root).unwrap();
        fs::write(&stale_file, b"stale project content").unwrap();
        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![source_path.clone()],
            // Drop removes directories in reverse order, so keep parents before children.
            subdirectories: vec![
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/GameObject"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--delete-existing"),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        let asset = output_root.join("Assets/GameObject/GameObject.asset");
        let meta = output_root.join("Assets/GameObject/GameObject.asset.meta");
        assert!(
            !stale_file.exists(),
            "the stale export root content survives"
        );
        assert!(fs::read_to_string(&asset)
            .unwrap()
            .contains("--- !u!1 &100000"));
        assert!(fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt")).is_ok());
        input.files.extend([
            asset,
            meta,
            output_root.join("ProjectSettings/ProjectVersion.txt"),
            output_root.join("Packages/manifest.json"),
        ]);
        drop(input);
    }

    #[test]
    fn export_project_accepts_delete_existing_after_the_inputs() {
        let mut input = TestInputDirectory::new();
        let source_path = input.files[0].clone();
        let output_root = input.directory.join("project");

        // Dispatch hands the command parser the arguments after the command name.
        super::export_project_from_arguments(
            [
                output_root.clone().into_os_string(),
                source_path.into_os_string(),
                std::ffi::OsString::from("--delete-existing"),
            ]
            .into_iter(),
        )
        .unwrap();

        let asset = output_root.join("Assets/GameObject/GameObject.asset");
        assert!(asset.is_file());
        input.files.extend([
            asset,
            output_root.join("Assets/GameObject/GameObject.asset.meta"),
            output_root.join("ProjectSettings/ProjectVersion.txt"),
            output_root.join("Packages/manifest.json"),
        ]);
        input.subdirectories.extend([
            output_root.clone(),
            output_root.join("Assets"),
            output_root.join("Assets/GameObject"),
            output_root.join("ProjectSettings"),
            output_root.join("Packages"),
        ]);
        drop(input);
    }

    #[test]
    fn export_project_treats_delete_existing_as_a_flag_not_as_the_output_directory() {
        // Dispatch hands the command parser the arguments after the command name.
        let error = super::export_project_from_arguments(
            [std::ffi::OsString::from("--delete-existing")].into_iter(),
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("export-project requires a new output directory"));
    }

    #[test]
    fn export_file_yaml_reports_baseline_omitted_image_textures_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("serialized-file.yaml");
        fs::write(
            &serialized_path,
            make_streamed_image_texture_file_with_stream("", 3),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_file_yaml_to(&[serialized_path], 0, &output_path, &mut status).unwrap();
        assert!(!output_path.exists());
        assert!(String::from_utf8(status).unwrap().contains(
            "all 1 objects were omitted because the baseline YAML exporter found ImageTexture objects without usable inline or streamed data"
        ));
        drop(input);
    }

    #[test]
    fn export_file_yaml_rejects_unresolved_and_cross_file_references_without_output() {
        let unresolved_directory = TestInputDirectory::new_root();
        let unresolved_path = unresolved_directory.join("main.assets");
        let unresolved_output = unresolved_directory.join("unresolved.yaml");
        fs::write(
            &unresolved_path,
            make_generation_10_file_with_pointer_path_id(&[], 99),
        )
        .unwrap();
        let unresolved_input = TestInputDirectory {
            directory: unresolved_directory,
            files: vec![unresolved_path.clone(), unresolved_output.clone()],
            subdirectories: Vec::new(),
        };
        let unresolved_error = super::export_file_yaml_to(
            std::slice::from_ref(&unresolved_path),
            0,
            &unresolved_output,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(unresolved_error
            .to_string()
            .contains("unresolved reference"));
        assert!(!unresolved_output.exists());
        drop(unresolved_input);

        let cross_file_directory = TestInputDirectory::new_root();
        let source_path = cross_file_directory.join("main.assets");
        let target_path = cross_file_directory.join("other.assets");
        let cross_file_output = cross_file_directory.join("cross-file.yaml");
        fs::write(
            &source_path,
            make_generation_10_file_with_pointer(&["other.assets"], 1, 42),
        )
        .unwrap();
        fs::write(&target_path, make_generation_10_file()).unwrap();
        let cross_file_input = TestInputDirectory {
            directory: cross_file_directory,
            files: vec![
                source_path.clone(),
                target_path.clone(),
                cross_file_output.clone(),
            ],
            subdirectories: Vec::new(),
        };
        let cross_file_error = super::export_file_yaml_to(
            &[source_path, target_path],
            0,
            &cross_file_output,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(cross_file_error
            .to_string()
            .contains("cannot assign a foreign collection GUID"));
        assert!(!cross_file_output.exists());
        drop(cross_file_input);
    }

    #[test]
    fn explicit_file_inspection_resolves_uint32_streaming_info_offset_from_selected_root_resource()
    {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("texture.resS");
        fs::write(
            &serialized_path,
            make_streaming_info_file_with_offset_width("texture.resS", 2, 3, false),
        )
        .unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), resource_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_explicit_files_to(&[serialized_path, resource_path], &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();

        assert!(summary.contains(
            "StreamingInfo resources: 1 candidates, 0 unset, 0 bundle ranges, 1 explicit root ranges, 0 external ranges, 0 missing, 0 invalid ranges, 0 resolution errors, 0 scan errors, 0 undecoded objects"
        ));
        assert!(summary.contains("complete=true"));
        assert!(!summary.contains("StreamingInfo diagnostic:"));
        drop(input);
    }

    #[test]
    fn explicit_file_inspection_keeps_unselected_sibling_resource_missing() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let unselected_resource_path = directory.join("missing.resS");
        fs::write(
            &serialized_path,
            make_streaming_info_file("missing.resS", 2, 3),
        )
        .unwrap();
        fs::write(&unselected_resource_path, b"not mounted as an input").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), unselected_resource_path],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_explicit_files_to(&[serialized_path], &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();

        assert!(summary.contains(
            "StreamingInfo resources: 1 candidates, 0 unset, 0 bundle ranges, 0 explicit root ranges, 0 external ranges, 1 missing"
        ));
        assert!(summary.contains("complete=false"));
        assert!(summary.contains("path \"missing.resS\""));
        assert!(summary.contains("resource missing"));
        drop(input);
    }

    #[test]
    fn directory_inspection_decodes_scene_collection_guid_words() {
        let directory = TestInputDirectory::new_root();
        let file = directory.join("level0");
        let expected = [0x0123_4567, 0x89ab_cdef, 3, 4];
        let bytes = make_occlusion_settings_file(expected);
        let parsed = assetripper::serialized_file::file::SerializedFile::parse(&bytes).unwrap();
        let payload = parsed.decode_object_payload(0).unwrap();
        assert_eq!(
            assetripper::processing::scene_collections::extract_occlusion_scene_guid(&payload.root)
                .unwrap()
                .unwrap()
                .data,
            expected
        );
        fs::write(&file, bytes).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![file],
            subdirectories: Vec::new(),
        };

        super::inspect_directories(std::slice::from_ref(&input.directory)).unwrap();
    }

    #[test]
    fn workspace_summary_maps_scene_collection_path_and_guid() {
        let directory = TestInputDirectory::new_root();
        let scene_file = directory.join("level0");
        let build_settings_file = directory.join("globalgamemanagers");
        let prefab_file = directory.join("prefab.assets");
        fs::write(
            &scene_file,
            make_occlusion_settings_file([0x0123_4567, 0x89ab_cdef, 3, 4]),
        )
        .unwrap();
        fs::write(
            &build_settings_file,
            make_build_settings_file(&["Assets/Scenes/Main.unity"]),
        )
        .unwrap();
        fs::write(&prefab_file, make_generation_10_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![scene_file, build_settings_file, prefab_file],
            subdirectories: Vec::new(),
        };
        let (mut filesystem, selected_paths, _) =
            super::mount_input_directories(std::slice::from_ref(&input.directory), false).unwrap();
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (workspace, dependencies) =
            super::SerializedFileWorkspace::load_from_vfs_with_dependencies(
                &mut filesystem,
                &mixed.serialized_file_inputs,
                &mixed.registered_files,
                &mixed.data_paths,
                super::SerializedFileWorkspaceLimits::default(),
                super::DependencyIndexLimits::default(),
                super::ContainerMountLimits::default(),
            )
            .unwrap();
        let mut output = Vec::new();
        super::print_workspace_summary_to(&mut output, &filesystem, &workspace, &dependencies)
            .unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(summary
            .contains("Object payload decode: 3/3 decoded, 0 undecoded, 0 raw fallback bytes"));
        assert!(summary.contains("Workspace collections: 3 (3 objects, 0 external identifiers)"));
        assert!(summary.contains("TypeTree reference graph:"));
        assert!(
            summary.contains("SceneGUID words: [0x01234567, 0x89abcdef, 0x00000003, 0x00000004]")
        );
        assert!(summary.contains(
            "Scene path mapping for collection \"level0\": Assets/Scenes/Main (BuildSettings index 0, duplicate path=false)"
        ));
        assert!(summary.contains("Non-scene hierarchy collection graphs: 1"));
        assert!(summary.contains(": 1 GameObjects, 0 Transforms,"));
    }

    #[test]
    fn workspace_summary_reports_game_object_hierarchy_with_cross_file_roots() {
        let directory = TestInputDirectory::new_root();
        fs::write(
            directory.join("level0"),
            make_occlusion_settings_file([1, 2, 3, 4]),
        )
        .unwrap();
        fs::write(
            directory.join("hero-go"),
            make_inspect_game_object_file("Hero", 10, &[(1, 20)], Some("hero-transform")),
        )
        .unwrap();
        fs::write(
            directory.join("hero-transform"),
            make_inspect_transform_file(20, (1, 10), &[], (0, 0), 3, &["hero-go"]),
        )
        .unwrap();
        fs::write(
            directory.join("side-go"),
            make_inspect_game_object_file("Sidekick", 11, &[(1, 21)], Some("side-transform")),
        )
        .unwrap();
        fs::write(
            directory.join("side-transform"),
            make_inspect_transform_file(21, (0, 9999), &[], (1, 20), 5, &["hero-transform"]),
        )
        .unwrap();
        let (mut filesystem, selected_paths, _) =
            super::mount_input_directories(std::slice::from_ref(&directory), false).unwrap();
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (workspace, dependencies) =
            super::SerializedFileWorkspace::load_from_vfs_with_dependencies(
                &mut filesystem,
                &mixed.serialized_file_inputs,
                &mixed.registered_files,
                &mixed.data_paths,
                super::SerializedFileWorkspaceLimits::default(),
                super::DependencyIndexLimits::default(),
                super::ContainerMountLimits::default(),
            )
            .unwrap();
        let mut output = Vec::new();
        super::print_workspace_summary_to(&mut output, &filesystem, &workspace, &dependencies)
            .unwrap();
        let summary = String::from_utf8(output).unwrap();
        // All five objects decode; the six PPtrs split into 1 null (hero
        // father), 4 cross-file resolutions, and 1 explicit local miss.
        assert!(summary
            .contains("Object payload decode: 5/5 decoded, 0 undecoded, 0 raw fallback bytes"));
        assert!(summary.contains(
            "TypeTree reference graph: 6 recognized PPtrs, 1 null, 4 uniquely resolved, 1 unresolved"
        ));
        assert!(summary.contains(
            "Unresolved PPtr: file 4 \"/inputs/0/side-transform\" (name \"side-transform\"), object 0, PathID 21, field \"m_GameObject\": MissingLocalObject { path_id: 9999 }"
        ));
        // The hero pair joins across files: the GameObject graph reports its
        // component edge, and root discovery follows the external Transform
        // with a null father and its stored root order.
        assert!(summary.contains("Scene graph: 1 GameObjects, 0 Transforms, 1 relationships"));
        assert!(summary.contains(
            "root candidate file 0 PathID 10 (Transform file 1 object 0 PathID Some(20); NullFather)"
        ));
        // The orphan side Transform (dangling m_GameObject, external father)
        // stays explicit: incomplete graph plus a hierarchy issue, never a
        // silent null root.
        assert!(summary.contains("Scene graph: 0 GameObjects, 1 Transforms, 2 relationships"));
        assert!(summary.contains("workspace hierarchy issues: 1, audit complete=false"));
        assert!(summary.contains("Non-scene hierarchy collection graphs: 4"));
        assert!(summary.contains(
            "GameObject 3:0 PathID 11 GetRoot is indeterminate: Some(IncompleteCollectionGraph { file_index: 4 })"
        ));
        // The manager file still owns the only SceneDefinition; standalone files
        // have no SerializedBundle candidate and the generated property stays null.
        assert!(summary.contains("SceneDefinitions: 1"));
        assert!(summary.contains("hierarchy AssetBundleName: null"));
        assert!(
            summary.contains("hierarchy AssetBundleName candidate: unset (no SerializedBundle)")
        );
        assert!(
            summary.contains("external requests: 4; newly loaded dependencies: 0; resolved: true")
        );
    }

    #[test]
    fn reference_graph_summary_distinguishes_null_resolved_and_unresolved_pointers() {
        for (pointer_path_id, expected_counts, complete) in [
            (0, "1 null, 0 uniquely resolved, 0 unresolved", true),
            (42, "0 null, 1 uniquely resolved, 0 unresolved", true),
            (99, "0 null, 0 uniquely resolved, 1 unresolved", false),
        ] {
            let mut filesystem = VirtualFileSystem::new();
            insert_vfs_file(
                &mut filesystem,
                "/game/main.assets",
                &make_generation_10_file_with_pointer_path_id(&[], pointer_path_id),
            );
            let workspace =
                assetripper::serialized_file::workspace::SerializedFileWorkspace::load_from_vfs(
                    &filesystem,
                    &[assetripper::io::dependency::DependencyFilePath {
                        file_name: "main.assets".to_owned(),
                        virtual_path: "/game/main.assets".to_owned(),
                    }],
                    assetripper::serialized_file::workspace::SerializedFileWorkspaceLimits::default(
                    ),
                )
                .unwrap();
            let graph = workspace
                .reference_graph(
                    assetripper::serialized_file::reference_graph::MultiFileReferenceGraphLimits::default(),
                )
                .unwrap();
            let mut output = Vec::new();
            super::print_reference_graph_summary_to(&mut output, &workspace, &graph).unwrap();
            let summary = String::from_utf8(output).unwrap();
            assert!(summary.contains(expected_counts), "{summary}");
            assert!(
                summary.contains(&format!("complete={complete}")),
                "{summary}"
            );
            if pointer_path_id == 99 {
                assert!(summary.contains("MissingLocalObject { path_id: 99 }"));
            }
        }
    }

    #[test]
    fn workspace_summary_selects_last_build_settings_in_source_collection_order() {
        let mut filesystem = VirtualFileSystem::new();
        insert_vfs_file(
            &mut filesystem,
            "/game/a.assets",
            &make_build_settings_file_with_path_id(&["Assets/Scenes/First.unity"], 42),
        );
        insert_vfs_file(
            &mut filesystem,
            "/game/z.assets",
            &make_build_settings_file_with_path_id(&["Assets/Scenes/Last.unity"], 43),
        );
        let selected_paths = vec!["/game/a.assets".to_owned(), "/game/z.assets".to_owned()];
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (workspace, dependencies) =
            super::SerializedFileWorkspace::load_from_vfs_with_dependencies(
                &mut filesystem,
                &mixed.serialized_file_inputs,
                &mixed.registered_files,
                &mixed.data_paths,
                super::SerializedFileWorkspaceLimits::default(),
                super::DependencyIndexLimits::default(),
                super::ContainerMountLimits::default(),
            )
            .unwrap();

        let mut output = Vec::new();
        super::print_workspace_summary_to(&mut output, &filesystem, &workspace, &dependencies)
            .unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(summary.contains(
            "BuildSettings object 42: 1 scene paths (selected by source collection traversal)"
        ));
        assert!(summary.contains("Generated EditorBuildSettings: 1 scenes"));
        assert!(summary.contains("scene[0]: enabled=true, path=\"Assets/Scenes/First.unity\""));
        assert!(!summary.contains(
            "BuildSettings object 43: 1 scene paths (selected by source collection traversal)"
        ));
    }

    #[test]
    fn workspace_summary_applies_streamed_scene_bundle_path_override() {
        let mut filesystem = VirtualFileSystem::new();
        let scene_file = make_occlusion_settings_file([1, 2, 3, 4]);
        let streamed_bundle = make_asset_bundle_file();
        let webdata = make_webdata_fixture(&[
            ("CAB-Scene", &scene_file),
            ("assetbundle.assets", &streamed_bundle),
        ]);
        insert_vfs_file(
            &mut filesystem,
            "/game/Game_Data/globalgamemanagers",
            &make_generation_10_file(),
        );
        insert_vfs_file(&mut filesystem, "/game/Game_Data/data.unity3d", &webdata);
        let structure = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: Some("/game/Game_Data".to_owned()),
            data_paths: vec!["/game/Game_Data".to_owned()],
        };
        let loaded = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap();

        let object_workspace = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &loaded.workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let collection_workspace = WorkspaceAssetCollections::build(
            &loaded.workspace,
            &object_workspace,
            WorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let bundle_name_plan =
            assetripper::processing::asset_bundle::plan_workspace_asset_bundle_names(
                &loaded.workspace,
                &object_workspace,
                &collection_workspace,
                Default::default(),
            )
            .unwrap();
        assert!(
            bundle_name_plan.is_complete(),
            "{:?}",
            bundle_name_plan.issues
        );
        assert_eq!(bundle_name_plan.bundles.len(), 1);
        assert_eq!(bundle_name_plan.bundles[0].source_name, "data.unity3d");
        assert_eq!(bundle_name_plan.bundles[0].asset_name, "data.unity3d");
        assert_eq!(bundle_name_plan.container_references.len(), 1);
        assert_eq!(
            bundle_name_plan.container_references[0].mapping.target,
            assetripper::processing::asset_bundle::AssetBundleContainerTarget::Null
        );
        assert!(bundle_name_plan.assignments.is_empty());

        let mut output = Vec::new();
        super::print_workspace_summary_to(
            &mut output,
            &filesystem,
            &loaded.workspace,
            &loaded.dependencies,
        )
        .unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(
            summary.contains("Streamed-scene AssetBundle PathID 42: SceneHashes, 1 path mappings")
        );
        assert!(summary.contains("collection \"CAB-Scene\" (workspace file"));
        assert!(summary.contains(": Assets/Scenes/BundleScene"));
        assert!(summary.contains(
            "effective scene path: Assets/Scenes/BundleScene (streamed-scene override; assignments=1, conflicting=false)"
        ));
        assert!(summary.contains(
            "SceneDefinition model 0: name=\"BundleScene\", path=\"Assets/Scenes/BundleScene\" (streamed-scene AssetBundle)"
        ));
        assert!(summary.contains("collection files: [0]"));
        assert!(summary
            .contains("scene GUID: preserve [0x00000001, 0x00000002, 0x00000003, 0x00000004]"));
        assert!(
            summary.contains("hierarchy AssetBundleName: null"),
            "{summary}"
        );
        assert!(
            summary.contains(
                "hierarchy AssetBundleName candidate: asset \"data.unity3d\" (first IAssetBundle in FetchAssets order)"
            ),
            "{summary}"
        );
        assert!(summary.contains("GeneratedObjectBase.AssetBundleName setter is a no-op"));
        assert!(
            summary.contains("Generated Prefab Transforms planned for non-scene GameObjects: 0")
        );
        assert!(summary.contains(
            "PrefabProcessor identity plan: 0 scene GameObjects, 0 existing PrefabInstances, 0 existing hierarchies, 0 existing instances skipped, 0 generated hierarchies, 0 virtual Transform entries in planned hierarchies, 0 remaining GameObjects skipped, 0 processed GameObjects, 0 hierarchy entries, 1 root issues, 0 incomplete hierarchies, complete=false, stopped=false"
        ), "{summary}");
        assert!(summary.contains(
            "GetRoot is indeterminate: Some(IncompleteCollectionGraph { file_index: 2 })"
        ));
        assert!(summary.contains(
            "hierarchy file 0: 0 GameObjects, 0 Transforms, 1 scene assets (1 managers, 0 PrefabInstances, 0 SceneRoots), 0 component candidates (0 Transform IDs, 0 GameObject edges, 0 m_GameObject fields), 0 missing Transform plans, 0 relationships, 0 roots, 0 graph issues, 0 root issues, 0 reciprocal issues, scene graph complete=true, reference scans complete=true, overall complete=true"
        ));
    }

    #[test]
    fn workspace_asset_bundle_plan_assigns_only_unique_local_container_targets() {
        let mut filesystem = VirtualFileSystem::new();
        let scene_file = make_occlusion_settings_file([1, 2, 3, 4]);
        let streamed_bundle = make_asset_bundle_file_with_local_reference();
        let webdata = make_webdata_fixture(&[
            ("CAB-Scene", &scene_file),
            ("assetbundle.assets", &streamed_bundle),
        ]);
        insert_vfs_file(
            &mut filesystem,
            "/game/Game_Data/globalgamemanagers",
            &make_generation_10_file(),
        );
        insert_vfs_file(&mut filesystem, "/game/Game_Data/data.unity3d", &webdata);
        let structure = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: Some("/game/Game_Data".to_owned()),
            data_paths: vec!["/game/Game_Data".to_owned()],
        };
        let loaded = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap();
        let object_workspace = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &loaded.workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let collection_workspace = WorkspaceAssetCollections::build(
            &loaded.workspace,
            &object_workspace,
            WorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let plan = assetripper::processing::asset_bundle::plan_workspace_asset_bundle_names(
            &loaded.workspace,
            &object_workspace,
            &collection_workspace,
            Default::default(),
        )
        .unwrap();

        assert!(plan.is_complete(), "{:?}", plan.issues);
        assert_eq!(plan.bundles.len(), 2);
        let source_bundle = plan
            .bundles
            .iter()
            .position(|bundle| bundle.source_path_id == 42)
            .unwrap();
        assert_eq!(plan.bundles[source_bundle].source_name, "data.unity3d");
        assert_eq!(plan.bundles[source_bundle].container_references, 0..1);
        assert_eq!(plan.assignments.len(), 1);
        assert_eq!(plan.assignments[0].bundle_record_index, source_bundle);
        assert_eq!(plan.assignments[0].target_object_index, 1);
        assert_eq!(plan.assignments[0].target_path_id, 43);
        assert_eq!(
            plan.asset_name_for_assignment(&plan.assignments[0]),
            Some("data.unity3d")
        );
        assert_eq!(
            plan.container_references[plan.assignments[0].reference_record_index]
                .mapping
                .target,
            assetripper::processing::asset_bundle::AssetBundleContainerTarget::LocalObject {
                object_index: 1
            }
        );
        let bundle_limit_error =
            assetripper::processing::asset_bundle::apply_workspace_asset_bundle_paths(
                &loaded.workspace,
                &object_workspace,
                &collection_workspace,
                &plan,
                assetripper::processing::asset_bundle::BundledAssetsExportMode::DirectExport,
                assetripper::processing::asset_bundle::WorkspaceAssetExportPathTable::empty(),
                assetripper::processing::asset_bundle::WorkspaceAssetExportPathLimits {
                    max_bundles: 1,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            bundle_limit_error,
            assetripper::processing::asset_bundle::WorkspaceAssetBundlePathError::CountLimitExceeded {
                resource: "AssetBundle path source records",
                count: 2,
                limit: 1,
            }
        ));
        let target_file_index = plan.assignments[0].target_file_index;
        let target_object_index = plan.assignments[0].target_object_index;
        let bundle_source_file_index = plan.bundles[source_bundle].source_file_index;
        let bundle_source_object_index = plan.bundles[source_bundle].source_object_index;

        let (direct_paths, direct_report) =
            assetripper::processing::asset_bundle::apply_workspace_asset_bundle_paths(
                &loaded.workspace,
                &object_workspace,
                &collection_workspace,
                &plan,
                assetripper::processing::asset_bundle::BundledAssetsExportMode::DirectExport,
                assetripper::processing::asset_bundle::WorkspaceAssetExportPathTable::empty(),
                Default::default(),
            )
            .unwrap();
        let direct_target = direct_paths
            .get(target_file_index, target_object_index)
            .expect("assigned target path metadata");
        assert_eq!(
            direct_target.asset_bundle_name.as_deref(),
            Some("data.unity3d")
        );
        assert_eq!(
            direct_target.original_path.as_deref(),
            Some("Assets/Scenes/Other.unity")
        );
        assert_eq!(direct_report.assigned_bundle_names, 1);
        assert_eq!(direct_report.assigned_original_paths, 1);

        let retained_path = "Assets/Prior/Existing.asset";
        let retained_directory = "Assets/Prior";
        let prior_bundle_name = "prior.bundle";
        let initial_paths =
            assetripper::processing::asset_bundle::WorkspaceAssetExportPathTable::from_records(
                vec![
                    assetripper::processing::asset_bundle::WorkspaceAssetExportPath {
                        file_index: target_file_index,
                        object_index: target_object_index,
                        path_id: plan.assignments[0].target_path_id,
                        asset_bundle_name: Some(prior_bundle_name.to_owned()),
                        original_path: Some(retained_path.to_owned()),
                        original_directory: Some(retained_directory.to_owned()),
                        ..Default::default()
                    },
                ],
                Default::default(),
            )
            .unwrap();
        let (type_grouped_paths, type_grouped_report) =
            assetripper::processing::asset_bundle::apply_workspace_asset_bundle_paths(
                &loaded.workspace,
                &object_workspace,
                &collection_workspace,
                &plan,
                assetripper::processing::asset_bundle::BundledAssetsExportMode::GroupByAssetType,
                initial_paths,
                Default::default(),
            )
            .unwrap();
        let type_grouped_target = type_grouped_paths
            .get(target_file_index, target_object_index)
            .expect("retained export path metadata");
        assert_eq!(
            type_grouped_target.asset_bundle_name.as_deref(),
            Some("data.unity3d")
        );
        assert_eq!(
            type_grouped_target.original_path.as_deref(),
            Some(retained_path)
        );
        assert_eq!(
            type_grouped_target.original_directory.as_deref(),
            Some(retained_directory)
        );
        assert_eq!(
            type_grouped_report.metadata_string_bytes,
            "data.unity3d".len()
                + retained_path.len()
                + retained_directory.len()
                + "Existing".len()
                + "asset".len()
        );

        let (grouped_paths, grouped_report) =
            assetripper::processing::asset_bundle::apply_workspace_asset_bundle_paths(
                &loaded.workspace,
                &object_workspace,
                &collection_workspace,
                &plan,
                assetripper::processing::asset_bundle::BundledAssetsExportMode::GroupByBundleName,
                assetripper::processing::asset_bundle::WorkspaceAssetExportPathTable::empty(),
                Default::default(),
            )
            .unwrap();
        let grouped_target = grouped_paths
            .get(target_file_index, target_object_index)
            .expect("assigned target path metadata");
        let group_path = format!(
            "Assets/AssetBundles{}data.unity3d{}Scenes/Other.unity",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        let group_directory = format!(
            "Assets/AssetBundles{}data.unity3d{}Scenes",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            grouped_target.original_path.as_deref(),
            Some(group_path.as_str())
        );
        assert_eq!(
            grouped_target.original_directory.as_deref(),
            Some(group_directory.as_str())
        );
        let bundle_source = grouped_paths
            .get(bundle_source_file_index, bundle_source_object_index)
            .expect("GroupByBundleName collection default");
        let group_type_directory = format!(
            "Assets/AssetBundles{}data.unity3d{}AssetBundle",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            bundle_source.original_directory.as_deref(),
            Some(group_type_directory.as_str())
        );
        let scene_collection = collection_workspace
            .collections()
            .iter()
            .find(|collection| collection.fixed_name == "cab-scene")
            .expect("source container scene collection");
        let scene_manager = object_workspace
            .objects()
            .get(scene_collection.object_range.start)
            .expect("scene manager object");
        let scene_manager_paths = grouped_paths
            .get(scene_manager.file_index, scene_manager.object_index)
            .expect("GroupByBundleName scene collection default");
        let scene_group_directory = format!(
            "Assets/AssetBundles{}data.unity3d{}OcclusionCullingSettings",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            scene_manager_paths.original_directory.as_deref(),
            Some(scene_group_directory.as_str())
        );
        assert_eq!(grouped_report.assigned_bundle_names, 1);
        assert_eq!(grouped_report.assigned_original_paths, 1);
        assert_eq!(grouped_report.assigned_collection_directories, 2);
    }

    #[test]
    fn resource_manager_paths_select_longest_container_path_and_restore_asset_name() {
        let mut filesystem = VirtualFileSystem::new();
        let input_path = "/inputs/resource.assets";
        insert_vfs_file(
            &mut filesystem,
            input_path,
            &make_resource_manager_file_with_local_reference(),
        );
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[DependencyFilePath {
                file_name: "resource.assets".to_owned(),
                virtual_path: input_path.to_owned(),
            }],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let collections = WorkspaceAssetCollections::build(
            &workspace,
            &objects,
            WorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let resource_plan =
            assetripper::processing::asset_bundle::plan_workspace_resource_manager_paths(
                &workspace,
                &objects,
                &collections,
                Default::default(),
            )
            .unwrap();
        assert!(resource_plan.is_complete(), "{:?}", resource_plan.issues);
        assert_eq!(resource_plan.resource_manager_count, 1);
        assert_eq!(resource_plan.container_entry_count, 2);
        assert_eq!(resource_plan.assignments.len(), 2);
        assert_eq!(
            resource_plan.assignments[0].path,
            "Textures/inner/wood.shader"
        );
        assert_eq!(resource_plan.assignments[1].path, "wood.shader");
        assert_eq!(resource_plan.assignments[0].file_id, 0);
        let assignment_limit_error =
            assetripper::processing::asset_bundle::plan_workspace_resource_manager_paths(
                &workspace,
                &objects,
                &collections,
                assetripper::processing::asset_bundle::WorkspaceResourceManagerPathLimits {
                    max_assignments: 1,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            assignment_limit_error,
            assetripper::processing::asset_bundle::WorkspaceResourceManagerPathPlanError::CountLimitExceeded {
                resource: "ResourceManager path assignments",
                count: 2,
                limit: 1,
            }
        ));
        assert!(matches!(
            objects.objects()[1]
                .payload
                .as_ref()
                .unwrap()
                .root
                .unique_field_in_base_chain("m_Name")
                .unwrap(),
            Some(assetripper::serialized_file::object::SerializedValue::StringBytes(bytes))
                if bytes == b"Wood"
        ));

        let bundle_plan =
            assetripper::processing::asset_bundle::WorkspaceAssetBundleNamePlan::default();
        let (paths, report) =
            assetripper::processing::asset_bundle::apply_workspace_original_paths(
                &workspace,
                &objects,
                &collections,
                assetripper::processing::asset_bundle::WorkspaceOriginalPathPlans {
                    asset_bundles: &bundle_plan,
                    resource_managers: &resource_plan,
                },
                assetripper::processing::asset_bundle::BundledAssetsExportMode::DirectExport,
                assetripper::processing::asset_bundle::WorkspaceAssetExportPathTable::empty(),
                Default::default(),
            )
            .unwrap();
        let target = paths.get(0, 1).expect("ResourceManager target metadata");
        let expected_path = format!(
            "Assets/Resources{}Textures/inner{}Wood.shader",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            target.original_path.as_deref(),
            Some(expected_path.as_str())
        );
        assert_eq!(target.original_name.as_deref(), Some("Wood"));
        let expected_shader_directory = format!(
            "Assets/Resources{}Textures/inner",
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            target.override_directory.as_deref(),
            Some(expected_shader_directory.as_str())
        );
        assert_eq!(target.override_name.as_deref(), Some("Wood"));
        assert_eq!(target.override_extension.as_deref(), Some("shader"));
        assert_eq!(report.assigned_resource_manager_original_paths, 1);
        assert_eq!(report.restored_original_names, 1);
        assert_eq!(report.initialized_shader_overrides, 1);
    }

    #[test]
    fn platform_directory_inspection_preserves_and_expands_registered_containers() {
        let input = TestInputDirectory::new_windows_platform();
        let (mut filesystem, selected_paths, _) =
            super::mount_input_directories(std::slice::from_ref(&input.directory), true).unwrap();
        let platform = detect_platform_structure(
            &filesystem,
            &selected_paths,
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(platform.platform, PlatformKind::Windows);

        let loaded = platform
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap();

        assert_eq!(loaded.workspace.files().len(), 2);
        assert_eq!(loaded.initial_container_expansions.len(), 1);
        assert_eq!(
            loaded.initial_container_expansions[0].kind,
            FileKind::UnityWebData
        );
        assert!(matches!(
            loaded.dependencies.requests[0].outcome,
            SerializedFileDependencyOutcome::AlreadyLoaded { .. }
        ));
        assert_eq!(
            loaded.dependencies.requests[1].outcome,
            SerializedFileDependencyOutcome::ContainerAlreadyExpanded {
                virtual_path: format!("{}/data.unity3d", platform.data_paths[0]),
                serialized_files_loaded: 1,
                matching_serialized_files: 0,
            }
        );
        super::inspect_directories(std::slice::from_ref(&input.directory)).unwrap();
    }

    #[test]
    fn platform_directory_inspection_unions_mixed_inputs_and_resolves_their_references() {
        let platform_input = TestInputDirectory::new_windows_platform();
        let mixed_input = TestInputDirectory::new();
        let primary_path = platform_input
            .files
            .iter()
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name == "globalgamemanagers")
            })
            .expect("synthetic platform input has globalgamemanagers");
        fs::write(
            primary_path,
            make_generation_10_file_with_externals(&["main.assets"]),
        )
        .unwrap();

        let selected_directories = [
            platform_input.directory.clone(),
            mixed_input.directory.clone(),
        ];
        let (mut filesystem, selected_paths, _) =
            super::mount_input_directories(&selected_directories, true).unwrap();
        let platform = detect_platform_structure(
            &filesystem,
            &selected_paths,
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .expect("the first selected directory is a Windows platform root");
        let mixed = super::MixedGameStructure::from_vfs_paths_with_consumed_paths(
            &filesystem,
            &selected_paths,
            &platform.consumed_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        assert_eq!(mixed.serialized_file_inputs.len(), 1);

        let loaded = platform
            .load_primary_serialized_workspace_with_additional_inputs(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
                &mixed.serialized_file_inputs,
                &mixed.registered_files,
                &mixed.data_paths,
            )
            .unwrap();
        assert_eq!(loaded.workspace.files().len(), 3);
        let main_assets_request = loaded
            .dependencies
            .requests
            .iter()
            .find(|request| request.dependency_name == "main.assets")
            .expect("platform external table requests the mixed SerializedFile");
        let main_assets_index = match main_assets_request.outcome {
            SerializedFileDependencyOutcome::AlreadyLoaded { file_index } => file_index,
            ref outcome => {
                panic!("mixed SerializedFile was not resolved in the common workspace: {outcome:?}")
            }
        };
        assert_eq!(
            loaded.workspace.files()[main_assets_index].file_name,
            "main.assets"
        );

        let mut output = Vec::new();
        super::inspect_directories_to(&selected_directories, &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(summary.contains("mixed registrations: 1"), "{summary}");
        assert!(summary.contains("SerializedFiles: 3"), "{summary}");
        assert!(summary.contains("external requests: 1"), "{summary}");
        assert!(summary.contains("resolved: true"), "{summary}");
        let source_row = include_str!("io/cli_inspect_fixture.txt")
            .lines()
            .find(|line| line.starts_with("PLATFORM_MIXED|"))
            .expect("source runtime fixture contains the platform/mixed result");
        assert_eq!(
            source_row,
            "PLATFORM_MIXED|input_names=globalgamemanagers,main.assets|processors_requested=1"
        );
    }

    fn os_args(words: &[&str]) -> Vec<std::ffi::OsString> {
        words.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn cli_command_classifier_pins_every_accepted_spelling() {
        use super::{classify_cli_command, CliCommand};
        assert_eq!(classify_cli_command("--help"), CliCommand::Help);
        assert_eq!(classify_cli_command("-h"), CliCommand::Help);
        assert_eq!(classify_cli_command("help"), CliCommand::Help);
        assert_eq!(classify_cli_command("--version"), CliCommand::Version);
        assert_eq!(classify_cli_command("-V"), CliCommand::Version);
        assert_eq!(
            classify_cli_command("export-object-yaml"),
            CliCommand::ExportObjectYaml
        );
        assert_eq!(
            classify_cli_command("export-file-yaml"),
            CliCommand::ExportFileYaml
        );
        assert_eq!(
            classify_cli_command("export-texture"),
            CliCommand::ExportTexture
        );
        assert_eq!(classify_cli_command("export-mesh"), CliCommand::ExportMesh);
        assert_eq!(
            classify_cli_command("export-shader"),
            CliCommand::ExportShader
        );
        assert_eq!(
            classify_cli_command("export-audio"),
            CliCommand::ExportAudio
        );
        assert_eq!(
            classify_cli_command("export-project"),
            CliCommand::ExportProject
        );
        assert_eq!(classify_cli_command("inspect"), CliCommand::Inspect);
        assert_eq!(
            classify_cli_command("inspect-files"),
            CliCommand::InspectFiles
        );
        // Matching is exact and case sensitive; near misses stay unknown.
        for unknown in [
            "",
            "Inspect",
            "INSPECT",
            "export",
            "--he",
            "--hel",
            "-v",
            "--Version",
            "serve",
            "inspect files",
        ] {
            assert_eq!(
                classify_cli_command(unknown),
                CliCommand::Unknown,
                "{unknown:?}"
            );
        }
    }

    #[test]
    fn cli_dispatch_rejects_missing_and_unknown_commands() {
        // No command at all.
        let error = super::run(Vec::new().into_iter()).unwrap_err();
        assert_eq!(error.to_string(), "a command is required");
        // Unknown commands name the offender.
        let error = super::run(os_args(&["frobnicate"]).into_iter()).unwrap_err();
        assert_eq!(error.to_string(), "unknown command 'frobnicate'");
        // Inspect commands without inputs name the command.
        let error = super::run(os_args(&["inspect"]).into_iter()).unwrap_err();
        assert_eq!(error.to_string(), "inspect requires at least one input");
        let error = super::run(os_args(&["inspect-files"]).into_iter()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "inspect-files requires at least one input"
        );
    }

    #[test]
    fn cli_dispatch_accepts_help_and_version_flags() {
        for command in ["--help", "-h", "help", "--version", "-V"] {
            super::run(os_args(&[command]).into_iter()).unwrap();
        }
    }

    #[test]
    fn load_process_plan_follows_the_bundle_presence_gate() {
        use super::{plan_load_process, LoadProcessPlan};
        // Any loaded collection runs processing, like `HasAnyAssetCollections()`.
        assert_eq!(plan_load_process(1), LoadProcessPlan::Process);
        assert_eq!(plan_load_process(2), LoadProcessPlan::Process);
        assert_eq!(plan_load_process(usize::MAX), LoadProcessPlan::Process);
        // An empty bundle warns, skips processing, and still returns.
        assert_eq!(plan_load_process(0), LoadProcessPlan::ReturnUnprocessed);
    }

    #[test]
    fn explicit_inspect_plan_prefers_workspace_then_roots_then_failure() {
        use super::{plan_explicit_inspect, ExplicitInspectPlan};
        // Serialized inputs always load, even alongside roots.
        assert_eq!(
            plan_explicit_inspect(1, 0),
            ExplicitInspectPlan::LoadWorkspace
        );
        assert_eq!(
            plan_explicit_inspect(2, 3),
            ExplicitInspectPlan::LoadWorkspace
        );
        // Roots without serialized inputs still report, like `FromPaths` keeping
        // `ResourceFile` inputs without any `SerializedFile`.
        assert_eq!(
            plan_explicit_inspect(0, 1),
            ExplicitInspectPlan::ReportRootsOnly
        );
        // Neither names nothing to inspect.
        assert_eq!(
            plan_explicit_inspect(0, 0),
            ExplicitInspectPlan::FailNoRecognizedInputs
        );
    }

    #[test]
    fn explicit_file_inspection_reports_roots_without_serialized_inputs() {
        let directory = TestInputDirectory::new_root();
        let resource_path = directory.join("direct.resS");
        fs::write(&resource_path, b"explicit root stream").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![resource_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_explicit_files_to(std::slice::from_ref(&resource_path), &mut output)
            .unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(summary.contains("Explicit GameBundle root ResourceFiles: 1"));
        assert!(summary.contains("No SerializedFile inputs were selected."));
        assert!(!summary.contains("explicit file import:"));
        drop(input);
    }

    #[test]
    fn explicit_file_inspection_fails_without_recognized_inputs() {
        let directory = TestInputDirectory::new_root();
        // A web-data container with no serialized entries mounts cleanly but
        // registers nothing, so the selection names nothing to inspect.
        let container_path = directory.join("payload.bin");
        fs::write(
            &container_path,
            make_webdata_fixture(&[("notes.txt", b"plain resource bytes")]),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![container_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        let error =
            super::inspect_explicit_files_to(std::slice::from_ref(&container_path), &mut output)
                .unwrap_err();
        assert_eq!(
            error.to_string(),
            "the selected files contain no recognized SerializedFile or root ResourceFile inputs"
        );
        drop(input);
    }
}
