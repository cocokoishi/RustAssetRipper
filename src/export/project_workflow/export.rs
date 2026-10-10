use super::*;

fn validate_workspace_project_export<'inputs, 'provider, 'graph>(
    inputs: WorkspaceProjectExportInputs<'inputs, 'provider, 'graph>,
) -> Result<WorkspaceProjectExportContext<'inputs, 'provider, 'graph>, Box<dyn Error>> {
    let (editor_version_text, project_version) = workspace_project_version(inputs.workspace)?;
    let has_mono_scripts = inputs
        .objects
        .objects()
        .iter()
        .any(|object| object.factory.class_id == 115);
    let reference_assembly_maps =
        source_reference_assembly_maps(has_mono_scripts, inputs.managed_assemblies)?;
    let settings_facts =
        ProjectExportSettings::from_export_settings(inputs.settings, project_version);
    if inputs.objects.report().object_count > inputs.limits.max_objects {
        return Err(invalid_project(format!(
            "project export has {} objects, exceeding the limit {}",
            inputs.objects.report().object_count,
            inputs.limits.max_objects
        )));
    }
    let opaque_objects = opaque_factory_object_keys(inputs.objects)?;
    let incomplete_objects = inputs
        .objects
        .objects()
        .iter()
        .filter(|object| {
            !object.is_fully_materialized()
                && !opaque_objects.contains(&(object.file_index, object.object_index))
        })
        .take(4)
        .map(|object| {
            let detail = object
                .payload
                .as_ref()
                .err()
                .map(ToString::to_string)
                .or_else(|| object.factory.log_message.clone())
                .unwrap_or_else(|| "the source factory retained unread structure data".into());
            format!(
                "file {} object {} PathID {} ({:?}): {detail}",
                object.file_index, object.object_index, object.path_id, object.factory.kind
            )
        })
        .collect::<Vec<_>>();
    if !incomplete_objects.is_empty() {
        return Err(invalid_project(format!(
            "project export requires every decoded object to be fully materialized; other than source-supported unknown/unreadable raw fallbacks, {} object(s) are incomplete: [{}]",
            inputs.objects.report().undecoded_objects.saturating_sub(opaque_objects.len()),
            incomplete_objects.join("; ")
        )));
    }
    if !inputs
        .reference_graph
        .is_complete_except_opaque_object_payloads(|file_index, object_index| {
            opaque_objects.contains(&(file_index, object_index))
        })
    {
        let incomplete_scans = (0..inputs.reference_graph.file_count())
            .filter_map(|file_index| {
                let graph = inputs.reference_graph.managed_graph_for_file(file_index)?;
                (!graph.is_complete()).then(|| {
                    let issues = graph.issues();
                    format!(
                        "{}: {} scan issue(s), first issue {:?}",
                        inputs.reference_graph.normalized_file_names()[file_index],
                        issues.len(),
                        issues.first()
                    )
                })
            })
            .take(4)
            .collect::<Vec<_>>();
        let unresolved_references = inputs
            .reference_graph
            .references()
            .iter()
            .filter(|reference| {
                !matches!(
                    reference.resolution,
                    MultiFilePPtrResolution::Null | MultiFilePPtrResolution::Object { .. }
                )
            })
            .take(4)
            .map(|reference| format!("{reference:?}"))
            .collect::<Vec<_>>();
        return Err(invalid_project(format!(
            "project export requires a complete workspace PPtr scan; incomplete scans: [{}]; unresolved references: [{}]",
            incomplete_scans.join("; "),
            unresolved_references.join("; ")
        )));
    }
    let PreparedSceneCollections {
        processing: scene_processing,
        routed: scene_collections,
        member_keys: scene_member_keys,
        prefabs: prefab_collections,
        prefab_member_keys,
        processing_warnings,
        used_guids,
    } = prepare_scene_collections(
        inputs.filesystem,
        inputs.workspace,
        inputs.objects,
        inputs.reference_graph,
        project_version,
    )?;
    let lightmap_texture_directories = lightmap_texture_scene_directories(
        &scene_processing,
        inputs.objects,
        inputs.reference_graph,
    )?;

    Ok(WorkspaceProjectExportContext {
        inputs,
        editor_version_text,
        project_version,
        has_mono_scripts,
        reference_assembly_maps,
        settings_facts,
        opaque_objects,
        scene_processing,
        scene_collections,
        scene_member_keys,
        prefab_collections,
        prefab_member_keys,
        processing_warnings,
        used_guids,
        lightmap_texture_directories,
    })
}

