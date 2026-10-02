//! Platform game file registration matching `PlatformGameStructure.CollectFiles`.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Read, Seek, SeekFrom};

use crate::io::dependency::DependencyFilePath;
use crate::io::dependency::DependencyIndexLimits;
use crate::io::filesystem::{SearchOption, VirtualFileSystem};
use crate::io::platform_structure::{PlatformKind, PlatformStructure};
use crate::io::stream::{logical_file_name, logical_file_path};
use crate::scheme::{
    ContainerMountError, ContainerMountLimits, FileKind, SchemeError, SchemeReader,
};
use crate::serialized_file::workspace::{
    PreExpandedContainer, SerializedFileDependencyLoadOptions, SerializedFileDependencyReport,
    SerializedFileWorkspace, SerializedFileWorkspaceError, SerializedFileWorkspaceLimits,
};

/// Bounds applied while collecting platform files and probing bundle headers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformFileInventoryLimits {
    pub max_inspected_files: usize,
    pub max_single_file_bytes: u64,
    pub max_total_inspected_bytes: u64,
    pub max_assemblies: usize,
    pub max_directories: usize,
    pub max_directory_depth: usize,
    pub max_path_bytes: usize,
}

impl Default for PlatformFileInventoryLimits {
    fn default() -> Self {
        Self {
            max_inspected_files: 1_000_000,
            max_single_file_bytes: 2 * 1024 * 1024 * 1024,
            max_total_inspected_bytes: 16 * 1024 * 1024 * 1024,
            max_assemblies: 100_000,
            max_directories: 100_000,
            max_directory_depth: 256,
            max_path_bytes: 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlatformFileInventoryOptions {
    pub skip_streaming_assets: bool,
}

/// Files that the platform structure registers for dependency lookup.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlatformFileInventory {
    /// Deterministically ordered `(logical file name, virtual path)` registrations.
    pub registered_files: Vec<DependencyFilePath>,
    /// Source `IsPrimaryEngineFile` matches to parse as initial SerializedFile inputs.
    pub primary_file_inputs: Vec<DependencyFilePath>,
    pub scripting_backend: ScriptingBackend,
    /// Mono assemblies selected by the source platform's `ManagedPath` rules.
    pub assemblies: Vec<DependencyFilePath>,
    pub report: PlatformFileInventoryReport,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScriptingBackend {
    #[default]
    Unknown,
    Mono,
    Il2Cpp,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlatformFileInventoryReport {
    pub inspected_files: usize,
    pub inspected_bytes: u64,
    pub traversed_directories: usize,
    pub assembly_candidates: usize,
    pub duplicate_assembly_names: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlatformWorkspaceLoadOptions {
    pub file_inventory: PlatformFileInventoryOptions,
    pub file_inventory_limits: PlatformFileInventoryLimits,
    pub workspace_limits: SerializedFileWorkspaceLimits,
    pub dependency_index_limits: DependencyIndexLimits,
    pub container_mount_limits: ContainerMountLimits,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformWorkspaceLoadResult {
    pub file_inventory: PlatformFileInventory,
    pub workspace: SerializedFileWorkspace,
    pub dependencies: SerializedFileDependencyReport,
    pub initial_container_expansions: Vec<PlatformContainerExpansionReport>,
    /// Registered files that dispatch as Resources and are retained for dependency lookup.
    pub resource_candidates: Vec<DependencyFilePath>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformContainerExpansionReport {
    pub source_file: DependencyFilePath,
    pub kind: FileKind,
    pub mounted_entries: usize,
    pub mounted_bytes: u64,
    pub serialized_files_found: usize,
}

impl PlatformStructure {
    /// Collect engine files and bundle files using the source platform structure rules.
    ///
    /// This covers `CollectGameFiles`, platform WebGL/WebPlayer registrations, and streaming
    /// bundle discovery. Managed assembly collection is intentionally a separate step because
    /// the source only enables it after identifying the scripting backend as Mono.
    pub fn collect_game_files(
        &self,
        filesystem: &VirtualFileSystem,
        options: PlatformFileInventoryOptions,
        limits: PlatformFileInventoryLimits,
    ) -> Result<PlatformFileInventory, PlatformFileInventoryError> {
        let mut state = InventoryState {
            filesystem,
            limits,
            report: PlatformFileInventoryReport::default(),
            inspected_paths: BTreeSet::new(),
            registered_files: BTreeSet::new(),
            primary_file_inputs: BTreeSet::new(),
            assemblies: BTreeMap::new(),
        };

        state.collect_web_platform_file(self)?;
        for data_path in &self.data_paths {
            state.validate_path(data_path)?;
            if !state.directory_exists(data_path)? {
                return Err(PlatformFileInventoryError::new(
                    Some(data_path.clone()),
                    PlatformFileInventoryErrorKind::DataPathNotFound,
                ));
            }
            state.collect_data_path(data_path)?;
        }

        if !options.skip_streaming_assets {
            state.collect_streaming_assets(self)?;
        }
        if self.platform == PlatformKind::Android {
            state.collect_android_asset_bundles(self)?;
        }
        let scripting_backend = state.detect_backend_and_collect_assemblies(self)?;

        Ok(PlatformFileInventory {
            registered_files: state
                .registered_files
                .into_iter()
                .map(|(file_name, virtual_path)| DependencyFilePath {
                    file_name,
                    virtual_path,
                })
                .collect(),
            primary_file_inputs: state
                .primary_file_inputs
                .into_iter()
                .map(|(file_name, virtual_path)| DependencyFilePath {
                    file_name,
                    virtual_path,
                })
                .collect(),
            scripting_backend,
            assemblies: state
                .assemblies
                .into_iter()
                .map(|(file_name, virtual_path)| DependencyFilePath {
                    file_name,
                    virtual_path,
                })
                .collect(),
            report: state.report,
        })
    }

    /// Load platform SerializedFiles and resolve their external references using the registered
    /// files and ordered DataPaths. Registered initial containers are expanded before the
    /// workspace starts its external dependency walk.
    pub fn load_primary_serialized_workspace(
        &self,
        filesystem: &mut VirtualFileSystem,
        options: PlatformWorkspaceLoadOptions,
    ) -> Result<PlatformWorkspaceLoadResult, PlatformWorkspaceLoadError> {
        let file_inventory = self
            .collect_game_files(
                filesystem,
                options.file_inventory,
                options.file_inventory_limits,
            )
            .map_err(|error| {
                PlatformWorkspaceLoadError::new(PlatformWorkspaceLoadErrorKind::Inventory(
                    Box::new(error),
                ))
            })?;
        let imported = import_initial_platform_files(filesystem, &file_inventory, options)?;
        if imported.inputs.is_empty() {
            return Err(PlatformWorkspaceLoadError::new(
                PlatformWorkspaceLoadErrorKind::NoSerializedFileInputs,
            ));
        }
        let (workspace, dependencies) =
            SerializedFileWorkspace::load_from_vfs_with_preexpanded_containers(
                filesystem,
                &imported.inputs,
                &imported.registered_files,
                SerializedFileDependencyLoadOptions {
                    data_paths: &self.data_paths,
                    limits: imported.workspace_limits,
                    index_limits: options.dependency_index_limits,
                    container_limits: options.container_mount_limits,
                    preexpanded_containers: &imported.preexpanded_containers,
                },
            )
            .map_err(|error| {
                PlatformWorkspaceLoadError::new(PlatformWorkspaceLoadErrorKind::Workspace(error))
            })?;
        Ok(PlatformWorkspaceLoadResult {
            file_inventory,
            workspace,
            dependencies,
            initial_container_expansions: imported.expansions,
            resource_candidates: imported.resource_candidates,
        })
    }
}

struct InventoryState<'a> {
    filesystem: &'a VirtualFileSystem,
    limits: PlatformFileInventoryLimits,
    report: PlatformFileInventoryReport,
    inspected_paths: BTreeSet<String>,
    registered_files: BTreeSet<(String, String)>,
    primary_file_inputs: BTreeSet<(String, String)>,
    assemblies: BTreeMap<String, String>,
}

struct ImportedInitialPlatformFiles {
    inputs: Vec<DependencyFilePath>,
    registered_files: Vec<DependencyFilePath>,
    preexpanded_containers: Vec<PreExpandedContainer>,
    expansions: Vec<PlatformContainerExpansionReport>,
    resource_candidates: Vec<DependencyFilePath>,
    workspace_limits: SerializedFileWorkspaceLimits,
}

fn import_initial_platform_files(
    filesystem: &mut VirtualFileSystem,
    inventory: &PlatformFileInventory,
    options: PlatformWorkspaceLoadOptions,
) -> Result<ImportedInitialPlatformFiles, PlatformWorkspaceLoadError> {
    let mut inputs = BTreeSet::<(String, String)>::new();
    let mut registered_files = BTreeSet::<(String, String)>::new();
    let mut resource_candidates = BTreeSet::<(String, String)>::new();
    let primary_paths: BTreeSet<&str> = inventory
        .primary_file_inputs
        .iter()
        .map(|entry| entry.virtual_path.as_str())
        .collect();
    for entry in &inventory.primary_file_inputs {
        inputs.insert((entry.file_name.clone(), entry.virtual_path.clone()));
    }
    for entry in &inventory.registered_files {
        registered_files.insert((entry.file_name.clone(), entry.virtual_path.clone()));
    }
    if inputs.len() > options.workspace_limits.max_files {
        return Err(PlatformWorkspaceLoadError::new(
            PlatformWorkspaceLoadErrorKind::InitialSerializedFileCountLimitExceeded {
                count: inputs.len(),
                limit: options.workspace_limits.max_files,
            },
        ));
    }
    if registered_files.len() > options.dependency_index_limits.max_registered_files {
        return Err(PlatformWorkspaceLoadError::new(
            PlatformWorkspaceLoadErrorKind::InitialRegistryCountLimitExceeded {
                count: registered_files.len(),
                limit: options.dependency_index_limits.max_registered_files,
            },
        ));
    }

    let mut preexpanded_containers = Vec::new();
    let mut expansions = Vec::new();
    let mut namespace_index = 0usize;
    let mut total_container_input_bytes = 0u64;
    let mut total_container_entries = 0usize;
    let mut total_container_output_bytes = 0u64;

    for entry in &inventory.registered_files {
        if primary_paths.contains(entry.virtual_path.as_str()) {
            continue;
        }
        let mut stream = filesystem
            .open_file(&entry.virtual_path)
            .map_err(|source| {
                PlatformWorkspaceLoadError::new(PlatformWorkspaceLoadErrorKind::FileSystem {
                    virtual_path: entry.virtual_path.clone(),
                    source: Box::new(source),
                })
            })?;
        let dispatched = SchemeReader::read_stream(&mut stream, options.workspace_limits.scheme)
            .map_err(|source| {
                PlatformWorkspaceLoadError::new(PlatformWorkspaceLoadErrorKind::Scheme {
                    virtual_path: entry.virtual_path.clone(),
                    source: Box::new(source),
                })
            })?;

        match dispatched.kind() {
            FileKind::SerializedFile => {
                inputs.insert((entry.file_name.clone(), entry.virtual_path.clone()));
                if inputs.len() > options.workspace_limits.max_files {
                    return Err(PlatformWorkspaceLoadError::new(
                        PlatformWorkspaceLoadErrorKind::InitialSerializedFileCountLimitExceeded {
                            count: inputs.len(),
                            limit: options.workspace_limits.max_files,
                        },
                    ));
                }
            }
            FileKind::Resource => {
                resource_candidates.insert((entry.file_name.clone(), entry.virtual_path.clone()));
            }
            kind if is_platform_container_kind(kind) => {
                if expansions.len() >= options.workspace_limits.max_files {
                    return Err(PlatformWorkspaceLoadError::new(
                        PlatformWorkspaceLoadErrorKind::InitialContainerCountLimitExceeded {
                            count: expansions.len() + 1,
                            limit: options.workspace_limits.max_files,
                        },
                    ));
                }
                let input_bytes = u64::try_from(dispatched.bytes().len()).map_err(|_| {
                    PlatformWorkspaceLoadError::new(
                        PlatformWorkspaceLoadErrorKind::InitialContainerInputByteCountOverflow,
                    )
                })?;
                total_container_input_bytes = total_container_input_bytes
                    .checked_add(input_bytes)
                    .ok_or_else(|| {
                        PlatformWorkspaceLoadError::new(
                            PlatformWorkspaceLoadErrorKind::InitialContainerInputByteCountOverflow,
                        )
                    })?;
                if total_container_input_bytes
                    > options
                        .workspace_limits
                        .max_total_dependency_container_input_bytes
                {
                    return Err(PlatformWorkspaceLoadError::new(
                        PlatformWorkspaceLoadErrorKind::InitialContainerInputLimitExceeded {
                            byte_count: total_container_input_bytes,
                            limit: options
                                .workspace_limits
                                .max_total_dependency_container_input_bytes,
                        },
                    ));
                }
                let remaining_entries = options
                    .workspace_limits
                    .max_total_dependency_container_entries
                    .checked_sub(total_container_entries)
                    .ok_or_else(|| {
                        PlatformWorkspaceLoadError::new(
                            PlatformWorkspaceLoadErrorKind::InitialContainerEntryCountOverflow,
                        )
                    })?;
                let remaining_output_bytes = options
                    .workspace_limits
                    .max_total_dependency_container_output_bytes
                    .checked_sub(total_container_output_bytes)
                    .ok_or_else(|| {
                        PlatformWorkspaceLoadError::new(
                            PlatformWorkspaceLoadErrorKind::InitialContainerOutputByteCountOverflow,
                        )
                    })?;
                let mut mount_limits = options.container_mount_limits;
                mount_limits.max_entries = mount_limits.max_entries.min(remaining_entries);
                mount_limits.max_total_output_bytes = mount_limits
                    .max_total_output_bytes
                    .min(remaining_output_bytes);

                let mount_root = next_initial_container_root(
                    filesystem,
                    &mut namespace_index,
                    options.workspace_limits.max_files,
                )
                .map_err(|source| {
                    PlatformWorkspaceLoadError::new(PlatformWorkspaceLoadErrorKind::FileSystem {
                        virtual_path: entry.virtual_path.clone(),
                        source: Box::new(source),
                    })
                })?;
                let mount_report = dispatched
                    .mount_container_tree(filesystem, Some(&mount_root), mount_limits)
                    .map_err(|source| {
                        PlatformWorkspaceLoadError::new(
                            PlatformWorkspaceLoadErrorKind::ContainerMount {
                                virtual_path: entry.virtual_path.clone(),
                                source: Box::new(source),
                            },
                        )
                    })?;
                total_container_entries = total_container_entries
                    .checked_add(mount_report.mounted_entries)
                    .ok_or_else(|| {
                        PlatformWorkspaceLoadError::new(
                            PlatformWorkspaceLoadErrorKind::InitialContainerEntryCountOverflow,
                        )
                    })?;
                if total_container_entries
                    > options
                        .workspace_limits
                        .max_total_dependency_container_entries
                {
                    return Err(PlatformWorkspaceLoadError::new(
                        PlatformWorkspaceLoadErrorKind::InitialContainerEntryLimitExceeded {
                            count: total_container_entries,
                            limit: options
                                .workspace_limits
                                .max_total_dependency_container_entries,
                        },
                    ));
                }
                total_container_output_bytes = total_container_output_bytes
                    .checked_add(mount_report.mounted_bytes)
                    .ok_or_else(|| {
                        PlatformWorkspaceLoadError::new(
                            PlatformWorkspaceLoadErrorKind::InitialContainerOutputByteCountOverflow,
                        )
                    })?;
                if total_container_output_bytes
                    > options
                        .workspace_limits
                        .max_total_dependency_container_output_bytes
                {
                    return Err(PlatformWorkspaceLoadError::new(
                        PlatformWorkspaceLoadErrorKind::InitialContainerOutputLimitExceeded {
                            byte_count: total_container_output_bytes,
                            limit: options
                                .workspace_limits
                                .max_total_dependency_container_output_bytes,
                        },
                    ));
                }

                let resulting_registry_count = registered_files
                    .len()
                    .checked_add(mount_report.mounted_entries)
                    .ok_or_else(|| {
                        PlatformWorkspaceLoadError::new(
                            PlatformWorkspaceLoadErrorKind::InitialRegistryCountOverflow,
                        )
                    })?;
                if resulting_registry_count > options.dependency_index_limits.max_registered_files {
                    return Err(PlatformWorkspaceLoadError::new(
                        PlatformWorkspaceLoadErrorKind::InitialRegistryCountLimitExceeded {
                            count: resulting_registry_count,
                            limit: options.dependency_index_limits.max_registered_files,
                        },
                    ));
                }

                let mut serialized_files_found = 0usize;
                if filesystem
                    .directory_exists(Some(&mount_root))
                    .map_err(|source| {
                        PlatformWorkspaceLoadError::new(
                            PlatformWorkspaceLoadErrorKind::FileSystem {
                                virtual_path: mount_root.clone(),
                                source: Box::new(source),
                            },
                        )
                    })?
                {
                    let leaf_paths = filesystem
                        .enumerate_files(Some(&mount_root), "*", SearchOption::AllDirectories)
                        .map_err(|source| {
                            PlatformWorkspaceLoadError::new(
                                PlatformWorkspaceLoadErrorKind::FileSystem {
                                    virtual_path: mount_root.clone(),
                                    source: Box::new(source),
                                },
                            )
                        })?;
                    let resulting_registry_count = registered_files
                        .len()
                        .checked_add(leaf_paths.len())
                        .ok_or_else(|| {
                            PlatformWorkspaceLoadError::new(
                                PlatformWorkspaceLoadErrorKind::InitialRegistryCountOverflow,
                            )
                        })?;
                    if resulting_registry_count
                        > options.dependency_index_limits.max_registered_files
                    {
                        return Err(PlatformWorkspaceLoadError::new(
                            PlatformWorkspaceLoadErrorKind::InitialRegistryCountLimitExceeded {
                                count: resulting_registry_count,
                                limit: options.dependency_index_limits.max_registered_files,
                            },
                        ));
                    }
                    for leaf_path in leaf_paths {
                        let relative_name = relative_path_under(&leaf_path, &mount_root)
                            .ok_or_else(|| {
                                PlatformWorkspaceLoadError::new(
                                    PlatformWorkspaceLoadErrorKind::UnexpectedContainerLeaf {
                                        virtual_path: leaf_path.clone(),
                                        kind: FileKind::Resource,
                                    },
                                )
                            })?;
                        let leaf_entry = DependencyFilePath {
                            file_name: relative_name.to_owned(),
                            virtual_path: leaf_path.clone(),
                        };
                        registered_files.insert((
                            leaf_entry.file_name.clone(),
                            leaf_entry.virtual_path.clone(),
                        ));
                        let mut leaf_stream =
                            filesystem.open_file(&leaf_path).map_err(|source| {
                                PlatformWorkspaceLoadError::new(
                                    PlatformWorkspaceLoadErrorKind::FileSystem {
                                        virtual_path: leaf_path.clone(),
                                        source: Box::new(source),
                                    },
                                )
                            })?;
                        let leaf = SchemeReader::read_stream(
                            &mut leaf_stream,
                            options.workspace_limits.scheme,
                        )
                        .map_err(|source| {
                            PlatformWorkspaceLoadError::new(
                                PlatformWorkspaceLoadErrorKind::Scheme {
                                    virtual_path: leaf_path.clone(),
                                    source: Box::new(source),
                                },
                            )
                        })?;
                        match leaf.kind() {
                            FileKind::SerializedFile => {
                                let inserted = inputs.insert((
                                    leaf_entry.file_name.clone(),
                                    leaf_entry.virtual_path.clone(),
                                ));
                                if inserted {
                                    serialized_files_found = serialized_files_found
                                        .checked_add(1)
                                        .ok_or_else(|| {
                                            PlatformWorkspaceLoadError::new(
                                                PlatformWorkspaceLoadErrorKind::InitialSerializedFileCountOverflow,
                                            )
                                        })?;
                                    if inputs.len() > options.workspace_limits.max_files {
                                        return Err(PlatformWorkspaceLoadError::new(
                                            PlatformWorkspaceLoadErrorKind::InitialSerializedFileCountLimitExceeded {
                                                count: inputs.len(),
                                                limit: options.workspace_limits.max_files,
                                            },
                                        ));
                                    }
                                }
                            }
                            FileKind::Resource => {
                                resource_candidates
                                    .insert((leaf_entry.file_name, leaf_entry.virtual_path));
                            }
                            leaf_kind => {
                                return Err(PlatformWorkspaceLoadError::new(
                                    PlatformWorkspaceLoadErrorKind::UnexpectedContainerLeaf {
                                        virtual_path: leaf_path,
                                        kind: leaf_kind,
                                    },
                                ));
                            }
                        }
                    }
                }
                preexpanded_containers.push(PreExpandedContainer {
                    virtual_path: entry.virtual_path.clone(),
                    serialized_files_loaded: serialized_files_found,
                    container_scopes: mount_report.container_scopes,
                });
                expansions.push(PlatformContainerExpansionReport {
                    source_file: entry.clone(),
                    kind,
                    mounted_entries: mount_report.mounted_entries,
                    mounted_bytes: mount_report.mounted_bytes,
                    serialized_files_found,
                });
            }
            _ => {}
        }
    }

    if inputs.len() > options.workspace_limits.max_files {
        return Err(PlatformWorkspaceLoadError::new(
            PlatformWorkspaceLoadErrorKind::InitialSerializedFileCountLimitExceeded {
                count: inputs.len(),
                limit: options.workspace_limits.max_files,
            },
        ));
    }
    if registered_files.len() > options.dependency_index_limits.max_registered_files {
        return Err(PlatformWorkspaceLoadError::new(
            PlatformWorkspaceLoadErrorKind::InitialRegistryCountLimitExceeded {
                count: registered_files.len(),
                limit: options.dependency_index_limits.max_registered_files,
            },
        ));
    }
    let mut workspace_limits = options.workspace_limits;
    workspace_limits.max_total_dependency_container_input_bytes -= total_container_input_bytes;
    workspace_limits.max_total_dependency_container_entries -= total_container_entries;
    workspace_limits.max_total_dependency_container_output_bytes -= total_container_output_bytes;

    Ok(ImportedInitialPlatformFiles {
        inputs: into_dependency_paths(inputs),
        registered_files: into_dependency_paths(registered_files),
        preexpanded_containers,
        expansions,
        resource_candidates: into_dependency_paths(resource_candidates),
        workspace_limits,
    })
}

fn into_dependency_paths(files: BTreeSet<(String, String)>) -> Vec<DependencyFilePath> {
    files
        .into_iter()
        .map(|(file_name, virtual_path)| DependencyFilePath {
            file_name,
            virtual_path,
        })
        .collect()
}

fn is_platform_container_kind(kind: FileKind) -> bool {
    matches!(
        kind,
        FileKind::UnityFs
            | FileKind::UnityRaw
            | FileKind::UnityWebBundle
            | FileKind::UnityWebData
            | FileKind::UnityArchive
    )
}

fn relative_path_under<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    let prefix = format!("{}/", root.trim_end_matches('/'));
    path.strip_prefix(&prefix)
        .filter(|relative| !relative.is_empty())
}

fn next_initial_container_root(
    filesystem: &VirtualFileSystem,
    namespace_index: &mut usize,
    maximum_attempts: usize,
) -> io::Result<String> {
    for _ in 0..maximum_attempts {
        let index = *namespace_index;
        *namespace_index = namespace_index.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "initial container namespace overflows",
            )
        })?;
        let root = format!("/.assetripper/import/initial/{index}");
        if !filesystem.directory_exists(Some(&root))? && !filesystem.file_exists(Some(&root))? {
            return Ok(root);
        }
    }
    Err(io::Error::other(
        "could not allocate a unique initial container namespace",
    ))
}

impl InventoryState<'_> {
    fn collect_data_path(&mut self, root: &str) -> Result<(), PlatformFileInventoryError> {
        for path in self.top_level_files(root)? {
            let name = file_name(&path);
            if matches!(name, "data.unity3d" | "datapack.unity3d") {
                self.register(name.to_owned(), path);
            } else if is_primary_engine_file(name) {
                let logical_name = logical_file_name(&path).ok_or_else(|| {
                    PlatformFileInventoryError::new(
                        Some(path.clone()),
                        PlatformFileInventoryErrorKind::InvalidFileName,
                    )
                })?;
                let logical_path = logical_file_path(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                self.primary_file_inputs
                    .insert((logical_name.clone(), logical_path.clone()));
                self.register(logical_name, logical_path);
            }
        }
        Ok(())
    }

    fn collect_web_platform_file(
        &mut self,
        structure: &PlatformStructure,
    ) -> Result<(), PlatformFileInventoryError> {
        match structure.platform {
            PlatformKind::WebGl => {
                let Some(data_directory) = structure.data_paths.get(1) else {
                    return Ok(());
                };
                let suffix = match file_name(data_directory) {
                    "Build" => ".data.unityweb",
                    "Development" => ".data",
                    "Release" => ".datagz",
                    _ => return Ok(()),
                };
                for path in self.top_level_files(data_directory)? {
                    let name = file_name(&path);
                    if let Some(stem) = name.strip_suffix(suffix) {
                        self.register(stem.to_owned(), path);
                        break;
                    }
                }
            }
            PlatformKind::WebPlayer => {
                let Some(name) = &structure.name else {
                    return Ok(());
                };
                let path = join_path(&structure.root_path, &format!("{name}.unity3d"));
                if self.file_exists(&path)? {
                    self.register(name.clone(), path);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn collect_streaming_assets(
        &mut self,
        structure: &PlatformStructure,
    ) -> Result<(), PlatformFileInventoryError> {
        match structure.platform {
            PlatformKind::Android => {}
            PlatformKind::Ios => {
                self.collect_optional_bundle_tree(&join_path(&structure.root_path, "Raw"))?;
            }
            PlatformKind::Windows
            | PlatformKind::Linux
            | PlatformKind::Mac
            | PlatformKind::Switch
            | PlatformKind::WiiU
            | PlatformKind::WindowsPhone => {
                if let Some(game_data_path) = &structure.game_data_path {
                    self.collect_optional_bundle_tree(&join_path(
                        game_data_path,
                        "StreamingAssets",
                    ))?;
                }
            }
            PlatformKind::WebGl => {
                self.collect_optional_bundle_tree(&structure.root_path)?;
            }
            PlatformKind::Ps4 | PlatformKind::WebPlayer => {}
        }
        Ok(())
    }

    fn collect_android_asset_bundles(
        &mut self,
        structure: &PlatformStructure,
    ) -> Result<(), PlatformFileInventoryError> {
        let assets = join_path(&structure.root_path, "assets");
        if !self.directory_exists(&assets)? {
            return Err(PlatformFileInventoryError::new(
                Some(assets),
                PlatformFileInventoryErrorKind::AndroidAssetsPathNotFound,
            ));
        }
        self.collect_bundle_directory(&assets, true)
    }

    fn detect_backend_and_collect_assemblies(
        &mut self,
        structure: &PlatformStructure,
    ) -> Result<ScriptingBackend, PlatformFileInventoryError> {
        let managed_path = managed_path(structure);
        let backend = if self.has_il2cpp_files(structure, managed_path.as_deref())? {
            ScriptingBackend::Il2Cpp
        } else if let Some(path) = managed_path.as_deref() {
            if self.has_mono_assemblies(path)? {
                ScriptingBackend::Mono
            } else {
                ScriptingBackend::Unknown
            }
        } else {
            ScriptingBackend::Unknown
        };

        if backend == ScriptingBackend::Mono {
            if let Some(path) = managed_path.as_deref() {
                if self.directory_exists(path)? {
                    self.collect_assemblies(path)?;
                    return Ok(backend);
                }
            }
            if let Some(game_data_path) = structure.game_data_path.as_deref() {
                let lib_path = join_path(game_data_path, "lib");
                if self.directory_exists(&lib_path)? {
                    self.collect_assemblies(game_data_path)?;
                    self.collect_assemblies(&lib_path)?;
                }
            }
        }
        Ok(backend)
    }

    fn has_il2cpp_files(
        &mut self,
        structure: &PlatformStructure,
        managed_path: Option<&str>,
    ) -> Result<bool, PlatformFileInventoryError> {
        let Some(game_data_path) = structure.game_data_path.as_deref() else {
            return Ok(false);
        };
        let exists_pair = |this: &Self, game_assembly: &str, metadata: &str| {
            Ok(this.file_exists(game_assembly)? && this.file_exists(metadata)?)
        };
        match structure.platform {
            PlatformKind::Windows => exists_pair(
                self,
                &join_path(&structure.root_path, "GameAssembly.dll"),
                &join_path(game_data_path, "il2cpp_data/Metadata/global-metadata.dat"),
            ),
            PlatformKind::Linux => exists_pair(
                self,
                &join_path(&structure.root_path, "GameAssembly.so"),
                &join_path(game_data_path, "il2cpp_data/Metadata/global-metadata.dat"),
            ),
            PlatformKind::Mac => exists_pair(
                self,
                &join_path(
                    &structure.root_path,
                    "Contents/Frameworks/GameAssembly.dylib",
                ),
                &join_path(game_data_path, "il2cpp_data/Metadata/global-metadata.dat"),
            ),
            PlatformKind::Android => {
                let Some(managed_path) = managed_path else {
                    return Ok(false);
                };
                let metadata = join_path(managed_path, "Metadata/global-metadata.dat");
                if !self.file_exists(&metadata)? {
                    return Ok(false);
                }
                let lib_path = join_path(&structure.root_path, "lib");
                self.contains_file_recursively(&lib_path, "libil2cpp.so")
            }
            PlatformKind::Ios => {
                let Some(name) = structure.name.as_deref() else {
                    return Ok(false);
                };
                let app_path = join_path(
                    &join_path(&structure.root_path, "Payload"),
                    &format!("{name}.app"),
                );
                let assembly_path = join_path(&app_path, name);
                let Some(managed_path) = managed_path else {
                    return Ok(false);
                };
                exists_pair(
                    self,
                    &assembly_path,
                    &join_path(managed_path, "Metadata/global-metadata.dat"),
                )
            }
            PlatformKind::Switch => {
                let Some(managed_path) = managed_path else {
                    return Ok(false);
                };
                exists_pair(
                    self,
                    &join_path(&structure.root_path, "exefs/main"),
                    &join_path(managed_path, "Metadata/global-metadata.dat"),
                )
            }
            PlatformKind::Ps4 => exists_pair(
                self,
                &join_path(game_data_path, "Modules/Il2CppUserAssemblies.prx"),
                &join_path(game_data_path, "Metadata/global-metadata.dat"),
            ),
            PlatformKind::WindowsPhone => exists_pair(
                self,
                &join_path(&structure.root_path, "GameAssembly.dll"),
                &join_path(game_data_path, "il2cpp_data/Metadata/global-metadata.dat"),
            ),
            PlatformKind::WiiU | PlatformKind::WebGl | PlatformKind::WebPlayer => Ok(false),
        }
    }

    fn has_mono_assemblies(&self, managed_path: &str) -> Result<bool, PlatformFileInventoryError> {
        if !self.directory_exists(managed_path)? {
            return Ok(false);
        }
        Ok(self
            .top_level_files(managed_path)?
            .iter()
            .any(|path| file_name(path).ends_with(".dll")))
    }

    fn contains_file_recursively(
        &mut self,
        root: &str,
        expected_name: &str,
    ) -> Result<bool, PlatformFileInventoryError> {
        if !self.directory_exists(root)? {
            return Ok(false);
        }
        let mut pending = vec![(root.to_owned(), 0usize)];
        while let Some((directory, depth)) = pending.pop() {
            self.record_directory_visit(&directory)?;
            if self
                .top_level_files(&directory)?
                .iter()
                .any(|path| file_name(path) == expected_name)
            {
                return Ok(true);
            }
            for child in self.top_level_directories(&directory)? {
                let child_depth = depth.checked_add(1).ok_or_else(|| {
                    PlatformFileInventoryError::new(
                        Some(child.clone()),
                        PlatformFileInventoryErrorKind::DirectoryDepthOverflow,
                    )
                })?;
                if child_depth > self.limits.max_directory_depth {
                    return Err(PlatformFileInventoryError::new(
                        Some(child.clone()),
                        PlatformFileInventoryErrorKind::DirectoryDepthLimitExceeded {
                            depth: child_depth,
                            limit: self.limits.max_directory_depth,
                        },
                    ));
                }
                pending.push((child, child_depth));
            }
        }
        Ok(false)
    }

    fn collect_assemblies(&mut self, root: &str) -> Result<(), PlatformFileInventoryError> {
        for path in self.top_level_files(root)? {
            let name = file_name(&path);
            if !name.ends_with(".dll") {
                continue;
            }
            let next_count = self
                .report
                .assembly_candidates
                .checked_add(1)
                .ok_or_else(|| {
                    PlatformFileInventoryError::new(
                        Some(path.clone()),
                        PlatformFileInventoryErrorKind::AssemblyCountOverflow,
                    )
                })?;
            if next_count > self.limits.max_assemblies {
                return Err(PlatformFileInventoryError::new(
                    Some(path.clone()),
                    PlatformFileInventoryErrorKind::AssemblyCountLimitExceeded {
                        count: next_count,
                        limit: self.limits.max_assemblies,
                    },
                ));
            }
            self.report.assembly_candidates = next_count;
            if self
                .assemblies
                .insert(name.to_owned(), path.clone())
                .is_some()
            {
                self.report.duplicate_assembly_names.push(name.to_owned());
            }
        }
        Ok(())
    }

    fn record_directory_visit(&mut self, path: &str) -> Result<(), PlatformFileInventoryError> {
        self.report.traversed_directories = self
            .report
            .traversed_directories
            .checked_add(1)
            .ok_or_else(|| {
                PlatformFileInventoryError::new(
                    Some(path.to_owned()),
                    PlatformFileInventoryErrorKind::DirectoryCountOverflow,
                )
            })?;
        if self.report.traversed_directories > self.limits.max_directories {
            return Err(PlatformFileInventoryError::new(
                Some(path.to_owned()),
                PlatformFileInventoryErrorKind::DirectoryCountLimitExceeded {
                    count: self.report.traversed_directories,
                    limit: self.limits.max_directories,
                },
            ));
        }
        Ok(())
    }

    fn collect_optional_bundle_tree(
        &mut self,
        root: &str,
    ) -> Result<(), PlatformFileInventoryError> {
        if self.directory_exists(root)? {
            self.collect_bundle_directory(root, false)?;
        }
        Ok(())
    }

    fn collect_bundle_directory(
        &mut self,
        root: &str,
        skip_android_bin: bool,
    ) -> Result<(), PlatformFileInventoryError> {
        let mut pending = vec![(root.to_owned(), 0usize)];
        while let Some((directory, depth)) = pending.pop() {
            self.record_directory_visit(&directory)?;

            for path in self.top_level_files(&directory)? {
                if self.probe_bundle_header(&path)? {
                    let bundle_name = file_stem(file_name(&path)).to_lowercase();
                    self.register(bundle_name, path);
                }
            }

            for child in self.top_level_directories(&directory)? {
                if skip_android_bin && depth == 0 && file_name(&child) == "bin" {
                    continue;
                }
                let child_depth = depth.checked_add(1).ok_or_else(|| {
                    PlatformFileInventoryError::new(
                        Some(child.clone()),
                        PlatformFileInventoryErrorKind::DirectoryDepthOverflow,
                    )
                })?;
                if child_depth > self.limits.max_directory_depth {
                    return Err(PlatformFileInventoryError::new(
                        Some(child.clone()),
                        PlatformFileInventoryErrorKind::DirectoryDepthLimitExceeded {
                            depth: child_depth,
                            limit: self.limits.max_directory_depth,
                        },
                    ));
                }
                pending.push((child, child_depth));
            }
        }
        Ok(())
    }

    fn probe_bundle_header(&mut self, path: &str) -> Result<bool, PlatformFileInventoryError> {
        if !self.inspected_paths.insert(path.to_owned()) {
            return Ok(false);
        }
        let mut stream = self.open_file(path)?;
        let length = stream
            .length()
            .map_err(|source| PlatformFileInventoryError::filesystem(path, source))?;
        self.record_inspection(path, length)?;
        let prefix_len = usize::try_from(length.min(32)).unwrap_or(32);
        let mut prefix = [0u8; 32];
        stream
            .seek(SeekFrom::Start(0))
            .and_then(|_| stream.read_exact(&mut prefix[..prefix_len]))
            .map_err(|source| PlatformFileInventoryError::filesystem(path, source))?;
        Ok(matches!(
            SchemeReader::probe_raw_kind(&prefix[..prefix_len], length),
            FileKind::UnityFs | FileKind::UnityRaw | FileKind::UnityWebBundle
        ))
    }

    fn record_inspection(
        &mut self,
        path: &str,
        length: u64,
    ) -> Result<(), PlatformFileInventoryError> {
        let next_count = self.report.inspected_files.checked_add(1).ok_or_else(|| {
            PlatformFileInventoryError::new(
                Some(path.to_owned()),
                PlatformFileInventoryErrorKind::InspectedFileCountOverflow,
            )
        })?;
        if next_count > self.limits.max_inspected_files {
            return Err(PlatformFileInventoryError::new(
                Some(path.to_owned()),
                PlatformFileInventoryErrorKind::InspectedFileCountLimitExceeded {
                    count: next_count,
                    limit: self.limits.max_inspected_files,
                },
            ));
        }
        if length > self.limits.max_single_file_bytes {
            return Err(PlatformFileInventoryError::new(
                Some(path.to_owned()),
                PlatformFileInventoryErrorKind::SingleFileLimitExceeded {
                    size: length,
                    limit: self.limits.max_single_file_bytes,
                },
            ));
        }
        let inspected_bytes = self
            .report
            .inspected_bytes
            .checked_add(length)
            .ok_or_else(|| {
                PlatformFileInventoryError::new(
                    Some(path.to_owned()),
                    PlatformFileInventoryErrorKind::InspectedByteCountOverflow,
                )
            })?;
        if inspected_bytes > self.limits.max_total_inspected_bytes {
            return Err(PlatformFileInventoryError::new(
                Some(path.to_owned()),
                PlatformFileInventoryErrorKind::TotalInspectionLimitExceeded {
                    size: inspected_bytes,
                    limit: self.limits.max_total_inspected_bytes,
                },
            ));
        }
        self.report.inspected_files = next_count;
        self.report.inspected_bytes = inspected_bytes;
        Ok(())
    }

    fn top_level_files(&self, path: &str) -> Result<Vec<String>, PlatformFileInventoryError> {
        self.validate_path(path)?;
        let paths = self
            .filesystem
            .enumerate_files(Some(path), "*", SearchOption::TopDirectoryOnly)
            .map_err(|source| PlatformFileInventoryError::filesystem(path, source))?;
        for file_path in &paths {
            self.validate_path(file_path)?;
        }
        Ok(paths)
    }

    fn top_level_directories(&self, path: &str) -> Result<Vec<String>, PlatformFileInventoryError> {
        self.validate_path(path)?;
        let paths = self
            .filesystem
            .enumerate_directories(Some(path), "*", SearchOption::TopDirectoryOnly)
            .map_err(|source| PlatformFileInventoryError::filesystem(path, source))?;
        for directory_path in &paths {
            self.validate_path(directory_path)?;
        }
        Ok(paths)
    }

    fn file_exists(&self, path: &str) -> Result<bool, PlatformFileInventoryError> {
        self.validate_path(path)?;
        self.filesystem
            .file_exists(Some(path))
            .map_err(|source| PlatformFileInventoryError::filesystem(path, source))
    }

    fn directory_exists(&self, path: &str) -> Result<bool, PlatformFileInventoryError> {
        self.validate_path(path)?;
        self.filesystem
            .directory_exists(Some(path))
            .map_err(|source| PlatformFileInventoryError::filesystem(path, source))
    }

    fn open_file(
        &self,
        path: &str,
    ) -> Result<crate::io::stream::SmartStream, PlatformFileInventoryError> {
        self.validate_path(path)?;
        self.filesystem
            .open_file(path)
            .map_err(|source| PlatformFileInventoryError::filesystem(path, source))
    }

    fn validate_path(&self, path: &str) -> Result<(), PlatformFileInventoryError> {
        if path.len() > self.limits.max_path_bytes {
            return Err(PlatformFileInventoryError::new(
                Some(path.to_owned()),
                PlatformFileInventoryErrorKind::PathTooLong {
                    byte_count: path.len(),
                    limit: self.limits.max_path_bytes,
                },
            ));
        }
        if path
            .replace('\\', "/")
            .split('/')
            .any(|part| matches!(part, "." | ".."))
        {
            return Err(PlatformFileInventoryError::new(
                Some(path.to_owned()),
                PlatformFileInventoryErrorKind::InvalidPath,
            ));
        }
        Ok(())
    }

    fn register(&mut self, name: String, path: String) {
        self.registered_files.insert((name, path));
    }
}

fn is_primary_engine_file(name: &str) -> bool {
    matches!(
        name,
        "mainData" | "globalgamemanagers" | "globalgamemanagers.assets" | "resources.assets"
    ) || name
        .strip_prefix("level")
        .is_some_and(is_number_with_optional_split_zero)
        || name
            .strip_prefix("sharedassets")
            .is_some_and(is_shared_assets_number)
}

fn managed_path(structure: &PlatformStructure) -> Option<String> {
    match structure.platform {
        PlatformKind::Windows
        | PlatformKind::Linux
        | PlatformKind::Mac
        | PlatformKind::Android
        | PlatformKind::Ios
        | PlatformKind::Switch
        | PlatformKind::Ps4
        | PlatformKind::WiiU => structure
            .game_data_path
            .as_deref()
            .map(|path| join_path(path, "Managed")),
        PlatformKind::WindowsPhone => Some(structure.root_path.clone()),
        PlatformKind::WebGl | PlatformKind::WebPlayer => None,
    }
}

fn is_number_with_optional_split_zero(suffix: &str) -> bool {
    let digits = suffix.strip_suffix(".split0").unwrap_or(suffix);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_shared_assets_number(suffix: &str) -> bool {
    let Some((digits, tail)) = suffix.split_once(".assets") else {
        return false;
    };
    (tail.is_empty() || tail == ".split0")
        && !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or_default()
}

fn file_stem(name: &str) -> &str {
    name.rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(stem, _)| stem)
        .unwrap_or(name)
}

fn join_path(root: &str, child: &str) -> String {
    if root == "/" {
        format!("/{child}")
    } else {
        format!("{}/{child}", root.trim_end_matches('/'))
    }
}

#[derive(Debug)]
pub struct PlatformFileInventoryError {
    pub virtual_path: Option<String>,
    pub kind: PlatformFileInventoryErrorKind,
}

impl PlatformFileInventoryError {
    fn new(virtual_path: Option<String>, kind: PlatformFileInventoryErrorKind) -> Self {
        Self { virtual_path, kind }
    }

    fn filesystem(path: &str, source: io::Error) -> Self {
        Self::new(
            Some(path.to_owned()),
            PlatformFileInventoryErrorKind::FileSystem(source),
        )
    }
}

#[derive(Debug)]
pub enum PlatformFileInventoryErrorKind {
    InvalidFileName,
    InvalidPath,
    DataPathNotFound,
    AndroidAssetsPathNotFound,
    PathTooLong { byte_count: usize, limit: usize },
    InspectedFileCountOverflow,
    InspectedFileCountLimitExceeded { count: usize, limit: usize },
    SingleFileLimitExceeded { size: u64, limit: u64 },
    InspectedByteCountOverflow,
    TotalInspectionLimitExceeded { size: u64, limit: u64 },
    DirectoryCountOverflow,
    DirectoryCountLimitExceeded { count: usize, limit: usize },
    DirectoryDepthOverflow,
    DirectoryDepthLimitExceeded { depth: usize, limit: usize },
    AssemblyCountOverflow,
    AssemblyCountLimitExceeded { count: usize, limit: usize },
    FileSystem(io::Error),
}

impl Display for PlatformFileInventoryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match &self.virtual_path {
            Some(path) => write!(
                formatter,
                "platform file inventory failed at '{path}': {}",
                self.kind
            ),
            None => write!(formatter, "platform file inventory failed: {}", self.kind),
        }
    }
}

impl Display for PlatformFileInventoryErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFileName => formatter.write_str("file has no valid logical name"),
            Self::InvalidPath => formatter.write_str("path contains a traversal component"),
            Self::DataPathNotFound => {
                formatter.write_str("platform DataPath directory was not found")
            }
            Self::AndroidAssetsPathNotFound => {
                formatter.write_str("Android assets directory was not found")
            }
            Self::PathTooLong { byte_count, limit } => {
                write!(
                    formatter,
                    "path has {byte_count} bytes, exceeding limit {limit}"
                )
            }
            Self::InspectedFileCountOverflow => {
                formatter.write_str("inspected file count overflows")
            }
            Self::InspectedFileCountLimitExceeded { count, limit } => write!(
                formatter,
                "inspected file count {count} exceeds limit {limit}"
            ),
            Self::SingleFileLimitExceeded { size, limit } => {
                write!(formatter, "file has {size} bytes, exceeding limit {limit}")
            }
            Self::InspectedByteCountOverflow => {
                formatter.write_str("inspected byte count overflows")
            }
            Self::TotalInspectionLimitExceeded { size, limit } => {
                write!(formatter, "inspected {size} bytes, exceeding limit {limit}")
            }
            Self::DirectoryCountOverflow => {
                formatter.write_str("traversed directory count overflows")
            }
            Self::DirectoryCountLimitExceeded { count, limit } => write!(
                formatter,
                "traversed directory count {count} exceeds limit {limit}"
            ),
            Self::DirectoryDepthOverflow => formatter.write_str("directory depth overflows"),
            Self::DirectoryDepthLimitExceeded { depth, limit } => {
                write!(formatter, "directory depth {depth} exceeds limit {limit}")
            }
            Self::AssemblyCountOverflow => formatter.write_str("assembly count overflows"),
            Self::AssemblyCountLimitExceeded { count, limit } => {
                write!(formatter, "assembly count {count} exceeds limit {limit}")
            }
            Self::FileSystem(source) => write!(formatter, "{source}"),
        }
    }
}

impl Error for PlatformFileInventoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            PlatformFileInventoryErrorKind::FileSystem(source) => Some(source),
            _ => None,
        }
    }
}

