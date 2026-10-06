//! Platform root and DataPath discovery for mounted Unity game layouts.
//!
//! Covers the root/marker decisions per platform (including the Android APK/OBB bitmask and the
//! `assets/bin/Data` gate) plus the Android, iOS, Switch, and PS4 directory layouts. Backend
//! selection, assembly and metadata collection, and bundle gathering live in the file inventory;
//! bundle-hierarchy lookups live in the serialized-file workspace.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use crate::io::filesystem::{SearchOption, VirtualFileSystem};
use crate::io::special_names::fix_resource_path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformKind {
    Windows,
    Linux,
    Mac,
    Android,
    Ios,
    Switch,
    Ps4,
    WebGl,
    WebPlayer,
    WiiU,
    WindowsPhone,
}

pub type DesktopPlatform = PlatformKind;

/// A source-shaped platform root and ordered dependency DataPaths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformStructure {
    pub platform: PlatformKind,
    /// The selected input path that matched this structure.
    pub selected_path: String,
    /// Every selected path consumed by the structure (Android can include an OBB root).
    pub consumed_paths: Vec<String>,
    /// Normalized application root (the parent for executable and `_Data` selections).
    pub root_path: String,
    pub name: Option<String>,
    pub game_data_path: Option<String>,
    pub data_paths: Vec<String>,
}

/// Identifies which source structure supplied a platform resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformResourceSource {
    Platform,
    Mixed,
}

/// A platform-resource lookup result. Missing DataPaths are retained as diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlatformResourceLookup {
    Found {
        virtual_path: String,
        source: PlatformResourceSource,
        data_path_index: usize,
        unavailable_data_paths: Vec<String>,
    },
    Missing {
        unavailable_data_paths: Vec<String>,
    },
}

/// Searches platform DataPaths first, then the optional MixedStructure paths.
pub struct PlatformResourceProvider<'a> {
    pub platform: Option<&'a PlatformStructure>,
    pub mixed: Option<&'a PlatformStructure>,
    pub mixed_data_paths: Option<&'a [String]>,
}

impl<'a> PlatformResourceProvider<'a> {
    pub fn new(
        platform: Option<&'a PlatformStructure>,
        mixed: Option<&'a PlatformStructure>,
    ) -> Self {
        Self {
            platform,
            mixed,
            mixed_data_paths: None,
        }
    }

    /// Builds a provider from a platform structure and the DataPaths discovered by
    /// MixedGameStructure, which does not itself carry platform metadata.
    pub fn with_mixed_data_paths(
        platform: Option<&'a PlatformStructure>,
        mixed_data_paths: Option<&'a [String]>,
    ) -> Self {
        Self {
            platform,
            mixed: None,
            mixed_data_paths,
        }
    }

    /// Looks for `original_name` in each source DataPath in order. This applies the baseline
    /// provider's `FixResourcePath` behavior, then returns the first direct or split-file match.
    pub fn find_resource(
        &self,
        filesystem: &VirtualFileSystem,
        original_name: &str,
        limits: PlatformStructureLimits,
    ) -> Result<PlatformResourceLookup, PlatformResourceLookupError> {
        if original_name.len() > limits.max_path_bytes {
            return Err(PlatformResourceLookupError::PathTooLong {
                byte_count: original_name.len(),
                limit: limits.max_path_bytes,
            });
        }
        let resource_name = fix_resource_path(original_name);
        validate_resource_name(&resource_name, limits.max_path_bytes)?;
        let mut unavailable_data_paths = Vec::new();

        for (source, data_paths) in [
            (
                PlatformResourceSource::Platform,
                self.platform
                    .map(|structure| structure.data_paths.as_slice()),
            ),
            (
                PlatformResourceSource::Mixed,
                self.mixed
                    .map(|structure| structure.data_paths.as_slice())
                    .or(self.mixed_data_paths),
            ),
        ] {
            let Some(data_paths) = data_paths else {
                continue;
            };
            if data_paths.len() > limits.max_selected_paths {
                return Err(PlatformResourceLookupError::DataPathCountLimitExceeded {
                    count: data_paths.len(),
                    limit: limits.max_selected_paths,
                });
            }
            for (data_path_index, data_path) in data_paths.iter().enumerate() {
                let root = normalize_resource_root(data_path, limits.max_path_bytes)?;
                let candidate_path =
                    join_resource_path(&root, &resource_name, limits.max_path_bytes)?;
                let root_exists = filesystem.directory_exists(Some(&root)).map_err(|source| {
                    PlatformResourceLookupError::FileSystem {
                        virtual_path: root.clone(),
                        source,
                    }
                })?;
                if !root_exists {
                    push_unavailable_data_path(
                        &mut unavailable_data_paths,
                        data_path,
                        limits.max_selected_paths,
                    )?;
                    continue;
                }
                let resource_exists =
                    filesystem
                        .logical_file_exists(&candidate_path)
                        .map_err(|source| PlatformResourceLookupError::FileSystem {
                            virtual_path: candidate_path.clone(),
                            source,
                        })?;
                if resource_exists {
                    return Ok(PlatformResourceLookup::Found {
                        virtual_path: candidate_path,
                        source,
                        data_path_index,
                        unavailable_data_paths,
                    });
                }
            }
        }

        Ok(PlatformResourceLookup::Missing {
            unavailable_data_paths,
        })
    }
}

fn validate_resource_name(
    name: &str,
    maximum_bytes: usize,
) -> Result<(), PlatformResourceLookupError> {
    if name.is_empty() || name.len() > maximum_bytes {
        return Err(PlatformResourceLookupError::InvalidResourceName {
            name: name.to_owned(),
        });
    }
    if name.starts_with('/')
        || name.starts_with('\\')
        || name
            .split(['/', '\\'])
            .any(|component| matches!(component, "." | ".."))
    {
        return Err(PlatformResourceLookupError::InvalidResourceName {
            name: name.to_owned(),
        });
    }
    Ok(())
}

fn normalize_resource_root(
    root: &str,
    maximum_bytes: usize,
) -> Result<String, PlatformResourceLookupError> {
    if root.len() > maximum_bytes {
        return Err(PlatformResourceLookupError::PathTooLong {
            byte_count: root.len(),
            limit: maximum_bytes,
        });
    }
    let requested_bytes = root
        .len()
        .checked_add(1)
        .ok_or(PlatformResourceLookupError::PathSizeOverflow)?;
    let mut normalized = String::new();
    normalized.try_reserve_exact(requested_bytes).map_err(|_| {
        PlatformResourceLookupError::AllocationFailed {
            requested: requested_bytes,
        }
    })?;
    normalized.push('/');
    let mut is_first = true;
    for component in root
        .split(['/', '\\'])
        .filter(|component| !component.is_empty())
    {
        if matches!(component, "." | "..") {
            return Err(PlatformResourceLookupError::InvalidDataPath {
                data_path: root.to_owned(),
            });
        }
        if !is_first {
            normalized.push('/');
        }
        normalized.push_str(component);
        is_first = false;
    }
    Ok(normalized)
}

