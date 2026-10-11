use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use assetripper::io::dependency::DependencyIndexLimits;
use assetripper::io::filesystem::VirtualFileSystem;
use assetripper::io::game_structure::{MixedGameStructure, MixedGameStructureLimits};
use assetripper::processing::asset_factory::reference_class_layouts;
use assetripper::scheme::{
    BatchMountLimits, ContainerMountLimits, PathMountInput, SchemeReader, SchemeReaderLimits,
};
use assetripper::serialized_file::object::SerializedValue;
use assetripper::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
use assetripper::serialized_file::workspace::{
    PreExpandedContainer, SerializedFileWorkspace, SerializedFileWorkspaceLimits,
    SerializedFileWorkspaceLoadOptions,
};

fn main() -> Result<(), Box<dyn Error>> {
    let input = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: probe_vrcw_mono <bundle>")?;
    let selected_path = "/probe/input".to_owned();
    let mut filesystem = VirtualFileSystem::new();
    let mut mount_report = SchemeReader::mount_paths(
        &[PathMountInput {
            path: input,
            destination_root: Some(selected_path.clone()),
        }],
        &mut filesystem,
        SchemeReaderLimits::default(),
        ContainerMountLimits::default(),
        BatchMountLimits::default(),
    )?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &[selected_path],
        MixedGameStructureLimits::default(),
    )?;
    let mut preexpanded = Vec::new();
    for tree in std::mem::take(&mut mount_report.container_trees) {
        let tree = Arc::try_unwrap(tree).unwrap_or_else(|shared| (*shared).clone());
        preexpanded.push(PreExpandedContainer {
            virtual_path: tree.virtual_path,
            serialized_files_loaded: tree.serialized_files.len(),
            container_scopes: tree.container_scopes,
            container_bundles: tree.container_bundles,
            serialized_files: tree.serialized_files,
            resource_files: tree.resource_files,
            failed_entries: tree.failed_entries,
        });
    }
    let (workspace, _) =
        SerializedFileWorkspace::load_from_vfs_with_dependencies_and_layout_source(
            &mut filesystem,
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            SerializedFileWorkspaceLoadOptions {
                data_paths: &mixed.data_paths,
                limits: SerializedFileWorkspaceLimits::default(),
                index_limits: DependencyIndexLimits::default(),
                container_limits: ContainerMountLimits::default(),
                preexpanded_containers: &preexpanded,
                layouts: reference_class_layouts(),
            },
        )?;
    let objects = ObjectWorkspace::load_from_vfs_with_layout_source(
        &filesystem,
        &workspace,
        ObjectWorkspaceLimits::default(),
        reference_class_layouts(),
    )?;

    for (file_index, file) in workspace.files().iter().enumerate() {
        println!(
            "serialized file #{file_index}: name={:?} objects={} externals={:?}",
            file.file_name,
            file.metadata.objects.len(),
            file.metadata.externals
        );
    }
    for object in objects.objects().iter().filter(|object| {
        object.factory.class_id == 21
            && (object.factory.name == "unlit" || object.factory.name == "color (6)")
    }) {
        let Ok(payload) = &object.payload else {
            continue;
        };
        let shader = payload
            .root
            .unique_field_in_base_chain("m_Shader")
            .ok()
            .flatten();
        println!(
            "material {}:{} name={:?} m_Shader={shader:?}",
            object.file_index, object.path_id, object.factory.name
        );
        if let Some(SerializedValue::PPtr { file_id, path_id }) = shader {
            let file = &workspace.files()[object.file_index];
            let target_file = if *file_id == 0 {
                Some(file.file_name.clone())
            } else {
                usize::try_from(*file_id - 1)
                    .ok()
                    .and_then(|index| file.metadata.externals.get(index))
                    .map(|identifier| identifier.fixed_path_name())
            };
            let matching_shaders = objects
                .objects()
                .iter()
                .filter(|candidate| {
                    candidate.factory.class_id == 48 && candidate.path_id == *path_id
                })
                .map(|candidate| {
                    format!(
                        "{}:{} {:?}",
                        candidate.file_index, candidate.path_id, candidate.factory.name
                    )
                })
                .collect::<Vec<_>>();
            println!(
                "  shader pointer fileID={file_id} PathID={path_id} external={target_file:?} matching_shader_objects={matching_shaders:?}"
            );
        }
    }
    for object in objects.objects().iter().filter(|object| {
        object.factory.class_id == 48 && object.factory.name.to_ascii_lowercase().contains("unlit")
    }) {
        println!(
            "shader object {}:{} name={:?}",
            object.file_index, object.path_id, object.factory.name
        );
    }

    let mut exact_animation_track_ordinal = 0usize;
    for object in objects
        .objects()
        .iter()
        .filter(|object| object.factory.class_id == 114)
    {
        let Ok(payload) = &object.payload else {
            continue;
        };
        let type_name = payload.type_name.as_str();
        let name = object.factory.name.as_str();
        if name != "Animation Track" {
            continue;
        }
        exact_animation_track_ordinal += 1;
        let clip_count = payload
            .root
            .unique_field_in_base_chain("m_Clips")
            .ok()
            .flatten()
            .map(|value| match value {
                SerializedValue::Array(values) => values.len(),
                SerializedValue::Bytes(bytes) => bytes.len(),
                SerializedValue::Struct(fields) => fields.len(),
                _ => 0,
            });
        println!(
            "exact Animation Track #{exact_animation_track_ordinal}: file={} object={} pathID={} type={type_name:?} m_Clips={clip_count:?}",
            object.file_index, object.object_index, object.path_id
        );
    }
    Ok(())
}
