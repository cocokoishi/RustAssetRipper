//! Source-shaped lookup for SerializedFile external dependencies in the mounted VFS.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use crate::scheme::{DispatchedFile, SchemeError, SchemeReader, SchemeReaderLimits};
use crate::serialized_file::metadata::FileIdentifier;

use super::filesystem::VirtualFileSystem;
use super::special_names::{
    fix_file_identifier, is_builtin_extra, is_default_resource, BUILTIN_EXTRA_NAME_1,
    BUILTIN_EXTRA_NAME_2, DEFAULT_RESOURCE_NAME_1, DEFAULT_RESOURCE_NAME_2,
};
use super::stream::SmartStream;

/// One source-recognized file identity and its path in the mounted virtual filesystem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyFilePath {
    /// The source `FileBase.Name` or structure name. It is normalized on insertion.
    pub file_name: String,
    pub virtual_path: String,
}

/// Bounds for constructing and querying a VFS dependency index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DependencyIndexLimits {
    pub max_registered_files: usize,
    pub max_data_paths: usize,
    pub max_name_bytes: usize,
    pub max_virtual_path_bytes: usize,
    pub max_total_index_bytes: usize,
    pub max_candidate_paths: usize,
}

impl Default for DependencyIndexLimits {
    fn default() -> Self {
        Self {
            max_registered_files: 1_000_000,
            max_data_paths: 100_000,
            max_name_bytes: 1024 * 1024,
            max_virtual_path_bytes: 1024 * 1024,
            max_total_index_bytes: 64 * 1024 * 1024,
            max_candidate_paths: 100_000,
        }
    }
}

/// A resolved dependency candidate and the source lookup path that found it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyCandidate {
    pub virtual_path: String,
    pub source: DependencyCandidateSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencyCandidateSource {
    /// Exact normalized name match from the caller's source-compatible file registry.
    RegisteredFileName,
    /// Exact `DataPath / dependency` existence match.
    DataPath,
    /// Unity default-resource or builtin-extra fallback under DataPath, Resources, or unity.
    EngineResourceFallback,
}

/// Lookup outcome. More than one viable path is kept ambiguous instead of selecting one silently.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DependencyLookup {
    Found(DependencyCandidate),
    Missing {
        dependency_name: String,
    },
    Ambiguous {
        dependency_name: String,
        candidates: Vec<DependencyCandidate>,
    },
}

/// A bounded index matching the source `RequestDependency` search stages.
///
/// The caller provides file names already recognized by its game/container structure, plus the
/// ordered DataPaths for that structure. Lookup checks registered names first, then exact paths
/// under each DataPath, then the source engine-resource aliases. Duplicate viable paths are
/// surfaced as ambiguous because selecting one would hide an identity collision.
pub struct DependencyIndex<'a> {
    filesystem: &'a VirtualFileSystem,
    /// Sorted by normalized file name, then virtual path. Duplicate names remain visible.
    registered_files: Vec<(String, String)>,
    data_paths: Vec<String>,
    limits: DependencyIndexLimits,
}

impl<'a> DependencyIndex<'a> {
    pub fn new(
        filesystem: &'a VirtualFileSystem,
        registered_files: &[DependencyFilePath],
        data_paths: &[String],
    ) -> Result<Self, DependencyIndexBuildError> {
        Self::with_limits(
            filesystem,
            registered_files,
            data_paths,
            DependencyIndexLimits::default(),
        )
    }

