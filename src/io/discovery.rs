//! Bounded discovery of regular files beneath a local input directory.
//!
//! Ports the split-file recognition of `MultiFileStream`
//! (`AssetRipper/Source/AssetRipper.IO.Files/Streams/MultiFileStream.cs`): a
//! nonempty base plus `.split` plus a nonempty ASCII-digit suffix folds into one
//! logical input, matching `SplitFileRegex` (`.+\.split[0-9]+$`). Part contiguity
//! and numeric read order (the `SplitNameComparer` rule) live in the opener
//! (`io::stream`), which rejects gapped groups, so the lexical part order kept here
//! only selects the representative path. Platform file classification
//! (`IsPrimaryEngineFile`, bundle sniffing, assemblies) runs downstream in
//! `platform_file_inventory` and `game_structure`, never here.
//!
//! Deliberately stricter than the source enumeration: lexical traversal order,
//! rejected symlinks and non-regular files, depth/entry/input/size bounds, and an
//! explicit error when a regular file collides with a split group on the same
//! logical path. `expand_import_paths` additionally accepts files alongside
//! directories, mirroring the reference tools' mixed `GetAllFilePaths` inputs.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DirectoryDiscoveryReport {
    pub directory_entries_scanned: usize,
    pub discovered_inputs: usize,
    pub source_bytes: u64,
    /// Import paths that did not exist and were skipped by `expand_import_paths`.
    pub skipped_inputs: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredInput {
    /// A physical path accepted by `open_logical_file`; split groups point at one part.
    pub path: PathBuf,
    /// The logical relative filename, with `.splitN` removed for split groups.
    pub relative_path: PathBuf,
    /// Aggregate physical size of the regular file or all numbered split parts.
    pub source_bytes: u64,
}

#[derive(Debug)]
pub struct DirectoryDiscoveryError {
    pub report: DirectoryDiscoveryReport,
    pub path: PathBuf,
    pub kind: DirectoryDiscoveryErrorKind,
}

#[derive(Debug)]
pub enum DirectoryDiscoveryErrorKind {
    Io(io::Error),
    InputIsNotDirectory,
    SymbolicLinkNotAllowed,
    UnsupportedFileType,
    DepthLimitExceeded { depth: usize, limit: usize },
    EntryLimitExceeded { count: usize, limit: usize },
    InputLimitExceeded { count: usize, limit: usize },
    InvalidPathEncoding,
    DuplicateLogicalPath,
    SourceSizeOverflow,
    AllocationFailed { requested: usize },
}

#[derive(Default)]
struct InputGroup {
    regular_file: Option<(PathBuf, u64)>,
    split_parts: Vec<(PathBuf, u64)>,
}