/// Execute project export with the imported assembly catalog available to script exporters.
#[allow(clippy::too_many_arguments)]
pub fn export_workspace_yaml_project_with_assembly_catalog(
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
    managed_assemblies: Option<&ManagedAssemblyCatalog>,
    limits: WorkspaceProjectExportLimits,
) -> Result<WorkspaceProjectExportReport, Box<dyn Error>> {
    let context = validate_workspace_project_export(WorkspaceProjectExportInputs {
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
        managed_assemblies,
        limits,
    })?;
    let WorkspaceProjectExportContext {
        inputs,
        editor_version_text,
        project_version,
        has_mono_scripts,
        reference_assembly_maps,
        settings_facts,
        opaque_objects,
        scene_processing,
        scene_collections,
        scene_member_keys,
        prefab_collections,
        prefab_member_keys,
        processing_warnings,
        used_guids,
        lightmap_texture_directories,
    } = context;
    let WorkspaceProjectExportInputs {
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
        managed_assemblies,
        limits,
    } = inputs;
    // Pass 1: route every object through the registration stack. No file is planned yet, so a
    // mis-routed or unhandled object aborts with nothing written.
    let routing_context = ProjectRoutingContext {
        workspace,
        objects,
        reference_graph,
        settings,
        project_version,
        editor_version_text,
        settings_facts,
        assembly_manager_is_set,
        managed_assemblies,
        reference_assembly_maps: reference_assembly_maps.as_ref(),
        opaque_objects: &opaque_objects,
        scene_member_keys: &scene_member_keys,
        prefab_member_keys: &prefab_member_keys,
        lightmap_texture_directories: &lightmap_texture_directories,
        existing_guids: &used_guids,
    };
    let RoutedProjectObjects {
        routed,
        mut notices,
        unresolved_script_content,
        fallback_notices,
        opaque_fallback_notices,
        next_collection_index,
        next_guid_sequence,
        used_guids,
    } = route_project_objects(&routing_context)?;
    let saved_assemblies = if assembly_manager_is_set && has_mono_scripts {
        let catalog = managed_assemblies.ok_or_else(|| {
            invalid_project(
                "manager-backed MonoScript project export requires the imported assembly catalog",
            )
        })?;
        plan_saved_assembly_exports(
            catalog,
            reference_assembly_maps.as_ref(),
            editor_version_text,
            settings,
        )?
    } else {
        Vec::new()
    };

    let PreparedGeneratedCollections {
        scenes: scene_collections,
        prefabs: prefab_collections,
        lighting_data_assets,
        next_collection_index,
    } = prepare_generated_collections(GeneratedCollectionPreparationContext {
        workspace,
        objects,
        scene_processing: &scene_processing,
        scenes: scene_collections,
        prefabs: prefab_collections,
        next_collection_index,
        next_guid_sequence,
        used_guids,
    })?;
    // Pass 2: build the project pointer map from exactly the objects that will be written, so a
    // reference to a skipped asset resolves to the baseline missing-reference pointer.
    let ProjectPointerPlan {
        pointer_map,
        collection_count,
    } = build_project_pointer_map(ProjectPointerPlanningContext {
        routed: &routed,
        scenes: &scene_collections,
        prefabs: &prefab_collections,
        lighting_data_assets: &lighting_data_assets,
        next_collection_index,
        limits: limits.pointer_map,
    })?;
    let streamed_resources = WorkspaceStreamedYamlResourceContext {
        filesystem,
        root_catalog,
        provider: resource_provider,
        content_limits: limits.streamed_content,
        staging_limits: StreamedYamlAssetLimits::default(),
    };

    // Pass 3: materialize every routed object into a staged file.
    let mut plan_builder = ProjectExportPlanBuilder::new(collection_count, saved_assemblies.len())?;
    plan_routed_asset_files(
        &RoutedAssetPlanningContext {
            filesystem,
            root_catalog,
            resource_provider,
            workspace,
            objects,
            reference_graph,
            settings,
            routed: &routed,
            saved_assemblies: &saved_assemblies,
            editor_version_text,
            project_version,
            assembly_manager_is_set,
            pointer_map: &pointer_map,
            streamed_resources,
            limits,
        },
        &mut plan_builder,
        &mut notices,
    )?;
    plan_prefab_files(
        &PrefabPlanningContext {
            workspace,
            objects,
            reference_graph,
            pointer_map: &pointer_map,
            prefabs: &prefab_collections,
            scene_processing: &scene_processing,
            editor_version_text,
            project_version,
            limits,
        },
        &mut plan_builder,
    )?;
    plan_scene_files(
        &ScenePlanningContext {
            workspace,
            objects,
            reference_graph,
            pointer_map: &pointer_map,
            scenes: &scene_collections,
            scene_processing: &scene_processing,
            lighting_data_assets: &lighting_data_assets,
            streamed_resources,
            project_version,
            limits,
        },
        &mut plan_builder,
        &mut notices,
    )?;
    plan_lighting_data_files(
        &LightingDataPlanningContext {
            lighting_data_assets: &lighting_data_assets,
            reference_graph,
            pointer_map: &pointer_map,
            routed: &routed,
            limits,
            project_version,
        },
        &mut plan_builder,
    )?;
    let StagedProjectFiles {
        files,
        support_files,
        total_bytes,
        exported_object_count,
    } = plan_builder.finish();
    let manifest = package_manifest_json(project_version);
    let path_id_map = build_path_id_map(workspace, objects)?;
    let path_id_map_bytes = serialize_path_id_map(&path_id_map).into_bytes();
    let plan = ProjectExportPlan {
        project_version_text: editor_version_text.to_owned(),
        project_version_file: project_version_file(editor_version_text, project_version),
        package_manifest: manifest,
        files,
        support_files,
        post_export_files: vec![PlannedProjectPostExportFile {
            relative_path: PATH_ID_MAP_FILE.to_owned(),
            bytes: path_id_map_bytes,
        }],
        skipped: notices,
        fallbacks: fallback_notices,
    };
    let report = execute_project_export(
        output_root,
        delete_existing,
        &plan,
        limits.project_output,
        limits.yaml_write,
    )?;
    Ok(WorkspaceProjectExportReport {
        serialized_file_count: workspace.files().len(),
        exported_object_count,
        asset_file_count: report.asset_file_count,
        support_file_count: report.support_file_count,
        post_export_file_count: report.post_export_file_count,
        meta_file_count: report.meta_file_count,
        total_asset_payload_bytes: total_bytes,
        project_version,
        collection_count,
        skipped_objects: report.skipped,
        unresolved_script_content,
        fallbacks: report.fallbacks,
        opaque_fallbacks: opaque_fallback_notices,
        processing_warnings,
    })
}
