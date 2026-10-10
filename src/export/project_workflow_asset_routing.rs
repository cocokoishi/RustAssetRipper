/// Execute a workspace-backed Unity YAML project export with the built-in export settings.
///
/// The output root is validated and prepared exactly like the reference GUI export guard: the
/// path must not be empty or a system directory, and an existing non-empty directory is only
/// deleted when `delete_existing` selects the reference's consented arm. Because the root is
/// guaranteed empty afterwards, no project file can be written over pre-existing content.
#[allow(clippy::too_many_arguments)]
pub fn export_workspace_yaml_project(
    output_root: &Path,
    delete_existing: bool,
    filesystem: &VirtualFileSystem,
    root_catalog: &RootResourceFileCatalog,
    resource_provider: &PlatformResourceProvider<'_>,
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    limits: WorkspaceProjectExportLimits,
) -> Result<WorkspaceProjectExportReport, Box<dyn Error>> {
    let settings = ExportSettingsFile::from(super::settings::ExportDefaults::default());
    export_workspace_yaml_project_with_settings(
        output_root,
        delete_existing,
        filesystem,
        root_catalog,
        resource_provider,
        workspace,
        objects,
        reference_graph,
        &settings,
        limits,
    )
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

fn plan_script_export_details(
    object: &WorkspaceObject,
    settings: &ExportSettingsFile,
    assembly_manager_is_set: bool,
    managed_assemblies: Option<&ManagedAssemblyCatalog>,
    unity_version_text: &str,
    reference_assembly_maps: Option<&ReferenceAssemblyMaps>,
) -> Result<Option<ScriptExportDetails>, Box<dyn Error>> {
    let metadata = mono_script_export_identity(object).map_err(|error| {
        invalid_project(format!(
            "file {} object {} PathID {} MonoScript export fields are invalid: {error}",
            object.file_index, object.object_index, object.path_id
        ))
    })?;
    let identity = MonoScriptIdentity {
        class: metadata.class,
        namespace: metadata.namespace,
        assembly: metadata.assembly,
    };
    if identity.is_injected() {
        return Ok(None);
    }
    let reference_guid = match reference_assembly_maps {
        Some(maps) => {
            let version = FactoryUnityVersion::parse(unity_version_text).ok_or_else(|| {
                invalid_project(format!(
                    "file {} object {} PathID {} MonoScript reference lookup cannot parse Unity version {unity_version_text:?}",
                    object.file_index, object.object_index, object.path_id
                ))
            })?;
            maps.guid_for(&version, &identity.assembly)
                .map_err(|error| {
                    invalid_project(format!(
                        "file {} object {} PathID {} MonoScript reference lookup failed: {error}",
                        object.file_index, object.object_index, object.path_id
                    ))
                })?
        }
        None if assembly_manager_is_set => {
            return Err(invalid_project(format!(
                "file {} object {} PathID {} MonoScript reference classification requires the manager-backed reference map, which is unavailable for this mscorlib configuration",
                object.file_index, object.object_index, object.path_id
            )))
        }
        None => None,
    };
    let export_type = assembly_export_type(
        reference_guid.is_some(),
        assembly_manager_is_set,
        settings.script_export_mode,
        &identity.assembly,
    );
    let mut missing_assembly = false;
    let mut source_script_present = None;
    if export_type == AssemblyExportType::Save {
        let catalog = managed_assemblies.ok_or_else(|| {
            invalid_project(format!(
                "file {} object {} PathID {} MonoScript selects Save for ScriptExportMode::{:?}, but the imported assembly catalog was not supplied to project export",
                object.file_index,
                object.object_index,
                object.path_id,
                settings.script_export_mode
            ))
        })?;
        match catalog.assembly(&identity.assembly) {
            Some(assembly) if assembly.source_virtual_path.is_none() => {
                return Err(invalid_project(format!(
                    "file {} object {} PathID {} MonoScript assembly {:?} has no source VFS stream to save",
                    object.file_index, object.object_index, object.path_id, identity.assembly
                )))
            }
            Some(_) => {}
            None => missing_assembly = true,
        }
    } else if export_type == AssemblyExportType::Decompile && assembly_manager_is_set {
        let catalog = managed_assemblies.ok_or_else(|| {
            invalid_project(format!(
                "file {} object {} PathID {} MonoScript type presence requires the imported assembly catalog",
                object.file_index, object.object_index, object.path_id
            ))
        })?;
        missing_assembly = !catalog.contains_assembly(&identity.assembly);
        source_script_present = Some(catalog.is_present(&ScriptIdentifier {
            assembly: identity.assembly.clone(),
            namespace: identity.namespace.clone(),
            name: Some(identity.class.clone()),
        }));
    } else if export_type != AssemblyExportType::Decompile
        && !(export_type == AssemblyExportType::Skip && reference_guid.is_some())
    {
        return Err(invalid_project(format!(
            "file {} object {} PathID {} MonoScript selects {export_type:?} for ScriptExportMode::{:?}; that script export route is not wired into the project path",
            object.file_index,
            object.object_index,
            object.path_id,
            settings.script_export_mode
        )));
    }
    let script_guid = calculate_script_guid_str(
        &identity.assembly,
        &identity.namespace,
        &identity.class,
        ScriptHashLimits::default(),
    )
    .map_err(|error| {
        invalid_project(format!(
            "file {} object {} PathID {} MonoScript GUID could not be calculated: {error}",
            object.file_index, object.object_index, object.path_id
        ))
    })?;
    let (pointer_guid, pointer_file_id) = match export_type {
        AssemblyExportType::Decompile => (
            script_guid.to_byte_array(),
            super::script_export_flow::decompiled_file_id().map_err(|error| {
                invalid_project(format!(
                    "MonoScript export ID could not be planned: {error}"
                ))
            })?,
        ),
        AssemblyExportType::Skip => {
            let file_id = calculate_script_file_id_str(
                &identity.namespace,
                &identity.class,
                ScriptHashLimits::default(),
            )
            .map_err(|error| {
                invalid_project(format!(
                    "file {} object {} PathID {} MonoScript file ID could not be calculated: {error}",
                    object.file_index, object.object_index, object.path_id
                ))
            })?;
            (
                reference_guid
                    .ok_or_else(|| invalid_project("skipped MonoScript has no reference GUID"))?
                    .to_byte_array(),
                i64::from(file_id),
            )
        }
        AssemblyExportType::Save => {
            let file_id = calculate_script_file_id_str(
                &identity.namespace,
                &identity.class,
                ScriptHashLimits::default(),
            )
            .map_err(|error| {
                invalid_project(format!(
                    "file {} object {} PathID {} MonoScript file ID could not be calculated: {error}",
                    object.file_index, object.object_index, object.path_id
                ))
            })?;
            (
                calculate_assembly_guid(&identity.assembly).to_byte_array(),
                i64::from(file_id),
            )
        }
    };
    Ok(Some(ScriptExportDetails {
        identity,
        execution_order: metadata.execution_order,
        guid: script_guid.to_byte_array(),
        pointer_guid,
        pointer_file_id,
        export_type,
        missing_assembly,
        source_script_present,
    }))
}

fn source_reference_assembly_maps(
    has_mono_scripts: bool,
    managed_assemblies: Option<&ManagedAssemblyCatalog>,
) -> Result<Option<ReferenceAssemblyMaps>, Box<dyn Error>> {
    if !has_mono_scripts {
        return Ok(None);
    }

    let has_mscorlib2 = match managed_assemblies.and_then(|catalog| catalog.assembly("mscorlib")) {
        Some(assembly) => match assembly.version_major {
            Some(major) => major == 2,
            None => {
                return Err(invalid_project(
                    "Mono reference lookup found mscorlib without a manifest version",
                ))
            }
        },
        // MonoManager falls back to the .NET runtime's version 4 mscorlib when the
        // imported platform does not contain one. An absent manager also selects false.
        None => false,
    };

    ReferenceAssemblyMaps::from_source_fixture_with_mscorlib2(has_mscorlib2)
        .map(Some)
        .map_err(|error| {
            invalid_project(format!(
                "source reference assembly map could not be loaded: {error}"
            ))
        })
}

fn plan_saved_assembly_exports(
    catalog: &ManagedAssemblyCatalog,
    reference_assembly_maps: Option<&ReferenceAssemblyMaps>,
    unity_version_text: &str,
    settings: &ExportSettingsFile,
) -> Result<Vec<SavedAssemblyExport>, Box<dyn Error>> {
    let version = FactoryUnityVersion::parse(unity_version_text).ok_or_else(|| {
        invalid_project(format!(
            "saved assembly export cannot parse Unity version {unity_version_text:?}"
        ))
    })?;
    let reference_assembly_maps = reference_assembly_maps.ok_or_else(|| {
        invalid_project(
            "saved assembly export requires a source reference map for this mscorlib configuration",
        )
    })?;
    let mut exports = Vec::new();
    exports
        .try_reserve_exact(catalog.assemblies().len())
        .map_err(|_| invalid_project("could not allocate saved assembly export plans"))?;
    for assembly in catalog.assemblies() {
        let is_reference = reference_assembly_maps
            .guid_for(&version, &assembly.assembly_name)
            .map_err(|error| {
                invalid_project(format!(
                    "assembly {:?} reference lookup failed: {error}",
                    assembly.assembly_name
                ))
            })?
            .is_some();
        if assembly_export_type(
            is_reference,
            true,
            settings.script_export_mode,
            &assembly.assembly_name,
        ) != AssemblyExportType::Save
        {
            continue;
        }
        let source_virtual_path = assembly.source_virtual_path.clone().ok_or_else(|| {
            invalid_project(format!(
                "saved assembly {:?} has no source VFS stream",
                assembly.assembly_name
            ))
        })?;
        let file_name = add_assembly_file_extension(&assembly.assembly_name);
        exports.push(SavedAssemblyExport {
            assembly_name: assembly.assembly_name.clone(),
            source_virtual_path,
            relative_path: format!("Assets/Plugins/{file_name}"),
            guid: calculate_assembly_guid(&file_name).to_byte_array(),
            generated_forwarder: None,
        });
    }

    if let Some(mscorlib) = catalog.assembly("mscorlib") {
        let mscorlib_path = mscorlib.source_virtual_path.clone().ok_or_else(|| {
            invalid_project("loaded mscorlib has no source VFS stream for forwarding modules")
        })?;
        for helper_name in [
            "System.Runtime.CompilerServices.Unsafe",
            "System.Runtime.InteropServices",
        ] {
            if catalog.assembly(helper_name).is_some() {
                continue;
            }
            let is_reference = reference_assembly_maps
                .guid_for(&version, helper_name)
                .map_err(|error| {
                    invalid_project(format!(
                        "forwarding assembly {helper_name:?} reference lookup failed: {error}"
                    ))
                })?
                .is_some();
            if assembly_export_type(is_reference, true, settings.script_export_mode, helper_name)
                != AssemblyExportType::Save
            {
                continue;
            }
            let file_name = add_assembly_file_extension(helper_name);
            exports.push(SavedAssemblyExport {
                assembly_name: helper_name.to_owned(),
                source_virtual_path: mscorlib_path.clone(),
                relative_path: format!("Assets/Plugins/{file_name}"),
                guid: calculate_assembly_guid(&file_name).to_byte_array(),
                generated_forwarder: Some(helper_name.to_owned()),
            });
        }
    }
    Ok(exports)
}

fn lightmap_texture_scene_directories(
    processing: &WorkspaceSceneProcessing,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<HashMap<(usize, usize), String>, Box<dyn Error>> {
    let mut directories = HashMap::new();
    for scene in processing.hierarchies.scenes() {
        let Some(owner) = scene.lighting_data_dependency_owner() else {
            continue;
        };
        for reference in reference_graph.references().iter().filter(|reference| {
            reference.source_file_index == owner.file_index
                && reference.source_object_index == owner.object_index
                && is_lightmap_texture_pointer_field(reference.field_path)
        }) {
            let (target_file_index, target_object_index) = match reference.resolution {
                MultiFilePPtrResolution::Null => continue,
                MultiFilePPtrResolution::Object {
                    target_file_index,
                    target_object_index,
                } => (target_file_index, target_object_index),
                _ => {
                    return Err(invalid_project(format!(
                        "LightmapSettings {}:{} has unresolved lightmap texture pointer at {}",
                        owner.file_index, owner.object_index, reference.field_path
                    )))
                }
            };
            let target = objects
                .get(target_file_index, target_object_index)
                .ok_or_else(|| {
                    invalid_project(format!(
                        "LightmapSettings {}:{} points to missing workspace object {}:{} at {}",
                        owner.file_index,
                        owner.object_index,
                        target_file_index,
                        target_object_index,
                        reference.field_path
                    ))
                })?;
            if target.factory.class_id != 28 {
                continue;
            }
            let key = (target_file_index, target_object_index);
            if !directories.contains_key(&key) {
                directories.try_reserve(1).map_err(|_| {
                    invalid_project("could not allocate lightmap texture ownership index")
                })?;
                directories.insert(key, scene.plan.path.clone());
            }
        }
    }
    Ok(directories)
}

fn is_lightmap_texture_pointer_field(field_path: &str) -> bool {
    let Some(lightmap_data_path) = field_path.strip_prefix("m_Lightmaps[") else {
        return false;
    };
    let Some((_, field_name)) = lightmap_data_path.split_once("].") else {
        return false;
    };
    matches!(
        field_name,
        "m_DirLightmap" | "m_IndirectLightmap" | "m_Lightmap" | "m_ShadowMask"
    )
}

fn prepare_scene_collections(
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    project_version: SceneUnityVersion,
) -> Result<PreparedSceneCollections, Box<dyn Error>> {
    let processing = process_workspace_scenes_and_prefabs(
        filesystem,
        workspace,
        objects,
        reference_graph,
        WorkspaceSceneProcessingLimits::default(),
    )
    .map_err(|error| invalid_project(format!("Scene/Prefab processing failed: {error}")))?;
    let scenes = processing.hierarchies.scenes();
    let mut scene_collections = Vec::new();
    scene_collections
        .try_reserve_exact(scenes.len())
        .map_err(|_| invalid_project("could not allocate processed Scene collections"))?;
    let mut scene_member_keys = HashSet::new();
    let mut used_guids = HashSet::new();
    if !scenes.is_empty() && !processing.processed_assets.is_complete() {
        return Err(invalid_project(format!(
            "Scene export requires a complete PrefabProcessor asset plan: {}",
            processing
                .processed_assets
                .incomplete_reason()
                .unwrap_or("no reason was recorded")
        )));
    }
    if processing.scene_hierarchies.len() != scenes.len() {
        return Err(invalid_project(format!(
            "Scene processor produced {} hierarchy assets for {} scene definitions",
            processing.scene_hierarchies.len(),
            scenes.len()
        )));
    }

    for (scene_index, scene) in scenes.iter().enumerate() {
        let mut hierarchy_matches = processing
            .scene_hierarchies
            .iter()
            .filter(|hierarchy| hierarchy.scene_index == scene_index);
        let hierarchy = hierarchy_matches.next().ok_or_else(|| {
            invalid_project(format!(
                "SceneDefinition {scene_index} has no materialized SceneHierarchyObject"
            ))
        })?;
        if hierarchy_matches.next().is_some() {
            return Err(invalid_project(format!(
                "SceneDefinition {scene_index} has multiple materialized SceneHierarchyObjects"
            )));
        }
        if hierarchy.incomplete {
            return Err(invalid_project(format!(
                "SceneDefinition {:?} has incomplete hierarchy membership or roots",
                scene.plan.name
            )));
        }

        let assignments = plan_workspace_scene_export_ids_with_processed_assets(
            scene,
            scene_index,
            &processing.processed_assets,
            project_version,
            Default::default(),
            SceneHierarchyAssetOrderLimits::default(),
            SceneExportIdPlanLimits::default(),
        )
        .map_err(|error| {
            invalid_project(format!(
                "SceneDefinition {:?} export-ID planning failed: {error}",
                scene.plan.name
            ))
        })?;

        let mut non_exportable_scene_assets = HashSet::new();
        non_exportable_scene_assets
            .try_reserve(usize::from(hierarchy.scene_roots.is_some()))
            .map_err(|_| invalid_project("could not allocate Scene manager identity index"))?;
        if let Some(scene_roots) = hierarchy.scene_roots {
            non_exportable_scene_assets.insert(source_object_key(
                scene_roots.file_index,
                scene_roots.object_index,
            ));
        }

        let mut exportable_assignments = Vec::new();
        exportable_assignments
            .try_reserve_exact(assignments.len())
            .map_err(|_| invalid_project("could not allocate Scene YAML assignments"))?;
        for assignment in &assignments {
            if let ExportAssetCollectionIdentity::SerializedFile { file_index } =
                assignment.asset.collection
            {
                let key = source_object_key(file_index, assignment.asset.object_index);
                if !scene_member_keys.insert(key) {
                    return Err(invalid_project(format!(
                        "workspace object {file_index}:{} belongs to multiple exported Scene collections",
                        assignment.asset.object_index
                    )));
                }
                if !non_exportable_scene_assets.contains(&key) {
                    exportable_assignments.push(*assignment);
                }
            } else {
                exportable_assignments.push(*assignment);
            }
        }

        if let SceneDefinitionGuidPlan::Preserve(words) = scene.plan.guid {
            if !words.is_zero() {
                used_guids.insert(scene_guid_words_to_bytes(words));
            }
        }
        scene_collections.push(RoutedSceneCollection {
            scene_index,
            name: scene.plan.name.clone(),
            path: scene.plan.path.clone(),
            path_source: scene.plan.path_source,
            guid_plan: scene.plan.guid,
            asset_bundle_name: hierarchy.asset_bundle_name.clone(),
            assignments,
            exportable_assignments,
            collection_index: usize::MAX,
            guid: [0; 16],
        });
    }

    let (prefabs, prefab_member_keys) = prepare_prefab_collections(
        workspace,
        objects,
        &processing,
        project_version,
        &scene_member_keys,
    )?;

    let mut processing_warnings = Vec::new();
    if scenes.is_empty() && !processing.prefab_processing.is_complete() {
        let report = processing.prefab_processing.report();
        processing_warnings.push(format!(
            "PrefabProcessor hierarchy planning is incomplete ({} root-resolution issues, {} incomplete hierarchy walks); no processed Prefab collection was emitted, and source objects used their regular export collections",
            report.root_resolution_issues,
            report.existing_hierarchy_failures + report.generated_hierarchy_failures,
        ));
    }

    Ok(PreparedSceneCollections {
        processing,
        routed: scene_collections,
        member_keys: scene_member_keys,
        prefabs,
        prefab_member_keys,
        processing_warnings,
        used_guids,
    })
}

fn prepare_prefab_collections(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    processing: &WorkspaceSceneProcessing,
    project_version: SceneUnityVersion,
    scene_member_keys: &HashSet<ExportAssetObjectKey>,
) -> Result<(Vec<RoutedPrefabCollection>, HashSet<ExportAssetObjectKey>), Box<dyn Error>> {
    let has_prefab_hierarchy_assets =
        processing
            .processed_assets
            .collections()
            .iter()
            .any(|collection| {
                collection.kind
                == crate::processing::prefab_processing::WorkspaceProcessedCollectionKind::
                    PrefabHierarchies
                && !collection.assets.is_empty()
            });
    if !processing.prefab_processing.is_complete()
        && !has_prefab_hierarchy_assets
        && !has_unrouteable_prefab_candidate(workspace, processing)
    {
        return Ok((Vec::new(), HashSet::new()));
    }
    if !processing.prefab_processing.is_complete() {
        return Err(invalid_project(
            "Prefab export requires a complete PrefabProcessor hierarchy pass",
        ));
    }
    if !processing.processed_assets.is_complete() {
        return Err(invalid_project(format!(
            "Prefab export requires a complete processed-asset plan: {}",
            processing
                .processed_assets
                .incomplete_reason()
                .unwrap_or("no reason was recorded")
        )));
    }
    let Some(hierarchies) = processing
        .processed_assets
        .collections()
        .iter()
        .find(|collection| {
            collection.kind
            == crate::processing::prefab_processing::WorkspaceProcessedCollectionKind::
                PrefabHierarchies
        })
    else {
        return Ok((Vec::new(), HashSet::new()));
    };
    let mut prefabs = Vec::new();
    prefabs
        .try_reserve_exact(hierarchies.assets.len())
        .map_err(|_| invalid_project("could not allocate processed Prefab collections"))?;
    let mut member_keys = HashSet::new();
    member_keys
        .try_reserve(hierarchies.assets.len())
        .map_err(|_| invalid_project("could not allocate Prefab membership index"))?;

    for (hierarchy_object_index, hierarchy_asset) in hierarchies.assets.iter().enumerate() {
        let crate::processing::prefab_processing::WorkspaceProcessedPrefabAssetKind::
            PrefabHierarchy {
                root_game_object,
                prefab: prefab_reference,
            } = hierarchy_asset.kind
        else {
            return Err(invalid_project(format!(
                "Prefab Hierarchies collection contains unexpected asset kind {:?}",
                hierarchy_asset.kind
            )));
        };
        let hierarchy = find_prefab_hierarchy_plan(
            &processing.prefab_processing,
            root_game_object,
            prefab_reference,
        )?;
        if !hierarchy.is_complete() {
            return Err(invalid_project(format!(
                "Prefab root {}:{} has incomplete hierarchy membership",
                root_game_object.file_index, root_game_object.path_id
            )));
        }

        let root_identity = source_asset_identity(root_game_object)?;
        if root_identity.class_id != 1 {
            return Err(invalid_project(format!(
                "Prefab hierarchy root {}:{} has class ID {}, expected GameObject class ID 1",
                root_game_object.file_index, root_game_object.path_id, root_identity.class_id
            )));
        }
        let root_object = objects
            .get(root_game_object.file_index, root_game_object.object_index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "Prefab root object {}:{} is absent from the workspace",
                    root_game_object.file_index, root_game_object.object_index
                ))
            })?;
        let root_payload = root_object.payload.as_ref().map_err(|error| {
            invalid_project(format!(
                "Prefab root {}:{} is undecodable: {error}",
                root_game_object.file_index, root_game_object.path_id
            ))
        })?;
        let root_name = object_asset_name(root_payload);

        let hierarchy_identity = ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: hierarchies.collection_index,
            },
            object_index: hierarchy_object_index,
            path_id: hierarchy_asset.identity.path_id,
            class_id: hierarchy_asset.identity.class_id,
        };
        let hierarchy_versioned = VersionedPrefabAsset {
            identity: hierarchy_identity,
            unity_version: project_version,
        };

        let (prefab_identity, representation, class_name) = match prefab_reference {
            crate::processing::prefab_processing::WorkspaceProcessedPrefabReference::Existing(
                source_prefab,
            ) => {
                let source_identity = source_asset_identity(source_prefab)?;
                if source_identity.class_id != 1001 {
                    return Err(invalid_project(format!(
                        "Prefab main object {}:{} has class ID {}, expected 1001",
                        source_prefab.file_index, source_prefab.path_id, source_identity.class_id
                    )));
                }
                let version = source_unity_version(workspace, source_prefab.file_index)?;
                let object = objects
                    .get(source_prefab.file_index, source_prefab.object_index)
                    .ok_or_else(|| {
                        invalid_project(format!(
                            "Prefab main object {}:{} is absent from the workspace",
                            source_prefab.file_index, source_prefab.object_index
                        ))
                    })?;
                let payload = object.payload.as_ref().map_err(|error| {
                    invalid_project(format!(
                        "Prefab main object {}:{} is undecodable: {error}",
                        source_prefab.file_index, source_prefab.path_id
                    ))
                })?;
                let representation = match payload.type_name.as_str() {
                    "Prefab" => PrefabAssetRepresentation::LegacyPrefabMarker,
                    "PrefabInstance" => PrefabAssetRepresentation::PrefabInstanceMarker,
                    _ => PrefabAssetRepresentation::Other,
                };
                (
                    VersionedPrefabAsset {
                        identity: source_identity,
                        unity_version: version,
                    },
                    representation,
                    payload.type_name.clone(),
                )
            }
            crate::processing::prefab_processing::WorkspaceProcessedPrefabReference::Generated(
                generated_prefab,
            ) => {
                if generated_prefab.class_id != 1001 {
                    return Err(invalid_project(format!(
                        "generated Prefab main asset has class ID {}, expected 1001",
                        generated_prefab.class_id
                    )));
                }
                let generated_collection = processing
                    .processed_assets
                    .collections()
                    .get(generated_prefab.collection_index)
                    .ok_or_else(|| {
                        invalid_project(format!(
                            "generated Prefab collection {} is absent from the plan",
                            generated_prefab.collection_index
                        ))
                    })?;
                let generated_asset = generated_collection
                    .assets
                    .iter()
                    .find(|asset| asset.identity == generated_prefab)
                    .ok_or_else(|| {
                        invalid_project(format!(
                            "generated Prefab asset {:?} is absent from its collection",
                            generated_prefab
                        ))
                    })?;
                if !matches!(
                    generated_asset.kind,
                    crate::processing::prefab_processing::
                        WorkspaceProcessedPrefabAssetKind::GeneratedPrefabInstance {
                            root_game_object: generated_root
                        } if generated_root == root_game_object
                ) {
                    return Err(invalid_project(
                        "generated Prefab main asset disagrees with its hierarchy root",
                    ));
                }
                (
                    VersionedPrefabAsset {
                        identity: ExportAssetIdentity {
                            collection: ExportAssetCollectionIdentity::ProcessedCollection {
                                collection_index: generated_prefab.collection_index,
                            },
                            object_index: generated_collection
                                .assets
                                .iter()
                                .position(|asset| asset.identity == generated_prefab)
                                .ok_or_else(|| {
                                    invalid_project(
                                        "generated Prefab main asset index is unavailable",
                                    )
                                })?,
                            path_id: generated_prefab.path_id,
                            class_id: generated_prefab.class_id,
                        },
                        unity_version: project_version,
                    },
                    if unity_version_has_prefab_importer(project_version) {
                        PrefabAssetRepresentation::PrefabInstanceMarker
                    } else {
                        PrefabAssetRepresentation::LegacyPrefabMarker
                    },
                    // The generated class-1001 marker is hidden implementation state. The
                    // baseline names a Prefab collection created from a loose GameObject after
                    // its root GameObject, even when the collection uses PrefabImporter.
                    "GameObject".to_owned(),
                )
            }
        };

        if let crate::processing::prefab_processing::WorkspaceProcessedPrefabReference::Generated(
            generated_prefab,
        ) = prefab_reference
        {
            let found = processing
                .processed_assets
                .collections()
                .iter()
                .flat_map(|collection| collection.assets.iter())
                .any(|asset| {
                    asset.identity == generated_prefab
                        && matches!(
                            asset.kind,
                            crate::processing::prefab_processing::
                                WorkspaceProcessedPrefabAssetKind::GeneratedPrefabInstance {
                                    root_game_object: generated_root
                                } if generated_root == root_game_object
                        )
                });
            if !found {
                return Err(invalid_project(
                    "Prefab hierarchy references an unregistered generated Prefab asset",
                ));
            }
        }

        let mut game_objects = Vec::new();
        let mut components = Vec::new();
        game_objects
            .try_reserve_exact(hierarchy.entries().len())
            .map_err(|_| invalid_project("could not allocate Prefab GameObject members"))?;
        components
            .try_reserve_exact(hierarchy.entries().len())
            .map_err(|_| invalid_project("could not allocate Prefab component members"))?;
        for entry in hierarchy.entries() {
            match entry {
                crate::processing::prefab_processing::WorkspacePrefabHierarchyPlanEntry::Source(
                    entry,
                ) => {
                    let member_identity = source_asset_identity(entry.identity)?;
                    let version = source_unity_version(workspace, entry.identity.file_index)?;
                    let member = VersionedPrefabAsset {
                        identity: member_identity,
                        unity_version: version,
                    };
                    match entry.kind {
                        crate::processing::scene_hierarchy::
                            WorkspaceGameObjectHierarchyEntryKind::GameObject => {
                            game_objects.push(member);
                        }
                        crate::processing::scene_hierarchy::
                            WorkspaceGameObjectHierarchyEntryKind::Component => {
                            components.push(member);
                        }
                    }
                }
                crate::processing::prefab_processing::WorkspacePrefabHierarchyPlanEntry::
                    GeneratedTransform { game_object, .. } => {
                    let transform = find_generated_transform(
                        &processing.processed_assets,
                        *game_object,
                    )?;
                    components.push(VersionedPrefabAsset {
                        identity: transform,
                        unity_version: project_version,
                    });
                }
            }
        }
        let hidden_assets = if representation == PrefabAssetRepresentation::PrefabInstanceMarker {
            vec![prefab_identity.identity.object_key()]
        } else {
            Vec::new()
        };
        let export_ids = plan_prefab_export_ids(
            PrefabHierarchyAssetGroups {
                game_objects: &game_objects,
                components: &components,
                prefab_instances: &[],
                prefab_asset: prefab_identity,
                hierarchy_asset: hierarchy_versioned,
                representation,
            },
            &hidden_assets,
            PrefabExportPlanLimits::default(),
        )
        .map_err(|error| {
            invalid_project(format!(
                "Prefab root {}:{} export-ID planning failed: {error}",
                root_game_object.file_index, root_game_object.path_id
            ))
        })?;

        for assignment in &export_ids.registered_assets {
            let ExportAssetCollectionIdentity::SerializedFile { file_index } =
                assignment.asset.collection
            else {
                continue;
            };
            let key = source_object_key(file_index, assignment.asset.object_index);
            if scene_member_keys.contains(&key) || !member_keys.insert(key) {
                return Err(invalid_project(format!(
                    "workspace object {file_index}:{} belongs to more than one Scene or Prefab export collection",
                    assignment.asset.object_index
                )));
            }
        }
        prefabs.push(RoutedPrefabCollection {
            name: root_name
                .clone()
                .unwrap_or_else(|| "PrefabInstance".to_owned()),
            class_name,
            named_name: root_name,
            asset_bundle_name: object_asset_bundle_name(root_payload),
            representation,
            export_ids,
            collection_index: usize::MAX,
            guid: [0; 16],
        });
    }
    Ok((prefabs, member_keys))
}