impl Error for PlatformFileInventoryErrorKind {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FileSystem(source) => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct PlatformWorkspaceLoadError {
    pub kind: PlatformWorkspaceLoadErrorKind,
}

impl PlatformWorkspaceLoadError {
    fn new(kind: PlatformWorkspaceLoadErrorKind) -> Self {
        Self { kind }
    }
}

#[derive(Debug)]
pub enum PlatformWorkspaceLoadErrorKind {
    NoSerializedFileInputs,
    Inventory(Box<PlatformFileInventoryError>),
    Workspace(Box<SerializedFileWorkspaceError>),
    FileSystem {
        virtual_path: String,
        source: Box<io::Error>,
    },
    Scheme {
        virtual_path: String,
        source: Box<SchemeError>,
    },
    ContainerMount {
        virtual_path: String,
        source: Box<ContainerMountError>,
    },
    UnexpectedContainerLeaf {
        virtual_path: String,
        kind: FileKind,
    },
    InitialContainerCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    InitialContainerInputByteCountOverflow,
    InitialContainerInputLimitExceeded {
        byte_count: u64,
        limit: u64,
    },
    InitialContainerEntryCountOverflow,
    InitialContainerEntryLimitExceeded {
        count: usize,
        limit: usize,
    },
    InitialContainerOutputByteCountOverflow,
    InitialContainerOutputLimitExceeded {
        byte_count: u64,
        limit: u64,
    },
    InitialRegistryCountOverflow,
    InitialRegistryCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    InitialSerializedFileCountOverflow,
    InitialSerializedFileCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    ContainerNamespaceExhausted,
}

impl Display for PlatformWorkspaceLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "platform workspace load failed: {}", self.kind)
    }
}

impl Display for PlatformWorkspaceLoadErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSerializedFileInputs => formatter.write_str(
                "no source-named or container-discovered SerializedFile inputs were found",
            ),
            Self::Inventory(error) => write!(formatter, "{error}"),
            Self::Workspace(error) => write!(formatter, "{error}"),
            Self::FileSystem {
                virtual_path,
                source,
            } => {
                write!(formatter, "VFS input '{virtual_path}' failed: {source}")
            }
            Self::Scheme {
                virtual_path,
                source,
            } => {
                write!(
                    formatter,
                    "scheme dispatch failed for '{virtual_path}': {source}"
                )
            }
            Self::ContainerMount {
                virtual_path,
                source,
            } => {
                write!(
                    formatter,
                    "container '{virtual_path}' could not be expanded: {source}"
                )
            }
            Self::UnexpectedContainerLeaf { virtual_path, kind } => write!(
                formatter,
                "container leaf '{virtual_path}' has unexpected scheme {kind:?}"
            ),
            Self::InitialContainerCountLimitExceeded { count, limit } => write!(
                formatter,
                "initial container count {count} exceeds limit {limit}"
            ),
            Self::InitialContainerInputByteCountOverflow => {
                formatter.write_str("initial container input byte count overflows")
            }
            Self::InitialContainerInputLimitExceeded { byte_count, limit } => write!(
                formatter,
                "initial container inputs use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::InitialContainerEntryCountOverflow => {
                formatter.write_str("initial container entry count overflows")
            }
            Self::InitialContainerEntryLimitExceeded { count, limit } => write!(
                formatter,
                "initial container entry count {count} exceeds limit {limit}"
            ),
            Self::InitialContainerOutputByteCountOverflow => {
                formatter.write_str("initial container output byte count overflows")
            }
            Self::InitialContainerOutputLimitExceeded { byte_count, limit } => write!(
                formatter,
                "initial container outputs use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::InitialRegistryCountOverflow => {
                formatter.write_str("initial dependency registry count overflows")
            }
            Self::InitialRegistryCountLimitExceeded { count, limit } => write!(
                formatter,
                "initial dependency registry count {count} exceeds limit {limit}"
            ),
            Self::InitialSerializedFileCountOverflow => {
                formatter.write_str("initial SerializedFile count overflows")
            }
            Self::InitialSerializedFileCountLimitExceeded { count, limit } => write!(
                formatter,
                "initial SerializedFile count {count} exceeds limit {limit}"
            ),
            Self::ContainerNamespaceExhausted => {
                formatter.write_str("no collision-free initial container namespace was available")
            }
        }
    }
}