fn join_resource_path(
    root: &str,
    resource_name: &str,
    maximum_bytes: usize,
) -> Result<String, PlatformResourceLookupError> {
    let separator_bytes = usize::from(root != "/");
    let path_bytes = root
        .len()
        .checked_add(separator_bytes)
        .and_then(|bytes| bytes.checked_add(resource_name.len()))
        .ok_or(PlatformResourceLookupError::PathSizeOverflow)?;
    if path_bytes > maximum_bytes {
        return Err(PlatformResourceLookupError::PathTooLong {
            byte_count: path_bytes,
            limit: maximum_bytes,
        });
    }
    let mut path = String::new();
    path.try_reserve_exact(path_bytes).map_err(|_| {
        PlatformResourceLookupError::AllocationFailed {
            requested: path_bytes,
        }
    })?;
    if root == "/" {
        path.push('/');
    } else {
        path.push_str(root.trim_end_matches('/'));
        path.push('/');
    }
    path.push_str(resource_name);
    Ok(path)
}

fn push_unavailable_data_path(
    output: &mut Vec<String>,
    data_path: &str,
    maximum_paths: usize,
) -> Result<(), PlatformResourceLookupError> {
    if output.len() >= maximum_paths {
        return Err(
            PlatformResourceLookupError::UnavailableDataPathCountLimitExceeded {
                count: output.len().saturating_add(1),
                limit: maximum_paths,
            },
        );
    }
    output
        .try_reserve(1)
        .map_err(|_| PlatformResourceLookupError::AllocationFailed {
            requested: output.len().saturating_add(1),
        })?;
    let mut path = String::new();
    path.try_reserve_exact(data_path.len()).map_err(|_| {
        PlatformResourceLookupError::AllocationFailed {
            requested: data_path.len(),
        }
    })?;
    path.push_str(data_path);
    output.push(path);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformStructureLimits {
    pub max_selected_paths: usize,
    pub max_path_bytes: usize,
}

impl Default for PlatformStructureLimits {
    fn default() -> Self {
        Self {
            max_selected_paths: 100_000,
            max_path_bytes: 1024 * 1024,
        }
    }
}

/// Detect one platform root in source priority order: Windows, Linux, then macOS.
///
/// This ports the root/DataPath portion of the corresponding `PlatformChecker` branches. It does
/// not collect game files, inspect assemblies, or detect mobile/console/web-player layouts.
pub fn detect_desktop_platform(
    filesystem: &VirtualFileSystem,
    selected_paths: &[String],
    limits: PlatformStructureLimits,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    detect_platforms(
        filesystem,
        selected_paths,
        limits,
        &[
            PlatformKind::Windows,
            PlatformKind::Linux,
            PlatformKind::Mac,
        ],
    )
}

/// Detect a local platform structure in `PlatformChecker` order.
pub fn detect_platform_structure(
    filesystem: &VirtualFileSystem,
    selected_paths: &[String],
    limits: PlatformStructureLimits,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    detect_platforms(
        filesystem,
        selected_paths,
        limits,
        &[
            PlatformKind::Windows,
            PlatformKind::Linux,
            PlatformKind::Mac,
            PlatformKind::Android,
            PlatformKind::Ios,
            PlatformKind::Switch,
            PlatformKind::Ps4,
            PlatformKind::WebGl,
            PlatformKind::WebPlayer,
            PlatformKind::WiiU,
            PlatformKind::WindowsPhone,
        ],
    )
}

fn detect_platforms(
    filesystem: &VirtualFileSystem,
    selected_paths: &[String],
    limits: PlatformStructureLimits,
    priority: &[PlatformKind],
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    if selected_paths.len() > limits.max_selected_paths {
        return Err(PlatformStructureError::new(
            None,
            PlatformStructureErrorKind::SelectedPathCountLimitExceeded {
                count: selected_paths.len(),
                limit: limits.max_selected_paths,
            },
        ));
    }

    let mut normalized = Vec::new();
    normalized
        .try_reserve_exact(selected_paths.len())
        .map_err(|_| {
            PlatformStructureError::new(
                None,
                PlatformStructureErrorKind::AllocationFailed {
                    requested: selected_paths.len(),
                },
            )
        })?;
    for path in selected_paths {
        normalized.push(normalize_selected_path(path, limits.max_path_bytes)?);
    }

    for platform in priority {
        if *platform == PlatformKind::Android {
            if let Some(structure) = detect_android(filesystem, &normalized)? {
                return Ok(Some(structure));
            }
            continue;
        }
        for selected_path in &normalized {
            let detected = match platform {
                PlatformKind::Windows => detect_windows(filesystem, selected_path)?,
                PlatformKind::Linux => detect_linux(filesystem, selected_path)?,
                PlatformKind::Mac => detect_mac(filesystem, selected_path)?,
                PlatformKind::Ios => detect_ios(filesystem, selected_path)?,
                PlatformKind::Switch => detect_switch(filesystem, selected_path)?,
                PlatformKind::Ps4 => detect_ps4(filesystem, selected_path)?,
                PlatformKind::WebGl => detect_webgl(filesystem, selected_path)?,
                PlatformKind::WebPlayer => detect_web_player(filesystem, selected_path)?,
                PlatformKind::WiiU => detect_wiiu(filesystem, selected_path)?,
                PlatformKind::WindowsPhone => detect_windows_phone(filesystem, selected_path)?,
                PlatformKind::Android => None,
            };
            if let Some(structure) = detected {
                return Ok(Some(structure));
            }
        }
    }
    Ok(None)
}

fn detect_windows(
    filesystem: &VirtualFileSystem,
    selected_path: &str,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    let is_executable = exists_file(filesystem, selected_path)?
        && file_name(selected_path)
            .to_ascii_lowercase()
            .ends_with(".exe");
    let is_data_directory = exists_directory(filesystem, selected_path)?
        && selected_path.ends_with("_Data")
        && windows_data_directory_sibling_exists(filesystem, selected_path)?;
    let root_path = if is_executable || is_data_directory {
        parent_path(selected_path)
    } else {
        selected_path.to_owned()
    };
    if !exists_directory(filesystem, &root_path)? {
        return Ok(None);
    }

    let Some((game_data_path, name)) = windows_game_data_directory(filesystem, &root_path)? else {
        return Ok(None);
    };
    Ok(Some(PlatformStructure {
        platform: PlatformKind::Windows,
        selected_path: selected_path.to_owned(),
        consumed_paths: vec![selected_path.to_owned()],
        root_path,
        name: Some(name),
        data_paths: vec![game_data_path.clone()],
        game_data_path: Some(game_data_path),
    }))
}

fn windows_data_directory_sibling_exists(
    filesystem: &VirtualFileSystem,
    data_path: &str,
) -> Result<bool, PlatformStructureError> {
    let folder = file_name(data_path);
    let Some(game_name) = folder.strip_suffix("_Data") else {
        return Ok(false);
    };
    let executable = join_path(&parent_path(data_path), &format!("{game_name}.exe"));
    exists_file(filesystem, &executable)
}

fn windows_game_data_directory(
    filesystem: &VirtualFileSystem,
    root_path: &str,
) -> Result<Option<(String, String)>, PlatformStructureError> {
    let files = top_level_files(filesystem, root_path)?;
    let mut executable_count = 0usize;
    let mut last_name = String::new();
    for file in &files {
        let filename = file_name(file);
        if !filename.to_ascii_lowercase().ends_with(".exe") {
            continue;
        }
        executable_count += 1;
        last_name = file_stem(filename).to_owned();
        let named_data_path = join_path(root_path, &format!("{last_name}_Data"));
        if exists_directory(filesystem, &named_data_path)? {
            return Ok(Some((named_data_path, last_name)));
        }
    }
    if executable_count == 0 {
        return Ok(None);
    }

    let fallback = join_path(root_path, "Data");
    if !exists_directory(filesystem, &fallback)? {
        return Ok(None);
    }
    let name = if executable_count == 1 {
        last_name
    } else {
        file_name(root_path).to_owned()
    };
    Ok(Some((fallback, name)))
}

fn detect_linux(
    filesystem: &VirtualFileSystem,
    selected_path: &str,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    let is_executable = exists_file(filesystem, selected_path)?
        && matches!(
            extension(file_name(selected_path)),
            "" | ".x86" | ".x64" | ".x86_64"
        );
    let is_data_directory = exists_directory(filesystem, selected_path)?
        && selected_path.ends_with("_Data")
        && linux_data_directory_sibling_exists(filesystem, selected_path)?;
    let root_path = if is_executable || is_data_directory {
        parent_path(selected_path)
    } else {
        selected_path.to_owned()
    };
    if !exists_directory(filesystem, &root_path)? {
        return Ok(None);
    }

    let Some((game_data_path, name)) = linux_game_data_directory(filesystem, &root_path)? else {
        return Ok(None);
    };
    Ok(Some(PlatformStructure {
        platform: PlatformKind::Linux,
        selected_path: selected_path.to_owned(),
        consumed_paths: vec![selected_path.to_owned()],
        root_path,
        name: Some(name),
        data_paths: vec![game_data_path.clone()],
        game_data_path: Some(game_data_path),
    }))
}

fn linux_data_directory_sibling_exists(
    filesystem: &VirtualFileSystem,
    data_path: &str,
) -> Result<bool, PlatformStructureError> {
    let folder = file_name(data_path);
    let Some(game_name) = folder.strip_suffix("_Data") else {
        return Ok(false);
    };
    for suffix in [".x86", ".x64", ".x86_64", ""] {
        let executable = join_path(&parent_path(data_path), &format!("{game_name}{suffix}"));
        if exists_file(filesystem, &executable)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn linux_game_data_directory(
    filesystem: &VirtualFileSystem,
    root_path: &str,
) -> Result<Option<(String, String)>, PlatformStructureError> {
    let files = top_level_files(filesystem, root_path)?;
    let mut executable_count = 0usize;
    let mut last_name = String::new();
    for file in &files {
        let filename = file_name(file);
        if !matches!(extension(filename), "" | ".x86" | ".x64" | ".x86_64") {
            continue;
        }
        executable_count += 1;
        last_name = file_stem(filename).to_owned();
        let named_data_path = join_path(root_path, &format!("{last_name}_Data"));
        if exists_directory(filesystem, &named_data_path)? {
            return Ok(Some((named_data_path, last_name)));
        }
    }
    if executable_count == 0 {
        return Ok(None);
    }

    let fallback = join_path(root_path, "Data");
    if !exists_directory(filesystem, &fallback)? {
        return Ok(None);
    }
    let name = if executable_count == 1 {
        last_name
    } else {
        file_name(root_path).to_owned()
    };
    Ok(Some((fallback, name)))
}

fn detect_mac(
    filesystem: &VirtualFileSystem,
    selected_path: &str,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    if !selected_path.ends_with(".app") || !exists_directory(filesystem, selected_path)? {
        return Ok(None);
    }
    let resource_path = join_path(selected_path, "Contents/Resources");
    let game_data_path = join_path(&resource_path, "Data");
    if !exists_directory(filesystem, &resource_path)?
        || !exists_directory(filesystem, &game_data_path)?
    {
        return Ok(None);
    }
    Ok(Some(PlatformStructure {
        platform: PlatformKind::Mac,
        selected_path: selected_path.to_owned(),
        consumed_paths: vec![selected_path.to_owned()],
        root_path: selected_path.to_owned(),
        name: Some(file_stem(file_name(selected_path)).to_owned()),
        game_data_path: Some(game_data_path.clone()),
        data_paths: vec![game_data_path, resource_path],
    }))
}

fn detect_android(
    filesystem: &VirtualFileSystem,
    selected_paths: &[String],
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    let mut apk_roots = Vec::new();
    let mut obb_roots = Vec::new();
    for path in selected_paths {
        if !exists_directory(filesystem, path)? {
            continue;
        }
        let marker = android_root_marker(filesystem, path)?;
        let data_path = join_path(path, "assets/bin/Data");
        if marker > 8 && exists_directory(filesystem, &data_path)? {
            apk_roots.push(path.clone());
        } else if marker == 8 && exists_directory(filesystem, &data_path)? {
            obb_roots.push(path.clone());
        }
    }
    if apk_roots.len() > 1 {
        return Err(PlatformStructureError::new(
            None,
            PlatformStructureErrorKind::AmbiguousAndroidRoots {
                role: "APK",
                count: apk_roots.len(),
            },
        ));
    }
    if obb_roots.len() > 1 {
        return Err(PlatformStructureError::new(
            None,
            PlatformStructureErrorKind::AmbiguousAndroidRoots {
                role: "OBB",
                count: obb_roots.len(),
            },
        ));
    }
    let Some(apk_root) = apk_roots.pop() else {
        return Ok(None);
    };
    let apk_data = join_path(&apk_root, "assets/bin/Data");
    let mut data_paths = vec![apk_data.clone()];
    let mut consumed_paths = vec![apk_root.clone()];
    if let Some(obb_root) = obb_roots.pop() {
        data_paths.push(join_path(&obb_root, "assets/bin/Data"));
        consumed_paths.push(obb_root);
    }
    Ok(Some(make_structure(
        PlatformKind::Android,
        &apk_root,
        consumed_paths,
        apk_root.clone(),
        None,
        Some(apk_data),
        data_paths,
    )))
}

fn android_root_marker(
    filesystem: &VirtualFileSystem,
    root_path: &str,
) -> Result<u8, PlatformStructureError> {
    let mut marker = 0u8;
    for directory in top_level_directories(filesystem, root_path)? {
        match file_name(&directory) {
            "assets" => marker |= 8,
            "META-INF" => marker |= 4,
            "lib" => marker |= 2,
            _ => {}
        }
    }
    Ok(marker)
}

/// `AndroidGameStructure` directory layout for one APK or OBB root.
///
/// Mirrors the constructor joins: `assets/bin/Data` under the root, `Resources` and `Managed`
/// under the data directory, `global-metadata.dat` under `Managed/Metadata`, and `lib` under
/// the root. `StreamingAssetsPath` and `UnityPlayerPath` stay unset on Android, like the source,
/// so they have no slots here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AndroidDataLayout {
    pub game_data_path: String,
    pub resources_path: String,
    pub managed_path: String,
    pub lib_path: String,
    pub il2cpp_metadata_path: String,
}

pub fn android_data_layout(root_path: &str) -> AndroidDataLayout {
    let game_data_path = join_path(root_path, "assets/bin/Data");
    let managed_path = join_path(&game_data_path, "Managed");
    AndroidDataLayout {
        resources_path: join_path(&game_data_path, "Resources"),
        il2cpp_metadata_path: join_path(&managed_path, "Metadata/global-metadata.dat"),
        managed_path,
        lib_path: join_path(root_path, "lib"),
        game_data_path,
    }
}

/// `iOSGameStructure` directory layout for one `.app` directory.
///
/// Mirrors the constructor joins: `Data` under the app directory, `Resources` and `Managed`
/// under it, the IL2Cpp game assembly as the app binary itself (`appPath` plus the `.app`-less
/// name), `global-metadata.dat` under `Managed/Metadata`, and `Raw` streaming assets under the
/// selected root. `UnityPlayerPath` stays unset on iOS, like the source, so it has no slot here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IosDataLayout {
    pub game_data_path: String,
    pub streaming_assets_path: String,
    pub resources_path: String,
    pub managed_path: String,
    pub il2cpp_game_assembly_path: String,
    pub il2cpp_metadata_path: String,
}

pub fn ios_data_layout(root_path: &str, app_path: &str, app_name: &str) -> IosDataLayout {
    let game_data_path = join_path(app_path, "Data");
    let managed_path = join_path(&game_data_path, "Managed");
    IosDataLayout {
        streaming_assets_path: join_path(root_path, "Raw"),
        resources_path: join_path(&game_data_path, "Resources"),
        il2cpp_game_assembly_path: join_path(app_path, app_name),
        il2cpp_metadata_path: join_path(&managed_path, "Metadata/global-metadata.dat"),
        managed_path,
        game_data_path,
    }
}

/// `SwitchGameStructure` directory layout for one title root.
///
/// Mirrors the constructor joins: `romfs/Data` under the root, `StreamingAssets`, `Resources`,
/// and `Managed` under it, the IL2Cpp game assembly as `exefs/main`, and `global-metadata.dat`
/// under `Managed/Metadata`. `UnityPlayerPath` stays unset on Switch, like the source, so it has
/// no slot here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchDataLayout {
    pub game_data_path: String,
    pub streaming_assets_path: String,
    pub resources_path: String,
    pub managed_path: String,
    pub exefs_path: String,
    pub il2cpp_game_assembly_path: String,
    pub il2cpp_metadata_path: String,
}