fn has_unrouteable_prefab_candidate(
    workspace: &SerializedFileWorkspace,
    processing: &WorkspaceSceneProcessing,
) -> bool {
    if workspace.files().iter().any(|file| {
        file.metadata.objects.iter().any(|object| {
            crate::processing::asset_factory::resolve_read_class_id(object.raw_type_id) == 1001
        })
    }) {
        return true;
    }
    processing.prefab_processing.events().iter().any(|event| {
        matches!(
            event,
            crate::processing::prefab_processing::WorkspacePrefabProcessingEvent::
                ExistingPrefabHierarchyCreated { .. }
                | crate::processing::prefab_processing::WorkspacePrefabProcessingEvent::
                    ExistingPrefabHierarchyIncomplete { .. }
                | crate::processing::prefab_processing::WorkspacePrefabProcessingEvent::
                    GeneratedPrefabHierarchyCreated { .. }
                | crate::processing::prefab_processing::WorkspacePrefabProcessingEvent::
                    GeneratedPrefabHierarchyIncomplete { .. }
        )
    })
}

fn find_prefab_hierarchy_plan(
    processing: &crate::processing::prefab_processing::WorkspacePrefabProcessingPlan,
    root: crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity,
    prefab: crate::processing::prefab_processing::WorkspaceProcessedPrefabReference,
) -> Result<&crate::processing::prefab_processing::WorkspacePrefabHierarchyPlan, Box<dyn Error>> {
    use crate::processing::prefab_processing::{
        WorkspacePrefabProcessingEvent, WorkspaceProcessedPrefabReference,
    };
    let mut matching = processing
        .events()
        .iter()
        .filter_map(|event| match (prefab, event) {
            (
                WorkspaceProcessedPrefabReference::Existing(expected),
                WorkspacePrefabProcessingEvent::ExistingPrefabHierarchyCreated {
                    prefab_instance,
                    root_game_object,
                    hierarchy,
                    ..
                },
            ) if expected == *prefab_instance && root == *root_game_object => Some(hierarchy),
            (
                WorkspaceProcessedPrefabReference::Generated(_),
                WorkspacePrefabProcessingEvent::GeneratedPrefabHierarchyCreated {
                    root_game_object,
                    hierarchy,
                    ..
                },
            ) if root == *root_game_object => Some(hierarchy),
            _ => None,
        });
    let hierarchy = matching.next().ok_or_else(|| {
        invalid_project(format!(
            "Prefab hierarchy for root {}:{} has no matching PrefabProcessor event",
            root.file_index, root.path_id
        ))
    })?;
    if matching.next().is_some() {
        return Err(invalid_project(format!(
            "Prefab hierarchy for root {}:{} has multiple matching PrefabProcessor events",
            root.file_index, root.path_id
        )));
    }
    Ok(hierarchy)
}