    pub fn with_limits(
        filesystem: &'a VirtualFileSystem,
        registered_files: &[DependencyFilePath],
        data_paths: &[String],
        limits: DependencyIndexLimits,
    ) -> Result<Self, DependencyIndexBuildError> {
        if registered_files.len() > limits.max_registered_files {
            return Err(
                DependencyIndexBuildError::RegisteredFileCountLimitExceeded {
                    count: registered_files.len(),
                    limit: limits.max_registered_files,
                },
            );
        }
        if data_paths.len() > limits.max_data_paths {
            return Err(DependencyIndexBuildError::DataPathCountLimitExceeded {
                count: data_paths.len(),
                limit: limits.max_data_paths,
            });
        }

        let mut total_index_bytes = 0usize;
        let mut indexed_files = Vec::new();
        indexed_files
            .try_reserve_exact(registered_files.len())
            .map_err(|_| DependencyIndexBuildError::AllocationFailed {
                resource: "registered dependency paths",
                requested: registered_files.len(),
            })?;
        for (index, entry) in registered_files.iter().enumerate() {
            if entry.file_name.len() > limits.max_name_bytes {
                return Err(DependencyIndexBuildError::NameTooLong {
                    entry_index: index,
                    byte_count: entry.file_name.len(),
                    limit: limits.max_name_bytes,
                });
            }
            let name = fix_file_identifier(&entry.file_name);
            if name.is_empty() {
                return Err(DependencyIndexBuildError::EmptyFileName { entry_index: index });
            }
            if name.len() > limits.max_name_bytes {
                return Err(DependencyIndexBuildError::NameTooLong {
                    entry_index: index,
                    byte_count: name.len(),
                    limit: limits.max_name_bytes,
                });
            }
            let path = normalize_virtual_path(&entry.virtual_path, limits, index)?;
            total_index_bytes = add_index_bytes(
                total_index_bytes,
                name.len().saturating_add(path.len()),
                limits.max_total_index_bytes,
            )?;
            indexed_files.push((name, path));
        }
        indexed_files.sort_unstable();
        indexed_files.dedup();

        let mut normalized_data_paths = Vec::new();
        normalized_data_paths
            .try_reserve_exact(data_paths.len())
            .map_err(|_| DependencyIndexBuildError::AllocationFailed {
                resource: "dependency DataPaths",
                requested: data_paths.len(),
            })?;
        let mut seen_data_paths = BTreeSet::new();
        for (index, data_path) in data_paths.iter().enumerate() {
            let path = normalize_virtual_path(data_path, limits, index)?;
            if !seen_data_paths.insert(path.clone()) {
                continue;
            }
            total_index_bytes =
                add_index_bytes(total_index_bytes, path.len(), limits.max_total_index_bytes)?;
            normalized_data_paths.push(path);
        }

        Ok(Self {
            filesystem,
            registered_files: indexed_files,
            data_paths: normalized_data_paths,
            limits,
        })
    }

    pub fn resolve(
        &self,
        identifier: &FileIdentifier,
    ) -> Result<DependencyLookup, DependencyLookupError> {
        if identifier.path_name_origin.len() > self.limits.max_name_bytes {
            return Err(DependencyLookupError::NameTooLong {
                byte_count: identifier.path_name_origin.len(),
                limit: self.limits.max_name_bytes,
            });
        }
        let dependency_name = identifier.fixed_path_name();
        if dependency_name.len() > self.limits.max_name_bytes {
            return Err(DependencyLookupError::NameTooLong {
                byte_count: dependency_name.len(),
                limit: self.limits.max_name_bytes,
            });
        }
        validate_relative_dependency_name(&dependency_name)?;

        let first = self
            .registered_files
            .partition_point(|(name, _)| name < &dependency_name);
        let end = self
            .registered_files
            .partition_point(|(name, _)| name <= &dependency_name);
        match end - first {
            1 => {
                let path = self.registered_files[first].1.clone();
                return Ok(DependencyLookup::Found(DependencyCandidate {
                    virtual_path: path,
                    source: DependencyCandidateSource::RegisteredFileName,
                }));
            }
            count if count > 1 => {
                if count > self.limits.max_candidate_paths {
                    return Err(DependencyLookupError::CandidateCountLimitExceeded {
                        limit: self.limits.max_candidate_paths,
                    });
                }
                let mut candidates = Vec::new();
                candidates
                    .try_reserve_exact(count)
                    .map_err(|_| DependencyLookupError::AllocationFailed { requested: count })?;
                for (_, path) in &self.registered_files[first..end] {
                    push_candidate(
                        &mut candidates,
                        path,
                        DependencyCandidateSource::RegisteredFileName,
                        self.limits.max_candidate_paths,
                    )?;
                }
                return Ok(DependencyLookup::Ambiguous {
                    dependency_name,
                    candidates,
                });
            }
            _ => {}
        }

        let mut candidates = Vec::new();
        for data_path in &self.data_paths {
            let exact_path = join_virtual_path(data_path, &dependency_name);
            if self.file_exists(&exact_path)? {
                push_candidate(
                    &mut candidates,
                    &exact_path,
                    DependencyCandidateSource::DataPath,
                    self.limits.max_candidate_paths,
                )?;
                continue;
            }

            let aliases = if is_default_resource(&dependency_name) {
                Some([DEFAULT_RESOURCE_NAME_1, DEFAULT_RESOURCE_NAME_2])
            } else if is_builtin_extra(&dependency_name) {
                Some([BUILTIN_EXTRA_NAME_1, BUILTIN_EXTRA_NAME_2])
            } else {
                None
            };
            if let Some(aliases) = aliases {
                if let Some(path) = self.find_engine_dependency(data_path, &aliases)? {
                    push_candidate(
                        &mut candidates,
                        &path,
                        DependencyCandidateSource::EngineResourceFallback,
                        self.limits.max_candidate_paths,
                    )?;
                }
            }
        }

        candidates.sort_by(|left, right| {
            left.virtual_path
                .cmp(&right.virtual_path)
                .then(candidate_source_rank(left.source).cmp(&candidate_source_rank(right.source)))
        });
        candidates.dedup_by(|left, right| left.virtual_path == right.virtual_path);
        match candidates.len() {
            0 => Ok(DependencyLookup::Missing { dependency_name }),
            1 => Ok(DependencyLookup::Found(candidates.remove(0))),
            _ => Ok(DependencyLookup::Ambiguous {
                dependency_name,
                candidates,
            }),
        }
    }