/// Expand mixed file-or-directory import paths, mirroring the reference tools'
/// `GetAllFilePaths`: files become single inputs, directories reuse the bounded
/// recursive discovery below, and missing paths are skipped with a report count
/// instead of failing the batch.
///
/// Deliberate differences are documented, not changed: directories expand
/// recursively under the same depth/entry/input bounds (the reference tools use
/// top-level `EnumerateFiles` only); skips are counted, not printed, because
/// there is no logging sink in Phase 1; collisions between inputs from different
/// roots stay the mount layer's concern.
pub fn expand_import_paths(
    paths: &[PathBuf],
    max_directory_entries: usize,
    max_directory_depth: usize,
    max_inputs: usize,
) -> Result<(Vec<DiscoveredInput>, DirectoryDiscoveryReport), DirectoryDiscoveryError> {
    let mut report = DirectoryDiscoveryReport::default();
    let mut inputs = Vec::new();
    for path in paths {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                report.skipped_inputs = report.skipped_inputs.saturating_add(1);
                continue;
            }
            Err(error) => {
                return Err(discovery_error(
                    &report,
                    path,
                    DirectoryDiscoveryErrorKind::Io(error),
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(discovery_error(
                &report,
                path,
                DirectoryDiscoveryErrorKind::SymbolicLinkNotAllowed,
            ));
        }
        if metadata.is_dir() {
            let remaining_inputs = max_inputs.saturating_sub(inputs.len());
            let (mut discovered, directory_report) = discover_directory_inputs(
                path,
                max_directory_entries,
                max_directory_depth,
                remaining_inputs,
            )?;
            report.directory_entries_scanned = report
                .directory_entries_scanned
                .checked_add(directory_report.directory_entries_scanned)
                .ok_or_else(|| {
                    discovery_error(
                        &report,
                        path,
                        DirectoryDiscoveryErrorKind::EntryLimitExceeded {
                            count: usize::MAX,
                            limit: max_directory_entries,
                        },
                    )
                })?;
            report.source_bytes = report
                .source_bytes
                .checked_add(directory_report.source_bytes)
                .ok_or_else(|| {
                    discovery_error(
                        &report,
                        path,
                        DirectoryDiscoveryErrorKind::SourceSizeOverflow,
                    )
                })?;
            inputs.append(&mut discovered);
            report.discovered_inputs = inputs.len();
        } else if metadata.is_file() {
            if inputs.len() >= max_inputs {
                return Err(discovery_error(
                    &report,
                    path,
                    DirectoryDiscoveryErrorKind::InputLimitExceeded {
                        count: inputs.len().saturating_add(1),
                        limit: max_inputs,
                    },
                ));
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return Err(discovery_error(
                    &report,
                    path,
                    DirectoryDiscoveryErrorKind::InvalidPathEncoding,
                ));
            };
            let source_bytes = metadata.len();
            report.source_bytes =
                report
                    .source_bytes
                    .checked_add(source_bytes)
                    .ok_or_else(|| {
                        discovery_error(
                            &report,
                            path,
                            DirectoryDiscoveryErrorKind::SourceSizeOverflow,
                        )
                    })?;
            inputs.push(DiscoveredInput {
                path: path.clone(),
                relative_path: PathBuf::from(name),
                source_bytes,
            });
            report.discovered_inputs = inputs.len();
        } else {
            return Err(discovery_error(
                &report,
                path,
                DirectoryDiscoveryErrorKind::UnsupportedFileType,
            ));
        }
    }
    Ok((inputs, report))
}

/// Recursively lists files in lexical order and folds each contiguous-looking `.splitN`
/// filename family into one logical input. Contiguity is validated when the file is opened.
pub(crate) fn discover_directory_inputs(
    root: impl AsRef<Path>,
    max_directory_entries: usize,
    max_directory_depth: usize,
    max_inputs: usize,
) -> Result<(Vec<DiscoveredInput>, DirectoryDiscoveryReport), DirectoryDiscoveryError> {
    let root = root.as_ref();
    let mut report = DirectoryDiscoveryReport::default();
    let root_metadata = std::fs::symlink_metadata(root)
        .map_err(|error| discovery_error(&report, root, DirectoryDiscoveryErrorKind::Io(error)))?;
    if root_metadata.file_type().is_symlink() {
        return Err(discovery_error(
            &report,
            root,
            DirectoryDiscoveryErrorKind::SymbolicLinkNotAllowed,
        ));
    }
    if !root_metadata.is_dir() {
        return Err(discovery_error(
            &report,
            root,
            DirectoryDiscoveryErrorKind::InputIsNotDirectory,
        ));
    }

    let mut pending_directories = vec![(root.to_path_buf(), 0usize)];
    let mut groups = BTreeMap::<PathBuf, InputGroup>::new();

    while let Some((directory, depth)) = pending_directories.pop() {
        let iterator = std::fs::read_dir(&directory).map_err(|error| {
            discovery_error(&report, &directory, DirectoryDiscoveryErrorKind::Io(error))
        })?;
        let mut entries = Vec::new();
        for result in iterator {
            let entry = result.map_err(|error| {
                discovery_error(&report, &directory, DirectoryDiscoveryErrorKind::Io(error))
            })?;
            report.directory_entries_scanned = report
                .directory_entries_scanned
                .checked_add(1)
                .ok_or_else(|| {
                    discovery_error(
                        &report,
                        &entry.path(),
                        DirectoryDiscoveryErrorKind::EntryLimitExceeded {
                            count: usize::MAX,
                            limit: max_directory_entries,
                        },
                    )
                })?;
            if report.directory_entries_scanned > max_directory_entries {
                return Err(discovery_error(
                    &report,
                    &entry.path(),
                    DirectoryDiscoveryErrorKind::EntryLimitExceeded {
                        count: report.directory_entries_scanned,
                        limit: max_directory_entries,
                    },
                ));
            }
            entries.try_reserve(1).map_err(|_| {
                discovery_error(
                    &report,
                    &directory,
                    DirectoryDiscoveryErrorKind::AllocationFailed {
                        requested: entries.len().saturating_add(1),
                    },
                )
            })?;
            entries.push(entry);
        }
        entries.sort_by_key(|entry| entry.file_name());

        let mut child_directories = Vec::new();
        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type().map_err(|error| {
                discovery_error(&report, &path, DirectoryDiscoveryErrorKind::Io(error))
            })?;
            if file_type.is_symlink() {
                return Err(discovery_error(
                    &report,
                    &path,
                    DirectoryDiscoveryErrorKind::SymbolicLinkNotAllowed,
                ));
            }
            if file_type.is_dir() {
                let child_depth = depth.checked_add(1).ok_or_else(|| {
                    discovery_error(
                        &report,
                        &path,
                        DirectoryDiscoveryErrorKind::DepthLimitExceeded {
                            depth: usize::MAX,
                            limit: max_directory_depth,
                        },
                    )
                })?;
                if child_depth > max_directory_depth {
                    return Err(discovery_error(
                        &report,
                        &path,
                        DirectoryDiscoveryErrorKind::DepthLimitExceeded {
                            depth: child_depth,
                            limit: max_directory_depth,
                        },
                    ));
                }
                child_directories.try_reserve(1).map_err(|_| {
                    discovery_error(
                        &report,
                        &path,
                        DirectoryDiscoveryErrorKind::AllocationFailed {
                            requested: child_directories.len().saturating_add(1),
                        },
                    )
                })?;
                child_directories.push((path, child_depth));
                continue;
            }
            if !file_type.is_file() {
                return Err(discovery_error(
                    &report,
                    &path,
                    DirectoryDiscoveryErrorKind::UnsupportedFileType,
                ));
            }

            let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
                discovery_error(&report, &path, DirectoryDiscoveryErrorKind::Io(error))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(discovery_error(
                    &report,
                    &path,
                    DirectoryDiscoveryErrorKind::SymbolicLinkNotAllowed,
                ));
            }
            if !metadata.is_file() {
                return Err(discovery_error(
                    &report,
                    &path,
                    DirectoryDiscoveryErrorKind::UnsupportedFileType,
                ));
            }

            let relative_path = path.strip_prefix(root).map_err(|_| {
                discovery_error(
                    &report,
                    &path,
                    DirectoryDiscoveryErrorKind::InvalidPathEncoding,
                )
            })?;
            if relative_path.to_str().is_none() {
                return Err(discovery_error(
                    &report,
                    &path,
                    DirectoryDiscoveryErrorKind::InvalidPathEncoding,
                ));
            }
            let source_bytes = metadata.len();
            report.source_bytes =
                report
                    .source_bytes
                    .checked_add(source_bytes)
                    .ok_or_else(|| {
                        discovery_error(
                            &report,
                            &path,
                            DirectoryDiscoveryErrorKind::SourceSizeOverflow,
                        )
                    })?;
            let (logical_path, is_split_part) = split_logical_path(relative_path);
            let is_new_input = !groups.contains_key(&logical_path);
            if is_new_input && groups.len() >= max_inputs {
                return Err(discovery_error(
                    &report,
                    &path,
                    DirectoryDiscoveryErrorKind::InputLimitExceeded {
                        count: groups.len().saturating_add(1),
                        limit: max_inputs,
                    },
                ));
            }
            if is_new_input {
                report.discovered_inputs = groups.len().saturating_add(1);
            }
            let group = groups.entry(logical_path).or_default();
            if is_split_part {
                group.split_parts.try_reserve(1).map_err(|_| {
                    discovery_error(
                        &report,
                        &path,
                        DirectoryDiscoveryErrorKind::AllocationFailed {
                            requested: group.split_parts.len().saturating_add(1),
                        },
                    )
                })?;
                group.split_parts.push((path, source_bytes));
            } else if group
                .regular_file
                .replace((path.clone(), source_bytes))
                .is_some()
            {
                return Err(discovery_error(
                    &report,
                    &path,
                    DirectoryDiscoveryErrorKind::DuplicateLogicalPath,
                ));
            }
        }

        child_directories.sort_by_key(|(path, _)| path.clone());
        pending_directories
            .try_reserve(child_directories.len())
            .map_err(|_| {
                discovery_error(
                    &report,
                    &directory,
                    DirectoryDiscoveryErrorKind::AllocationFailed {
                        requested: pending_directories
                            .len()
                            .saturating_add(child_directories.len()),
                    },
                )
            })?;
        pending_directories.extend(child_directories.into_iter().rev());
    }

    let mut inputs = Vec::new();
    inputs.try_reserve_exact(groups.len()).map_err(|_| {
        discovery_error(
            &report,
            root,
            DirectoryDiscoveryErrorKind::AllocationFailed {
                requested: groups.len(),
            },
        )
    })?;
    for (relative_path, mut group) in groups {
        let (path, source_bytes) = match (group.regular_file.take(), group.split_parts.is_empty()) {
            (Some(regular), true) => regular,
            (Some(_), false) => {
                let path = root.join(&relative_path);
                return Err(discovery_error(
                    &report,
                    &path,
                    DirectoryDiscoveryErrorKind::DuplicateLogicalPath,
                ));
            }
            (None, false) => {
                group
                    .split_parts
                    .sort_by(|(left, _), (right, _)| left.file_name().cmp(&right.file_name()));
                let path = group.split_parts[0].0.clone();
                let mut total = 0u64;
                for (_, part_size) in group.split_parts {
                    total = total.checked_add(part_size).ok_or_else(|| {
                        discovery_error(
                            &report,
                            &path,
                            DirectoryDiscoveryErrorKind::SourceSizeOverflow,
                        )
                    })?;
                }
                (path, total)
            }
            (None, true) => continue,
        };
        inputs.push(DiscoveredInput {
            path,
            relative_path,
            source_bytes,
        });
    }

    Ok((inputs, report))
}