fn find_generated_transform(
    processed_assets: &crate::processing::prefab_processing::WorkspaceProcessedPrefabAssetPlan,
    game_object: crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity,
) -> Result<ExportAssetIdentity, Box<dyn Error>> {
    use crate::processing::prefab_processing::{
        WorkspaceProcessedCollectionKind, WorkspaceProcessedPrefabAssetKind,
    };
    let mut matches = processed_assets
        .collections()
        .iter()
        .filter(|collection| {
            collection.kind == WorkspaceProcessedCollectionKind::MissingPrefabTransforms
        })
        .flat_map(|collection| {
            collection
                .assets
                .iter()
                .enumerate()
                .filter_map(move |(object_index, asset)| {
                    matches!(
                        asset.kind,
                        WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
                            game_object: target,
                            ..
                        } if target == game_object
                    )
                    .then_some(ExportAssetIdentity {
                        collection: ExportAssetCollectionIdentity::ProcessedCollection {
                            collection_index: collection.collection_index,
                        },
                        object_index,
                        path_id: asset.identity.path_id,
                        class_id: asset.identity.class_id,
                    })
                })
        });
    let identity = matches.next().ok_or_else(|| {
        invalid_project(format!(
            "Prefab GameObject {}:{} has no planned generated Transform asset",
            game_object.file_index, game_object.path_id
        ))
    })?;
    if matches.next().is_some() {
        return Err(invalid_project(format!(
            "Prefab GameObject {}:{} has multiple generated Transform assets",
            game_object.file_index, game_object.path_id
        )));
    }
    Ok(identity)
}

