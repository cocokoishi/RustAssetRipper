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
//! Scene/Prefab collection reconstruction (which would attach `SceneHierarchyObject`/
//! `PrefabHierarchyObject` main assets and group objects into scene collections) is a separate
//! required integration owned by the scene workstream; until it is connected, scene and prefab
//! objects take the plain asset-family route, exactly like the baseline's unprocessed case.

use std::collections::HashMap;
use std::error::Error;
use std::hash::BuildHasher;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::PlatformResourceProvider;
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
use super::project_exporter::{
    execute_project_export, export_extension_for, native_format_importer, object_class_name,
    package_manifest_json, project_version_file, select_collection, ObjectClassFacts,
    PlannedProjectFile, PlannedProjectMeta, ProjectExportError, ProjectExportNotice,
    ProjectExportPlan, ProjectExportSettings, ProjectPathPlanner,
};
use super::scene_collection::{ExportAssetCollectionIdentity, ExportAssetIdentity};
use super::settings_file::ExportSettingsFile;
use super::streamed_assets::StreamedYamlAssetLimits;
use super::unity_yaml::{write_unity_yaml_documents, UnityYamlWriteLimits};
use super::workspace_yaml::{
    workspace_object_to_unity_yaml_document_with_streamed_resources,
    UnityYamlWorkspaceCollectionLimits, WorkspaceStreamedYamlResourceContext,
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
    /// Objects that claimed an exportable collection (one collection per object today).
    pub collection_count: usize,
    /// Objects whose baseline collection writes nothing, with the reason.
    pub skipped_objects: Vec<ProjectExportNotice>,
    /// Files written through the generic asset-family renderer instead of their own.
    pub fallbacks: Vec<ProjectExportNotice>,
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
                guid: new_collection_guid(collection_index as u64),
                file_id: object_info.path_id,
            });
        }
    }

    // Pass 2: build the project pointer map from exactly the objects that will be written, so a
    // reference to a skipped asset resolves to the baseline missing-reference pointer.
    let mut pointer_entries = Vec::new();
    pointer_entries
        .try_reserve_exact(routed.len())
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
        exported_object_count: report.file_count,
        asset_file_count: report.asset_file_count,
        meta_file_count: report.meta_file_count,
        total_yaml_bytes: total_bytes,
        project_version,
        collection_count,
        skipped_objects: report.skipped,
        fallbacks: report.fallbacks,
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
    use super::{asset_family_fallback, new_collection_guid, package_manifest_json};
    use crate::processing::scene_naming::SceneUnityVersion;

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
}