    /// Open a unique lookup result as a readable VFS stream for normal scheme dispatch.
    pub fn open_candidate(&self, candidate: &DependencyCandidate) -> io::Result<SmartStream> {
        self.filesystem.open_file(&candidate.virtual_path)
    }

    /// Open and scheme-dispatch one resolved candidate using the normal bounded file reader.
    pub fn dispatch_candidate(
        &self,
        candidate: &DependencyCandidate,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'static>, DependencyDispatchError> {
        let mut stream = self
            .open_candidate(candidate)
            .map_err(DependencyDispatchError::Open)?;
        SchemeReader::read_stream(&mut stream, limits).map_err(DependencyDispatchError::Scheme)
    }

    fn find_engine_dependency(
        &self,
        data_path: &str,
        aliases: &[&str; 2],
    ) -> Result<Option<String>, DependencyLookupError> {
        for alias in aliases {
            let candidates = [
                join_virtual_path(data_path, alias),
                join_virtual_path(&join_virtual_path(data_path, "Resources"), alias),
                join_virtual_path(&join_virtual_path(data_path, "unity"), alias),
            ];
            for candidate in candidates {
                if self.file_exists(&candidate)? {
                    return Ok(Some(candidate));
                }
            }
        }
        Ok(None)
    }

    fn file_exists(&self, path: &str) -> Result<bool, DependencyLookupError> {
        self.filesystem.file_exists(Some(path)).map_err(|source| {
            DependencyLookupError::FileSystem {
                path: path.to_owned(),
                source,
            }
        })
    }
}

fn push_candidate(
    candidates: &mut Vec<DependencyCandidate>,
    path: &str,
    source: DependencyCandidateSource,
    maximum: usize,
) -> Result<(), DependencyLookupError> {
    if candidates.len() >= maximum {
        return Err(DependencyLookupError::CandidateCountLimitExceeded { limit: maximum });
    }
    candidates
        .try_reserve(1)
        .map_err(|_| DependencyLookupError::AllocationFailed { requested: 1 })?;
    candidates.push(DependencyCandidate {
        virtual_path: path.to_owned(),
        source,
    });
    Ok(())
}

fn candidate_source_rank(source: DependencyCandidateSource) -> u8 {
    match source {
        DependencyCandidateSource::RegisteredFileName => 0,
        DependencyCandidateSource::DataPath => 1,
        DependencyCandidateSource::EngineResourceFallback => 2,
    }
}

fn normalize_virtual_path(
    path: &str,
    limits: DependencyIndexLimits,
    entry_index: usize,
) -> Result<String, DependencyIndexBuildError> {
    if path.len() > limits.max_virtual_path_bytes {
        return Err(DependencyIndexBuildError::VirtualPathTooLong {
            entry_index,
            byte_count: path.len(),
            limit: limits.max_virtual_path_bytes,
        });
    }
    let replaced = path.replace('\\', "/");
    let mut parts = Vec::new();
    for component in replaced
        .split('/')
        .filter(|component| !component.is_empty())
    {
        if component == "." || component == ".." || component.contains('\0') {
            return Err(DependencyIndexBuildError::InvalidVirtualPath {
                entry_index,
                path: path.to_owned(),
            });
        }
        parts.push(component);
    }
    if parts.is_empty() {
        return Ok("/".to_owned());
    }
    Ok(format!("/{}", parts.join("/")))
}