pub fn switch_data_layout(root_path: &str) -> SwitchDataLayout {
    let game_data_path = join_path(root_path, "romfs/Data");
    let managed_path = join_path(&game_data_path, "Managed");
    let exefs_path = join_path(root_path, "exefs");
    SwitchDataLayout {
        streaming_assets_path: join_path(&game_data_path, "StreamingAssets"),
        resources_path: join_path(&game_data_path, "Resources"),
        il2cpp_game_assembly_path: join_path(&exefs_path, "main"),
        il2cpp_metadata_path: join_path(&managed_path, "Metadata/global-metadata.dat"),
        managed_path,
        exefs_path,
        game_data_path,
    }
}

/// `PS4GameStructure` directory layout for one title root.
///
/// Mirrors the constructor joins: `Media` as the data directory (gated on `eboot.bin`),
/// `Resources` and `Managed` under it, the PS4-only `Modules` directory holding
/// `Il2CppUserAssemblies.prx`, and `global-metadata.dat` directly under `Data/Metadata` rather
/// than under `Managed`. Streaming assets and the Unity player stay unset on PS4, like the
/// source, so they have no slots here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ps4DataLayout {
    pub game_data_path: String,
    pub resources_path: String,
    pub managed_path: String,
    pub modules_path: String,
    pub il2cpp_game_assembly_path: String,
    pub il2cpp_metadata_path: String,
}

