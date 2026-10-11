//! Platform root and DataPath discovery for mounted Unity game layouts.
//!
//! Covers the root/marker decisions per platform (including the Android APK/OBB bitmask and the
//! `assets/bin/Data` gate), the Android, iOS, Switch, PS4, and WiiU directory layouts, the WebGL
//! build-flavor preference, and the WebPlayer html-to-bundle pairing. The desktop detectors share
//! a bare-`Data` fallback (the Linux baseline has no such arm; the port applies the Windows rule
//! to both deliberately). Backend selection, assembly and metadata collection, and bundle
//! gathering live in the file inventory; bundle-hierarchy lookups live in the serialized-file
//! workspace.

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
    // Container mounting expands the processing filesystem. Post-export copying
    // instead needs the unchanged physical StreamingAssets files, including wrappers.
    streaming_assets_filesystem: Option<&'a VirtualFileSystem>,
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
            streaming_assets_filesystem: None,
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
            streaming_assets_filesystem: None,
        }
    }

    /// Supply the original StreamingAssets tree when the processing VFS contains
    /// expanded containers. This does not change dependency/resource lookup.
    pub fn with_streaming_assets_filesystem(mut self, filesystem: &'a VirtualFileSystem) -> Self {
        self.streaming_assets_filesystem = Some(filesystem);
        self
    }

    pub(crate) fn streaming_assets_filesystem(&self) -> Option<&'a VirtualFileSystem> {
        self.streaming_assets_filesystem
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

