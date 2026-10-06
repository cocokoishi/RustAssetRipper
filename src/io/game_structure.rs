//! Deterministic VFS inventory matching the source `MixedGameStructure` input path.
//!
//! Mirrors `CollectFromDirectory` (per-directory source collection with parent-chain data paths)
//! and `CollectWebFiles` (the `.data`/`.datagz`/`.data.unityweb` extension switch, where a bare
//! `.unityweb` file is ignored). Directory scans inspect at most 32 bytes per file and use the
//! declared VFS length to validate SerializedFile headers; they do not decode wrappers or read
//! resource bodies. Explicitly selected files register even when they probe as resources, and all
//! counts and byte totals are bounded by [`MixedGameStructureLimits`] instead of failing like the
//! source's unbounded enumeration.
//!
//! Wrapper-input verdict (the task-201 suspicion, exonerated): GZip/Brotli bytes probe as
//! `Resource` and register nothing from directory scans, mirroring the baseline `Collect*`
//! arms, which have no wrapper branch (`BundleHeader.IsBundleHeader` matches bundle signatures,
//! so wrapper magic cannot match). Explicitly selected wrappers still register — like
//! `MultiFileStream.Exists` falling through to `AddFile` for any existing file — and unwrap
//! downstream in both implementations (the source GZip/Brotli schemes versus the
//! `SchemeReader::read` wrapper loop), so nothing is silently dropped.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Read, Seek, SeekFrom};

use crate::io::dependency::DependencyFilePath;
use crate::io::filesystem::{SearchOption, VirtualFileSystem};
use crate::io::stream::logical_file_name;
use crate::io::stream::logical_file_path;
use crate::processing::script_identity::is_mono_assembly;
use crate::scheme::{FileKind, SchemeReader};

/// Resource bounds for scanning explicitly selected mounted paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MixedGameStructureLimits {
    pub max_selected_paths: usize,
    pub max_scanned_files: usize,
    pub max_single_file_bytes: u64,
    pub max_total_scanned_bytes: u64,
    pub max_path_bytes: usize,
}

impl Default for MixedGameStructureLimits {
    fn default() -> Self {
        Self {
            max_selected_paths: 100_000,
            max_scanned_files: 1_000_000,
            max_single_file_bytes: 2 * 1024 * 1024 * 1024,
            max_total_scanned_bytes: 16 * 1024 * 1024 * 1024,
            max_path_bytes: 1024 * 1024,
        }
    }
}

/// Selected VFS files and the dependency-search roots discovered for a mixed input set.
///
/// When platform detection already consumed some selections (`PlatformChecker` removes matched
/// paths before the mixed pass), pass them as consumed paths so the union does not scan them
/// again; the skip count is retained in the report.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MixedGameStructure {
    /// Source-style `Files` entries: serialized files, bundle containers, and web data files.
    pub registered_files: Vec<DependencyFilePath>,
    /// Registered entries whose raw headers identify them as SerializedFiles.
    pub serialized_file_inputs: Vec<DependencyFilePath>,
    /// Managed assembly names and their mounted paths. Names are unique: the first path wins,
    /// like the source `Assemblies` dictionary, and later duplicates are counted in the report.
    pub assemblies: Vec<DependencyFilePath>,
    /// Direct-file parent directories and directories containing recognized source files.
    pub data_paths: Vec<String>,
    pub report: MixedGameStructureReport,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MixedGameStructureReport {
    pub selected_paths: usize,
    pub scanned_files: usize,
    pub scanned_bytes: u64,
    /// Normalized selections skipped because platform detection consumed them.
    pub skipped_consumed_paths: usize,
    /// Assembly paths dropped because their file name was already registered.
    pub duplicate_assemblies: usize,
}

impl MixedGameStructure {
    /// Build the source mixed-input registry from explicitly selected VFS files and directories.
    /// Directory scans inspect at most 32 bytes per file and use the declared VFS length to
    /// validate SerializedFile headers. They do not decode wrappers or read resource bodies.
    pub fn from_vfs_paths(
        filesystem: &VirtualFileSystem,
        selected_paths: &[String],
        limits: MixedGameStructureLimits,
    ) -> Result<Self, MixedGameStructureError> {
        Self::from_vfs_paths_with_consumed_paths(filesystem, selected_paths, &[], limits)
    }

