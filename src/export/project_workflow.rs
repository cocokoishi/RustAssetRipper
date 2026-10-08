//! Executable Unity project export composed from collection dispatch, staged planning, and
//! the `ExportHandler.Export` write sequence.
//!
//! This is the product-level project path: every decoded workspace object is routed through the
//! frozen `ProjectExporter.CreateCollection` stack walk ([`crate::export::project_exporter`]),
//! every route is materialized into a staged [`ProjectExportPlan`], and only then does
//! `execute_project_export` touch the disk. Objects whose baseline collection is not exportable
//! are reported as explicit notices; families whose specialized renderer is not ported fall back
//! to the generic Unity YAML asset family instead of being dropped.
//!
//! Scene definitions are processed into workspace hierarchies and emitted as `.unity` files
//! through the scene collection route, with scene IDs, pointers, and `DefaultImporter` metadata.
//! Modern PrefabProcessor hierarchies are emitted as `.prefab` collections with planned IDs,
//! generated Transforms, pointer mapping, and `PrefabImporter` metadata. Legacy generated Prefabs,
//! full AssetBundle-derived importer metadata, and the remaining class-specific exporters remain
//! required integrations.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::hash::BuildHasher;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::PlatformResourceProvider;
use crate::processing::scene_collections::{
    SceneDefinitionGuidPlan, SceneDefinitionPathSource, SceneGuidWords,
};
use crate::processing::scene_naming::SceneUnityVersion;
use crate::resource::{RootResourceFileCatalog, StreamedResourceContentLimits};
use crate::serialized_file::object::ParsedObjectPayload;
use crate::serialized_file::object_workspace::{ObjectWorkspace, WorkspaceObject};
use crate::serialized_file::reference_graph::{MultiFilePPtrResolution, MultiFileReferenceGraph};
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::collection_kind::ExportCollectionKind;
use super::export_extension::DEFAULT_EXPORT_EXTENSION;
use super::pointer_map::{
    ExportCollectionPointerMetadata, ExportPointerMapLimits, ExportedAssetPointerEntry,
    ProjectExportPointerMap, UnityProjectAssetType,
};
use super::prefab_collection::{
    generate_prefab_source_pointer, plan_prefab_export_ids, PrefabAssetRepresentation,
    PrefabExportIdPlan, PrefabExportPlanLimits, PrefabHierarchyAssetGroups, VersionedPrefabAsset,
};
use super::project_exporter::{
    execute_project_export, export_extension_for, native_format_importer, object_class_name,
    package_manifest_json, project_version_file, select_collection, ObjectClassFacts,
    PlannedProjectFile, PlannedProjectMeta, ProjectExportError, ProjectExportNotice,
    ProjectExportPlan, ProjectExportSettings, ProjectPathPlanner,
};
use super::project_plan::plan_scene_export_paths;
use super::scene_collection::{
    plan_workspace_scene_export_ids_with_processed_assets, ExportAssetCollectionIdentity,
    ExportAssetIdentity, ExportAssetObjectKey, SceneExportIdAssignment, SceneExportIdPlanLimits,
    SceneHierarchyAssetOrderLimits,
};
use super::scene_wiring::{
    process_workspace_scenes_and_prefabs, WorkspaceSceneProcessing, WorkspaceSceneProcessingLimits,
};
use super::settings_file::ExportSettingsFile;
use super::streamed_assets::StreamedYamlAssetLimits;
use super::unity_yaml::{
    write_unity_yaml_documents, UnityYamlMappingStyle, UnityYamlNode, UnityYamlWriteLimits,
};
use super::workspace_yaml::{
    workspace_object_to_unity_yaml_document_with_streamed_resources,
    workspace_prefab_hierarchy_to_unity_yaml_with_generated_transforms,
    workspace_scene_assignments_to_unity_yaml_with_streamed_resources,
    workspace_scene_assignments_to_unity_yaml_with_streamed_resources_and_generated_transforms,
    UnityYamlWorkspaceCollectionLimits, WorkspaceGeneratedTransformYamlContext,
    WorkspaceStreamedYamlResourceContext,
};

