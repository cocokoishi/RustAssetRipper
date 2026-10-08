//! Executable YAML project workflow composed from workspace loading, reference
//! mapping, resource-aware Unity YAML, importer metadata, and post-export files.
//!
//! This is the first project-level execution path. It writes one multi-document
//! `.asset` file per input SerializedFile. Scene/Prefab collection reconstruction,
//! generated objects, class-specific binary exporters, and platform post-export
//! outputs remain separate required integrations; this workflow refuses undecoded,
//! unresolved, or omitted objects instead of reporting a partial export as complete.

use std::collections::hash_map::RandomState;
use std::error::Error;
use std::fs;
use std::hash::BuildHasher;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::export::export_root::prepare_export_root;
use crate::export::meta::UnityMetaFileOptions;
use crate::export::pointer_map::{
    ExportCollectionPointerMetadata, ExportPointerMapLimits, ExportedAssetPointerEntry,
    ProjectExportPointerMap, UnityProjectAssetType,
};
use crate::export::post_export::{default_package_dependencies, project_version_file_contents};
use crate::export::project_output::{
    export_asset_with_meta, write_new_project_file, AssetExportWithMetaPlan,
    UnityProjectOutputLimits,
};
use crate::export::scene_collection::{
    ExportAssetCollectionIdentity, ExportAssetIdentity, SceneExportIdAssignment,
};
use crate::export::unity_yaml::{
    UnityYamlMappingStyle, UnityYamlNode, UnityYamlScalar, UnityYamlWriteLimits,
};
use crate::export::workspace_yaml::{
    workspace_assignments_to_unity_yaml_with_streamed_resources,
    UnityYamlWorkspaceCollectionLimits, WorkspaceStreamedYamlResourceContext,
};
use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::PlatformResourceProvider;
use crate::processing::scene_naming::SceneUnityVersion;
use crate::resource::{RootResourceFileCatalog, StreamedResourceContentLimits};
use crate::serialized_file::object_workspace::{ObjectWorkspace, WorkspaceObject};
use crate::serialized_file::reference_graph::{MultiFilePPtrResolution, MultiFileReferenceGraph};
use crate::serialized_file::workspace::SerializedFileWorkspace;

const MAX_PROJECT_OBJECTS: usize = 20_000_000;
const MAX_PROJECT_YAML_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// Resource and allocation bounds for one workspace project export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceProjectExportLimits {
    pub max_objects: usize,
    pub max_total_yaml_bytes: usize,
    pub pointer_map: ExportPointerMapLimits,
    pub yaml_collection: UnityYamlWorkspaceCollectionLimits,
    pub project_output: UnityProjectOutputLimits,
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
            project_output: UnityProjectOutputLimits::default(),
            yaml_write: UnityYamlWriteLimits::default(),
            streamed_content: StreamedResourceContentLimits::default(),
        }
    }
}

/// Summary of the files written by the current workspace YAML project path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceProjectExportReport {
    pub serialized_file_count: usize,
    pub exported_object_count: usize,
    pub asset_file_count: usize,
    pub total_yaml_bytes: usize,
    pub project_version: SceneUnityVersion,
}

struct PlannedSerializedFileAsset {
    relative_path: String,
    guid: [u8; 16],
    main_file_id: i64,
    yaml: String,
}