    /// Build the registry while skipping selections platform detection already consumed.
    ///
    /// Mirrors `PlatformChecker`: matched platform paths are removed before the mixed pass, so a
    /// normalized selection equal to a consumed path is not scanned again. Consumed paths are
    /// normalized with the same rules as selections.
    pub fn from_vfs_paths_with_consumed_paths(
        filesystem: &VirtualFileSystem,
        selected_paths: &[String],
        consumed_paths: &[String],
        limits: MixedGameStructureLimits,
    ) -> Result<Self, MixedGameStructureError> {
        let mut report = MixedGameStructureReport::default();
        if selected_paths.len() > limits.max_selected_paths {
            return Err(MixedGameStructureError::new(
                None,
                MixedGameStructureErrorKind::SelectedPathCountLimitExceeded {
                    count: selected_paths.len(),
                    limit: limits.max_selected_paths,
                },
            ));
        }
        if selected_paths.is_empty() {
            return Err(MixedGameStructureError::new(
                None,
                MixedGameStructureErrorKind::NoSelectedPaths,
            ));
        }

        let mut files = BTreeSet::new();
        let mut serialized_files = BTreeSet::new();
        let mut assemblies = BTreeMap::new();
        let mut data_paths = BTreeSet::new();
        let mut unique_selections = BTreeSet::new();
        let mut unique_logical_paths = BTreeSet::new();
        let mut scanned_paths = BTreeSet::new();
        let mut consumed = BTreeSet::new();
        for consumed_path in consumed_paths {
            consumed.insert(normalize_selected_path(
                consumed_path,
                limits.max_path_bytes,
            )?);
        }

        for selected_path in selected_paths {
            let selected_path = normalize_selected_path(selected_path, limits.max_path_bytes)?;
            if !unique_selections.insert(selected_path.clone()) {
                continue;
            }
            if is_consumed_path(&selected_path, &consumed) {
                report.skipped_consumed_paths += 1;
                continue;
            }
            if !select_unique_logical_path(&mut unique_logical_paths, &selected_path) {
                continue;
            }

            if filesystem
                .file_exists(Some(&selected_path))
                .map_err(|source| MixedGameStructureError::filesystem(&selected_path, source))?
            {
                let identity = logical_file_name(&selected_path).ok_or_else(|| {
                    MixedGameStructureError::new(
                        Some(selected_path.clone()),
                        MixedGameStructureErrorKind::InvalidFileName,
                    )
                })?;
                let is_new = files.insert((identity.clone(), selected_path.clone()));
                if is_new {
                    data_paths.insert(parent_path(&selected_path));
                }
                let kind = scan_file_kind(
                    filesystem,
                    &selected_path,
                    &mut scanned_paths,
                    &mut report,
                    limits,
                )?;
                if kind == FileKind::SerializedFile {
                    serialized_files.insert((identity, selected_path));
                }
                continue;
            }

            if !filesystem
                .directory_exists(Some(&selected_path))
                .map_err(|source| MixedGameStructureError::filesystem(&selected_path, source))?
            {
                return Err(MixedGameStructureError::new(
                    Some(selected_path.clone()),
                    MixedGameStructureErrorKind::SelectedPathNotFound,
                ));
            }

            let paths = filesystem
                .enumerate_files(Some(&selected_path), "*", SearchOption::AllDirectories)
                .map_err(|source| MixedGameStructureError::filesystem(&selected_path, source))?;
            for path in paths {
                let kind =
                    scan_file_kind(filesystem, &path, &mut scanned_paths, &mut report, limits)?;
                let filename = logical_file_name(&path).ok_or_else(|| {
                    MixedGameStructureError::new(
                        Some(path.clone()),
                        MixedGameStructureErrorKind::InvalidFileName,
                    )
                })?;

                let primary_identity = match kind {
                    FileKind::SerializedFile => {
                        serialized_files.insert((filename.clone(), path.clone()));
                        Some(filename.clone())
                    }
                    FileKind::UnityFs
                    | FileKind::UnityRaw
                    | FileKind::UnityWebBundle
                    | FileKind::UnityArchive => Some(file_stem(&filename).to_lowercase()),
                    FileKind::UnityWebData | FileKind::Resource => None,
                };

                if let Some(identity) = primary_identity {
                    if files.insert((identity, path.clone())) {
                        add_parent_chain(&path, &selected_path, &mut data_paths);
                    }
                }
                if is_web_data_filename(&filename)
                    && files.insert((web_data_stem(&filename).to_owned(), path.clone()))
                {
                    add_parent_chain(&path, &selected_path, &mut data_paths);
                }
                if is_mono_assembly(&filename) {
                    insert_assembly(&mut assemblies, &mut report, filename, path);
                }
            }
        }

        report.selected_paths = unique_selections.len();
        Ok(MixedGameStructure {
            registered_files: into_dependency_paths(files),
            serialized_file_inputs: into_dependency_paths(serialized_files),
            assemblies: into_dependency_paths(assemblies.into_iter().collect()),
            data_paths: data_paths.into_iter().collect(),
            report,
        })
    }
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

fn scan_file_kind(
    filesystem: &VirtualFileSystem,
    path: &str,
    scanned_paths: &mut BTreeSet<String>,
    report: &mut MixedGameStructureReport,
    limits: MixedGameStructureLimits,
) -> Result<FileKind, MixedGameStructureError> {
    if !scanned_paths.insert(path.to_owned()) {
        let stream = filesystem
            .open_file(path)
            .map_err(|source| MixedGameStructureError::filesystem(path, source))?;
        let length = stream
            .length()
            .map_err(|source| MixedGameStructureError::filesystem(path, source))?;
        return probe_file_kind(stream, path, length);
    }

    let next_count = report.scanned_files.checked_add(1).ok_or_else(|| {
        MixedGameStructureError::new(
            Some(path.to_owned()),
            MixedGameStructureErrorKind::ScannedFileCountOverflow,
        )
    })?;
    if next_count > limits.max_scanned_files {
        return Err(MixedGameStructureError::new(
            Some(path.to_owned()),
            MixedGameStructureErrorKind::ScannedFileCountLimitExceeded {
                count: next_count,
                limit: limits.max_scanned_files,
            },
        ));
    }

    let stream = filesystem
        .open_file(path)
        .map_err(|source| MixedGameStructureError::filesystem(path, source))?;
    let length = stream
        .length()
        .map_err(|source| MixedGameStructureError::filesystem(path, source))?;
    if length > limits.max_single_file_bytes {
        return Err(MixedGameStructureError::new(
            Some(path.to_owned()),
            MixedGameStructureErrorKind::SingleFileLimitExceeded {
                size: length,
                limit: limits.max_single_file_bytes,
            },
        ));
    }
    let total_bytes = report.scanned_bytes.checked_add(length).ok_or_else(|| {
        MixedGameStructureError::new(
            Some(path.to_owned()),
            MixedGameStructureErrorKind::ScannedByteCountOverflow,
        )
    })?;
    if total_bytes > limits.max_total_scanned_bytes {
        return Err(MixedGameStructureError::new(
            Some(path.to_owned()),
            MixedGameStructureErrorKind::TotalScanLimitExceeded {
                size: total_bytes,
                limit: limits.max_total_scanned_bytes,
            },
        ));
    }
    report.scanned_files = next_count;
    report.scanned_bytes = total_bytes;

    probe_file_kind(stream, path, length)
}

fn probe_file_kind(
    mut stream: crate::io::stream::SmartStream,
    path: &str,
    total_length: u64,
) -> Result<FileKind, MixedGameStructureError> {
    let prefix_len = usize::try_from(total_length.min(32)).unwrap_or(32);
    let mut prefix = [0u8; 32];
    stream
        .seek(SeekFrom::Start(0))
        .and_then(|_| stream.read_exact(&mut prefix[..prefix_len]))
        .map_err(|source| MixedGameStructureError::filesystem(path, source))?;
    Ok(SchemeReader::probe_raw_kind(
        &prefix[..prefix_len],
        total_length,
    ))
}

fn normalize_selected_path(
    path: &str,
    maximum_bytes: usize,
) -> Result<String, MixedGameStructureError> {
    if path.len() > maximum_bytes {
        return Err(MixedGameStructureError::new(
            Some(path.to_owned()),
            MixedGameStructureErrorKind::SelectedPathTooLong {
                byte_count: path.len(),
                limit: maximum_bytes,
            },
        ));
    }
    let normalized_path = path.replace('\\', "/");
    let normalized: Vec<&str> = normalized_path
        .split('/')
        .filter(|component| !component.is_empty())
        .collect();
    if normalized
        .iter()
        .any(|component| matches!(*component, "." | ".."))
    {
        return Err(MixedGameStructureError::new(
            Some(path.to_owned()),
            MixedGameStructureErrorKind::InvalidSelectedPath,
        ));
    }
    Ok(if normalized.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", normalized.join("/"))
    })
}

