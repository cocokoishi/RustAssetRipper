use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::metadata::{FileIdentifier, SerializedFileMetadata, TypeTree};

use super::{external_table_index, ObjectReferenceGraph};

type ExternalDependencyResolver<'a> = dyn FnMut(usize, &FileIdentifier) -> Option<usize> + 'a;

#[derive(Clone, Copy)]
struct ReferenceSource<'a> {
    file_index: usize,
    metadata: &'a SerializedFileMetadata,
    graph: &'a ObjectReferenceGraph,
}
/// `Bundle.ResolveCollection` engine-resource fallback: when the fixed dependency
/// name has no loaded file, these spellings alias each other before the dependency
/// counts as missing. Resolver-driven builds delegate this fallback to the caller
/// resolver; it applies here to the flat name join only.
pub(super) fn engine_resource_alias(normalized_name: &str) -> Option<&'static str> {
    match normalized_name {
        crate::io::special_names::DEFAULT_RESOURCE_NAME_1 => {
            Some(crate::io::special_names::DEFAULT_RESOURCE_NAME_2)
        }
        crate::io::special_names::DEFAULT_RESOURCE_NAME_2 => {
            Some(crate::io::special_names::DEFAULT_RESOURCE_NAME_1)
        }
        crate::io::special_names::BUILTIN_EXTRA_NAME_1 => {
            Some(crate::io::special_names::BUILTIN_EXTRA_NAME_2)
        }
        crate::io::special_names::BUILTIN_EXTRA_NAME_2 => {
            Some(crate::io::special_names::BUILTIN_EXTRA_NAME_1)
        }
        _ => None,
    }
}

/// Resource bounds for joining references from a set of already parsed SerializedFiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MultiFileReferenceGraphLimits {
    pub max_files: usize,
    pub max_references: usize,
    pub max_file_name_bytes: usize,
    pub max_total_file_name_bytes: usize,
}

impl Default for MultiFileReferenceGraphLimits {
    fn default() -> Self {
        Self {
            max_files: 1_000_000,
            max_references: 5_000_000,
            max_file_name_bytes: 1024 * 1024,
            max_total_file_name_bytes: 64 * 1024 * 1024,
        }
    }
}

/// One parsed file and its independently built per-file reference scan.
///
/// `file_name` is the identity exposed by the source `FileBase.Name`; names are normalized with
/// the same `SpecialFileNames.FixFileIdentifier` rules used by `FileIdentifier.PathName`.
pub struct NamedObjectReferenceGraph<'a> {
    pub file_name: &'a str,
    pub metadata: &'a SerializedFileMetadata,
    pub graph: &'a ObjectReferenceGraph,
}

/// One PPtr resolution across a set of loaded SerializedFiles, retaining the serialized pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultiFileObjectReference<'a> {
    pub source_file_index: usize,
    pub source_object_index: usize,
    pub source_path_id: i64,
    pub field_path: &'a str,
    pub file_id: i32,
    pub path_id: i64,
    pub resolution: MultiFilePPtrResolution,
}

/// Stable object identity within the caller-ordered set joined by a multi-file graph.
///
/// File indexes disambiguate duplicate file names; object indexes preserve duplicate PathIDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultiFileObjectIdentity<'a> {
    pub file_index: usize,
    pub normalized_file_name: &'a str,
    pub object_index: usize,
    pub path_id: i64,
}

/// Explicit result of resolving a PPtr against the supplied files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultiFilePPtrResolution {
    Null,
    Object {
        target_file_index: usize,
        target_object_index: usize,
    },
    MissingLocalObject {
        path_id: i64,
    },
    AmbiguousLocalObject {
        path_id: i64,
        match_count: usize,
    },
    MissingDependency {
        external_index: usize,
        path_id: i64,
    },
    AmbiguousDependency {
        external_index: usize,
        path_id: i64,
        candidate_count: usize,
    },
    MissingExternalObject {
        target_file_index: usize,
        path_id: i64,
    },
    AmbiguousExternalObject {
        target_file_index: usize,
        path_id: i64,
        match_count: usize,
    },
    InvalidFileId {
        file_id: i32,
        external_count: usize,
    },
}