pub fn ps4_data_layout(root_path: &str) -> Ps4DataLayout {
    let game_data_path = join_path(root_path, "Media");
    let managed_path = join_path(&game_data_path, "Managed");
    let modules_path = join_path(&game_data_path, "Modules");
    Ps4DataLayout {
        resources_path: join_path(&game_data_path, "Resources"),
        il2cpp_game_assembly_path: join_path(&modules_path, "Il2CppUserAssemblies.prx"),
        il2cpp_metadata_path: join_path(&game_data_path, "Metadata/global-metadata.dat"),
        managed_path,
        modules_path,
        game_data_path,
    }
}

fn detect_ios(
    filesystem: &VirtualFileSystem,
    selected_path: &str,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    if !exists_directory(filesystem, selected_path)? {
        return Ok(None);
    }
    let payload_path = join_path(selected_path, "Payload");
    if !exists_directory(filesystem, &payload_path)? {
        return Ok(None);
    }
    for app_path in top_level_directories(filesystem, &payload_path)? {
        let app_name = file_name(&app_path);
        if !app_name.ends_with(".app") {
            continue;
        }
        let game_data_path = join_path(&app_path, "Data");
        if exists_directory(filesystem, &game_data_path)? {
            return Ok(Some(make_structure(
                PlatformKind::Ios,
                selected_path,
                vec![selected_path.to_owned()],
                selected_path.to_owned(),
                Some(file_stem(app_name).to_owned()),
                Some(game_data_path.clone()),
                vec![game_data_path],
            )));
        }
    }
    Ok(None)
}

fn detect_ps4(
    filesystem: &VirtualFileSystem,
    selected_path: &str,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    if !exists_directory(filesystem, selected_path)? {
        return Ok(None);
    }
    let has_executable = top_level_files(filesystem, selected_path)?
        .iter()
        .any(|path| file_name(path) == "eboot.bin");
    let layout = ps4_data_layout(selected_path);
    if !has_executable || !exists_directory(filesystem, &layout.game_data_path)? {
        return Ok(None);
    }
    Ok(Some(make_structure(
        PlatformKind::Ps4,
        selected_path,
        vec![selected_path.to_owned()],
        selected_path.to_owned(),
        Some(file_name(selected_path).to_owned()),
        Some(layout.game_data_path.clone()),
        vec![layout.game_data_path],
    )))
}

