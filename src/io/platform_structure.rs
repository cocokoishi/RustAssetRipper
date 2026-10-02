//! Platform root and DataPath discovery for desktop Unity game layouts.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use crate::io::filesystem::{SearchOption, VirtualFileSystem};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopPlatform {
    Windows,
    Linux,
    Mac,
}

/// A source-shaped desktop root and ordered dependency DataPaths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformStructure {
    pub platform: DesktopPlatform,
    /// The selected input path that matched this structure.
    pub selected_path: String,
    /// Normalized application root (the parent for executable and `_Data` selections).
    pub root_path: String,
    pub name: String,
    pub game_data_path: String,
    pub data_paths: Vec<String>,
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

    for platform in [
        DesktopPlatform::Windows,
        DesktopPlatform::Linux,
        DesktopPlatform::Mac,
    ] {
        for selected_path in &normalized {
            let detected = match platform {
                DesktopPlatform::Windows => detect_windows(filesystem, selected_path)?,
                DesktopPlatform::Linux => detect_linux(filesystem, selected_path)?,
                DesktopPlatform::Mac => detect_mac(filesystem, selected_path)?,
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
        platform: DesktopPlatform::Windows,
        selected_path: selected_path.to_owned(),
        root_path,
        name,
        data_paths: vec![game_data_path.clone()],
        game_data_path,
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
        platform: DesktopPlatform::Linux,
        selected_path: selected_path.to_owned(),
        root_path,
        name,
        data_paths: vec![game_data_path.clone()],
        game_data_path,
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
        platform: DesktopPlatform::Mac,
        selected_path: selected_path.to_owned(),
        root_path: selected_path.to_owned(),
        name: file_stem(file_name(selected_path)).to_owned(),
        game_data_path: game_data_path.clone(),
        data_paths: vec![game_data_path, resource_path],
    }))
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
        detect_desktop_platform, DesktopPlatform, PlatformStructureErrorKind,
        PlatformStructureLimits,
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
        assert_eq!(from_executable.game_data_path, "/games/Nova_Data");

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
        assert_eq!(linux.game_data_path, "/linux/Data");

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
}