fn source_asset_identity(
    identity: crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity,
) -> Result<ExportAssetIdentity, Box<dyn Error>> {
    let class_id = identity.class_id.ok_or_else(|| {
        invalid_project(format!(
            "workspace object {}:{} has no class ID",
            identity.file_index, identity.path_id
        ))
    })?;
    if identity.path_id == 0 {
        return Err(invalid_project(format!(
            "workspace object {}:{} has PathID 0",
            identity.file_index, identity.path_id
        )));
    }
    Ok(ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: identity.file_index,
        },
        object_index: identity.object_index,
        path_id: identity.path_id,
        class_id,
    })
}

fn source_unity_version(
    workspace: &SerializedFileWorkspace,
    file_index: usize,
) -> Result<SceneUnityVersion, Box<dyn Error>> {
    let file = workspace
        .files()
        .get(file_index)
        .ok_or_else(|| invalid_project(format!("workspace file {file_index} is unavailable")))?;
    let text = file
        .metadata
        .prefix
        .unity_version_text
        .as_deref()
        .ok_or_else(|| {
            invalid_project(format!(
                "workspace file {file_index} has no Unity version for Prefab export IDs"
            ))
        })?;
    SceneUnityVersion::parse(text).ok_or_else(|| {
        invalid_project(format!(
            "workspace file {file_index} has invalid Unity version {text:?}"
        ))
    })
}