fn detect_switch(
    filesystem: &VirtualFileSystem,
    selected_path: &str,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    let layout = switch_data_layout(selected_path);
    if !exists_directory(filesystem, selected_path)?
        || !exists_directory(filesystem, &layout.exefs_path)?
        || !exists_directory(filesystem, &layout.game_data_path)?
    {
        return Ok(None);
    }
    Ok(Some(make_structure(
        PlatformKind::Switch,
        selected_path,
        vec![selected_path.to_owned()],
        selected_path.to_owned(),
        Some(file_name(selected_path).to_owned()),
        Some(layout.game_data_path.clone()),
        vec![layout.game_data_path],
    )))
}

fn detect_wiiu(
    filesystem: &VirtualFileSystem,
    selected_path: &str,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    let game_data_path = join_path(selected_path, "content/Data");
    if !exists_directory(filesystem, &game_data_path)? {
        return Ok(None);
    }
    Ok(Some(make_structure(
        PlatformKind::WiiU,
        selected_path,
        vec![selected_path.to_owned()],
        selected_path.to_owned(),
        Some(file_name(selected_path).to_owned()),
        Some(game_data_path.clone()),
        vec![game_data_path],
    )))
}

fn detect_windows_phone(
    filesystem: &VirtualFileSystem,
    selected_path: &str,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    if !exists_directory(filesystem, selected_path)?
        || !exists_directory(filesystem, &join_path(selected_path, "Assets"))?
    {
        return Ok(None);
    }
    let game_data_path = join_path(selected_path, "Data");
    if !exists_directory(filesystem, &game_data_path)? {
        return Ok(None);
    }
    let executables: Vec<String> = top_level_files(filesystem, selected_path)?
        .into_iter()
        .filter(|path| extension(file_name(path)) == ".exe")
        .collect();
    if executables.len() != 1 {
        return Ok(None);
    }
    let name = file_stem(file_name(&executables[0])).to_owned();
    Ok(Some(make_structure(
        PlatformKind::WindowsPhone,
        selected_path,
        vec![selected_path.to_owned()],
        selected_path.to_owned(),
        Some(name),
        Some(game_data_path.clone()),
        vec![game_data_path],
    )))
}

fn detect_webgl(
    filesystem: &VirtualFileSystem,
    selected_path: &str,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    if !exists_directory(filesystem, selected_path)?
        || !has_top_level_extension(filesystem, selected_path, ".html")?
    {
        return Ok(None);
    }
    for (directory, suffix) in [
        ("Build", ".data.unityweb"),
        ("Development", ".data"),
        ("Release", ".datagz"),
    ] {
        let data_directory = join_path(selected_path, directory);
        if exists_directory(filesystem, &data_directory)?
            && has_top_level_suffix(filesystem, &data_directory, suffix)?
        {
            return Ok(Some(make_structure(
                PlatformKind::WebGl,
                selected_path,
                vec![selected_path.to_owned()],
                selected_path.to_owned(),
                Some(file_name(selected_path).to_owned()),
                Some(selected_path.to_owned()),
                vec![selected_path.to_owned(), data_directory],
            )));
        }
    }
    Ok(None)
}

fn detect_web_player(
    filesystem: &VirtualFileSystem,
    selected_path: &str,
) -> Result<Option<PlatformStructure>, PlatformStructureError> {
    if !exists_directory(filesystem, selected_path)? {
        return Ok(None);
    }
    for html_path in top_level_files(filesystem, selected_path)? {
        let html_name = file_name(&html_path);
        if extension(html_name) != ".html" {
            continue;
        }
        let name = file_stem(html_name);
        let bundle_path = join_path(selected_path, &format!("{name}.unity3d"));
        if exists_file(filesystem, &bundle_path)? {
            return Ok(Some(make_structure(
                PlatformKind::WebPlayer,
                selected_path,
                vec![selected_path.to_owned()],
                selected_path.to_owned(),
                Some(name.to_owned()),
                None,
                vec![selected_path.to_owned()],
            )));
        }
    }
    Ok(None)
}

fn make_structure(
    platform: PlatformKind,
    selected_path: &str,
    consumed_paths: Vec<String>,
    root_path: String,
    name: Option<String>,
    game_data_path: Option<String>,
    data_paths: Vec<String>,
) -> PlatformStructure {
    PlatformStructure {
        platform,
        selected_path: selected_path.to_owned(),
        consumed_paths,
        root_path,
        name,
        game_data_path,
        data_paths,
    }
}

fn exists_file(filesystem: &VirtualFileSystem, path: &str) -> Result<bool, PlatformStructureError> {
    filesystem
        .file_exists(Some(path))
        .map_err(|source| PlatformStructureError::filesystem(path, source))
}

fn exists_directory(
    filesystem: &VirtualFileSystem,
    path: &str,
) -> Result<bool, PlatformStructureError> {
    filesystem
        .directory_exists(Some(path))
        .map_err(|source| PlatformStructureError::filesystem(path, source))
}

fn top_level_files(
    filesystem: &VirtualFileSystem,
    root_path: &str,
) -> Result<Vec<String>, PlatformStructureError> {
    filesystem
        .enumerate_files(Some(root_path), "*", SearchOption::TopDirectoryOnly)
        .map_err(|source| PlatformStructureError::filesystem(root_path, source))
}

fn top_level_directories(
    filesystem: &VirtualFileSystem,
    root_path: &str,
) -> Result<Vec<String>, PlatformStructureError> {
    filesystem
        .enumerate_directories(Some(root_path), "*", SearchOption::TopDirectoryOnly)
        .map_err(|source| PlatformStructureError::filesystem(root_path, source))
}

fn has_top_level_extension(
    filesystem: &VirtualFileSystem,
    root_path: &str,
    expected: &str,
) -> Result<bool, PlatformStructureError> {
    Ok(top_level_files(filesystem, root_path)?
        .iter()
        .any(|path| extension(file_name(path)) == expected))
}

fn has_top_level_suffix(
    filesystem: &VirtualFileSystem,
    root_path: &str,
    suffix: &str,
) -> Result<bool, PlatformStructureError> {
    Ok(top_level_files(filesystem, root_path)?
        .iter()
        .any(|path| file_name(path).ends_with(suffix)))
}

fn normalize_selected_path(
    path: &str,
    maximum_bytes: usize,
) -> Result<String, PlatformStructureError> {
    if path.len() > maximum_bytes {
        return Err(PlatformStructureError::new(
            Some(path.to_owned()),
            PlatformStructureErrorKind::SelectedPathTooLong {
                byte_count: path.len(),
                limit: maximum_bytes,
            },
        ));
    }
    let normalized_path = path.replace('\\', "/");
    let parts: Vec<&str> = normalized_path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    if parts
        .iter()
        .any(|component| matches!(*component, "." | ".."))
    {
        return Err(PlatformStructureError::new(
            Some(path.to_owned()),
            PlatformStructureErrorKind::InvalidSelectedPath,
        ));
    }
    Ok(if parts.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", parts.join("/"))
    })
}