impl Error for PlatformWorkspaceLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            PlatformWorkspaceLoadErrorKind::Inventory(error) => Some(error),
            PlatformWorkspaceLoadErrorKind::Workspace(error) => Some(error.as_ref()),
            PlatformWorkspaceLoadErrorKind::NoSerializedFileInputs
            | PlatformWorkspaceLoadErrorKind::InitialContainerCountLimitExceeded { .. }
            | PlatformWorkspaceLoadErrorKind::InitialContainerInputByteCountOverflow
            | PlatformWorkspaceLoadErrorKind::InitialContainerInputLimitExceeded { .. }
            | PlatformWorkspaceLoadErrorKind::InitialContainerEntryCountOverflow
            | PlatformWorkspaceLoadErrorKind::InitialContainerEntryLimitExceeded { .. }
            | PlatformWorkspaceLoadErrorKind::InitialContainerOutputByteCountOverflow
            | PlatformWorkspaceLoadErrorKind::InitialContainerOutputLimitExceeded { .. }
            | PlatformWorkspaceLoadErrorKind::InitialRegistryCountOverflow
            | PlatformWorkspaceLoadErrorKind::InitialRegistryCountLimitExceeded { .. }
            | PlatformWorkspaceLoadErrorKind::InitialSerializedFileCountOverflow
            | PlatformWorkspaceLoadErrorKind::InitialSerializedFileCountLimitExceeded { .. }
            | PlatformWorkspaceLoadErrorKind::ContainerNamespaceExhausted
            | PlatformWorkspaceLoadErrorKind::UnexpectedContainerLeaf { .. } => None,
            PlatformWorkspaceLoadErrorKind::FileSystem { source, .. } => Some(source),
            PlatformWorkspaceLoadErrorKind::Scheme { source, .. } => Some(source),
            PlatformWorkspaceLoadErrorKind::ContainerMount { source, .. } => Some(source),
        }
    }
}

