use super::*;

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
    let settings = ExportSettingsFile::from(crate::export::settings::ExportDefaults::default());
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
    export_workspace_yaml_project_with_assembly_catalog(
        output_root,
        delete_existing,
        filesystem,
        root_catalog,
        resource_provider,
        workspace,
        objects,
        reference_graph,
        settings,
        assembly_manager_is_set,
        None,
        limits,
    )
}