fn parent_path(path: &str) -> String {
    match path.rsplit_once('/') {
        Some(("", _)) | None => "/".to_owned(),
        Some((parent, _)) => parent.to_owned(),
    }
}

fn join_path(root: &str, child: &str) -> String {
    if root == "/" {
        format!("/{child}")
    } else {
        format!("{}/{child}", root.trim_end_matches('/'))
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or_default()
}

fn extension(filename: &str) -> &str {
    filename
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, extension)| &filename[filename.len() - extension.len() - 1..])
        .unwrap_or_default()
}

fn file_stem(filename: &str) -> &str {
    filename
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(stem, _)| stem)
        .unwrap_or(filename)
}

#[derive(Debug)]
pub enum PlatformResourceLookupError {
    InvalidResourceName {
        name: String,
    },
    InvalidDataPath {
        data_path: String,
    },
    DataPathCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    UnavailableDataPathCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    PathTooLong {
        byte_count: usize,
        limit: usize,
    },
    PathSizeOverflow,
    AllocationFailed {
        requested: usize,
    },
    FileSystem {
        virtual_path: String,
        source: io::Error,
    },
}

impl Display for PlatformResourceLookupError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidResourceName { name } => {
                write!(
                    f,
                    "resource name '{name}' is empty, rooted, or traverses a parent path"
                )
            }
            Self::InvalidDataPath { data_path } => {
                write!(
                    f,
                    "platform DataPath '{data_path}' contains a parent path component"
                )
            }
            Self::DataPathCountLimitExceeded { count, limit } => write!(
                f,
                "platform DataPath count {count} exceeds resource lookup limit {limit}"
            ),
            Self::UnavailableDataPathCountLimitExceeded { count, limit } => write!(
                f,
                "unavailable DataPath count {count} exceeds diagnostic limit {limit}"
            ),
            Self::PathTooLong { byte_count, limit } => write!(
                f,
                "platform resource path has {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::PathSizeOverflow => f.write_str("platform resource path size overflows"),
            Self::AllocationFailed { requested } => write!(
                f,
                "could not allocate metadata for a {requested}-byte resource lookup"
            ),
            Self::FileSystem {
                virtual_path,
                source,
            } => write!(
                f,
                "could not inspect resource path '{virtual_path}': {source}"
            ),
        }
    }
}

impl Error for PlatformResourceLookupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FileSystem { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct PlatformStructureError {
    pub virtual_path: Option<String>,
    pub kind: PlatformStructureErrorKind,
}

impl PlatformStructureError {
    fn new(virtual_path: Option<String>, kind: PlatformStructureErrorKind) -> Self {
        Self { virtual_path, kind }
    }

    fn filesystem(path: &str, source: io::Error) -> Self {
        Self::new(
            Some(path.to_owned()),
            PlatformStructureErrorKind::FileSystem(source),
        )
    }
}

#[derive(Debug)]
pub enum PlatformStructureErrorKind {
    AmbiguousAndroidRoots { role: &'static str, count: usize },
    SelectedPathCountLimitExceeded { count: usize, limit: usize },
    SelectedPathTooLong { byte_count: usize, limit: usize },
    InvalidSelectedPath,
    AllocationFailed { requested: usize },
    FileSystem(io::Error),
}

impl Display for PlatformStructureError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.virtual_path {
            write!(f, "VFS path '{path}': ")?;
        }
        match &self.kind {
            PlatformStructureErrorKind::AmbiguousAndroidRoots { role, count } => {
                write!(
                    f,
                    "found {count} Android {role} roots; expected at most one"
                )
            }
            PlatformStructureErrorKind::SelectedPathCountLimitExceeded { count, limit } => {
                write!(f, "selected {count} paths, exceeding limit {limit}")
            }
            PlatformStructureErrorKind::SelectedPathTooLong { byte_count, limit } => write!(
                f,
                "selected path has {byte_count} bytes, exceeding limit {limit}"
            ),
            PlatformStructureErrorKind::InvalidSelectedPath => {
                f.write_str("selected path contains a dot or parent component")
            }
            PlatformStructureErrorKind::AllocationFailed { requested } => {
                write!(f, "could not allocate {requested} selected platform paths")
            }
            PlatformStructureErrorKind::FileSystem(source) => write!(f, "{source}"),
        }
    }
}

