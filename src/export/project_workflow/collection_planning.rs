use super::*;

pub(super) struct PreparedGeneratedCollections {
    pub(super) scenes: Vec<RoutedSceneCollection>,
    pub(super) prefabs: Vec<RoutedPrefabCollection>,
    pub(super) lighting_data_assets: Vec<RoutedLightingDataAsset>,
    pub(super) next_collection_index: usize,
    /// GUID mint state forwarded to the asset planning pass, which mints file
    /// GUIDs for controller sub-assets that fell out of their hierarchies.
    pub(super) next_guid_sequence: u64,
    pub(super) used_guids: HashSet<[u8; 16]>,
}

pub(super) struct GeneratedCollectionPreparationContext<'context, 'graph> {
    pub(super) workspace: &'context SerializedFileWorkspace,
    pub(super) objects: &'context ObjectWorkspace,
    pub(super) reference_graph: &'context MultiFileReferenceGraph<'graph>,
    pub(super) scene_processing: &'context WorkspaceSceneProcessing,
    pub(super) scenes: Vec<RoutedSceneCollection>,
    pub(super) prefabs: Vec<RoutedPrefabCollection>,
    pub(super) next_collection_index: usize,
    pub(super) next_guid_sequence: u64,
    pub(super) used_guids: HashSet<[u8; 16]>,
}