/// A deterministic cross-file PPtr graph over files that have already been loaded and scanned.
///
/// This joins existing per-file graphs by normalized serialized-file name. It does not discover
/// or load missing files from disk or containers; those remain `MissingDependency` results.
/// Engine-resource externals additionally try their `Bundle.ResolveCollection` spelling alias
/// (`unity default resources` / `unity_default_resources`, `unity builtin extra` /
/// `unity_builtin_extra`) when the exact name has no loaded file; resolver-driven builds
/// delegate that fallback to the caller resolver instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultiFileReferenceGraph<'a> {
    normalized_file_names: Vec<String>,
    /// File indices sorted by normalized name and then caller-supplied file index. Duplicate
    /// names remain separate entries so they resolve as ambiguous instead of being merged.
    name_index: Vec<usize>,
    file_graphs: Vec<&'a ObjectReferenceGraph>,
    file_metadata: Vec<&'a SerializedFileMetadata>,
    references: Vec<MultiFileObjectReference<'a>>,
    input_graphs_complete: bool,
}

impl<'a> MultiFileReferenceGraph<'a> {
    pub fn build(
        files: &[NamedObjectReferenceGraph<'a>],
    ) -> Result<Self, MultiFileReferenceGraphBuildError> {
        Self::build_with_limits(files, MultiFileReferenceGraphLimits::default())
    }

    pub fn build_with_limits(
        files: &[NamedObjectReferenceGraph<'a>],
        limits: MultiFileReferenceGraphLimits,
    ) -> Result<Self, MultiFileReferenceGraphBuildError> {
        Self::build_internal(files, limits, None)
    }

    pub(crate) fn build_with_dependency_resolver<F>(
        files: &[NamedObjectReferenceGraph<'a>],
        limits: MultiFileReferenceGraphLimits,
        mut dependency_resolver: F,
    ) -> Result<Self, MultiFileReferenceGraphBuildError>
    where
        F: FnMut(usize, &FileIdentifier) -> Option<usize>,
    {
        Self::build_internal(files, limits, Some(&mut dependency_resolver))
    }

    fn build_internal(
        files: &[NamedObjectReferenceGraph<'a>],
        limits: MultiFileReferenceGraphLimits,
        mut dependency_resolver: Option<&mut ExternalDependencyResolver<'_>>,
    ) -> Result<Self, MultiFileReferenceGraphBuildError> {
        if files.len() > limits.max_files {
            return Err(MultiFileReferenceGraphBuildError::FileCountLimitExceeded {
                file_count: files.len(),
                limit: limits.max_files,
            });
        }

        let mut reference_count = 0usize;
        for file in files {
            reference_count = reference_count
                .checked_add(file.graph.references.len())
                .ok_or(MultiFileReferenceGraphBuildError::ReferenceCountOverflow)?;
        }
        if reference_count > limits.max_references {
            return Err(
                MultiFileReferenceGraphBuildError::ReferenceCountLimitExceeded {
                    reference_count,
                    limit: limits.max_references,
                },
            );
        }

        let mut normalized_file_names = Vec::new();
        normalized_file_names
            .try_reserve_exact(files.len())
            .map_err(|_| MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "normalized file names",
                requested: files.len(),
            })?;
        let mut name_index = Vec::new();
        name_index.try_reserve_exact(files.len()).map_err(|_| {
            MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "file-name index",
                requested: files.len(),
            }
        })?;
        let mut file_graphs = Vec::new();
        file_graphs.try_reserve_exact(files.len()).map_err(|_| {
            MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "per-file reference graph index",
                requested: files.len(),
            }
        })?;
        let mut file_metadata = Vec::new();
        file_metadata.try_reserve_exact(files.len()).map_err(|_| {
            MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "per-file metadata index",
                requested: files.len(),
            }
        })?;
        let mut total_file_name_bytes = 0usize;
        for (file_index, file) in files.iter().enumerate() {
            if file.file_name.len() > limits.max_file_name_bytes {
                return Err(MultiFileReferenceGraphBuildError::FileNameLimitExceeded {
                    file_index,
                    byte_count: file.file_name.len(),
                    limit: limits.max_file_name_bytes,
                });
            }
            let normalized_name = crate::io::special_names::fix_file_identifier(file.file_name);
            if normalized_name.len() > limits.max_file_name_bytes {
                return Err(MultiFileReferenceGraphBuildError::FileNameLimitExceeded {
                    file_index,
                    byte_count: normalized_name.len(),
                    limit: limits.max_file_name_bytes,
                });
            }
            total_file_name_bytes = total_file_name_bytes
                .checked_add(normalized_name.len())
                .ok_or(MultiFileReferenceGraphBuildError::TotalFileNameBytesOverflow)?;
            if total_file_name_bytes > limits.max_total_file_name_bytes {
                return Err(
                    MultiFileReferenceGraphBuildError::TotalFileNameBytesLimitExceeded {
                        byte_count: total_file_name_bytes,
                        limit: limits.max_total_file_name_bytes,
                    },
                );
            }
            normalized_file_names.push(normalized_name);
            name_index.push(file_index);
            file_graphs.push(file.graph);
            file_metadata.push(file.metadata);
        }
        name_index.sort_unstable_by(|left, right| {
            normalized_file_names[*left]
                .cmp(&normalized_file_names[*right])
                .then(left.cmp(right))
        });

        let mut references = Vec::new();
        references.try_reserve_exact(reference_count).map_err(|_| {
            MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "cross-file references",
                requested: reference_count,
            }
        })?;

