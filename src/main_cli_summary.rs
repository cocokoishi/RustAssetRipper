use super::*;

#[cfg(test)]
pub(super) fn print_workspace_summary_to<W: io::Write>(
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

pub(super) fn print_workspace_summary_with_streamed_resources_to<W: io::Write>(
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

pub(super) fn print_workspace_summary_with_optional_resources_to<W: io::Write>(
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
        load_object_workspace(filesystem, workspace, ObjectWorkspaceLimits::default())?;
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
    for (file_index, file) in workspace.files().iter().enumerate() {
        for (external_index, identifier) in file.metadata.externals.iter().enumerate() {
            let mut detail = format!(
                "External: file {file_index} external {external_index}: {}",
                identifier.path_name_origin
            );
            if let Some([data0, data1, data2, data3]) = identifier.guid {
                let mut guid = String::with_capacity(32);
                for word in [data0, data1, data2, data3] {
                    for shift in 0..8 {
                        let digit = ((word >> (shift * 4)) & 0x0f) as usize;
                        guid.push(b"0123456789abcdef"[digit] as char);
                    }
                }
                detail.push_str(&format!(" guid {guid}"));
            }
            if let Some(file_type) = identifier.file_type {
                detail.push_str(&format!(" type {file_type}"));
            }
            writeln!(output, "{detail}")?;
        }
    }
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
            let mut scene_graph =
                build_workspace_scene_graph(filesystem, file_index, file, &decoded_objects)?;
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
        let mut scene_graph =
            build_workspace_scene_graph(filesystem, file_index, file, &decoded_objects)?;
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
    drop(decoded_objects);

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

pub(super) fn workspace_object_class_id(object: &ObjectInfo) -> Option<i32> {
    object
        .class_id
        .map(i32::from)
        .or_else(|| object.resolved_type_index.map(|_| object.raw_type_id))
}

pub(super) fn relative_container_collection_name<'a>(
    virtual_path: &'a str,
    scope: &str,
) -> Option<&'a str> {
    let relative_path = virtual_path.strip_prefix(scope)?.strip_prefix('/')?;
    (!relative_path.is_empty()).then_some(relative_path)
}

pub(super) fn source_bundle_for_collection(
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

pub(super) fn remaining_budget_u64(
    limit: u64,
    consumed: u64,
    description: &str,
) -> Result<u64, io::Error> {
    limit.checked_sub(consumed).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("aggregate {description} exceeds its configured limit"),
        )
    })
}

pub(super) fn remaining_budget_usize(
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

pub(super) fn merge_mount_report(
    total: &mut BatchMountReport,
    next: &mut BatchMountReport,
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
        .container_trees
        .try_reserve(next.container_trees.len())
        .map_err(|_| invalid_data("expanded container tree metadata allocation failed"))?;
    total
        .failed_entries
        .extend(next.failed_entries.iter().cloned());
    total.container_trees.append(&mut next.container_trees);
    total
        .explicit_root_resource_files
        .extend_from_slice(&next.explicit_root_resource_files);
    Ok(())
}

pub(super) fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

pub(super) fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn invalid_data_owned(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn print_usage() {
    println!(
        "Usage:\n  assetripper-rs inspect <input-directory> [input-directory ...]\n  assetripper-rs inspect-files <input-file> [input-file ...]\n  assetripper-rs export-project [--delete-existing] [--settings <path>] <output-directory> <input-file-or-directory> [input-file-or-directory ...]\n  assetripper-rs export-object-yaml <serialized-file-index> <path-id> <new-output-yaml> <input-file> [input-file ...]\n  assetripper-rs export-file-yaml <serialized-file-index> <new-output-yaml> <input-file> [input-file ...]\n  assetripper-rs export-texture [--settings <path>] <serialized-file-index> <path-id> <new-output-image> <input-file> [input-file ...]\n  assetripper-rs export-mesh <serialized-file-index> <path-id> <new-output-glb> <input-file> [input-file ...]\n  assetripper-rs export-shader [--shader-export-mode <dummy|yaml|decompile>] [--settings <path>] <serialized-file-index> <path-id> <new-output-shader> <input-file> [input-file ...]\n  assetripper-rs export-audio [--settings <path>] <serialized-file-index> <path-id> <new-output-audio> <input-file> [input-file ...]\n  assetripper-rs save-default-settings [--settings <path>]\n  assetripper-rs delete-default-settings\n  assetripper-rs --help\n  assetripper-rs --version\n\n`inspect` mounts directories into isolated VFS roots. `inspect-files` dispatches explicitly selected file paths, registers top-level ResourceFiles as GameBundle root inputs, and reports the parsed SerializedFiles. `export-project` loads mixed file and directory inputs, resolves cross-file references and streamed resources, and writes processed Scene collections as `.unity` YAML with `DefaultImporter` `.meta`, Material `.mat` YAML with cross-file Shader pointer mapping; Shader collections write `.shader` plus `ShaderImporter` in Dummy/Decompile modes (raw source text wins when present) or `.asset` plus `NativeFormatImporter` and version-gated patches in YAML mode; TextAssets write raw `m_Script` bytes with `TextScriptImporter` metadata and a `TextExportMode`-selected extension; other routed objects use generic YAML `.asset` files, `ProjectSettings/ProjectVersion.txt`, the default package manifest, and `AuxiliaryFiles/path_id_map.json`. It stages all project outputs before writing and refuses undecoded objects, unresolved pointers, or objects omitted by YAML conversion. The generated Scene hierarchy's effective `AssetBundleName` remains null because its `GeneratedObjectBase` setter is a no-op; its `.meta` bundle-name field is blank. Duplicate BuildSettings paths keep the first Scene output. The output directory must not be empty or named `Desktop`, `Documents`, or `Downloads`; an existing non-empty directory aborts the export unless `--delete-existing` is given, which deletes and recreates it exactly like the reference exporter does after deletion consent. Processed Scene and modern/legacy Prefab collections are supported on the routed workspace path; remaining class-specific exporter/importer families (including complete Material/Shader field coverage), settings application, conditional StreamingAssets and GameAssemblies post-exporters, and full project-output parity remain open. `export-object-yaml` writes one decoded object as YAML. `export-file-yaml` writes YAML-exportable objects from one SerializedFile in object-table order, resolves streamed Mesh and image-texture bytes, preserves intra-file references through source PathIDs, and refuses unresolved or cross-file references. It reports ImageTexture objects omitted by the baseline when they have no usable image data. `export-texture` decodes one class-28 Texture2D, resolves its inline or streamed image data, and writes a PNG/BMP/TGA image; the output extension selects the format (otherwise the configured settings format, default PNG). `export-mesh` decodes one class-43 Mesh, resolves its vertex data and compressed mesh, and writes a glTF 2.0 binary file with one node per sub-mesh, converting Unity's left-handed coordinates to glTF's right-handed space and rewriting the sub-mesh topology. `export-shader` decodes one class-48 Shader and writes `.shader` text: decompiled script text is written verbatim under every mode, otherwise `dummy` (the default) and `decompile` render the fallback dummy body and `yaml` reports that YAML mode writes a YAML asset instead. `export-audio` decodes one class-83 AudioClip, resolves its inline or streamed audio data, and writes the decoded bytes; tracker audio passes through, `supported single-stream FSB5 Vorbis is reconstructed as Ogg, and unknown bytes fail with the reference diagnostic. None of the commands replaces an existing output file."
    );
}