pub(super) fn prepare_generated_collections(
    context: GeneratedCollectionPreparationContext<'_, '_>,
) -> Result<PreparedGeneratedCollections, Box<dyn Error>> {
    let GeneratedCollectionPreparationContext {
        workspace,
        objects,
        reference_graph,
        scene_processing,
        scenes,
        prefabs,
        mut next_collection_index,
        mut next_guid_sequence,
        mut used_guids,
    } = context;
    let mut scene_collections = scenes;
    let mut prefab_collections = prefabs;
    for prefab in &mut prefab_collections {
        prefab.collection_index = next_collection_index;
        next_collection_index = next_collection_index
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        prefab.guid = mint_unique_collection_guid(&mut next_guid_sequence, &mut used_guids)?;
    }

    for scene in &mut scene_collections {
        scene.collection_index = next_collection_index;
        next_collection_index = next_collection_index
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        scene.guid = match scene.guid_plan {
            SceneDefinitionGuidPlan::Preserve(words) if !words.is_zero() => {
                scene_guid_words_to_bytes(words)
            }
            SceneDefinitionGuidPlan::Preserve(_) | SceneDefinitionGuidPlan::GenerateAtExport(_) => {
                mint_unique_collection_guid(&mut next_guid_sequence, &mut used_guids)?
            }
        };
        used_guids.insert(scene.guid);
    }

    let mut lighting_data_assets = Vec::new();
    lighting_data_assets
        .try_reserve_exact(scene_collections.len())
        .map_err(|_| invalid_project("could not allocate generated LightingDataAsset routes"))?;
    for scene in &scene_collections {
        let hierarchy = scene_processing
            .hierarchies
            .scenes()
            .get(scene.scene_index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "SceneDefinition {} has no hierarchy while planning LightingDataAsset",
                    scene.scene_index
                ))
            })?;
        let Some(owner_identity) = hierarchy.lighting_data_dependency_owner() else {
            continue;
        };
        let source_owner = export_identity_for_scene_object(owner_identity)?;
        let owner_file = workspace
            .files()
            .get(owner_identity.file_index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "LightmapSettings file index {} is outside the workspace",
                    owner_identity.file_index
                ))
            })?;
        let version_text = owner_file
            .metadata
            .prefix
            .unity_version_text
            .as_deref()
            .ok_or_else(|| {
                invalid_project(format!(
                    "LightmapSettings file {} has no Unity version for LightingDataAsset generation",
                    owner_file.file_name
                ))
            })?;
        let source_version = FactoryUnityVersion::parse(version_text).ok_or_else(|| {
            invalid_project(format!(
                "LightmapSettings file {} has unparseable Unity version {version_text:?}",
                owner_file.file_name
            ))
        })?;
        let scene_version = SceneUnityVersion::parse(version_text).ok_or_else(|| {
            invalid_project(format!(
                "LightmapSettings file {} has unsupported Unity version {version_text:?}",
                owner_file.file_name
            ))
        })?;
        let renderer_inputs =
            collect_lighting_data_renderer_inputs(&scene.assignments, objects, reference_graph)?;
        let terrain_inputs = collect_lighting_data_terrain_inputs(&scene.assignments, objects)?;
        let light_inputs = collect_lighting_data_light_inputs(&scene.assignments, objects)?;
        validate_bounded_lighting_data_scene(hierarchy, owner_identity)?;
        let render_settings_identity =
            scene_manager_identity(hierarchy, SceneManagerKind::RenderSettings).ok_or_else(
                || {
                    invalid_project(format!(
                        "LightmapSettings {}:{} has no RenderSettings owner",
                        owner_identity.file_index, owner_identity.path_id
                    ))
                },
            )?;
        let (type_tree, _) = reference_class_layouts()
            .tpk_trees(1120, &source_version)
            .map_err(|error| {
                invalid_project(format!(
                    "LightingDataAsset TypeTree lookup failed for Unity {version_text}: {error}"
                ))
            })?
            .ok_or_else(|| {
                invalid_project(format!(
                    "LightingDataAsset has no reference TypeTree for Unity {version_text}"
                ))
            })?;
        let source_owner_payload = objects
            .get(owner_identity.file_index, owner_identity.object_index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "LightmapSettings {}:{} has no materialized workspace object",
                    owner_identity.file_index, owner_identity.path_id
                ))
            })?
            .payload
            .as_ref()
            .map_err(|error| {
                invalid_project(format!(
                    "LightmapSettings {}:{} could not be decoded for LightingDataAsset generation: {error}",
                    owner_identity.file_index, owner_identity.path_id
                ))
            })?;
        let source_render_settings_payload = objects
            .get(
                render_settings_identity.file_index,
                render_settings_identity.object_index,
            )
            .ok_or_else(|| {
                invalid_project(format!(
                    "RenderSettings {}:{} has no materialized workspace object",
                    render_settings_identity.file_index, render_settings_identity.path_id
                ))
            })?
            .payload
            .as_ref()
            .map_err(|error| {
                invalid_project(format!(
                    "RenderSettings {}:{} could not be decoded for LightingDataAsset generation: {error}",
                    render_settings_identity.file_index, render_settings_identity.path_id
                ))
            })?;
        let mut root = build_lighting_data_asset_root(
            &type_tree,
            owner_identity.object_index,
            &source_owner_payload.root,
            &source_render_settings_payload.root,
            &source_version,
            scene.guid,
        )?;
        let renderer_uv_mesh_targets =
            append_lighting_data_renderer_records(&type_tree, &mut root, &renderer_inputs)?;
        append_lighting_data_terrain_records(&type_tree, &mut root, &terrain_inputs)?;
        append_lighting_data_light_records(&type_tree, &mut root, &light_inputs)?;
        let collection_index = next_collection_index;
        next_collection_index = next_collection_index
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        let guid = mint_unique_collection_guid(&mut next_guid_sequence, &mut used_guids)?;
        lighting_data_assets.push(RoutedLightingDataAsset {
            scene_index: scene.scene_index,
            source_owner,
            render_settings_owner: export_identity_for_scene_object(render_settings_identity)?,
            scene_guid: scene.guid,
            collection_index,
            identity: ExportAssetIdentity {
                collection: ExportAssetCollectionIdentity::ProcessedCollection { collection_index },
                object_index: 0,
                path_id: 1,
                class_id: 1120,
            },
            guid,
            original_directory: scene_processing.hierarchies.scenes()[scene.scene_index]
                .plan
                .path
                .clone(),
            source_version: scene_version,
            type_tree: (*type_tree).clone(),
            root,
            renderer_uv_mesh_targets,
        });
    }

    Ok(PreparedGeneratedCollections {
        scenes: scene_collections,
        prefabs: prefab_collections,
        lighting_data_assets,
        next_collection_index,
        next_guid_sequence,
        used_guids,
    })
}

pub(super) struct ProjectPointerPlanningContext<'context, 'object> {
    pub(super) routed: &'context [RoutedObject<'object>],
    pub(super) scenes: &'context [RoutedSceneCollection],
    pub(super) prefabs: &'context [RoutedPrefabCollection],
    pub(super) lighting_data_assets: &'context [RoutedLightingDataAsset],
    pub(super) next_collection_index: usize,
    pub(super) limits: ExportPointerMapLimits,
}

