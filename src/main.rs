use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use assetripper::io::dependency::DependencyIndexLimits;
use assetripper::io::filesystem::VirtualFileSystem;
use assetripper::io::game_structure::{MixedGameStructure, MixedGameStructureLimits};
use assetripper::io::platform_file_inventory::{
    PlatformFileInventoryLimits, PlatformFileInventoryOptions, PlatformWorkspaceLoadOptions,
};
use assetripper::io::platform_structure::{detect_platform_structure, PlatformStructureLimits};
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
use assetripper::scheme::{
    BatchMountLimits, BatchMountReport, ContainerMountLimits, SchemeReader, SchemeReaderLimits,
};
use assetripper::serialized_file::collection_workspace::{
    WorkspaceAssetCollections, WorkspaceCollectionLimits,
};
use assetripper::serialized_file::file::SerializedFile;
use assetripper::serialized_file::metadata::ObjectInfo;
use assetripper::serialized_file::object::ObjectDecodeLimits;
use assetripper::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
use assetripper::serialized_file::reference_graph::{
    MultiFileObjectReference, MultiFilePPtrResolution, MultiFileReferenceGraph,
    MultiFileReferenceGraphLimits,
};
use assetripper::serialized_file::workspace::{
    SerializedFileDependencyReport, SerializedFileWorkspace, SerializedFileWorkspaceLimits,
    WorkspaceSerializedBundle,
};

const MAX_INPUT_DIRECTORIES: usize = 100_000;
const MAX_STREAMED_SCENE_BUNDLE_OBJECTS: usize = 1_000_000;
const MAX_TOTAL_STREAMED_SCENE_BUNDLE_ENTRIES: usize = 1_000_000;
const MAX_TOTAL_STREAMED_SCENE_BUNDLE_STRING_BYTES: usize = 256 * 1024 * 1024;
const MAX_TOTAL_STREAMED_SCENE_PATH_MAPPINGS: usize = 1_000_000;
const MAX_OBJECT_DECODE_DIAGNOSTICS: usize = 128;
const MAX_REFERENCE_GRAPH_DIAGNOSTICS: usize = 128;
const MAX_GENERATED_TRANSFORM_DIAGNOSTICS: usize = 128;

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

fn run(arguments: impl Iterator<Item = OsString>) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    let Some(command) = arguments.next() else {
        print_usage();
        return Err(invalid_input("a command is required"));
    };
    let command = command
        .into_string()
        .map_err(|_| invalid_input("the command name must be valid UTF-8"))?;
    if command == "--help" || command == "-h" || command == "help" {
        print_usage();
        return Ok(());
    }
    if command == "--version" || command == "-V" {
        println!("assetripper-rs {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if command != "inspect" {
        print_usage();
        return Err(invalid_input(format!("unknown command '{command}'")));
    }

    let mut input_directories = Vec::new();
    for argument in arguments {
        if input_directories.len() >= MAX_INPUT_DIRECTORIES {
            return Err(invalid_input(format!(
                "input directory count exceeds {MAX_INPUT_DIRECTORIES}"
            )));
        }
        input_directories.push(PathBuf::from(argument));
    }
    if input_directories.is_empty() {
        print_usage();
        return Err(invalid_input("inspect requires at least one directory"));
    }

    inspect_directories(&input_directories)
}

fn inspect_directories(input_directories: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, total_mount_report) =
        mount_input_directories(input_directories, true)?;
    let structure = detect_platform_structure(
        &filesystem,
        &selected_paths,
        PlatformStructureLimits::default(),
    )?;
    if let Some(structure) = structure {
        let inventory = structure.collect_game_files(
            &filesystem,
            PlatformFileInventoryOptions::default(),
            PlatformFileInventoryLimits::default(),
        )?;
        let loaded = structure.load_primary_serialized_workspace(
            &mut filesystem,
            PlatformWorkspaceLoadOptions::default(),
        )?;
        println!(
            "platform: {:?}; source registrations: {}; scripting backend: {:?}; assemblies: {}; initial containers: {}",
            structure.platform,
            inventory.registered_files.len(),
            inventory.scripting_backend,
            inventory.assemblies.len(),
            loaded.initial_container_expansions.len()
        );
        println!(
            "platform directory import: {} logical files, {} source bytes",
            total_mount_report.discovered_inputs, total_mount_report.discovered_source_bytes
        );
        print_workspace_summary(&filesystem, &loaded.workspace, &loaded.dependencies)?;
        return Ok(());
    }

    drop(filesystem);
    let (mut filesystem, selected_paths, total_mount_report) =
        mount_input_directories(input_directories, false)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected directories contain no recognized SerializedFile inputs",
        ));
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
    println!(
        "platform: mixed; registered files: {}; assemblies: {}",
        mixed.registered_files.len(),
        mixed.assemblies.len()
    );
    println!(
        "recursively mounted directory import: {} logical inputs, {} leaf files, {} output bytes",
        total_mount_report.discovered_inputs,
        total_mount_report.mounted_entries,
        total_mount_report.mounted_bytes
    );
    print_workspace_summary(&filesystem, &workspace, &dependencies)?;
    Ok(())
}