/// Execute a workspace-backed Unity YAML project export.
///
/// Every parsed object must decode, have a complete PPtr scan, resolve its references,
/// and produce exactly one YAML document. The complete output is staged in memory before
/// the first filesystem write, so unsupported or malformed objects do not leave a project
/// that looks successful. Output paths are deterministic; collection GUIDs follow the
/// source's random-per-collection policy.
///
/// The export root is validated and prepared exactly like the reference GUI export
/// guard: the path must not be empty or a system directory, and an existing non-empty
/// directory is only deleted when `delete_existing` selects the reference's consented
/// arm. Because the root is guaranteed empty afterwards, no project file can be written
/// over pre-existing content on this path.
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
    prepare_export_root(output_root, delete_existing)?;
    if workspace.files().is_empty() {
        return Err(invalid_project("project export has no SerializedFiles"));
    }
    if objects.report().object_count > limits.max_objects {
        return Err(invalid_project(format!(
            "project export has {} objects, exceeding the limit {}",
            objects.report().object_count,
            limits.max_objects
        )));
    }
    if !objects.report().is_complete() {
        return Err(invalid_project(format!(
            "project export requires every object to decode ({} decoded, {} failed)",
            objects.report().decoded_objects,
            objects.report().undecoded_objects
        )));
    }
    if !reference_graph.is_complete() {
        return Err(invalid_project(
            "project export requires a complete workspace PPtr scan",
        ));
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

    let object_count = objects.objects().len();
    let mut pointer_entries = Vec::new();
    pointer_entries
        .try_reserve_exact(object_count)
        .map_err(|_| invalid_project("could not allocate project export pointer entries"))?;
    let mut collection_metadata = Vec::new();
    collection_metadata
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_project("could not allocate project export collection metadata"))?;
    let mut collection_guids = Vec::new();
    collection_guids
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_project("could not allocate project export collection GUIDs"))?;

    for file_index in 0..workspace.files().len() {
        let guid = new_collection_guid(file_index as u64);
        collection_guids.push(guid);
        collection_metadata.push(ExportCollectionPointerMetadata {
            collection_index: file_index,
            guid,
            asset_type: UnityProjectAssetType::Serialized,
        });
    }

    let mut assignments_by_file = Vec::new();
    assignments_by_file
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_project("could not allocate per-file project assignments"))?;
    for (file_index, file) in workspace.files().iter().enumerate() {
        let mut assignments = Vec::new();
        assignments
            .try_reserve_exact(file.metadata.objects.len())
            .map_err(|_| invalid_project("could not allocate file project assignments"))?;
        for (object_index, object_info) in file.metadata.objects.iter().enumerate() {
            let object = objects.get(file_index, object_index).ok_or_else(|| {
                invalid_project(format!(
                    "workspace object is missing at file {file_index}, object {object_index}"
                ))
            })?;
            validate_exportable_object(object, object_info.path_id, reference_graph)?;
            let class_id = object.class_id.map(i32::from).ok_or_else(|| {
                invalid_project(format!(
                    "file {file_index} object {object_index} PathID {} has no class ID",
                    object_info.path_id
                ))
            })?;
            let asset = ExportAssetIdentity {
                collection: ExportAssetCollectionIdentity::SerializedFile { file_index },
                object_index,
                path_id: object_info.path_id,
                class_id,
            };
            let assignment = SceneExportIdAssignment {
                asset,
                // The input PathID is unique within a SerializedFile and remains stable
                // while the per-file GUID provides cross-file identity.
                file_id: object_info.path_id,
            };
            pointer_entries.push(ExportedAssetPointerEntry {
                asset,
                file_id: assignment.file_id,
                collection_index: file_index,
            });
            assignments.push(assignment);
        }
        assignments_by_file.push(assignments);
    }

    let pointer_map =
        ProjectExportPointerMap::new(&pointer_entries, &collection_metadata, limits.pointer_map)?;
    let streamed_resources = WorkspaceStreamedYamlResourceContext {
        filesystem,
        root_catalog,
        provider: resource_provider,
        content_limits: limits.streamed_content,
        staging_limits: Default::default(),
    };

    let mut planned_files = Vec::new();
    planned_files
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| invalid_project("could not allocate staged project asset files"))?;
    let mut total_yaml_bytes = 0usize;
    let mut exported_object_count = 0usize;
    for (file_index, assignments) in assignments_by_file.iter().enumerate() {
        if assignments.is_empty() {
            continue;
        }
        let yaml = workspace_assignments_to_unity_yaml_with_streamed_resources(
            workspace,
            objects,
            reference_graph,
            &pointer_map,
            assignments,
            file_index,
            None,
            limits.yaml_collection,
            &streamed_resources,
        )?;
        let document_count = yaml.match_indices("--- !u!").count();
        if document_count != assignments.len() {
            return Err(invalid_project(format!(
                "file {file_index} produced {document_count} YAML documents for {} input objects; refusing an incomplete project export",
                assignments.len()
            )));
        }
        total_yaml_bytes = total_yaml_bytes
            .checked_add(yaml.len())
            .ok_or_else(|| invalid_project("project YAML byte count overflows"))?;
        if total_yaml_bytes > limits.max_total_yaml_bytes {
            return Err(invalid_project(format!(
                "project YAML uses {total_yaml_bytes} bytes, exceeding the limit {}",
                limits.max_total_yaml_bytes
            )));
        }
        exported_object_count = exported_object_count
            .checked_add(document_count)
            .ok_or_else(|| invalid_project("project object count overflows"))?;
        planned_files.push(PlannedSerializedFileAsset {
            relative_path: format!("Assets/AssetRipper/SerializedFiles/file_{file_index:06}.asset"),
            guid: collection_guids[file_index],
            main_file_id: assignments[0].file_id,
            yaml,
        });
    }

    let major_is_five = project_version.major == 5;
    let version_file = project_version_file_contents(editor_version_text, major_is_five);
    write_new_project_file(
        output_root,
        "ProjectSettings/ProjectVersion.txt",
        version_file.as_bytes(),
        limits.project_output,
    )?;
    let manifest = package_manifest_json(
        project_version.major > 2019
            || (project_version.major == 2019 && project_version.minor >= 2),
    );
    write_new_project_file(
        output_root,
        "Packages/manifest.json",
        manifest.as_bytes(),
        limits.project_output,
    )?;

    for planned in &planned_files {
        let importer = native_format_importer(planned.main_file_id)?;
        let yaml = planned.yaml.as_bytes();
        export_asset_with_meta(
            output_root,
            AssetExportWithMetaPlan {
                relative_asset_path: planned.relative_path.clone(),
                guid: planned.guid,
                importer_class: "NativeFormatImporter".to_owned(),
                importer,
                meta_options: UnityMetaFileOptions::default(),
            },
            true,
            |path| {
                fs::write(path, yaml)?;
                Ok(yaml.len())
            },
            limits.project_output,
            limits.yaml_write,
        )?;
    }

    Ok(WorkspaceProjectExportReport {
        serialized_file_count: workspace.files().len(),
        exported_object_count,
        asset_file_count: planned_files.len(),
        total_yaml_bytes,
        project_version,
    })
}