impl Error for PlatformWorkspaceLoadErrorKind {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Inventory(error) => Some(error),
            Self::Workspace(error) => Some(error.as_ref()),
            Self::NoSerializedFileInputs
            | Self::InitialContainerCountLimitExceeded { .. }
            | Self::InitialContainerInputByteCountOverflow
            | Self::InitialContainerInputLimitExceeded { .. }
            | Self::InitialContainerEntryCountOverflow
            | Self::InitialContainerEntryLimitExceeded { .. }
            | Self::InitialContainerOutputByteCountOverflow
            | Self::InitialContainerOutputLimitExceeded { .. }
            | Self::InitialRegistryCountOverflow
            | Self::InitialRegistryCountLimitExceeded { .. }
            | Self::InitialSerializedFileCountOverflow
            | Self::InitialSerializedFileCountLimitExceeded { .. }
            | Self::ContainerNamespaceExhausted
            | Self::UnexpectedContainerLeaf { .. } => None,
            Self::FileSystem { source, .. } => Some(source),
            Self::Scheme { source, .. } => Some(source),
            Self::ContainerMount { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_primary_engine_file, PlatformFileInventoryErrorKind, PlatformFileInventoryLimits,
        PlatformFileInventoryOptions, ScriptingBackend,
    };
    use crate::io::filesystem::VirtualFileSystem;
    use crate::io::platform_structure::{PlatformKind, PlatformStructure};