impl Error for PlatformStructureError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            PlatformStructureErrorKind::FileSystem(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        android_data_layout, detect_desktop_platform, detect_platform_structure, ios_data_layout,
        ps4_data_layout, switch_data_layout, DesktopPlatform, PlatformKind, PlatformResourceLookup,
        PlatformResourceProvider, PlatformResourceSource, PlatformStructure,
        PlatformStructureErrorKind, PlatformStructureLimits,
    };
    use crate::io::filesystem::VirtualFileSystem;

    fn insert_file(filesystem: &mut VirtualFileSystem, path: &str) {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, &[1]).unwrap();
    }

    fn insert_directory(filesystem: &mut VirtualFileSystem, path: &str) {
        filesystem.create_directory(Some(path)).unwrap();
    }

    fn resource_structure(paths: &[&str]) -> PlatformStructure {
        PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: None,
            data_paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    #[test]
    fn external_resource_lookup_uses_ordered_platform_then_mixed_paths_and_split_files() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/platform/second/tex.resS");
        insert_file(&mut filesystem, "/platform/second/same.resS");
        insert_file(&mut filesystem, "/mixed/split.resS.split0");
        insert_file(&mut filesystem, "/mixed/split.resS.split1");
        insert_file(&mut filesystem, "/mixed/same.resS");

        let platform = resource_structure(&["/platform/missing", "/platform/second"]);
        let mixed = resource_structure(&["/mixed"]);
        let provider = PlatformResourceProvider::new(Some(&platform), Some(&mixed));
        assert_eq!(
            provider
                .find_resource(
                    &filesystem,
                    "archive:/container/tex.resS",
                    PlatformStructureLimits::default(),
                )
                .unwrap(),
            PlatformResourceLookup::Found {
                virtual_path: "/platform/second/tex.resS".to_owned(),
                source: PlatformResourceSource::Platform,
                data_path_index: 1,
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            }
        );
        assert_eq!(
            provider
                .find_resource(&filesystem, "same.resS", PlatformStructureLimits::default(),)
                .unwrap(),
            PlatformResourceLookup::Found {
                virtual_path: "/platform/second/same.resS".to_owned(),
                source: PlatformResourceSource::Platform,
                data_path_index: 1,
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            }
        );
        assert_eq!(
            provider
                .find_resource(
                    &filesystem,
                    "split.resS",
                    PlatformStructureLimits::default(),
                )
                .unwrap(),
            PlatformResourceLookup::Found {
                virtual_path: "/mixed/split.resS".to_owned(),
                source: PlatformResourceSource::Mixed,
                data_path_index: 0,
                unavailable_data_paths: vec!["/platform/missing".to_owned()],
            }
        );
    }

    #[test]
    fn external_resource_lookup_accepts_mixed_game_structure_data_paths() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/mixed/resources/texture.resS");
        let mixed_data_paths = vec!["/mixed/missing".to_owned(), "/mixed/resources".to_owned()];
        let provider =
            PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed_data_paths));

        assert_eq!(
            provider
                .find_resource(
                    &filesystem,
                    "texture.resS",
                    PlatformStructureLimits::default(),
                )
                .unwrap(),
            PlatformResourceLookup::Found {
                virtual_path: "/mixed/resources/texture.resS".to_owned(),
                source: PlatformResourceSource::Mixed,
                data_path_index: 1,
                unavailable_data_paths: vec!["/mixed/missing".to_owned()],
            }
        );
    }

    #[test]
    fn external_resource_lookup_keeps_not_found_and_unavailable_paths_distinct() {
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/game/data");
        let platform = resource_structure(&["/missing", "/game/data"]);
        let provider = PlatformResourceProvider::new(Some(&platform), None);

        assert_eq!(
            provider
                .find_resource(
                    &filesystem,
                    "absent.resS",
                    PlatformStructureLimits::default(),
                )
                .unwrap(),
            PlatformResourceLookup::Missing {
                unavailable_data_paths: vec!["/missing".to_owned()],
            }
        );
    }

    #[test]
    fn detects_windows_roots_from_executable_and_data_directory() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/games/Nova.exe");
        insert_directory(&mut filesystem, "/games/Nova_Data");

        let from_executable = detect_desktop_platform(
            &filesystem,
            &["/games/Nova.exe".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(from_executable.platform, DesktopPlatform::Windows);
        assert_eq!(from_executable.root_path, "/games");
        assert_eq!(
            from_executable.game_data_path.as_deref(),
            Some("/games/Nova_Data")
        );

        let from_data = detect_desktop_platform(
            &filesystem,
            &["/games/Nova_Data".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(from_data.selected_path, "/games/Nova_Data");
        assert_eq!(from_data.root_path, "/games");
    }

    #[test]
    fn detects_linux_fallback_data_directory_and_mac_ordered_paths() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/linux/game.x86_64");
        insert_directory(&mut filesystem, "/linux/Data");
        let linux = detect_desktop_platform(
            &filesystem,
            &["/linux".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(linux.platform, DesktopPlatform::Linux);
        assert_eq!(linux.game_data_path.as_deref(), Some("/linux/Data"));

        insert_directory(&mut filesystem, "/App.app/Contents/Resources/Data");
        let mac = detect_desktop_platform(
            &filesystem,
            &["/App.app".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(mac.platform, DesktopPlatform::Mac);
        assert_eq!(
            mac.data_paths,
            [
                "/App.app/Contents/Resources/Data",
                "/App.app/Contents/Resources"
            ]
        );
    }

    #[test]
    fn desktop_detectors_cover_fallback_names_direct_selection_and_empty_shapes() {
        // Windows falls back to a bare `Data` directory: one executable keeps
        // its stem as the game name.
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/solo/only.exe");
        insert_directory(&mut filesystem, "/solo/Data");
        let solo = detect_desktop_platform(
            &filesystem,
            &["/solo".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(solo.platform, DesktopPlatform::Windows);
        assert_eq!(solo.name.as_deref(), Some("only"));
        assert_eq!(solo.game_data_path.as_deref(), Some("/solo/Data"));

        // Several executables without named pairs share the root folder name.
        insert_file(&mut filesystem, "/multi/b.exe");
        insert_file(&mut filesystem, "/multi/a.exe");
        insert_directory(&mut filesystem, "/multi/Data");
        let multi = detect_desktop_platform(
            &filesystem,
            &["/multi".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(multi.platform, DesktopPlatform::Windows);
        assert_eq!(multi.name.as_deref(), Some("multi"));
        assert_eq!(multi.game_data_path.as_deref(), Some("/multi/Data"));

        // A Linux executable selects its parent root directly, and a named
        // pair wins without any fallback.
        insert_file(&mut filesystem, "/lin/game.x86");
        insert_directory(&mut filesystem, "/lin/game_Data");
        let direct = detect_desktop_platform(
            &filesystem,
            &["/lin/game.x86".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(direct.platform, DesktopPlatform::Linux);
        assert_eq!(direct.selected_path, "/lin/game.x86");
        assert_eq!(direct.root_path, "/lin");
        assert_eq!(direct.game_data_path.as_deref(), Some("/lin/game_Data"));

        // A `.app` bundle without resources and an orphaned data directory
        // match no desktop shape.
        insert_directory(&mut filesystem, "/Empty.app");
        assert!(detect_desktop_platform(
            &filesystem,
            &["/Empty.app".to_owned()],
            PlatformStructureLimits::default()
        )
        .unwrap()
        .is_none());
        insert_directory(&mut filesystem, "/o/Orphan_Data");
        assert!(detect_desktop_platform(
            &filesystem,
            &["/o/Orphan_Data".to_owned()],
            PlatformStructureLimits::default()
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn follows_source_priority_and_reports_bad_selection_paths() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/both/Game.exe");
        insert_directory(&mut filesystem, "/both/Game_Data");
        insert_file(&mut filesystem, "/both/game.x86");
        insert_directory(&mut filesystem, "/both/game_Data");
        let selected = ["/both".to_owned()];
        assert_eq!(
            detect_desktop_platform(&filesystem, &selected, PlatformStructureLimits::default())
                .unwrap()
                .unwrap()
                .platform,
            DesktopPlatform::Windows
        );

        let bad_path = detect_desktop_platform(
            &filesystem,
            &["/both/../escape".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            bad_path.kind,
            PlatformStructureErrorKind::InvalidSelectedPath
        ));

        assert!(detect_desktop_platform(
            &filesystem,
            &["/not-a-game".to_owned()],
            PlatformStructureLimits::default()
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn detects_android_apk_and_obb_with_source_ordered_data_paths() {
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/apk/assets/bin/Data");
        insert_directory(&mut filesystem, "/apk/lib/arm64-v8a");
        insert_directory(&mut filesystem, "/apk/META-INF");
        insert_directory(&mut filesystem, "/obb/assets/bin/Data");

        let structure = detect_platform_structure(
            &filesystem,
            &["/obb".to_owned(), "/apk".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(structure.platform, PlatformKind::Android);
        assert_eq!(structure.selected_path, "/apk");
        assert_eq!(structure.consumed_paths, ["/apk", "/obb"]);
        assert_eq!(
            structure.data_paths,
            ["/apk/assets/bin/Data", "/obb/assets/bin/Data"]
        );

        insert_directory(&mut filesystem, "/apk2/assets/bin/Data");
        insert_directory(&mut filesystem, "/apk2/META-INF");
        let ambiguous = detect_platform_structure(
            &filesystem,
            &["/apk".to_owned(), "/apk2".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            ambiguous.kind,
            PlatformStructureErrorKind::AmbiguousAndroidRoots {
                role: "APK",
                count: 2
            }
        ));
    }

    #[test]
    fn android_directory_layout_matches_the_constructor_joins() {
        let layout = android_data_layout("/apk");
        assert_eq!(layout.game_data_path, "/apk/assets/bin/Data");
        assert_eq!(layout.resources_path, "/apk/assets/bin/Data/Resources");
        assert_eq!(layout.managed_path, "/apk/assets/bin/Data/Managed");
        assert_eq!(layout.lib_path, "/apk/lib");
        assert_eq!(
            layout.il2cpp_metadata_path,
            "/apk/assets/bin/Data/Managed/Metadata/global-metadata.dat"
        );
        // The same joins apply to an OBB root.
        let obb = android_data_layout("/obb");
        assert_eq!(obb.game_data_path, "/obb/assets/bin/Data");
        assert_eq!(
            obb.il2cpp_metadata_path,
            "/obb/assets/bin/Data/Managed/Metadata/global-metadata.dat"
        );
    }

    #[test]
    fn ios_directory_layout_matches_the_constructor_joins() {
        let layout = ios_data_layout("/ios", "/ios/Payload/Example.app", "Example");
        assert_eq!(layout.game_data_path, "/ios/Payload/Example.app/Data");
        assert_eq!(layout.streaming_assets_path, "/ios/Raw");
        assert_eq!(
            layout.resources_path,
            "/ios/Payload/Example.app/Data/Resources"
        );
        assert_eq!(layout.managed_path, "/ios/Payload/Example.app/Data/Managed");
        // The IL2Cpp assembly is the app binary itself, not a lib file.
        assert_eq!(
            layout.il2cpp_game_assembly_path,
            "/ios/Payload/Example.app/Example"
        );
        assert_eq!(
            layout.il2cpp_metadata_path,
            "/ios/Payload/Example.app/Data/Managed/Metadata/global-metadata.dat"
        );
    }

    #[test]
    fn switch_directory_layout_matches_the_constructor_joins() {
        let layout = switch_data_layout("/switch");
        assert_eq!(layout.game_data_path, "/switch/romfs/Data");
        assert_eq!(
            layout.streaming_assets_path,
            "/switch/romfs/Data/StreamingAssets"
        );
        assert_eq!(layout.resources_path, "/switch/romfs/Data/Resources");
        assert_eq!(layout.managed_path, "/switch/romfs/Data/Managed");
        assert_eq!(layout.exefs_path, "/switch/exefs");
        // The IL2Cpp assembly is the `main` binary under `exefs`.
        assert_eq!(layout.il2cpp_game_assembly_path, "/switch/exefs/main");
        assert_eq!(
            layout.il2cpp_metadata_path,
            "/switch/romfs/Data/Managed/Metadata/global-metadata.dat"
        );
    }

    #[test]
    fn ps4_directory_layout_matches_the_constructor_joins() {
        let layout = ps4_data_layout("/ps4");
        assert_eq!(layout.game_data_path, "/ps4/Media");
        assert_eq!(layout.resources_path, "/ps4/Media/Resources");
        assert_eq!(layout.managed_path, "/ps4/Media/Managed");
        assert_eq!(layout.modules_path, "/ps4/Media/Modules");
        // The IL2Cpp assembly is the PS4-only prx under `Modules`.
        assert_eq!(
            layout.il2cpp_game_assembly_path,
            "/ps4/Media/Modules/Il2CppUserAssemblies.prx"
        );
        // The metadata sits directly under `Data/Metadata`, not under `Managed`.
        assert_eq!(
            layout.il2cpp_metadata_path,
            "/ps4/Media/Metadata/global-metadata.dat"
        );
    }

    #[test]
    fn detects_ios_and_console_game_data_roots() {
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/ios/Payload/Example.app/Data");
        let ios = detect_platform_structure(
            &filesystem,
            &["/ios".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(ios.platform, PlatformKind::Ios);
        assert_eq!(ios.name.as_deref(), Some("Example"));
        assert_eq!(ios.data_paths, ["/ios/Payload/Example.app/Data"]);

        insert_file(&mut filesystem, "/ps4/eboot.bin");
        insert_directory(&mut filesystem, "/ps4/Media");
        let ps4 = detect_platform_structure(
            &filesystem,
            &["/ps4".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(ps4.platform, PlatformKind::Ps4);
        assert_eq!(ps4.game_data_path.as_deref(), Some("/ps4/Media"));

        insert_directory(&mut filesystem, "/switch/exefs");
        insert_directory(&mut filesystem, "/switch/romfs/Data");
        let switch = detect_platform_structure(
            &filesystem,
            &["/switch".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(switch.platform, PlatformKind::Switch);
        assert_eq!(switch.game_data_path.as_deref(), Some("/switch/romfs/Data"));

        insert_directory(&mut filesystem, "/wiiu/content/Data");
        let wiiu = detect_platform_structure(
            &filesystem,
            &["/wiiu".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(wiiu.platform, PlatformKind::WiiU);
        assert_eq!(wiiu.game_data_path.as_deref(), Some("/wiiu/content/Data"));
    }

    #[test]
    fn detects_webgl_layout_precedence_and_web_player_bundle_roots() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/web/index.html");
        insert_file(&mut filesystem, "/web/Build/player.data.unityweb");
        insert_directory(&mut filesystem, "/web/Development");
        insert_file(&mut filesystem, "/web/Development/player.data");
        let webgl = detect_platform_structure(
            &filesystem,
            &["/web".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(webgl.platform, PlatformKind::WebGl);
        assert_eq!(webgl.data_paths, ["/web", "/web/Build"]);

        insert_file(&mut filesystem, "/player/index.html");
        insert_file(&mut filesystem, "/player/index.unity3d");
        let web_player = detect_platform_structure(
            &filesystem,
            &["/player".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(web_player.platform, PlatformKind::WebPlayer);
        assert_eq!(web_player.name.as_deref(), Some("index"));
        assert_eq!(web_player.game_data_path, None);
        assert_eq!(web_player.data_paths, ["/player"]);
    }

    #[test]
    fn preserves_windows_before_windows_phone_detection_priority() {
        let mut filesystem = VirtualFileSystem::new();
        insert_directory(&mut filesystem, "/phone/Assets");
        insert_directory(&mut filesystem, "/phone/Data");
        insert_file(&mut filesystem, "/phone/Game.exe");

        // The source checker tests Windows before Windows Phone. Windows' generic `Data`
        // fallback therefore wins for this overlapping shape.
        let structure = detect_platform_structure(
            &filesystem,
            &["/phone".to_owned()],
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(structure.platform, PlatformKind::Windows);
    }
}
