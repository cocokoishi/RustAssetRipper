use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use assetripper::io::dependency::DependencyIndexLimits;
use assetripper::io::filesystem::VirtualFileSystem;
use assetripper::io::game_structure::{MixedGameStructure, MixedGameStructureLimits};
use assetripper::processing::asset_factory::reference_class_layouts;
use assetripper::scheme::{BatchMountLimits, ContainerMountLimits, PathMountInput, SchemeReader, SchemeReaderLimits};
use assetripper::serialized_file::workspace::{PreExpandedContainer, SerializedFileWorkspace, SerializedFileWorkspaceLimits, SerializedFileWorkspaceLoadOptions};
fn main() -> Result<(), Box<dyn Error>> {
    let input = std::env::args_os().nth(1).map(PathBuf::from).ok_or("usage: target_dependency_probe <bundle>")?;
    let destination = "/probe/target".to_owned();
    let mut filesystem = VirtualFileSystem::new();
    let mut mount = SchemeReader::mount_paths(&[PathMountInput { path: input, destination_root: Some(destination.clone()) }], &mut filesystem, SchemeReaderLimits::default(), ContainerMountLimits::default(), BatchMountLimits::default())?;
    let mixed = MixedGameStructure::from_vfs_paths(&filesystem, &[destination], MixedGameStructureLimits::default())?;
    let mut preexpanded = Vec::new();
    for tree in std::mem::take(&mut mount.container_trees) {
        let tree = Arc::try_unwrap(tree).unwrap_or_else(|shared| (*shared).clone());
        preexpanded.push(PreExpandedContainer { virtual_path: tree.virtual_path, serialized_files_loaded: tree.serialized_files.len(), container_scopes: tree.container_scopes, container_bundles: tree.container_bundles, serialized_files: tree.serialized_files, resource_files: tree.resource_files, failed_entries: tree.failed_entries });
    }
    let (workspace, dependency_report) = SerializedFileWorkspace::load_from_vfs_with_dependencies_and_layout_source(
        &mut filesystem, &mixed.serialized_file_inputs, &mixed.registered_files,
        SerializedFileWorkspaceLoadOptions { data_paths: &mixed.data_paths, limits: SerializedFileWorkspaceLimits::default(), index_limits: DependencyIndexLimits::default(), container_limits: ContainerMountLimits::default(), preexpanded_containers: &preexpanded, layouts: reference_class_layouts() })?;
    for (i, file) in workspace.files().iter().enumerate() {
        println!("FILE {i} name={:?}", file.file_name);
        for (external_index, identifier) in file.metadata.externals.iter().enumerate() {
            println!("  EXTERNAL {external_index}={identifier:?} fixed={:?}", identifier.fixed_path_name());
        }
    }
    for request in dependency_report.requests { println!("REQUEST {request:?}"); }
    Ok(())
}