fn unity_version_has_prefab_importer(version: SceneUnityVersion) -> bool {
    version.major > 2018 || (version.major == 2018 && version.minor >= 3)
}

fn source_object_key(file_index: usize, object_index: usize) -> ExportAssetObjectKey {
    ExportAssetObjectKey {
        collection: ExportAssetCollectionIdentity::SerializedFile { file_index },
        object_index,
    }
}

fn has_sprite_reference_to_object(
    reference_graph: &MultiFileReferenceGraph<'_>,
    file_index: usize,
    object_index: usize,
) -> bool {
    const SPRITE_CLASS_ID: i32 = 213;
    reference_graph
        .references_to_object(file_index, object_index)
        .any(|reference| {
            reference_graph
                .object_class_id(reference.source_file_index, reference.source_object_index)
                == Some(SPRITE_CLASS_ID)
        })
}

fn opaque_factory_object_keys(
    objects: &ObjectWorkspace,
) -> Result<HashSet<(usize, usize)>, Box<dyn Error>> {
    let mut keys = HashSet::new();
    keys.try_reserve(objects.report().undecoded_objects)
        .map_err(|_| invalid_project("could not allocate opaque raw-object identities"))?;
    for object in objects.objects() {
        if !object.is_fully_materialized()
            && matches!(
                &object.factory.kind,
                FactoryAssetKind::Unknown | FactoryAssetKind::Unreadable
            )
        {
            keys.insert((object.file_index, object.object_index));
        }
    }
    Ok(keys)
}

