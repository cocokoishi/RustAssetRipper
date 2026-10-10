use super::*;

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
pub(super) enum LoadProcessPlan {
    /// The bundle holds asset collections: quiet load, then run processing.
    Process,
    /// The bundle holds none: warn that no valid assets were found, skip
    /// processing, and return the bundle unprocessed.
    ReturnUnprocessed,
}

/// Decides whether a loaded bundle with `asset_collection_count` collections runs
/// the processing phase.
pub(super) const fn plan_load_process(asset_collection_count: usize) -> LoadProcessPlan {
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
pub(super) enum ExplicitInspectPlan {
    /// No recognized inputs: fail the command.
    FailNoRecognizedInputs,
    /// Roots only: print the catalog plus the no-SerializedFile notice.
    ReportRootsOnly,
    /// Serialized inputs present: load the workspace.
    LoadWorkspace,
}

/// Selects the `inspect-files` outcome from the recognized SerializedFile and
/// explicit root resource counts.
pub(super) const fn plan_explicit_inspect(
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

pub(super) fn inspect_explicit_files(input_files: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    inspect_explicit_files_to(input_files, &mut output)
}

pub(super) fn inspect_explicit_files_to<W: io::Write>(
    input_files: &[PathBuf],
    output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mut mount_report) = mount_input_files(input_files)?;
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

    let (workspace, dependencies) = load_workspace_with_mount_report(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        WorkspaceLoadLimits::default(),
        &mut mount_report,
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

pub(super) fn print_explicit_root_resource_catalog_to<W: io::Write>(
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
pub(super) fn deduplicate_logical_input_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for path in paths {
        if seen.insert(logical_file_path(path)) {
            unique.push(path.clone());
        }
    }
    unique
}

pub(super) fn mount_input_files(
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

pub(super) fn mount_input_files_for_export(
    input_files: &[PathBuf],
) -> Result<(VirtualFileSystem, Vec<String>, BatchMountReport), Box<dyn Error>> {
    let mounted = mount_input_files(input_files)?;
    ensure_no_batch_container_failures(&mounted.2)?;
    Ok(mounted)
}

pub(super) fn ensure_no_batch_container_failures(
    report: &BatchMountReport,
) -> Result<(), Box<dyn Error>> {
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

pub(super) fn ensure_no_workspace_container_failures(
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

pub(super) fn print_batch_container_failures_to<W: io::Write>(
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

pub(super) fn print_workspace_container_failures_to<W: io::Write>(
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

pub(super) fn mount_project_inputs(
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
        let mut next = if metadata.is_dir() {
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
        merge_mount_report(&mut total, &mut next)?;
        selected_paths.push(destination_root);
    }
    Ok((filesystem, selected_paths, total))
}

pub(super) fn inspect_directories(input_directories: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    inspect_directories_to(input_directories, &mut output)
}

pub(super) fn inspect_directories_to<W: io::Write>(
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
    let (mut filesystem, selected_paths, mut total_mount_report) =
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
    let (workspace, dependencies) = load_workspace_with_mount_report(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        WorkspaceLoadLimits::default(),
        &mut total_mount_report,
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

pub(super) fn mount_input_directories(
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
        let mut report = if preserve_containers {
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
        merge_mount_report(&mut total_mount_report, &mut report)?;
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

pub(super) fn print_reference_graph_summary_to<W: io::Write>(
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

pub(super) fn write_unresolved_reference_diagnostic<W: io::Write>(
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

pub(super) fn print_streamed_resource_summary_to<W: io::Write>(
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

pub(super) fn increment_streamed_resource_count(
    count: &mut usize,
    description: &str,
) -> Result<(), Box<dyn Error>> {
    *count = count
        .checked_add(1)
        .ok_or_else(|| invalid_data_owned(format!("{description} overflows")))?;
    Ok(())
}

pub(super) fn write_streamed_resource_diagnostic<W: io::Write>(
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

pub(super) struct BoundedDiagnosticText<'a> {
    text: &'a str,
    truncated: bool,
}

pub(super) fn bounded_diagnostic_text(value: &str, max_bytes: usize) -> BoundedDiagnosticText<'_> {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    BoundedDiagnosticText {
        text: &value[..end],
        truncated: end < value.len(),
    }
}