fn split_logical_path(path: &Path) -> (PathBuf, bool) {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return (path.to_path_buf(), false);
    };
    let Some((base, suffix)) = name.rsplit_once(".split") else {
        return (path.to_path_buf(), false);
    };
    if base.is_empty() || suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return (path.to_path_buf(), false);
    }
    (path.with_file_name(base), true)
}

fn discovery_error(
    report: &DirectoryDiscoveryReport,
    path: &Path,
    kind: DirectoryDiscoveryErrorKind,
) -> DirectoryDiscoveryError {
    DirectoryDiscoveryError {
        report: report.clone(),
        path: path.to_path_buf(),
        kind,
    }
}

impl Display for DirectoryDiscoveryErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::InputIsNotDirectory => formatter.write_str("input path is not a directory"),
            Self::SymbolicLinkNotAllowed => formatter.write_str("symbolic links are not traversed"),
            Self::UnsupportedFileType => {
                formatter.write_str("input contains a non-regular file entry")
            }
            Self::DepthLimitExceeded { depth, limit } => write!(
                formatter,
                "directory depth {depth} exceeds configured limit {limit}"
            ),
            Self::EntryLimitExceeded { count, limit } => write!(
                formatter,
                "directory scan reached {count} entries, exceeding configured limit {limit}"
            ),
            Self::InputLimitExceeded { count, limit } => write!(
                formatter,
                "directory contains {count} logical inputs, exceeding configured limit {limit}"
            ),
            Self::InvalidPathEncoding => {
                formatter.write_str("input path cannot be represented as UTF-8")
            }
            Self::DuplicateLogicalPath => formatter
                .write_str("a regular file and split-file group resolve to the same logical path"),
            Self::SourceSizeOverflow => formatter.write_str("aggregate source size overflows u64"),
            Self::AllocationFailed { requested } => {
                write!(
                    formatter,
                    "could not allocate directory discovery storage for {requested} entries"
                )
            }
        }
    }
}