fn engine_resource_collection_guid(name: &str) -> Option<[u8; 16]> {
    let words = if crate::io::special_names::is_default_resource(name) {
        crate::export::guid_policy::ENGINE_DEFAULT_RESOURCES_GUID_WORDS
    } else if crate::io::special_names::is_builtin_extra(name) {
        crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS
    } else {
        return None;
    };
    let mut bytes = [0; 16];
    for (index, word) in words.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    Some(bytes)
}

fn scene_guid_words_to_bytes(words: SceneGuidWords) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (index, word) in words.data.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn mint_unique_collection_guid(
    next_sequence: &mut u64,
    used_guids: &mut HashSet<[u8; 16]>,
) -> Result<[u8; 16], Box<dyn Error>> {
    loop {
        let sequence = *next_sequence;
        *next_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection GUID sequence overflows"))?;
        let guid = new_collection_guid(sequence);
        if guid != [0; 16] && used_guids.insert(guid) {
            return Ok(guid);
        }
    }
}

fn scene_build_settings_entry_is_duplicate(
    processing: &WorkspaceSceneProcessing,
    path_source: SceneDefinitionPathSource,
) -> bool {
    let SceneDefinitionPathSource::BuildSettings { scene_index } = path_source else {
        return false;
    };
    let Some(scenes) = processing
        .editor_build_settings
        .as_ref()
        .map(|settings| settings.scenes())
    else {
        return false;
    };
    let Some(selected) = scenes.get(scene_index) else {
        return false;
    };
    scenes
        .iter()
        .enumerate()
        .any(|(index, scene)| index != scene_index && scene.path == selected.path)
}

fn mono_importer_yaml(execution_order: i16) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    importer.insert_field(
        "serializedVersion",
        UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(2)),
    )?;
    importer.insert_field(
        "externalObjects",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field(
        "defaultReferences",
        UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow),
    )?;
    importer.insert_field(
        "executionOrder",
        UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(i64::from(
            execution_order,
        ))),
    )?;
    let mut icon = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    icon.insert_field(
        "fileID",
        UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(0)),
    )?;
    importer.insert_field("icon", icon)?;
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field("assetBundleName", UnityYamlNode::string(""))?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

