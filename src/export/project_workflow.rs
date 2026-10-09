//! Executable Unity project export composed from collection dispatch, staged planning, and
//! the `ExportHandler.Export` write sequence.
//!
//! This is the product-level project path: every decoded workspace object is routed through the
//! frozen `ProjectExporter.CreateCollection` stack walk ([`crate::export::project_exporter`]),
//! every route is materialized into a staged [`ProjectExportPlan`], and only then does
//! `execute_project_export` touch the disk. Objects whose baseline collection is not exportable
//! are reported as explicit notices; families whose specialized renderer is not ported fall back
//! to the generic Unity YAML asset family instead of being dropped. TextAssets with non-empty
//! `m_Script` bytes use their binary exporter, settings-based extension, and TextScriptImporter.
//!
//! Scene definitions are processed into workspace hierarchies and emitted as `.unity` files
//! through the scene collection route, with scene IDs, pointers, and `DefaultImporter` metadata.
//! Modern PrefabProcessor hierarchies are emitted as `.prefab` collections with planned IDs,
//! generated Transforms, pointer mapping, and `PrefabImporter` metadata. A source-matched legacy
//! generated Prefab route covers a synthetic Unity 2017.4.40f1 hierarchy; other legacy branches,
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
use crate::processing::asset_factory::{FactoryAssetKind, FactoryUnityVersion};
use crate::processing::managed_assembly::mono_script_export_identity;
use crate::processing::scene_collections::{
    SceneDefinitionGuidPlan, SceneDefinitionPathSource, SceneGuidWords,
};
use crate::processing::scene_naming::SceneUnityVersion;
use crate::resource::{RootResourceFileCatalog, StreamedResourceContentLimits};
use crate::resources::shader::{script_has_decompiled_text, ShaderView, SHADER_CLASS_ID};
use crate::serialized_file::object::ParsedObjectPayload;
use crate::serialized_file::object_workspace::{ObjectWorkspace, WorkspaceObject};
use crate::serialized_file::reference_graph::{MultiFilePPtrResolution, MultiFileReferenceGraph};
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::collection_kind::ExportCollectionKind;
use super::collection_paths::{
    plan_script_export_paths, remove_assembly_file_extension, scripts_folder_name,
};
use super::empty_script::{empty_script_content, is_predefined_assembly, MonoScriptIdentity};
use super::export_extension::DEFAULT_EXPORT_EXTENSION;
use super::pointer_map::{
    ExportCollectionPointerMetadata, ExportPointerMapLimits, ExportedAssetPointerEntry,
    ProjectExportPointerMap, ProjectPointerRequest, ProjectPointerResolution,
    UnityProjectAssetType,
};
use super::prefab_collection::{
    generate_prefab_source_pointer, plan_prefab_export_ids, PrefabAssetRepresentation,
    PrefabExportIdPlan, PrefabExportPlanLimits, PrefabHierarchyAssetGroups, VersionedPrefabAsset,
};
use super::project_exporter::{
    execute_project_export, export_extension_for, native_format_importer_for_version,
    object_class_name, package_manifest_json, project_version_file,
    select_collection_with_text_asset_validity,
    shader_importer_for_version_with_non_modifiable_textures, ObjectClassFacts, PlannedProjectFile,
    PlannedProjectMeta, PlannedProjectSupportFile, ProjectExportError, ProjectExportNotice,
    ProjectExportPlan, ProjectExportSettings, ProjectPathPlanner,
};
use super::project_plan::plan_scene_export_paths;
use super::reference_assemblies::ReferenceAssemblyMaps;
use super::resource_extensions::text_export_extension;
use super::scene_collection::{
    plan_workspace_scene_export_ids_with_processed_assets, ExportAssetCollectionIdentity,
    ExportAssetIdentity, ExportAssetObjectKey, SceneExportIdAssignment, SceneExportIdPlanLimits,
    SceneHierarchyAssetOrderLimits,
};
use super::scene_wiring::{
    process_workspace_scenes_and_prefabs, WorkspaceSceneProcessing, WorkspaceSceneProcessingLimits,
};
use super::script_export_flow::{
    assembly_definition_file_name, assembly_definition_json, assembly_export_type,
    emit_assembly_definitions, AssemblyExportType,
};
use super::script_hashing::{
    calculate_script_file_id_str, calculate_script_guid_str, ScriptHashLimits,
};
use super::settings_file::ExportSettingsFile;
use super::shader_export::{
    export_shader_text, unity_patches, yaml_shader_patch_texts, yaml_shader_patches,
    YAML_SHADER_EXTENSION,
};
use super::streamed_assets::StreamedYamlAssetLimits;
use super::unity_yaml::{
    project_pointer_to_unity_yaml, write_unity_yaml_documents, UnityYamlMappingStyle,
    UnityYamlNode, UnityYamlSequenceStyle, UnityYamlWriteLimits,
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
const MAX_PROJECT_ASSET_PAYLOAD_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// Resource and allocation bounds for one workspace project export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceProjectExportLimits {
    pub max_objects: usize,
    pub max_total_asset_payload_bytes: usize,
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
            max_total_asset_payload_bytes: MAX_PROJECT_ASSET_PAYLOAD_BYTES,
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
    pub support_file_count: usize,
    /// Asset files that received an importer sidecar.
    pub meta_file_count: usize,
    pub total_asset_payload_bytes: usize,
    pub project_version: SceneUnityVersion,
    /// Exportable source-object collections, processed Scenes, and processed Prefabs written.
    pub collection_count: usize,
    /// Objects whose baseline collection writes nothing, with the reason.
    pub skipped_objects: Vec<ProjectExportNotice>,
    /// Files written through the generic asset-family renderer instead of their own.
    pub fallbacks: Vec<ProjectExportNotice>,
    /// Unknown or unreadable objects preserved through the source raw-data exporter. Their
    /// payload bytes are retained, but their internal fields and references could not be read.
    pub opaque_fallbacks: Vec<ProjectExportNotice>,
    /// Scene/Prefab processing diagnostics that did not prevent generic source-object export.
    pub processing_warnings: Vec<String>,
}