fn validate_exportable_object(
    object: &WorkspaceObject,
    path_id: i64,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<(), Box<dyn Error>> {
    if path_id == 0 {
        return Err(invalid_project(format!(
            "file {} object {} has PathID 0, which cannot identify a Unity YAML document",
            object.file_index, object.object_index
        )));
    }
    if object.payload.is_err() {
        return Err(invalid_project(format!(
            "file {} object {} PathID {path_id} failed TypeTree decoding",
            object.file_index, object.object_index
        )));
    }
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

fn native_format_importer(main_file_id: i64) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    importer.insert_field(
        "externalObjects",
        UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
    )?;
    importer.insert_field(
        "mainObjectFileID",
        UnityYamlNode::scalar(UnityYamlScalar::Signed(main_file_id)),
    )?;
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field("assetBundleName", UnityYamlNode::string(""))?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

fn package_manifest_json(version_at_least_2019_2: bool) -> String {
    let dependencies = default_package_dependencies(version_at_least_2019_2);
    let mut manifest = String::from("{\n  \"dependencies\": {\n");
    for (index, (name, version)) in dependencies.iter().enumerate() {
        manifest.push_str("    \"");
        manifest.push_str(name);
        manifest.push_str("\": \"");
        manifest.push_str(version);
        manifest.push('\"');
        if index + 1 != dependencies.len() {
            manifest.push(',');
        }
        manifest.push('\n');
    }
    manifest.push_str("  }\n}\n");
    manifest
}

fn new_collection_guid(sequence: u64) -> [u8; 16] {
    static NEXT_GUID: AtomicU64 = AtomicU64::new(0);
    let counter = NEXT_GUID.fetch_add(1, Ordering::Relaxed);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let identity = (sequence, counter, std::process::id(), time);
    let first = RandomState::new().hash_one((&identity, 0u8));
    let second = RandomState::new().hash_one((&identity, 1u8));
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
    use super::{new_collection_guid, package_manifest_json};

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
        assert!(!package_manifest_json(false).contains("com.unity.modules.androidjni"));
        assert!(package_manifest_json(true).contains("\"com.unity.modules.androidjni\""));
    }
}