fn plugin_importer_yaml(unity_version_text: &str) -> Result<UnityYamlNode, Box<dyn Error>> {
    let version = FactoryUnityVersion::parse(unity_version_text).ok_or_else(|| {
        invalid_project(format!(
            "PluginImporter export cannot parse Unity version {unity_version_text:?}"
        ))
    })?;
    if version.is_unsupported() {
        return Err(invalid_project(format!(
            "PluginImporter export does not support baseline-unsupported Unity version {unity_version_text:?}"
        )));
    }
    let version_bits = version.source_order_bits().ok_or_else(|| {
        invalid_project(format!(
            "PluginImporter export cannot order Unity version {unity_version_text:?}"
        ))
    })?;
    // The frozen class-1050 dispatch changes the generated PluginImporter type at 2023.3.0a15;
    // that version switches platformData from the old pair sequence to the named map.
    let named_platform_data_from = FactoryUnityVersion::parse("2023.3.0a15")
        .and_then(|version| version.source_order_bits())
        .ok_or_else(|| invalid_project("invalid internal PluginImporter version transition"))?;
    let uses_named_platform_data = version_bits >= named_platform_data_from;
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    importer.insert_field(
        "serializedVersion",
        UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(
            if uses_named_platform_data { 3 } else { 2 },
        )),
    )?;
    importer.insert_field(
        "externalObjects",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field(
        "iconMap",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field(
        "executionOrder",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field(
        "defineConstraints",
        UnityYamlNode::sequence(UnityYamlSequenceStyle::Block),
    )?;
    for field in [
        "isPreloaded",
        "isOverridable",
        "isExplicitlyReferenced",
        "validateReferences",
    ] {
        importer.insert_field(
            field,
            UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(0)),
        )?;
    }

    let platform_data = if uses_named_platform_data {
        let mut platform_data = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        let mut any_settings = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        any_settings.insert_field(
            "enabled",
            UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(1)),
        )?;
        any_settings.insert_field(
            "settings",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
        )?;
        platform_data.insert_field("Any", any_settings)?;

        let mut editor_settings = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        editor_settings.insert_field(
            "enabled",
            UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(0)),
        )?;
        let mut editor_defaults = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        editor_defaults.insert_field(
            "DefaultValueInitialized",
            UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::PlainString(
                "true".to_owned(),
            )),
        )?;
        editor_settings.insert_field("settings", editor_defaults)?;
        platform_data.insert_field("Editor", editor_settings)?;
        platform_data
    } else {
        let mut platform_data = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
        let mut any_platform = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        any_platform.insert_field("Any", UnityYamlNode::string(""))?;
        let mut any_settings = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        any_settings.insert_field(
            "enabled",
            UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(1)),
        )?;
        any_settings.insert_field(
            "settings",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
        )?;
        let mut any_entry = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        any_entry.insert_field("first", any_platform)?;
        any_entry.insert_field("second", any_settings)?;
        platform_data.push(any_entry)?;

        let mut editor_platform = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        editor_platform.insert_field("Editor", UnityYamlNode::string("Editor"))?;
        let mut editor_settings = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        editor_settings.insert_field(
            "enabled",
            UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(0)),
        )?;
        let mut initialized = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        initialized.insert_field(
            "DefaultValueInitialized",
            UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::PlainString(
                "true".to_owned(),
            )),
        )?;
        editor_settings.insert_field("settings", initialized)?;
        let mut editor_entry = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        editor_entry.insert_field("first", editor_platform)?;
        editor_entry.insert_field("second", editor_settings)?;
        platform_data.push(editor_entry)?;
        platform_data
    };
    importer.insert_field("platformData", platform_data)?;

    let mut output = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    output.insert_field(
        "dllType",
        UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(0)),
    )?;
    output.insert_field(
        "scriptingRuntimeVersion",
        UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(0)),
    )?;
    importer.insert_field("output", output)?;
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field("assetBundleName", UnityYamlNode::string(""))?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

fn default_importer(asset_bundle_name: Option<&str>) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(super::unity_yaml::UnityYamlMappingStyle::Block);
    importer.insert_field(
        "externalObjects",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field(
        "assetBundleName",
        UnityYamlNode::string(asset_bundle_name.unwrap_or("")),
    )?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

fn scene_default_importer(
    version: SceneUnityVersion,
    asset_bundle_name: Option<&str>,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    if (version.major, version.minor) < (2018, 3) {
        importer.insert_field("name", UnityYamlNode::string(""))?;
    } else {
        importer.insert_field(
            "externalObjects",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
        )?;
    }
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field(
        "assetBundleName",
        UnityYamlNode::string(asset_bundle_name.unwrap_or("")),
    )?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

fn prefab_importer(asset_bundle_name: Option<&str>) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    importer.insert_field(
        "externalObjects",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field("addedObjectFileIDs", UnityYamlNode::string(""))?;
    importer.insert_field(
        "isPrefabVariant",
        UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(0)),
    )?;
    importer.insert_field(
        "unableToImportOnPreviousDomainReload",
        UnityYamlNode::scalar(super::unity_yaml::UnityYamlScalar::Signed(0)),
    )?;
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field(
        "assetBundleName",
        UnityYamlNode::string(asset_bundle_name.unwrap_or("")),
    )?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

fn count_unity_yaml_documents(yaml: &str) -> usize {
    yaml.lines()
        .filter(|line| line.starts_with("--- !u!"))
        .count()
}

fn workspace_project_version(
    workspace: &SerializedFileWorkspace,
) -> Result<(&str, SceneUnityVersion), Box<dyn Error>> {
    if workspace.files().is_empty() {
        return Err(invalid_project("project export has no SerializedFiles"));
    }
    let editor_version_text = workspace
        .files()
        .iter()
        .find_map(|file| file.metadata.prefix.unity_version_text.as_deref())
        .ok_or_else(|| invalid_project("project export needs a SerializedFile Unity version"))?;
    let project_version = SceneUnityVersion::parse(editor_version_text).ok_or_else(|| {
        invalid_project(format!(
            "project export cannot parse Unity version {editor_version_text:?}"
        ))
    })?;
    Ok((editor_version_text, project_version))
}

/// Build the `PathIdMapExporter` input in source collection and object-table order.
fn build_path_id_map(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
) -> Result<SerializedGameInfo, Box<dyn Error>> {
    let mut files = Vec::new();
    files
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_project("could not reserve PathIdMap serialized-file records"))?;
    for (file_index, file) in workspace.files().iter().enumerate() {
        let collection_name = crate::io::special_names::fix_file_identifier(&file.file_name);
        let objects_in_file = objects.objects_for_file(file_index);
        let mut assets = Vec::new();
        assets
            .try_reserve_exact(objects_in_file.len())
            .map_err(|_| invalid_project("could not reserve PathIdMap object records"))?;
        for object in objects_in_file {
            let crate::processing::asset_factory::FactoryAssetKind::Generated(generated_type) =
                &object.factory.kind
            else {
                // The source raw and TypeTree fallback wrappers do not implement generated
                // asset interfaces, even when their class ID has generated types in other
                // versions.
                continue;
            };
            let class_id = object.factory.class_id;
            if !has_path_id_map_generated_types(class_id) {
                continue;
            }
            let Some(interfaces) = generated_type_interfaces(class_id, generated_type) else {
                return Err(invalid_project(format!(
                    "PathIdMap has no frozen generated-interface record for class {class_id} type {generated_type:?} in {} PathID {}",
                    file.file_name, object.path_id
                )));
            };
            if !interfaces.is_recorded() {
                continue;
            }
            assets.push(PathIdMapAssetFacts {
                is_mesh: interfaces.is_mesh,
                is_texture: interfaces.is_texture,
                is_audio_clip: interfaces.is_audio_clip,
                is_text_asset: interfaces.is_text_asset,
                is_named: interfaces.is_named,
                name: interfaces.is_named.then_some(object.factory.name.as_str()),
                class_name: object.factory.class_name.as_ref(),
                path_id: object.path_id,
            });
        }
        files.push(build_serialized_file(Some(&collection_name), &assets));
    }
    Ok(SerializedGameInfo { files })
}

/// The named asset name (`INamed.Name`) of one decoded payload, if the TypeTree has one.
fn object_asset_name(payload: &ParsedObjectPayload) -> Option<String> {
    for field in ["m_Name", "Name"] {
        if let Ok(Some(crate::serialized_file::object::SerializedValue::StringBytes(bytes))) =
            payload.root.unique_field_in_base_chain(field)
        {
            if !bytes.is_empty() {
                return Some(String::from_utf8_lossy(bytes).into_owned());
            }
        }
    }
    None
}

fn validate_bounded_lighting_data_scene(
    hierarchy: &WorkspaceSceneHierarchyScene,
    owner: WorkspaceSceneObjectIdentity,
) -> Result<(), Box<dyn Error>> {
    let mut unsupported_objects = Vec::new();
    for collection in hierarchy.collections() {
        if collection.game_object_count() != 0
            || collection.transform_count() != 0
            || !collection.component_candidates().is_empty()
        {
            unsupported_objects.push(format!(
                "file {} has {} GameObjects, {} Transforms, and {} component candidates",
                collection.collection_file_index(),
                collection.game_object_count(),
                collection.transform_count(),
                collection.component_candidates().len()
            ));
        }
        for asset in collection.scene_assets() {
            if matches!(
                asset.category,
                WorkspaceSceneAssetCategory::GameObject
                    | WorkspaceSceneAssetCategory::PrefabInstance
            ) {
                unsupported_objects.push(format!(
                    "file {} object {} class {:?} is outside the bounded LightingDataAsset route",
                    asset.identity.file_index, asset.identity.object_index, asset.identity.class_id
                ));
            }
        }
    }
    if !unsupported_objects.is_empty() {
        return Err(invalid_project(format!(
            "LightingDataAsset generation for LightmapSettings {}:{} cannot omit renderer, terrain, light, or scene-object lighting data: {}",
            owner.file_index,
            owner.path_id,
            unsupported_objects.join("; ")
        )));
    }
    Ok(())
}

fn scene_manager_identity(
    hierarchy: &WorkspaceSceneHierarchyScene,
    kind: SceneManagerKind,
) -> Option<WorkspaceSceneObjectIdentity> {
    hierarchy
        .collections()
        .iter()
        .flat_map(|collection| collection.scene_assets())
        .find(|asset| asset.category == WorkspaceSceneAssetCategory::LevelGameManager(kind))
        .map(|asset| asset.identity)
}

fn export_identity_for_scene_object(
    identity: WorkspaceSceneObjectIdentity,
) -> Result<ExportAssetIdentity, Box<dyn Error>> {
    let class_id = identity.class_id.ok_or_else(|| {
        invalid_project(format!(
            "workspace object {}:{} has no class ID for project export",
            identity.file_index, identity.path_id
        ))
    })?;
    if identity.path_id == 0 {
        return Err(invalid_project(format!(
            "workspace object {}:{} has a null PathID",
            identity.file_index, identity.path_id
        )));
    }
    Ok(ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: identity.file_index,
        },
        object_index: identity.object_index,
        path_id: identity.path_id,
        class_id,
    })
}