pub(super) struct ProjectPointerPlan {
    pub(super) pointer_map: ProjectExportPointerMap,
    pub(super) collection_count: usize,
}

pub(super) fn build_project_pointer_map(
    context: ProjectPointerPlanningContext<'_, '_>,
) -> Result<ProjectPointerPlan, Box<dyn Error>> {
    let ProjectPointerPlanningContext {
        routed,
        scenes,
        prefabs,
        lighting_data_assets,
        next_collection_index,
        limits,
    } = context;
    let scene_assignment_count = scenes
        .iter()
        .try_fold(0usize, |count, scene| {
            count.checked_add(scene.assignments.len())
        })
        .ok_or_else(|| invalid_project("project pointer count overflows"))?;
    let routed_pointer_count = routed.iter().try_fold(0usize, |count, entry| {
        count
            .checked_add(1)?
            .checked_add(
                entry
                    .scriptable_object_group
                    .as_ref()
                    .map_or(0, |group| group.children.len()),
            )?
            .checked_add(
                entry
                    .font_collection
                    .as_ref()
                    .map_or(0, |group| group.children.len()),
            )?
            .checked_add(
                entry
                    .controller_collection
                    .as_ref()
                    .map_or(0, |group| group.children.len()),
            )
    });
    let pointer_capacity = routed_pointer_count
        .ok_or_else(|| invalid_project("project pointer count overflows"))?
        .checked_add(scene_assignment_count)
        .and_then(|count| count.checked_add(lighting_data_assets.len()))
        .and_then(|count| {
            prefabs.iter().try_fold(count, |total, prefab| {
                total.checked_add(prefab.export_ids.registered_assets.len())
            })
        })
        .ok_or_else(|| invalid_project("project pointer count overflows"))?;
    let mut pointer_entries = Vec::new();
    let mut local_copies = Vec::new();
    let local_copy_count = routed
        .iter()
        .try_fold(0usize, |count, entry| {
            count.checked_add(entry.controller_collection.as_ref().map_or(0, |group| {
                group
                    .children
                    .iter()
                    .filter(|child| !child.register_global)
                    .count()
            }))
        })
        .ok_or_else(|| invalid_project("controller-local export copy count overflows"))?;
    local_copies
        .try_reserve_exact(local_copy_count)
        .map_err(|_| invalid_project("could not allocate controller-local export copies"))?;
    pointer_entries
        .try_reserve_exact(pointer_capacity)
        .map_err(|_| invalid_project("could not allocate project export pointer entries"))?;
    let mut collection_metadata = Vec::new();
    collection_metadata
        .try_reserve_exact(next_collection_index)
        .map_err(|_| invalid_project("could not allocate project export collection metadata"))?;
    let mut collection_count = 0usize;
    for entry in routed {
        if entry.route == ObjectRoute::Skipped {
            continue;
        }
        if entry.route != ObjectRoute::EngineRedirect {
            collection_count += 1;
        }
        // UnknownExportCollection and UnreadableExportCollection write raw files without
        // `.meta` files or export GUIDs. Do not register their synthetic in-memory GUIDs as
        // Unity project pointers; references to them resolve through the explicit missing-map
        // path instead of targeting a GUID that no output metadata defines.
        if matches!(entry.route, ObjectRoute::Unknown | ObjectRoute::Unreadable) {
            continue;
        }
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: entry.collection_index,
            guid: entry.pointer_guid,
            asset_type: if matches!(
                entry.route,
                ObjectRoute::Script
                    | ObjectRoute::ScriptPointerOnly
                    | ObjectRoute::TextAsset
                    | ObjectRoute::AudioBinary
                    | ObjectRoute::FontBinary
                    | ObjectRoute::TextureImage
                    // The frozen `BinaryAssetExporter.ToExportType` (which the Dummy and
                    // Simple shader exporters inherit) reports `AssetType.Meta`; only the
                    // opt-in YAML shader exporter reports `Serialized`.
                    | ObjectRoute::ShaderText
            ) {
                UnityProjectAssetType::Meta
            } else if entry.route == ObjectRoute::EngineRedirect {
                UnityProjectAssetType::Internal
            } else {
                UnityProjectAssetType::Serialized
            },
        });
        pointer_entries.push(ExportedAssetPointerEntry {
            asset: identity_of(entry),
            file_id: entry.file_id,
            collection_index: entry.collection_index,
        });
        if let Some(group) = &entry.scriptable_object_group {
            pointer_entries.extend(
                group
                    .children
                    .iter()
                    .map(|child| ExportedAssetPointerEntry {
                        asset: ExportAssetIdentity {
                            collection: ExportAssetCollectionIdentity::SerializedFile {
                                file_index: child.object.file_index,
                            },
                            object_index: child.object.object_index,
                            path_id: child.object.path_id,
                            class_id: child.object.factory.class_id,
                        },
                        file_id: child.file_id,
                        collection_index: entry.collection_index,
                    }),
            );
        }
        if let Some(group) = &entry.font_collection {
            pointer_entries.extend(
                group
                    .children
                    .iter()
                    .map(|child| ExportedAssetPointerEntry {
                        asset: ExportAssetIdentity {
                            collection: ExportAssetCollectionIdentity::SerializedFile {
                                file_index: child.object.file_index,
                            },
                            object_index: child.object.object_index,
                            path_id: child.object.path_id,
                            class_id: child.object.factory.class_id,
                        },
                        file_id: child.file_id,
                        collection_index: entry.collection_index,
                    }),
            );
        }
        if let Some(group) = &entry.controller_collection {
            for child in &group.children {
                let pointer = ExportedAssetPointerEntry {
                    asset: ExportAssetIdentity {
                        collection: ExportAssetCollectionIdentity::SerializedFile {
                            file_index: child.object.file_index,
                        },
                        object_index: child.object.object_index,
                        path_id: child.object.path_id,
                        class_id: child.object.factory.class_id,
                    },
                    file_id: child.file_id,
                    collection_index: entry.collection_index,
                };
                if child.register_global {
                    pointer_entries.push(pointer);
                } else {
                    local_copies.push(pointer);
                }
            }
        }
    }
    for scene in scenes {
        collection_count = collection_count
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: scene.collection_index,
            guid: scene.guid,
            asset_type: UnityProjectAssetType::Serialized,
        });
        pointer_entries.extend(scene.assignments.iter().map(|assignment| {
            ExportedAssetPointerEntry {
                asset: assignment.asset,
                file_id: assignment.file_id,
                collection_index: scene.collection_index,
            }
        }));
    }
    for prefab in prefabs {
        // Keep validating the distinct source-prefab pointer's GUID and main fileID.
        let _source_pointer = generate_prefab_source_pointer(prefab.guid, prefab.representation)
            .map_err(|error| invalid_project(format!("Prefab pointer planning failed: {error}")))?;
        collection_count = collection_count
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: prefab.collection_index,
            guid: prefab.guid,
            // PPtrs to assets contained in a PrefabExportCollection use the YAML
            // exporter's serialized type. GenerateMetaPtrForPrefab is a separate
            // pointer used for IPrefabInstance.SourcePrefabP, not this collection's
            // ordinary member references.
            asset_type: UnityProjectAssetType::Serialized,
        });
        pointer_entries.extend(
            prefab
                .export_ids
                .registered_assets
                .iter()
                .map(|assignment| ExportedAssetPointerEntry {
                    asset: assignment.asset,
                    file_id: assignment.file_id,
                    collection_index: prefab.collection_index,
                }),
        );
    }
    for lighting_data in lighting_data_assets {
        collection_count = collection_count
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: lighting_data.collection_index,
            guid: lighting_data.guid,
            asset_type: UnityProjectAssetType::Serialized,
        });
        pointer_entries.push(ExportedAssetPointerEntry {
            asset: lighting_data.identity,
            file_id: 112_000_000,
            collection_index: lighting_data.collection_index,
        });
    }
    let mut pointer_map =
        ProjectExportPointerMap::new(&pointer_entries, &collection_metadata, limits)?;
    pointer_map.register_local_copies(&local_copies, limits)?;
    Ok(ProjectPointerPlan {
        pointer_map,
        collection_count,
    })
}