fn validate_relative_dependency_name(name: &str) -> Result<(), DependencyLookupError> {
    if name.starts_with('/')
        || name.starts_with('\\')
        || name.contains(':')
        || name
            .split(['/', '\\'])
            .any(|component| component == "." || component == ".." || component.contains('\0'))
    {
        return Err(DependencyLookupError::InvalidDependencyName {
            dependency_name: name.to_owned(),
        });
    }
    Ok(())
}

fn join_virtual_path(parent: &str, child: &str) -> String {
    let parent = parent.trim_end_matches('/');
    let child = child.trim_start_matches('/');
    if parent.is_empty() {
        format!("/{child}")
    } else {
        format!("{parent}/{child}")
    }
}

fn add_index_bytes(
    current: usize,
    addition: usize,
    maximum: usize,
) -> Result<usize, DependencyIndexBuildError> {
    let total = current
        .checked_add(addition)
        .ok_or(DependencyIndexBuildError::TotalIndexBytesOverflow)?;
    if total > maximum {
        return Err(DependencyIndexBuildError::TotalIndexBytesLimitExceeded {
            byte_count: total,
            limit: maximum,
        });
    }
    Ok(total)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DependencyIndexBuildError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    RegisteredFileCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    DataPathCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    NameTooLong {
        entry_index: usize,
        byte_count: usize,
        limit: usize,
    },
    EmptyFileName {
        entry_index: usize,
    },
    VirtualPathTooLong {
        entry_index: usize,
        byte_count: usize,
        limit: usize,
    },
    InvalidVirtualPath {
        entry_index: usize,
        path: String,
    },
    TotalIndexBytesOverflow,
    TotalIndexBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
}

impl Display for DependencyIndexBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::RegisteredFileCountLimitExceeded { count, limit } => {
                write!(f, "registered file count {count} exceeds limit {limit}")
            }
            Self::DataPathCountLimitExceeded { count, limit } => {
                write!(f, "DataPath count {count} exceeds limit {limit}")
            }
            Self::NameTooLong {
                entry_index,
                byte_count,
                limit,
            } => write!(
                f,
                "file name at index {entry_index} has {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::EmptyFileName { entry_index } => {
                write!(f, "registered file at index {entry_index} has an empty name")
            }
            Self::VirtualPathTooLong {
                entry_index,
                byte_count,
                limit,
            } => write!(
                f,
                "virtual path at index {entry_index} has {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::InvalidVirtualPath { entry_index, path } => write!(
                f,
                "virtual path at index {entry_index} contains an unsafe component: '{path}'"
            ),
            Self::TotalIndexBytesOverflow => f.write_str("dependency index byte count overflows"),
            Self::TotalIndexBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "dependency index stores {byte_count} bytes, exceeding limit {limit}"
            ),
        }
    }
}

impl Error for DependencyIndexBuildError {}

#[derive(Debug)]
pub enum DependencyLookupError {
    AllocationFailed { requested: usize },
    NameTooLong { byte_count: usize, limit: usize },
    InvalidDependencyName { dependency_name: String },
    CandidateCountLimitExceeded { limit: usize },
    FileSystem { path: String, source: io::Error },
}

#[derive(Debug)]
pub enum DependencyDispatchError {
    Open(io::Error),
    Scheme(SchemeError),
}

impl Display for DependencyDispatchError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(error) => write!(f, "could not open resolved dependency: {error}"),
            Self::Scheme(error) => write!(f, "could not dispatch resolved dependency: {error}"),
        }
    }
}

impl Error for DependencyDispatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open(error) => Some(error),
            Self::Scheme(error) => Some(error),
        }
    }
}

impl Display for DependencyLookupError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { requested } => {
                write!(f, "could not allocate {requested} dependency candidates")
            }
            Self::NameTooLong { byte_count, limit } => write!(
                f,
                "dependency name has {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::InvalidDependencyName { dependency_name } => write!(
                f,
                "dependency name contains an absolute path or unsafe component: '{dependency_name}'"
            ),
            Self::CandidateCountLimitExceeded { limit } => {
                write!(f, "dependency candidate count exceeds limit {limit}")
            }
            Self::FileSystem { path, source } => {
                write!(f, "could not check dependency path '{path}': {source}")
            }
        }
    }
}