fn build_lighting_data_asset_root(
    type_tree: &crate::serialized_file::metadata::TypeTree,
    object_index: usize,
    lightmap_settings: &SerializedValue,
    render_settings: &SerializedValue,
    source_version: &FactoryUnityVersion,
    scene_guid: [u8; 16],
) -> Result<SerializedValue, Box<dyn Error>> {
    let mut root = default_value_for_type_tree_node(
        type_tree,
        object_index,
        0,
        ObjectDefaultValueLimits::default(),
    )
    .map_err(|error| {
        invalid_project(format!(
            "LightingDataAsset default value could not be built from its reference TypeTree: {error}"
        ))
    })?;
    prepend_lighting_data_object_fields(&mut root, source_version)?;

    set_required_lighting_data_field(
        &mut root,
        "m_Name",
        SerializedValue::StringBytes(b"LightingData".to_vec()),
    )?;
    if unique_lighting_data_field_index(&root, "m_Scene")?.is_some() {
        set_required_lighting_data_field(
            &mut root,
            "m_Scene",
            SerializedValue::PPtr {
                file_id: 0,
                // A non-null sentinel forces the YAML converter through the generated SceneAsset
                // pointer resolver. It is never written to a SerializedFile or emitted as YAML.
                path_id: i64::MIN,
            },
        )?;
    } else if unique_lighting_data_field_index(&root, "m_SceneGUID")?.is_some() {
        set_lighting_data_guid_field(&mut root, "m_SceneGUID", scene_guid)?;
    }
    let enlighten_data = create_empty_lighting_enlighten_data(source_version)?;
    let enlighten_data_value =
        lighting_data_bytes_field_value(&root, "m_EnlightenData", enlighten_data)?;
    set_required_lighting_data_field(&mut root, "m_EnlightenData", enlighten_data_value)?;
    if unique_lighting_data_field_index(&root, "m_EnlightenDataVersion")?.is_some() {
        set_required_lighting_data_field(
            &mut root,
            "m_EnlightenDataVersion",
            SerializedValue::I32(112),
        )?;
    }

    copy_required_lighting_data_field(lightmap_settings, &mut root, "m_Lightmaps", "m_Lightmaps")?;
    copy_optional_lighting_data_field(
        lightmap_settings,
        &mut root,
        "m_LightmapsMode",
        "m_LightmapsMode",
    )?;
    copy_required_lighting_data_field(
        lightmap_settings,
        &mut root,
        "m_EnlightenSceneMapping",
        "m_EnlightenSceneMapping",
    )?;
    copy_optional_lighting_data_field(
        lightmap_settings,
        &mut root,
        "m_LightProbes",
        "m_LightProbes",
    )?;
    copy_render_settings_probe(
        render_settings,
        &mut root,
        "m_AmbientProbe",
        "m_BakedAmbientProbeInLinear",
        "m_BakedAmbientProbesInLinear",
    )?;
    copy_render_settings_probe(
        render_settings,
        &mut root,
        "m_AmbientProbeInGamma",
        "m_BakedAmbientProbeInGamma",
        "m_BakedAmbientProbesInGamma",
    )?;

    if contains_non_null_pointer(render_settings) {
        return Err(invalid_project(
            "bounded LightingDataAsset generation cannot omit a non-null RenderSettings object reference used by the source reflection path",
        ));
    }
    Ok(root)
}