fn mount_input_directories(
    input_directories: &[PathBuf],
    preserve_containers: bool,
) -> Result<(VirtualFileSystem, Vec<String>, BatchMountReport), Box<dyn Error>> {
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

#[cfg(not(test))]
fn print_workspace_summary(
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    dependencies: &SerializedFileDependencyReport,
) -> Result<(), Box<dyn Error>> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    print_workspace_summary_to(&mut output, filesystem, workspace, dependencies)
}

#[cfg(test)]
fn print_workspace_summary(
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    dependencies: &SerializedFileDependencyReport,
) -> Result<(), Box<dyn Error>> {
    print_workspace_summary_to(&mut io::sink(), filesystem, workspace, dependencies)
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

fn print_workspace_summary_to<W: io::Write>(
    output: &mut W,
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    dependencies: &SerializedFileDependencyReport,
) -> Result<(), Box<dyn Error>> {
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

    writeln!(output, "SerializedFiles: {}", workspace.files().len())?;
    let decoded_objects =
        ObjectWorkspace::load_from_vfs(filesystem, workspace, ObjectWorkspaceLimits::default())?;
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
    Ok(())
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn print_usage() {
    println!(
        "Usage:\n  assetripper-rs inspect <input-directory> [input-directory ...]\n  assetripper-rs --help\n  assetripper-rs --version\n\nMounts each directory into an isolated VFS root, detects platform or mixed inputs, and reports the parsed SerializedFiles."
    );
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{
        merge_mount_report, remaining_budget_u64, remaining_budget_usize,
        source_bundle_for_collection, WorkspaceSerializedBundle,
    };
    use assetripper::io::filesystem::VirtualFileSystem;
    use assetripper::io::platform_file_inventory::PlatformWorkspaceLoadOptions;
    use assetripper::io::platform_structure::{
        detect_platform_structure, PlatformKind, PlatformStructure, PlatformStructureLimits,
    };
    use assetripper::scheme::BatchMountReport;
    use assetripper::scheme::FileKind;
    use assetripper::serialized_file::workspace::SerializedFileDependencyOutcome;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn streamed_scene_bundle_lookup_uses_direct_collections_without_a_scope_path() {
        let bundles = [WorkspaceSerializedBundle {
            scope_path: None,
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
            let index = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::current_dir()
                .unwrap()
                .join("target")
                .join(format!("cli-input-{}-{index}", std::process::id()));
            fs::create_dir(&directory).unwrap();
            directory
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
        put_u16_le(output, 1);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, 0);
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
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, 1);
        put_asset_bundle_string_payload(&mut payload, "Assets/Scenes/Other.unity");
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 0);
        payload.extend_from_slice(&0_i64.to_le_bytes());
        payload.push(1);
        put_i32_le(&mut payload, 1);
        put_asset_bundle_string_payload(&mut payload, "Assets/Scenes/BundleScene.unity");
        put_asset_bundle_string_payload(&mut payload, "CAB-Scene");

        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 142);
        put_u16_le(&mut metadata, 142);
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
        put_i32_le(&mut payload, 0);
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

    fn make_build_settings_file(scene_paths: &[&str]) -> Vec<u8> {
        make_build_settings_file_with_path_id(scene_paths, 42)
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

    fn make_webdata_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let header_size = 20usize
            + entries
                .iter()
                .map(|(name, _)| 9usize + name.len())
                .sum::<usize>();
        let mut container = b"UnityWebData1.0\0".to_vec();
        container.extend_from_slice(&i32::try_from(header_size).unwrap().to_le_bytes());
        let mut data_offset = header_size;
        for (name, bytes) in entries {
            container.extend_from_slice(&i32::try_from(data_offset).unwrap().to_le_bytes());
            container.extend_from_slice(&i32::try_from(bytes.len()).unwrap().to_le_bytes());
            container.push(u8::try_from(name.len()).unwrap());
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
        };
        let next = BatchMountReport {
            directory_entries_scanned: 4,
            discovered_inputs: 4,
            discovered_source_bytes: 20,
            completed_inputs: 3,
            mounted_entries: 5,
            mounted_bytes: 30,
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
    fn directory_inspection_loads_a_synthetic_serialized_file() {
        let input = TestInputDirectory::new();
        super::inspect_directories(std::slice::from_ref(&input.directory)).unwrap();
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
            summary.contains("Generated Prefab Transforms planned for non-scene GameObjects: 0")
        );
        assert!(summary.contains(
            "hierarchy file 0: 0 GameObjects, 0 Transforms, 1 scene assets (1 managers, 0 PrefabInstances, 0 SceneRoots), 0 component candidates (0 Transform IDs, 0 GameObject edges, 0 m_GameObject fields), 0 missing Transform plans, 0 relationships, 0 roots, 0 graph issues, 0 root issues, 0 reciprocal issues, scene graph complete=true, reference scans complete=true, overall complete=true"
        ));
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
}