impl PlatformStructure {
    /// `PlatformGameStructure.StreamingAssetsPath`, before checking directory existence.
    pub fn streaming_assets_path(&self) -> Option<String> {
        match self.platform {
            PlatformKind::Android | PlatformKind::Ps4 | PlatformKind::WebPlayer => None,
            PlatformKind::Ios => Some(join_path(&self.root_path, "Raw")),
            PlatformKind::WebGl => Some(self.root_path.clone()),
            PlatformKind::Windows
            | PlatformKind::Linux
            | PlatformKind::Mac
            | PlatformKind::Switch
            | PlatformKind::WiiU
            | PlatformKind::WindowsPhone => self
                .game_data_path
                .as_ref()
                .map(|path| join_path(path, "StreamingAssets")),
        }
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
    desktop_data_fallback(filesystem, root_path, executable_count, &last_name)
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
    desktop_data_fallback(filesystem, root_path, executable_count, &last_name)
}

/// Bare-`Data` fallback shared by the desktop detectors: when executables exist but none has a
/// matching `{name}_Data` directory, the source `WindowsGameStructure.GetDataDirectory` falls
/// back to `Data` (single-executable stem, else the root name). The Linux baseline has no such
/// arm; the port applies it to both desktop layouts deliberately so renamed data directories
/// still resolve.
fn desktop_data_fallback(
    filesystem: &VirtualFileSystem,
    root_path: &str,
    executable_count: usize,
    last_name: &str,
) -> Result<Option<(String, String)>, PlatformStructureError> {
    if executable_count == 0 {
        return Ok(None);
    }
    let fallback = join_path(root_path, "Data");
    if !exists_directory(filesystem, &fallback)? {
        return Ok(None);
    }
    let name = if executable_count == 1 {
        last_name.to_owned()
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
        match android_root_role(marker, exists_directory(filesystem, &data_path)?) {
            AndroidRootRole::Apk => apk_roots.push(path.clone()),
            AndroidRootRole::Obb => obb_roots.push(path.clone()),
            AndroidRootRole::Neither => {}
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

/// An Android root's role in the APK/OBB pairing.
///
/// Mirrors the `CheckAndroid` split: `IsAndroidStructure` (assets plus `lib` and/or
/// `META-INF` with the data directory present) claims the APK slot, `IsAndroidObbStructure`
/// (assets alone with the data directory present) claims the OBB slot, and anything else
/// stays out of the pairing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AndroidRootRole {
    Apk,
    Obb,
    Neither,
}

fn android_root_role(marker: u8, data_directory_exists: bool) -> AndroidRootRole {
    if !data_directory_exists {
        AndroidRootRole::Neither
    } else if marker > 8 {
        AndroidRootRole::Apk
    } else if marker == 8 {
        AndroidRootRole::Obb
    } else {
        AndroidRootRole::Neither
    }
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

/// `SwitchGameStructure` directory names: the executable filesystem, the game-data
/// filesystem, and the IL2Cpp `main` binary inside the former.
pub const SWITCH_EXEFS_DIR: &str = "exefs";
pub const SWITCH_ROMFS_DIR: &str = "romfs";
pub const SWITCH_MAIN_BINARY: &str = "main";

pub fn switch_data_layout(root_path: &str) -> SwitchDataLayout {
    let game_data_path = join_path(&join_path(root_path, SWITCH_ROMFS_DIR), "Data");
    let managed_path = join_path(&game_data_path, "Managed");
    let exefs_path = join_path(root_path, SWITCH_EXEFS_DIR);
    SwitchDataLayout {
        streaming_assets_path: join_path(&game_data_path, "StreamingAssets"),
        resources_path: join_path(&game_data_path, "Resources"),
        il2cpp_game_assembly_path: join_path(&exefs_path, SWITCH_MAIN_BINARY),
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

/// `WiiUGameStructure` directory layout for one title root.
///
/// Mirrors the constructor joins: `content/Data` under the root, `StreamingAssets`, `Resources`,
/// and `Managed` under it. The Unity player, version, and IL2Cpp paths stay unset on WiiU —
/// WiiU does not support IL2Cpp, so the backend rule never takes an IL2Cpp arm — and they have
/// no slots here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WiiUDataLayout {
    pub game_data_path: String,
    pub streaming_assets_path: String,
    pub resources_path: String,
    pub managed_path: String,
}

pub fn wiiu_data_layout(root_path: &str) -> WiiUDataLayout {
    let game_data_path = join_path(root_path, "content/Data");
    WiiUDataLayout {
        streaming_assets_path: join_path(&game_data_path, "StreamingAssets"),
        resources_path: join_path(&game_data_path, "Resources"),
        managed_path: join_path(&game_data_path, "Managed"),
        game_data_path,
    }
}

/// `WebGLGameStructure` build-flavor search order: `Build` (`*.data.unityweb`), then
/// `Development` (`*.data`), then `Release` (`*.datagz`), mirroring the constructor preference.
pub const WEBGL_BUILD_FLAVORS: [(&str, &str); 3] = [
    ("Build", ".data.unityweb"),
    ("Development", ".data"),
    ("Release", ".datagz"),
];

/// The WebPlayer html marker extension.
pub const WEB_PLAYER_HTML_EXTENSION: &str = ".html";

/// The asset-bundle extension a WebPlayer html file pairs with.
pub const WEB_PLAYER_BUNDLE_EXTENSION: &str = ".unity3d";

/// `WebPlayerGameStructure.GetWebPlayerName` pairing rule over caller facts: an `.html` file
/// stem names the player, and its bundle sits beside it with the asset-bundle extension.
/// Returns the player name and the sibling bundle path.
pub fn web_player_bundle_path(root_path: &str, html_file_name: &str) -> Option<(String, String)> {
    if extension(html_file_name) != WEB_PLAYER_HTML_EXTENSION {
        return None;
    }
    let name = file_stem(html_file_name).to_owned();
    let bundle_path = join_path(root_path, &format!("{name}{WEB_PLAYER_BUNDLE_EXTENSION}"));
    Some((name, bundle_path))
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
        let data_path = join_path(&app_path, "Data");
        if let Some((name, game_data_path)) =
            ios_app_data(&app_path, exists_directory(filesystem, &data_path)?)
        {
            return Ok(Some(make_structure(
                PlatformKind::Ios,
                selected_path,
                vec![selected_path.to_owned()],
                selected_path.to_owned(),
                Some(name),
                Some(game_data_path.clone()),
                vec![game_data_path],
            )));
        }
    }
    Ok(None)
}

/// `GetDataiOSDirectory` candidate step over caller facts: a `Payload` child whose name ends
/// in `.app` (ordinal) yields its stemmed app name and data path when its `Data` directory
/// exists; anything else is skipped like the source loop.
fn ios_app_data(app_path: &str, data_directory_exists: bool) -> Option<(String, String)> {
    let app_name = file_name(app_path);
    if !app_name.ends_with(".app") || !data_directory_exists {
        return None;
    }
    Some((file_stem(app_name).to_owned(), join_path(app_path, "Data")))
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
    let layout = wiiu_data_layout(selected_path);
    if !exists_directory(filesystem, &layout.game_data_path)? {
        return Ok(None);
    }
    Ok(Some(make_structure(
        PlatformKind::WiiU,
        selected_path,
        vec![selected_path.to_owned()],
        selected_path.to_owned(),
        Some(file_name(selected_path).to_owned()),
        Some(layout.game_data_path.clone()),
        vec![layout.game_data_path],
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
    for (directory, suffix) in WEBGL_BUILD_FLAVORS {
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
        let Some((name, bundle_path)) =
            web_player_bundle_path(selected_path, file_name(&html_path))
        else {
            continue;
        };
        if exists_file(filesystem, &bundle_path)? {
            return Ok(Some(make_structure(
                PlatformKind::WebPlayer,
                selected_path,
                vec![selected_path.to_owned()],
                selected_path.to_owned(),
                Some(name),
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
    include!("platform_structure_tests_01.rs");
}