/// Detection-side consumed-path skip for the platform+mixed union.
///
/// `PlatformChecker` removes matched platform paths before the mixed pass, so a normalized
/// selection equal to an already-consumed path is not scanned again. Only exact normalized
/// matches skip; sub-paths of consumed roots still scan, like the source leftover list.
fn is_consumed_path(normalized_selected: &str, consumed_paths: &BTreeSet<String>) -> bool {
    consumed_paths.contains(normalized_selected)
}

/// `MixedGameStructure.SelectUniquePaths` over caller facts: selections collapse by
/// logical path (`MultiFileStream.GetFilePath`), so a file and its split parts count as
/// one selection with the first occurrence winning. Directories keep their path unless
/// it carries a split suffix, exactly like the source mapping.
fn select_unique_logical_path(
    seen_logical_paths: &mut BTreeSet<String>,
    normalized_selected: &str,
) -> bool {
    seen_logical_paths.insert(
        logical_file_path(normalized_selected)
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

/// `CollectAssemblies` name rule over caller facts: assemblies are keyed by file name and
/// the first path wins, mirroring the source `Assemblies` dictionary. Later duplicates are
/// counted, not registered; the source logs a warning, and there is no logging sink here.
fn insert_assembly(
    assemblies: &mut BTreeMap<String, String>,
    report: &mut MixedGameStructureReport,
    filename: String,
    path: String,
) {
    if assemblies.contains_key(&filename) {
        report.duplicate_assemblies += 1;
        return;
    }
    assemblies.insert(filename, path);
}

fn parent_path(path: &str) -> String {
    match path.rsplit_once('/') {
        Some(("", _)) | None => "/".to_owned(),
        Some((parent, _)) => parent.to_owned(),
    }
}

fn add_parent_chain(path: &str, selected_root: &str, data_paths: &mut BTreeSet<String>) {
    let mut parent = parent_path(path);
    loop {
        data_paths.insert(parent.clone());
        if parent == selected_root || parent == "/" {
            break;
        }
        let next = parent_path(&parent);
        if next == parent {
            break;
        }
        parent = next;
    }
}

fn file_stem(file_name: &str) -> &str {
    file_name
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(stem, _)| stem)
        .unwrap_or(file_name)
}

fn is_web_data_filename(file_name: &str) -> bool {
    file_name.ends_with(".data")
        || file_name.ends_with(".datagz")
        || file_name.ends_with(".data.unityweb")
}

fn web_data_stem(file_name: &str) -> &str {
    file_name
        .strip_suffix(".data.unityweb")
        .or_else(|| file_name.strip_suffix(".datagz"))
        .or_else(|| file_name.strip_suffix(".data"))
        .unwrap_or(file_name)
}

#[derive(Debug)]
pub struct MixedGameStructureError {
    pub virtual_path: Option<String>,
    pub kind: MixedGameStructureErrorKind,
}

impl MixedGameStructureError {
    fn new(virtual_path: Option<String>, kind: MixedGameStructureErrorKind) -> Self {
        Self { virtual_path, kind }
    }

    fn filesystem(path: &str, source: io::Error) -> Self {
        Self::new(
            Some(path.to_owned()),
            MixedGameStructureErrorKind::FileSystem(source),
        )
    }
}

#[derive(Debug)]
pub enum MixedGameStructureErrorKind {
    NoSelectedPaths,
    SelectedPathCountLimitExceeded { count: usize, limit: usize },
    SelectedPathTooLong { byte_count: usize, limit: usize },
    InvalidSelectedPath,
    SelectedPathNotFound,
    InvalidFileName,
    ScannedFileCountOverflow,
    ScannedFileCountLimitExceeded { count: usize, limit: usize },
    SingleFileLimitExceeded { size: u64, limit: u64 },
    ScannedByteCountOverflow,
    TotalScanLimitExceeded { size: u64, limit: u64 },
    FileSystem(io::Error),
}

impl Display for MixedGameStructureError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.virtual_path {
            write!(f, "VFS path '{path}': ")?;
        }
        match &self.kind {
            MixedGameStructureErrorKind::NoSelectedPaths => {
                f.write_str("no mounted paths were selected")
            }
            MixedGameStructureErrorKind::SelectedPathCountLimitExceeded { count, limit } => {
                write!(f, "selected {count} paths, exceeding limit {limit}")
            }
            MixedGameStructureErrorKind::SelectedPathTooLong { byte_count, limit } => write!(
                f,
                "selected path has {byte_count} bytes, exceeding limit {limit}"
            ),
            MixedGameStructureErrorKind::InvalidSelectedPath => {
                f.write_str("selected path contains a dot or parent component")
            }
            MixedGameStructureErrorKind::SelectedPathNotFound => {
                f.write_str("selected file or directory does not exist")
            }
            MixedGameStructureErrorKind::InvalidFileName => {
                f.write_str("mounted file path has no valid filename")
            }
            MixedGameStructureErrorKind::ScannedFileCountOverflow => {
                f.write_str("scanned file count overflows")
            }
            MixedGameStructureErrorKind::ScannedFileCountLimitExceeded { count, limit } => {
                write!(f, "scan includes {count} files, exceeding limit {limit}")
            }
            MixedGameStructureErrorKind::SingleFileLimitExceeded { size, limit } => {
                write!(f, "file has {size} bytes, exceeding scan limit {limit}")
            }
            MixedGameStructureErrorKind::ScannedByteCountOverflow => {
                f.write_str("scanned byte count overflows")
            }
            MixedGameStructureErrorKind::TotalScanLimitExceeded { size, limit } => {
                write!(f, "scan includes {size} bytes, exceeding limit {limit}")
            }
            MixedGameStructureErrorKind::FileSystem(source) => write!(f, "{source}"),
        }
    }
}

