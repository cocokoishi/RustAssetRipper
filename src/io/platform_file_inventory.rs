//! Platform game file registration matching `PlatformGameStructure.CollectFiles`.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Read, Seek, SeekFrom};

use crate::io::dependency::DependencyFilePath;
use crate::io::filesystem::{SearchOption, VirtualFileSystem};
use crate::io::platform_structure::{PlatformKind, PlatformStructure};
use crate::io::stream::{logical_file_name, logical_file_path};
use crate::scheme::{FileKind, SchemeReader};

/// Bounds applied while collecting platform files and probing bundle headers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformFileInventoryLimits {
    pub max_inspected_files: usize,
    pub max_single_file_bytes: u64,
    pub max_total_inspected_bytes: u64,
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
    pub report: PlatformFileInventoryReport,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlatformFileInventoryReport {
    pub inspected_files: usize,
    pub inspected_bytes: u64,
    pub traversed_directories: usize,
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

        Ok(PlatformFileInventory {
            registered_files: state
                .registered_files
                .into_iter()
                .map(|(file_name, virtual_path)| DependencyFilePath {
                    file_name,
                    virtual_path,
                })
                .collect(),
            report: state.report,
        })
    }
}

struct InventoryState<'a> {
    filesystem: &'a VirtualFileSystem,
    limits: PlatformFileInventoryLimits,
    report: PlatformFileInventoryReport,
    inspected_paths: BTreeSet<String>,
    registered_files: BTreeSet<(String, String)>,
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
            self.report.traversed_directories = self
                .report
                .traversed_directories
                .checked_add(1)
                .ok_or_else(|| {
                    PlatformFileInventoryError::new(
                        Some(directory.clone()),
                        PlatformFileInventoryErrorKind::DirectoryCountOverflow,
                    )
                })?;
            if self.report.traversed_directories > self.limits.max_directories {
                return Err(PlatformFileInventoryError::new(
                    Some(directory.clone()),
                    PlatformFileInventoryErrorKind::DirectoryCountLimitExceeded {
                        count: self.report.traversed_directories,
                        limit: self.limits.max_directories,
                    },
                ));
            }

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

#[cfg(test)]
mod tests {
    use super::{
        is_primary_engine_file, PlatformFileInventoryErrorKind, PlatformFileInventoryLimits,
        PlatformFileInventoryOptions,
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