const MAX_PROJECT_OBJECTS: usize = 20_000_000;
const MAX_PROJECT_YAML_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// Resource and allocation bounds for one workspace project export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceProjectExportLimits {
    pub max_objects: usize,
    pub max_total_yaml_bytes: usize,
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
            max_total_yaml_bytes: MAX_PROJECT_YAML_BYTES,
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
    /// Asset files that received an importer sidecar.
    pub meta_file_count: usize,
    pub total_yaml_bytes: usize,
    pub project_version: SceneUnityVersion,
    /// Exportable source-object collections, processed Scenes, and processed Prefabs written.
    pub collection_count: usize,
    /// Objects whose baseline collection writes nothing, with the reason.
    pub skipped_objects: Vec<ProjectExportNotice>,
    /// Files written through the generic asset-family renderer instead of their own.
    pub fallbacks: Vec<ProjectExportNotice>,
    /// Scene/Prefab processing diagnostics that did not prevent generic source-object export.
    pub processing_warnings: Vec<String>,
}

/// One object's export route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObjectRoute {
    /// `AssetExportCollection` and its subclasses: unique asset path plus `.meta`.
    Asset,
    /// `ManagerExportCollection`/`EditorBuildSettingsExportCollection`: fixed path, no `.meta`.
    Manager,
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
    facts: ObjectClassFacts,
    selection: super::project_exporter::CollectionSelection,
    route: ObjectRoute,
    collection_index: usize,
    guid: [u8; 16],
    file_id: i64,
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
    let (editor_version_text, project_version) = workspace_project_version(workspace)?;
    if objects.report().object_count > limits.max_objects {
        return Err(invalid_project(format!(
            "project export has {} objects, exceeding the limit {}",
            objects.report().object_count,
            limits.max_objects
        )));
    }
    if !reference_graph.is_complete() {
        return Err(invalid_project(
            "project export requires a complete workspace PPtr scan",
        ));
    }
    let settings_facts = ProjectExportSettings::from_export_settings(settings, project_version);
    let PreparedSceneCollections {
        processing: scene_processing,
        routed: mut scene_collections,
        member_keys: scene_member_keys,
        prefabs: mut prefab_collections,
        prefab_member_keys,
        processing_warnings,
        mut used_guids,
    } = prepare_scene_collections(
        filesystem,
        workspace,
        objects,
        reference_graph,
        project_version,
    )?;
    let mut next_guid_sequence = 0u64;

    // Pass 1: route every object through the registration stack. No file is planned yet, so a
    // mis-routed or unhandled object aborts with nothing written.
    let mut routed = Vec::new();
    routed
        .try_reserve_exact(objects.report().object_count)
        .map_err(|_| invalid_project("could not allocate routed project objects"))?;
    let mut notices: Vec<ProjectExportNotice> = Vec::new();
    let mut fallback_notices: Vec<ProjectExportNotice> = Vec::new();
    let mut next_collection_index = 0usize;
    for (file_index, file) in workspace.files().iter().enumerate() {
        for (object_index, object_info) in file.metadata.objects.iter().enumerate() {
            let source_key = source_object_key(file_index, object_index);
            if scene_member_keys.contains(&source_key) || prefab_member_keys.contains(&source_key) {
                continue;
            }
            let object = objects.get(file_index, object_index).ok_or_else(|| {
                invalid_project(format!(
                    "workspace object is missing at file {file_index}, object {object_index}"
                ))
            })?;
            if object_info.path_id == 0 {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} has PathID 0, which cannot identify a Unity YAML document"
                )));
            }
            let class_id = object.class_id.map(i32::from).ok_or_else(|| {
                invalid_project(format!(
                    "file {file_index} object {object_index} PathID {} has no class ID",
                    object_info.path_id
                ))
            })?;
            let payload = object.payload.as_ref().ok();
            if payload.is_some() {
                validate_decoded_object(object, object_info.path_id, reference_graph)?;
            }
            let facts = ObjectClassFacts::new(class_id, payload.is_none());
            let selection = select_collection(facts, settings_facts)?;
            let kind = selection.kind();
            let (route, skip_reason, fallback_reason) = match kind {
                ExportCollectionKind::Manager => (ObjectRoute::Manager, "", None),
                ExportCollectionKind::Unknown => (ObjectRoute::Unknown, "", None),
                ExportCollectionKind::Unreadable => (ObjectRoute::Unreadable, "", None),
                ExportCollectionKind::Asset => {
                    (ObjectRoute::Asset, "", asset_family_fallback(selection.collection_name))
                }
                ExportCollectionKind::Scene
                | ExportCollectionKind::Prefab
                | ExportCollectionKind::Script
                | ExportCollectionKind::EmptyScript => (
                    ObjectRoute::Asset,
                    "",
                    Some(
                        "the specialized collection renderer is not ported; the object was written \
                         through the generic asset family",
                    ),
                ),
                ExportCollectionKind::Fail => (
                    ObjectRoute::Skipped,
                    "FailExportCollection writes nothing and reports a failed export",
                    None,
                ),
                ExportCollectionKind::Empty => (
                    ObjectRoute::Skipped,
                    "EmptyExportCollection is not exportable; the baseline writes nothing",
                    None,
                ),
                ExportCollectionKind::Skip => (
                    ObjectRoute::Skipped,
                    "SkipExportCollection is not exportable; references become missing references",
                    None,
                ),
                ExportCollectionKind::Deleted => (
                    ObjectRoute::Skipped,
                    "DeletedAssetsExportCollection is not exportable",
                    None,
                ),
                ExportCollectionKind::Redirect => (
                    ObjectRoute::Skipped,
                    "RedirectExportCollection is not exportable",
                    None,
                ),
            };
            let label = if route == ObjectRoute::Skipped {
                format!("{} (class {class_id})", selection.collection_name)
            } else {
                selection.collection_name.to_owned()
            };
            if route == ObjectRoute::Skipped {
                notices.push(ProjectExportNotice {
                    subject: label.clone(),
                    reason: skip_reason,
                });
            }
            if let Some(reason) = fallback_reason {
                fallback_notices.push(ProjectExportNotice {
                    subject: label,
                    reason,
                });
            }
            let collection_index = if route == ObjectRoute::Skipped {
                usize::MAX
            } else {
                let index = next_collection_index;
                next_collection_index = next_collection_index
                    .checked_add(1)
                    .ok_or_else(|| invalid_project("project collection count overflows"))?;
                index
            };
            let class_name =
                object_class_name(class_id, payload.map(|payload| payload.type_name.as_str()));
            let named_name = payload.and_then(object_asset_name);
            routed.push(RoutedObject {
                object,
                class_id,
                class_name: class_name.clone(),
                named_name,
                facts,
                selection,
                route,
                collection_index,
                guid: mint_unique_collection_guid(&mut next_guid_sequence, &mut used_guids)?,
                file_id: object_info.path_id,
            });
        }
    }

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

    // Pass 2: build the project pointer map from exactly the objects that will be written, so a
    // reference to a skipped asset resolves to the baseline missing-reference pointer.
    let scene_assignment_count = scene_collections
        .iter()
        .try_fold(0usize, |count, scene| {
            count.checked_add(scene.assignments.len())
        })
        .ok_or_else(|| invalid_project("project pointer count overflows"))?;
    let pointer_capacity = routed
        .len()
        .checked_add(scene_assignment_count)
        .and_then(|count| {
            prefab_collections.iter().try_fold(count, |total, prefab| {
                total.checked_add(prefab.export_ids.registered_assets.len())
            })
        })
        .ok_or_else(|| invalid_project("project pointer count overflows"))?;
    let mut pointer_entries = Vec::new();
    pointer_entries
        .try_reserve_exact(pointer_capacity)
        .map_err(|_| invalid_project("could not allocate project export pointer entries"))?;
    let mut collection_metadata = Vec::new();
    collection_metadata
        .try_reserve_exact(next_collection_index)
        .map_err(|_| invalid_project("could not allocate project export collection metadata"))?;
    let mut collection_count = 0usize;
    for entry in &routed {
        if entry.route == ObjectRoute::Skipped {
            continue;
        }
        collection_count += 1;
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: entry.collection_index,
            guid: entry.guid,
            asset_type: UnityProjectAssetType::Serialized,
        });
        pointer_entries.push(ExportedAssetPointerEntry {
            asset: identity_of(entry),
            file_id: entry.file_id,
            collection_index: entry.collection_index,
        });
    }
    for scene in &scene_collections {
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
    for prefab in &prefab_collections {
        let source_pointer = generate_prefab_source_pointer(prefab.guid, prefab.representation)
            .map_err(|error| invalid_project(format!("Prefab pointer planning failed: {error}")))?;
        collection_count = collection_count
            .checked_add(1)
            .ok_or_else(|| invalid_project("project collection count overflows"))?;
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: prefab.collection_index,
            guid: prefab.guid,
            asset_type: source_pointer.asset_type,
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
    let pointer_map =
        ProjectExportPointerMap::new(&pointer_entries, &collection_metadata, limits.pointer_map)?;
    let streamed_resources = WorkspaceStreamedYamlResourceContext {
        filesystem,
        root_catalog,
        provider: resource_provider,
        content_limits: limits.streamed_content,
        staging_limits: StreamedYamlAssetLimits::default(),
    };

    // Pass 3: materialize every routed object into a staged file.
    let mut planner = ProjectPathPlanner::new(ProjectPathPlanner::host_platform());
    let mut files = Vec::new();
    files
        .try_reserve_exact(collection_count)
        .map_err(|_| invalid_project("could not allocate staged project asset files"))?;
    let mut total_bytes = 0usize;
    let mut exported_object_count = 0usize;
    let mut raw_file_cache: HashMap<usize, Vec<u8>> = HashMap::new();
    for entry in &routed {
        if entry.route == ObjectRoute::Skipped {
            continue;
        }
        let bytes = match entry.route {
            ObjectRoute::Unknown | ObjectRoute::Unreadable => raw_payload_bytes(
                filesystem,
                workspace,
                entry.object,
                &mut raw_file_cache,
                limits,
            )?,
            _ => {
                let source = identity_of(entry);
                let document = workspace_object_to_unity_yaml_document_with_streamed_resources(
                    workspace,
                    reference_graph,
                    &pointer_map,
                    entry.object,
                    source,
                    entry.file_id,
                    entry.collection_index,
                    None,
                    limits.yaml_collection.object_conversion,
                    &streamed_resources,
                )?;
                let Some(document) = document else {
                    notices.push(ProjectExportNotice {
                        subject: entry.selection.collection_name.to_owned(),
                        reason:
                            "the baseline exporter skipped an image texture with no usable data",
                    });
                    continue;
                };
                write_unity_yaml_documents(&[document], limits.yaml_write)?.into_bytes()
            }
        };
        exported_object_count = exported_object_count
            .checked_add(1)
            .ok_or_else(|| invalid_project("exported object count overflows"))?;
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid_project("project output byte count overflows"))?;
        if total_bytes > limits.max_total_yaml_bytes {
            return Err(invalid_project(format!(
                "project output uses {total_bytes} bytes, exceeding the limit {}",
                limits.max_total_yaml_bytes
            )));
        }
        let mut identity = super::asset_path::ExportAssetPathIdentity {
            class_name: entry.class_name.clone(),
            named_name: entry.named_name.clone(),
            ..Default::default()
        };
        if identity.class_name.is_empty() {
            identity.class_name = entry.class_name.clone();
        }
        let relative_path = match entry.route {
            ObjectRoute::Asset => {
                let extension = export_extension_for(entry.facts);
                let extension = if extension.is_empty() {
                    DEFAULT_EXPORT_EXTENSION
                } else {
                    extension
                };
                planner.plan_asset_path(&identity, extension)?
            }
            ObjectRoute::Manager => planner.plan_manager_path(&entry.class_name)?,
            ObjectRoute::Unknown => planner.plan_unknown_path(
                &entry.class_name,
                entry.named_name.as_deref().unwrap_or(&entry.class_name),
            )?,
            ObjectRoute::Unreadable => planner.plan_unreadable_path(
                &entry.class_name,
                entry.named_name.as_deref().unwrap_or(&entry.class_name),
            )?,
            ObjectRoute::Skipped => unreachable!("skipped routes are filtered above"),
        };
        let meta = match entry.route {
            ObjectRoute::Asset => Some(PlannedProjectMeta {
                importer_class: "NativeFormatImporter".to_owned(),
                importer: native_format_importer(entry.file_id)?,
            }),
            _ => None,
        };
        files.push(PlannedProjectFile {
            collection_name: entry.selection.collection_name,
            kind: entry.selection.kind(),
            relative_path,
            guid: entry.guid,
            bytes,
            meta,
            label: entry.selection.collection_name.to_owned(),
        });
    }

    for prefab in &prefab_collections {
        let yaml = workspace_prefab_hierarchy_to_unity_yaml_with_generated_transforms(
            workspace,
            objects,
            reference_graph,
            &pointer_map,
            &prefab.export_ids,
            prefab.collection_index,
            None,
            limits.yaml_collection,
            &WorkspaceGeneratedTransformYamlContext::with_materialized_prefab_transforms(
                &scene_processing.processed_assets,
                &scene_processing.generated_transforms,
            ),
        )?;
        exported_object_count = exported_object_count
            .checked_add(count_unity_yaml_documents(&yaml))
            .ok_or_else(|| invalid_project("exported object count overflows"))?;
        total_bytes = total_bytes
            .checked_add(yaml.len())
            .ok_or_else(|| invalid_project("project output byte count overflows"))?;
        if total_bytes > limits.max_total_yaml_bytes {
            return Err(invalid_project(format!(
                "project output uses {total_bytes} bytes, exceeding the limit {}",
                limits.max_total_yaml_bytes
            )));
        }

        let relative_path = planner.plan_asset_path(
            &super::asset_path::ExportAssetPathIdentity {
                class_name: prefab.class_name.clone(),
                named_name: prefab.named_name.clone(),
                ..Default::default()
            },
            "prefab",
        )?;
        let meta = if prefab.representation == PrefabAssetRepresentation::LegacyPrefabMarker {
            let main_file_id = prefab
                .export_ids
                .registered_assets
                .iter()
                .find(|assignment| assignment.is_main_asset)
                .map(|assignment| assignment.file_id)
                .ok_or_else(|| invalid_project("Prefab export plan has no main asset ID"))?;
            Some(PlannedProjectMeta {
                importer_class: "NativeFormatImporter".to_owned(),
                importer: native_format_importer(main_file_id)?,
            })
        } else {
            Some(PlannedProjectMeta {
                importer_class: "PrefabImporter".to_owned(),
                importer: prefab_importer(prefab.asset_bundle_name.as_deref())?,
            })
        };
        files.push(PlannedProjectFile {
            collection_name: "PrefabExportCollection",
            kind: ExportCollectionKind::Prefab,
            relative_path,
            guid: prefab.guid,
            bytes: yaml.into_bytes(),
            meta,
            label: prefab.name.clone(),
        });
    }

    let mut scene_paths_seen: HashSet<String> = HashSet::new();
    for scene in &mut scene_collections {
        let path_plan =
            plan_scene_export_paths(&scene.path, ProjectPathPlanner::host_platform(), "")?;
        if scene_paths_seen.contains(&path_plan.relative_path) {
            if scene_build_settings_entry_is_duplicate(&scene_processing, scene.path_source) {
                notices.push(ProjectExportNotice {
                    subject: format!("{} ({})", scene.name, path_plan.relative_path),
                    reason: "a previous duplicate BuildSettings scene already wrote this path",
                });
                continue;
            }
            return Err(invalid_project(format!(
                "scene {:?} resolves to duplicate output path {:?} without a duplicate BuildSettings entry",
                scene.name, path_plan.relative_path
            )));
        }
        scene_paths_seen.insert(path_plan.relative_path.clone());

        let generated_assignment_count = scene
            .assignments
            .iter()
            .filter(|assignment| {
                matches!(
                    assignment.asset.collection,
                    ExportAssetCollectionIdentity::ProcessedCollection { .. }
                )
            })
            .count();
        let generated_transforms: Vec<_> = scene_processing
            .generated_transforms
            .iter()
            .filter(|transform| {
                scene.assignments.iter().any(|assignment| {
                    matches!(
                        assignment.asset.collection,
                        ExportAssetCollectionIdentity::ProcessedCollection {
                            collection_index
                        } if collection_index == transform.asset.collection_index
                    ) && assignment.asset.path_id == transform.asset.path_id
                })
            })
            .collect();
        if generated_assignment_count != generated_transforms.len() {
            return Err(invalid_project(format!(
                "scene {:?} has {generated_assignment_count} generated assignments but {} materialized generated Transforms",
                scene.name,
                generated_transforms.len(),
            )));
        }
        let scene_yaml = if !generated_transforms.is_empty() {
            workspace_scene_assignments_to_unity_yaml_with_streamed_resources_and_generated_transforms(
                workspace,
                objects,
                reference_graph,
                &pointer_map,
                &scene.exportable_assignments,
                scene.collection_index,
                None,
                limits.yaml_collection,
                &streamed_resources,
                &WorkspaceGeneratedTransformYamlContext::with_materialized_scene_transforms(
                    &scene_processing.processed_assets,
                    scene.scene_index,
                    &scene_processing.generated_transforms,
                ),
            )?
        } else {
            workspace_scene_assignments_to_unity_yaml_with_streamed_resources(
                workspace,
                objects,
                reference_graph,
                &pointer_map,
                &scene.exportable_assignments,
                scene.collection_index,
                None,
                limits.yaml_collection,
                &streamed_resources,
            )?
        };
        exported_object_count = exported_object_count
            .checked_add(count_unity_yaml_documents(&scene_yaml))
            .ok_or_else(|| invalid_project("exported object count overflows"))?;
        let bytes = scene_yaml.into_bytes();
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid_project("project output byte count overflows"))?;
        if total_bytes > limits.max_total_yaml_bytes {
            return Err(invalid_project(format!(
                "project output uses {total_bytes} bytes, exceeding the limit {}",
                limits.max_total_yaml_bytes
            )));
        }
        files.push(PlannedProjectFile {
            collection_name: "SceneExportCollection",
            kind: ExportCollectionKind::Scene,
            relative_path: path_plan.relative_path,
            guid: scene.guid,
            bytes,
            meta: Some(PlannedProjectMeta {
                importer_class: "DefaultImporter".to_owned(),
                importer: default_importer(scene.asset_bundle_name.as_deref())?,
            }),
            label: scene.name.clone(),
        });
    }

    let manifest = package_manifest_json(project_version);
    let plan = ProjectExportPlan {
        project_version_text: editor_version_text.to_owned(),
        project_version_file: project_version_file(editor_version_text, project_version),
        package_manifest: manifest,
        files,
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
        meta_file_count: report.meta_file_count,
        total_yaml_bytes: total_bytes,
        project_version,
        collection_count,
        skipped_objects: report.skipped,
        fallbacks: report.fallbacks,
        processing_warnings,
    })
}

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
            .try_reserve(
                hierarchy
                    .managers
                    .len()
                    .saturating_add(usize::from(hierarchy.scene_roots.is_some())),
            )
            .map_err(|_| invalid_project("could not allocate Scene manager identity index"))?;
        for manager in &hierarchy.managers {
            non_exportable_scene_assets
                .insert(source_object_key(manager.file_index, manager.object_index));
        }
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
                if !unity_version_has_prefab_importer(project_version) {
                    return Err(invalid_project(format!(
                            "Unity {}.{} generated legacy Prefab payload serialization is not implemented",
                            project_version.major, project_version.minor
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
                    PrefabAssetRepresentation::PrefabInstanceMarker,
                    "PrefabInstance".to_owned(),
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

        if export_ids.emit_prefab_asset
            && matches!(
                prefab_reference,
                crate::processing::prefab_processing::WorkspaceProcessedPrefabReference::Generated(
                    _
                )
            )
        {
            return Err(invalid_project(
                "generated legacy Prefab serialization requires a versioned generated-object walker",
            ));
        }
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
        file.metadata
            .objects
            .iter()
            .any(|object| object.class_id == Some(1001))
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

fn prefab_importer(asset_bundle_name: Option<&str>) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
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

fn object_asset_bundle_name(payload: &ParsedObjectPayload) -> Option<String> {
    for field in ["AssetBundleName_R", "AssetBundleName", "m_AssetBundleName"] {
        if let Ok(Some(crate::serialized_file::object::SerializedValue::StringBytes(bytes))) =
            payload.root.unique_field_in_base_chain(field)
        {
            return Some(String::from_utf8_lossy(bytes).into_owned());
        }
    }
    None
}

/// The collection names whose generic YAML document is the baseline content.
fn asset_family_fallback(collection_name: &str) -> Option<&'static str> {
    match collection_name {
        "AssetExportCollection"
        | "ScriptableObjectExportCollection"
        | "ScriptableObjectGroupExportCollection"
        | "YamlStreamedAssetExportCollection" => None,
        "AnimatorControllerExportCollection" => Some(
            "the animator-controller renderer is not ported; the object was written through the \
             generic asset family",
        ),
        "AudioMixerExportCollection" => Some(
            "the audio-mixer renderer is not ported; the object was written through the generic \
             asset family",
        ),
        "TerrainYamlExportCollection" => Some(
            "the terrain renderer is not ported; the object was written through the generic asset \
             family",
        ),
        "ShaderExportCollection" => Some(
            "the shader exporter (dummy text, YAML mode, or decompiled source) is not wired into \
             the project path; the object was written through the generic asset family",
        ),
        _ => Some(
            "the specialized exporter for this collection is not wired into the project path; the \
             object was written through the generic asset family",
        ),
    }
}

fn raw_payload_bytes(
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    object: &WorkspaceObject,
    cache: &mut HashMap<usize, Vec<u8>>,
    limits: WorkspaceProjectExportLimits,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let file = workspace.files().get(object.file_index).ok_or_else(|| {
        invalid_project(format!(
            "raw payload needs workspace file {}",
            object.file_index
        ))
    })?;
    if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(object.file_index) {
        let bytes = filesystem.read_all_bytes(&file.virtual_path, limits.max_total_yaml_bytes)?;
        entry.insert(bytes);
    }
    let bytes = cache
        .get(&object.file_index)
        .expect("file bytes cached above");
    let start = usize::try_from(object.raw_payload_range.start)
        .map_err(|_| invalid_project("raw payload offset does not fit the host address space"))?;
    let end = usize::try_from(object.raw_payload_range.end)
        .map_err(|_| invalid_project("raw payload end does not fit the host address space"))?;
    let slice = bytes
        .get(start..end)
        .ok_or(ProjectExportError::MissingRawPayload {
            file_index: object.file_index,
            object_index: object.object_index,
        })?;
    Ok(slice.to_vec())
}

fn validate_decoded_object(
    object: &WorkspaceObject,
    path_id: i64,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<(), Box<dyn Error>> {
    if reference_graph.object_scan_is_complete(object.file_index, object.object_index) != Some(true)
    {
        return Err(invalid_project(format!(
            "file {} object {} PathID {path_id} has no complete PPtr scan",
            object.file_index, object.object_index
        )));
    }
    for reference in reference_graph.references_from_object(object.file_index, object.object_index)
    {
        match &reference.resolution {
            MultiFilePPtrResolution::Null | MultiFilePPtrResolution::Object { .. } => {}
            unresolved => {
                return Err(invalid_project(format!(
                    "file {} object {} PathID {path_id} has unresolved PPtr at {:?}: {unresolved:?}",
                    object.file_index, object.object_index, reference.field_path
                )));
            }
        }
    }
    Ok(())
}

fn new_collection_guid(sequence: u64) -> [u8; 16] {
    static NEXT_GUID: AtomicU64 = AtomicU64::new(0);
    let counter = NEXT_GUID.fetch_add(1, Ordering::Relaxed);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let identity = (sequence, counter, std::process::id(), time);
    let first = std::collections::hash_map::RandomState::new().hash_one((&identity, 0u8));
    let second = std::collections::hash_map::RandomState::new().hash_one((&identity, 1u8));
    let mut guid = [0u8; 16];
    guid[..8].copy_from_slice(&first.to_le_bytes());
    guid[8..].copy_from_slice(&second.to_le_bytes());
    if guid == [0; 16] {
        guid[0] = 1;
    }
    guid
}

fn invalid_project(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

#[cfg(test)]
mod tests {
    use super::{
        asset_family_fallback, export_workspace_yaml_project_with_settings, new_collection_guid,
        package_manifest_json, WorkspaceProjectExportLimits,
    };
    use crate::export::settings::ExportDefaults;
    use crate::export::settings_file::ExportSettingsFile;
    use crate::export::workspace_yaml::tests::{
        load_workspace_bytes, make_existing_prefab_instance_file,
        make_game_object_without_transform_file,
    };
    use crate::io::platform_structure::PlatformResourceProvider;
    use crate::processing::scene_naming::SceneUnityVersion;
    use crate::resource::RootResourceFileCatalog;
    use crate::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
    use crate::serialized_file::reference_graph::MultiFileReferenceGraphLimits;

    #[test]
    fn project_collection_guids_are_nonzero_and_unique_per_collection() {
        let first = new_collection_guid(0);
        let second = new_collection_guid(1);
        assert_ne!(first, [0; 16]);
        assert_ne!(second, [0; 16]);
        assert_ne!(first, second);
    }

    #[test]
    fn project_manifest_keeps_the_android_jni_version_gate() {
        let before = SceneUnityVersion::parse("2019.1.0f1").unwrap();
        let after = SceneUnityVersion::parse("2019.2.0f1").unwrap();
        assert!(!package_manifest_json(before).contains("com.unity.modules.androidjni"));
        assert!(package_manifest_json(after).contains("\"com.unity.modules.androidjni\""));
    }

    #[test]
    fn generic_yaml_families_do_not_report_a_fallback() {
        for name in [
            "AssetExportCollection",
            "ScriptableObjectExportCollection",
            "ScriptableObjectGroupExportCollection",
            "YamlStreamedAssetExportCollection",
        ] {
            assert!(asset_family_fallback(name).is_none(), "{name}");
        }
        for name in [
            "ShaderExportCollection",
            "TextAssetExportCollection",
            "AudioClipExportCollection",
            "TerrainYamlExportCollection",
        ] {
            assert!(asset_family_fallback(name).is_some(), "{name}");
        }
    }

    #[test]
    fn project_export_writes_a_processed_prefab_collection_with_generated_transform() {
        let bytes = make_game_object_without_transform_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(
            reference_graph.is_complete(),
            "reference graph is incomplete"
        );
        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root = crate::test_support::unique_test_directory("project-prefab-e2e");

        let report = export_workspace_yaml_project_with_settings(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &reference_graph,
            &settings,
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();

        let prefab_path = output_root.join("Assets/PrefabInstance/Root.prefab");
        let prefab_meta_path = output_root.join("Assets/PrefabInstance/Root.prefab.meta");
        let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
        let prefab_meta = std::fs::read_to_string(&prefab_meta_path).unwrap();
        let game_object_id = yaml_document_file_id(&prefab_yaml, 1);
        let transform_id = yaml_document_file_id(&prefab_yaml, 4);
        assert!(prefab_yaml.contains("m_Name: Root"), "{prefab_yaml}");
        assert!(prefab_yaml.contains("m_LocalPosition:"), "{prefab_yaml}");
        assert!(
            prefab_yaml.contains(&format!("component: {{fileID: {transform_id}}}")),
            "{prefab_yaml}"
        );
        assert!(
            prefab_yaml.contains(&format!("m_GameObject: {{fileID: {game_object_id}}}")),
            "{prefab_yaml}"
        );
        assert!(!prefab_yaml.contains("!u!1001"), "{prefab_yaml}");
        assert!(prefab_meta.contains("PrefabImporter:"), "{prefab_meta}");
        assert!(prefab_meta.contains("externalObjects: {}"), "{prefab_meta}");
        assert!(prefab_meta.contains("userData:\n"), "{prefab_meta}");
        assert!(prefab_meta.contains("assetBundleName:\n"), "{prefab_meta}");
        assert!(
            prefab_meta.contains("assetBundleVariant:\n"),
            "{prefab_meta}"
        );
        assert!(!output_root.join("Assets/GameObject").exists());
        assert_eq!(report.asset_file_count, 1);
        assert_eq!(report.meta_file_count, 1);
        assert_eq!(report.collection_count, 1);
        assert_eq!(report.exported_object_count, 2);

        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn project_export_routes_an_existing_modern_prefab_instance_collection() {
        let bytes = make_existing_prefab_instance_file();
        if let Ok(path) = std::env::var("ASSET_RIPPER_REFERENCE_PREFAB_FIXTURE") {
            std::fs::write(path, &bytes).unwrap();
        }
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(
            objects
                .objects()
                .iter()
                .all(|object| object.payload.is_ok()),
            "undecoded Prefab fixture objects: {:?}",
            objects
                .objects()
                .iter()
                .filter_map(|object| object.payload.as_ref().err())
                .collect::<Vec<_>>()
        );
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(
            reference_graph.is_complete(),
            "reference graph is incomplete"
        );
        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root = crate::test_support::unique_test_directory("project-existing-prefab-e2e");

        let report = export_workspace_yaml_project_with_settings(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &reference_graph,
            &settings,
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();

        let prefab_path = output_root.join("Assets/PrefabInstance/Root.prefab");
        let prefab_meta_path = output_root.join("Assets/PrefabInstance/Root.prefab.meta");
        let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
        let prefab_meta = std::fs::read_to_string(&prefab_meta_path).unwrap();
        let game_object_id = yaml_document_file_id(&prefab_yaml, 1);
        let transform_id = yaml_document_file_id(&prefab_yaml, 4);
        assert!(prefab_yaml.contains("m_Name: Root"), "{prefab_yaml}");
        assert!(
            prefab_yaml.contains(&format!("component: {{fileID: {transform_id}}}")),
            "{prefab_yaml}"
        );
        assert!(
            prefab_yaml.contains(&format!("m_GameObject: {{fileID: {game_object_id}}}")),
            "{prefab_yaml}"
        );
        assert!(!prefab_yaml.contains("!u!1001"), "{prefab_yaml}");
        assert!(prefab_meta.contains("PrefabImporter:"), "{prefab_meta}");
        assert_eq!(report.asset_file_count, 1);
        assert_eq!(report.meta_file_count, 1);
        assert_eq!(report.collection_count, 1);
        assert_eq!(report.exported_object_count, 2);
        assert!(report.processing_warnings.is_empty());

        let source_rows = include_str!("../processing/prefab_instance_runtime_fixture.txt");
        assert!(source_rows.lines().any(|line| {
            line == "PREFAB-HIER|root=10:Root|prefab=12|gameObjects=GameObject_2018_3#10:Root|components=Transform_2018_3#11:null|prefabInstances=|exportable=GameObject_2018_3#10:Root,Transform_2018_3#11:null|hidden=PrefabInstance_2018_3#12:null"
        }));

        std::fs::remove_dir_all(output_root).unwrap();
    }

    fn yaml_document_file_id(yaml: &str, class_id: i32) -> String {
        let prefix = format!("--- !u!{class_id} &");
        yaml.lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .and_then(|tail| tail.split_whitespace().next())
            .expect("the expected Unity YAML document exists")
            .to_owned()
    }
}