/// One object's export route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObjectRoute {
    /// `AssetExportCollection` and its subclasses: unique asset path plus `.meta`.
    Asset,
    /// `TextAssetExportCollection`: raw `m_Script` bytes, mode-based extension, and TextScriptImporter.
    TextAsset,
    /// `ShaderExportCollection` using `DummyShaderTextExporter` or `SimpleShaderExporter`.
    ShaderText,
    /// `YamlShaderExportCollection` plus its version-gated Unity editor patches.
    ShaderYaml,
    /// `ManagerExportCollection`/`EditorBuildSettingsExportCollection`: fixed path, no `.meta`.
    Manager,
    /// `ScriptExportCollection`/`EmptyScriptExportCollection`: deterministic `.cs` and MonoImporter metadata.
    Script,
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
    shader_view: Option<ShaderView>,
    text_asset_export: Option<TextAssetExportDetails>,
    script_export: Option<ScriptExportDetails>,
    selection: super::project_exporter::CollectionSelection,
    route: ObjectRoute,
    collection_index: usize,
    /// GUID written into the collection's `.meta` file.
    guid: [u8; 16],
    /// GUID registered in the source project pointer map. A reference MonoScript uses the
    /// reference assembly GUID here while retaining its calculated script GUID in its `.meta`.
    pointer_guid: [u8; 16],
    file_id: i64,
}

#[derive(Clone, Debug)]
struct ScriptExportDetails {
    identity: MonoScriptIdentity,
    execution_order: i16,
    guid: [u8; 16],
    pointer_guid: [u8; 16],
    pointer_file_id: i64,
}

#[derive(Clone, Debug)]
struct TextAssetExportDetails {
    script: Vec<u8>,
    extension: String,
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
    export_workspace_yaml_project_with_script_manager(
        output_root,
        delete_existing,
        filesystem,
        root_catalog,
        resource_provider,
        workspace,
        objects,
        reference_graph,
        settings,
        false,
        limits,
    )
}

