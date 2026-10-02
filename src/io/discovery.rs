//! Bounded discovery of regular files beneath a local input directory.

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