impl Error for MixedGameStructureError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            MixedGameStructureErrorKind::FileSystem(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MixedGameStructure, MixedGameStructureErrorKind, MixedGameStructureLimits};
    use crate::compression::wrappers::compress_gzip;
    use crate::io::filesystem::VirtualFileSystem;

    fn insert_file(filesystem: &mut VirtualFileSystem, path: &str, bytes: &[u8]) {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, bytes).unwrap();
    }

    fn serialized_file_header(total_length: u64) -> Vec<u8> {
        let mut bytes = vec![0; usize::try_from(total_length).unwrap()];
        bytes[8..12].copy_from_slice(&22i32.to_be_bytes());
        bytes[20..24].copy_from_slice(&13i32.to_be_bytes());
        bytes[24..32].copy_from_slice(&total_length.to_be_bytes());
        bytes
    }

    fn bundle_header() -> Vec<u8> {
        let mut bytes = vec![0; 32];
        bytes[..8].copy_from_slice(b"UnityFS\0");
        bytes
    }

    #[test]
    fn directory_inventory_registers_source_file_kinds_and_parent_paths() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/Data/globalgamemanagers",
            &serialized_file_header(61),
        );
        insert_file(
            &mut filesystem,
            "/game/Data/bundles/Avatar.BUNDLE",
            &bundle_header(),
        );
        insert_file(&mut filesystem, "/game/Data/web/player.data", b"web-data");
        insert_file(
            &mut filesystem,
            "/game/Data/scripts/Assembly-CSharp.dll",
            b"assembly",
        );
        insert_file(&mut filesystem, "/game/Data/images/albedo.png", b"texture");

        let structure = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["\\game\\Data\\".to_owned()],
            MixedGameStructureLimits::default(),
        )
        .unwrap();

        assert_eq!(structure.serialized_file_inputs.len(), 1);
        assert_eq!(
            structure.serialized_file_inputs[0].file_name,
            "globalgamemanagers"
        );
        assert!(structure.registered_files.iter().any(|file| {
            file.file_name == "globalgamemanagers"
                && file.virtual_path == "/game/Data/globalgamemanagers"
        }));
        assert!(structure
            .registered_files
            .iter()
            .any(|file| file.file_name == "avatar"
                && file.virtual_path == "/game/Data/bundles/Avatar.BUNDLE"));
        assert!(structure
            .registered_files
            .iter()
            .any(|file| file.file_name == "player"
                && file.virtual_path == "/game/Data/web/player.data"));
        assert!(!structure
            .registered_files
            .iter()
            .any(|file| file.virtual_path.ends_with("albedo.png")));
        assert_eq!(structure.assemblies.len(), 1);
        assert_eq!(structure.assemblies[0].file_name, "Assembly-CSharp.dll");
        assert_eq!(
            structure.data_paths,
            ["/game/Data", "/game/Data/bundles", "/game/Data/web"]
        );
        assert_eq!(structure.report.scanned_files, 5);
    }

    #[test]
    fn explicitly_selected_file_is_registered_even_when_it_is_a_resource() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/selected/custom.blob", b"opaque");
        let structure = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["selected/custom.blob".to_owned()],
            MixedGameStructureLimits::default(),
        )
        .unwrap();

        assert_eq!(structure.registered_files.len(), 1);
        assert_eq!(structure.registered_files[0].file_name, "custom.blob");
        assert!(structure.serialized_file_inputs.is_empty());
        assert_eq!(structure.data_paths, ["/selected"]);
    }

    #[test]
    fn scan_file_and_byte_limits_return_errors() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/scan/a.bin", &[0; 4]);
        insert_file(&mut filesystem, "/scan/b.bin", &[0; 4]);

        let file_limited = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/scan".to_owned()],
            MixedGameStructureLimits {
                max_scanned_files: 1,
                ..MixedGameStructureLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            file_limited.kind,
            MixedGameStructureErrorKind::ScannedFileCountLimitExceeded { count: 2, limit: 1 }
        ));

        let byte_limited = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/scan".to_owned()],
            MixedGameStructureLimits {
                max_total_scanned_bytes: 7,
                ..MixedGameStructureLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            byte_limited.kind,
            MixedGameStructureErrorKind::TotalScanLimitExceeded { size: 8, limit: 7 }
        ));

        let single_file_limited = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/scan/a.bin".to_owned()],
            MixedGameStructureLimits {
                max_single_file_bytes: 3,
                ..MixedGameStructureLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            single_file_limited.kind,
            MixedGameStructureErrorKind::SingleFileLimitExceeded { size: 4, limit: 3 }
        ));
    }

    #[test]
    fn selected_path_and_not_found_errors_are_explicit() {
        let filesystem = VirtualFileSystem::new();
        let empty = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &[],
            MixedGameStructureLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            empty.kind,
            MixedGameStructureErrorKind::NoSelectedPaths
        ));

        let count_limited = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/scan".to_owned()],
            MixedGameStructureLimits {
                max_selected_paths: 0,
                ..MixedGameStructureLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            count_limited.kind,
            MixedGameStructureErrorKind::SelectedPathCountLimitExceeded { count: 1, limit: 0 }
        ));

        let traversal = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/scan/../other".to_owned()],
            MixedGameStructureLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            traversal.kind,
            MixedGameStructureErrorKind::InvalidSelectedPath
        ));

        let too_long = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/scan".to_owned()],
            MixedGameStructureLimits {
                max_path_bytes: 4,
                ..MixedGameStructureLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            too_long.kind,
            MixedGameStructureErrorKind::SelectedPathTooLong {
                byte_count: 5,
                limit: 4
            }
        ));

        let missing = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/absent".to_owned()],
            MixedGameStructureLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            missing.kind,
            MixedGameStructureErrorKind::SelectedPathNotFound
        ));
    }

    #[test]
    fn web_data_variants_follow_the_source_extension_switch() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/web/run.datagz", b"gzipped");
        insert_file(&mut filesystem, "/web/blob.data.unityweb", b"web");
        insert_file(&mut filesystem, "/web/plain.unityweb", b"ignored");
        let structure = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/web".to_owned()],
            MixedGameStructureLimits::default(),
        )
        .unwrap();

        assert!(structure
            .registered_files
            .iter()
            .any(|file| { file.file_name == "run" && file.virtual_path == "/web/run.datagz" }));
        assert!(structure.registered_files.iter().any(|file| {
            file.file_name == "blob" && file.virtual_path == "/web/blob.data.unityweb"
        }));
        // A bare `.unityweb` file matches no switch arm and is ignored.
        assert!(!structure
            .registered_files
            .iter()
            .any(|file| file.virtual_path == "/web/plain.unityweb"));
        assert_eq!(structure.data_paths, ["/web"]);
        assert_eq!(structure.report.scanned_files, 3);
    }

    #[test]
    fn split_part_selections_collapse_to_their_logical_path() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/mix/level5", b"base");
        insert_file(&mut filesystem, "/mix/level5.split0", b"part");
        insert_file(&mut filesystem, "/mix/other", b"other");
        let structure = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &[
                "/mix/level5".to_owned(),
                "/mix/level5.split0".to_owned(),
                "/mix/level5.split0".to_owned(),
                "/mix/other".to_owned(),
            ],
            MixedGameStructureLimits::default(),
        )
        .unwrap();

        // A file and its split part count as one selection with the first occurrence
        // winning (the exact duplicate never reaches the gate); only two paths scan.
        assert_eq!(structure.report.selected_paths, 3);
        assert_eq!(structure.report.scanned_files, 2);
        let level: Vec<_> = structure
            .registered_files
            .iter()
            .filter(|file| file.file_name == "level5")
            .collect();
        assert_eq!(level.len(), 1);
        assert_eq!(level[0].virtual_path, "/mix/level5");

        // Reversed order keeps the split part instead, like `Distinct` keeping the
        // first logical path.
        let reversed = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/mix/level5.split0".to_owned(), "/mix/level5".to_owned()],
            MixedGameStructureLimits::default(),
        )
        .unwrap();
        let level: Vec<_> = reversed
            .registered_files
            .iter()
            .filter(|file| file.file_name == "level5")
            .collect();
        assert_eq!(level.len(), 1);
        assert_eq!(level[0].virtual_path, "/mix/level5.split0");
    }

    #[test]
    fn platform_consumed_paths_are_not_scanned_again() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/apk/assets/bin/Data/globalgamemanagers",
            &serialized_file_header(61),
        );
        insert_file(&mut filesystem, "/loose/bundle.BUNDLE", &bundle_header());
        let structure = MixedGameStructure::from_vfs_paths_with_consumed_paths(
            &filesystem,
            &["/apk".to_owned(), "/apk".to_owned(), "/loose".to_owned()],
            &["\\apk\\".to_owned()],
            MixedGameStructureLimits::default(),
        )
        .unwrap();

        // The consumed selection skips once (the duplicate never reaches the gate),
        // the loose directory scans normally, and nothing from `/apk` registers.
        assert_eq!(structure.report.selected_paths, 2);
        assert_eq!(structure.report.skipped_consumed_paths, 1);
        assert_eq!(structure.report.scanned_files, 1);
        assert!(structure
            .registered_files
            .iter()
            .all(|file| file.virtual_path.starts_with("/loose")));
        assert_eq!(structure.data_paths, ["/loose"]);
    }

    #[test]
    fn directory_wrappers_match_the_baseline_collect_arms() {
        let wrapped = compress_gzip(b"payload", 1024).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/scan/wrapped.gz", &wrapped);

        // Directory scans register no wrapper input, like the source Collect* arms.
        let scanned = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/scan".to_owned()],
            MixedGameStructureLimits::default(),
        )
        .unwrap();
        assert!(scanned.registered_files.is_empty());
        assert!(scanned.serialized_file_inputs.is_empty());

        // An explicitly selected wrapper still registers, like Exists -> AddFile.
        let explicit = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/scan/wrapped.gz".to_owned()],
            MixedGameStructureLimits::default(),
        )
        .unwrap();
        assert!(explicit
            .registered_files
            .iter()
            .any(|file| file.virtual_path == "/scan/wrapped.gz"));
    }

    #[test]
    fn duplicate_assembly_names_keep_the_first_path_and_count_the_rest() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/a/Data/Managed/Engine.dll", b"first");
        insert_file(&mut filesystem, "/b/Data/Managed/Engine.dll", b"second");
        insert_file(&mut filesystem, "/b/Data/Managed/Other.dll", b"other");
        let structure = MixedGameStructure::from_vfs_paths(
            &filesystem,
            &["/".to_owned()],
            MixedGameStructureLimits::default(),
        )
        .unwrap();

        // Enumeration is lexical, so the `/a` copy wins like the source first-wins rule.
        assert_eq!(structure.assemblies.len(), 2);
        assert!(structure.assemblies.iter().any(|entry| {
            entry.file_name == "Engine.dll" && entry.virtual_path == "/a/Data/Managed/Engine.dll"
        }));
        assert!(structure.assemblies.iter().any(|entry| {
            entry.file_name == "Other.dll" && entry.virtual_path == "/b/Data/Managed/Other.dll"
        }));
        assert_eq!(structure.report.duplicate_assemblies, 1);
    }
}
