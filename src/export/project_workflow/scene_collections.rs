use super::*;

pub(super) fn lightmap_texture_scene_directories(
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

pub(super) fn prepare_scene_collections(
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

pub(super) fn source_object_key(file_index: usize, object_index: usize) -> ExportAssetObjectKey {
    ExportAssetObjectKey {
        collection: ExportAssetCollectionIdentity::SerializedFile { file_index },
        object_index,
    }
}

pub(super) fn has_sprite_reference_to_object(
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

pub(super) fn opaque_factory_object_keys(
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

pub(super) fn engine_resource_collection_guid(name: &str) -> Option<[u8; 16]> {
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

pub(super) fn scene_guid_words_to_bytes(words: SceneGuidWords) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (index, word) in words.data.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    bytes
}

pub(super) fn mint_unique_collection_guid(
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

pub(super) fn scene_build_settings_entry_is_duplicate(
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