impl Display for DirectoryDiscoveryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.kind)
    }
}

impl Error for DirectoryDiscoveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.kind.source()
    }
}

impl Error for DirectoryDiscoveryErrorKind {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        discover_directory_inputs, expand_import_paths, split_logical_path,
        DirectoryDiscoveryErrorKind,
    };
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestDiscoveryRoot(PathBuf);

    impl TestDiscoveryRoot {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("assetripper-discovery-{}-{id}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, bytes).unwrap();
            path
        }
    }

    impl Drop for TestDiscoveryRoot {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn logical_file_name(path: &Path) -> String {
        split_logical_path(path)
            .0
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap()
            .to_owned()
    }

    #[test]
    fn split_suffix_recognition_matches_the_source_regex_shape() {
        // `SplitFileRegex` (`.+\.split[0-9]+$`): split parts.
        for name in [
            "a.bundle.split0",
            "a.bundle.split12",
            "level0.split0",
            "sharedassets0.assets.split3",
            "a.split.split1",
        ] {
            assert!(
                split_logical_path(Path::new(name)).1,
                "{name} should fold into a split group"
            );
        }
        // Bare names, missing bases or suffixes, and non-digit tails stay regular.
        for name in [
            "a.bundle",
            "split0",
            "a.split",
            ".split0",
            "a.split0x",
            "a.SPLIT0",
            "sharedassets.assets",
        ] {
            assert!(
                !split_logical_path(Path::new(name)).1,
                "{name} should stay a regular file"
            );
        }
        assert_eq!(logical_file_name(Path::new("a.bundle.split2")), "a.bundle");
        assert_eq!(logical_file_name(Path::new("a.split.split1")), "a.split");
    }

    #[test]
    fn discovers_regular_files_in_lexical_order_with_byte_totals() {
        let root = TestDiscoveryRoot::new();
        root.write("b.bin", b"bbb");
        root.write("a.bin", b"aaaaa");
        root.write("sub/c.bin", b"cc");
        let (inputs, report) = discover_directory_inputs(root.path(), 10_000, 16, 10_000).unwrap();
        let relative: Vec<&Path> = inputs
            .iter()
            .map(|input| input.relative_path.as_path())
            .collect();
        assert_eq!(
            relative,
            [
                Path::new("a.bin"),
                Path::new("b.bin"),
                &Path::new("sub").join("c.bin"),
            ]
        );
        assert_eq!(
            inputs
                .iter()
                .map(|input| input.source_bytes)
                .collect::<Vec<_>>(),
            [5, 3, 2]
        );
        assert_eq!(inputs[2].path, root.path().join("sub/c.bin"));
        assert_eq!(report.directory_entries_scanned, 4);
        assert_eq!(report.discovered_inputs, 3);
        assert_eq!(report.source_bytes, 10);
    }

    #[test]
    fn folds_split_parts_into_one_logical_input() {
        let root = TestDiscoveryRoot::new();
        root.write("data.bundle.split2", b"cccccc");
        root.write("data.bundle.split0", b"dddd");
        root.write("data.bundle.split1", b"eeeee");
        root.write("other.bin", b"o");
        let (inputs, report) = discover_directory_inputs(root.path(), 10_000, 16, 10_000).unwrap();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].relative_path, PathBuf::from("data.bundle"));
        assert_eq!(inputs[0].path, root.path().join("data.bundle.split0"));
        assert_eq!(inputs[0].source_bytes, 15);
        assert_eq!(inputs[1].relative_path, PathBuf::from("other.bin"));
        assert_eq!(report.discovered_inputs, 2);
        assert_eq!(report.source_bytes, 16);
    }

    #[test]
    fn rejects_roots_that_are_not_directories() {
        let root = TestDiscoveryRoot::new();
        let file = root.write("input.bin", b"data");
        let error = discover_directory_inputs(&file, 10_000, 16, 10_000).unwrap_err();
        assert!(
            matches!(error.kind, DirectoryDiscoveryErrorKind::InputIsNotDirectory),
            "unexpected kind: {}",
            error.kind
        );
        let missing = root.path().join("missing");
        let error = discover_directory_inputs(&missing, 10_000, 16, 10_000).unwrap_err();
        assert!(
            matches!(error.kind, DirectoryDiscoveryErrorKind::Io(_)),
            "unexpected kind: {}",
            error.kind
        );
    }

    #[test]
    fn enforces_depth_entry_and_input_limits() {
        let root = TestDiscoveryRoot::new();
        root.write("sub/nested/deep.bin", b"deep");
        let error = discover_directory_inputs(root.path(), 10_000, 1, 10_000).unwrap_err();
        assert!(
            matches!(
                error.kind,
                DirectoryDiscoveryErrorKind::DepthLimitExceeded { .. }
            ),
            "unexpected kind: {}",
            error.kind
        );

        let root = TestDiscoveryRoot::new();
        root.write("one.bin", b"1");
        root.write("two.bin", b"2");
        let error = discover_directory_inputs(root.path(), 1, 16, 10_000).unwrap_err();
        assert!(
            matches!(
                error.kind,
                DirectoryDiscoveryErrorKind::EntryLimitExceeded { .. }
            ),
            "unexpected kind: {}",
            error.kind
        );

        let error = discover_directory_inputs(root.path(), 10_000, 16, 1).unwrap_err();
        assert!(
            matches!(
                error.kind,
                DirectoryDiscoveryErrorKind::InputLimitExceeded { .. }
            ),
            "unexpected kind: {}",
            error.kind
        );
    }

    #[test]
    fn rejects_a_regular_file_colliding_with_a_split_group() {
        let root = TestDiscoveryRoot::new();
        root.write("data", b"regular");
        root.write("data.split0", b"part");
        let error = discover_directory_inputs(root.path(), 10_000, 16, 10_000).unwrap_err();
        assert!(
            matches!(
                error.kind,
                DirectoryDiscoveryErrorKind::DuplicateLogicalPath
            ),
            "unexpected kind: {}",
            error.kind
        );
    }

    /// Mixed `GetAllFilePaths` inputs: files pass through, directories expand,
    /// and missing paths are skipped with a report count.
    #[test]
    fn mixed_file_and_directory_inputs_expand_with_missing_skipped() {
        let root = TestDiscoveryRoot::new();
        let direct = root.write("direct.bin", b"1234");
        root.write("bundle/data.bundle.split0", b"aa");
        root.write("bundle/data.bundle.split1", b"bb");
        let missing = root.path().join("gone.bin");
        let inputs = vec![direct.clone(), root.path().join("bundle"), missing];
        let (expanded, report) = expand_import_paths(&inputs, 10_000, 16, 10_000).unwrap();
        assert_eq!(expanded.len(), 2);
        assert_eq!(expanded[0].path, direct);
        assert_eq!(expanded[0].relative_path, PathBuf::from("direct.bin"));
        assert_eq!(expanded[0].source_bytes, 4);
        assert_eq!(expanded[1].relative_path, PathBuf::from("data.bundle"));
        assert_eq!(expanded[1].source_bytes, 4);
        assert_eq!(
            expanded[1].path,
            root.path().join("bundle/data.bundle.split0")
        );
        assert_eq!(report.discovered_inputs, 2);
        assert_eq!(report.skipped_inputs, 1);
        assert_eq!(report.source_bytes, 8);
    }

    #[test]
    fn import_path_expansion_enforces_the_total_input_limit() {
        let root = TestDiscoveryRoot::new();
        root.write("a.bin", b"a");
        root.write("sub/b.bin", b"b");
        let inputs = vec![root.path().join("a.bin"), root.path().join("sub")];
        let error = expand_import_paths(&inputs, 10_000, 16, 1).unwrap_err();
        assert!(
            matches!(
                error.kind,
                DirectoryDiscoveryErrorKind::InputLimitExceeded { .. }
            ),
            "unexpected kind: {}",
            error.kind
        );
    }
}