impl Error for DependencyLookupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FileSystem { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DependencyCandidateSource, DependencyFilePath, DependencyIndex, DependencyIndexLimits,
        DependencyLookup, DependencyLookupError,
    };
    use crate::io::filesystem::VirtualFileSystem;
    use crate::serialized_file::metadata::FileIdentifier;

    fn identifier(path_name: &str) -> FileIdentifier {
        FileIdentifier {
            asset_path: Some(String::new()),
            guid: None,
            file_type: None,
            path_name_origin: path_name.to_owned(),
        }
    }

    fn insert_file(filesystem: &mut VirtualFileSystem, path: &str) {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, &[1, 2, 3]).unwrap();
    }

    #[test]
    fn registered_file_name_match_precedes_datapath_search() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/bundle/shared.assets");
        insert_file(&mut filesystem, "/game/Data/shared.assets");
        let registered = [DependencyFilePath {
            file_name: "Shared.Assets".to_owned(),
            virtual_path: "/bundle/shared.assets".to_owned(),
        }];
        let data_paths = ["/game/Data".to_owned()];
        let index = DependencyIndex::new(&filesystem, &registered, &data_paths).unwrap();

        let DependencyLookup::Found(candidate) = index
            .resolve(&identifier("resources/SHARED.ASSETS"))
            .unwrap()
        else {
            panic!("registered file name should resolve uniquely");
        };
        assert_eq!(
            candidate,
            super::DependencyCandidate {
                virtual_path: "/bundle/shared.assets".to_owned(),
                source: DependencyCandidateSource::RegisteredFileName,
            }
        );
        let dispatched = index
            .dispatch_candidate(&candidate, crate::scheme::SchemeReaderLimits::default())
            .unwrap();
        assert_eq!(dispatched.kind(), crate::scheme::FileKind::Resource);
    }

    #[test]
    fn meta_bundle_identity_uses_guid_but_platform_dependency_uses_path_name() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/bundle/by-guid.asset");
        insert_file(&mut filesystem, "/bundle/by-path.asset");
        let registered = [
            DependencyFilePath {
                file_name: "76543210fedcba9800000000ffffffff".to_owned(),
                virtual_path: "/bundle/by-guid.asset".to_owned(),
            },
            DependencyFilePath {
                file_name: "ignored-name.asset".to_owned(),
                virtual_path: "/bundle/by-path.asset".to_owned(),
            },
        ];
        let index = DependencyIndex::new(&filesystem, &registered, &[]).unwrap();
        let meta_identifier = FileIdentifier {
            asset_path: Some(String::new()),
            guid: Some([0x0123_4567, 0x89ab_cdef, 0, u32::MAX]),
            file_type: Some(3),
            path_name_origin: "Library/ignored-name.asset".to_owned(),
        };

        assert_eq!(
            meta_identifier.get_file_path().as_deref(),
            Some("76543210fedcba9800000000ffffffff")
        );
        assert_eq!(
            index.resolve(&meta_identifier).unwrap(),
            DependencyLookup::Found(super::DependencyCandidate {
                virtual_path: "/bundle/by-path.asset".to_owned(),
                source: DependencyCandidateSource::RegisteredFileName,
            })
        );
    }

    #[test]
    fn non_meta_identifier_keeps_path_identity_even_when_guid_is_present() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/data/ordinary.assets");
        let index = DependencyIndex::new(&filesystem, &[], &["/data".to_owned()]).unwrap();
        let ordinary_identifier = FileIdentifier {
            asset_path: Some(String::new()),
            guid: Some([0x0123_4567, 0x89ab_cdef, 0, u32::MAX]),
            file_type: Some(2),
            path_name_origin: "ordinary.assets".to_owned(),
        };

        assert_eq!(
            index.resolve(&ordinary_identifier).unwrap(),
            DependencyLookup::Found(super::DependencyCandidate {
                virtual_path: "/data/ordinary.assets".to_owned(),
                source: DependencyCandidateSource::DataPath,
            })
        );
    }

    #[test]
    fn malformed_meta_bundle_identity_without_guid_is_unavailable() {
        let invalid_meta_identifier = FileIdentifier {
            asset_path: Some(String::new()),
            guid: None,
            file_type: Some(3),
            path_name_origin: "fallback.asset".to_owned(),
        };

        assert_eq!(invalid_meta_identifier.get_file_path(), None);
    }

    #[test]
    fn datapath_finds_files_and_missing_inputs_are_explicit() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/game/Data/nested/scene.assets");
        let registered = [];
        let data_paths = ["/game/Data".to_owned()];
        let index = DependencyIndex::new(&filesystem, &registered, &data_paths).unwrap();

        assert_eq!(
            index.resolve(&identifier("nested/scene.assets")).unwrap(),
            DependencyLookup::Found(super::DependencyCandidate {
                virtual_path: "/game/Data/nested/scene.assets".to_owned(),
                source: DependencyCandidateSource::DataPath,
            })
        );
        assert_eq!(
            index.resolve(&identifier("absent.assets")).unwrap(),
            DependencyLookup::Missing {
                dependency_name: "absent.assets".to_owned(),
            }
        );
    }

    #[test]
    fn duplicate_file_names_and_datapaths_are_reported_as_ambiguous() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/bundle-a/shared.assets");
        insert_file(&mut filesystem, "/bundle-b/shared.assets");
        let registered = [
            DependencyFilePath {
                file_name: "shared.assets".to_owned(),
                virtual_path: "/bundle-a/shared.assets".to_owned(),
            },
            DependencyFilePath {
                file_name: "SHARED.ASSETS".to_owned(),
                virtual_path: "/bundle-b/shared.assets".to_owned(),
            },
        ];
        let data_paths = [];
        let index = DependencyIndex::new(&filesystem, &registered, &data_paths).unwrap();
        assert!(matches!(
            index.resolve(&identifier("shared.assets")).unwrap(),
            DependencyLookup::Ambiguous { candidates, .. } if candidates.len() == 2
        ));
        let limited_index = DependencyIndex::with_limits(
            &filesystem,
            &registered,
            &data_paths,
            DependencyIndexLimits {
                max_candidate_paths: 1,
                ..DependencyIndexLimits::default()
            },
        )
        .unwrap();
        assert!(matches!(
            limited_index.resolve(&identifier("shared.assets")),
            Err(DependencyLookupError::CandidateCountLimitExceeded { limit: 1 })
        ));

        let registered = [];
        let data_paths = ["/bundle-a".to_owned(), "/bundle-b".to_owned()];
        let index = DependencyIndex::new(&filesystem, &registered, &data_paths).unwrap();
        assert!(matches!(
            index.resolve(&identifier("shared.assets")).unwrap(),
            DependencyLookup::Ambiguous { candidates, .. }
                if candidates.iter().map(|candidate| candidate.virtual_path.as_str()).collect::<Vec<_>>()
                    == ["/bundle-a/shared.assets", "/bundle-b/shared.assets"]
        ));
    }

    #[test]
    fn engine_alias_search_matches_source_alias_and_directory_order() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/Data/Resources/unity default resources",
        );
        insert_file(&mut filesystem, "/game/Data/unity_default_resources");
        insert_file(&mut filesystem, "/game/Data/unity/unity builtin extra");
        let registered = [];
        let data_paths = ["/game/Data".to_owned()];
        let index = DependencyIndex::new(&filesystem, &registered, &data_paths).unwrap();

        assert_eq!(
            index
                .resolve(&identifier("unity default resources"))
                .unwrap(),
            DependencyLookup::Found(super::DependencyCandidate {
                virtual_path: "/game/Data/Resources/unity default resources".to_owned(),
                source: DependencyCandidateSource::EngineResourceFallback,
            })
        );
        assert_eq!(
            index.resolve(&identifier("unity builtin extra")).unwrap(),
            DependencyLookup::Found(super::DependencyCandidate {
                virtual_path: "/game/Data/unity/unity builtin extra".to_owned(),
                source: DependencyCandidateSource::EngineResourceFallback,
            })
        );
    }

    #[test]
    fn dependency_traversal_and_candidate_bounds_fail_explicitly() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(&mut filesystem, "/data/../secret.assets");
        let registered = [];
        let data_paths = ["/data".to_owned()];
        let index = DependencyIndex::new(&filesystem, &registered, &data_paths).unwrap();
        assert!(matches!(
            index.resolve(&identifier("../secret.assets")),
            Err(DependencyLookupError::InvalidDependencyName { .. })
        ));
        assert!(matches!(
            index.resolve(&identifier("..\\secret.assets")),
            Err(DependencyLookupError::InvalidDependencyName { .. })
        ));
        drop(index);

        insert_file(&mut filesystem, "/data/shared.assets");
        insert_file(&mut filesystem, "/other/shared.assets");
        let data_paths = ["/data".to_owned(), "/other".to_owned()];
        let index = DependencyIndex::with_limits(
            &filesystem,
            &registered,
            &data_paths,
            DependencyIndexLimits {
                max_candidate_paths: 1,
                ..DependencyIndexLimits::default()
            },
        )
        .unwrap();
        assert!(matches!(
            index.resolve(&identifier("shared.assets")),
            Err(DependencyLookupError::CandidateCountLimitExceeded { limit: 1 })
        ));
    }
}