        let input_graphs_complete = files.iter().all(|file| file.graph.is_complete());
        let mut result = Self {
            normalized_file_names,
            name_index,
            file_graphs,
            file_metadata,
            references,
            input_graphs_complete,
        };
        for (source_file_index, source_file) in files.iter().enumerate() {
            let mut dependency_targets = HashMap::<usize, Option<usize>>::new();
            for reference in source_file.graph.references() {
                let resolution = if let Some(resolver) = dependency_resolver.as_deref_mut() {
                    result.resolve_reference_with_dependency_resolver(
                        ReferenceSource {
                            file_index: source_file_index,
                            metadata: source_file.metadata,
                            graph: source_file.graph,
                        },
                        reference.file_id,
                        reference.path_id,
                        &mut dependency_targets,
                        resolver,
                    )?
                } else {
                    result.resolve_reference(
                        source_file_index,
                        source_file.metadata,
                        source_file.graph,
                        reference.file_id,
                        reference.path_id,
                    )
                };
                result.references.push(MultiFileObjectReference {
                    source_file_index,
                    source_object_index: reference.source_object_index,
                    source_path_id: reference.source_path_id,
                    field_path: &reference.field_path,
                    file_id: reference.file_id,
                    path_id: reference.path_id,
                    resolution,
                });
            }
        }
        Ok(result)
    }

    pub fn file_count(&self) -> usize {
        self.normalized_file_names.len()
    }

    /// Returns the number of object-table records for one loaded file.
    pub fn file_object_count(&self, file_index: usize) -> Option<usize> {
        Some(self.file_metadata.get(file_index)?.objects.len())
    }

    pub fn normalized_file_names(&self) -> &[String] {
        &self.normalized_file_names
    }

    /// Return PPtrs in source-file, source-object, then field traversal order.
    ///
    /// The source-file/object prefix is monotonic, so callers can use a partition point to find
    /// one object's contiguous reference slice without scanning unrelated object edges.
    pub fn references(&self) -> &[MultiFileObjectReference<'a>] {
        &self.references
    }

    /// Returns the scoped managed-reference object/edge graph for one loaded file.
    ///
    /// Managed IDs are local to the nearest struct that owns a Registry field. Scope IDs are
    /// stable preorder indexes within the owning PathID, and edge targets index this object's
    /// managed-object slice. They are separate from the cross-file PPtr namespace.
    pub fn managed_graph_for_file(&self, file_index: usize) -> Option<&'a ObjectReferenceGraph> {
        self.file_graphs.get(file_index).copied()
    }

    /// Returns the parsed identity for a file-scoped object-table entry.
    pub fn object_identity(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> Option<MultiFileObjectIdentity<'_>> {
        let path_id = self
            .file_metadata
            .get(file_index)?
            .objects
            .get(object_index)?
            .path_id;
        Some(MultiFileObjectIdentity {
            file_index,
            normalized_file_name: self.normalized_file_names.get(file_index)?,
            object_index,
            path_id,
        })
    }

    /// Returns the parsed class ID for a file-scoped object-table entry when it is known.
    pub fn object_class_id(&self, file_index: usize, object_index: usize) -> Option<i32> {
        let object = self
            .file_metadata
            .get(file_index)?
            .objects
            .get(object_index)?;
        object
            .class_id
            .map(i32::from)
            .or_else(|| object.resolved_type_index.map(|_| object.raw_type_id))
    }

    /// Returns the resolved serialized TypeTree used to scan this object, when one is present.
    pub fn object_type_tree(&self, file_index: usize, object_index: usize) -> Option<&TypeTree> {
        let metadata = self.file_metadata.get(file_index)?;
        let object = metadata.objects.get(object_index)?;
        let type_index = object.resolved_type_index?;
        metadata.types.get(type_index)?.type_tree.as_ref()
    }

    /// Returns whether a loaded object's TypeTree reference scan was complete.
    pub fn object_scan_is_complete(&self, file_index: usize, object_index: usize) -> Option<bool> {
        Some(
            self.file_graphs
                .get(file_index)?
                .object_scan_is_complete(object_index),
        )
    }

    /// Returns references emitted by one file-scoped source object.
    pub fn references_from_object(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> impl Iterator<Item = &MultiFileObjectReference<'a>> {
        let key = (file_index, object_index);
        let first = self.references.partition_point(|reference| {
            (reference.source_file_index, reference.source_object_index) < key
        });
        let end = self.references.partition_point(|reference| {
            (reference.source_file_index, reference.source_object_index) <= key
        });
        self.references[first..end].iter()
    }

    /// Returns incoming uniquely resolved references to one file-scoped object.
    ///
    /// This scans the bounded edge list; ambiguous or missing references are not reported as
    /// incoming object edges.
    pub fn references_to_object(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> impl Iterator<Item = &MultiFileObjectReference<'a>> {
        self.references.iter().filter(move |reference| {
            matches!(
                &reference.resolution,
                MultiFilePPtrResolution::Object {
                    target_file_index,
                    target_object_index,
                } if *target_file_index == file_index && *target_object_index == object_index
            )
        })
    }

    /// Return the source external-table entry associated with one unresolved dependency result.
    pub fn dependency_identifier(
        &self,
        source_file_index: usize,
        external_index: usize,
    ) -> Option<&'a crate::serialized_file::metadata::FileIdentifier> {
        self.file_metadata
            .get(source_file_index)?
            .externals
            .get(external_index)
    }

    /// Return the distinct source external-table indexes with no loaded dependency
    /// file, in ascending identifier order.
    ///
    /// This mirrors `SerializedAssetCollection.InitializeDependencyList` reporting
    /// each unresolved identifier once: several PPtrs can share one external slot,
    /// but the missing dependency is reported per identifier, not per reference.
    /// Pair with `dependency_identifier` to fetch the reported entries.
    pub fn missing_dependency_indexes(&self, source_file_index: usize) -> Vec<usize> {
        let mut indexes: Vec<usize> = self
            .references
            .iter()
            .filter(|reference| reference.source_file_index == source_file_index)
            .filter_map(|reference| match reference.resolution {
                MultiFilePPtrResolution::MissingDependency { external_index, .. } => {
                    Some(external_index)
                }
                _ => None,
            })
            .collect();
        indexes.sort_unstable();
        indexes.dedup();
        indexes
    }

    /// Return every supplied file matching the source-normalized dependency name.
    pub fn candidate_file_indices<'b>(
        &'b self,
        file_name: &str,
    ) -> impl Iterator<Item = usize> + 'b {
        let normalized_name = crate::io::special_names::fix_file_identifier(file_name);
        let first = self.name_index.partition_point(|file_index| {
            self.normalized_file_names[*file_index].as_str() < normalized_name.as_str()
        });
        let end = self.name_index.partition_point(|file_index| {
            self.normalized_file_names[*file_index].as_str() <= normalized_name.as_str()
        });
        self.name_index[first..end].iter().copied()
    }

    /// True only when all input scans are complete and every non-null PPtr resolves uniquely.
    pub fn is_complete(&self) -> bool {
        self.input_graphs_complete
            && self.references.iter().all(|reference| {
                matches!(
                    reference.resolution,
                    MultiFilePPtrResolution::Null | MultiFilePPtrResolution::Object { .. }
                )
            })
    }

    /// Whether all known references resolve and every graph issue belongs to an explicitly
    /// opaque object payload. Opaque object identities remain in the PathID index, so references
    /// to those objects are still classified; their own unparsed payloads are not claimed to be
    /// reference-free.
    pub fn is_complete_except_opaque_object_payloads(
        &self,
        mut is_opaque: impl FnMut(usize, usize) -> bool,
    ) -> bool {
        self.file_graphs
            .iter()
            .enumerate()
            .all(|(file_index, graph)| {
                graph.is_complete_except_opaque_object_payloads(|object_index| {
                    is_opaque(file_index, object_index)
                })
            })
            && self.references.iter().all(|reference| {
                matches!(
                    reference.resolution,
                    MultiFilePPtrResolution::Null | MultiFilePPtrResolution::Object { .. }
                )
            })
    }

    fn resolve_reference_with_dependency_resolver(
        &self,
        source: ReferenceSource<'_>,
        file_id: i32,
        path_id: i64,
        dependency_targets: &mut HashMap<usize, Option<usize>>,
        dependency_resolver: &mut dyn FnMut(usize, &FileIdentifier) -> Option<usize>,
    ) -> Result<MultiFilePPtrResolution, MultiFileReferenceGraphBuildError> {
        if path_id == 0 || file_id == 0 {
            return Ok(self.resolve_reference(
                source.file_index,
                source.metadata,
                source.graph,
                file_id,
                path_id,
            ));
        }
        let Some(external_index) = external_table_index(file_id, source.metadata.externals.len())
        else {
            return Ok(MultiFilePPtrResolution::InvalidFileId {
                file_id,
                external_count: source.metadata.externals.len(),
            });
        };
        let identifier = &source.metadata.externals[external_index];
        let target_file_index =
            if let Some(target_file_index) = dependency_targets.get(&external_index) {
                *target_file_index
            } else {
                let target_file_index = dependency_resolver(source.file_index, identifier);
                dependency_targets.try_reserve(1).map_err(|_| {
                    MultiFileReferenceGraphBuildError::AllocationFailed {
                        resource: "per-file external collection resolution cache",
                        requested: 1,
                    }
                })?;
                dependency_targets.insert(external_index, target_file_index);
                target_file_index
            };
        let Some(target_file_index) = target_file_index else {
            return Ok(MultiFilePPtrResolution::MissingDependency {
                external_index,
                path_id,
            });
        };
        let Some(target_graph) = self.file_graphs.get(target_file_index) else {
            return Err(
                MultiFileReferenceGraphBuildError::DependencyTargetFileIndexOutOfBounds {
                    source_file_index: source.file_index,
                    target_file_index,
                    file_count: self.file_graphs.len(),
                },
            );
        };
        Ok(match local_path_id_resolution(target_graph, path_id) {
            LocalPathIdResolution::Unique(target_object_index) => MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index,
            },
            LocalPathIdResolution::Missing => MultiFilePPtrResolution::MissingExternalObject {
                target_file_index,
                path_id,
            },
            LocalPathIdResolution::Ambiguous(match_count) => {
                MultiFilePPtrResolution::AmbiguousExternalObject {
                    target_file_index,
                    path_id,
                    match_count,
                }
            }
        })
    }

    fn name_range(&self, normalized_name: &str) -> (usize, usize) {
        let first = self.name_index.partition_point(|file_index| {
            self.normalized_file_names[*file_index].as_str() < normalized_name
        });
        let end = self.name_index.partition_point(|file_index| {
            self.normalized_file_names[*file_index].as_str() <= normalized_name
        });
        (first, end)
    }

    fn resolve_external_target(
        &self,
        target_file_index: usize,
        path_id: i64,
    ) -> MultiFilePPtrResolution {
        match local_path_id_resolution(self.file_graphs[target_file_index], path_id) {
            LocalPathIdResolution::Unique(object_index) => MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index: object_index,
            },
            LocalPathIdResolution::Missing => MultiFilePPtrResolution::MissingExternalObject {
                target_file_index,
                path_id,
            },
            LocalPathIdResolution::Ambiguous(match_count) => {
                MultiFilePPtrResolution::AmbiguousExternalObject {
                    target_file_index,
                    path_id,
                    match_count,
                }
            }
        }
    }

    fn resolve_reference(
        &self,
        source_file_index: usize,
        source_metadata: &SerializedFileMetadata,
        source_graph: &ObjectReferenceGraph,
        file_id: i32,
        path_id: i64,
    ) -> MultiFilePPtrResolution {
        if path_id == 0 {
            return MultiFilePPtrResolution::Null;
        }
        if file_id == 0 {
            return match local_path_id_resolution(source_graph, path_id) {
                LocalPathIdResolution::Unique(object_index) => MultiFilePPtrResolution::Object {
                    target_file_index: source_file_index,
                    target_object_index: object_index,
                },
                LocalPathIdResolution::Missing => {
                    MultiFilePPtrResolution::MissingLocalObject { path_id }
                }
                LocalPathIdResolution::Ambiguous(match_count) => {
                    MultiFilePPtrResolution::AmbiguousLocalObject {
                        path_id,
                        match_count,
                    }
                }
            };
        }

        let Some(external_index) = external_table_index(file_id, source_metadata.externals.len())
        else {
            return MultiFilePPtrResolution::InvalidFileId {
                file_id,
                external_count: source_metadata.externals.len(),
            };
        };
        let identifier = &source_metadata.externals[external_index];
        let dependency_name = identifier.fixed_path_name();
        let (first, end) = self.name_range(&dependency_name);
        match end - first {
            0 => {
                let Some(alias) = engine_resource_alias(&dependency_name) else {
                    return MultiFilePPtrResolution::MissingDependency {
                        external_index,
                        path_id,
                    };
                };
                let (alias_first, alias_end) = self.name_range(alias);
                match alias_end - alias_first {
                    0 => MultiFilePPtrResolution::MissingDependency {
                        external_index,
                        path_id,
                    },
                    1 => self.resolve_external_target(self.name_index[alias_first], path_id),
                    candidate_count => MultiFilePPtrResolution::AmbiguousDependency {
                        external_index,
                        path_id,
                        candidate_count,
                    },
                }
            }
            1 => self.resolve_external_target(self.name_index[first], path_id),
            candidate_count => MultiFilePPtrResolution::AmbiguousDependency {
                external_index,
                path_id,
                candidate_count,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultiFileReferenceGraphBuildError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    FileCountLimitExceeded {
        file_count: usize,
        limit: usize,
    },
    FileNameLimitExceeded {
        file_index: usize,
        byte_count: usize,
        limit: usize,
    },
    TotalFileNameBytesOverflow,
    TotalFileNameBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    ReferenceCountOverflow,
    ReferenceCountLimitExceeded {
        reference_count: usize,
        limit: usize,
    },
    DependencyTargetFileIndexOutOfBounds {
        source_file_index: usize,
        target_file_index: usize,
        file_count: usize,
    },
    SourceCollectionTopologyInvalid {
        message: String,
    },
}

impl Display for MultiFileReferenceGraphBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::FileCountLimitExceeded { file_count, limit } => {
                write!(f, "file count {file_count} exceeds limit {limit}")
            }
            Self::FileNameLimitExceeded {
                file_index,
                byte_count,
                limit,
            } => write!(
                f,
                "file name at index {file_index} has {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::TotalFileNameBytesOverflow => {
                f.write_str("total normalized file-name bytes overflow")
            }
            Self::TotalFileNameBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "total normalized file-name bytes {byte_count} exceed limit {limit}"
            ),
            Self::ReferenceCountOverflow => f.write_str("cross-file reference count overflows"),
            Self::ReferenceCountLimitExceeded {
                reference_count,
                limit,
            } => write!(f, "reference count {reference_count} exceeds limit {limit}"),
            Self::DependencyTargetFileIndexOutOfBounds {
                source_file_index,
                target_file_index,
                file_count,
            } => write!(
                f,
                "dependency resolver selected file {target_file_index} for source file {source_file_index}, outside the {file_count} loaded files"
            ),
            Self::SourceCollectionTopologyInvalid { message } => {
                write!(f, "source collection topology is invalid: {message}")
            }
        }
    }
}

impl Error for MultiFileReferenceGraphBuildError {}

fn local_path_id_resolution(graph: &ObjectReferenceGraph, path_id: i64) -> LocalPathIdResolution {
    let first = graph
        .path_id_index
        .partition_point(|(candidate, _)| *candidate < path_id);
    let end = graph
        .path_id_index
        .partition_point(|(candidate, _)| *candidate <= path_id);
    match end - first {
        0 => LocalPathIdResolution::Missing,
        1 => LocalPathIdResolution::Unique(graph.path_id_index[first].1),
        match_count => LocalPathIdResolution::Ambiguous(match_count),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalPathIdResolution {
    Unique(usize),
    Missing,
    Ambiguous(usize),
}
