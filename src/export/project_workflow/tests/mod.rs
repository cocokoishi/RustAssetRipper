use super::{
    asset_family_fallback, export_workspace_yaml_project_with_assembly_catalog,
    export_workspace_yaml_project_with_settings, new_collection_guid, package_manifest_json,
    plugin_importer_yaml, source_reference_assembly_maps, WorkspaceProjectExportLimits,
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
use crate::processing::asset_factory::FactoryUnityVersion;
use crate::processing::managed_assembly::{
    ManagedAssembly, ManagedAssemblyCatalog, ManagedAssemblyCatalogLimits, ManagedAssemblyLimits,
};
use crate::processing::managed_assembly_schema::ManagedSchemaLimits;
use crate::processing::scene_naming::SceneUnityVersion;
use crate::resource::RootResourceFileCatalog;
use crate::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
use crate::serialized_file::reference_graph::{
    MultiFilePPtrResolution, MultiFileReferenceGraphLimits,
};
use crate::serialized_file::workspace::{SerializedFileWorkspace, SerializedFileWorkspaceLimits};

mod managed_schema_export;
mod metadata_and_routing;
mod prefab_export;
mod reference_project;