/// Execute project export with the source Mono manager availability fact supplied by import.
#[allow(clippy::too_many_arguments)]
pub fn export_workspace_yaml_project_with_script_manager(
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
    limits: WorkspaceProjectExportLimits,
) -> Result<WorkspaceProjectExportReport, Box<dyn Error>> {
    let (editor_version_text, project_version) = workspace_project_version(workspace)?;
    let has_mono_scripts = objects
        .objects()
        .iter()
        .any(|object| object.factory.class_id == 115);
    let reference_assembly_maps = if assembly_manager_is_set || !has_mono_scripts {
        None
    } else {
        Some(
            ReferenceAssemblyMaps::from_source_fixture().map_err(|error| {
                invalid_project(format!(
                    "source reference assembly map could not be loaded: {error}"
                ))
            })?,
        )
    };
    let settings_facts = ProjectExportSettings::from_export_settings(settings, project_version);
    if objects.report().object_count > limits.max_objects {
        return Err(invalid_project(format!(
            "project export has {} objects, exceeding the limit {}",
            objects.report().object_count,
            limits.max_objects
        )));
    }
    let opaque_objects = opaque_factory_object_keys(objects)?;
    let incomplete_objects = objects
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
            objects.report().undecoded_objects.saturating_sub(opaque_objects.len()),
            incomplete_objects.join("; ")
        )));
    }
    if !reference_graph.is_complete_except_opaque_object_payloads(|file_index, object_index| {
        opaque_objects.contains(&(file_index, object_index))
    }) {
        let incomplete_scans = (0..reference_graph.file_count())
            .filter_map(|file_index| {
                let graph = reference_graph.managed_graph_for_file(file_index)?;
                (!graph.is_complete()).then(|| {
                    let issues = graph.issues();
                    format!(
                        "{}: {} scan issue(s), first issue {:?}",
                        reference_graph.normalized_file_names()[file_index],
                        issues.len(),
                        issues.first()
                    )
                })
            })
            .take(4)
            .collect::<Vec<_>>();
        let unresolved_references = reference_graph
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
    let mut opaque_fallback_notices: Vec<ProjectExportNotice> = Vec::new();
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
            // Newer SerializedFile generations omit the legacy object-table class ID;
            // the source factory resolves it from the serialized type table.
            let class_id = object.factory.class_id;
            let payload = object.payload.as_ref().ok();
            if payload.is_some() && !opaque_objects.contains(&(file_index, object_index)) {
                validate_decoded_object(object, object_info.path_id, reference_graph)?;
            }
            let facts = match &object.factory.kind {
                FactoryAssetKind::TypeTreeObject => ObjectClassFacts::type_tree_object(class_id),
                FactoryAssetKind::Unknown => ObjectClassFacts::with_known(class_id, false, false),
                FactoryAssetKind::Unreadable => ObjectClassFacts::new(class_id, true),
                FactoryAssetKind::Generated(_) => {
                    ObjectClassFacts::new(class_id, payload.is_none())
                }
            };
            let shader_view = if class_id == SHADER_CLASS_ID {
                payload
                    .map(|payload| ShaderView::from_value(&payload.root))
                    .transpose()?
            } else {
                None
            };
            let shader_text = match shader_view.as_ref().and_then(|view| view.script.as_deref()) {
                None => crate::export::collection_claims::ShaderTextState::NoScript,
                Some(script) if script_has_decompiled_text(script) => {
                    crate::export::collection_claims::ShaderTextState::DecompiledSource
                }
                Some(_) => crate::export::collection_claims::ShaderTextState::CompiledOrEmpty,
            };
            let text_asset_script = if class_id == 49 {
                payload.map(text_asset_script_bytes).transpose()?.flatten()
            } else {
                None
            };
            let selection = select_collection_with_text_asset_validity(
                facts,
                settings_facts,
                shader_text,
                text_asset_script
                    .as_ref()
                    .is_some_and(|script| !script.is_empty()),
            )?;
            let text_asset_export = if selection.exporter == "TextAssetExporter" {
                let script = text_asset_script.ok_or_else(|| {
                    invalid_project("TextAssetExporter was selected without decoded m_Script bytes")
                })?;
                let text = String::from_utf8_lossy(&script);
                let extension = text_export_extension(None, settings.text_export_mode, &text);
                Some(TextAssetExportDetails { script, extension })
            } else {
                None
            };
            let kind = selection.kind();
            let script_export = if class_id == 115 {
                plan_script_export_details(
                    object,
                    settings,
                    assembly_manager_is_set,
                    editor_version_text,
                    reference_assembly_maps.as_ref(),
                )?
            } else {
                None
            };
            let is_opaque_fallback = opaque_objects.contains(&(file_index, object_index));
            let raw_fallback_kind_matches = match kind {
                ExportCollectionKind::Unknown => {
                    matches!(&object.factory.kind, FactoryAssetKind::Unknown)
                }
                ExportCollectionKind::Unreadable => {
                    matches!(&object.factory.kind, FactoryAssetKind::Unreadable)
                }
                _ => true,
            };
            if matches!(
                kind,
                ExportCollectionKind::Unknown | ExportCollectionKind::Unreadable
            ) && (!is_opaque_fallback || !raw_fallback_kind_matches)
            {
                return Err(invalid_project(format!(
                    "{} selected a raw fallback exporter, but the source factory did not retain an opaque UnknownObject or UnreadableObject payload",
                    selection.collection_name
                )));
            }
            let (route, skip_reason, fallback_reason) = match kind {
                ExportCollectionKind::Manager => (ObjectRoute::Manager, "", None),
                ExportCollectionKind::Unknown => (ObjectRoute::Unknown, "", None),
                ExportCollectionKind::Unreadable => (ObjectRoute::Unreadable, "", None),
                ExportCollectionKind::Asset if selection.exporter == "TextAssetExporter" => {
                    (ObjectRoute::TextAsset, "", None)
                }
                ExportCollectionKind::Asset if selection.exporter == "YamlShaderExporter" => {
                    (ObjectRoute::ShaderYaml, "", None)
                }
                ExportCollectionKind::Asset
                    if selection.exporter == "DummyShaderTextExporter"
                        || selection.exporter == "SimpleShaderExporter" =>
                {
                    (ObjectRoute::ShaderText, "", None)
                }
                ExportCollectionKind::Asset => {
                    (ObjectRoute::Asset, "", asset_family_fallback(selection.collection_name))
                }
                ExportCollectionKind::Script | ExportCollectionKind::EmptyScript
                    if script_export.is_some() => (ObjectRoute::Script, "", None),
                ExportCollectionKind::Script | ExportCollectionKind::EmptyScript
                    if class_id == 115 => (
                    ObjectRoute::Skipped,
                    "injected MonoScripts have no standalone script file in the source exporter",
                    None,
                ),
                ExportCollectionKind::Scene | ExportCollectionKind::Prefab => (
                    ObjectRoute::Asset,
                    "",
                    Some(
                        "the specialized collection renderer is not ported; the object was written \
                         through the generic asset family",
                    ),
                ),
                ExportCollectionKind::Script | ExportCollectionKind::EmptyScript => (
                    ObjectRoute::Skipped,
                    "a script export collection was selected without a MonoScript object",
                    None,
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
                    subject: label.clone(),
                    reason,
                });
            }
            if matches!(route, ObjectRoute::Unknown | ObjectRoute::Unreadable) {
                opaque_fallback_notices.push(ProjectExportNotice {
                    subject: label,
                    reason: "raw bytes are preserved, but this object has no decoded fields or reference scan",
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
            let (class_name, named_name) = if opaque_objects.contains(&(file_index, object_index)) {
                (
                    object.factory.class_name.to_string(),
                    Some(object.factory.name.clone()),
                )
            } else {
                (
                    object_class_name(class_id, payload.map(|payload| payload.type_name.as_str())),
                    payload.and_then(object_asset_name),
                )
            };
            let file_id = match route {
                ObjectRoute::Asset
                | ObjectRoute::TextAsset
                | ObjectRoute::ShaderText
                | ObjectRoute::ShaderYaml => crate::export::ids::get_main_export_id(class_id, 0)
                    .map_err(|error| {
                        invalid_project(format!(
                            "class {class_id} export file ID could not be planned: {error}"
                        ))
                    })?,
                ObjectRoute::Script => {
                    script_export
                        .as_ref()
                        .ok_or_else(|| invalid_project("MonoScript route has no export details"))?
                        .pointer_file_id
                }
                _ => object_info.path_id,
            };
            let guid = if let Some(script) = script_export.as_ref() {
                script.guid
            } else {
                mint_unique_collection_guid(&mut next_guid_sequence, &mut used_guids)?
            };
            let pointer_guid = script_export
                .as_ref()
                .map_or(guid, |script| script.pointer_guid);
            routed.push(RoutedObject {
                object,
                class_id,
                class_name: class_name.clone(),
                named_name,
                facts,
                shader_view,
                text_asset_export,
                script_export,
                selection,
                route,
                collection_index,
                guid,
                pointer_guid,
                file_id,
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
            asset_type: if entry.route == ObjectRoute::Script {
                UnityProjectAssetType::Meta
            } else {
                UnityProjectAssetType::Serialized
            },
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
    let mut support_files = Vec::new();
    let mut support_file_indexes = HashMap::new();
    let mut total_bytes = 0usize;
    let mut exported_object_count = 0usize;
    let mut raw_file_cache: HashMap<usize, Vec<u8>> = HashMap::new();
    for entry in &routed {
        if entry.route == ObjectRoute::Skipped {
            continue;
        }
        if entry.route == ObjectRoute::ShaderYaml {
            let patch_version = (
                project_version.major,
                project_version.minor,
                project_version.patch,
            );
            for (name, text) in yaml_shader_patch_texts(yaml_shader_patches(patch_version)) {
                let relative_path = format!(
                    "{}{}",
                    unity_patches::PATCHES_RELATIVE_DIRECTORY,
                    unity_patches::patch_file_name(name)
                );
                add_project_support_file(
                    &mut support_files,
                    &mut support_file_indexes,
                    relative_path,
                    text.as_bytes(),
                    files.len(),
                )?;
            }
        }
        let bytes = match entry.route {
            ObjectRoute::Unknown | ObjectRoute::Unreadable => raw_payload_bytes(
                filesystem,
                workspace,
                entry.object,
                &mut raw_file_cache,
                limits,
            )?,
            ObjectRoute::ShaderText => {
                let shader = entry.shader_view.as_ref().ok_or_else(|| {
                    invalid_project(format!(
                        "{} selected a text Shader exporter without a decoded Shader view",
                        entry.selection.exporter
                    ))
                })?;
                export_shader_text(shader, settings.shader_export_mode).map_err(|error| {
                    invalid_project(format!(
                        "file {} object {} PathID {} Shader text export failed: {error}",
                        entry.object.file_index, entry.object.object_index, entry.object.path_id
                    ))
                })?
            }
            ObjectRoute::Script => {
                let script = entry.script_export.as_ref().ok_or_else(|| {
                    invalid_project("MonoScript route has no validated script identity")
                })?;
                empty_script_content(&script.identity.namespace, &script.identity.class)
                    .map_err(|error| {
                        invalid_project(format!(
                            "MonoScript dummy source generation failed: {error}"
                        ))
                    })?
                    .into_bytes()
            }
            ObjectRoute::TextAsset => {
                let script = &entry
                    .text_asset_export
                    .as_ref()
                    .ok_or_else(|| invalid_project("TextAsset route has no script bytes"))?
                    .script;
                let mut output = Vec::new();
                output
                    .try_reserve_exact(script.len())
                    .map_err(|_| invalid_project("could not allocate TextAsset export bytes"))?;
                output.extend_from_slice(script);
                output
            }
            ObjectRoute::Asset | ObjectRoute::ShaderYaml | ObjectRoute::Manager => {
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
            ObjectRoute::Skipped => unreachable!("skipped routes are filtered above"),
        };
        exported_object_count = exported_object_count
            .checked_add(1)
            .ok_or_else(|| invalid_project("exported object count overflows"))?;
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid_project("project output byte count overflows"))?;
        if total_bytes > limits.max_total_asset_payload_bytes {
            return Err(invalid_project(format!(
                "project output uses {total_bytes} bytes, exceeding the limit {}",
                limits.max_total_asset_payload_bytes
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
            ObjectRoute::TextAsset => {
                let text_asset = entry
                    .text_asset_export
                    .as_ref()
                    .ok_or_else(|| invalid_project("TextAsset route has no planned extension"))?;
                planner.plan_asset_path(&identity, &text_asset.extension)?
            }
            ObjectRoute::ShaderText => planner.plan_asset_path(&identity, "shader")?,
            ObjectRoute::ShaderYaml => planner.plan_asset_path(&identity, YAML_SHADER_EXTENSION)?,
            ObjectRoute::Script => {
                let script = entry.script_export.as_ref().ok_or_else(|| {
                    invalid_project("MonoScript route has no validated script identity")
                })?;
                plan_script_export_paths(
                    &script.identity.assembly,
                    &script.identity.namespace,
                    &script.identity.class,
                    ProjectPathPlanner::host_platform(),
                    "",
                )?
                .relative_asset_path
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
            ObjectRoute::Asset | ObjectRoute::ShaderYaml => Some(PlannedProjectMeta {
                importer_class: "NativeFormatImporter".to_owned(),
                importer: native_format_importer_for_version(
                    entry.file_id,
                    project_version,
                    entry
                        .object
                        .payload
                        .as_ref()
                        .ok()
                        .and_then(object_asset_bundle_name)
                        .as_deref(),
                )?,
            }),
            ObjectRoute::TextAsset => {
                let asset_bundle_name = entry
                    .object
                    .payload
                    .as_ref()
                    .ok()
                    .and_then(object_asset_bundle_name);
                Some(PlannedProjectMeta {
                    importer_class: "TextScriptImporter".to_owned(),
                    importer: default_importer(asset_bundle_name.as_deref())?,
                })
            }
            ObjectRoute::Script => {
                let script = entry.script_export.as_ref().ok_or_else(|| {
                    invalid_project("MonoScript route has no validated script identity")
                })?;
                Some(PlannedProjectMeta {
                    importer_class: "MonoImporter".to_owned(),
                    importer: mono_importer_yaml(script.execution_order)?,
                })
            }
            ObjectRoute::ShaderText => {
                let shader = entry.shader_view.as_ref().ok_or_else(|| {
                    invalid_project("ShaderImporter requires a decoded Shader view")
                })?;
                let non_modifiable_textures = shader_non_modifiable_textures_yaml(
                    shader,
                    entry,
                    objects,
                    reference_graph,
                    &pointer_map,
                    project_version,
                )?;
                Some(PlannedProjectMeta {
                    importer_class: "ShaderImporter".to_owned(),
                    importer: shader_importer_for_version_with_non_modifiable_textures(
                        project_version,
                        entry
                            .object
                            .payload
                            .as_ref()
                            .ok()
                            .and_then(object_asset_bundle_name)
                            .as_deref(),
                        non_modifiable_textures,
                    )?,
                })
            }
            ObjectRoute::Manager | ObjectRoute::Unknown | ObjectRoute::Unreadable => None,
            ObjectRoute::Skipped => unreachable!("skipped routes are filtered above"),
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

    // `EmptyScriptExportCollection` writes an asmdef for each non-predefined assembly when no
    // managed assembly manager was initialized. Its details have no module, so the source JSON
    // contains an empty reference array.
    if !assembly_manager_is_set
        && emit_assembly_definitions(
            routed
                .iter()
                .any(|entry| entry.route == ObjectRoute::Script),
            project_version,
        )
    {
        let mut emitted_assemblies = HashSet::new();
        for entry in &routed {
            if entry.route != ObjectRoute::Script {
                continue;
            }
            let script = entry.script_export.as_ref().ok_or_else(|| {
                invalid_project("MonoScript route has no validated script identity")
            })?;
            let assembly = &script.identity.assembly;
            if is_predefined_assembly(assembly) || !emitted_assemblies.insert(assembly.clone()) {
                continue;
            }
            let assembly_folder = remove_assembly_file_extension(assembly);
            let relative_path = format!(
                "Assets/{}/{}/{}",
                scripts_folder_name(assembly_folder),
                assembly_folder,
                assembly_definition_file_name(assembly_folder)
            );
            let bytes = assembly_definition_json(assembly_folder, &[]).into_bytes();
            add_project_support_file(
                &mut support_files,
                &mut support_file_indexes,
                relative_path,
                &bytes,
                files.len(),
            )?;
        }
    }

    for prefab in &prefab_collections {
        let mut generated_transform_context =
            WorkspaceGeneratedTransformYamlContext::with_materialized_prefab_transforms(
                &scene_processing.processed_assets,
                &scene_processing.generated_transforms,
            );
        let generated_legacy_prefab = prefab.representation
            == PrefabAssetRepresentation::LegacyPrefabMarker
            && prefab
                .export_ids
                .registered_assets
                .iter()
                .any(|assignment| {
                    assignment.is_main_asset
                        && matches!(
                            assignment.asset.collection,
                            ExportAssetCollectionIdentity::ProcessedCollection { .. }
                        )
                });
        if generated_legacy_prefab {
            generated_transform_context = generated_transform_context
                .with_legacy_prefab_export(&prefab.export_ids, editor_version_text);
        }
        let yaml = workspace_prefab_hierarchy_to_unity_yaml_with_generated_transforms(
            workspace,
            objects,
            reference_graph,
            &pointer_map,
            &prefab.export_ids,
            prefab.collection_index,
            None,
            limits.yaml_collection,
            &generated_transform_context,
        )?;
        exported_object_count = exported_object_count
            .checked_add(count_unity_yaml_documents(&yaml))
            .ok_or_else(|| invalid_project("exported object count overflows"))?;
        total_bytes = total_bytes
            .checked_add(yaml.len())
            .ok_or_else(|| invalid_project("project output byte count overflows"))?;
        if total_bytes > limits.max_total_asset_payload_bytes {
            return Err(invalid_project(format!(
                "project output uses {total_bytes} bytes, exceeding the limit {}",
                limits.max_total_asset_payload_bytes
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
                importer: native_format_importer_for_version(
                    main_file_id,
                    project_version,
                    prefab.asset_bundle_name.as_deref(),
                )?,
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
        if total_bytes > limits.max_total_asset_payload_bytes {
            return Err(invalid_project(format!(
                "project output uses {total_bytes} bytes, exceeding the limit {}",
                limits.max_total_asset_payload_bytes
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
        support_files,
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
        meta_file_count: report.meta_file_count,
        total_asset_payload_bytes: total_bytes,
        project_version,
        collection_count,
        skipped_objects: report.skipped,
        fallbacks: report.fallbacks,
        opaque_fallbacks: opaque_fallback_notices,
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

fn plan_script_export_details(
    object: &WorkspaceObject,
    settings: &ExportSettingsFile,
    assembly_manager_is_set: bool,
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
    if assembly_manager_is_set && !is_predefined_assembly(&identity.assembly) {
        return Err(invalid_project(format!(
            "file {} object {} PathID {} MonoScript assembly {:?} requires manager-backed reference-map or assembly-export handling, which is not wired into the project path",
            object.file_index, object.object_index, object.path_id, identity.assembly
        )));
    }
    let reference_guid = if assembly_manager_is_set {
        None
    } else {
        let version = FactoryUnityVersion::parse(unity_version_text).ok_or_else(|| {
            invalid_project(format!(
                "file {} object {} PathID {} MonoScript reference lookup cannot parse Unity version {unity_version_text:?}",
                object.file_index, object.object_index, object.path_id
            ))
        })?;
        let maps = reference_assembly_maps.ok_or_else(|| {
            invalid_project("MonoScript reference lookup has no source reference assembly map")
        })?;
        maps.guid_for(&version, &identity.assembly)
            .map_err(|error| {
                invalid_project(format!(
                    "file {} object {} PathID {} MonoScript reference lookup failed: {error}",
                    object.file_index, object.object_index, object.path_id
                ))
            })?
    };
    let export_type = assembly_export_type(
        reference_guid.is_some(),
        assembly_manager_is_set,
        settings.script_export_mode,
        &identity.assembly,
    );
    if export_type != AssemblyExportType::Decompile
        && !(export_type == AssemblyExportType::Skip
            && !assembly_manager_is_set
            && reference_guid.is_some())
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
                invalid_project(format!("MonoScript export ID could not be planned: {error}"))
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
            return Err(invalid_project(format!(
                "file {} object {} PathID {} MonoScript selects Save for ScriptExportMode::{:?}; that script export route is not wired into the project path",
                object.file_index,
                object.object_index,
                object.path_id,
                settings.script_export_mode
            )))
        }
    };
    Ok(Some(ScriptExportDetails {
        identity,
        execution_order: metadata.execution_order,
        guid: script_guid.to_byte_array(),
        pointer_guid,
        pointer_file_id,
    }))
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

fn text_asset_script_bytes(
    payload: &ParsedObjectPayload,
) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
    let script = payload
        .root
        .unique_field_in_base_chain("m_Script")
        .map_err(|error| invalid_project(format!("TextAsset m_Script lookup failed: {error}")))?;
    let Some(crate::serialized_file::object::SerializedValue::StringBytes(bytes)) = script else {
        if script.is_none() {
            return Ok(None);
        }
        return Err(invalid_project(
            "TextAsset m_Script field is not a serialized string",
        ));
    };
    let mut output = Vec::new();
    output
        .try_reserve_exact(bytes.len())
        .map_err(|_| invalid_project("could not allocate TextAsset script bytes"))?;
    output.extend_from_slice(bytes);
    Ok(Some(output))
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

fn shader_non_modifiable_textures_yaml(
    shader: &ShaderView,
    entry: &RoutedObject<'_>,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    project_version: SceneUnityVersion,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let Some(textures) = shader.non_modifiable_textures.as_deref() else {
        return Ok(UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow));
    };
    if textures.is_empty() {
        return Ok(UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow));
    }
    let mut output = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for (index, texture) in textures.iter().enumerate() {
        let expected_path = format!("m_NonModifiableTextures[{index}].second");
        let mut matching = reference_graph
            .references_from_object(entry.object.file_index, entry.object.object_index)
            .filter(|reference| {
                reference.field_path == expected_path
                    && reference.file_id == texture.file_id
                    && reference.path_id == texture.path_id
            });
        let reference = matching.next().ok_or_else(|| {
            invalid_project(format!(
                "ShaderImporter texture key {:?} at index {index} has no matching PPtr scan edge at {expected_path:?}",
                texture.name
            ))
        })?;
        if matching.next().is_some() {
            return Err(invalid_project(format!(
                "ShaderImporter texture key {:?} at index {index} has duplicate PPtr scan edges",
                texture.name
            )));
        }

        let resolution = match &reference.resolution {
            MultiFilePPtrResolution::Null => ProjectPointerResolution::Null,
            MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index,
            } => {
                let target = objects
                    .get(*target_file_index, *target_object_index)
                    .ok_or_else(|| {
                        invalid_project(format!(
                            "ShaderImporter texture key {:?} resolves to absent workspace object {target_file_index}:{target_object_index}",
                            texture.name
                        ))
                    })?;
                let identity = ExportAssetIdentity {
                    collection: ExportAssetCollectionIdentity::SerializedFile {
                        file_index: target.file_index,
                    },
                    object_index: target.object_index,
                    path_id: target.path_id,
                    class_id: target.factory.class_id,
                };
                pointer_map.resolve(
                    ProjectPointerRequest::ResolvedAsset(identity),
                    entry.collection_index,
                )?
            }
            unresolved => {
                return Err(invalid_project(format!(
                    "ShaderImporter texture key {:?} at {expected_path:?} has unresolved input PPtr {:?}",
                    texture.name, unresolved
                )));
            }
        };
        let pointer = project_pointer_to_unity_yaml(
            &resolution,
            project_version,
            Some(crate::export::script_hashing::UnityGuid::MISSING_REFERENCE.to_byte_array()),
        )
        .map_err(|error| {
            invalid_project(format!(
                "ShaderImporter texture key {:?} PPtr could not be converted to project YAML: {error}",
                texture.name
            ))
        })?;
        let mut pair = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        pair.insert_field(texture.name.clone(), pointer)?;
        output.push(pair)?;
    }
    Ok(output)
}

fn add_project_support_file(
    files: &mut Vec<PlannedProjectSupportFile>,
    indexes: &mut HashMap<String, usize>,
    relative_path: String,
    bytes: &[u8],
    before_file_index: usize,
) -> Result<(), Box<dyn Error>> {
    if let Some(index) = indexes.get(&relative_path).copied() {
        let existing = files
            .get(index)
            .ok_or_else(|| invalid_project("Shader support-file index is inconsistent"))?;
        if existing.bytes != bytes {
            return Err(invalid_project(format!(
                "project exporters generated conflicting support file {relative_path:?}"
            )));
        }
        if before_file_index < existing.before_file_index {
            files[index].before_file_index = before_file_index;
        }
        return Ok(());
    }

    let mut owned_bytes = Vec::new();
    owned_bytes
        .try_reserve_exact(bytes.len())
        .map_err(|_| invalid_project("could not allocate Shader support file bytes"))?;
    owned_bytes.extend_from_slice(bytes);
    files
        .try_reserve(1)
        .map_err(|_| invalid_project("could not allocate Shader support-file list"))?;
    indexes
        .try_reserve(1)
        .map_err(|_| invalid_project("could not allocate Shader support-file index"))?;
    let index = files.len();
    indexes.insert(relative_path.clone(), index);
    files.push(PlannedProjectSupportFile {
        relative_path,
        bytes: owned_bytes,
        before_file_index,
    });
    Ok(())
}

/// The collection names whose generic YAML document is the baseline content.
fn asset_family_fallback(collection_name: &str) -> Option<&'static str> {
    match collection_name {
        "AssetExportCollection"
        | "ScriptableObjectExportCollection"
        | "ScriptableObjectGroupExportCollection"
        | "YamlStreamedAssetExportCollection"
        | "ShaderExportCollection" => None,
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
        let bytes =
            filesystem.read_all_bytes(&file.virtual_path, limits.max_total_asset_payload_bytes)?;
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
        load_workspace_bytes, load_workspace_bytes_with_source_factory,
        load_workspace_named_bytes_with_source_factory, make_existing_prefab_instance_file,
        make_game_object_without_transform_file,
    };
    use crate::io::dependency::DependencyFilePath;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::io::platform_structure::PlatformResourceProvider;
    use crate::processing::scene_naming::SceneUnityVersion;
    use crate::resource::RootResourceFileCatalog;
    use crate::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
    use crate::serialized_file::reference_graph::{
        MultiFilePPtrResolution, MultiFileReferenceGraphLimits,
    };
    use crate::serialized_file::workspace::{
        SerializedFileWorkspace, SerializedFileWorkspaceLimits,
    };

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
    fn baseline_yaml_and_shader_collections_do_not_report_a_generic_fallback() {
        for name in [
            "AssetExportCollection",
            "ScriptableObjectExportCollection",
            "ScriptableObjectGroupExportCollection",
            "YamlStreamedAssetExportCollection",
            "ShaderExportCollection",
        ] {
            assert!(asset_family_fallback(name).is_none(), "{name}");
        }
        for name in [
            "TextAssetExportCollection",
            "AudioClipExportCollection",
            "TerrainYamlExportCollection",
        ] {
            assert!(asset_family_fallback(name).is_some(), "{name}");
        }
    }

    #[test]
    fn project_export_keeps_source_managed_registry_cycles_and_cross_file_pointer() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        for file_name in ["managed.assets", "target.assets"] {
            let bytes = match file_name {
                "managed.assets" => {
                    include_bytes!("managed_reference_project_fixture/managed.assets").as_slice()
                }
                "target.assets" => {
                    include_bytes!("managed_reference_project_fixture/target.assets").as_slice()
                }
                _ => unreachable!(),
            };
            filesystem
                .write_all_bytes(&format!("/game/{file_name}"), bytes)
                .unwrap();
        }
        let inputs = [
            DependencyFilePath {
                file_name: "managed.assets".to_owned(),
                virtual_path: "/game/managed.assets".to_owned(),
            },
            DependencyFilePath {
                file_name: "target.assets".to_owned(),
                virtual_path: "/game/target.assets".to_owned(),
            },
        ];
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        assert_eq!(workspace.files().len(), 2);
        assert_eq!(
            workspace
                .files()
                .iter()
                .map(|file| file.metadata.objects.len())
                .sum::<usize>(),
            2
        );
        let root_file_index = workspace
            .files()
            .iter()
            .position(|file| file.file_name == "managed.assets")
            .unwrap();
        let target_file_index = workspace
            .files()
            .iter()
            .position(|file| file.file_name == "target.assets")
            .unwrap();

        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(
            objects.get(root_file_index, 0).unwrap().factory.class_id,
            114
        );
        assert_eq!(
            objects.get(target_file_index, 0).unwrap().factory.class_id,
            114
        );

        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let pptr = reference_graph
            .references()
            .iter()
            .find(|reference| {
                reference.source_file_index == root_file_index
                    && reference.source_path_id == 42
                    && reference.field_path == "references.RefIds[0].data.m_Asset"
            })
            .expect("NodeA's serialized PPtr is scanned from the managed payload");
        assert_eq!(pptr.file_id, 1);
        assert_eq!(pptr.path_id, 900);
        assert_eq!(
            pptr.resolution,
            MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index: 0,
            }
        );

        let managed_graph = reference_graph
            .managed_graph_for_file(root_file_index)
            .expect("managed graph exists for the source file");
        assert_eq!(managed_graph.managed_objects().len(), 2);
        assert_eq!(managed_graph.managed_objects()[0].reference_id, 100);
        assert_eq!(managed_graph.managed_objects()[0].class_name, "NodeA");
        assert_eq!(managed_graph.managed_objects()[1].reference_id, 200);
        assert_eq!(managed_graph.managed_objects()[1].class_name, "NodeB");
        assert_eq!(managed_graph.managed_references().len(), 3);
        assert!(managed_graph.managed_references().iter().all(|reference| matches!(
            reference.resolution,
            crate::serialized_file::reference_graph::SerializedManagedReferenceResolution::Object { .. }
        )));

        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root =
            crate::test_support::unique_test_directory("project-managed-reference-e2e");
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

        let root_path = output_root.join("Assets/MonoBehaviour/ManagedRoot.asset");
        let target_path = output_root.join("Assets/MonoBehaviour/ReferencedRoot.asset");
        let target_meta_path = output_root.join("Assets/MonoBehaviour/ReferencedRoot.asset.meta");
        let rust_root_yaml = std::fs::read_to_string(&root_path).unwrap();
        let source_root_yaml =
            include_str!("managed_reference_project_fixture/source_ManagedRoot.asset");
        let source_target_meta =
            include_str!("managed_reference_project_fixture/source_ReferencedRoot.asset.meta");
        let rust_target_meta = std::fs::read_to_string(&target_meta_path).unwrap();
        let guid_from_meta = |text: &str| -> String {
            text.lines()
                .find_map(|line| line.strip_prefix("guid: "))
                .expect("Unity .meta has a GUID")
                .to_owned()
        };
        let source_target_guid = guid_from_meta(source_target_meta);
        let rust_target_guid = guid_from_meta(&rust_target_meta);
        let normalized_source_yaml = source_root_yaml.replace(&source_target_guid, "<TARGET_GUID>");
        let normalized_rust_yaml = rust_root_yaml.replace(&rust_target_guid, "<TARGET_GUID>");
        assert_eq!(
            normalized_source_yaml.trim_end(),
            normalized_rust_yaml.trim_end(),
            "managed-reference MonoBehaviour YAML should match the source after GUID normalization"
        );
        let source_registry = source_root_yaml
            .split_once("  m_Root:\n")
            .expect("source YAML includes the managed-reference body")
            .1
            .replace(&source_target_guid, "<TARGET_GUID>");
        let rust_registry = rust_root_yaml
            .split_once("  m_Root:\n")
            .expect("Rust YAML includes the managed-reference body")
            .1
            .replace(&rust_target_guid, "<TARGET_GUID>");
        assert_eq!(source_registry.trim_end(), rust_registry.trim_end());
        assert!(rust_root_yaml.contains(&format!(
            "m_Asset: {{fileID: 11400000, guid: {rust_target_guid}, type: 2}}"
        )));
        assert!(rust_root_yaml.contains("        m_Link:\n          rid: 200\n"));
        assert!(rust_root_yaml.contains("        m_Link:\n          rid: 100\n"));
        let target_yaml = std::fs::read_to_string(target_path).unwrap();
        let source_target_yaml =
            include_str!("managed_reference_project_fixture/source_ReferencedRoot.asset");
        assert_eq!(
            source_target_yaml.trim_end(),
            target_yaml.trim_end(),
            "referenced MonoBehaviour YAML should match the source"
        );
        assert!(target_yaml.contains("m_Name: ReferencedRoot"));
        assert!(target_yaml.contains("m_Health: 789"));
        assert_eq!(report.asset_file_count, 2);
        assert_eq!(report.meta_file_count, 2);
        assert_eq!(report.exported_object_count, 2);
        assert!(report.processing_warnings.is_empty());
        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn project_export_writes_a_processed_prefab_collection_with_generated_transform() {
        let bytes = make_game_object_without_transform_file();
        let (filesystem, workspace) = load_workspace_bytes(&bytes);
        let objects = ObjectWorkspace::load_from_vfs_with_layout_source(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
            &crate::processing::asset_factory::FileTypeTreeTestLayouts,
        )
        .unwrap();
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
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

        let prefab_path = output_root.join("Assets/GameObject/Root.prefab");
        let prefab_meta_path = output_root.join("Assets/GameObject/Root.prefab.meta");
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
        assert!(!output_root.join("Assets/PrefabInstance").exists());
        assert_eq!(report.asset_file_count, 1);
        assert_eq!(report.meta_file_count, 1);
        assert_eq!(report.collection_count, 1);
        assert_eq!(report.exported_object_count, 2);

        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn project_export_matches_source_for_a_generated_legacy_prefab() {
        let bytes = include_bytes!("legacy_prefab_2017_4_input.unity");
        let (filesystem, workspace) = load_workspace_bytes_with_source_factory(bytes);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.objects().len(), 2);
        assert!(objects.objects().iter().all(|object| matches!(
            &object.factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(_)
        )));
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root = crate::test_support::unique_test_directory("project-legacy-prefab-e2e");

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

        let prefab_path = output_root.join("Assets/GameObject/LegacyRoot.prefab");
        let prefab_meta_path = output_root.join("Assets/GameObject/LegacyRoot.prefab.meta");
        let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
        let expected_yaml = include_str!("legacy_prefab_source_2017_4_fixture.yaml");
        assert_eq!(
            prefab_yaml.replace("\r\n", "\n"),
            expected_yaml.replace("\r\n", "\n")
        );
        let prefab_meta = std::fs::read_to_string(&prefab_meta_path).unwrap();
        assert!(
            prefab_meta.contains("NativeFormatImporter:"),
            "{prefab_meta}"
        );
        assert!(
            prefab_meta.contains("mainObjectFileID: 100100000"),
            "{prefab_meta}"
        );
        assert!(!prefab_meta.contains("PrefabImporter:"), "{prefab_meta}");
        assert_eq!(report.asset_file_count, 1);
        assert_eq!(report.meta_file_count, 1);
        assert_eq!(report.collection_count, 1);
        assert_eq!(report.exported_object_count, 3);
        assert!(report.processing_warnings.is_empty());

        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn project_export_matches_source_for_a_no_tree_generated_prefab_hierarchy() {
        let fixture = include_str!("no_tree_prefab_source_2019_4_input.hex");
        let hex = fixture
            .lines()
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .expect("source-authored input fixture has a hex payload");
        assert_eq!(hex.len() % 2, 0, "source fixture hex has an odd length");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("source fixture contains ASCII hexadecimal bytes")
            })
            .collect::<Vec<_>>();
        assert_eq!(bytes.len(), 360);

        let (filesystem, workspace) =
            load_workspace_named_bytes_with_source_factory("NoTreeRoot.unity", &bytes);
        let serialized_file = workspace.files().first().expect("one source file");
        assert_eq!(serialized_file.metadata.objects.len(), 2);
        assert!(serialized_file
            .metadata
            .types
            .iter()
            .all(|serialized_type| serialized_type
                .type_tree
                .as_ref()
                .is_none_or(|tree| tree.nodes.is_empty())));
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.objects().len(), 2);
        assert!(objects.objects().iter().all(|object| {
            matches!(
                &object.factory.kind,
                crate::processing::asset_factory::FactoryAssetKind::Generated(_)
            ) && object.recovered_type_tree.is_some()
                && object.payload.is_ok()
        }));
        assert_eq!(objects.get(0, 0).unwrap().factory.name, "NoTreeRoot");

        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        assert!(reference_graph.references().iter().any(|reference| {
            reference.source_path_id == 10
                && reference.path_id == 11
                && reference.field_path == "m_Component[0].component"
                && matches!(
                    reference.resolution,
                    MultiFilePPtrResolution::Object {
                        target_file_index: 0,
                        target_object_index: 1,
                    }
                )
        }));
        assert!(reference_graph.references().iter().any(|reference| {
            reference.source_path_id == 11
                && reference.path_id == 10
                && reference.field_path == "m_GameObject"
                && matches!(
                    reference.resolution,
                    MultiFilePPtrResolution::Object {
                        target_file_index: 0,
                        target_object_index: 0,
                    }
                )
        }));

        let root_catalog = RootResourceFileCatalog::default();
        let provider = PlatformResourceProvider::new(None, None);
        let settings = ExportSettingsFile::from(ExportDefaults::default());
        let output_root = crate::test_support::unique_test_directory("project-no-tree-prefab-e2e");
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

        let prefab_path = output_root.join("Assets/GameObject/NoTreeRoot.prefab");
        let prefab_meta_path = output_root.join("Assets/GameObject/NoTreeRoot.prefab.meta");
        let prefab_yaml = std::fs::read_to_string(&prefab_path).unwrap();
        let source_yaml = include_str!("no_tree_prefab_source_2019_4.yaml");
        assert_eq!(
            prefab_yaml.replace("\r\n", "\n"),
            source_yaml.replace("\r\n", "\n")
        );
        assert!(!output_root.join("Assets/PrefabInstance").exists());
        let prefab_meta = std::fs::read_to_string(prefab_meta_path).unwrap();
        let source_meta = include_str!("no_tree_prefab_source_2019_4.meta.yaml");
        let normalize_meta = |text: &str| {
            text.lines()
                .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(normalize_meta(&prefab_meta), normalize_meta(source_meta));
        assert_eq!(report.asset_file_count, 1);
        assert_eq!(report.meta_file_count, 1);
        assert_eq!(report.collection_count, 1);
        assert_eq!(report.exported_object_count, 2);
        assert!(report.processing_warnings.is_empty());

        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn project_export_routes_an_existing_modern_prefab_instance_collection() {
        let bytes = make_existing_prefab_instance_file();
        if let Ok(path) = std::env::var("ASSET_RIPPER_REFERENCE_PREFAB_FIXTURE") {
            std::fs::write(path, &bytes).unwrap();
        }
        let (filesystem, workspace) = load_workspace_bytes_with_source_factory(&bytes);
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
        assert_eq!(objects.objects().len(), 3);
        assert_eq!(
            objects.get(0, 0).unwrap().factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(
                "GameObject_2018_3".to_owned()
            )
        );
        assert_eq!(objects.get(0, 0).unwrap().factory.name, "Root");
        assert_eq!(
            objects.get(0, 1).unwrap().factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(
                "Transform_2018_3".to_owned()
            )
        );
        assert_eq!(
            objects.get(0, 2).unwrap().factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(
                "PrefabInstance_2018_3".to_owned()
            )
        );
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(
            reference_graph.is_complete(),
            "reference graph is incomplete"
        );
        let resolves_to = |source_path_id, target_path_id| {
            reference_graph.references().iter().any(|reference| {
                reference.source_path_id == source_path_id
                    && reference.path_id == target_path_id
                    && reference.file_id == 0
                    && matches!(
                        reference.resolution,
                        MultiFilePPtrResolution::Object {
                            target_file_index: 0,
                            ..
                        }
                    )
            })
        };
        assert!(
            resolves_to(10, 11),
            "GameObject component PPtr was not retained"
        );
        assert!(
            resolves_to(11, 10),
            "Transform-to-GameObject PPtr was not retained"
        );
        assert!(
            resolves_to(12, 10),
            "PrefabInstance root PPtr was not retained"
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