    fn structure(
        platform: PlatformKind,
        root: &str,
        data_paths: &[&str],
        game_data_path: Option<&str>,
        name: Option<&str>,
    ) -> PlatformStructure {
        PlatformStructure {
            platform,
            selected_path: root.to_owned(),
            consumed_paths: vec![root.to_owned()],
            root_path: root.to_owned(),
            name: name.map(str::to_owned),
            game_data_path: game_data_path.map(str::to_owned),
            data_paths: data_paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    fn make_directory(filesystem: &mut VirtualFileSystem, path: &str) {
        filesystem.create_directory(Some(path)).unwrap();
    }

    fn write_file(filesystem: &mut VirtualFileSystem, path: &str, bytes: &[u8]) {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        let mut stream = filesystem.create_file(path).unwrap();
        use std::io::Write;
        stream.write_all(bytes).unwrap();
    }

    fn bundle_header() -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[..7].copy_from_slice(b"UnityFS");
        bytes
    }

    #[test]
    fn collects_compressed_and_primary_files_in_data_paths() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/game/Game_Data");
        for name in [
            "data.unity3d",
            "datapack.unity3d",
            "mainData",
            "globalgamemanagers",
            "globalgamemanagers.assets",
            "resources.assets",
            "level2.split0",
            "sharedassets10.assets.split0",
            "unrelated.assets",
        ] {
            write_file(&mut filesystem, &format!("/game/Game_Data/{name}"), b"x");
        }
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Game_Data"],
            Some("/game/Game_Data"),
            Some("Game"),
        );

        let inventory = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        let entries: Vec<_> = inventory
            .registered_files
            .iter()
            .map(|entry| (entry.file_name.as_str(), entry.virtual_path.as_str()))
            .collect();
        assert_eq!(entries.len(), 8);
        assert!(entries.contains(&("data.unity3d", "/game/Game_Data/data.unity3d")));
        assert!(entries.contains(&("level2", "/game/Game_Data/level2")));
        assert!(entries.contains(&(
            "sharedassets10.assets",
            "/game/Game_Data/sharedassets10.assets"
        )));
        assert_eq!(inventory.primary_file_inputs.len(), 6);
        assert!(!inventory
            .primary_file_inputs
            .iter()
            .any(|entry| entry.file_name == "data.unity3d"));
        assert!(!entries.iter().any(|(name, _)| *name == "unrelated.assets"));
    }

    #[test]
    fn collects_streaming_bundles_and_respects_skip_option() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/game/Game_Data/StreamingAssets/Nested");
        write_file(
            &mut filesystem,
            "/game/Game_Data/StreamingAssets/root.bundle",
            &bundle_header(),
        );
        write_file(
            &mut filesystem,
            "/game/Game_Data/StreamingAssets/Nested/Avatar.unity3d",
            &bundle_header(),
        );
        write_file(
            &mut filesystem,
            "/game/Game_Data/StreamingAssets/readme.txt",
            b"not a bundle",
        );
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Game_Data"],
            Some("/game/Game_Data"),
            Some("Game"),
        );

        let included = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert!(included
            .registered_files
            .iter()
            .any(|entry| entry.file_name == "root"));
        assert!(included
            .registered_files
            .iter()
            .any(|entry| entry.file_name == "avatar"));
        assert_eq!(included.report.inspected_files, 3);

        let skipped = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert!(skipped.registered_files.is_empty());
        assert_eq!(skipped.report.inspected_files, 0);
    }

    #[test]
    fn android_asset_collection_skips_bin_subtree() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/apk/assets/bin/Data");
        make_directory(&mut filesystem, "/apk/assets/aa/sub");
        write_file(
            &mut filesystem,
            "/apk/assets/direct.bundle",
            &bundle_header(),
        );
        write_file(
            &mut filesystem,
            "/apk/assets/aa/sub/content.unity3d",
            &bundle_header(),
        );
        write_file(
            &mut filesystem,
            "/apk/assets/bin/Data/not-collected.bundle",
            &bundle_header(),
        );
        let structure = structure(
            PlatformKind::Android,
            "/apk",
            &["/apk/assets/bin/Data"],
            Some("/apk/assets/bin/Data"),
            None,
        );

        let inventory = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        let names: Vec<_> = inventory
            .registered_files
            .iter()
            .map(|entry| entry.file_name.as_str())
            .collect();
        assert_eq!(names, ["content", "direct"]);
    }

    #[test]
    fn missing_platform_data_path_is_reported() {
        let filesystem = VirtualFileSystem::new();
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Game_Data"],
            Some("/game/Game_Data"),
            Some("Game"),
        );
        let error = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            PlatformFileInventoryErrorKind::DataPathNotFound
        ));
        assert_eq!(error.virtual_path.as_deref(), Some("/game/Game_Data"));
    }

    #[test]
    fn workspace_load_reports_when_the_structure_has_no_primary_serialized_files() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/player");
        write_file(&mut filesystem, "/player/game.unity3d", b"bundle");
        let structure = structure(
            PlatformKind::WebPlayer,
            "/player",
            &["/player"],
            None,
            Some("game"),
        );

        let error = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                super::PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            super::PlatformWorkspaceLoadErrorKind::NoSerializedFileInputs
        ));
    }

    #[test]
    fn mono_backend_collects_direct_managed_assemblies_only() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/game/Game_Data/Managed/Nested");
        write_file(
            &mut filesystem,
            "/game/Game_Data/Managed/Assembly-CSharp.dll",
            b"assembly",
        );
        write_file(
            &mut filesystem,
            "/game/Game_Data/Managed/Nested/NotCollected.dll",
            b"nested assembly",
        );
        write_file(
            &mut filesystem,
            "/game/Game_Data/Managed/notice.txt",
            b"text",
        );
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Game_Data"],
            Some("/game/Game_Data"),
            Some("Game"),
        );

        let inventory = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert_eq!(inventory.scripting_backend, ScriptingBackend::Mono);
        assert_eq!(inventory.assemblies.len(), 1);
        assert_eq!(inventory.assemblies[0].file_name, "Assembly-CSharp.dll");
        assert_eq!(
            inventory.assemblies[0].virtual_path,
            "/game/Game_Data/Managed/Assembly-CSharp.dll"
        );

        let error = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                },
                PlatformFileInventoryLimits {
                    max_assemblies: 0,
                    ..PlatformFileInventoryLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            PlatformFileInventoryErrorKind::AssemblyCountLimitExceeded { count: 1, limit: 0 }
        ));
    }

    #[test]
    fn backend_falls_back_to_mono_when_either_il2cpp_marker_is_missing() {
        for (assembly_marker, metadata_marker) in [(true, false), (false, true)] {
            let mut filesystem = VirtualFileSystem::new();
            if assembly_marker {
                write_file(&mut filesystem, "/game/GameAssembly.dll", b"player marker");
            }
            if metadata_marker {
                write_file(
                    &mut filesystem,
                    "/game/Game_Data/il2cpp_data/Metadata/global-metadata.dat",
                    b"metadata marker",
                );
            }
            write_file(
                &mut filesystem,
                "/game/Game_Data/Managed/Assembly-CSharp.dll",
                b"managed assembly",
            );
            let structure = structure(
                PlatformKind::Windows,
                "/game",
                &["/game/Game_Data"],
                Some("/game/Game_Data"),
                Some("Game"),
            );

            let inventory = structure
                .collect_game_files(
                    &filesystem,
                    PlatformFileInventoryOptions {
                        skip_streaming_assets: true,
                    },
                    PlatformFileInventoryLimits::default(),
                )
                .unwrap();
            assert_eq!(inventory.scripting_backend, ScriptingBackend::Mono);
            assert_eq!(inventory.assemblies.len(), 1);
        }
    }

    #[test]
    fn il2cpp_layout_markers_take_precedence_over_managed_dlls() {
        let cases = [
            (
                structure(
                    PlatformKind::Windows,
                    "/win",
                    &["/win/Game_Data"],
                    Some("/win/Game_Data"),
                    Some("Game"),
                ),
                "/win/GameAssembly.dll",
                "/win/Game_Data/il2cpp_data/Metadata/global-metadata.dat",
                "/win/Game_Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Linux,
                    "/linux",
                    &["/linux/Game_Data"],
                    Some("/linux/Game_Data"),
                    Some("Game"),
                ),
                "/linux/GameAssembly.so",
                "/linux/Game_Data/il2cpp_data/Metadata/global-metadata.dat",
                "/linux/Game_Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Mac,
                    "/Game.app",
                    &[
                        "/Game.app/Contents/Resources/Data",
                        "/Game.app/Contents/Resources",
                    ],
                    Some("/Game.app/Contents/Resources/Data"),
                    Some("Game"),
                ),
                "/Game.app/Contents/Frameworks/GameAssembly.dylib",
                "/Game.app/Contents/Resources/Data/il2cpp_data/Metadata/global-metadata.dat",
                "/Game.app/Contents/Resources/Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Android,
                    "/apk",
                    &["/apk/assets/bin/Data"],
                    Some("/apk/assets/bin/Data"),
                    None,
                ),
                "/apk/lib/arm64-v8a/libil2cpp.so",
                "/apk/assets/bin/Data/Managed/Metadata/global-metadata.dat",
                "/apk/assets/bin/Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Ios,
                    "/ipa",
                    &["/ipa/Payload/Game.app/Data"],
                    Some("/ipa/Payload/Game.app/Data"),
                    Some("Game"),
                ),
                "/ipa/Payload/Game.app/Game",
                "/ipa/Payload/Game.app/Data/Managed/Metadata/global-metadata.dat",
                "/ipa/Payload/Game.app/Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Switch,
                    "/switch",
                    &["/switch/romfs/Data"],
                    Some("/switch/romfs/Data"),
                    Some("switch"),
                ),
                "/switch/exefs/main",
                "/switch/romfs/Data/Managed/Metadata/global-metadata.dat",
                "/switch/romfs/Data/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::Ps4,
                    "/ps4",
                    &["/ps4/Media"],
                    Some("/ps4/Media"),
                    Some("ps4"),
                ),
                "/ps4/Media/Modules/Il2CppUserAssemblies.prx",
                "/ps4/Media/Metadata/global-metadata.dat",
                "/ps4/Media/Managed/Assembly-CSharp.dll",
            ),
            (
                structure(
                    PlatformKind::WindowsPhone,
                    "/wp",
                    &["/wp/Data"],
                    Some("/wp/Data"),
                    Some("wp"),
                ),
                "/wp/GameAssembly.dll",
                "/wp/Data/il2cpp_data/Metadata/global-metadata.dat",
                "/wp/Assembly-CSharp.dll",
            ),
        ];

        for (platform_structure, game_assembly, metadata, managed_assembly) in cases {
            let mut filesystem = VirtualFileSystem::new();
            write_file(&mut filesystem, game_assembly, b"native player marker");
            write_file(&mut filesystem, metadata, b"metadata marker");
            write_file(&mut filesystem, managed_assembly, b"managed assembly");
            let inventory = platform_structure
                .collect_game_files(
                    &filesystem,
                    PlatformFileInventoryOptions {
                        skip_streaming_assets: true,
                    },
                    PlatformFileInventoryLimits::default(),
                )
                .unwrap();
            assert_eq!(inventory.scripting_backend, ScriptingBackend::Il2Cpp);
            assert!(inventory.assemblies.is_empty());
        }
    }

    #[test]
    fn wiiu_remains_mono_even_when_other_platform_il2cpp_markers_exist() {
        let mut filesystem = VirtualFileSystem::new();
        write_file(
            &mut filesystem,
            "/wii/exefs/main",
            b"not a supported backend marker",
        );
        write_file(
            &mut filesystem,
            "/wii/content/Data/Metadata/global-metadata.dat",
            b"metadata",
        );
        write_file(
            &mut filesystem,
            "/wii/content/Data/Managed/Assembly-CSharp.dll",
            b"assembly",
        );
        let structure = structure(
            PlatformKind::WiiU,
            "/wii",
            &["/wii/content/Data"],
            Some("/wii/content/Data"),
            Some("wii"),
        );

        let inventory = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert_eq!(inventory.scripting_backend, ScriptingBackend::Mono);
        assert_eq!(inventory.assemblies.len(), 1);
    }

    #[test]
    fn webgl_data_and_webplayer_bundle_are_registered_by_platform_rule() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/web/Build");
        write_file(&mut filesystem, "/web/index.html", b"html");
        write_file(&mut filesystem, "/web/Build/site.data.unityweb", b"data");
        let webgl = structure(
            PlatformKind::WebGl,
            "/web",
            &["/web", "/web/Build"],
            Some("/web"),
            Some("web"),
        );
        let inventory = webgl
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: true,
                },
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert!(inventory.registered_files.iter().any(|entry| {
            entry.file_name == "site" && entry.virtual_path == "/web/Build/site.data.unityweb"
        }));

        make_directory(&mut filesystem, "/player");
        write_file(&mut filesystem, "/player/game.html", b"html");
        write_file(
            &mut filesystem,
            "/player/game.unity3d",
            b"not checked by source",
        );
        let web_player = structure(
            PlatformKind::WebPlayer,
            "/player",
            &["/player"],
            None,
            Some("game"),
        );
        let player_inventory = web_player
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits::default(),
            )
            .unwrap();
        assert!(player_inventory.registered_files.iter().any(|entry| {
            entry.file_name == "game" && entry.virtual_path == "/player/game.unity3d"
        }));
    }

    #[test]
    fn bundle_probe_obeys_scan_and_traversal_limits() {
        let mut filesystem = VirtualFileSystem::new();
        make_directory(&mut filesystem, "/game/Data/StreamingAssets/Nested");
        write_file(
            &mut filesystem,
            "/game/Data/StreamingAssets/a.bundle",
            &bundle_header(),
        );
        let structure = structure(
            PlatformKind::Windows,
            "/game",
            &["/game/Data"],
            Some("/game/Data"),
            Some("Game"),
        );
        let error = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits {
                    max_inspected_files: 0,
                    ..PlatformFileInventoryLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            PlatformFileInventoryErrorKind::InspectedFileCountLimitExceeded { count: 1, limit: 0 }
        ));

        let error = structure
            .collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits {
                    max_directory_depth: 0,
                    ..PlatformFileInventoryLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            PlatformFileInventoryErrorKind::DirectoryDepthLimitExceeded { depth: 1, limit: 0 }
        ));
    }

    #[test]
    fn primary_file_name_matching_follows_source_patterns() {
        for name in [
            "mainData",
            "globalgamemanagers",
            "globalgamemanagers.assets",
            "resources.assets",
            "level0",
            "level42.split0",
            "sharedassets2.assets",
            "sharedassets2.assets.split0",
        ] {
            assert!(is_primary_engine_file(name), "{name}");
        }
        for name in [
            "MainData",
            "level",
            "levelx",
            "level1.split1",
            "sharedassets.assets",
            "sharedassets1.assets.split1",
        ] {
            assert!(!is_primary_engine_file(name), "{name}");
        }
    }
}
